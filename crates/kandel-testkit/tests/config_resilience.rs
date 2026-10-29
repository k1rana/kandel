//! End-to-end checks that one broken entry cannot take down the whole config.
//!
//! The regression this guards: a single bad entry made every other entry
//! unloadable, so the UI showed nothing and a new entry could not be added.
use std::collections::BTreeSet;

use kandel_config::{
    load_config_lenient, save_config_preserving, save_config_preserving_removing, ConfigError,
    InvalidKind,
};

/// Writes `body` to a fresh `config.toml` and returns the dir and path.
fn write_config(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, body).unwrap();
    (dir, path)
}

/// A fake-but-absolute command path; absolute paths validate without existing.
fn fake_exe(dir: &tempfile::TempDir, name: &str) -> String {
    dir.path().join(name).to_string_lossy().into_owned()
}

#[test]
fn a_broken_entry_does_not_hide_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_exe(&dir, "app.exe");
    let body = format!(
        "# a real entry\n\
         [\"good\"]\n\
         command = '{exe}'\n\
         \n\
         # a typo makes this one invalid\n\
         [\"broken\"]\n\
         command = '{exe}'\n\
         restart_delay = 3\n"
    );
    let (_dir, path) = write_config(&body);

    let loaded = load_config_lenient(&path).unwrap();

    // The valid entry still loads...
    assert!(loaded.apps.contains_key("good"));
    assert!(!loaded.apps.contains_key("broken"));
    // ...and the broken one is reported, with a reason, instead of vanishing.
    let broken = &loaded.invalid["broken"];
    assert!(
        broken.messages.iter().any(|m| m.contains("restart_delay")),
        "the report should name the offending key: {:?}",
        broken.messages
    );
    // The draft keeps what the user wrote so it can be repaired.
    assert_eq!(broken.draft.command, exe);
}

#[test]
fn saving_a_good_entry_never_deletes_a_broken_one() {
    // The dangerous regression: the UI loads only what parses, so a save wrote a
    // map without the broken entry — and the next save deleted it outright.
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_exe(&dir, "app.exe");
    let body = format!(
        "[\"good\"]\n\
         command = '{exe}'\n\
         \n\
         [\"broken\"]\n\
         command = '{exe}'\n\
         restart_delay = 3\n"
    );
    let (_dir, path) = write_config(&body);

    // The exact UI loop: load leniently, edit the good entry, save preserving.
    let loaded = load_config_lenient(&path).unwrap();
    let mut apps = loaded.apps;
    apps.get_mut("good").unwrap().restart_delay_sec = 9;
    save_config_preserving(&path, &apps).unwrap();

    let got = std::fs::read_to_string(&path).unwrap();
    assert!(
        got.contains("[\"broken\"]"),
        "the broken entry was deleted by a save that did not touch it:\n{got}"
    );
    assert!(
        got.contains("restart_delay = 3"),
        "the broken entry lost the data the user wrote:\n{got}"
    );

    // And the edit still landed.
    let reloaded = load_config_lenient(&path).unwrap();
    assert_eq!(reloaded.apps["good"].restart_delay_sec, 9);
}

#[test]
fn a_broken_entry_can_still_be_deleted_explicitly() {
    // A broken entry must be removable: the user can see it and may want it gone.
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_exe(&dir, "app.exe");
    let body = format!(
        "[\"good\"]\n\
         command = '{exe}'\n\
         \n\
         [\"broken\"]\n\
         command = '{exe}'\n\
         restart_delay = 3\n"
    );
    let (_dir, path) = write_config(&body);

    let loaded = load_config_lenient(&path).unwrap();
    let apps = loaded.apps;
    let mut remove = BTreeSet::new();
    remove.insert("broken".to_string());
    save_config_preserving_removing(&path, &apps, &remove).unwrap();

    let got = std::fs::read_to_string(&path).unwrap();
    assert!(
        !got.contains("[\"broken\"]"),
        "broken entry lingers:\n{got}"
    );
    assert!(got.contains("[\"good\"]"), "good entry was lost:\n{got}");
}

#[test]
fn a_whole_file_syntax_error_is_still_reported() {
    // Leniency is per entry, not a blanket silence: a file that is not TOML at
    // all is still an error the user must see.
    let (_dir, path) = write_config("this is not = = toml");
    assert!(matches!(
        load_config_lenient(&path),
        Err(ConfigError::Parse(_))
    ));
}

#[test]
fn a_bad_writing_is_structural_and_a_bad_value_is_not() {
    // The split the UI relies on: a misspelled key is repairable (rewrite the
    // entry), while a command that is not on PATH is a value the user must edit.
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_exe(&dir, "app.exe");
    let body = format!(
        "[\"typo\"]\n\
         command = '{exe}'\n\
         restart_delay = 3\n\
         \n\
         [\"no_such_tool\"]\n\
         command = 'kandel-no-such-tool-xyz'\n"
    );
    let (_dir, path) = write_config(&body);

    let loaded = load_config_lenient(&path).unwrap();
    assert_eq!(loaded.invalid["typo"].kind, InvalidKind::Structural);
    assert_eq!(loaded.invalid["no_such_tool"].kind, InvalidKind::Value);
}

#[test]
fn adding_a_new_entry_works_alongside_a_broken_one() {
    // The original complaint's second half: "I can't add the new entry". With a
    // broken entry present, a fresh valid entry must still be insertable.
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_exe(&dir, "app.exe");
    let body = format!(
        "[\"broken\"]\n\
         command = '{exe}'\n\
         restart_delay = 3\n"
    );
    let (_dir, path) = write_config(&body);

    let loaded = load_config_lenient(&path).unwrap();
    let mut apps = loaded.apps;
    apps.insert(
        "new".to_string(),
        kandel_config::App {
            name: "new".into(),
            command: exe.clone(),
            ..kandel_config::App::default()
        },
    );
    save_config_preserving(&path, &apps).unwrap();

    let reloaded = load_config_lenient(&path).unwrap();
    assert!(reloaded.apps.contains_key("new"));
    assert!(reloaded.invalid.contains_key("broken"));
}
