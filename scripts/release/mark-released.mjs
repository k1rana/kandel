/**
 * Reports that a release was published, and at which version.
 *
 * Invoked by semantic-release's `successCmd`, which runs only when a release
 * actually happened. In CI it appends `released` and `version` to the step's
 * `$GITHUB_OUTPUT`, so the workflow can tell a real release from a no-op run and
 * pass the version on (for example, to the WinGet publishing job).
 *
 *   node scripts/release/mark-released.mjs 1.1.0
 */
import { appendFileSync } from 'node:fs';
import process from 'node:process';

const version = process.argv[2];
if (!version) {
  console.error('usage: node scripts/release/mark-released.mjs <version>');
  process.exit(1);
}

const output = process.env.GITHUB_OUTPUT;
if (output) {
  appendFileSync(output, `released=true\nversion=${version}\n`);
} else {
  // Local run: no GitHub step to feed, so just report it.
  console.log(`[mark-released] released ${version}`);
}
