/**
 * Prepares a release: writes the version, then builds and packages the
 * binaries at that version, for every architecture Kandel ships. Invoked by
 * semantic-release's `prepareCmd`, after the version guard has passed and
 * before the version bump is committed.
 *
 *   node scripts/release/prepare.mjs 1.1.0
 *
 * Both architectures are cross-built on the one runner: x64 natively, arm64
 * from an x64 host, which needs the `aarch64-pc-windows-msvc` target. Building
 * both in one job at the just-written version lets semantic-release attach every
 * architecture's assets to the single release it creates.
 *
 * The build refreshes Cargo.lock (a workspace member's version changed), so the
 * commit that follows captures both files.
 */
import { spawnSync } from 'node:child_process';
import process from 'node:process';

import { writeWorkspaceVersion } from './set-version.mjs';

/** The architectures a release ships: WinGet token -> Rust target triple. */
const TARGETS = {
  x64: 'x86_64-pc-windows-msvc',
  arm64: 'aarch64-pc-windows-msvc',
};

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

// Build and package each architecture at the version just written. `build.mjs`
// runs `tauri build` (producing the NSIS installer) and stages both binaries;
// `package.mjs` archives them under the matching `<arch>` asset name.
for (const [arch, triple] of Object.entries(TARGETS)) {
  console.log(`[release] building ${arch} (${triple})…`);
  run('node', ['scripts/build.mjs', '--target', triple]);
  console.log(`[release] packaging ${arch}…`);
  run('node', ['scripts/package.mjs', '--target', triple]);
}

// One manifest for both architectures, at the version just built. It points at
// the signed installers; release notes are fetched by the app, not carried here.
console.log('[release] writing the updater manifest…');
run('node', ['scripts/release/manifest.mjs']);
