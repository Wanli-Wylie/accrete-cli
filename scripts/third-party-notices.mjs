#!/usr/bin/env node
/** Collect license notices from the exact resolved local Cargo packages. */
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const cargo = process.env.ACCRETE_CARGO || 'cargo';
const rustc = process.env.ACCRETE_RUSTC || 'rustc';
const metadata = JSON.parse(execFileSync(cargo, ['metadata', '--locked', '--offline', '--format-version', '1', '--manifest-path', path.join(root, 'Cargo.toml')], { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }));
const licenses = path.join(root, 'licenses');
fs.mkdirSync(licenses, { recursive: true });
const texts = new Map();
const lines = [
  'ACCRETE third-party notices',
  '==========================',
  '',
  'Generated from Cargo.lock and the locally resolved package sources.',
  'This inventory includes optional/target-specific and build dependencies;',
  'not every listed component is linked into every native target.',
  'The ACCRETE source itself is covered by the root MIT LICENSE.',
  '',
];
function addText(filename) {
  const bytes = fs.readFileSync(filename);
  const digest = crypto.createHash('sha256').update(bytes).digest('hex');
  if (!texts.has(digest)) texts.set(digest, bytes.toString('utf8'));
  return digest;
}
function licenseFiles(directory, depth = 0) {
  const found = [];
  for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
    const filename = path.join(directory, item.name);
    if (item.isFile() && /^(licen[cs]e|copying|copyright|notice|unlicense)([._-]|$)/i.test(item.name)) found.push(filename);
    if (item.isDirectory() && depth === 0 && /^(licen[cs]es?)$/i.test(item.name)) {
      for (const sub of fs.readdirSync(filename, { withFileTypes: true })) if (sub.isFile()) found.push(path.join(filename, sub.name));
    }
  }
  return found;
}
for (const pkg of metadata.packages.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))) {
  if (!pkg.source) continue;
  const directory = path.dirname(pkg.manifest_path);
  const files = licenseFiles(directory);
  if (pkg.license_file) files.push(path.join(directory, pkg.license_file));
  const selected = [...new Set(files)].sort();
  lines.push(`${pkg.name} ${pkg.version}`, `Declared license: ${pkg.license || 'see package license file'}`);
  if (pkg.repository) lines.push(`Upstream: ${pkg.repository}`);
  for (const filename of selected) lines.push(`License text: ${path.relative(directory, filename)} [${addText(filename)}]`);
  if (selected.length === 0) lines.push('No separate license-text file is included in this resolved target-specific crate; see the declared upstream license and companion crate notices.');
  lines.push('');
}
lines.push('SQLite', '------', 'Bundled SQLite is dedicated to the public domain by its authors.', 'The libsqlite3-sys Rust binding license is listed above.', 'https://www.sqlite.org/copyright.html', '');
const sysroot = execFileSync(rustc, ['--print', 'sysroot'], { encoding: 'utf8' }).trim();
const rustCopyright = path.join(sysroot, 'share', 'doc', 'rust', 'COPYRIGHT-library.html');
if (fs.existsSync(rustCopyright)) {
  fs.copyFileSync(rustCopyright, path.join(licenses, 'RUST-COPYRIGHT-library.html'));
  const rustLicenseDir = path.join(sysroot, 'share', 'doc', 'rust', 'licenses');
  if (fs.existsSync(rustLicenseDir)) fs.cpSync(rustLicenseDir, path.join(licenses, 'rust'), { recursive: true });
  lines.push('Rust standard library notices: licenses/RUST-COPYRIGHT-library.html', 'Associated license texts: licenses/rust/', '');
}
if (fs.existsSync(path.join(licenses, 'musl-COPYRIGHT'))) lines.push('musl static libc notices (Linux musl targets): licenses/musl-COPYRIGHT', '');
lines.push('Collected license texts', '=======================', '');
for (const [digest, content] of texts) lines.push(`[${digest}]`, content, '');
const output = path.join(root, 'THIRD-PARTY-NOTICES');
fs.writeFileSync(output, lines.join('\n'));
console.log(`Wrote ${output}; ${metadata.packages.length - 1} packages, ${texts.size} distinct license texts.`);
