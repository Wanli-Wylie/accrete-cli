# Changelog

## 0.1.0 — 2026-11-03

Initial functional native release: five object families, Model iterations,
per-host configuration, atomic document publication, SQLite projection,
Data locations/replicas, Git retention, Skill exposures, metadata exchange,
bounded recovery, legacy migration, native release archives and release CI
configuration.

The Kernel works inside an existing Workspace, found from `--project`, `ACCRETE_PROJECT`
or the nearest `.accrete/project.toml`. It has no `init` or template logic: `accrete-kit`, built from `accrete-template`, creates
Workspaces and upgrades their template files. The marker needs only `format = 1` and a safe `project_id`; another format or an
unsafe ID is refused. Store directories and host-local state appear on first use. An
exposure stays out of shared Git through the repository-local `info/exclude`.

`accrete-kit` installs the Kernel as `.accrete/bin/accrete` in each Workspace. A version tag
builds all targets and attaches their archives and one `SHA256SUMS` to a GitHub Release. A
rerun uploads only missing assets and fails rather than replace different bytes. Nothing is
published to npm from this repository.

The index uses SQLite's rollback journal instead of WAL, so `sqlite3 -readonly` can open
it after a Kernel write. It is one closed file between invocations. A reader can briefly
get "database is locked" while a refresh commits, and should retry.

Commands aligned with the template's catalog: `migrate` without `--from`,
`tool refresh --agent`, `--from` on retire and `model iteration open`, the `--from` and
repeatable `--commit` form of `model iteration close`, `repository.checkout`, `data evict
--location` by ID or name, safe project IDs, and the `activity` fallback for the index name.

This version establishes document/project/index format 1 and the initial
agent-facing CLI contract. See `VALIDATION.md` for completed checks and
`docs/implementation.md` for finite first-release boundaries.
