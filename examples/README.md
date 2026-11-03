# Object input examples

These are creation inputs, so they omit the generated `format`, `kind`,
`project_id`, and `created_at` envelope. Fixed example IDs make repeated example
submissions easy to reconcile. Adapt paths, identities, and observations to the
work being recorded.

```sh
accrete model register --from /path/to/accrete-cli/examples/model.json
accrete tool admit --from /path/to/accrete-cli/examples/tool.json
accrete data register --from /path/to/accrete-cli/examples/data.json
accrete result record --from /path/to/accrete-cli/examples/result.json
accrete memory append --from /path/to/accrete-cli/examples/memory.md \
  --id example-memory --result example-result
```

The Model example supplies a repository hint without cloning or binding it.
Use `model bind example-model --checkout PATH` for the real local checkout.
The Tool example supplies a live source path; create or bind that capability
source before requesting an exposure. The Data example is an external reference;
it intentionally performs no network transfer. Its descriptor does not promise
that this illustrative locator is reachable.

The Result leaves execution revisions and measurements unspecified, under the
free-form `execution` and `observations` fields. Supply the values from the actual
activity when known. The Markdown is an ordinary draft;
`memory append` adds the managed frontmatter and date-organized path.
