//! Clap CLI surface for the `letter` binary.

use crate::cadence_intake::{self, IntakeConfig, IntakeOutcome};
use crate::cadence_record::{self, RecordParams};
use crate::error::{LetterError, LetterResult};
use crate::letter;
use crate::listing::{self, Stats};
use crate::state::{State, StateFilter};
use chrono::Datelike;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

/// Top-level CLI.
#[derive(Debug, Parser)]
#[command(name = "letter", about = "curate letters-we-never-sent drafts")]
pub struct Cli {
    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List letters under the root.
    List(ListArgs),
    /// Mark a letter as accepted.
    Accept(MutateArgs),
    /// Mark a letter as declined.
    Decline(MutateArgs),
    /// Mark a letter to send for real.
    MarkSendReal(MutateArgs),
    /// Aggregate counts.
    Stats(StatsArgs),
    /// Open a letter in $EDITOR.
    Open(MutateArgs),
    /// Curate a month's letters with optional cadence intake.
    Curate(CurateArgs),
}

/// Arguments to `list`.
#[derive(Debug, Parser)]
pub struct ListArgs {
    /// Root letters directory.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Filter to a single year (matches `month: YYYY-MM` frontmatter).
    #[arg(long)]
    pub year: Option<i32>,
    /// State filter.
    #[arg(long, value_enum, default_value_t = StateArg::Pending)]
    pub state: StateArg,
}

/// Arguments to `accept` / `decline` / `mark-send-real` / `open`.
#[derive(Debug, Parser)]
pub struct MutateArgs {
    /// Letter file (relative to root or absolute path).
    pub filename: String,
    /// Root letters directory.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Arguments to `stats`.
#[derive(Debug, Parser)]
pub struct StatsArgs {
    /// Root letters directory.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Year filter; defaults to the current year.
    #[arg(long)]
    pub year: Option<i32>,
}

/// Arguments to `curate`.
#[derive(Debug, Parser)]
pub struct CurateArgs {
    /// Root letters directory.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Year to curate (defaults to current year).
    #[arg(long)]
    pub year: Option<i32>,
    /// Output path for the monthly aggregate Markdown.
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// Pull weekly cadence records from confidant as additional intake.
    /// Defaults to true when cadence substrate is present.
    #[arg(long, default_value_t = true)]
    pub cadence_intake: bool,
    /// Duration window for cadence intake (e.g. `30d`).
    #[arg(long, default_value = "30d")]
    pub cadence_since: String,
    /// Suppress writing the monthly cadence record after curation.
    #[arg(long)]
    pub no_cadence_record: bool,
    /// Print all candidate source paths before curation output.
    #[arg(long)]
    pub print_sources: bool,
}

/// CLI value for `--state`.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum StateArg {
    /// All letters.
    All,
    /// Pending letters.
    Pending,
    /// Accepted letters.
    Accepted,
    /// Declined letters.
    Declined,
    /// Marked send-for-real letters.
    SendReal,
}

impl StateArg {
    /// Convert into the runtime `StateFilter`.
    #[must_use]
    pub const fn filter(self) -> StateFilter {
        match self {
            Self::All => StateFilter::All,
            Self::Pending => StateFilter::Only(State::Pending),
            Self::Accepted => StateFilter::Only(State::Accepted),
            Self::Declined => StateFilter::Only(State::Declined),
            Self::SendReal => StateFilter::Only(State::SendReal),
        }
    }
}

/// Resolve `--root` against the user's home if not provided.
#[must_use]
pub fn default_root() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".claude").join("letters-we-never-sent");
    }
    PathBuf::from(".")
}

fn root_or_default(opt: Option<PathBuf>) -> PathBuf {
    opt.unwrap_or_else(default_root)
}

/// Run the `list` subcommand. Returns rows formatted for stdout.
///
/// # Errors
/// Propagates I/O failures from directory traversal.
pub fn run_list(args: ListArgs) -> LetterResult<String> {
    let root = root_or_default(args.root);
    let entries = listing::collect(&root, args.year)?;
    let filtered = listing::filter(entries, args.state.filter());
    let mut out = String::new();
    for e in filtered {
        out.push_str(&format!(
            "{} {} {}  {}\n",
            e.filename,
            e.state_str(),
            e.accepted_at_col(),
            e.subject_col()
        ));
    }
    Ok(out)
}

/// Run the `stats` subcommand.
///
/// # Errors
/// Propagates I/O failures.
pub fn run_stats(args: StatsArgs) -> LetterResult<String> {
    let root = root_or_default(args.root);
    let year = args.year.unwrap_or_else(|| chrono::Utc::now().date_naive().year());
    let entries = listing::collect(&root, Some(year))?;
    let s = Stats::from_entries(&entries);
    Ok(format_stats(s))
}

