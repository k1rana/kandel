/**
 * Pre-bump consistency guard: run in semantic-release's `verifyRelease` step,
 * before anything is written or built.
 *
 *   node scripts/release/verify-version.mjs <next-version>
 *
 * semantic-release works out the next version from git tags alone — it never
 * reads `Cargo.toml`. That leaves two ways a release can go wrong, and this
 * script refuses both:
 *
 *   1. The version about to be published already exists (as a tag or a GitHub
 *      release). Should not happen, but a release created outside
 *      semantic-release — or a tag pushed by hand — would cause it.
 *
 *   2. `Cargo.toml` sits at a version that was never released. Because
 *      semantic-release ignores the file, it would bump *past* that version and
 *      silently discard it: e.g. Cargo.toml says 1.2.0 but the last tag is
 *      v1.1.0, so the next release becomes 1.1.1 and 1.2.0 is lost. This is the
 *      "current version was never released" case.
 *
 * Exits non-zero with a reason on stdout, which semantic-release surfaces.
 */
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

/** Reads the current `[workspace.package]` version from the root Cargo.toml. */
function currentVersion() {
  const toml = readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
  const section = toml
    .split(/^\[/m)
    .find((part) => part.startsWith('workspace.package]'));
  const match = section && /^version\s*=\s*"([^"]+)"/m.exec(section);
  if (!match) {
    console.log('could not read the current version from [workspace.package] in Cargo.toml');
    process.exit(1);
  }
  return match[1];
}

/** Fails the release with `reason` shown to the user. */
function refuse(reason) {
  console.log(reason);
  process.exit(1);
}

/** True when the command exits 0; false on any failure (including "not found"). */
function exists(command, args) {
  try {
    execFileSync(command, args, { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

/** True when a git tag for `version` exists (the source semantic-release uses). */
function tagExists(version) {
  return exists('git', ['rev-parse', '--verify', '--quiet', `refs/tags/v${version}`]);
}

/**
 * True when a GitHub release for `version` exists. Catches a release created
 * without a tag. Best-effort: if `gh` is unavailable or unauthenticated the
 * check is skipped, and the tag check still applies.
 */
function releaseExists(version) {
  try {
    execFileSync('gh', ['release', 'view', `v${version}`], { stdio: 'ignore' });
    return true;
  } catch (error) {
    // A non-zero exit with stderr is "not found"; anything else (ENOENT, auth)
    // means the tool could not answer, so do not treat it as "exists".
    return false;
  }
}

const next = process.argv[2];
if (!next) {
  console.error('usage: node scripts/release/verify-version.mjs <next-version>');
  process.exit(1);
}

const current = currentVersion();

// 1. The version about to be released must not already exist.
if (tagExists(next)) {
  refuse(`git tag v${next} already exists — refusing to re-release the same version`);
}
if (releaseExists(next)) {
  refuse(`GitHub release v${next} already exists — refusing to re-release the same version`);
}

// 2. The current version must not be stranded. If it differs from the next
//    version and was never released, bumping would skip it.
if (current !== next && !tagExists(current)) {
  refuse(
    `Cargo.toml is at ${current}, which was never released (no v${current} tag). ` +
      `Releasing ${next} now would skip it. Either bump Cargo.toml back to the last ` +
      `released version, or release ${current} first.`,
  );
}

console.log(
  `[verify-version] ok: current ${current} -> next ${next} ` +
    `(tag v${next} free${current === next ? '' : `, v${current} released`})`,
);
