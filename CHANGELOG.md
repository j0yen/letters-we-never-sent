# Changelog

## v0.2.0 — 2026-05-29

Added cadence weekly-record intake and monthly record registration to `letter-curate`.

New modules:
- `cadence_intake`: shells out to `cadence list weekly --produced-by confidant` to pull weekly letter ledes into the curation source pool.
- `cadence_record`: shells out to `cadence record monthly --produced-by letter-curate` to register each monthly aggregate as a cadence record.

New `letter curate` subcommand with flags:
- `--cadence-intake` (default: true) — pull weekly records into source pool
- `--cadence-since <duration>` (default: `30d`) — intake window
- `--no-cadence-record` — suppress cadence record after curation
- `--print-sources` — print all candidate sources before output

Also added `[[bin]] name = "letter"` alias so acceptance tests resolve `CARGO_BIN_EXE_letter` correctly.
