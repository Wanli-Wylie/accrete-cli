#!/usr/bin/env node
/** Install packaged tarballs offline. This is a maintainer/user helper, not a postinstall. */
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const [releaseArg, prefixArg] = process.argv.slice(2);
if (!releaseArg || !prefixArg || process.argv.length !== 4) {
  console.error('Usage: node scripts/install-local.mjs RELEASE_DIR INSTALL_PREFIX');
  process.exit(2);
}
const release = fs.realpathSync(releaseArg);
const prefix = path.resolve(prefixArg);
const manifest = JSON.parse(fs.readFileSync(path.join(release, 'release-manifest.json'), 'utf8'));
if (manifest.os !== process.platform || manifest.cpu !== process.arch) {
  throw new Error(`This payload targets ${manifest.os}-${manifest.cpu}; current Node process is ${process.platform}-${process.arch}.`);
}
const tarballs = manifest.packages.map(item => path.join(release, item.filename));
for (const tarball of tarballs) if (!fs.statSync(tarball).isFile()) throw new Error(`Missing package: ${tarball}`);
fs.mkdirSync(prefix, { recursive: true });
execFileSync('npm', ['install', '--prefix', prefix, '--offline', '--ignore-scripts', '--no-audit', '--no-fund', '--include=optional', ...tarballs], { stdio: 'inherit' });
console.log(`Installed locally. Run ${path.join(prefix, 'node_modules', '.bin', 'accrete')} --help`);
