use std::fs;
use std::path::{Path, PathBuf};

use sakatsu_desktop_lib::commands::dictionary_archive_in;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sakatsu-path-boundary-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn dictionary_import_accepts_only_a_zip_directly_inside_its_directory() {
    let scratch = Scratch::new();
    let dictionaries = scratch.path().join("dictionaries");
    fs::create_dir(&dictionaries).expect("dictionary directory");
    let offered = dictionaries.join("jitendex-yomitan.zip");
    fs::write(&offered, b"zip placeholder").expect("offered archive");

    assert_eq!(
        dictionary_archive_in(&dictionaries, "jitendex-yomitan.zip").expect("listed zip"),
        offered.canonicalize().expect("canonical archive")
    );
    assert!(dictionary_archive_in(&dictionaries, "../jitendex-yomitan.zip").is_err());
    assert!(dictionary_archive_in(&dictionaries, offered.to_string_lossy().as_ref()).is_err());
    assert!(dictionary_archive_in(&dictionaries, "jitendex-yomitan.json").is_err());
    assert!(dictionary_archive_in(&dictionaries, "missing.zip").is_err());
}

/// **A fetch must write where the matching loader reads.**
///
/// These were two functions with two rules: `resource_dir` picked the legacy roaming root when
/// it held usable content, while `install_dir` always picked the new local one. They agreed on
/// a fresh machine and disagreed on an upgraded one, which is the worst way round — the case
/// nobody tests. A dictionary fetched there landed in a directory the loader never opened, so
/// the ones already imported vanished from lookups; a frequency list, whose directory never
/// satisfies the dictionary predicate, downloaded successfully and left ranking silently off.
///
/// Each pair now shares one resolution. This pins that, so a separate install root cannot come
/// back without the test that says why it must not.
#[test]
fn every_fetch_lands_where_its_loader_looks() {
    use sakatsu_desktop_lib as app;

    let pairs: [(&str, PathBuf, PathBuf); 7] = [
        ("kanji", app::kanji_dir(), app::kanji_install_dir()),
        ("audio", app::audio_dir(), app::audio_install_dir()),
        ("wordnet", app::wordnet_dir(), app::wordnet_install_dir()),
        ("manga-ocr", app::model_dir(), app::model_install_dir()),
        (
            "paddle-ocr",
            app::paddle_model_dir(),
            app::paddle_model_install_dir(),
        ),
        (
            "translation",
            app::translation_model_dir(),
            app::translation_model_install_dir(),
        ),
        (
            "dictionaries",
            app::dictionaries_dir(),
            app::dictionaries_install_dir(),
        ),
    ];

    for (name, load, install) in pairs {
        assert_eq!(
            load,
            install,
            "{name}: a fetch writing to {} while the loader reads {} installs into a \
             directory nothing opens",
            install.display(),
            load.display()
        );
    }
}
