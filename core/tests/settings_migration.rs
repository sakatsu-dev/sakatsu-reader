//! What happens to a settings file when something goes wrong.
//!
//! # Why these come before believing the migration works
//!
//! "Back up before migrating" is not a guarantee until each failure is observable. A migration
//! that backs up *after* publishing, or that leaves a half-written file, or that re-runs on every
//! open, all look identical on a machine where nothing goes wrong — and the thing being protected
//! is a learner's preferences, which they cannot reconstruct if this is wrong.
//!
//! So each row of the acceptance table is a test here, and each one is arranged to fail if the
//! ordering in `store::migrate_settings` is reversed.
//!
//! These need nothing installed.

use std::path::Path;

#[path = "common/scratch.rs"]
mod scratch_guard;

use sakatsu_core::store::{Settings, Store, SETTINGS_VERSION};

fn scratch(name: &str) -> scratch_guard::Scratch {
    scratch_guard::Scratch::new("settings", name)
}

fn write(dir: &Path, json: &str) {
    std::fs::write(dir.join("settings.json"), json).expect("write settings");
}

fn read(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("settings.json")).expect("read settings")
}

/// A file written before the envelope existed is version 0, migrated, and backed up.
#[test]
fn a_pre_envelope_file_is_migrated_and_backed_up() {
    let dir = scratch("v0");
    let original = r#"{"anki_deck": "Japanese::Reading", "font_size": 17.0}"#;
    write(&dir, original);

    let store = Store::open(&dir).expect("open");
    assert_eq!(store.settings().anki_deck, "Japanese::Reading");
    assert_eq!(store.settings().font_size, 17.0);

    let backup = dir.join("settings.json.v0");
    assert!(
        backup.is_file(),
        "the original must be kept before anything is written over it"
    );
    assert_eq!(
        std::fs::read_to_string(&backup).expect("backup"),
        original,
        "and kept byte for byte, not re-serialised"
    );

    let migrated: serde_json::Value =
        serde_json::from_str(&read(&dir)).expect("the migrated file is JSON");
    assert_eq!(migrated["version"], SETTINGS_VERSION);
    assert_eq!(migrated["settings"]["anki_deck"], "Japanese::Reading");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Reopening a migrated file does not migrate it again.
///
/// The check is that the file on disk is untouched — including its backup, which a second
/// migration would either overwrite or duplicate.
#[test]
fn a_migrated_file_is_not_migrated_again() {
    let dir = scratch("twice");
    write(&dir, r#"{"anki_deck": "Deck A"}"#);

    let first = Store::open(&dir).expect("first open");
    drop(first);
    let after_migration = read(&dir);

    // Something only a second migration would do: back up the already-current file.
    let second = Store::open(&dir).expect("second open");
    assert_eq!(second.settings().anki_deck, "Deck A");
    assert_eq!(
        read(&dir),
        after_migration,
        "a second open must not rewrite a file that is already current"
    );
    assert!(
        !dir.join(format!("settings.json.v{SETTINGS_VERSION}"))
            .exists(),
        "and must not back up a file it had no reason to change"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A file from a **newer** build survives being opened, and survives a save.
///
/// **Surviving the load is the easy half.** The half that catches a naive implementation is the
/// next ordinary write: the app saves settings on all sorts of occasions, and one of them would
/// otherwise replace a file whose extra fields this build cannot even see.
#[test]
fn a_future_version_file_survives_startup_and_a_save() {
    let dir = scratch("future");
    let future = format!(
        r#"{{"version": {}, "settings": {{"anki_deck": "From Tomorrow", "a_setting_from_the_future": 42}}}}"#,
        SETTINGS_VERSION + 7
    );
    write(&dir, &future);

    let store = Store::open(&dir).expect("open");
    assert_eq!(
        store.settings().anki_deck,
        "From Tomorrow",
        "what this build can read, it reads"
    );
    assert!(
        store.settings_are_read_only(),
        "and it knows it must not write"
    );
    assert!(
        store
            .note()
            .is_some_and(|note| note.contains("newer version")),
        "the learner is told why, rather than finding out by losing something"
    );

    let mut changed = store.settings();
    changed.anki_deck = "Overwritten".into();
    assert!(
        store.set_settings(changed).is_err(),
        "a save must be refused, not silently dropped: the interface has to be able to say why"
    );

    assert_eq!(
        read(&dir),
        future,
        "and the file is byte-identical, including the field this build cannot see"
    );
    assert!(
        !dir.join("settings.json.unreadable").exists(),
        "it must not be set aside either — it is readable, just newer"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// One invalid value costs that value and nothing else.
///
/// Before this, `Settings` failed to deserialize as a whole, the file was set aside, and the
/// learner got defaults for their deck, their palettes, their bindings and their chords — none of
/// which was wrong.
#[test]
fn one_invalid_field_does_not_cost_the_others() {
    let dir = scratch("invalid");
    write(
        &dir,
        r#"{"anki_deck": "Kept", "font_size": "much too big", "furigana": false}"#,
    );

    let store = Store::open(&dir).expect("open");
    let settings = store.settings();
    assert_eq!(settings.anki_deck, "Kept", "an unrelated string survives");
    assert!(!settings.furigana, "an unrelated bool survives");
    assert_eq!(
        settings.font_size,
        Settings::default().font_size,
        "and only the broken one is returned to its default"
    );
    assert!(
        store.note().is_some_and(|note| note.contains("font_size")),
        "the note names the field, because \"your settings were reset\" is a different sentence"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A document that is not JSON at all is still set aside whole, as it always was.
#[test]
fn a_file_that_is_not_json_is_still_set_aside() {
    let dir = scratch("garbage");
    write(&dir, "this is not JSON {{{{");

    let store = Store::open(&dir).expect("open");
    assert_eq!(store.settings(), {
        let mut defaults = Settings::default();
        defaults.normalise();
        defaults
    });
    assert!(
        dir.join("settings.json.unreadable").is_file(),
        "there is nothing to recover field by field, so the file is preserved whole"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A backup that cannot be written stops the migration, and the original is untouched.
///
/// Arranged by making the backup path a **directory**, so creating a file there fails. That is
/// the only part of the ordering that can be tested without a fault-injecting filesystem, and it
/// is the part that matters: publish-before-backup would leave the original already gone.
#[test]
fn a_backup_that_cannot_be_written_leaves_the_original_alone() {
    let dir = scratch("nobackup");
    let original = r#"{"anki_deck": "Precious"}"#;
    write(&dir, original);
    std::fs::create_dir(dir.join("settings.json.v0")).expect("occupy the backup path");

    let store = Store::open(&dir).expect("open still succeeds — the settings were readable");
    assert_eq!(
        store.settings().anki_deck,
        "Precious",
        "the settings are still read"
    );
    assert_eq!(
        read(&dir),
        original,
        "and the file on disk is untouched, because the backup came first and failed"
    );
    assert!(
        store
            .note()
            .is_some_and(|note| note.contains("could not be updated")),
        "and the learner is told the format was not updated: {:?}",
        store.note()
    );

    // The next open tries again, which is why an interrupted migration needs no recovery state.
    std::fs::remove_dir(dir.join("settings.json.v0")).expect("free the backup path");
    let retried = Store::open(&dir).expect("open");
    assert_eq!(retried.settings().anki_deck, "Precious");
    let migrated: serde_json::Value =
        serde_json::from_str(&read(&dir)).expect("JSON after the retry");
    assert_eq!(
        migrated["version"], SETTINGS_VERSION,
        "a retry completes what the failure left undone"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The version envelope is not mistaken for a setting, and a setting is not mistaken for it.
#[test]
fn only_a_real_envelope_is_read_as_one() {
    let dir = scratch("shape");
    // A `version` key with no `settings` body is a v0 file that happens to contain that word.
    // Reading it as an envelope would throw every other field away.
    write(&dir, r#"{"version": 99, "anki_deck": "Not An Envelope"}"#);
    let store = Store::open(&dir).expect("open");
    assert_eq!(
        store.settings().anki_deck,
        "Not An Envelope",
        "both keys are required before a document is treated as an envelope"
    );
    assert!(
        !store.settings_are_read_only(),
        "and a stray version number must not lock the file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A valid legacy alias survives beside an unrelated invalid field.
///
/// # The bug this pins
///
/// `field_by_field` starts from `Settings::default()` serialised, so **every canonical key is
/// already in the map**. Inserting a legacy alias then hands serde two keys for one field —
/// `collect_sentences` and its `#[serde(alias = "log_content")]` — which is a duplicate-field
/// error. The alias is rejected as unreadable and the learner's real setting is replaced by a
/// default.
///
/// So the recovery path damaged exactly the files it exists to rescue: an old settings file with
/// one hand-edited value lost its aliased settings too, silently, and the note said those fields
/// "could not be read" when they were perfectly readable.
#[test]
fn a_valid_legacy_alias_survives_beside_an_invalid_field() {
    let dir = scratch("alias");
    // `log_content` is the pre-rename name of `collect_sentences`. `font_size` is nonsense, which
    // is what sends the whole file down the field-by-field path in the first place.
    write(
        &dir,
        r#"{"log_content": true, "font_size": "much too big", "anki_deck": "Kept"}"#,
    );

    let store = Store::open(&dir).expect("open");
    let settings = store.settings();
    assert!(
        settings.collect_sentences,
        "a valid legacy alias must be read, not discarded: the whole point of the alias is that \
         an existing install keeps its answer"
    );
    assert_eq!(
        settings.anki_deck, "Kept",
        "an unrelated valid field survives"
    );
    assert_eq!(
        settings.font_size,
        Settings::default().font_size,
        "and the genuinely broken one is defaulted"
    );

    let note = store.note().unwrap_or_default().to_string();
    assert!(
        note.contains("font_size"),
        "the note names the broken field: {note:?}"
    );
    assert!(
        !note.contains("log_content"),
        "and must NOT claim the alias was unreadable — it was read: {note:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// An ordinary save after a failed backup is refused, and the original is still there.
///
/// # Why this is the hole and not a detail
///
/// `Store::open` records a failed migration backup as a note and carries on, which is right — the
/// settings were readable and the app should start. But `set_settings` then wrote the envelope
/// with no backup and no retry, so the **first ordinary save** overwrote an original that had
/// never been preserved. The backup guarantee held for exactly as long as nobody touched a
/// setting.
#[test]
fn a_save_after_a_failed_backup_is_refused_and_the_original_survives() {
    let dir = scratch("save-nobackup");
    let original = r#"{"anki_deck": "Precious"}"#;
    write(&dir, original);
    std::fs::create_dir(dir.join("settings.json.v0")).expect("occupy the backup path");

    let store = Store::open(&dir).expect("open");
    assert_eq!(store.settings().anki_deck, "Precious");
    assert_eq!(read(&dir), original, "open alone leaves the original alone");

    let mut changed = store.settings();
    changed.anki_deck = "Overwritten".into();
    assert!(
        store.set_settings(changed).is_err(),
        "a save must be refused while the original is unbacked-up, not silently written"
    );
    assert_eq!(
        read(&dir),
        original,
        "and the original must still be exactly what it was"
    );

    // Free the backup path: the save now succeeds, having taken the backup it owed.
    std::fs::remove_dir(dir.join("settings.json.v0")).expect("free the backup path");
    let mut changed = store.settings();
    changed.anki_deck = "Now Saved".into();
    store
        .set_settings(changed)
        .expect("a save succeeds once the backup can be taken");
    assert_eq!(
        std::fs::read_to_string(dir.join("settings.json.v0")).expect("backup"),
        original,
        "the backup is the original, taken late rather than never"
    );
    let saved: serde_json::Value =
        serde_json::from_str(&read(&dir)).expect("the saved file is JSON");
    assert_eq!(saved["version"], SETTINGS_VERSION);
    assert_eq!(saved["settings"]["anki_deck"], "Now Saved");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_settings_survive_migration_save_and_restart() {
    for versioned in [false, true] {
        let dir = scratch(if versioned {
            "unknown-v1"
        } else {
            "unknown-v0"
        });
        let body = serde_json::json!({
            "future_option": {"keep": [1, 2, 3]},
            "log_content": true,
            "font_size": "invalid",
            "theme": {"future_colour": "indigo"},
            "theme_presets": [
                {"name": "A", "theme": {"future_colour": "amber"}, "future_preset": 17},
                {"name": "B", "theme": {}, "future_preset": 23}
            ]
        });
        let document = if versioned {
            serde_json::json!({"version": 1, "settings": body, "future_metadata": [7, 8]})
        } else {
            body
        };
        write(&dir, &document.to_string());
        let store = Store::open(&dir).unwrap();
        let mut settings = store.settings();
        assert!(settings.collect_sentences);
        settings.anki_deck = "Edited".into();
        settings.theme_presets.reverse();
        store.set_settings(settings).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&read(&dir)).unwrap();
        assert_eq!(
            saved["settings"]["future_option"],
            serde_json::json!({"keep": [1, 2, 3]})
        );
        assert_eq!(saved["settings"]["theme"]["future_colour"], "indigo");
        assert_eq!(saved["settings"]["theme_presets"][0]["future_preset"], 23);
        assert_eq!(
            saved["settings"]["theme_presets"][1]["theme"]["future_colour"],
            "amber"
        );
        assert!(
            saved["settings"].get("log_content").is_none(),
            "aliases must not return beside canonical keys"
        );
        assert!(
            saved["settings"]["font_size"].is_number(),
            "a repaired known field must stay repaired"
        );
        if versioned {
            assert_eq!(saved["future_metadata"], serde_json::json!([7, 8]));
        }
        let reopened = Store::open(&dir).unwrap();
        assert_eq!(reopened.settings().anki_deck, "Edited");
        assert!(reopened.settings().collect_sentences);
        reopened.set_settings(reopened.settings()).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&read(&dir)).unwrap(),
            saved
        );
    }
}
