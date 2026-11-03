# Data, locations, and managed replicas

A Data ID names a product or collection selected for continuing work. A content
SHA-256 identifies particular file bytes. A location describes where that
material can be accessed. Different Data IDs may name equal bytes, and a Data ID
can retain several locations.

## Register in place

```sh
accrete data register --id forcing-2005 --name forcing-2005 \
  --path /scratch/climate/forcing-2005.nc --kind file --hash
accrete data register --name yearly-output \
  --path /scratch/climate/year-1 --kind directory
accrete data register --from scratch/data-description.json
```

`--hash` is explicit and supports regular files. Directory registration reads
only the directory's own metadata; it does not enumerate, package, copy, or hash
the contents. A producer can supply a manifest reference in the open JSON
content fields. Registration never claims permission to delete the source.

Relative `--path` arguments resolve from the selected project root, like other
ACCRETE paths. Absolute paths work as ordinary argv values, including spaces.
Registering a symlink through `--path` is rejected; describe an external locator
or package the intended links using a project Tool.

A file description can be supplied directly:

```json
{
  "id": "forcing-2005",
  "name": "Annual forcing",
  "content": {
    "kind": "file",
    "mode": "closed",
    "size_bytes": 4096,
    "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
  },
  "origin": {"description": "Project-maintained acquisition", "result_id": "acquisition-result"},
  "locations": [
    {
      "name": "archive",
      "kind": "rclone",
      "path": "forcing/2005.nc"
    }
  ],
  "extensions": {"calendar": "noleap"}
}
```

The hash and size above illustrate the schema; supply the actual producer values
or omit those fields. `--result ID` writes the same `origin.result_id`. `content.edition`
holds optional words describing the intended edition, such as a provider's release name. `mode` defaults to `closed`; `live` explicitly describes a
mutable source. A corrected closed product gets a new Data ID, optionally with
`replaces` naming the previous ID. Adding a replica leaves `content` unchanged.

The descriptor envelope uses `format`, `kind`, `id`, `project_id`, and
`created_at`. Data-specific kind is `content.kind` (`file`, `directory`,
`collection`, or `external`), because the envelope's `kind` is always `data`.
Unknown fields are preserved. Native scientific formats stay unchanged.

## Configure locations

Shared `.accrete/project.toml` names the purposes and transfer kinds:

```toml
[locations.working]
kind = "filesystem"
purpose = "local analysis replicas"

[locations.archive]
kind = "rclone"
purpose = "retained scientific products"
```

Host `.accrete/local.toml` supplies placement:

```toml
host = "gpu-a"

[locations.working]
root = "/scratch/project-a/materialized"

[locations.archive]
remote = "project-archive"
prefix = "project-a"

[native]
rclone = "/usr/local/bin/rclone"
```

`remote` is an existing rclone configuration name, without a colon or a path.
Authentication remains in rclone's native configuration. ACCRETE does not copy
credentials into descriptors or recovery receipts. The default `working`
location is `.accrete/data` under the project root; change it for large data.

## Add independently obtained copies

```sh
accrete data location add forcing-2005 --location archive --path forcing/2005.nc
accrete data location add forcing-2005 --from scratch/location.json
```

A location has its own `id`. The CLI generates a deterministic location ID for
an ordinary registration from the name, kind, path, host, and version fields.
Repeated identical additions are no-ops. Locations added this way are borrowed
(`managed: false`). A location can carry an observation, a backend token, or an
explicit host. A missing local file does not erase the Data object.

Filesystem locations use absolute resolved paths and a host label. rclone
locations use a logical configured name and a relative path. Neither path alone
asserts an immutable historical edition. Opaque ETags and provider versions stay
opaque; they are never treated as universal SHA-256 values.

## Materialize explicitly

```sh
accrete data materialize forcing-2005 --to working
accrete data materialize forcing-2005 --to working --source LOCATION_ID
```

