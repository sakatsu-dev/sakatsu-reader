use sakatsu_core::dict::{managed, Dictionary};
use sakatsu_core::setup::install::{prepare_missing_dictionaries, InstallHooks, InstallPhase};
use std::io::Write;
use std::path::Path;
use std::sync::{atomic::AtomicBool, Arc};
#[path = "common/scratch.rs"]
mod scratch;

fn archive(path: &Path, title: &str, revision: &str, term: &str) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("index.json", options).unwrap();
    zip.write_all(serde_json::json!({"title":title,"revision":revision,"format":3,"isUpdatable":true,"indexUrl":"https://example.com/index","downloadUrl":"https://example.com/dictionary.zip"}).to_string().as_bytes()).unwrap();
    zip.start_file("term_bank_1.json", options).unwrap();
    zip.write_all(
        serde_json::json!([[term, term, "n", "", 0, ["fixture definition"], 1, ""]])
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    zip.finish().unwrap();
}
struct Hooks;
impl InstallHooks for Hooks {
    fn detail(&mut self, _: &str, _: &str) {}
    fn phase(&mut self, _: InstallPhase) {}
    fn verify_model(&mut self, _: &str, _: &Path) -> Result<(), String> {
        panic!("not a model")
    }
    fn install_runtime(&mut self, _: &Path) -> Result<String, String> {
        panic!("not a runtime")
    }
}

#[test]
fn bundled_install_fills_missing_dictionary_without_downgrading_an_existing_one() {
    let root = scratch::Scratch::new("bundled-dictionaries", "preservation");
    let work = root.join("work");
    std::fs::create_dir(&work).unwrap();
    archive(
        &work.join("jitendex-yomitan.zip"),
        "Bundled old title",
        "2026.10.03.0",
        "old",
    );
    archive(
        &work.join("JMnedict.zip"),
        "Bundled names",
        "JMnedict.2026-10-18",
        "name",
    );
    let newer = root.join("newer.zip");
    archive(&newer, "User-selected title", "2099.01.01.0", "new");
    let live = root.join("installed");
    let held = Dictionary::import(&newer, &live.join("jitendex")).unwrap();
    let original = held.root().to_path_buf();
    let (prepared, _) = prepare_missing_dictionaries(
        &mut Hooks,
        sakatsu_core::setup::catalog::find("dictionaries").unwrap(),
        &live,
        &work,
        &Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(
        prepared.len(),
        1,
        "only the missing names dictionary is prepared"
    );
    std::fs::create_dir(root.join("transaction")).unwrap();
    sakatsu_core::setup::publication::publish_offline(&root.join("transaction"), &[], prepared)
        .unwrap();
    let kept = Dictionary::open(&live.join("jitendex")).unwrap();
    assert_eq!(kept.root(), original);
    assert_eq!(kept.meta().revision, "2099.01.01.0");
    assert_eq!(kept.lookup("new").unwrap().len(), 1);
    assert!(kept.lookup("old").unwrap().is_empty());
    assert_eq!(
        Dictionary::open(&live.join("jmnedict"))
            .unwrap()
            .lookup("name")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(held.lookup("new").unwrap().len(), 1);
    let (none, _) = prepare_missing_dictionaries(
        &mut Hooks,
        sakatsu_core::setup::catalog::find("dictionaries").unwrap(),
        &live,
        &work,
        &Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(none.is_empty(), "repeated bundled installation is a no-op");
}

#[test]
fn publisher_metadata_and_source_bytes_survive_import_and_reopen() {
    let root = scratch::Scratch::new("bundled-dictionaries", "metadata");
    let zip = root.join("dictionary.zip");
    archive(&zip, "Dated title", "revision-a", "word");
    assert_eq!(managed::inspect_words(&zip).unwrap().revision, "revision-a");
    Dictionary::import(&zip, &root.join("installed")).unwrap();
    let dict = Dictionary::open(&root.join("installed")).unwrap();
    assert!(dict.meta().is_updatable);
    assert_eq!(
        dict.meta().index_url.as_deref(),
        Some("https://example.com/index")
    );
    assert_eq!(
        dict.meta().source_sha256.as_deref(),
        Some(managed::sha256(&zip).unwrap().as_str())
    );
    assert!(serde_json::from_str::<sakatsu_core::dict::DictionaryMeta>(
        r#"{"title":"Historical","format":3}"#
    )
    .unwrap()
    .index_url
    .is_none());
}

#[test]
fn non_word_archives_and_unsafe_members_are_refused() {
    let root = scratch::Scratch::new("bundled-dictionaries", "bad-input");
    for (case, member) in [
        ("kanji", "kanji_bank_1.json"),
        ("unsafe", "../term_bank_1.json"),
    ] {
        let path = root.join(format!("{case}.zip"));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file(member, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"[]").unwrap();
        zip.finish().unwrap();
        assert!(managed::inspect_words(&path).is_err(), "{case}");
    }
}
