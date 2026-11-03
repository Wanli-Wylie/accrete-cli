# create-accrete

A thin initializer for the ACCRETE native kernel. After publication:

```sh
npm create accrete@0.1.0 -- my-project
```

Arguments are forwarded to `accrete init`. The native kernel owns all project
files and persistent operations. The initializer always depends on the exact
matching `@accrete/cli` release.
