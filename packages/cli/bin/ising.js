#!/usr/bin/env node

/**
 * Thin wrapper that resolves and runs the platform-specific ising binary
 * installed via optionalDependencies.
 */

const { execFileSync } = require('child_process');
const { join, resolve } = require('path');
const { existsSync } = require('fs');

const PLATFORMS = {
  'darwin-arm64': '@codervisor/ising-cli-darwin-arm64',
  'darwin-x64': '@codervisor/ising-cli-darwin-x64',
  'linux-x64': '@codervisor/ising-cli-linux-x64',
  'win32-x64': '@codervisor/ising-cli-windows-x64',
};

const platformKey = `${process.platform}-${process.arch}`;
const pkg = PLATFORMS[platformKey];

if (!pkg) {
  console.error(
    `Unsupported platform: ${process.platform}-${process.arch}\n` +
    `Supported: ${Object.keys(PLATFORMS).join(', ')}`
  );
  process.exit(1);
}

let binPath;

// Try platform package first (npm-installed binary)
try {
  const pkgDir = require.resolve(`${pkg}/package.json`);
  const ext = process.platform === 'win32' ? '.exe' : '';
  binPath = join(pkgDir, '..', `ising${ext}`);
} catch {
  // Fall back to locally-built cargo binary (development)
  const ext = process.platform === 'win32' ? '.exe' : '';
  const candidates = [
    resolve(__dirname, '..', '..', '..', 'target', 'release', `ising${ext}`),
    resolve(__dirname, '..', '..', '..', 'target', 'debug', `ising${ext}`),
  ];
  binPath = candidates.find(p => existsSync(p));

  if (!binPath) {
    console.error(
      `Could not find the ising binary for your platform (${platformKey}).\n` +
      `Expected npm package: ${pkg}\n\n` +
      'For local development, build with: cargo build\n' +
      'For production use: npm install @codervisor/ising-cli'
    );
    process.exit(1);
  }
}

try {
  execFileSync(binPath, process.argv.slice(2), { stdio: 'inherit' });
} catch (e) {
  if (e && typeof e === 'object' && 'status' in e) {
    process.exit(e.status);
  }
  throw e;
}
