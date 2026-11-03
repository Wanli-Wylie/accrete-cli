#!/usr/bin/env node
/** Package an already built native binary. No compilation, downloads or publish. */
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const options = {};
for (let i = 0; i < args.length; i += 2) {
  if (!['--binary', '--target', '--out'].includes(args[i]) || !args[i + 1]) {
    console.error('Usage: node scripts/package-release.mjs --binary PATH --target RUST_TARGET [--out DIR]');
    process.exit(2);
  }
  options[args[i].slice(2)] = args[i + 1];
}
if (!options.binary || !options.target) {
  console.error('--binary and --target are required; the target must describe the actual binary.');
  process.exit(2);
}
const targets = {
  'x86_64-unknown-linux-musl': { os: 'linux', cpu: 'x64', abi: 'static-musl' },
  'aarch64-unknown-linux-musl': { os: 'linux', cpu: 'arm64', abi: 'static-musl' },
  'x86_64-unknown-linux-gnu': { os: 'linux', cpu: 'x64', abi: 'glibc', libc: ['glibc'] },
  'aarch64-unknown-linux-gnu': { os: 'linux', cpu: 'arm64', abi: 'glibc', libc: ['glibc'] },
  'x86_64-apple-darwin': { os: 'darwin', cpu: 'x64', abi: 'darwin' },
  'aarch64-apple-darwin': { os: 'darwin', cpu: 'arm64', abi: 'darwin' },
};
const platform = targets[options.target];
if (!platform) throw new Error(`Unsupported release target: ${options.target}`);
const binary = fs.realpathSync(options.binary);
if (!fs.statSync(binary).isFile()) throw new Error(`Binary is not a regular file: ${binary}`);
const nativeBytes = fs.readFileSync(binary);
if (platform.os === 'linux') {
  if (nativeBytes.length < 64 || nativeBytes.subarray(0, 4).toString('hex') !== '7f454c46' || nativeBytes[4] !== 2 || nativeBytes[5] !== 1) {
    throw new Error('Linux payload must be a little-endian 64-bit ELF executable.');
  }
  const expectedMachine = platform.cpu === 'x64' ? 62 : 183;
  if (nativeBytes.readUInt16LE(18) !== expectedMachine || nativeBytes.readBigUInt64LE(24) === 0n) {
    throw new Error('ELF executable does not match the declared CPU target.');
  }
  const offset = Number(nativeBytes.readBigUInt64LE(32));
  const size = nativeBytes.readUInt16LE(54);
  const count = nativeBytes.readUInt16LE(56);
  if (size < 56 || offset + size * count > nativeBytes.length) throw new Error('Invalid ELF program headers.');
  for (let i = 0; i < count; i++) {
    if (platform.abi === 'static-musl' && nativeBytes.readUInt32LE(offset + i * size) === 3) {
      throw new Error('The musl release must be static; this binary contains a dynamic interpreter. Rebuild with the default Rust linker, using musl-gcc only as CC for bundled SQLite.');
    }
  }
} else {
  const expectedCpu = platform.cpu === 'x64' ? 0x01000007 : 0x0100000c;
  if (nativeBytes.length < 32 || nativeBytes.readUInt32LE(0) !== 0xfeedfacf || nativeBytes.readUInt32LE(4) !== expectedCpu || nativeBytes.readUInt32LE(12) !== 2) {
    throw new Error('macOS payload must be a Mach-O executable matching the declared CPU target.');
  }
}

