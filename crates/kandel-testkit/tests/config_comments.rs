//! End-to-end check of the config round-trip the management UI performs.
//!
//! The UI never edits TOML directly: it loads `config.toml` into `App` values,
//! mutates one entry, and writes the whole map back with
//! `save_config_preserving`. That loop is what these tests drive, with no UI in
//! the middle, so a regression in preservation shows up as a lost comment here
//! rather than as a bug report from someone whose hand-written config was
//! silently stripped.
//!
//! The unit tests in `kandel-config` cover the function; these cover the
//! realistic sequence a user hits (comment on disk, edit one app, save, reload).

use kandel_config::{load_config, save_config_preserving, App, ConfigError};

/// Writes `body` to a fresh `config.toml` in a temp dir and returns the dir
/// (kept alive by the caller) and the file path.
fn write_config(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, body).unwrap();
    (dir, path)
}

/// A fake-but-absolute command path. `resolved_command` returns an absolute
/// path verbatim without checking that it exists, so validation passes without
/// needing a real binary on disk.
fn fake_exe(dir: &tempfile::TempDir, name: &str) -> String {
    dir.path().join(name).to_string_lossy().into_owned()
}

#[test]
fn editing_one_app_keeps_comments_everywhere_else() {
    let dir = tempfile::tempdir().unwrap();
    let web = fake_exe(&dir, "web.exe");
    let worker = fake_exe(&dir, "worker.exe");
    let path = dir.path().join("config.toml");

    // A hand-written config: a file banner, a per-entry comment, a trailing
    // note, and a second entry we will not touch.
    let original = format!(
        "# Kandel config\n\
         # `npm` lives on PATH; keep it bare\n\
         \n\
         # the web server\n\
         [\"web\"]\n\
         command = '{web}'\n\
         \n\
         # background worker\n\
         [\"worker\"]\n\
         command = '{worker}'\n\
         restart_delay_sec = 7\n\
         \n\
         # end\n"
    );
    std::fs::write(&path, &original).unwrap();

    // The exact UI sequence: load, mutate one entry, save preserving.
    let mut apps = load_config(&path).unwrap();
    let worker_cfg = apps.get_mut("worker").unwrap();
    worker_cfg.args = vec!["--once".into()];
    worker_cfg.max_retries = 3;
    save_config_preserving(&path, &apps).unwrap();

    let got = std::fs::read_to_string(&path).unwrap();
    for comment in [
        "# Kandel config",
        "# `npm` lives on PATH; keep it bare",
        "# the web server",
        "# background worker",
        "# end",
    ] {
        assert!(got.contains(comment), "lost comment {comment:?}:\n{got}");
    }

    // The edit itself round-trips through the file.
    let reloaded = load_config(&path).unwrap();
    assert_eq!(reloaded["worker"].args, ["--once"]);
    assert_eq!(reloaded["worker"].max_retries, 3);
    // The untouched entry is unchanged.
    assert_eq!(reloaded["web"].command, web);
}

#[test]
fn a_no_op_save_is_byte_for_byte_identical() {
    // This is what keeps the core's file watcher from seeing a change (and
    // reconciling) when nothing actually changed.
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_exe(&dir, "app.exe");
    let path = dir.path().join("config.toml");

    let original = format!(
        "# header\n\
         [\"app\"]\n\
         command = '{exe}'\n\
         args = [\"run\", \"dev\"]\n\
         enabled = false\n"
    );
    std::fs::write(&path, &original).unwrap();

    let apps = load_config(&path).unwrap();
    save_config_preserving(&path, &apps).unwrap();

    let got = std::fs::read_to_string(&path).unwrap();
    assert_eq!(got, original, "a no-op save must not touch the file");
}

#[test]
fn deleting_an_entry_removes_it_and_leaves_the_rest_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_exe(&dir, "app.exe");
    let path = dir.path().join("config.toml");

    let original = format!(
        "# keep me\n\
         [\"keep\"]\n\
         command = '{exe}'\n\
         \n\
         # delete me\n\
         [\"delete\"]\n\
         command = '{exe}'\n"
    );
    std::fs::write(&path, &original).unwrap();

    let mut apps = load_config(&path).unwrap();
    apps.remove("delete");
    save_config_preserving(&path, &apps).unwrap();

    let got = std::fs::read_to_string(&path).unwrap();
    assert!(
        got.contains("# keep me"),
        "kept entry's comment lost:\n{got}"
    );
    assert!(got.contains("[\"keep\"]"), "kept entry lost:\n{got}");
    assert!(
        !got.contains("[\"delete\"]"),
        "removed entry lingers:\n{got}"
    );

    let reloaded = load_config(&path).unwrap();
    assert_eq!(reloaded.len(), 1);
    assert!(reloaded.contains_key("keep"));
}

#[test]
fn adding_an_entry_preserves_existing_comments() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_exe(&dir, "app.exe");
    let path = dir.path().join("config.toml");

    let original = format!("# existing\n[\"existing\"]\ncommand = '{exe}'\n");
    std::fs::write(&path, &original).unwrap();

    let mut apps = load_config(&path).unwrap();
    apps.insert(
        "added".to_string(),
        App {
            name: "added".into(),
            command: exe.clone(),
            ..App::default()
        },
    );
    save_config_preserving(&path, &apps).unwrap();

    let got = std::fs::read_to_string(&path).unwrap();
    assert!(got.contains("# existing"), "existing comment lost:\n{got}");

    // A newly inserted entry is written as a bare key (`[added]`); the original
    // quoted header is only a user formatting choice, so assert on the reloaded
    // config rather than on the exact header spelling.
    let reloaded = load_config(&path).unwrap();
    assert_eq!(reloaded.len(), 2);
    assert!(reloaded.contains_key("existing"));
    assert!(reloaded.contains_key("added"));
}

#[test]
fn adding_an_entry_with_spaces_in_its_name_is_quoted() {
    // Kandel entry names routinely contain spaces ("My Svelte Dev Server"). A
    // newly inserted table must be emitted with a quoted header, or the file
    // would not parse back.
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_exe(&dir, "app.exe");
    let path = dir.path().join("config.toml");

    let original = format!("# existing\n[\"existing\"]\ncommand = '{exe}'\n");
    std::fs::write(&path, &original).unwrap();

    let mut apps = load_config(&path).unwrap();
    apps.insert(
        "My dev server".to_string(),
        App {
            name: "My dev server".into(),
            command: exe.clone(),
            ..App::default()
        },
    );
    save_config_preserving(&path, &apps).unwrap();

    // The file must parse back, and the spaced name must survive intact.
    let reloaded = load_config(&path).unwrap();
    assert_eq!(reloaded.len(), 2);
    assert!(reloaded.contains_key("My dev server"));
    assert!(reloaded.contains_key("existing"));
}

#[test]
fn a_misspelled_field_is_rejected_instead_of_silently_dropped() {
    // The regression: an unknown key used to be ignored on load and then deleted
    // by the next save, so the user's typo vanished without a word.
    let (_dir, path) = write_config("[\"app\"]\ncommand = \"cmd.exe\"\nrestart_delay = 3\n");

    let err = load_config(&path).unwrap_err();
    match err {
        ConfigError::Invalid { app, message } => {
            assert_eq!(app, "app");
            assert!(
                message.contains("restart_delay"),
                "the error should name the offending key: {message}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}
