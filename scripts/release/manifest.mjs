/**
 * Builds `build/latest.json`, the static updater manifest the app fetches from
 * the release's `latest` alias.
 *
 * Runs after both architectures are packaged, so every platform entry lands in
 * one file. Each entry pairs a download URL with the signature Tauri wrote
 * beside that installer; the updater verifies the downloaded artifact against
 * it before running anything.
 *
 * The version is read from `[workspace.package]` in the root Cargo.toml — the
 * one place Cargo and Tauri both read it from — so it cannot drift from the
 * binaries it describes.
 */
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const buildDir = path.join(root, "build");
const repo = "https://github.com/k1rana/kandel";

/** Reads `[workspace.package] version` from the root Cargo.toml. */
function workspaceVersion() {
  const toml = readFileSync(path.join(root, "Cargo.toml"), "utf8");
  const section = toml.split(/^\[/m).find((part) => part.startsWith("workspace.package]"));
  const match = section && /^version\s*=\s*"([^"]+)"/m.exec(section);
  if (!match) {
    console.error("[manifest] cannot read the version from [workspace.package] in Cargo.toml");
    process.exit(1);
  }
  return match[1];
}

const version = workspaceVersion();

// Tauri's static-manifest target keys: `windows-<arch>`, with the Rust arch
// token (`x86_64`, `aarch64`), mapped to the WinGet token used in asset names.
const ARCHES = { "windows-x86_64": "x64", "windows-aarch64": "arm64" };

const platforms = {};
for (const [target, arch] of Object.entries(ARCHES)) {
  const installer = `kandel-${version}-windows-${arch}-setup.exe`;
  const sigPath = path.join(buildDir, `${installer}.sig`);
  if (!existsSync(sigPath)) {
    console.error(
      `[manifest] missing ${path.relative(root, sigPath)} — build with TAURI_SIGNING_PRIVATE_KEY set`,
    );
    process.exit(1);
  }
  platforms[target] = {
    signature: readFileSync(sigPath, "utf8").trim(),
    url: `${repo}/releases/latest/download/${installer}`,
  };
}

// Release notes are not embedded here: the app fetches them from the GitHub
// API, so they stay accurate even if a published release's notes are edited.
// The manifest stays a pure update pointer (version + signed artifacts).
const manifest = {
  version,
  pub_date: new Date().toISOString(),
  platforms,
};

const out = path.join(buildDir, "latest.json");
writeFileSync(out, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`[manifest] wrote ${path.relative(root, out)} for ${Object.keys(platforms).join(", ")}`);
