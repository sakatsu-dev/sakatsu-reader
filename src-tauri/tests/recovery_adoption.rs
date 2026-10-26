//! A recovered resource has to be usable in the session that recovered it.
//!
//! # The gap this covers
//!
//! Every resource an interrupted install could restore is read into a handle **before** recovery
//! runs: the dictionaries and the frequency list before the Tauri builder, the kanji table and the
//! particle senses as managed state. Recovery lives in `.setup()`, because it must not touch the
//! install directories until the single-instance plugin has decided this process owns them — which
//! puts it after all of that.
//!
//! So a recovery that only repaired the disk would leave the session serving the empty table it
//! loaded seconds earlier, with nothing on screen to say why. The reader would restore an
//! installation and find it still missing until they restarted, which is the kind of fix that reads
//! as a different bug.
//!
//! # Why a child process
//!
//! The resource directories are resolved from the environment, and setting variables in-process
//! would leak into every other test in this binary. `performance.rs` and `resource_isolation.rs`
//! both spawn instead; this does the same, with the parent planting the interrupted state and the
//! child proving a fresh session picks it up.
//!
//! # The journal is the publisher's, not the test's
//!
//! `publish_with_commit` writes `publication.json` before its first rename, and that is where the
//! interrupted state's evidence comes from here: the fixture is produced by **running the
//! publisher** and then reproducing what a stopped process leaves. An earlier version wrote the
//! file by hand — in the right format, which is not the same as being the publisher's output, and
//! would have gone on passing if the publisher's format moved.

use std::path::{Path, PathBuf};

mod common;
use common::interrupt_a_real_publication;

const CHILD: &str = "SAKATSU_RECOVERY_ADOPTION_CHILD";
const SELECTED: &str = "SAKATSU_RECOVERY_ADOPTION_SELECTED";
/// The outcome the report must carry, named by the test rather than derived in the child — a
/// child that works out what to expect from the same inputs it is checking proves less.
const EXPECT: &str = "SAKATSU_RECOVERY_ADOPTION_EXPECT";

/// One kanji, in the tab-separated shape `KanjiDic::load` reads.
const KANJI_ROW: &str = "語\tゴ\tかた-る\n";

fn scratch(name: &str) -> common::Scratch {
    common::Scratch::new("recovery-adoption", name)
}

/// The three roots a session resolves its resources from, redirected into one scratch profile.
struct Profile {
    root: common::Scratch,
    roaming: PathBuf,
    local: PathBuf,
    device: PathBuf,
}

fn profile(name: &str) -> Profile {
    let root = scratch(name);
    let roaming = root.join("Roaming");
    let local = root.join("Local");
    let device = local.join("com.sakatsu.app");
    std::fs::create_dir_all(&device).expect("device data");
    std::fs::create_dir_all(roaming.join("com.sakatsu.app")).expect("roaming");
    Profile {
        root,
        roaming,
        local,
        device,
    }
}

fn run_child(test: &str, profile: &Profile, extra: &[(&str, &str)]) -> std::process::Output {
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let mut command = std::process::Command::new(&exe);
    command
        .args([test, "--exact", "--nocapture"])
        .env(CHILD, &*profile.root)
        .env("APPDATA", &profile.roaming)
        .env("LOCALAPPDATA", &profile.local)
        .env("SAKATSU_DEVICE_DATA_DIR", &profile.device)
        // Cleared, so a stale override in whoever ran the suite cannot send the resources
        // somewhere else and make this pass for the wrong reason.
        .env_remove("SAKATSU_KANJI_DIR")
        .env_remove("SAKATSU_DICT_DIR")
        .env_remove("SAKATSU_WORDNET_DIR");
    for (key, value) in extra {
        command.env(key, value);
    }
    command.output().expect("spawn a fresh session")
}

