# `accrete-template` integration contract, version 1

The CLI repository owns native initialization, project formats, object operations,
file publication and generated-file ownership. The independent template repository
owns foundational Skills, command explanations, Coordinator guidance and optional
host assets. No coordination with its development session is required to use this
interface.

A template snapshot is an ordinary directory containing `accrete-template.json`
and the files it names. It need not be a Git checkout. The native kernel reads the
snapshot locally and copies declared small assets. It does not clone repositories,
select a template release over the network, execute template scripts or interpret
agent instructions as software operations.

## Manifest

```json
{
  "format": 1,
  "asset_version": "0.1.0",
  "requires": {
    "cli_min": "0.1.0",
    "cli_max_exclusive": "0.2.0",
    "project_format": 1
  },
  "files": [
    {
      "source": "template/AGENTS.md",
      "destination": "AGENTS.md"
    },
    {
      "source": "template/CLAUDE.md",
      "destination": "CLAUDE.md"
    },
    {
      "source": "template/.agents/skills/accrete-models/SKILL.md",
      "destination": ".agents/skills/accrete-models/SKILL.md"
    },
    {
      "source": "template/.claude/skills/accrete-models/SKILL.md",
      "destination": ".claude/skills/accrete-models/SKILL.md"
    }
  ]
}
```

`format` is the manifest format, independently of the project format. Both are 1
in this release. `asset_version` is a nonempty template release label; it need not
be the CLI package version because the two repositories are independently owned.
`cli_min` is inclusive; `cli_max_exclusive` is exclusive. Both must have three
numeric components (`major.minor.patch`), with no ranges, prerelease or build
suffix. Compatibility is checked against the running native executable before any
asset changes. A future template can narrow its supported CLI interval explicitly.

Each source and destination is a nonempty forward-slash relative file path.
Absolute paths, drive letters, backslashes, `.`/`..` segments, control characters,
symlink traversal and duplicate destinations are rejected. A destination cannot
also be another destination's parent. Sources must be ordinary files. Directories
are created automatically. The optional `executable` Boolean defaults to false;
Unix files are installed with mode 0644 or 0755 respectively. Source file modes
are not inferred.

The manifest lists files explicitly. There are no glob patterns, implicit recursive
copies, interpolated variables, install hooks, file deletions or managed-section
text rewriting. This keeps a template snapshot inspectable and gives all persistent
changes the same native publication path.

Destinations cannot overlap `.git`, the shared/local project configuration, the
template inventory, managed object/Result/Memory roots, domain Tool source roots,
Scratch or local state. Templates should contain foundational guidance and host
assets, not scientific payloads or domain implementations. The native initializer
already creates `.gitignore`; omit a template-wide replacement for that file.

## Initialize and refresh

```sh
accrete init ./new-project --name 'Ocean study' --timezone America/Los_Angeles \
  --template ../accrete-template

accrete --project ./existing-project project upgrade \
  --from ../accrete-template --dry-run --json

accrete --project ./existing-project project upgrade \
  --from ../accrete-template --json
```

`init` creates `.accrete/project.toml` and empty managed roots, adds narrow local
ignore entries, and initializes Git unless `--no-git` is supplied. It does not
create a first commit, remote, root `package.json`, Agent runtime, or a foundational
Skill suite. The SQL projection is created by the first object write or an explicit
`accrete index refresh`; initialization does not require an empty database. If the directory is already inside a Git repository, no nested Git
repository is created. `--template` invokes the same native refresh as
`project upgrade` after initialization.

Repeating `init` preserves an existing project marker and its timezone. Its name
and ID flags are checked when explicitly supplied. `--timezone` chooses the
calendar for a newly created project; edit shared configuration deliberately to
change an existing project's timezone. `--state-dir`, `--host` and the corresponding
configuration/environment choices apply to initialization just as to other commands.

A dry run reports each destination as `create`, `replace`, `unchanged`,
`finish_permissions` (interrupted publication), or `conflict`. It writes no project
assets or template inventory. Acquiring the ordinary local lock may initialize
host-local state. An applying refresh stops on every already-known conflict before
writing assets, and its error includes the concrete paths.

## Ownership and owner edits

The portable generated-file inventory is `.accrete/template-install.json`:

```json
{
  "format": 1,
  "asset_version": "0.1.0",
  "manifest_sha256": "sha256-of-manifest-bytes",
  "installed_at": "2026-11-03T12:00:00.000Z",
  "files": {
    "AGENTS.md": {
      "sha256": "sha256-of-installed-file-bytes",
      "executable": false
    }
  }
}
```

The actual values are emitted by the kernel. These illustrative hashes and times
are not valid installed state. The inventory covers only declared installed
assets. It does not fingerprint the project, scientific programs, or data.

An installed file may be replaced when its current digest and executable flag
still match the previously installed base. A changed file is an owner edit and is
preserved. An existing file not owned by the inventory is a conflict, even if it
happens to match a template file. The kernel does not silently adopt existing
`AGENTS.md` or `CLAUDE.md`. A missing destination can be recreated from the
explicitly selected template. Files omitted from a later manifest are retained;
there is no implicit deletion or removal of their historical inventory entries.

For an edited file, inspect the candidate directly at the reported template source,
then merge with an ordinary editor or Git. This release intentionally provides no
force-overwrite switch. A project that wants to preserve existing root guidance
can omit those destinations from its manifest and reference the installed manuals
from its own prose. A maintained template should keep root guidance short to limit
these conflicts.

## Publication and restart

Refresh holds the project's local metadata-view lock. It reads all incoming asset
bytes, computes the concrete plan, then writes a host-local recovery receipt before
publishing files individually. It rechecks each destination immediately before
publication. After asset publication it writes the portable inventory and removes
the receipt. The operation is a restartable sequence, not an atomic transaction
across a whole tree.

If interrupted, rerun with the same template snapshot. The saved receipt recognizes
already-published files, preserves their new ownership and finishes remaining
work. Source-byte changes or later destination edits produce an explicit conflict.
The error/response names the receipt where continuation is required. A request for
a different template waits until the original interrupted snapshot is resolved.
A directory-sync failure reports `durability: "uncertain"` and retains the receipt;
a successful retry synchronizes the published files and directories.

Project files may already exist if initialization succeeds and its requested
`--template` conflicts. Correct the chosen manifest or destination and rerun
`project upgrade`; initialization is not rolled back by deleting the project.

## Agent-facing stable interface

Foundational guidance should use `accrete --help` and the documented commands in
[cli-reference.md](cli-reference.md). Object-write receipts use IDs, concrete paths and byte-derived
revision tokens; callers should consume JSON with `--json`. Large/nested payloads
belong in `--from FILE` or standard input, and an amendment supplies `--expect`.
Native Git, SQL and file readers remain the normal reading interfaces.

Keep foundational persistence operations in the Rust kernel. A Skill may explain
commands and a domain capability may contain its own scientific code, but a
second Python or JavaScript writer for managed ACCRETE metadata would violate the
integration boundary. Domain Tool exposure is a separate `tool expose/refresh`
operation and is not owned by this foundational template inventory.
