# ACCRETE CLI reference

This manual describes `accrete` 0.1.0, interface 1, project/document format 1,
and projection format 1. `accrete interface --json` reports these conventions
without requiring a project. The executable performs organizational writes;
read source, documents, observations, and scientific arrays with their native tools.

## Invocation and output

```sh
accrete --help
accrete --version
accrete interface --json
accrete --project /work/research --host gpu-a --json config resolve
accrete model iteration open --help
```

Global options may appear before or after a subcommand:

| Option | Meaning |
| --- | --- |
| `--project DIRECTORY` | Project root; takes precedence over `ACCRETE_PROJECT` and ancestor discovery. |
| `--state-dir DIRECTORY` | Host-local state base, partitioned by metadata-view key. |
| `--host LABEL` | Host alias used for local bindings and availability observations. |
| `--json` | One JSON response on stdout; diagnostics and structured errors on stderr. |

`--host gpu-a` identifies the machine context. `tool expose --agent codex` selects
an agent discovery interface. These options have different purposes.

Object selectors accept a stable ID, name, or alias. IDs take priority; ambiguous
aliases conflict. Creation normally generates a UUID. Supply `--id` when a caller
needs to retry a submission after losing its response. A repeat with matching
content returns the same object; changed content requires an amendment.

Creation commands accept `--from FILE`, `--id`, `--name`, and `--description`.
The convenience flags override matching fields in the supplied JSON object.
Unknown extension fields remain in the stored document. `--from -` accepts stdin
for object creation, amendments, Data locations, and Memory append; the portable
bundle import and template/migration commands require actual paths.

Input JSON/Markdown filenames, metadata export/import paths, and template/migration
source paths resolve from the invocation directory. Model checkout/worktree paths,
Tool source/export/exposure paths, Data paths, and source-capture destinations
resolve from the project root. Absolute paths are useful across agent worktrees.

## Initialization and configuration

```sh
accrete init ./research --name research --timezone Asia/Shanghai
accrete init ./research --template ../accrete-template
accrete init ./research --no-git
accrete --project ./research config resolve --json
```

`init [DIRECTORY]` defaults to the current directory. It creates the shared
`.accrete/project.toml`, native store directories, and local-storage ignore rules.
It initializes Git when needed, unless `--no-git` is supplied. It does not create
an initial commit. `--id UUID` provides an explicit project identity.

Repeated initialization preserves the existing identity and configuration. A
different requested ID or name conflicts. Existing Git source remains where it is.
An optional template is installed through the same ownership-aware mechanism as
`project upgrade`; an asset conflict can leave native initialization completed.

Shared settings live in `.accrete/project.toml`; ignored machine bindings live in
`.accrete/local.toml`. Defaults are overridden by shared configuration, then local
configuration, then supported invocation overrides. Configuration is ordinary TOML.
Read [configuration.md](configuration.md) for path and host conventions.

## Models and iterations

```sh
accrete model register --id model-main --name target --checkout ../numerical-model
accrete model register --from model.json
accrete model bind model-main --checkout /work/checkouts/target
accrete model update model-main --from model-change.json --expect REVISION
accrete model retire model-main --expect REVISION
```

Registration takes an existing Git checkout or a `repository` locator supplied in
JSON. It does not clone or fetch. Checkouts are local bindings; portable descriptors
retain identity, source scope, repository hints, and any registration observation.
`--source-scope PATH` can be repeated to record selected Git tree/blob scopes.
A registration observation is not automatically an execution-time revision.

```sh
accrete model iteration open model-main --id investigation-1 \
  --name investigate-routing --purpose 'Study routing changes'
accrete model iteration open model-main --id investigation-2 \
  --branch investigate-routing --worktree ../target-routing --start HEAD
accrete model iteration open model-main --id existing-effort \
  --worktree ../already-created-worktree --existing
accrete model iteration close investigation-2 --outcome integrated \
  --commit COMMIT --expect REVISION
accrete model iteration remove-worktree investigation-2
```

Opening without `--worktree` records an effort without allocating working storage.
With a new `--worktree`, the command uses the Model's actual Git repository.
`--branch` creates the requested new branch; without it, Git creates a detached
worktree. `--existing` attaches an existing worktree of the same repository.
`--start` defaults to `HEAD`; an attached existing worktree supplies its own HEAD.

Closing records a free-text outcome and optional retained revision. It does not
merge, delete a branch, or remove storage. `remove-worktree` is separate and keeps
Git's normal refusal to remove changed work; `--force` explicitly requests Git's
destructive removal behavior. Worktree receipts retain interrupted effects for
`recover`; recovery attaches surviving resources without repeating development.

Locally available cited commits may be kept under
`refs/accrete/retained/OBJECT_ID`. Those refs are local. Use native Git push or
bundle commands with an explicit refspec when transporting them to another host.

## Tools and agent exposure

```sh
accrete tool admit --id tool-residual --name residual-analysis \
  --description 'Analyze retained residual fields' --source tools/residual-analysis
accrete tool admit --from tool.json
accrete tool bind tool-residual --source /work/capabilities/residual-analysis
accrete tool update tool-residual --from tool-change.json --expect REVISION
```