/// Every resource path this session resolves is inside its own profile.
///
/// **Asserted before recovery runs, and not merely "the table is absent".** An absent table would
/// also be true of an unintended path outside the profile that happens to be empty — and the first
/// version of this test resolved the developer's own installed kanji table, which it would have
/// gone on doing silently. Nothing was written there, but reading real data was never the intent
/// and the assertion that would have caught it is this one.
fn assert_inside_the_profile(root: &Path) {
    let normalise = |path: &Path| path.display().to_string().replace('/', "\\").to_lowercase();
    let inside = normalise(root);
    for (what, path) in [
        ("kanji", sakatsu_desktop_lib::kanji_dir()),
        ("dictionaries", sakatsu_desktop_lib::dictionaries_dir()),
        ("wordnet", sakatsu_desktop_lib::wordnet_dir()),
        ("device data", sakatsu_desktop_lib::device_data_dir()),
    ] {
        assert!(
            normalise(&path).starts_with(&inside),
            "{what} resolved to {}, which is outside the scratch profile at {}",
            path.display(),
            root.display()
        );
    }
}

#[test]
fn a_recovered_resource_is_usable_without_another_restart() {
    if let Some(root) = std::env::var_os(CHILD) {
        return kanji_child(Path::new(&root));
    }

    let profile = profile("kanji");
    let interrupted = profile.device.join(".offline-setup-99999-1");
    std::fs::create_dir_all(&interrupted).expect("interrupted root");
    let destination = profile.device.join("kanji");
    interrupt_a_real_publication(
        &interrupted,
        "kanji",
        &destination,
        &[("kanjidic.tsv", KANJI_ROW)],
    );
    assert!(
        !destination.exists(),
        "the destination is missing, which is what makes this the interrupted case"
    );

    let output = run_child(
        "a_recovered_resource_is_usable_without_another_restart",
        &profile,
        &[],
    );
    assert!(
        output.status.success(),
        "the child session did not take up the recovered table:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        destination.join("kanjidic.tsv").is_file(),
        "and it was restored on disk as well as in memory"
    );
}

fn kanji_child(root: &Path) {
    use sakatsu_desktop_lib::commands::KanjiState;
    use tauri::Manager;

    assert_inside_the_profile(root);
    let table = sakatsu_desktop_lib::kanji_dir().join("kanjidic.tsv");
    assert!(!table.exists(), "the child begins with nothing there");

    // Loaded before recovery, exactly as the real startup order does it.
    let app = tauri::test::mock_builder()
        .manage(KanjiState::load(table))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build a mock session");
    assert!(
        app.state::<KanjiState>().0.now().get('語').is_none(),
        "the handle really was loaded from an empty table"
    );

    sakatsu_desktop_lib::recover_interrupted_offline_installs(app.handle());

    assert!(
        app.state::<KanjiState>().0.now().get('語').is_some(),
        "the recovered table must be usable in this session, not the next one"
    );

    // **And the report says so, rather than telling everybody to restart.** The panel used to
    // describe every restored component that way, which was the safe direction and not the true
    // one; this is the claim that had to become accurate.
    let reported = sakatsu_desktop_lib::outstanding_install_recoveries();
    let kanji = reported
        .iter()
        .find(|entry| entry.id == "kanji")
        .expect("the restoration is reported");
    assert_eq!(
        kanji.state,
        sakatsu_core::recovery::InstallRecoveryState::Restored
    );
    assert_eq!(
        kanji.loading,
        sakatsu_core::recovery::LoadingOutcome::Loaded,
        "the handle holds it, so this is established rather than assumed: {kanji:?}"
    );
}

