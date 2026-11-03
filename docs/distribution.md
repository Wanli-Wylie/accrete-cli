# Build, installation and distribution

ACCRETE 0.1.0 is a synchronous native Rust executable. Node.js participates only
when selecting and launching a binary installed through npm. There is no daemon,
installer compiler, installation script, runtime downloader, or self-updater.
The repository contains complete package construction; publication is an owner
operation. `@accrete/cli`, `@accrete/kernel-*`, and `create-accrete` are naming
defaults, with no claim that their npm names have been reserved or published.

## Build from source

Install Rust 1.90.0 and a C compiler (bundled SQLite is compiled from source).
The checked-in toolchain pin selects Rust 1.90.0; Cargo.lock fixes the dependency
graph. From the repository root:

```sh
cargo build --release --locked
./target/release/accrete --help
./target/release/accrete init ./my-project
```

For a source installation on the current machine:

```sh
cargo install --path . --locked
accrete --version
```

On Linux, the distribution target is a static musl binary. Install the native
architecture's musl compiler, then:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --locked --target x86_64-unknown-linux-musl
```

On an ARM64 Linux builder use `aarch64-unknown-linux-musl`. For an external musl
compiler, set `CC_x86_64_unknown_linux_musl=/absolute/path/to/musl-gcc`.
Keep Rust's normal linker selection so the target's self-contained static runtime
is linked correctly. The package builder rejects a musl binary containing a
dynamic interpreter. This setting applies to the build, never to normal CLI use. Do not set
`target-cpu=native` when building distributable executables.

## Release targets

| Machine | Rust target | Payload name |
|---|---|---|
| Linux x64 | `x86_64-unknown-linux-musl` | `@accrete/kernel-linux-x64` |
| Linux ARM64 | `aarch64-unknown-linux-musl` | `@accrete/kernel-linux-arm64` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `@accrete/kernel-darwin-arm64` |
| macOS Intel | `x86_64-apple-darwin` | `@accrete/kernel-darwin-x64` |

The CI workflows build and exercise each target on a native runner. They create
artifacts and do not publish anything. The local delivery's validation report
states which target was actually built and exercised in this session; the matrix
is not a claim of completed runs on the other machines. Windows is outside this
initial release's distribution contract.

The package builder also accepts corresponding Linux `*-gnu` binaries for local
development and records their glibc dependency. It must be given the actual target
used to build the binary. A GNU local package and a musl release package have the
same platform package name; do not publish both variants under one version. The
public Linux release uses musl. A glibc development build inherits the builder's
libc requirements and is not presented as a portable HPC artifact.

## Construct npm tarballs and a native archive

Node.js 20 or newer, npm, and `tar` are build-time packaging tools. Example:

```sh
node scripts/package-release.mjs \
  --binary target/x86_64-unknown-linux-musl/release/accrete \
  --target x86_64-unknown-linux-musl \
  --out dist/linux-x64-0.1.0