Without `--source`, ACCRETE chooses an available local location on the current
host, or one unambiguous rclone source. An ambiguous source name requires its
location ID. `--expect REVISION` can ensure the descriptor has not changed before
the transfer starts. The content description is compared again before adding
the completed replica, while independent location additions remain compatible.

Each copy receives a fresh replica UUID and is placed at
`ROOT/DATA_ID/REPLICA_UUID/payload`. A sibling `ownership.json` binds that replica
to this project, Data ID, host, and path. Bytes are first written to `.staging`
inside the same exclusively created container. This keeps partial content out
of the published location and avoids overwriting another copy.

Local materialization copies ordinary files/directories and preserves Unix
permission modes. It never hard-links a mutable source. Symlinks and special
files are refused during copying; package them with the project's intended
semantics first. There is no implicit tar creation or extraction.

The external adapter invokes a configured rclone executable with separate argv
arguments (`copyto --immutable` for a file, `copy --immutable` for a directory).
It downloads into a local destination. Standard output stays reserved for the
CLI response; transfer diagnostics go to standard error. rclone supplies its
normal configured backend transport. Directory operands must expose ordinary
files/keys with the intended transfer semantics; use an explicitly prepared
archive when filesystem links matter. Uploading to a remote destination is a
native rclone operation followed by `data location add`.

The adapter was exercised with the official rclone v1.70.3 Linux x64 executable,
using an alias remote backed by a local filesystem. Both a single file with a
known SHA-256 and a nested directory were materialized through the CLI, including
paths containing spaces; the returned JSON remained clean and the copied bytes
matched. No cloud account or cloud backend was exercised. The argument semantics
are documented in rclone's [copyto manual](https://rclone.org/commands/rclone_copyto/)
and [global flags](https://rclone.org/flags/).

This release does not translate backend `version`, `version_id`, or
`s3_version_at` selectors. A source that supplies one is refused rather than
silently retrieving its current content. Use a version-specific exported
locator or retrieve that version with the provider's native tool and register
the local copy. This is the explicit first-release selector boundary.

When `content.sha256` is present, materialization checks the regular-file bytes;
when a file size is supplied, it checks the resulting size. No full-directory
hash is invented. A producer's manifest is retained as metadata but not
interpreted by the generic kernel. The JSON response states the content checks
actually performed.

A repeat materialization can return an existing present replica at the same
configured root. This observes that the managed path still exists; it does not
rehash it or promise that an independent editor has left it unchanged. Native
read access remains unrestricted.

## Evict one owned replica

```sh
accrete data evict forcing-2005 --location working
accrete data evict forcing-2005 --location REPLICA_ID
```

`--location` takes a location ID or name. A name matching several current local
locations is a conflict; select the ID instead.

Eviction checks the saved ownership marker and the concrete local path before
removing `payload`. It marks that location `evicted` and preserves the descriptor,
other locations, associations, and source bytes. Registration alone never makes
a borrowed scientific directory eligible for eviction. Data retirement changes
its status and does not remove payload bytes.

The ownership marker/container is retained after eviction as a small local
receipt of which path belonged to the replica. It is not scientific data and
can be removed through ordinary owner-directed filesystem cleanup. The kernel
has no automatic project-wide garbage collector.

## Interrupted transfers and recovery

```sh
accrete recover
```

Transfer and descriptor publication are separate completion points. A bounded
receipt under the host-local state directory records the selected source,
staging path, completed destination, content description, and current phase.
The metadata lock is not held during the copy.

If the transfer finished, recovery can finish publishing its destination and
location. If eviction removed the payload, recovery can complete the location
update. A receipt for an incomplete transfer is reported with its concrete
paths and is left for explicit retry or cleanup; recovery does not start the
transfer again. Recovery never launches scientific work. A committed descriptor
whose SQL indexing is pending is still a successful mutation; run
`accrete index refresh` for the projection.
