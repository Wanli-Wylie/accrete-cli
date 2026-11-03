# Configuration and project views

## Selection and precedence

The Kernel selects the project from `--project PATH`, then `ACCRETE_PROJECT`,
then the closest ancestor containing `.accrete/project.toml`. An explicit
selection must name a project root. There is no global project registry.

Configuration merges in this order: distribution defaults, shared
`.accrete/project.toml`, ignored `.accrete/local.toml`, and supported invocation
overrides. Tables merge by key, arrays and scalar values replace. `project_id`
and `format` belong to the shared marker and cannot be changed locally.
Credentials stay in native Git/SSH/rclone configuration.

```toml
# .accrete/project.toml
format = 1
project_id = "bd5e4908-c82f-447e-ad54-be4938657b1f"
name = "scientific-project"
timezone = "Asia/Shanghai"

[paths]
metadata = ".accrete/objects"
results = "results"
memory = "memory"
tool_sources = "tools"
scratch = "scratch"

[locations.working]
kind = "filesystem"
purpose = "analysis replicas"

[locations.archive]
kind = "rclone"
purpose = "retained products"

[environments.solver]
description = "Project-maintained solver environment"
instructions = "tools/solver/references/environment.md"
```

The UUID is illustrative. `accrete init` creates an independent identity;
cloning preserves it. The current project format is 1. Unknown configuration
sections are retained and available in `config resolve`.

```toml
# .accrete/local.toml
host = "gpu-a"

[paths]
scratch = "/scratch/project-a/work"

[local_state]
base = "/local/project-a/accrete"

[models.target]
checkout = "/home/project-a/target"

[tools.diagnostics]
source = "/home/project-a/diagnostics"

[locations.working]
root = "/scratch/project-a/data"

[locations.archive]
remote = "project-archive"
prefix = "project-a"

[native]
rclone = "/usr/local/bin/rclone"

[environments.solver]
python = "/home/project-a/env/bin/python"
```

Model and Tool bindings prefer their stable ID, then name/declared aliases.
`model bind` and `tool bind` write ID-based local bindings. They preserve unknown
TOML values; reserialization can normalize formatting/comments. The environment
section is descriptive data for project Tools. The Kernel never activates it.

## Path rules

Shared and local configured relative paths resolve against the project root.
Absolute paths are supported. Existing managed roots are canonicalized through
the filesystem, including symlinks. The resolver creates a canonical identity
for roots that do not yet exist using their nearest existing ancestors.

| Argument/location | Relative path base |
|---|---|
| Project marker and configured `[paths]`, local bindings, local-state base | Selected project root |
| `--project`, `init DIRECTORY`, template `--from` | Invoking working directory |
| JSON/text input files (`--from`, `--text-file`, `--links-from`) | Invoking working directory; `-` reads stdin |
| Model checkout/worktree, Tool source/export/exposure destination, Data `--path`, source capture `--to` | Selected project root |
| Metadata `export --to` and `import --from` | Invoking working directory |
| Data location path with a named filesystem root | That configured root; otherwise project root |
| Data materialize `--to` | A configured location name, not an arbitrary path |

Paths are passed as distinct native argv arguments. Spaces are ordinary path
characters. This release does not expand `~`, `${NAME}`, shell substitutions,
or environment activation code in TOML; use explicit absolute paths as needed.

## Host and state

Host label precedence is `--host`, `ACCRETE_HOST`, local/shared `host`, then
the current OS hostname. A hostname is an observation label. It does not select
another configuration profile or imply that a remote machine is reachable.

State-base precedence is `--state-dir`, `ACCRETE_STATE_DIR`,
`[local_state].base`, `$XDG_STATE_HOME/accrete`, then
`$HOME/.local/state/accrete`. Explicit invocation overrides apply to that
invocation; use the same setting for subsequent writers or persist it locally.
The initializer honors these overrides too.

The selected base contains one subdirectory per SHA-256 metadata-view key:

```text
STATE_BASE/VIEW_KEY/
  mutation.lock
  view.json
  index.sqlite
  operations/
  receipts/
  exposures/
  migrations/
```

The key hashes the project ID and ordered canonical object/Result/Memory
roots. CWD and a Model checkout are absent from the key. Invocations from
different working directories therefore use the same lock when targeting the
same roots and state base. Separate metadata worktrees have separate indexes.
`view.json` prevents reuse of state already bound to another view.

The object, Result and Memory roots must not overlap each other, the project
root, Scratch, or local state. Two independent configurations that intentionally
share managed roots must also share their complete view and state binding and
have one metadata-writing host. An undeclared overlap in an unrelated project
cannot be discovered without an inventory; it is outside this local concurrency
contract. Avoid simultaneous native edits/Git structural changes and Kernel
writes to the same managed files.

SQLite and operation receipts need host-local storage. An NFS-mounted project
can hold portable documents and scientific payloads, while local state lives
on the selected host. Several compute nodes can use distinct project clones
and exchange metadata explicitly. A single SQLite file is not a multi-host
coordination mechanism.

`accrete config resolve --json` reports the resolved roots, host, view key,
state directory and exact index path. It does not query execution processes,
storage services or the scientific environment.

