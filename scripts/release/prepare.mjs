/**
 * Prepares a release: writes the version, then builds and packages the
 * binaries at that version. Invoked by semantic-release's `prepareCmd`, after
 * the version guard has passed and before the version bump is committed.
 *
 *   node scripts/release/prepare.mjs 1.1.0
 *
 * The build refreshes Cargo.lock (a workspace member's version changed), so the
 * commit that follows captures both files.
 */
import { spawnSync } from 'node:child_process';
import process from 'node:process';

import { writeWorkspaceVersion } from './set-version.mjs';

/** Runs a command to completion, inheriting stdio, and exits on failure. */
function run(command, args) {
  const result = spawnSync(command, args, {
    stdio: 'inherit',
    // `pnpm` is a `.cmd` shim on Windows, so it needs a shell there.
    shell: process.platform === 'win32',
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

const version = process.argv[2];
if (!version) {
  console.error('usage: node scripts/release/prepare.mjs <version>');
  process.exit(1);
}

console.log(`[release] setting version to ${version}`);
writeWorkspaceVersion(version);

// Builds the core and the UI (which runs `tauri build`, producing the NSIS
// installer), then packages the portable archive and normalizes the installer
// name. Both binaries are built at the version just written.
console.log('[release] building…');
run('pnpm', ['run', 'build']);
console.log('[release] packaging…');
run('pnpm', ['run', 'package']);
