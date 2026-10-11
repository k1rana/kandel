//! Tauri build script.
//!
//! `tauri-build` validates and copies every `bundle > externalBin` path on
//! *every* cargo invocation, not only when bundling (see
//! tauri-apps/tauri#14602). The real sidecar
//! (`binaries/kandel-<triple>.exe`) is staged by `scripts/stage-core.mjs`, which
//! runs only from Tauri's `beforeBuildCommand`. On a clean tree that leaves
//! `cargo check`, `cargo clippy`, and `cargo test` failing with:
//!
//! ```text
//! resource path `binaries/kandel-<triple>.exe` doesn't exist
//! ```
//!
//! When the sidecar is absent we therefore merge an empty `bundle.externalBin`
//! into the config this build sees, via the `TAURI_CONFIG` env var `tauri-build`
//! already honors. That suppresses the existence check and the copy without
//! touching the config file, so a real `tauri build` — where the sidecar *is*
//! staged first — still bundles the core.

use serde_json::{json, Value};
use std::path::PathBuf;

fn main() {
    drop_missing_external_bin();
    tauri_build::build();
}

/// Neutralizes `bundle.externalBin` for this build when the sidecar is
/// missing, by patching the `TAURI_CONFIG` override `tauri-build` merges.
///
/// Setting an empty list (rather than deleting the key) is enough: `tauri-build`
/// only checks paths that are present, so an empty list neither errors nor
/// copies anything into `target/`.
fn drop_missing_external_bin() {
    let Ok(triple) = std::env::var("TARGET") else {
        return;
    };
    let ext = if triple.contains("windows") {
        ".exe"
    } else {
        ""
    };

    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let sidecar = manifest_dir
        .join("binaries")
        .join(format!("kandel-{triple}{ext}"));

    // A real build stages the sidecar before cargo runs; leave the config as-is
    // so `externalBin` still lands in the bundle.
    if sidecar.exists() {
        return;
    }

    // Preserve any override the Tauri CLI already set (e.g. `tauri dev`), and
    // add ours on top.
    let mut config: Value = std::env::var("TAURI_CONFIG")
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| json!({}));

    if !config.is_object() {
        config = json!({});
    }
    let bundle = config
        .as_object_mut()
        .expect("config is an object")
        .entry("bundle")
        .or_insert_with(|| json!({}));
    if !bundle.is_object() {
        *bundle = json!({});
    }
    bundle
        .as_object_mut()
        .expect("bundle is an object")
        .insert("externalBin".into(), json!([]));

    std::env::set_var("TAURI_CONFIG", config.to_string());
}