const out = path.resolve(options.out || path.join(root, 'dist', options.target));
fs.mkdirSync(out, { recursive: true });
const cliSource = path.join(root, 'npm', 'cli');
const cliPackage = JSON.parse(fs.readFileSync(path.join(cliSource, 'package.json'), 'utf8'));
const initializerPackage = JSON.parse(fs.readFileSync(path.join(root, 'npm', 'create-accrete', 'package.json'), 'utf8'));
const version = cliPackage.version;
const cargoVersion = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8').match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (cargoVersion !== version || initializerPackage.version !== version || initializerPackage.dependencies[cliPackage.name] !== version) {
  throw new Error('Cargo, launcher, initializer and dependency release versions must match exactly.');
}
for (const value of Object.values(cliPackage.optionalDependencies)) {
  if (value !== version) throw new Error('Native optional dependencies must use the exact release version.');
}
const licensePath = path.join(root, 'LICENSE');
if (!fs.existsSync(licensePath)) throw new Error('A root LICENSE is required before packaging.');
const noticesPath = path.join(root, 'THIRD-PARTY-NOTICES');
if (!fs.existsSync(noticesPath)) throw new Error('Generate THIRD-PARTY-NOTICES with scripts/third-party-notices.mjs before packaging.');
function copyNotices(destination) {
  fs.copyFileSync(noticesPath, path.join(destination, 'THIRD-PARTY-NOTICES'));
  fs.cpSync(path.join(root, 'licenses'), path.join(destination, 'licenses'), { recursive: true });
}
const sha256 = filename => crypto.createHash('sha256').update(fs.readFileSync(filename)).digest('hex');
const nativeDigest = sha256(binary);
const nativeName = `accrete-${version}-${options.target}`;
const nativeDir = path.join(out, 'native', nativeName);
const payloadName = `@accrete/kernel-${platform.os}-${platform.cpu}`;
const payloadDir = path.join(out, 'packages', payloadName.split('/')[1]);
for (const directory of [nativeDir, payloadDir]) {
  if (fs.existsSync(directory)) throw new Error(`Refusing to overwrite existing release directory: ${directory}. Choose a fresh --out path.`);
  fs.mkdirSync(directory, { recursive: true });
}
const metadata = {
  version, target: options.target, os: platform.os, cpu: platform.cpu, abi: platform.abi,
  binary_sha256: nativeDigest, project_format: { min: 1, max: 1 },
  sqlite: 'bundled', cpu_baseline: 'Rust target default; no target-cpu=native',
  toolchain: fs.existsSync(path.join(root, 'rust-toolchain.toml')) ? fs.readFileSync(path.join(root, 'rust-toolchain.toml'), 'utf8').match(/channel\s*=\s*"([^"]+)"/)?.[1] || 'unspecified' : 'unspecified',
  template_integration: 'external manifest; see docs/template-integration.md',
};
fs.copyFileSync(binary, path.join(nativeDir, 'accrete'));
fs.chmodSync(path.join(nativeDir, 'accrete'), 0o755);
fs.copyFileSync(licensePath, path.join(nativeDir, 'LICENSE'));
copyNotices(nativeDir);
fs.writeFileSync(path.join(nativeDir, 'release.json'), JSON.stringify(metadata, null, 2) + '\n');
fs.mkdirSync(path.join(payloadDir, 'bin'));
fs.copyFileSync(binary, path.join(payloadDir, 'bin', 'accrete'));
fs.chmodSync(path.join(payloadDir, 'bin', 'accrete'), 0o755);
fs.copyFileSync(licensePath, path.join(payloadDir, 'LICENSE'));
copyNotices(payloadDir);
fs.writeFileSync(path.join(payloadDir, 'release.json'), JSON.stringify(metadata, null, 2) + '\n');
const payload = {
  name: payloadName, version, description: `ACCRETE native kernel for ${options.target}`,
  license: cliPackage.license, os: [platform.os], cpu: [platform.cpu],
  ...(platform.libc ? { libc: platform.libc } : {}),
  files: ['bin/accrete', 'release.json', 'LICENSE', 'THIRD-PARTY-NOTICES', 'licenses', 'README.md'],
};
fs.writeFileSync(path.join(payloadDir, 'package.json'), JSON.stringify(payload, null, 2) + '\n');
fs.writeFileSync(path.join(payloadDir, 'README.md'), `# ${payloadName}\n\nNative executable for ${options.target}. Installed by @accrete/cli@${version}.\nThe same executable can run directly without Node.js. See release.json for build assumptions.\n`);
const packageDirs = [payloadDir];
for (const name of ['cli', 'create-accrete']) {
  const destination = path.join(out, 'packages', name);
  if (fs.existsSync(destination)) throw new Error(`Release directory already exists: ${destination}`);
  fs.cpSync(path.join(root, 'npm', name), destination, { recursive: true });
  fs.copyFileSync(licensePath, path.join(destination, 'LICENSE'));
  copyNotices(destination);
  packageDirs.push(destination);
}
const tarballs = [];
for (const directory of packageDirs) {
  const packed = JSON.parse(execFileSync('npm', ['pack', '--json', '--ignore-scripts', '--pack-destination', out, directory], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }));
  tarballs.push({ name: packed[0].name, filename: packed[0].filename, sha256: sha256(path.join(out, packed[0].filename)) });
}
const archiveName = `${nativeName}.tar.gz`;
execFileSync('tar', ['-czf', path.join(out, archiveName), '-C', path.dirname(nativeDir), nativeName], { stdio: 'inherit' });
const archive = { filename: archiveName, sha256: sha256(path.join(out, archiveName)) };
const manifest = { ...metadata, packages: tarballs, native_archive: archive };
fs.writeFileSync(path.join(out, 'release-manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
fs.writeFileSync(path.join(out, 'SHA256SUMS'), [...tarballs, archive].map(x => `${x.sha256}  ${x.filename}`).join('\n') + '\n');
console.log(JSON.stringify({ output: out, manifest: path.join(out, 'release-manifest.json'), ...manifest }, null, 2));