fn format_stats(s: Stats) -> String {
    format!(
        "accepted: {}\ndeclined: {}\npending: {}\nsend-real: {}\ntotal: {}\n",
        s.accepted, s.declined, s.pending, s.send_real, s.total
    )
}

/// Run a state-mutation subcommand (accept / decline / mark-send-real).
///
/// # Errors
/// Returns parse / I/O / not-found errors from the letter module.
pub fn run_mutate(args: MutateArgs, new_state: State) -> LetterResult<()> {
    let root = root_or_default(args.root);
    let path = letter::resolve(&args.filename, &root);
    if !path.exists() {
        return Err(LetterError::NotFound(path));
    }
    letter::transition(&path, new_state)
}

/// Run the `curate` subcommand.
///
/// Collects letter sources from `--root`, optionally augments with
/// weekly cadence records (AC1/AC4/AC5), emits a monthly aggregate,
/// and optionally registers a cadence record (AC2).
///
/// Returns formatted output for stdout.
///
/// # Errors
/// Propagates I/O failures.
pub fn run_curate(args: CurateArgs) -> LetterResult<String> {
    let root = root_or_default(args.root);
    let year = args.year.unwrap_or_else(|| chrono::Utc::now().date_naive().year());

    // Collect local letter sources.
    let entries = listing::collect(&root, Some(year))?;
    let mut sources: Vec<String> = entries
        .iter()
        .map(|e| e.path.display().to_string())
        .collect();

    // Collect cadence weekly intake.
    let mut weekly_ids: Vec<String> = Vec::new();
    if args.cadence_intake {
        let cfg = IntakeConfig {
            since: args.cadence_since.clone(),
            cadence_home: None,
        };
        match cadence_intake::collect(&cfg) {
            IntakeOutcome::Sources(ws) => {
                for w in &ws {
                    sources.push(w.path.display().to_string());
                    weekly_ids.push(w.record_id.clone());
                }
            }
            IntakeOutcome::Empty => {}
            IntakeOutcome::Unavailable(msg) => {
                eprintln!("warning: cadence intake unavailable: {msg}");
            }
        }
    }

    // Build the output path.
    let out_path = args.output.unwrap_or_else(|| {
        root.join(format!("{year}-aggregate.md"))
    });

    // Emit the aggregate.
    let mut out = String::new();
    if args.print_sources {
        out.push_str("# sources\n");
        for s in &sources {
            out.push_str(&format!("- {s}\n"));
        }
        out.push('\n');
    }
    let stats = listing::Stats::from_entries(&entries);
    let summary = format!("{year}: {} letters", stats.total);
    out.push_str(&format!("# Monthly aggregate — {year}\n\n"));
    out.push_str(&format!(
        "accepted: {}  declined: {}  pending: {}  send-real: {}\n",
        stats.accepted, stats.declined, stats.pending, stats.send_real
    ));
    out.push_str(&format!("total: {}\n", stats.total));

    // Write aggregate to disk.
    std::fs::write(&out_path, &out).map_err(|e| LetterError::Io {
        path: out_path.clone(),
        source: e,
    })?;

    // Register cadence record unless suppressed.
    if !args.no_cadence_record {
        let params = RecordParams {
            path: &out_path,
            summary,
            source_ids: weekly_ids,
            cadence_home: None,
        };
        match cadence_record::register(&params) {
            cadence_record::RecordOutcome::Created { id } => {
                out.push_str(&format!("\ncadence record: {id}\n"));
            }
            cadence_record::RecordOutcome::Failed(msg) => {
                eprintln!("warning: cadence record not created: {msg}");
            }
        }
    }

    Ok(out)
}

/// Run `open` — invoke $EDITOR on the file. Falls back to `vi`.
///
/// # Errors
/// Returns `LetterError::NotFound` if the file is missing, or
/// `LetterError::Io` if the editor cannot be launched. Non-zero editor exit
/// codes are surfaced through the returned exit code.
pub fn run_open(args: MutateArgs) -> LetterResult<i32> {
    let root = root_or_default(args.root);
    let path = letter::resolve(&args.filename, &root);
    if !path.exists() {
        return Err(LetterError::NotFound(path));
    }
    let editor = std::env::var_os("EDITOR").unwrap_or_else(|| std::ffi::OsString::from("vi"));
    spawn_editor(&editor, &path)
}

fn spawn_editor(editor: &std::ffi::OsStr, path: &Path) -> LetterResult<i32> {
    let status = std::process::Command::new(editor)
        .arg(path)
        .status()
        .map_err(|e| LetterError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
    Ok(status.code().unwrap_or(1))
}
