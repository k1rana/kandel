/**
 * semantic-release configuration for Kandel.
 *
 * Releases are cut on CI from `main`, never locally. semantic-release reads the
 * Conventional Commits since the last tag, decides the next version, writes it
 * into the workspace Cargo.toml, builds both binaries at that version, publishes
 * the GitHub release with its assets, and commits the version bump back.
 *
 * The version lives in exactly one place — `[workspace.package]` in the root
 * Cargo.toml — which both Cargo and Tauri read. The plugins below write it and
 * commit it; nothing else in the tree declares a version.
 */
module.exports = {
  branches: ['main'],
  // Matches the tag scheme the repo already documents (`v1.0.0`). The WinGet
  // action strips the leading `v`, so `v1.0.0` becomes version `1.0.0` there.
  tagFormat: 'v${version}',
  plugins: [
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
        // Runs before the git plugin below, so the commit picks up the bump.
        prepareCmd: 'node scripts/release/prepare.mjs ${nextRelease.version}',
        // Runs only when a release was published, so the workflow can tell a
        // real release from a no-op run and hand the version to the WinGet job.
        successCmd: 'node scripts/release/mark-released.mjs ${nextRelease.version}',
      },
    ],
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
    [
      '@semantic-release/git',
      {
        // Commit the bump back so the tag, the binaries, and the checked-in
        // version all agree. `[skip ci]` keeps the push from re-triggering the
        // release workflow.
        assets: ['Cargo.toml', 'Cargo.lock'],
        message: 'chore(release): ${nextRelease.version} [skip ci]\n\n${nextRelease.notes}',
      },
    ],
  ],
};
