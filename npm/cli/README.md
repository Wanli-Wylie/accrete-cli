# ACCRETE CLI

This package launches the matching, registry-contained native ACCRETE executable.
It does not run installation scripts, compile Rust, download executables at startup,
or implement object operations in JavaScript.

After the owner publishes the packages:

```sh
npm install --global @accrete/cli@0.1.0 --include=optional
accrete --help
accrete init my-project
```

Linux x64/arm64 and macOS x64/arm64 payloads are selected by npm. Keep optional
dependencies enabled and install for the destination machine. A native executable
can also run without Node.js. See `docs/distribution.md` in the source repository
for builds, local tarballs, offline installation and release construction.

The package names are implementation defaults; this repository does not claim
that an npm scope is owned or that these packages have been published.
