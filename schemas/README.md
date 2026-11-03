# Stable JSON conventions

- `document.schema.json` describes the common format-1 persistent envelope,
  including Memory frontmatter after parsing.
- `operation-input.schema.json` describes an open creation-input object before
  the Kernel adds its envelope.
- `template-manifest.schema.json` describes `accrete-template.json`, the explicit
  integration boundary with the independent template repository.
- `metadata-bundle.schema.json` describes portable metadata export/import.

The schemas use JSON Schema draft 2020-12. Project extension fields remain open.
They document structures rather than replacing operation-specific checks in the
Rust Kernel: source availability, reserved paths, identity conflicts, local
ownership, compatible version bounds, and expected byte revisions are enforced
when an operation runs.

Memory frontmatter is YAML. Ordinary numbers are emitted as ordinary YAML numeric
scalars. If a value would lose numeric precision through the YAML number model,
the frontmatter uses JSON object syntax, which is valid YAML, and ACCRETE reads it
with its arbitrary-precision JSON parser. Its entire Markdown body is preserved.
