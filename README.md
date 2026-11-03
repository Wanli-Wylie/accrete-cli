# ACCRETE CLI

**A working Rust kernel for Models, Tools, Data, Results, and Memory.**

ACCRETE gives persistent identities and relationships to the computational
materials accumulated during scientific computing and software development.
Claude Code, Codex, and other agents use their normal terminal, programming,
Git, and domain tools. The `accrete` executable handles the organizational
writes that retain their work.

This repository is `accrete-cli`, version **0.1.0**. It contains the native
implementation, release packaging, integration schemas, manuals, and functional
tests. It works inside an existing
[Workspace](docs/workspace.md). Workspaces, with their Skills and agent
instructions, are created by the `accrete-kit` generator from the
`accrete-template` repository.

## Implemented behavior

| Area | Working operations |
|---|---|
| Project | Find the Workspace from `--project`, `ACCRETE_PROJECT` or the nearest marker; resolve shared/local/invocation configuration; create stores and host-local state on first use |
| Model | Register an embedded or external repository; bind a local checkout; create/attach actual Model Git worktrees; close iterations; explicitly remove worktrees; retain cited commits under native Git refs |
| Tool | Admit live or selected retained source; bind external source; update metadata; generate Codex/Claude project Skill exposures; refresh, retire, and export capabilities |
| Data | Register files, directories, collections, or external locators in place; add locations; optionally hash files; copy local/rclone sources into managed local replicas; evict a named owned replica |
| Result | Record arbitrary structured activity descriptions; amend with a byte revision; append retry-safe observations; optionally observe the present checkout or capture selected source changes |
| Memory | Publish complete Markdown with stable IDs, links, project-timezone numbering, and concurrency-safe allocation |
| Persistence | Atomic individual document publication, expected-revision conflicts, stable-ID retries, bounded recovery receipts, and explicit index-pending success |
| Index | Transactionally refresh a native SQLite projection with object, relationship, document, and freshness tables |
| Exchange | Export/import small metadata bundles with per-object outcomes and Memory filename collision handling |
| Migration | Convert the supplied legacy scaffold's conventional metadata with originals, persistent ID maps, and restartable per-document conversion |
| Distribution | Native archives for four platforms attached to GitHub Releases, with CI configurations |

JSON and Markdown are the durable representation. SQLite is disposable and
host-local. Scientific payloads remain in their native representation and
location. Ordinary source edits, computation, scheduling, SSH, Git integration
strategy, and scientific interpretation stay with agents and their tools.

## Build and install

### Build on your current machine

Prerequisites: Rust **1.90.0**, a C compiler for bundled SQLite, and Git for
Model operations. `rust-toolchain.toml` pins the compiler; `Cargo.lock` pins the
resolved dependency graph.

```sh
cargo build --release --locked
./target/release/accrete --version
./target/release/accrete --help

# Alternatively install into Cargo's executable directory:
cargo install --path . --locked
```

This works from the source repository on the supported Unix targets. A release
binary needs no Rust, Node, Python, or system SQLite to run. See
[VALIDATION.md](VALIDATION.md) for the platforms actually exercised.

### Package a native archive

Node.js **20 or newer** is needed only to run the packaging script:

```sh
node scripts/package-release.mjs --binary target/release/accrete --target aarch64-apple-darwin --out dist/local
```

See [distribution.md](docs/distribution.md) for targets. CI configurations cover
Linux x64/ARM64 and macOS Intel/Apple Silicon. Windows is outside this release's
distribution support. The Kernel is not published to npm; releases are archives on
GitHub.

## First Workspace

The `accrete-kit` generator creates a Workspace and installs the Kernel in it as a plain
native executable, so Workspaces on different Kernel versions can coexist. From the
Workspace root, run it as `.accrete/bin/accrete`:

```sh
npx accrete-kit@latest create research --timezone Asia/Shanghai
cd research
.accrete/bin/accrete config resolve --json
```

From anywhere else, such as a Model checkout or run directory, name the Workspace:

```sh
<workspace>/.accrete/bin/accrete --project <workspace> config resolve --json
```

