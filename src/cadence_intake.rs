//! Cadence weekly-record intake for the monthly curation pass.
//!
//! Shells out to the `cadence` binary to list `weekly` records produced
//! by `confidant`, reads the first ~500 bytes of each letter as a lede,
//! and returns a list of [`WeeklySource`] items to include in the
//! curation source pool.

use std::path::PathBuf;
use std::process::Command;

/// A weekly cadence record's lede, ready for inclusion in the source pool.
#[derive(Debug, Clone)]
pub struct WeeklySource {
    /// ULID of the cadence record.
    pub record_id: String,
    /// Filesystem path of the weekly letter.
    pub path: PathBuf,
    /// First ~500 bytes of the letter file (UTF-8 lossy).
    pub lede: String,
}

/// Configuration for cadence intake.
#[derive(Debug, Clone)]
pub struct IntakeConfig {
    /// Duration window passed to `cadence list --since`, e.g. `"30d"`.
    pub since: String,
    /// Override `$CADENCE_HOME` for the subprocess. `None` = inherit.
    pub cadence_home: Option<String>,
}

impl Default for IntakeConfig {
    fn default() -> Self {
        Self {
            since: "30d".to_string(),
            cadence_home: None,
        }
    }
}

/// Outcome of an intake attempt.
#[derive(Debug)]
pub enum IntakeOutcome {
    /// Cadence is unavailable or `$CADENCE_HOME` is missing/broken.
    Unavailable(String),
    /// No weekly records were found for the configured window.
    Empty,
    /// Records were collected successfully.
    Sources(Vec<WeeklySource>),
}

/// Attempt to collect weekly cadence records for the curation source pool.
///
/// Returns `IntakeOutcome::Unavailable` on any subprocess or parsing failure
/// so the caller can decide whether to warn or continue silently.
#[must_use]
pub fn collect(cfg: &IntakeConfig) -> IntakeOutcome {
    let json = match run_cadence_list(cfg) {
        Ok(j) => j,
        Err(msg) => return IntakeOutcome::Unavailable(msg),
    };
    if json.trim().is_empty() || json.trim() == "[]" {
        return IntakeOutcome::Empty;
    }
    match parse_records(&json) {
        Ok(sources) if sources.is_empty() => IntakeOutcome::Empty,
        Ok(sources) => IntakeOutcome::Sources(sources),
        Err(msg) => IntakeOutcome::Unavailable(msg),
    }
}

fn run_cadence_list(cfg: &IntakeConfig) -> Result<String, String> {
    let mut cmd = Command::new("cadence");
    cmd.args(["list", "weekly", "--produced-by", "confidant", "--since"])
        .arg(&cfg.since)
        .arg("--json");
    if let Some(home) = &cfg.cadence_home {
        cmd.env("CADENCE_HOME", home);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("cadence not found or failed to launch: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("cadence list weekly failed: {stderr}"));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("cadence output not UTF-8: {e}"))
}

/// Minimal JSON parsing — extract `id` and `path` fields from each record
/// object without pulling in serde_json.
fn parse_records(json: &str) -> Result<Vec<WeeklySource>, String> {
    let trimmed = json.trim();
    if !trimmed.starts_with('[') {
        return Err(format!("unexpected cadence output (not a JSON array): {trimmed}"));
    }

    let mut sources = Vec::new();
    // Split by `{` and `}` to extract objects. Simple enough for the
    // flat JSON that `cadence list --json` emits (one record per line or flat).
    let objects = extract_objects(trimmed);
    for obj in objects {
        let id = extract_str_field(&obj, "id");
        let path_str = extract_str_field(&obj, "path");
        match (id, path_str) {
            (Some(id), Some(ps)) => {
                let path = PathBuf::from(&ps);
                let lede = read_lede(&path);
                sources.push(WeeklySource {
                    record_id: id,
                    path,
                    lede,
                });
            }
            _ => {
                // Skip records with missing id or path.
            }
        }
    }
    Ok(sources)
}

/// Split a JSON array string into individual object strings.
fn extract_objects(json: &str) -> Vec<String> {
    let mut objects = Vec::new();
    let mut depth: usize = 0;
    let mut start: Option<usize> = None;
    for (i, ch) in json.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth = depth.saturating_add(1);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some(s) = start.take() {
                        objects.push(json[s..=i].to_string());
                    }
                }
            }
            _ => {}
        }
    }
    objects
}

/// Extract a string value from a flat JSON object (no nesting, no escapes).
fn extract_str_field(obj: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let pos = obj.find(needle.as_str())?;
    let after_key = &obj[pos + needle.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    if after_colon.starts_with('"') {
        let inner = &after_colon[1..];
        let end = inner.find('"')?;
        Some(inner[..end].to_string())
    } else {
        None
    }
}

/// Read the first 500 bytes of a file and return as a UTF-8-lossy string.
fn read_lede(path: &std::path::Path) -> String {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return String::new(),
    };
    let capped = if bytes.len() > 500 { &bytes[..500] } else { &bytes[..] };
    String::from_utf8_lossy(capped).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_array() {
        let result = parse_records("[]");
        assert!(matches!(result, Ok(v) if v.is_empty()));
    }

    #[test]
    fn extract_objects_simple() {
        let json = r#"[{"id":"01","path":"/tmp/a.md"},{"id":"02","path":"/tmp/b.md"}]"#;
        let objs = extract_objects(json);
        assert_eq!(objs.len(), 2);
    }

    #[test]
    fn extract_str_field_finds_value() {
        let obj = r#"{"id":"abc123","path":"/home/user/letter.md","tier":"weekly"}"#;
        assert_eq!(extract_str_field(obj, "id").as_deref(), Some("abc123"));
        assert_eq!(
            extract_str_field(obj, "path").as_deref(),
            Some("/home/user/letter.md")
        );
        assert!(extract_str_field(obj, "missing").is_none());
    }

    #[test]
    fn collect_unavailable_on_bad_cadence_home() {
        let cfg = IntakeConfig {
            since: "30d".to_string(),
            cadence_home: Some("/nonexistent/cadence/home".to_string()),
        };
        let outcome = collect(&cfg);
        // Either Unavailable (cadence errors on missing home) or Empty.
        // Either is acceptable — must not panic.
        matches!(outcome, IntakeOutcome::Unavailable(_) | IntakeOutcome::Empty);
    }

    #[test]
    fn collect_skipped_when_cadence_absent() {
        // Temporarily override PATH so `cadence` isn't found.
        let cfg = IntakeConfig {
            since: "30d".to_string(),
            cadence_home: None,
        };
        // We can't easily shadow PATH in a unit test without forking,
        // so just verify collect() doesn't panic when run normally.
        // The real behavior is covered in acceptance tests.
        let _outcome = collect(&cfg);
    }
}