The source directory contains the capability's ordinary code and knowledge.
Admission changes its durable project description; native editors and Git continue
to develop its source. A Tool can contain only instructions and references.

`--source-mode live` is the default. A retained selection uses
`--source-mode retained --revision COMMIT`. Its exposure requires a clean checkout
at that commit. Bind a matching retained checkout when necessary. ACCRETE does not
change the user's active branch to produce a historical exposure.

```sh
accrete tool expose tool-residual --agent codex
accrete tool expose tool-residual --agent claude --mode wrapper
accrete tool expose tool-residual --agent codex --mode symlink \
  --name residual-analysis
accrete tool refresh tool-residual
accrete tool retire tool-residual --expect REVISION
accrete tool export tool-residual --to ../residual-analysis-export
```

The default wrapper installs a generated `SKILL.md` in `.agents/skills/NAME` for
Codex or `.claude/skills/NAME` for Claude. It points to the original source and
explains where relative script/reference paths resolve. `--destination DIRECTORY`
selects an explicit location. Names use lowercase letters, digits, and single
hyphens, with a maximum of 64 characters.

Symlink exposure is available on Unix. It requires source `SKILL.md` frontmatter
whose `name` matches the exposure and whose `description` is nonempty. Generated
exposures are locally inventoried; unrelated or edited destination content is
preserved and reported as a conflict. A Tool remains admitted if exposure is pending.

Refresh regenerates existing managed exposures; a retired descriptor causes local
managed exposures to be removed when possible. Retirement preserves the descriptor
and historical references. Already loaded agent sessions may retain their content.
Tool export creates a local directory containing `tool.json` and `source/`, omits
Git internals, and refuses symlinks whose transfer semantics need an explicit choice.
It does not publish to a registry or remote repository.

## Data registration and locations

```sh
accrete data register --id year-1 --name year-1-output \
  --path scratch/year-1/output --kind directory --result run-1
accrete data register --id restart-1 --path scratch/restart.nc --hash
accrete data register --from external-data.json
accrete data register --name live-output --path scratch/live-run --live
```

Registration records existing material in place. It does not copy, pack, compress,
or recursively hash it. A regular local file contributes its size. `--hash`
explicitly computes SHA-256 for one regular file; directories can refer to a
producer-owned manifest. Supported content kinds are `file`, `directory`,
`collection`, and `external`.

Content mode defaults to `closed`; `--live` records mutable material. A corrected
closed edition normally receives a new Data ID. Origin and locations are separate:
`--result ID` supplies `origin.result_id`, while locations describe access paths.
Registration marks borrowed paths as unmanaged. A supplied backend token remains
an opaque token, and a path without a version or digest does not prove frozen bytes.

```sh
accrete data location add year-1 --location archive --path retained/year-1
accrete data location add year-1 --from location.json --expect REVISION
accrete data amend year-1 --from description-change.json --expect REVISION
accrete data retire year-1 --expect REVISION
```

Location input accepts `name`, `kind`, `path`, `host`, and descriptive observations.
`--kind filesystem|rclone|external` can override the input. A named configuration
supplies its backend/root; local filesystem paths are associated with the current
host unless explicitly supplied. A repeated matching location is a no-op.
`data amend` changes descriptive metadata; it cannot redefine closed content or
directly replace the locations array. Retirement does not delete bytes.

## Materialization, eviction, and recovery

```sh
accrete data materialize year-1 --to working
accrete data materialize year-1 --source archive --to analysis --expect REVISION
accrete data evict year-1 --location LOCATION_ID --expect REVISION
accrete recover
accrete recover --operation DATA_OPERATION_UUID
```

Destinations are named configured filesystem locations. The initial adapters copy
local regular files/directories or download through a configured rclone remote.
If several remote sources are possible, select a location ID or unambiguous name.
The default `working` location is `.accrete/data` beneath the project root.

Materialization uses a private staging path and publishes a completed replica before
adding its location. Known SHA-256 values are checked when applicable. The response
names the actual checks performed. Symlinks and special files require deliberate
packaging outside the Kernel. No directory is automatically converted into an archive.

For rclone, configure the remote with rclone's own credential mechanism and put its
name/prefix in local configuration. `native.rclone` can select its executable.
The adapter uses `copy` or `copyto` with `--immutable`. Explicit backend version
selectors are currently rejected; use a version-specific exported locator or perform
the native transfer and add its resulting location. Remote uploads are outside this
first adapter. See [data.md](data.md) for supported transfer details.

Eviction applies only to a replica created and owned by ACCRETE, with its matching
ownership marker. Registered borrowed paths are not eligible. Metadata identity,
other locations, and earlier Result references remain. Recovery reconciles completed
transfers, publications, and worktree receipts. It does not relaunch simulations.
If transfer itself was incomplete, inspect its staging path and retry deliberately.

## Results and observations

```sh
accrete result record --from result.json --id run-1
accrete result record --from result.json --observe-checkout ../numerical-model
accrete result amend run-1 --from result-change.json --expect REVISION
accrete result observe run-1 --text-file scratch/observation.md \
  --operation-id observation-1
```