/// A restoration whose files are there and whose loading produces nothing.
///
/// **The outcome a caller cannot see.** `adopt` reports that a reload was attempted; a reload that
/// ran over an unusable file looks identical from its side. Asking the handle afterwards is what
/// separates them, and this is the case where the reader most needs the truth — their files are
/// back and the app still cannot use them.
#[test]
fn a_restoration_whose_loading_fails_is_reported_as_such() {
    if let Some(root) = std::env::var_os(CHILD) {
        return failed_loading_child(Path::new(&root));
    }

    let profile = profile("load-fails");
    let interrupted = profile.device.join(".offline-setup-99999-3");
    std::fs::create_dir_all(&interrupted).expect("interrupted root");
    let destination = profile.device.join("kanji");
    // A table with nothing in it: restored perfectly, and useless.
    interrupt_a_real_publication(&interrupted, "kanji", &destination, &[("kanjidic.tsv", "")]);

    let output = run_child(
        "a_restoration_whose_loading_fails_is_reported_as_such",
        &profile,
        &[],
    );
    assert!(
        output.status.success(),
        "{}
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        destination.join("kanjidic.tsv").is_file(),
        "the files really were put back — that is what makes the loading failure the story"
    );
}

fn failed_loading_child(root: &Path) {
    use sakatsu_desktop_lib::commands::KanjiState;

    assert_inside_the_profile(root);
    let app = tauri::test::mock_builder()
        .manage(KanjiState::load(
            sakatsu_desktop_lib::kanji_dir().join("kanjidic.tsv"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build a mock session");

    sakatsu_desktop_lib::recover_interrupted_offline_installs(app.handle());

    let reported = sakatsu_desktop_lib::outstanding_install_recoveries();
    let kanji = reported
        .iter()
        .find(|entry| entry.id == "kanji")
        .expect("the restoration is reported");
    assert_eq!(
        kanji.state,
        sakatsu_core::recovery::InstallRecoveryState::Restored,
        "the files were restored: {kanji:?}"
    );
    assert_eq!(
        kanji.loading,
        sakatsu_core::recovery::LoadingOutcome::Failed,
        "but nothing loaded, and saying otherwise is the claim this exists to stop: {kanji:?}"
    );
    assert!(
        kanji
            .detail
            .as_deref()
            .is_some_and(|why| why.contains("characters")),
        "and the reason travels with it: {kanji:?}"
    );
}

/// WordNet categories can be reloaded, but `Categories` exposes no honest emptiness check.
///
/// This drives the production recovery sweep so `unknown` is a backend outcome rather than only a
/// frontend fixture. It deliberately does not turn "the reload function returned" into "loaded".
#[test]
fn a_recovered_wordnet_table_reports_that_loading_could_not_be_established() {
    if let Some(root) = std::env::var_os(CHILD) {
        return wordnet_child(Path::new(&root));
    }

    let profile = profile("wordnet-unknown");
    let interrupted = profile.device.join(".offline-setup-99999-4");
    std::fs::create_dir_all(&interrupted).expect("interrupted root");
    let destination = profile.device.join("wordnet");
    interrupt_a_real_publication(
        &interrupted,
        "wordnet",
        &destination,
        &[("categories.tsv", "00001740-n\tartifact\n")],
    );

    let output = run_child(
        "a_recovered_wordnet_table_reports_that_loading_could_not_be_established",
        &profile,
        &[],
    );
    assert!(
        output.status.success(),
        "the child did not report the backend's unknown outcome:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn wordnet_child(root: &Path) {
    use sakatsu_core::recovery::{InstallRecoveryState, LoadingOutcome};

    assert_inside_the_profile(root);
    let table = sakatsu_desktop_lib::wordnet_dir().join("categories.tsv");
    assert!(!table.exists(), "the session begins before the restoration");
    let app = tauri::test::mock_builder()
        .manage(sakatsu_desktop_lib::commands::ParticleState::load(
            table.clone(),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build a mock session");

    sakatsu_desktop_lib::recover_interrupted_offline_installs(app.handle());

    let entry = sakatsu_desktop_lib::outstanding_install_recoveries()
        .into_iter()
        .find(|entry| entry.id == "wordnet")
        .expect("the restoration is reported");
    assert_eq!(entry.state, InstallRecoveryState::Restored, "{entry:?}");
    assert_eq!(
        entry.loading,
        LoadingOutcome::Unknown,
        "the backend must preserve what it could not establish: {entry:?}"
    );
    assert!(
        table.is_file(),
        "the bytes were restored even though loading is unknown"
    );
}

/// A recovered frequency list is taken up **when it is the one the reader chose**.
///
/// `commands::adopt` ignores `UnpackFrequency` on purpose — fetching a list is not choosing one —
/// and recovery inherited that silence. But recovery can put back the list
/// `settings.frequency_source` already names, and the session is then holding an empty frequency
/// state loaded before those files existed: the same stale handle the kanji table had, arriving
/// through the one component `adopt` declines to touch.
#[test]
fn a_recovered_frequency_list_is_taken_up_when_it_is_the_selected_one() {
    if let Some(root) = std::env::var_os(CHILD) {
        return frequency_child(Path::new(&root));
    }
    let profile = profile("frequency-selected");
    plant_interrupted_frequency_list(&profile, USABLE_LIST);

    let output = run_child(
        "a_recovered_frequency_list_is_taken_up_when_it_is_the_selected_one",
        &profile,
        &[(SELECTED, "jpdb"), (EXPECT, "loaded")],
    );
    assert!(
        output.status.success(),
        "the selected list was not taken up:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// And a recovered list nobody selected does **not** become the selection.
///
/// The control, and the property `adopt` is protecting: restoring files must not change what the
/// reader chose. Without it, "reload the restored list" would be indistinguishable from "select the
/// restored list", which is a different and unwanted behaviour.
#[test]
fn a_recovered_frequency_list_that_is_not_selected_changes_nothing() {
    if let Some(root) = std::env::var_os(CHILD) {
        return frequency_child(Path::new(&root));
    }
    let profile = profile("frequency-unselected");
    plant_interrupted_frequency_list(&profile, USABLE_LIST);

    let output = run_child(
        "a_recovered_frequency_list_that_is_not_selected_changes_nothing",
        &profile,
        // The reader has chosen nothing, so the restored list must stay unloaded — and the report
        // must say that nothing needed doing rather than that something failed.
        &[(SELECTED, ""), (EXPECT, "not_needed")],
    );
    assert!(
        output.status.success(),
        "the unselected list changed something it should not have:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// **The selected list that comes back unusable.**
///
/// The case the report used to get wrong in the direction that matters: the files are restored, the
/// reader's chosen list is still empty, and the panel said `not_needed` — "nothing needed doing" to
/// somebody whose word list had just failed to come back. Restoring and being able to read are
/// different claims, and this is where they come apart while everything else looks like success.
#[test]
fn a_selected_frequency_list_that_cannot_be_read_is_reported_as_failed() {
    if let Some(root) = std::env::var_os(CHILD) {
        return frequency_child(Path::new(&root));
    }
    let profile = profile("frequency-unusable");
    plant_interrupted_frequency_list(&profile, UNUSABLE_LIST);

    let output = run_child(
        "a_selected_frequency_list_that_cannot_be_read_is_reported_as_failed",
        &profile,
        &[(SELECTED, "jpdb"), (EXPECT, "failed")],
    );
    assert!(
        output.status.success(),
        "a selected list that came back unreadable was not reported as failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A term-meta bank, which is what `Frequencies::load` actually reads.
const USABLE_LIST: (&str, &str) = ("term_meta_bank_1.json", r#"[["語","freq",1]]"#);

/// Restored bytes that are not a word list. The install put files back; there is still nothing to
/// read, which is exactly the state a truncated download or a half-written bank leaves.
const UNUSABLE_LIST: (&str, &str) = ("notes.txt", "not a term meta bank");

/// An interrupted install of `frequency-jpdb`, with the publisher's own journal.
fn plant_interrupted_frequency_list(profile: &Profile, file: (&str, &str)) {
    let interrupted = profile.device.join(".offline-setup-99999-2");
    std::fs::create_dir_all(&interrupted).expect("interrupted root");
    // `destination("frequency-jpdb")` is `dictionaries/frequency/jpdb` — the id and the directory
    // name deliberately differ, and `FrequencyState::load` reads the directory the *setting* names.
    let destination = profile
        .device
        .join("dictionaries")
        .join("frequency")
        .join("jpdb");
    // Which file is planted decides whether the restored list can be read at all — a name other
    // than a term-meta bank leaves the list empty, which is the unusable fixture and would
    // otherwise have made the unselected control pass for the wrong reason.
    interrupt_a_real_publication(&interrupted, "frequency-jpdb", &destination, &[file]);
}

fn frequency_child(root: &Path) {
    use sakatsu_core::recovery::LoadingOutcome;
    use sakatsu_desktop_lib::commands::FrequencyState;
    use tauri::Manager;

    assert_inside_the_profile(root);
    let chosen = std::env::var(SELECTED).unwrap_or_default();

    let store = sakatsu_desktop_lib::open_store(&root.join("store"));
    if let Some(opened) = store.store.as_ref() {
        let mut settings = opened.settings();
        settings.frequency_source = chosen.clone();
        opened.set_settings(settings).expect("save the selection");
    }

    let app = tauri::test::mock_builder()
        .manage(store)
        .manage(FrequencyState::load(
            sakatsu_desktop_lib::dictionaries_dir(),
            &chosen,
            sakatsu_core::dict::frequency::Direction::default(),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build a mock session");

    let loaded_before = app
        .state::<FrequencyState>()
        .0
        .read()
        .map(|held| held.is_some())
        .unwrap_or(false);
    assert!(
        !loaded_before,
        "the session must start with nothing loaded, or this proves nothing"
    );

    sakatsu_desktop_lib::recover_interrupted_offline_installs(app.handle());

    let loaded_after = app
        .state::<FrequencyState>()
        .0
        .read()
        .map(|held| held.is_some())
        .unwrap_or(false);

    // **The reported outcome, not only the handle.** The handle says what this session holds; the
    // report is what the reader is shown, and the two came apart in exactly one direction — a
    // selected list that failed to load held nothing and was reported as `not_needed`.
    let reported = sakatsu_desktop_lib::outstanding_install_recoveries();
    let entry = reported
        .iter()
        .find(|entry| entry.id == "frequency-jpdb")
        .expect("the restoration is reported at all");
    let expected = match std::env::var(EXPECT).unwrap_or_default().as_str() {
        "loaded" => LoadingOutcome::Loaded,
        "not_needed" => LoadingOutcome::NotNeeded,
        "failed" => LoadingOutcome::Failed,
        other => panic!("this test has no expectation named {other}"),
    };
    assert_eq!(entry.loading, expected, "{entry:?}");

    match expected {
        LoadingOutcome::Loaded => assert!(
            loaded_after,
            "the reader had already chosen this list and was getting nothing"
        ),
        LoadingOutcome::NotNeeded => assert!(
            !loaded_after,
            "restoring a list nobody selected must not select it"
        ),
        LoadingOutcome::Failed => {
            assert!(
                !loaded_after,
                "a list reported as failed must not also be in use"
            );
            assert!(
                entry
                    .detail
                    .as_deref()
                    .is_some_and(|why| why.contains("no entries")),
                "and the reason travels with it, because the files being back is the confusing \
                 part: {entry:?}"
            );
            // The selection is the reader's, and a failed reload is not a reason to change it.
            let still = app
                .state::<sakatsu_desktop_lib::commands::StoreState>()
                .settings()
                .frequency_source;
            assert_eq!(
                still, chosen,
                "a failed reload must not clear the selection"
            );
        }
        LoadingOutcome::Unknown => unreachable!("no fixture here leaves the outcome unestablished"),
    }
}
