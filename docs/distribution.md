# Build, installation and distribution

ACCRETE 0.1.0 is a synchronous native Rust executable, with no daemon, installation script,
runtime downloader, or self-updater. It needs no Node.js to run, and it is not published
to npm. `.github/workflows/release.yml` attaches native archives to a GitHub Release from a
version tag. The `accrete-kit` npm package, built from `accrete-template`, embeds those
binaries and installs one at `.accrete/bin/accrete` in each Workspace.

## Build from source

Install Rust 1.90.0 and a C compiler (bundled SQLite is compiled from source).
The checked-in toolchain pin selects Rust 1.90.0; Cargo.lock fixes the dependency
graph. From the repository root:

```sh
cargo build --release --locked
./target/release/accrete --help
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

| Machine | Rust target | Archive |
|---|---|---|
| Linux x64 | `x86_64-unknown-linux-musl` | `accrete-<version>-x86_64-unknown-linux-musl.tar.gz` |
| Linux ARM64 | `aarch64-unknown-linux-musl` | `accrete-<version>-aarch64-unknown-linux-musl.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `accrete-<version>-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | `accrete-<version>-x86_64-apple-darwin.tar.gz` |

The workflows build and exercise each target on a native runner. `ci.yml` only creates
artifacts; `release.yml` also attaches them to a release. [VALIDATION.md](../VALIDATION.md)
states which targets were actually built and exercised; the matrix is not a claim of
completed runs on the other machines. Windows is outside this initial release's
distribution contract.

The packaging script also accepts corresponding Linux `*-gnu` binaries for local
development and records their glibc dependency. It must be given the actual target used
to build the binary. The public Linux release uses musl. A glibc development build
inherits the builder's libc requirements and is not presented as a portable HPC artifact.

## Construct a native archive

Node.js 20 or newer and `tar` are build-time packaging tools. Example:

```sh
node scripts/package-release.mjs \
  --binary target/x86_64-unknown-linux-musl/release/accrete \
  --target x86_64-unknown-linux-musl \
  --out dist/x86_64-unknown-linux-musl
```

Without `--out`, the destination is `dist/<target>`. It emits:

- `accrete-<version>-<target>.tar.gz`, holding one directory of the same name with `accrete`,
  `LICENSE`, `THIRD-PARTY-NOTICES`, `licenses/` and `release.json` (target, binary digest,
  ABI, SQLite mode and toolchain).
- `SHA256SUMS`, with one `<sha256>  <archive filename>` line per archive. Several targets can
  share an output directory; an archive that already exists is never overwritten.

## How users get the Kernel

The Kernel arrives with the Workspace:

```sh
npx accrete-kit@latest create my-project
cd my-project
.accrete/bin/accrete --version
```

From anywhere other than the Workspace root, such as a Model checkout or run directory, use
`<workspace>/.accrete/bin/accrete --project <workspace>`. The binary is per host and is not
committed; after a fresh clone, `npx accrete-kit@<version> install` restores it.

## Direct native installation

A compute host needs no npm, Node.js, Python environment or source build when it receives
the matching archive from the GitHub Release. Check it against the release's `SHA256SUMS`
and extract it into the Workspace:

```sh
sha256sum --check --ignore-missing SHA256SUMS
mkdir -p <workspace>/.accrete/bin
tar -xzf accrete-0.1.0-x86_64-unknown-linux-musl.tar.gz -C <workspace>/.accrete/bin --strip-components=1
<workspace>/.accrete/bin/accrete --project <workspace> --version
```

Take the version the Workspace records. Git is needed for Git operations. rclone is needed
only when an explicitly configured rclone materialization is requested. Existing SSH,
scheduler and data transfer arrangements remain native project operations.

## Workspaces and the template

This repository knows nothing about the template and vendors none of it, so building
needs neither Node nor `accrete-template`. Software installation, Workspace creation
and template upgrades (the generator), project format migration and Tool source updates
are separate actions. [workspace.md](workspace.md) states what the Kernel requires of a
Workspace.

## Owner release procedure

Pushing a tag `vX.Y.Z`, where `X.Y.Z` is Cargo's version, runs
`.github/workflows/release.yml`. It builds and tests all four targets on native runners.
Only when all succeed does its `release` job, the only one with `contents: write`, attach
the four archives and one combined `SHA256SUMS` to a GitHub Release. The release stays a
draft until every asset is present. A run on a branch builds without releasing. Publish
`accrete-kit` from `accrete-template` afterward, because it embeds these archives.

To resume a failed run, re-run its failed jobs. Successful build jobs and their artifacts
are reused, missing assets are uploaded, and assets already present are compared. An asset
with different bytes fails the run and is never replaced, so re-running all jobs after a
partial upload fails unless the rebuild is byte-identical. In that case remove the draft's
assets deliberately or release a new version.

Installing an updated binary does not change existing project files, Model revisions,
data payloads, or project instructions.

## Upstream references

The implementation follows the supplied architecture's distribution decisions.
This is the primary interface reference used for CI targets:

- [GitHub runner images](https://github.com/actions/runner-images): current native runner labels. The workflows pin `macos-15` for ARM64 and `macos-15-intel` for Intel, plus Ubuntu 24.04 x64/ARM64.

## License notices

The initial implementation uses MIT for ACCRETE's own code. This is a documented
engineering choice that the owner can change before publication. Native archives
include `THIRD-PARTY-NOTICES` and the accompanying `licenses/`
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
