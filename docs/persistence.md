# Persistence, revisions, and portable exchange

This implementation follows design documents 03–06: a complete JSON or Markdown file is the persistent object. SQLite is a host-local query projection of those files. Git transports metadata by the project's own conventions. Scientific payloads are never scanned to rebuild this projection.

## Object documents

The managed paths, relative to their configurable roots, are:

| Kind | Default path |
| --- | --- |
| Model | `.accrete/objects/models/ID.json` |
| Iteration | `.accrete/objects/iterations/ID.json` |
| Tool | `.accrete/objects/tools/ID.json` |
| Data | `.accrete/objects/data/ID.json` |
| Result | `results/ID.json` |
| Memory | `memory/YYYY/MM/DD/NNN.md` |

JSON documents reserve `format`, `kind`, `id`, `project_id`, `created_at`, and the Kernel-managed `updated_at`. Format 1 is the current format. The first five fields cannot change through an amendment. `created_at` is the object's publication time, distinct from a Result's explicitly supplied execution timestamps. Unknown JSON fields are preserved. Arbitrarily large JSON numbers are preserved through parsing and serialization; downstream SQLite numeric operations still have SQLite's own numeric semantics.

New IDs are random UUIDs unless a caller supplies one. Legacy identifiers remain accepted when they use 1–180 ASCII letters, digits, dots, underscores, or hyphens; `.` and `..` are excluded. This restriction makes a supplied identifier safe as a filename. A durable ID takes priority over a matching name or alias. Ambiguous aliases produce a conflict. Different objects cannot claim the same ID, including across different object families.

References may be unresolved. A Result can identify a remote Data object or an unavailable Model commit without fabricating local content. The projection deliberately does not require that every relationship target be locally present.

## Publication and concurrency

Each invocation resolves the project ID and canonical managed roots. Their deterministic view key partitions the configured local-state base. One `mutation.lock` serializes short managed metadata writes to that view. `view.json` records the binding, and the SQLite index also records its view key. Another branch checkout has another view because its managed roots differ. No global instance registry is involved.

Publication writes a complete temporary sibling, flushes it, and creates or replaces the destination atomically. New identity creation uses no-clobber publication. On Unix, newly created parent directories and the final containing directory are synchronized as well. The initial supported durability contract is Linux/macOS on a local filesystem that provides their normal rename/link and directory-fsync semantics. Network and multi-host writing semantics are not added by the local lock.

The lock coordinates Kernel writers. Native editors and Git do not acquire it. Coordinate branch switches, merges, and direct managed-file edits with writers in that checkout, then run `accrete index refresh`. Two independently configured state bases pointing at overlapping documents do not provide one shared writer lock. Configuration should designate one local metadata writer view for shared physical roots.

Operations that copy Data or create Git resources keep the long native action outside this metadata lock. Their own receipts bridge the external action and subsequent descriptor publication. The files, Git repository, and SQLite index are separate completion boundaries.

### Mutation response

```json
{
  "object_id": "the-stable-id",
  "path": "/absolute/path/to/document.json",
  "revision": "sha256-of-the-actual-file-bytes",
  "committed": true,
  "published": true,
  "durability": "durable",
  "index_pending": false,
  "unchanged": false
}
```

The `revision` is lowercase SHA-256 without a prefix. It describes the actual file bytes, including formatting and trailing newline. Native tools can compute it directly. An amendment using `--expect REVISION` rechecks those bytes under the mutation lock. A mismatch leaves the existing file and the caller's draft unchanged and returns a conflict.

Amendments use JSON Merge Patch behavior: an object merges by key, arrays replace, and `null` removes a key. Reserved envelope fields cannot be removed or altered. Append-style observations operate under the lock and can use caller-generated operation IDs to make a repeated append a no-op; the same operation ID with different content conflicts.

A create with the same supplied ID and same payload returns the existing object. Automatically generated envelope values are reconciled with that object. A different payload requires an explicit amendment. Retry behavior is tied to the object, so callers needing it should supply stable IDs before their first call. A retry also attempts to refresh the disposable projection.

| Completion state | Response |
| --- | --- |
| Input or expected revision rejected | Error; primary file unchanged |
| Temporary file write fails | Error; primary file unchanged |
| Primary publication succeeds, projection fails | Exit 0, `committed: true`, `index_pending: true`; refresh the index |
| Destination visible, final directory sync fails | `published: true`, `durability: "uncertain"`, exit 8; keep the ID and receipt |
| All publication steps succeed | Exit 0 with stable ID, path, and revision |

Durability receipts are short-lived local files under `receipts/`. Recovery checks the named destination against its recorded byte revision, synchronizes that file and its containing directory, and removes the completed receipt. A changed document produces a conflict for explicit reconciliation. Recovery never reruns scientific computation.

## Memory

Memory is UTF-8 Markdown with a YAML frontmatter envelope:

```yaml
---
format: 1
kind: memory
id: 625c3dba-f352-477d-bae1-38cf166e9302
project_id: the-project-id
authored_at: 2026-11-03T12:00:00.000Z
links:
  - kind: result
    id: a-result-id
    relation: interprets
---
# What we learned

The complete authored content follows without rewriting its prose.
```