Hosts without Node extract the release archive into `<workspace>/.accrete/bin/`, as
[distribution.md](docs/distribution.md#direct-native-installation) describes. The rest of
this manual writes `accrete` as shorthand for whichever of these applies.

Store directories and host-local state appear on first use. No source commit is
required to record Results or Memory. A directory holding only
`.accrete/project.toml` is enough for the Kernel; [workspace.md](docs/workspace.md)
states what the marker needs.

Shared configuration travels with the repository. Machine paths belong in
the ignored `.accrete/local.toml`:

```toml
host = "workstation"

[local_state]
base = "/local/research/accrete-state"

[models.target]
checkout = "/work/model-checkouts/target"

[locations.working]
root = "/scratch/research/data"
```

Choose a host-local state location when a home/project directory is on NFS or
another shared filesystem. The resolver partitions it by project identity and
canonical managed document roots. Separate metadata worktrees get separate
indexes. See [configuration.md](docs/configuration.md).

## A normal working sequence

Register a repository you already work with. The name is an editable alias;
the returned `object_id` is its durable identity.

```sh
accrete model register --name target --checkout /work/model-checkouts/target --json
accrete model iteration open target --name improve-solver \
  --branch improve-solver --worktree /work/iterations/improve-solver --json
```

Edit, compile, run, commit, and integrate through native tools. When an output
is worth retaining, register it without moving it:

```sh
accrete data register --name year-one \
  --path /scratch/run-17/year-one --kind directory --json
```

Write the activity description as ordinary JSON. Associations may point to
objects not present on this host:

```json
{
  "id": "run-17",
  "name": "Year-one comparison",
  "models": [
    {"model_id": "RETURNED_MODEL_ID", "commit": "EXECUTED_GIT_COMMIT", "role": "target"}
  ],
  "inputs": [{"data_id": "forcing-2005", "role": "forcing"}],
  "outputs": [{"data_id": "RETURNED_DATA_ID", "role": "year-one"}],
  "execution": {"launcher": "project-owned", "mpi_ranks": 16},
  "observations": [{"elapsed_seconds": 1234.5}]
}
```

Replace illustrative IDs/revisions with the actual values, save the document
as `scratch/result.json`, and submit it:

```sh
accrete result record --from scratch/result.json --json
accrete memory append --from scratch/interpretation.md --result run-17 --json
accrete index refresh --json
```

Native tools read the resulting materials:

```sh
rg 'solver|forcing' memory
jq '.observations' results/run-17.json
# Use the index path reported by config resolve:
sqlite3 -readonly /resolved/local/state/VIEW/index.sqlite \
  'SELECT result_id,model_id,commit_id FROM result_models;'
```

No launch wrapper or workflow state machine is required. A Result may be
recorded before, during, or after an activity. The Kernel never fills an old
execution's revision from today's HEAD. `--observe-checkout` adds separately
labeled recording-time context.

## Mutation responses and retries

Use `--json` in agent-facing calls. A normal response includes:

```json
{
  "object_id": "run-17",
  "path": "/work/research/results/run-17.json",
  "revision": "SHA256_OF_EXACT_DOCUMENT_BYTES",
  "committed": true,
  "published": true,
  "durability": "durable",
  "index_pending": false,
  "ok": true
}
```

`result amend`, `model update`, `tool update`, and whole-descriptor amendments
require `--expect REVISION`. The patch uses JSON Merge Patch: objects merge,
arrays replace, and `null` removes a field. IDs, kinds, format, project identity,
and creation time remain unchanged. The comparison hashes the actual current
file bytes, including any native edit. Read the current file and deliberately
combine changes after a conflict.

Supply an `id` when a create may need retrying after a lost response. Identical
content for that identity returns the existing object; divergent content is a
conflict. Observation appends accept `--operation-id`. Memory supports `--id`.
Successful document publication with a temporarily unavailable index returns
exit **0**, `committed:true`, and `index_pending:true`; repair the projection
with `index refresh`. Do not repeat the computation to repair an index.

## Skills and manuals

Skills, manuals and their upgrades belong to the generator
(`npx accrete-kit@latest upgrade`). Installing a newer binary changes nothing in a
Workspace. `accrete interface --json` reports this Kernel's versions and conventions.
See the [command manual](docs/cli-reference.md) and [schemas](schemas/).

## Documentation and checks

- [CLI reference](docs/cli-reference.md): exact commands, payloads, flags, exit codes.
- [Configuration](docs/configuration.md): project discovery, path rules, local state and binding.
- [Persistence](docs/persistence.md): envelopes, byte revisions, SQL schema, concurrency and exchange.
- [Data](docs/data.md): location/replica semantics, rclone adapter, explicit content checks and eviction.
- [Migration](docs/migration.md): original scaffold conversion, retained originals and restart behavior.
- [Workspace](docs/workspace.md): what the Kernel requires of a Workspace, and the marker's fields.
- [Distribution](docs/distribution.md): build targets, native archives and owner release procedure.
- [Implementation decisions](docs/implementation.md): choices left open by the design and this release's practical scope.
- [Validation](VALIDATION.md): completed checks and unexercised platform boundaries.

```sh
cargo test --locked
cargo fmt --all --check
```

The tests exercise persistence, actual CLI/Git workflows, Data replicas, legacy
conversion and Workspace resolution, using temporary metadata and small files. They
do not run the historical scientific models.

## License

MIT; see [LICENSE](LICENSE). Bundled third-party notices are generated from
the pinned release dependency tree and travel with binary distributions.