```

The destination must be fresh. The command copies the same executable bytes into
one platform payload and one native archive, then packs the launcher and
initializer. It emits:

- `accrete-kernel-linux-x64-0.1.0.tgz`: the native payload, license and target metadata.
- `accrete-cli-0.1.0.tgz`: the small JavaScript dispatcher.
- `create-accrete-0.1.0.tgz`: the initializer that forwards to native `init`.
- `accrete-0.1.0-x86_64-unknown-linux-musl.tar.gz`: a directly runnable native release.
- `release-manifest.json`: targets, binary digest, ABI, SQLite mode and package digests.
- `SHA256SUMS`: tarball/archive SHA-256 checksums.

There is no network request in package assembly beyond npm's local `pack`
operation. No `postinstall` hook exists. All package version fields and dependency
versions are checked against Cargo's release version before packaging. The native
payload is an exact optional dependency of the launcher. The initializer has an
exact dependency on the launcher.

## Install the delivered packages without a registry

The local installer consumes the tarballs in a release directory, explicitly
selects offline mode, and disables lifecycle scripts:

```sh
node scripts/install-local.mjs dist/linux-x64-0.1.0 ./local-install
./local-install/node_modules/.bin/accrete --version
./local-install/node_modules/.bin/create-accrete ./my-project
```

It checks that the payload OS and architecture match the current Node process.
The installation prefix may be any explicitly chosen directory. The ordinary npm
package semantics and native binary remain unchanged. Once published, the public
route will be:

```sh
npm install --global @accrete/cli@0.1.0 --include=optional
accrete init ./my-project
# Or use npm's initializer convention:
npm create accrete@0.1.0 -- ./my-project
```

Do not copy `node_modules` across architectures or operating systems. An omitted
or mismatched native dependency produces an actionable launcher error and exit
status 5. There is no fallback download. Native CLI exit codes are otherwise
preserved. The wrapper forwards arguments, working directory, environment,
standard streams, and termination signals.

## Direct native installation

A compute node needs no npm, Node.js, Python environment or source build when it
receives the matching native archive:

```sh
tar -xzf accrete-0.1.0-x86_64-unknown-linux-musl.tar.gz
./accrete-0.1.0-x86_64-unknown-linux-musl/accrete --version
install -m 0755 ./accrete-0.1.0-x86_64-unknown-linux-musl/accrete /chosen/bin/accrete
```

Choose a writable destination already on PATH, or invoke the absolute binary
path. Git is needed for Git operations. rclone is needed only when an explicitly
configured rclone materialization is requested. Existing SSH, scheduler and data
transfer arrangements remain native project operations.

## Template integration

The independently developed `accrete-template` repository supplies its files and
an `accrete-template.json` manifest. The kernel initializes the project and owns
safe asset application; JavaScript never contains a second scaffold implementation:

```sh
accrete init ./my-project --template /path/to/accrete-template
accrete --project ./my-project project upgrade --from /path/to/accrete-template --dry-run
accrete --project ./my-project project upgrade --from /path/to/accrete-template
```

See [template-integration.md](template-integration.md) for the complete manifest,
compatibility and file ownership interface. This CLI repository does not include
a competing foundational Skill collection or Coordinator template. A later
release build can embed an explicitly selected template snapshot through a
coordinated integration change; the current stable seam accepts a local snapshot.
Software installation, template application, project format migration and Tool
source updates are separate actions.

## Owner release procedure

Before a public release, select an npm scope and align the two package manifests,
launcher payload naming and package-builder naming if the defaults change. Keep
the initializer, launcher, all payloads and native executable at one release
version. Build all chosen targets from the same committed source and lockfile.

Publish each platform payload first, with an explicit distribution tag such as
`next`. Publish the launcher after the complete payload set, then the initializer.
Only after the complete release is installed successfully should the owner
advance the normal distribution tag. No source build or npm command in this
repository publishes automatically. npm registry authentication and optional
trusted publishing belong to the maintainer's release environment.

Keep archive digests with the release. An interrupted publication can resume with
the same bytes; never substitute a different executable under an already released
version. Installing an updated binary does not change existing project files,
Model revisions, data payloads, or project instructions.

## Upstream references

The implementation follows the supplied architecture's distribution decisions.
These are the primary interface references used for package fields and CI targets:

- [npm package.json](https://docs.npmjs.com/cli/v11/configuring-npm/package-json/): optional dependencies, OS/CPU fields and executable mappings.
- [Node child processes](https://nodejs.org/api/child_process.html): argv-based spawning and process events.
- [GitHub runner images](https://github.com/actions/runner-images): current native runner labels. The workflows pin `macos-15` for ARM64 and `macos-15-intel` for Intel, plus Ubuntu 24.04 x64/ARM64.

## License notices

The initial implementation uses MIT for ACCRETE's own code. This is a documented
engineering choice that the owner can change before publication. Native archives
and npm packages include `THIRD-PARTY-NOTICES` and the accompanying `licenses/`
files for resolved Rust dependencies, the standard library, SQLite and Linux's
musl libc. To refresh the notices after deliberately changing dependencies, with
the locked dependencies already downloaded:

```sh
node scripts/third-party-notices.mjs
```

The generator reads local package sources and runs `cargo metadata --offline`;
it does not fetch license contents or evaluate dependencies. Optional and
platform-specific dependencies are included in the source notice inventory even
when a particular native binary does not link them. The musl notice accompanies
the statically linked Linux builds; macOS uses the platform's native system APIs.
