// Launcher regressions. Run with `node --test npm/test/launcher.test.js` (Node 18+; Unix only,
// since the scenarios lean on symlinked shims and POSIX signals).
'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const child_process = require('child_process');

const LAUNCHER = path.join(__dirname, '..', 'bin', 'filegoblin.js');
const skip = process.platform === 'win32' && 'POSIX shims and signals';

const PLATFORM = {
  'darwin-arm64': 'darwin-arm64',
  'darwin-x64': 'darwin-x64',
  'linux-x64': 'linux-x64',
  'linux-arm64': 'linux-arm64',
}[`${process.platform}-${process.arch}`];

// A global install: prefix/lib/node_modules/filegoblin plus npm's shim,
// prefix/bin/filegoblin -> the launcher. `fakeBinary`, when given, is the
// shell script installed as the platform package's binary.
function install(fakeBinary) {
  const prefix = fs.mkdtempSync(path.join(os.tmpdir(), 'fg-launcher-'));
  const pkg = path.join(prefix, 'lib', 'node_modules', 'filegoblin', 'bin');
  fs.mkdirSync(pkg, { recursive: true });
  fs.copyFileSync(LAUNCHER, path.join(pkg, 'filegoblin.js'));
  fs.mkdirSync(path.join(prefix, 'bin'));
  fs.symlinkSync(path.join(pkg, 'filegoblin.js'), path.join(prefix, 'bin', 'filegoblin'));
  if (fakeBinary) {
    const plat = path.join(prefix, 'lib', 'node_modules', '@filegoblin', PLATFORM);
    fs.mkdirSync(path.join(plat, 'bin'), { recursive: true });
    fs.writeFileSync(path.join(plat, 'package.json'), '{"name":"@filegoblin/x","version":"0.0.0"}');
    fs.writeFileSync(path.join(plat, 'bin', 'filegoblin'), fakeBinary, { mode: 0o755 });
  }
  const env = Object.assign({}, process.env, {
    PATH: `${path.join(prefix, 'bin')}${path.delimiter}/usr/bin${path.delimiter}/bin`,
  });
  delete env.FILEGOBLIN_NPM_LAUNCHER;
  return { prefix, env, shim: path.join(prefix, 'bin', 'filegoblin') };
}

// Regression: with no platform package, the PATH fallback found npm's shim for
// this very launcher and ran it, which ran itself, until the machine ran out of
// processes.
test('a missing platform package fails instead of recursing', { skip }, () => {
  const { env, shim } = install(null);
  const r = child_process.spawnSync(process.execPath, [shim, '--version'], {
    env,
    encoding: 'utf8',
    timeout: 15000,
  });
  assert.notStrictEqual(r.error && r.error.code, 'ETIMEDOUT', 'the launcher recursed');
  assert.strictEqual(r.status, 1, r.stderr);
  assert.match(r.stderr, /could not find precompiled filegoblin binary/);
});

test('exit codes pass through', { skip: skip || (!PLATFORM && 'unsupported platform') }, () => {
  const { env, shim } = install('#!/bin/sh\nexit 4\n');
  const r = child_process.spawnSync(process.execPath, [shim, 'x'], { env, timeout: 15000 });
  assert.strictEqual(r.status, 4);
});

// Regression: spawnSync left the binary running when the launcher was sent
// SIGTERM, and reported a signal death as a plain exit 1.
test('SIGTERM reaches the binary and the exit code says so', { skip: skip || (!PLATFORM && 'unsupported platform') }, async () => {
  const { prefix, env, shim } = install(null);
  const pidFile = path.join(prefix, 'child.pid');
  const plat = path.join(prefix, 'lib', 'node_modules', '@filegoblin', PLATFORM);
  fs.mkdirSync(path.join(plat, 'bin'), { recursive: true });
  fs.writeFileSync(path.join(plat, 'package.json'), '{"name":"@filegoblin/x","version":"0.0.0"}');
  fs.writeFileSync(path.join(plat, 'bin', 'filegoblin'), `#!/bin/sh\necho $$ > "${pidFile}"\nexec sleep 30\n`, { mode: 0o755 });

  const launcher = child_process.spawn(process.execPath, [shim], { env, stdio: 'ignore' });
  for (let i = 0; i < 100 && !fs.existsSync(pidFile); i++) await new Promise((r) => setTimeout(r, 50));
  const pid = Number(fs.readFileSync(pidFile, 'utf8'));
  const exited = new Promise((resolve) => launcher.on('exit', (code, signal) => resolve({ code, signal })));
  launcher.kill('SIGTERM');
  const { code } = await exited;
  assert.strictEqual(code, 128 + os.constants.signals.SIGTERM);
  await new Promise((r) => setTimeout(r, 200));
  assert.throws(() => process.kill(pid, 0), 'the binary outlived the launcher');
});
