# What the Kernel requires of a Workspace

A Workspace is an independent project owned by the user, created by the npm generator,
`npx accrete-kit@latest create my-project`, which ships with the template in the
`accrete-template` repository. The `accrete` executable has no initialization or template
logic and knows nothing about that repository. It works inside a directory that already
holds a marker, and the two components meet only at this contract.

## The marker

`.accrete/project.toml` is the only thing the Kernel requires. This is a complete marker:

```toml
format = 1
project_id = "6b0a7e0e-5a7c-4f0e-9d2f-1f3c9f4f7a11"
```

| Field | Required | Kernel behavior |
|---|---|---|
| `format` | Yes | Must be integer `1`, otherwise every command that needs a project stops with `unsupported_format` (exit 2). `interface --json` lists accepted values as `project_formats`. |
| `project_id` | Yes | A safe ID: 1-180 ASCII letters, digits, `.`, `_`, `-`; not `.` or `..`. Anything else is a `config` error (exit 3). It is copied into every document and cannot be overridden locally, so it must not change after the first write. |
| `name` | No | Not read by the Kernel. Retained and shown by `config resolve`. |
| `timezone` | No | IANA name, validated, default `UTC`. Numbers Memory paths. |
| `[paths]` | No | Each key defaults independently: `metadata = ".accrete/objects"`, `results = "results"`, `memory = "memory"`, `tool_sources = "tools"`, `scratch = "scratch"`. |
| `[locations.working]` | No | Defaults to `kind = "filesystem"`, `root = ".accrete/data"`. |

Writing `name`, `timezone`, `[paths]` and `[locations.working]` explicitly is fine and
keeps a project's choices visible. Unknown fields and sections are retained. See
[configuration.md](configuration.md) for local overrides and path rules.

## Running the Kernel

`accrete-kit` installs the Kernel as a plain native executable in each Workspace, so
Workspaces on different versions coexist.

| From | Command |
|---|---|
| The Workspace root | `.accrete/bin/accrete` |
| Anywhere else, such as a Model checkout or run directory | `<workspace>/.accrete/bin/accrete --project <workspace>` |

A host without Node extracts the [release archive](distribution.md#direct-native-installation)
into `<workspace>/.accrete/bin/`.

`.accrete/bin/` is not Kernel-owned. The Kernel never reads or writes it and runs equally
well from any other path; only `accrete-kit` and the person extracting an archive put files
there.

## Selection

The Kernel selects a Workspace from `--project PATH`, then `ACCRETE_PROJECT`, then the
nearest ancestor of the working directory holding the marker. When none is found it exits 3
with an error naming `npx accrete-kit@latest create`. `interface --json` needs no Workspace.

## What appears on first use

Nothing else needs to exist beforehand. Resolving a Workspace writes nothing.

| Created by the Kernel | When |
|---|---|
| Host-local state: the view directory, `mutation.lock`, `view.json`, `index.sqlite` | First mutation or `index refresh` |
| `.accrete/objects/`, `results/`, `memory/` | First document of that family |
| `.accrete/data/` | First materialization to `working` |
| `.accrete/local.toml` | First local binding |
| `.accrete/migrations/` | First applied legacy migration |
| `.agents/skills/<tool>`, `.claude/skills/<tool>` | `tool expose`, with the paths added to the repository's `info/exclude` when there is a Git repository |

The Kernel works with or without a Git repository at the Workspace root; only Model
operations need one to act on.

## What belongs to the scaffold

The Kernel never writes `.accrete/project.toml` or `.gitignore`. So that shared Git does not
pick up host-local content, the scaffold ignores `/.accrete/local.toml`, `/.accrete/data/`,
`/.accrete/bin/` and `/scratch/`.

Other files the scaffold keeps, such as `.accrete/bin/`, guidance under `.accrete/docs/` or
its install inventory, are invisible to the Kernel. They only need to stay clear of the Kernel's own
paths in the table above. Upgrading those files is the generator's job, and installing a
new binary changes nothing in a Workspace.

## Version compatibility

`accrete-kit` declares its compatible Kernel version in its own `package.json` and embeds
that release's binaries. A scaffold can read
`accrete --version` (`accrete 0.1.0`) and `accrete interface --json`, which reports
`cli_version` and `project_formats` among its integration conventions.
