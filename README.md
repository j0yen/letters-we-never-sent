# letters-we-never-sent

A small CLI for triaging the letters you draft but don't send — list them, accept or decline each one, and roll a year's worth into a single Markdown file for the annual binding.

## Why it exists

The monthly draft ritual leaves a directory full of letters: two to four Markdown files a month, addressed to people they'll never reach. Left alone, they just accumulate, and the year-end binding has no way to know which ones mattered. This tool is the triage step. It reads the drafts, lets you mark each one accepted, declined, or `send-real`, and records the decision in the file's own frontmatter — so the next pass, and the annual aggregate, have a signal to work from. It edits frontmatter only; the body of a letter is preserved byte for byte.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/j0yen/letters-we-never-sent/main/install.sh | bash
```

Or from a checkout:

```sh
git clone --depth 1 https://github.com/j0yen/letters-we-never-sent.git
cd letters-we-never-sent
./install.sh
```

`install.sh` runs `cargo install --path . --locked`, which puts the `letter` binary (and an alias, `letter-curate`) in `~/.cargo/bin/`. Needs `cargo` / `rustc` ≥ 1.85 and `git`.

## A letter

Each letter is a Markdown file with YAML frontmatter:

```markdown
---
recipient: the PM
month: 2026-05
state: pending
---

Body text, preserved exactly as written.
```

`state` is one of `pending`, `accepted`, `declined`, `send-real` (default `pending`); `accepted_at` is stamped in RFC 3339 when the state changes. Files with no frontmatter or malformed YAML aren't a crash — `list` shows them as `(unparseable)` and state changes refuse them with a clear error.

## Quickstart

The default root is `~/.claude/letters-we-never-sent/`; override it anywhere with `--root`.

```sh
letter list                         # pending letters: filename, state, accepted-at, subject
letter list --year 2026 --state all
letter accept 2026-05-03.md         # set state=accepted, stamp accepted_at
letter decline 2026-05-04.md
letter mark-send-real 2026-05-01.md # the rare one you actually mean to send
letter open 2026-05-03.md           # open in $EDITOR (falls back to vi)
letter stats --year 2026            # counts by state
```

Filenames are positional — relative to the root or absolute. Globs are not expanded; let the shell do that. The state transitions are idempotent: running `accept` twice leaves the same result.

## Curate — the annual roll-up

```sh
letter curate --year 2026
```

`curate` gathers the year's letters, writes a monthly aggregate (`<year>-aggregate.md` by default, or `--output <path>`), and prints the per-state counts. It can also pull weekly cadence records from [`confidant`](https://github.com/j0yen/confidant) as additional intake — on by default when the cadence substrate is present, windowed by `--cadence-since 30d`, suppressible with `--no-cadence-record`. If confidant isn't installed, curate notes it on stderr and proceeds with the local letters alone. Use `--print-sources` to see every file it considered.

## Where it fits

`letter` curates; [`confidant`](https://github.com/j0yen/confidant) supplies the weekly cadence records that `curate` can fold in. Both are part of the wintermute personal-tooling fleet, built through the [`autobuilder`](https://github.com/j0yen/autobuilder) pipeline.

## License

MIT OR Apache-2.0 — see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
