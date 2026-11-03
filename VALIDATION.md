# Completed validation — ACCRETE CLI 0.1.0

Validation performed on **3 November 2026**, on Linux x86-64 with Rust **1.90.0**,
Node **24.22.1**, npm **11.9.0**, and Git **2.51.1**. The checks use temporary
projects, ordinary Git repositories and small synthetic files. No historical
scientific computation was launched.

The sections down to "Boundaries of these results" describe that earlier Linux delivery,
whose Kernel still held project initialization, and its artifacts. The current boundary is
covered under [Workspace boundary](#workspace-boundary). Those artifacts are no longer in
the working tree.

## Native implementation

`cargo test --locked` completed with **23 passing integration tests**, zero
failures. `cargo fmt --all --check` passed on the delivered source.

| Suite | Tests | Behavior exercised |
|---|---:|---|
| CLI workflows | 5 | Real Model worktree creation, closure and Git dirty-removal refusal; Model retry; Tool wrappers/symlinks, export, edited-wrapper preservation and retirement; retained Tool checkout matching; Result observations, repeat operation IDs, stale/valid revision amendments and native SQL associations; input validation |
| Data and migration | 5 | Distinct Data IDs with equal bytes; stable-ID retries; cheap directory registration and real copies; managed eviction and borrowed-source preservation; failed digest and recovery without retransfer; link behavior; metadata conversion preserving large numbers, IDs, prose, date paths, reservations, original payloads and independent edits |
| Persistence | 8 | Parallel complete Memory allocation; byte revision conflicts; extension/large-number preservation; committed publication with unavailable index; complete old index retained after malformed/duplicate/future documents; import conflicts and explicit predecessor replacement; collision renumbering and retry; literal standard Memory frontmatter |
| Project/template integration | 5 | Initial asset installation, repeat and upgrade; preservation of owner edits and unrelated files; conflict preflight; incompatible release refusal; continuation after publication before inventory; global initialization state/host overrides and marker preservation |

The tests are present in `tests/`, so a source checkout can repeat these same
checks with Cargo. Empty unit/doc-test groups are not counted as additional
coverage.

## Actual transfer adapter

The rclone adapter was exercised with the official **rclone v1.70.3 Linux x64**
binary and an alias remote backed by a local filesystem. A file with a known
SHA-256 passed through `copyto --immutable`; a directory passed through
`copy --immutable`. Paths contained spaces, resulting bytes matched, and the
ACCRETE response remained valid JSON on stdout. No cloud credentials, network
storage backend or historical provider-version selector was exercised.

rclone is an optional external dependency and is not bundled in this repository
or release. Its tested executable was used only during implementation checks.

## Final delivered npm/native artifacts

The final binary was built with:

```sh
cargo build --release --locked --target x86_64-unknown-linux-musl
```

A local musl C compiler was supplied for bundled SQLite compilation. Rust's
normal linker was retained. Inspection reports a **static PIE** with no dynamic
interpreter. It runs directly as `accrete 0.1.0`.

Binary SHA-256:

```text
e49f7277a3e289edcab255014d0ad2dcd2998beb6311931e08a8a6eec816683b
```

The final release directory `dist/linux-x64-0.1.0/` includes the native archive,
three npm tarballs, release manifest and SHA-256 checksums. The same executable
bytes are present in the native archive and the npm payload.

A fresh installation of those final tarballs with `npm --offline
--ignore-scripts` succeeded. Through that installation:

1. `accrete --version` returned `accrete 0.1.0`.
2. `create-accrete` initialized a project at a path containing spaces, using
   `America/Los_Angeles`.
3. `result record` durably published a Result.
4. `memory append` durably published linked authored Memory.
5. Both object operations returned `committed:true` and `index_pending:false`.
6. The installed executable digest and all release artifact checksums matched
   the release manifest.

The machine-readable distribution observations are retained in
`docs/distribution-validation.json`.

## Boundaries of these results

Linux x64 is the compiled, executed prebuilt artifact in this delivery. CI
configurations exist for Linux ARM64 and macOS Intel/Apple Silicon; those jobs
were not run here. Windows is not a supported release target. npm package names
have not been reserved and no package was published.

The tests exercise normal Unix filesystem behavior and synthetic failure
conditions. They do not establish power-loss durability across every filesystem
or concurrent multi-host writes. The documented contract requires host-local
state and a single writing host per shared metadata view.

Schema JSON files parse and their examples follow the implementation conventions;
external JSON Schema-validator coverage is not claimed. Migration was exercised
on representative original-scaffold metadata, without accessing live historical
repositories or remote scientific payloads.

## Workspace boundary

Rerun on macOS arm64 with Rust 1.90.0 and Node 24.13.1 after moving Workspace creation
to the `accrete-kit` generator. `cargo test --locked` passed 33 integration tests with no
failures (CLI workflows 13, Data and migration 6, persistence 8, Workspace resolution 6),
and `cargo fmt --all --check` passed. Tests of template installation and upgrade moved to
`accrete-template` with the code they cover.

The release binary was run from directories holding only a hand-written
`.accrete/project.toml`, once with `format` and `project_id` alone and once with `name`,
`timezone`, `[paths]` and `[locations.working]` as well:

- `config resolve` found the Workspace from the working directory and wrote nothing.
- `result record` and `memory append --result` returned `committed:true`,
  `durability:"durable"` and `index_pending:false`.
- `index refresh`, run from a nested directory, reported both objects, and
  `sqlite3 -readonly` read the Memory entry's link to the Result.
- `results/`, `memory/` and the host-local view directory appeared only when first used.
- On the two-line marker, with no Git repository, `tool admit`, `tool expose`, `data
  register` and `data materialize --to working` also completed.
- Outside a Workspace, the ancestor search, `--project` and `ACCRETE_PROJECT` each exited 3
  with a message naming `npx accrete-kit@latest create`. `init` and `project` are rejected as unknown
  subcommands, exit 2.

`package-release.mjs` built `accrete-0.1.0-aarch64-apple-darwin.tar.gz` on this Mac, holding
`accrete`, `LICENSE`, `THIRD-PARTY-NOTICES`, `licenses/` and `release.json` under one
directory, and `SHA256SUMS` verified it with `shasum -a 256 -c`. Extracted into
`.accrete/bin/` beside a two-line marker, the binary recorded a Result and ran `index refresh`,
both from the Workspace root and, with `--project`, from outside it.

Not exercised here: the CI and release workflows themselves, including GitHub Release
uploads, the Linux and macOS Intel targets, and a Workspace written by the generator rather
than by hand.
