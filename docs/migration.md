# Explicit migration from the original scaffold

`accrete migrate --from LEGACY` imports the small metadata produced by the supplied Python
scaffold. `accrete migrate` without `--from` handles persisted format changes; format 1 is the
only one in existence, so it reports that the project is current and changes nothing. It preserves the existing computational content, source trees,
archives, original Results, and dated prose. Source files are never executed to
infer their behavior or placement.

The destination Workspace must exist first. For an in-place adoption:

```sh
npx accrete-kit@latest create .
.accrete/bin/accrete migrate --from . --dry-run
.accrete/bin/accrete migrate --from .
```

Or point an existing Workspace at a separate legacy tree:

```sh
accrete --project /work/new-project migrate --from /work/legacy-project --dry-run
accrete --project /work/new-project migrate --from /work/legacy-project
```

The dry run reports intended object paths, stable-identity mappings for the
preview, legacy empty reservations, and recognized script call sites. It does
not change managed metadata or create a migration backup. Opening the local
metadata lock can establish the ordinary local-state binding. IDs from a dry
run are provisional until the applied plan is saved.

## Recognized input layout

| Input | Conversion |
|---|---|
| `data/index.tsv` | One Data descriptor per distinct key, preserving key aliases, origin text, size, digest and the original row |
| `results/**/*.json` | Format-1 Result with the existing ID, filename, unknown values and historical fields retained |
| `memory/**/*.md` | Complete entries gain stable frontmatter IDs at their existing relative paths; bodies and known frontmatter fields survive |
| Empty Memory files | Remain untouched and are reported as legacy reservations |
| `.gitmodules` | Model descriptors for declared submodules; native Git parses the file, without fetching or changing checkouts |
| `tools/**/SKILL.md` | Live Tool descriptors pointing to the existing source; host exposure remains an explicit later action |
| `AGENTS.md`, `CLAUDE.md`, recognized files under `tools/` and `meta/` | Known legacy script references are reported for deliberate editing |

This is the supplied scaffold's conventional layout. For customized roots,
provide a staging view containing those small metadata paths or edit an
explicit copy into that layout before migration. The command does not infer
arbitrary Python path logic. It does not scan a model repository or payload tree
for potential metadata.

The legacy TSV header is exactly `key`, `origin`, `bytes`, `sha256`, separated by
tabs. Identical duplicate rows collapse to one imported key. Incompatible rows
for the same key produce an explicit conflict. Equal payload digests under
different keys retain different Data IDs.

Legacy Data locations are borrowed paths with unknown current availability.
Migration does not read or hash the payload, even when it is present. A `.tar.gz`
remains the original archive with the original digest scope. Origin text is
preserved verbatim and is not assumed to be a downloadable locator. Add usable
current locations deliberately afterward.

Historical Result IDs remain unchanged. If the ID differs from the filename,
the conversion stops and names the conflict rather than selecting one silently.
Absent IDs use the filename stem. Existing reserved envelope names must either
match their new role or be explicitly resolved in the source before migration.
All other nested JSON values—including integers larger than 64 bits—survive.
Current Git HEAD, current environments, and the migration clock do not become
historical execution observations.

Where a Result's legacy operands unambiguously match Data keys, submodule names,
or Tool source paths in this migration, the converter adds corresponding IDs.
Original operands remain present. Missing mappings remain unresolved; no fetch
or checkout is needed just to retain the association.

A legacy Memory day is preserved as a path. No UTC authored instant is invented
from that filename. Existing YAML frontmatter is decoded and re-emitted with
its values retained; the Markdown body remains byte-for-byte identical in its
UTF-8 representation. Empty reservations are not admitted as authored entries.

## Restartability and retained originals

The applied migration saves a plan containing assigned IDs, source fingerprints,
intended paths, and exact intended output text under the host-local state
`migrations/legacy-.../plan.json`. It saves exact original metadata bytes under
that directory's `backup/` before publishing any converted object. No scientific
payload, environment, or Model history is copied.

Run the same command after interruption. The saved identity mapping is reused.
An already matching destination is a no-op; a missing destination is completed.
An in-place replacement is allowed only while its bytes still match the
fingerprinted predecessor. A destination changed independently is preserved
and reported as a conflict. The plan reports which individual documents were
completed; the whole migration is not described as one filesystem transaction.

Only Kernel metadata writers use this local lock. Keep legacy scripts, editors,
and Git structural operations quiet on these metadata roots during conversion.
Numerical programs writing their own separate output files need not stop.

After all documents are published, the converter writes a portable identity/path
map to `.accrete/migrations/MIGRATION_ID.json` and refreshes the host-local SQLite
projection. A failed index refresh leaves the converted documents committed and
reports `index_pending`. Run `accrete index refresh` after fixing its cause.
Model and external Tool bindings are written separately to local configuration;
the response states their outcomes.

A completed saved migration remains a description of that selected conversion.
If legacy scripts add new records afterward, migrate those through an explicitly
selected separate source view or portable import. Do not keep the Python
writers and Kernel writes active against the same migrated roots.

## Updating callers and guidance

No project script or owner instruction is automatically rewritten. The response
lists recognized calls to `record.py`, `memory.py`, `index.py`, and `data.py` so
an Agent can adapt the actual callers:

- Full Result JSON uses `result record` or revision-checked `result amend`.
- Memory submits complete authored Markdown through `memory append`.
- Data registration leaves bytes in place; materialization is explicit.
- `index refresh` builds the projection; ordinary SQLite reads query it.

`npx accrete-kit@latest create` installs foundational guidance, separately from metadata conversion.
Existing root instructions keep their content and gain a managed block; other differing
files are preserved with a candidate. A migration does not overwrite `AGENTS.md`, `CLAUDE.md`, scientific code, Tool
sources, or existing Skill exposure directories.
