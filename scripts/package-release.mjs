#!/usr/bin/env node
/** Package an already built native binary as one release archive. No compilation, downloads or publish. */
import fs from 'node:fs';
import os from 'node:os';
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
  'x86_64-unknown-linux-gnu': { os: 'linux', cpu: 'x64', abi: 'glibc' },
  'aarch64-unknown-linux-gnu': { os: 'linux', cpu: 'arm64', abi: 'glibc' },
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
const version = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8').match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (!version) throw new Error('Cargo.toml declares no release version.');
const licensePath = path.join(root, 'LICENSE');
if (!fs.existsSync(licensePath)) throw new Error('A root LICENSE is required before packaging.');
const noticesPath = path.join(root, 'THIRD-PARTY-NOTICES');
if (!fs.existsSync(noticesPath)) throw new Error('Generate THIRD-PARTY-NOTICES with scripts/third-party-notices.mjs before packaging.');
const sha256 = filename => crypto.createHash('sha256').update(fs.readFileSync(filename)).digest('hex');
const nativeName = `accrete-${version}-${options.target}`;
const archiveName = `${nativeName}.tar.gz`;
const archivePath = path.join(out, archiveName);
if (fs.existsSync(archivePath)) throw new Error(`Refusing to overwrite existing archive: ${archivePath}. Choose a fresh --out path.`);
fs.mkdirSync(out, { recursive: true });

const metadata = {
  version, target: options.target, os: platform.os, cpu: platform.cpu, abi: platform.abi,
  binary_sha256: sha256(binary), project_format: { min: 1, max: 1 },
  sqlite: 'bundled', cpu_baseline: 'Rust target default; no target-cpu=native',
  toolchain: fs.existsSync(path.join(root, 'rust-toolchain.toml')) ? fs.readFileSync(path.join(root, 'rust-toolchain.toml'), 'utf8').match(/channel\s*=\s*"([^"]+)"/)?.[1] || 'unspecified' : 'unspecified',
};
const staging = fs.mkdtempSync(path.join(os.tmpdir(), 'accrete-release-'));
const nativeDir = path.join(staging, nativeName);
fs.mkdirSync(nativeDir);
fs.copyFileSync(binary, path.join(nativeDir, 'accrete'));
fs.chmodSync(path.join(nativeDir, 'accrete'), 0o755);
fs.copyFileSync(licensePath, path.join(nativeDir, 'LICENSE'));
fs.copyFileSync(noticesPath, path.join(nativeDir, 'THIRD-PARTY-NOTICES'));
fs.cpSync(path.join(root, 'licenses'), path.join(nativeDir, 'licenses'), { recursive: true });
fs.writeFileSync(path.join(nativeDir, 'release.json'), JSON.stringify(metadata, null, 2) + '\n');
// COPYFILE_DISABLE keeps macOS tar from adding AppleDouble ._* entries.
execFileSync('tar', ['-czf', archivePath, '-C', staging, nativeName], { stdio: 'inherit', env: { ...process.env, COPYFILE_DISABLE: '1' } });
fs.rmSync(staging, { recursive: true });

// One line per archive, so several targets can share an output directory.
const sumsPath = path.join(out, 'SHA256SUMS');
const digest = sha256(archivePath);
const others = fs.existsSync(sumsPath) ? fs.readFileSync(sumsPath, 'utf8').split('\n').filter(line => line && !line.endsWith(`  ${archiveName}`)) : [];
fs.writeFileSync(sumsPath, [...others, `${digest}  ${archiveName}`].sort((a, b) => a.slice(66).localeCompare(b.slice(66))).join('\n') + '\n');
console.log(JSON.stringify({ archive: archivePath, sha256: digest, checksums: sumsPath, ...metadata }, null, 2));
