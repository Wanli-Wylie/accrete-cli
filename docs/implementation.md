# Implementation decisions for 0.1.0

This implementation follows the attached **ACCRETE Architecture and Design,
3 November 2026**. The owner subsequently separated delivery into `accrete-cli`
and `accrete-template`; that instruction determines the guidance seam here.
The original design is retained in `architecture/ACCRETE-Architecture-Design.zip`
for reference. Its evidence describes earlier projects, not tests of this CLI.

## Decisions resolved during implementation

| Question left open in the design | Choice in this repository |
|---|---|
| Compiler and dependencies | Rust 1.90.0, edition 2024, synchronous implementation; exact direct versions plus Cargo.lock; bundled SQLite |
| Project/document/index versions | All start at 1; CLI release starts at 0.1.0; explicit compatibility checks reject unsupported formats |
| Durable IDs | Random UUID by default; safe explicit string IDs retain legacy Result identities and support retries |
| Revision token | Lowercase SHA-256 of the exact published document bytes |
| Whole-document editing | JSON Merge Patch applied under the view lock with required expected byte revision |
| Local publication | Flushed temporary sibling, atomic no-clobber creation/replacement, Unix directory synchronization, complete parent creation; uncertain post-publication synchronization is a distinct outcome |
| Metadata serialization | One local OS file lock per canonical metadata view; long copies/Git operations run outside it |
| SQLite refresh | Full metadata snapshot plus one immediate SQLite transaction, WAL, 5-second busy timeout, 100,000-document bound |
| Native read surface | Files, Git and SQLite; no duplicated list/show/query/search/log command families |
| Memory | YAML frontmatter, stable ID and authored instant, complete Markdown body, IANA project timezone, `YYYY/MM/DD/NNN.md` |
| Location ownership | Registration is borrowed; materialization creates a unique container, ownership marker and replica location |
| Data transport | Ordinary file/directory copies and rclone download into local staging; no implicit archive format or hardlinks |
| rclone version selection | Opaque backend version selectors are preserved but refused by generic materialization; use an explicit version-specific locator/native retrieval |
| Host exposure | Project-local generated Skill wrapper by default, Unix symlink option, local inventory; source-relative paths are spelled out |
| Retained Tool source | A clean Git checkout at the explicitly selected commit; the CLI does not change the owner's branch |
| Source capture | Selected tracked binary diff and selected nonignored untracked regular files; declared scope accompanies it |
| Git retention | Local `refs/accrete/retained/OBJECT_ID/COMMIT`; no automatic push/fetch or merge |
| Portable exchange | JSON metadata bundle, individual outcomes, IDs preserved, explicit predecessor for divergent replacement |
| Initial migration | Supplied scaffold's conventional small-file layout with saved plans, originals, stable mappings and exact-predecessor replacements |
| Template integration | Local `accrete-template.json`, explicit version interval, per-file ownership hashes, resumable apply and user-edit preservation |
| Distribution | Exact-version npm optional native payloads, no install scripts/download fallback, direct native archives |
| Platform scope | Linux x64/ARM64 musl and macOS x64/ARM64 release configuration; this delivery exercises Linux x64 |
| License | MIT for new repository code; upstream notices accompany binary artifacts |

## Module map

`config` resolves the selected project, paths and host. `store` owns identity,
locking, revisions and individual document publication. `index` projects
portable documents into SQLite. `models`, `tools`, `data`, `results` and
`memory` implement their concrete object operations. `git` invokes native Git
with argv arrays. `exchange` imports and exports small metadata bundles.
`migration` converts original scaffold metadata. `project` initializes and
applies declared template assets. `main` declares the CLI and its structured
output/error conventions.

The application is a normal Rust library plus one executable. Domain Tools
call the CLI as a process. There is no in-process plugin ABI, central server,
scheduler, daemon, LLM API client, agent supervisor or scientific execution
wrapper. The npm code only selects and invokes an executable. No Python script
implements persistence.

## Practical scope and completion boundaries

Model/Tool registrations and local bindings complete separately. Worktree
creation and iteration publication complete separately, with a local receipt
bridging the gap. Iteration closure records an outcome; removal is explicit
and retains Git's refusal to delete dirty work unless `--force` is supplied.

Data copies publish a complete local replica before the corresponding location
amendment. The receipt identifies the exact staging/destination paths.
Recovery completes proven transfer effects; it never starts a new transfer or
scientific execution. A borrowed input remains borrowed throughout registration,
retirement and eviction of other copies. The Kernel does not inspect NetCDF,
HDF5, Zarr or Parquet internals.

Filesystem links/special files need an explicitly prepared project artifact
before generic Data copying or Tool export. Uploading to remote storage uses
native rclone followed by `data location add`; the initial adapter downloads
into local managed storage. Producer manifests are retained and queryable,
with interpretation left to their domain Tool.

Result Model/Tool/Data links can remain unresolved locally. A present checkout
does not establish a past execution identity. Explicit execution revisions
are retained as supplied, and local retention attempts report warnings when
the repository/revision cannot be retained. Recording-time observations stay
in a separate field. Native Git namespace refs require explicit transport if
needed on another host.

For Tool exposure, retirement removes only unchanged generated files owned by
the local inventory. An edited wrapper is preserved and reported pending.
Other clones refresh their own exposures after receiving a retired descriptor.
Running host sessions may already have loaded earlier instructions; file
exposure does not claim to replace their conversational context.

The local index is a convenience for metadata queries. Every mutation currently
rebuilds its small projection transactionally; no scientific payload is walked.
Projects approaching the documented metadata bound can replace this with an
incremental projection later without changing the primary document formats.
An index failure after publication leaves a successful retained object and a
repairable projection. No command pretends that Git, files, SQLite and remote
storage participate in one transaction.

Template upgrades operate on the declared asset list and file hashes. They do
not install global Skills, select a coding model, create a worker-count policy,
rewrite scientific programs, or update local Tool sources. Future format
migrations remain explicit actions independent of binary/template updates.

