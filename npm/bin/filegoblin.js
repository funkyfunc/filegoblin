#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const os = require('os');
const child_process = require('child_process');

function getBinaryName() {
  return process.platform === 'win32' ? 'filegoblin.exe' : 'filegoblin';
}

function getPlatformPackageName() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === 'darwin' && arch === 'arm64') return '@filegoblin/darwin-arm64';
  if (platform === 'darwin' && arch === 'x64') return '@filegoblin/darwin-x64';
  if (platform === 'linux' && arch === 'x64') return '@filegoblin/linux-x64';
  if (platform === 'linux' && arch === 'arm64') return '@filegoblin/linux-arm64';
  if (platform === 'win32' && arch === 'x64') return '@filegoblin/win32-x64';
  return null;
}

function findBinary() {
  const binaryName = getBinaryName();

  // 1. Check local Cargo build directories (for repository development)
  const candidateLocalPaths = [
    path.resolve(__dirname, '../../target/release', binaryName),
    path.resolve(__dirname, '../../target/debug', binaryName),
    path.resolve(__dirname, '..', binaryName),
    path.resolve(__dirname, binaryName)
  ];

  for (const candidate of candidateLocalPaths) {
    if (fs.existsSync(candidate)) {
      try {
        fs.accessSync(candidate, fs.constants.X_OK);
        return candidate;
      } catch (_) {
        // Exists but not executable
      }
    }
  }

  // 2. Check platform optional package
  const pkgName = getPlatformPackageName();
  if (pkgName) {
    try {
      const resolvedPkg = require.resolve(`${pkgName}/package.json`);
      const pkgDir = path.dirname(resolvedPkg);
      const pkgBinary = path.join(pkgDir, 'bin', binaryName);
      if (fs.existsSync(pkgBinary)) {
        return pkgBinary;
      }
    } catch (_) {
      // Optional package not installed
    }
  }

  // 3. Fallback: check system PATH
  try {
    const whichCmd = process.platform === 'win32' ? 'where' : 'which';
    const systemPath = child_process.execSync(`${whichCmd} ${binaryName}`, { encoding: 'utf8', stdio: ['pipe', 'pipe', 'ignore'] }).trim().split(/\r?\n/)[0];
    if (systemPath && fs.existsSync(systemPath)) {
      return systemPath;
    }
  } catch (_) {
    // Not in PATH
  }

  return null;
}

function main() {
  const binaryPath = findBinary();
  if (!binaryPath) {
    const platform = process.platform;
    const arch = process.arch;
    console.error(
      `[filegoblin] Error: could not find precompiled filegoblin binary for ${platform}-${arch}.\n` +
      `Please ensure cargo build has been run, or install filegoblin via:\n` +
      `  cargo install filegoblin\n` +
      `or download the precompiled binary from https://github.com/funkyfunc/filegoblin/releases`
    );
    process.exit(1);
  }

  const args = process.argv.slice(2);
  const result = child_process.spawnSync(binaryPath, args, {
    stdio: 'inherit'
  });

  if (result.error) {
    console.error(`[filegoblin] Error executing binary at ${binaryPath}:`, result.error.message);
    process.exit(1);
  }

  process.exit(result.status ?? (result.signal ? 1 : 0));
}

main();
