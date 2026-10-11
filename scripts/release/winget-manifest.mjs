/**
 * Generates the WinGet manifest set for a release.
 *
 * WinGet needs three YAML files (version, installer, defaultLocale) submitted to
 * `microsoft/winget-pkgs`. This script produces them from the installer that a
 * release actually published — the SHA256 must come from that exact artifact,
 * because a Tauri build is not bit-reproducible, so hashing a local build would
 * not match what users download.
 *
 *   node scripts/release/winget-manifest.mjs \
 *     --installer <path-to-the-downloaded-setup.exe> [--version 1.0.0] [--out winget]
 *
 * The version defaults to `[workspace.package]` in Cargo.toml. The installer URL
 * is derived from the tag and asset name `scripts/package.mjs` produces.
 */
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

/** Kandel's WinGet identity. The identifier must match the first submission. */
const IDENTIFIER = 'Tojoo.Kandel';
const PUBLISHER = 'Farhan Brillan Widyakirana (k1rana)';
const REPO = 'https://github.com/k1rana/kandel';

/** Parses `--key value` arguments into an object. */
function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 1) {
    const key = argv[i];
    if (key.startsWith('--')) {
      args[key.slice(2)] = argv[i + 1];
      i += 1;
    }
  }
  return args;
}

/** Reads the current `[workspace.package]` version from the root Cargo.toml. */
function currentVersion() {
  const toml = readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
  const section = toml
    .split(/^\[/m)
    .find((part) => part.startsWith('workspace.package]'));
  const match = section && /^version\s*=\s*"([^"]+)"/m.exec(section);
  if (!match) {
    console.error('could not read the current version from [workspace.package] in Cargo.toml');
    process.exit(1);
  }
  return match[1];
}

const args = parseArgs(process.argv.slice(2));
const version = args.version ?? currentVersion();
const installer = args.installer;
const outDir = args.out ?? 'winget';

if (!installer) {
  console.error('usage: node scripts/release/winget-manifest.mjs --installer <setup.exe> [--version X] [--out dir]');
  process.exit(1);
}

const assetName = `kandel-${version}-windows-x64-setup.exe`;
const url = `${REPO}/releases/download/v${version}/${assetName}`;

const sha256 = createHash('sha256')
  .update(readFileSync(installer))
  .digest('hex')
  .toUpperCase();

const header = (type) =>
  `# yaml-language-server: $schema=https://aka.ms/winget-manifest.${type}.1.12.0.schema.json`;

const files = {
  [`${IDENTIFIER}.yaml`]: `${header('version')}
PackageIdentifier: ${IDENTIFIER}
PackageVersion: ${version}
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.12.0
`,
  [`${IDENTIFIER}.installer.yaml`]: `${header('installer')}
PackageIdentifier: ${IDENTIFIER}
PackageVersion: ${version}
Platform:
- Windows.Desktop
MinimumOSVersion: 10.0.17763.0
InstallerType: nullsoft
Scope: user
InstallModes:
- silent
- silentWithProgress
InstallerSwitches:
  Silent: /S
  SilentWithProgress: /P
UpgradeBehavior: install
ReleaseDate: ${new Date().toISOString().slice(0, 10)}
Installers:
- Architecture: x64
  InstallerUrl: ${url}
  InstallerSha256: ${sha256}
ManifestType: installer
ManifestVersion: 1.12.0
`,
  [`${IDENTIFIER}.locale.en-US.yaml`]: `${header('defaultLocale')}
PackageIdentifier: ${IDENTIFIER}
PackageVersion: ${version}
PackageLocale: en-US
Publisher: ${PUBLISHER}
PublisherUrl: https://github.com/k1rana
PackageName: Kandel
PackageUrl: ${REPO}
License: Apache-2.0
LicenseUrl: ${REPO}/blob/main/LICENSE
Copyright: Copyright (c) 2026 Farhan Brillan Widyakirana (k1rana)
ShortDescription: Per-process supervision for the Windows interactive session.
Description: Kandel launches, monitors, and auto-restarts the applications you tell it to keep alive, in the interactive desktop session that Windows services cannot reach.
Moniker: kandel
Tags:
- process-manager
- supervisor
- windows-service
- tray
- tauri
ReleaseNotesUrl: ${REPO}/releases/tag/v${version}
ManifestType: defaultLocale
ManifestVersion: 1.12.0
`,
};

// The community repository lays manifests out as
// `manifests/<first-lowercase>/<Publisher>/<Package>/<Version>/`.
const [publisher, pkg] = IDENTIFIER.split('.');
const dir = path.join(outDir, 'manifests', publisher[0].toLowerCase(), publisher, pkg, version);
mkdirSync(dir, { recursive: true });

for (const [name, content] of Object.entries(files)) {
  const file = path.join(dir, name);
  writeFileSync(file, content);
  console.log(`[winget] wrote ${path.relative(root, file)}`);
}

console.log(`[winget] installer ${assetName}`);
console.log(`[winget] sha256 ${sha256}`);
console.log(`[winget] submit the folder: ${path.relative(root, dir)}`);