A Result is a description of an activity. It may be submitted before, during, or
after execution; it never authorizes or launches that execution. JSON can contain
model/tool associations, input/output Data IDs, timestamps, environment information,
measurements, observations, and project extensions. Omitted historical details stay
unknown. `--observe-checkout` adds a separately labelled recording-time observation.

`--operation-id` makes an observation append retryable. Repeating the same ID and
text is a no-op; reusing it for different text conflicts. Supply execution-time
revisions and actual environment information from the launcher that knows them.

```sh
accrete result capture --checkout ../numerical-model \
  --path src --path pyproject.toml --to scratch/source-capture
```

Explicit source capture writes a selected binary Git diff and selected nonignored
untracked regular files to a new directory. Its manifest states the captured scope.
Ignored products, external dependencies, and runtime state remain outside it; it is
not a complete executable environment. Register the resulting artifact as Data if useful.

## Memory and index maintenance

```sh
accrete memory append --from scratch/understanding.md --id interpretation-1 \
  --title 'Routing investigation' --result run-1 --model model-main
accrete memory append --from scratch/correction.md --supersedes interpretation-1
accrete memory append --from scratch/notes.md --links-from scratch/links.json
accrete index refresh --json
```

Memory requires complete nonempty UTF-8 prose. Repeat `--result`, `--model`,
`--tool`, `--data`, or `--supersedes` for associations. `--links-from` accepts an
additional JSON link array. `--authored-at` supplies an explicit RFC 3339 instant;
the publication date directory still uses the current project-local calendar day.

The Kernel publishes YAML frontmatter containing an ordinary integer `format: 1`,
stable ID, project ID, authored timestamp, and links. The body stays unchanged.
Frontmatter containing numbers outside the YAML library's exact numeric range uses
valid JSON-form YAML and is read through the JSON parser to preserve those numbers.
Filename collisions during import are renumbered without changing identity.

`index refresh` rebuilds ordinary SQL tables transactionally from managed metadata.
Read the resulting index with SQLite, Memory with `rg`, and JSON with `jq` or an
editor. `config resolve` reports the index path. There is no ACCRETE query command.
See [persistence.md](persistence.md) for tables, byte revisions, and freshness.

## Metadata exchange and legacy migration

```sh
accrete export --to handoff.json
accrete export --to selected.json --id run-1 --id interpretation-1
accrete import --from handoff.json
accrete import --from another-project.json --allow-foreign
accrete migrate --from ../legacy-workspace --dry-run
accrete migrate --from ../legacy-workspace
```

Metadata bundles contain JSON/Markdown, IDs, revisions, and original Memory paths.
They omit scientific payloads, Git objects, Tool source, and machine bindings.
Import reports each object's outcome; a batch is not atomic. Repeating the same
bundle acknowledges existing entries and completes missing ones. Divergent content
requires an explicit `expected_revision` in that entry. Cross-project import needs
`--allow-foreign` and retains original project IDs.

Legacy migration reads the supplied scaffold's `data/index.tsv`, Result JSON,
dated Memory, and recognized Model/Tool arrangements. It retains exact original
metadata backups and a persistent identity map. Source changes after planning
produce a conflict. Repeating an interrupted migration uses its existing mapping.
Scientific payloads and Model histories remain at their existing locations.

## Separate template integration

```sh
accrete project upgrade --from ../accrete-template --dry-run
accrete project upgrade --from ../accrete-template
```

The template directory supplies `accrete-template.json`, with `format: 1`,
`asset_version`, a `requires` range, and explicit `files` entries. Each entry has
`source`, `destination`, and optional `executable`. Paths are portable relative
file paths. CLI version bounds use numeric `major.minor.patch`, with an exclusive
upper bound. The manifest schema is [template-manifest.schema.json](../schemas/template-manifest.schema.json).

An inventory tracks installed hashes and executable modes. Refresh preflights all
destinations, preserves owner edits, and replaces only matching managed assets.
Protected metadata/state/source roots and symlink traversal are rejected. Unlisted
old assets are retained. An interrupted refresh resumes by repeating the same
source snapshot. Binary installation does not update this inventory or project formats.

## Stable outcomes and exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success, including a committed document whose index refresh is pending. |
| `2` | Invalid invocation, input, or unsupported format. |
| `3` | Unresolved configuration/project or absent object. |
| `4` | Revision, identity, ownership, or destination conflict. |
| `5` | Native dependency unavailable. |
| `6` | Storage failure before the requested publication completes. |
| `7` | Incomplete external operation or partially failed import. |
| `8` | Publication is visible but durability is uncertain. |

Successful mutations return their object ID, absolute path, byte revision, and
completion details. `index_pending: true` means the document already exists; run
`index refresh` instead of repeating the scientific activity. Keep the same ID
when reconciling a lost acknowledgement. Post-publication uncertainty names surviving
paths and a receipt where applicable. Errors leave drafts available for an explicit
merge or retry. JSON Schema files document the stable envelope while preserving
extension fields; the native operations enforce their own filesystem/Git constraints.