The publication day comes from the configured IANA project timezone; the filename uses the next available local number on that day. Numbering and complete-content publication happen under the same lock. No empty permanent reservation is created. New entries reject an empty body.

Numeric scalars such as `format: 1` are emitted as ordinary YAML numbers. If extension metadata contains a number that cannot be represented exactly by the YAML library, the complete frontmatter uses JSON object syntax, which is valid YAML. ACCRETE reads that form with its arbitrary-precision JSON parser. No serializer-private number representation is written to the file.

The frontmatter ID is independent of the filename. Portable import retains original metadata and body. If the incoming relative Memory filename already exists, import allocates the next number in that directory and adds `origin_path`. Repeating the same import reconciles the added provenance and returns the existing ID. The Kernel does not rewrite handwritten Markdown links.

Historical conversion can publish an envelope over an exact legacy predecessor at its existing date path, preserving the original prose. A historical entry without a known authored timestamp remains without one. Ordinary unmarked Markdown files are left out of the Memory index. Malformed managed frontmatter or unsupported managed formats make a refresh fail rather than silently dropping an entry.

## SQLite projection

The index lives at the resolved host-local `index.sqlite` path; `accrete config resolve --json` reports its location. It uses bundled SQLite, the rollback journal, a five-second busy timeout, and short write transactions. SQLite connections close when a CLI invocation exits, so between invocations the index is one closed file that `sqlite3 -readonly` can open. A reader can briefly get "database is locked" while a refresh commits; retry the read. Do not share this database between machines.

A refresh reads a consistent Kernel-locked metadata snapshot, checks formats and unique IDs, then updates the existing database in one transaction. The previous complete projection remains available if snapshot parsing or the transaction fails. The implementation does not rename an open database file. The initial refresh limit is 100,000 managed documents across the view; it scans metadata only, independent of Data payload size.

| Table | Relevant columns |
| --- | --- |
| `models`, `tools`, `data_objects`, `results` | `id`, `name`, `project_id`, `status`, `path`, `revision`, `document` |
| `iterations` | Common object columns plus `model_id`, `starting_revision` |
| `data_locations` | `data_id`, `ordinal`, `location_id`, `name`, `kind`, `host`, `path`, `managed`, `availability`, `document` |
| `result_models` | `result_id`, `ordinal`, `model_id`, `role`, `commit_id`, `document` |
| `result_tools` | `result_id`, `ordinal`, `tool_id`, `revision_used`, `document` |
| `result_data` | `result_id`, `ordinal`, `data_id`, `role`, `document` |
| `memory_entries` | `id`, `name`, `project_id`, `path`, `revision`, `document`, `authored_at`, `body` |
| `memory_links` | `memory_id`, `ordinal`, `target_id`, `kind`, `relation`, `document` |
| `index_state` | `singleton`, `format`, `view_key`, `project_id`, `refreshed_at`, `object_count` |

`document` contains conventional JSON text. Result associations accept arrays of ID strings or objects. Model objects conventionally use `model_id` or `id`, `role`, and `commit`; Tool objects use `tool_id` or `id` and an explicitly supplied `revision`; Data associations use `inputs` and `outputs` with `data_id` or `id`. Role-keyed legacy maps remain readable where their meaning maps directly. The complete original values remain in the object JSON even when they do not produce a relationship row.

Native readers can use the index directly:

```sql
SELECT view_key, refreshed_at, object_count FROM index_state;

SELECT r.id, r.name, m.model_id, m.commit_id
FROM results r JOIN result_models m ON m.result_id = r.id;

SELECT result_id, role FROM result_data WHERE data_id = 'a-data-id';

SELECT id, json_extract(document, '$.measurements.runtime_seconds')
FROM results;
```

A Git merge or direct file edit can make the index stale. Its `refreshed_at` and indexed per-file revisions describe the last successful projection. Deleting the closed index and running `index refresh` rebuilds it. For a live snapshot, use SQLite's backup API or `VACUUM INTO`; copying the file while a refresh may be committing is not a defined snapshot operation.

## Portable exchange

`accrete export --to PATH` writes a new format-1 JSON metadata bundle. Repeated `--id ID` selects a subset; omitting IDs exports all managed documents. The bundle includes exact UTF-8 file content, byte revision, kind, ID, and the Memory-relative source path. Model repositories, Tool source trees, Data payloads, local bindings, recovery receipts, and SQLite files are outside this metadata bundle.

`accrete import --from PATH` imports a bundle object by object. The enclosing project ID and each document project ID must match unless `--allow-foreign` explicitly permits cross-project import. Cross-project import preserves origin project IDs and object IDs; it does not silently claim that the receiving project originally authored them. Duplicate stable IDs remain conflicts.

Each entry reports its own result. A partially completed batch has `atomic: false` and a nonzero `failed` count; successful publications remain committed. Retrying stable entries finishes missing objects and acknowledges identical ones. An incoming ID with different content is retained in the original bundle and reported as a conflict. To intentionally replace a predecessor, add the current local byte revision to that bundle entry's `expected_revision`. Identity and envelope fields remain immutable. No last-writer-wins merge is performed.

Export preserves exact JSON bytes. Memory import preserves metadata and body semantically, with generated provenance only for filename collisions. Its frontmatter may be reserialized. The original `authored_at`, links, stable ID, and entire Markdown body survive.
