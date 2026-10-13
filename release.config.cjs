/**
 * semantic-release configuration for Kandel.
 *
 * Releases are cut on CI, never locally. semantic-release reads the Conventional
 * Commits since the last tag, decides the next version, writes it into the
 * workspace Cargo.toml, builds both binaries at that version, and publishes the
 * GitHub release with its assets.
 *
 * Two kinds of branch cut releases:
 *
 *   * `main` — stable releases. The version bump is committed back so the tag,
 *     the binaries, and the checked-in version all agree.
 *   * `N.x-dev` (e.g. `1.x-dev`) — pre-releases (`1.1.0-beta.1`, `beta.2`, …).
 *     The bump is *not* committed: Cargo.toml is written in the CI working tree
 *     only, so a dev branch never diverges from `main` on the version line and
 *     merges never conflict there.
 *
 * Only the dev branch currently being run is declared, so every dev branch can
 * use the same `beta` prerelease id: semantic-release requires prerelease ids to
 * be unique across the declared branches, and declaring a glob would expand to
 * every `N.x-dev` at once. Tag lookup is already scoped per branch
 * (`git tag --merged`), so the lines never see each other's tags.
 *
 * The version lives in exactly one place — `[workspace.package]` in the root
 * Cargo.toml — which both Cargo and Tauri read. Nothing else in the tree
 * declares a version.
 */
const { execFileSync } = require('node:child_process');

// The branch this run is releasing from. `GITHUB_REF_NAME` is set on every
// Actions event; the git fallback only matters for local runs.
function currentBranch() {
  return (
    process.env.GITHUB_REF_NAME ??
    execFileSync('git', ['branch', '--show-current'], { encoding: 'utf8' }).trim()
  );
}

const branch = currentBranch();
const isDevBranch = /^\d+\.x-dev$/.test(branch);
const isMain = branch === 'main';

// Declaring only the running dev branch keeps `beta` unique (see the header).
const branches = ['main'];
if (isDevBranch) {
  branches.push({ name: branch, prerelease: 'beta' });
}

const plugins = [
  // A manual `RELEASE_TYPE` (from workflow_dispatch) overrides the version
  // that would be derived from the commits. Returning `type` (undefined in
  // the auto case) defers to the commit-analyzer below.
  {
    analyzeCommits(_, { type }) {
      const override = process.env.RELEASE_TYPE;
      if (
        override &&
        override !== 'auto' &&
        ['major', 'minor', 'patch'].includes(override)
      ) {
        return override;
      }
      return type;
    },
  },
  [
    '@semantic-release/commit-analyzer',
    { preset: 'conventionalcommits' },
  ],
  [
    '@semantic-release/release-notes-generator',
    {
      preset: 'conventionalcommits',
      writerOpts: {
        groupBy: 'type',
        commitGroupsSort: ['feat', 'fix', 'perf', 'docs', 'revert'],
        groups: [
          { type: 'feat', title: 'Features' },
          { type: 'fix', title: 'Fixes' },
          { type: 'perf', title: 'Improvements' },
          { type: 'docs', title: 'Docs' },
          { type: 'revert', title: 'Reverts' },
        ],
      },
    },
  ],
  [
    '@semantic-release/exec',
    {
      // Guard the computed version before anything is bumped or built:
      // refuse to release a version that already exists as a tag or a GitHub
      // release. See `scripts/release/verify-version.mjs`.
      verifyReleaseCmd: 'node scripts/release/verify-version.mjs ${nextRelease.version}',
      // Bump Cargo.toml, then build and package the binaries at that version.
      // Runs before the git plugin on `main`, so the commit picks up the bump.
      prepareCmd: 'node scripts/release/prepare.mjs ${nextRelease.version}',
      // Runs only when a release was published, so the workflow can tell a
      // real release from a no-op run and hand the version to the WinGet job.
      successCmd: 'node scripts/release/mark-released.mjs ${nextRelease.version}',
    },
  ],
  // Major releases only make sense on `main`. A `BREAKING CHANGE` commit on a
  // dev branch would otherwise cut `2.0.0-beta.1`; the workflow_dispatch guard
  // only covers a manual `major`, so gate it here to cover both paths. Runs
  // after the exec guard above so its clearer "version already exists" message
  // comes first when both apply.
  {
    verifyRelease(_, { branch: releasedFrom, nextRelease }) {
      if (releasedFrom.prerelease && nextRelease.type === 'major') {
        throw new Error(
          `Branch ${releasedFrom.name} cannot publish a major release. Cut majors from main.`,
        );
      }
    },
  },
  [
    '@semantic-release/github',
    {
      assets: [
        {
          path: 'build/kandel-*-windows-amd64.zip',
          label: 'Portable (windows-amd64)',
        },
        {
          path: 'build/kandel-*-windows-x64-setup.exe',
          label: 'NSIS installer (x64)',
        },
      ],
    },
  ],
];

// Only `main` commits the bump back. On a dev branch the version is transient:
// the release is cut and tagged, but Cargo.toml is left untouched on the branch.
if (isMain) {
  plugins.push([
    '@semantic-release/git',
    {
      // Commit the bump back so the tag, the binaries, and the checked-in
      // version all agree. `[skip ci]` keeps the push from re-triggering the
      // release workflow.
      assets: ['Cargo.toml', 'Cargo.lock'],
      message: 'chore(release): ${nextRelease.version} [skip ci]\n\n${nextRelease.notes}',
    },
  ]);
}

module.exports = {
  branches,
  // Matches the tag scheme the repo already documents (`v1.0.0`). The WinGet
  // action strips the leading `v`, so `v1.0.0` becomes version `1.0.0` there.
  tagFormat: 'v${version}',
  plugins,
};
