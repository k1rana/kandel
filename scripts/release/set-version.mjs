/**
 * Writes the version into `[workspace.package]` in the root Cargo.toml.
 *
 * This is the single source of truth for Kandel's version: Cargo reads it for
 * the crates (and `env!("CARGO_PKG_VERSION")` in the core), and Tauri falls back
 * to the same value for the app/installer. Used by `scripts/release/prepare.mjs`
 * during a release; runnable on its own for a manual bump.
 *
 *   node scripts/release/set-version.mjs 1.1.0
 */
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

/**
 * Rewrites the `version` line of `[workspace.package]`, leaving every other
 * section untouched. The `[^\[]*?` cap stops the match from crossing into the
 * next section if the version line were ever missing.
 */
export function writeWorkspaceVersion(version) {
  const file = path.join(root, 'Cargo.toml');
  const toml = readFileSync(file, 'utf8');
  const section = /^\[workspace\.package\][^\[]*?^version\s*=\s*"[^"]+"/m;
  if (!section.test(toml)) {
    throw new Error('could not find a version under [workspace.package] in Cargo.toml');
  }
  const next = toml.replace(section, (block) =>
    block.replace(/version\s*=\s*"[^"]+"/, `version = "${version}"`),
  );
  writeFileSync(file, next);
  return version;
}

// CLI entry point (not taken when imported by prepare.mjs).
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const version = process.argv[2];
  if (!version) {
    console.error('usage: node scripts/release/set-version.mjs <version>');
    process.exit(1);
  }
  writeWorkspaceVersion(version);
  console.log(`[set-version] Cargo.toml [workspace.package] -> ${version}`);
}
