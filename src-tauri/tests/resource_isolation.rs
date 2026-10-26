//! An isolated profile really is isolated — every resource path, not just the store.
//!
//! # The bug this pins
//!
//! `scripts/desktop-checkpoint.ps1` exists so desktop acceptance can be observed without risking
//! the developer's own saved words. Its first version redirected `APPDATA` and stopped there, and
//! that is not enough: `resource_dir` resolves
//!
//! ```text
//! SAKATSU_*_DIR override  ->  device_data_dir()/<name>  ->  app_data_dir()/<name>
//! ```
//!
//! and `device_data_dir()` falls through to **`LOCALAPPDATA`**. So the store went to scratch while
//! the dictionaries, kanji table and models went on resolving to the real machine — and because
//! `dictionaries_install_dir()` *is* `dictionaries_dir()`, an import during the checkpoint would
//! have written into them.
//!
//! A comment claiming the script is isolated is worth nothing. This asks the app.
//!
//! # Why a child process
//!
//! The paths come from environment variables read at call time, and a test that sets them in-process
//! would race every other test in the binary. `resolve-resources` is a tiny bin that prints what the
//! app would resolve; running it with a prepared environment is the same question the script asks,
//! answered the same way.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

fn scratch(name: &str) -> common::Scratch {
    let path = common::Scratch::new("resource-isolation", name);
    std::fs::create_dir_all(path.join("Roaming").join("com.sakatsu.app")).expect("roaming");
    std::fs::create_dir_all(path.join("Local").join("com.sakatsu.app")).expect("local");
    path
}

/// What the app would resolve, given these environment variables.
fn resolved(env: &[(&str, Option<&Path>)]) -> BTreeMap<String, String> {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_resolve-resources"));
    for (name, value) in env {
        match value {
            Some(path) => command.env(name, path),
            None => command.env_remove(name),
        };
    }
    let output = command.output().expect("resolve-resources runs");
    assert!(
        output.status.success(),
        "resolve-resources failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // A flat object of string values; parsed without a JSON dependency because that is all it is.
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.trim().trim_end_matches(',').split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches('"');
        let value = value.trim().trim_matches('"').replace("\\\\", "\\");
        if !key.is_empty() && !value.is_empty() && key != "{" {
            out.insert(key.to_string(), value);
        }
    }
    let expected = [
        "audio",
        "device_data",
        "dictionaries",
        "kanji",
        "model",
        "paddle",
        "store",
        "translation",
        "vlm",
        "wordnet",
    ];
    assert_eq!(
        out.keys().map(String::as_str).collect::<Vec<_>>(),
        expected,
        "the isolation gate must account for every resource path printed by the app:\n{text}"
    );
    out
}

fn inside(path: &str, root: &Path) -> bool {
    // Compared case-insensitively and with separators normalised: Windows hands back a mixture of
    // the two, and a comparison that fails on `/` versus `\` would report a leak that is not one.
    let normalise = |text: &str| text.replace('/', "\\").to_lowercase();
    normalise(path).starts_with(&normalise(&root.display().to_string()))
}

/// Redirecting all three puts **every** resource inside the profile.
#[test]
fn a_fully_redirected_profile_keeps_every_resource_inside_it() {
    let root = scratch("full");
    let roaming = root.join("Roaming");
    let local = root.join("Local");
    let device = local.join("com.sakatsu.app");

    let paths = resolved(&[
        ("APPDATA", Some(&roaming)),
        ("LOCALAPPDATA", Some(&local)),
        ("SAKATSU_DEVICE_DATA_DIR", Some(&device)),
        // Cleared, because a stale override in the caller's environment would defeat the whole
        // arrangement — and the script clears them for the same reason.
        ("SAKATSU_DICT_DIR", None),
        ("SAKATSU_KANJI_DIR", None),
    ]);

    for (name, path) in &paths {
        assert!(
            inside(path, &root),
            "{name} resolved to {path}, which is outside the isolated profile at {}",
            root.display()
        );
    }
}

/// **Redirecting `APPDATA` alone does not.** This is the teeth, and it is the bug that was shipped.
///
/// If this ever starts passing, either `resource_dir` stopped consulting `LOCALAPPDATA` — in which
/// case the test above is checking something weaker than it thinks — or the machine has no
/// `LOCALAPPDATA`, which is not a case worth being reassured by.
#[test]
fn redirecting_only_the_roaming_directory_leaks_device_resources() {
    let root = scratch("partial");
    let roaming = root.join("Roaming");

    let paths = resolved(&[
        ("APPDATA", Some(&roaming)),
        // LOCALAPPDATA deliberately left alone, as the first version of the script left it.
        ("SAKATSU_DEVICE_DATA_DIR", None),
        ("SAKATSU_DICT_DIR", None),
        ("SAKATSU_KANJI_DIR", None),
    ]);

    assert!(
        inside(&paths["store"], &root),
        "the store follows APPDATA, which is the half that worked: {}",
        paths["store"]
    );
    let escaped: Vec<&String> = ["dictionaries", "kanji", "device_data"]
        .iter()
        .filter_map(|name| paths.get(*name))
        .filter(|path| !inside(path, &root))
        .collect();
    assert!(
        !escaped.is_empty(),
        "every resource stayed inside the profile with only APPDATA redirected, so this test is \
         no longer demonstrating the leak the checkpoint script exists to avoid: {paths:?}"
    );
}

/// Adding a second binary to this package must not break `cargo tauri dev`.
///
/// # What happened
///
/// `resolve-resources` — the bin the tests above depend on — is the second binary in
/// `sakatsu-desktop`. `cargo tauri dev` runs `cargo run` with no `--bin`, so the moment there were
/// two, it stopped:
///
/// ```text
/// error: `cargo run` could not determine which binary to run.
/// available binaries: resolve-resources, sakatsu-desktop
/// ```
///
/// That is the command `project notes` and `DEVELOPMENT.md` both name as *the* way to run this app, broken
/// by a helper added for a test. `default-run` in `src-tauri/Cargo.toml` names the app binary and
/// resolves it.
///
/// This test is here rather than somewhere tidier because this file is the reason the second
/// binary exists, and a third one added later will break the same command the same way.
#[test]
fn the_package_names_which_binary_cargo_run_should_pick() {
    let manifest = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    )
    .expect("src-tauri/Cargo.toml is readable");

    assert!(
        manifest.contains("default-run = \"sakatsu-desktop\""),
        "src-tauri has more than one binary and no `default-run`, so `cargo run` — which is what \
         `cargo tauri dev` invokes — cannot choose between them. Add \
         `default-run = \"sakatsu-desktop\"` to [package]."
    );
}
