#!/usr/bin/env node
'use strict';

// The native executable owns all command parsing, initialization and persistence.
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const ownPackage = require('../package.json');

const supported = new Set(['linux-x64', 'linux-arm64', 'darwin-x64', 'darwin-arm64']);

function fail(message) {
  process.stderr.write(`accrete launcher: ${message}\n`);
  process.exitCode = 5;
}

function launch(args = process.argv.slice(2)) {
  const platform = `${process.platform}-${process.arch}`;
  if (!supported.has(platform)) {
    fail(`unsupported target ${platform}. This release contains Linux and macOS x64/arm64 payloads. Build the native Rust CLI on other targets; no binary is downloaded at runtime.`);
    return;
  }
  const packageName = `@accrete/kernel-${platform}`;
  let manifestPath;
  try {
    manifestPath = require.resolve(`${packageName}/package.json`);
  } catch (error) {
    if (error.code !== 'MODULE_NOT_FOUND') {
      fail(`cannot read ${packageName}: ${error.message}`);
      return;
    }
    fail(`required native payload ${packageName}@${ownPackage.version} is missing. Reinstall @accrete/cli@${ownPackage.version} with optional dependencies enabled (npm install --include=optional). Do not copy node_modules between operating systems or CPU architectures. For an offline installation use the matching payload tarball or standalone native archive.`);
    return;
  }
  let payload;
  try {
    payload = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  } catch (error) {
    fail(`cannot read native payload manifest: ${error.message}`);
    return;
  }
  if (payload.name !== packageName || payload.version !== ownPackage.version) {
    fail(`payload release mismatch: expected ${packageName}@${ownPackage.version}, found ${payload.name}@${payload.version}. Reinstall matching packages.`);
    return;
  }
  const binary = path.join(path.dirname(manifestPath), 'bin', 'accrete');
  try {
    fs.accessSync(binary, fs.constants.R_OK | fs.constants.X_OK);
  } catch (error) {
    fail(`native executable is missing, unreadable or not executable: ${binary} (${error.code}). Reinstall the matching payload or use the native archive.`);
    return;
  }
  const child = spawn(binary, args, {
    cwd: process.cwd(), env: process.env, stdio: 'inherit', shell: false,
  });
  const handlers = new Map();
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    const handler = () => { if (!child.killed) child.kill(signal); };
    handlers.set(signal, handler);
    process.on(signal, handler);
  }
  function cleanup() {
    for (const [signal, handler] of handlers) process.removeListener(signal, handler);
  }
  child.once('error', error => {
    cleanup();
    fail(`could not start ${binary}: ${error.message}. Check the payload target and execution permissions.`);
  });
  child.once('exit', (code, signal) => {
    cleanup();
    if (signal) {
      // Preserve termination semantics for calling shells and process supervisors.
      try { process.kill(process.pid, signal); }
      catch { process.exitCode = 128 + ({ SIGINT: 2, SIGTERM: 15, SIGHUP: 1 }[signal] || 1); }
    } else {
      process.exitCode = code === null ? 5 : code;
    }
  });
}

if (require.main === module) launch();
module.exports = { launch };
