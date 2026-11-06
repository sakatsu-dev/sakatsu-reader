//! Production-handler imports, isolated in a child so all resource/profile overrides are local.
use sakatsu_desktop_lib::{commands, dictionary_manager::DictionaryManagerState};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
mod common;

const CHILD: &str = "SAKATSU_DICTIONARY_MANAGER_CHILD";
fn archive(path: &Path, revision: &str, term: &str) {
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, value) in [
        (
            "index.json",
            json!({"title":"Shared dated title", "revision": revision,"format":3,"isUpdatable":true,"indexUrl":"https://example.org/index.json","downloadUrl":"https://example.org/words.zip"}),
        ),
        (
            "term_bank_1.json",
            json!([[
                term,
                term,
                "n",
                "",
                0,
                [format!("definition for {term}")],
                1,
                ""
            ]]),
        ),
    ] {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(value.to_string().as_bytes()).unwrap();
    }
    writer.finish().unwrap();
}
fn call(
    app: &tauri::App<tauri::test::MockRuntime>,
    cmd: &str,
    args: Value,
) -> Result<Value, Value> {
    use tauri::Manager;
    tauri::test::get_ipc_response(
        &app.get_webview_window("main").unwrap(),
        tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        },
    )
    .map(|response| response.deserialize::<Value>().unwrap())
}
fn session(root: &Path) -> tauri::App<tauri::test::MockRuntime> {
    let app = tauri::test::mock_builder()
        .manage(sakatsu_desktop_lib::discover_dictionaries(
            &root.join("dictionaries"),
        ))
        .manage(commands::KanjiState::load(root.join("kanji/kanjidic.tsv")))
        .manage(DictionaryManagerState::default())
        .manage(sakatsu_desktop_lib::warmup::WarmupState::default())
        .invoke_handler(sakatsu_desktop_lib::reader_commands!())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .unwrap();
    app
}
fn complete(app: &tauri::App<tauri::test::MockRuntime>, operation: &Value) -> Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let snapshot = call(app, "dictionary_manager", json!({})).unwrap();
        let op = &snapshot["operation"];
        if op["id"] == *operation
            && matches!(
                op["phase"].as_str(),
                Some("complete" | "failed" | "cancelled")
            )
        {
            return snapshot;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "operation did not complete: {snapshot}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
fn preview(app: &tauri::App<tauri::test::MockRuntime>, path: &Path) -> Value {
    call(app, "dictionary_preview", json!({"file":path})).unwrap()
}
fn import(
    app: &tauri::App<tauri::test::MockRuntime>,
    preview: &Value,
    target: Option<&str>,
) -> Value {
    let operation = call(
        app,
        "dictionary_import",
        json!({"previewId":preview["id"],"target":target}),
    )
    .unwrap();
    complete(app, &operation)
}

#[test]
fn management_round_trip() {
    let Some(root) = std::env::var_os(CHILD) else {
        let scratch = common::Scratch::new("dictionary-manager", "ipc");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["management_round_trip", "--exact", "--nocapture"])
            .env(CHILD, scratch.0.as_path())
            .env("APPDATA", scratch.join("roaming"))
            .env("LOCALAPPDATA", scratch.join("local"))
            .env("SAKATSU_DEVICE_DATA_DIR", scratch.0.as_path())
            .env("SAKATSU_DICT_DIR", scratch.join("dictionaries"))
            .env("SAKATSU_KANJI_DIR", scratch.join("kanji"))
            .output()
            .unwrap();
        let report = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        println!("{report}");
        assert!(output.status.success(), "{report}");
        assert!(
            report.contains("1 passed"),
            "child did not execute its test"
        );
        return;
    };
    let root = std::path::PathBuf::from(root);
    std::fs::create_dir_all(root.join("dictionaries")).unwrap();
    std::fs::create_dir_all(root.join("kanji")).unwrap();
    let original = root.join("original.zip");
    archive(&original, "revision-a", "old");
    sakatsu_core::dict::Dictionary::import(&original, &root.join("dictionaries/fixture")).unwrap();
    let app = session(&root);
    let initial = call(&app, "dictionary_manager", json!({})).unwrap();
    assert_eq!(initial["dictionaries"][0]["id"], "words:fixture");
    assert_eq!(initial["dictionaries"][0]["consented"], false);
    assert_eq!(initial["preferences"]["automatic"], false);
    assert!(call(
        &app,
        "dictionary_source_consent",
        json!({"id":"words:fixture","endpoint":"https://changed.example/","allowed":true})
    )
    .is_err());
    call(
        &app,
        "dictionary_source_consent",
        json!({"id":"words:fixture","endpoint":"https://example.org/index.json","allowed":true}),
    )
    .unwrap();
    assert_eq!(
        call(&app, "dictionary_manager", json!({})).unwrap()["dictionaries"][0]["consented"],
        true
    );
    call(
        &app,
        "dictionary_source_consent",
        json!({"id":"words:fixture","endpoint":"https://example.org/index.json","allowed":false}),
    )
    .unwrap();
    let check = call(&app, "dictionary_check", json!({})).unwrap();
    assert_eq!(
        complete(&app, &check)["operation"]["phase"],
        "complete",
        "no unapproved publisher is contacted"
    );
    assert!(call(&app, "dictionary_update", json!({"id":"words:fixture"})).is_err());
    let next = root.join("replacement.zip");
    archive(&next, "revision-b", "new");
    let proposed = preview(&app, &next);
    assert_eq!(proposed["revision"], "revision-b");
    let outcome = import(&app, &proposed, Some("words:fixture"));
    assert_eq!(outcome["operation"]["phase"], "complete", "{outcome}");
    assert_eq!(outcome["dictionaries"][0]["revision"], "revision-b");
    assert_ne!(
        outcome["dictionaries"][0]["generation"],
        initial["dictionaries"][0]["generation"]
    );
    use tauri::Manager;
    let dictionaries = app.state::<commands::DictionaryState>().0.now();
    assert_eq!(
        dictionaries.dictionaries[0].lookup("new").unwrap().len(),
        1,
        "same session must see the replacement"
    );

    let stale = preview(&app, &original);
    sakatsu_core::dict::Dictionary::import(&next, &root.join("dictionaries/fixture")).unwrap();
    commands::adopt(
        app.handle(),
        sakatsu_core::setup::catalog::find("dictionaries").unwrap(),
    );
    let refused = import(&app, &stale, Some("words:fixture"));
    assert_eq!(refused["operation"]["phase"], "failed");
    assert!(refused["operation"]["detail"]
        .as_str()
        .unwrap()
        .contains("changed"));
    assert_eq!(refused["dictionaries"][0]["revision"], "revision-b");

    let proposed = preview(&app, &original);
    let claim =
        sakatsu_core::setup::claim::claim([sakatsu_core::setup::claim::Claim::Dictionaries])
            .unwrap();
    let refused = import(&app, &proposed, Some("words:fixture"));
    assert_eq!(
        refused["operation"]["phase"], "failed",
        "an existing setup/publication claim must exclude import"
    );
    drop(claim);
    let separate = preview(&app, &original);
    let outcome = import(&app, &separate, None);
    assert_eq!(outcome["operation"]["phase"], "complete");
    assert_eq!(
        outcome["dictionaries"].as_array().unwrap().len(),
        2,
        "same title is not the same installation"
    );
    let dismissed = preview(&app, &next);
    call(
        &app,
        "dictionary_cancel",
        json!({"operationId":dismissed["id"]}),
    )
    .unwrap();
    assert!(call(
        &app,
        "dictionary_import",
        json!({"previewId":dismissed["id"], "target":null})
    )
    .is_err());

    let bad = root.join("bad.zip");
    std::fs::write(&bad, b"not a dictionary").unwrap();
    assert!(call(&app, "dictionary_preview", json!({"file":bad})).is_err());
    call(
        &app,
        "dictionary_preferences",
        json!({"preferences":{"check_updates":false,"automatic":false}}),
    )
    .unwrap();
    assert_eq!(
        call(&app, "dictionary_manager", json!({})).unwrap()["preferences"]["check_updates"],
        false
    );
    assert!(root
        .join("roaming/com.sakatsu.app/dictionary-management.json")
        .is_file());
    let kanji = root.join("kanji");
    std::fs::create_dir_all(kanji.join("kanjivg")).unwrap();
    std::fs::write(kanji.join("kanjivg/fixture.json"), b"diagram marker").unwrap();
    std::fs::write(kanji.join("krad.tsv"), b"component marker").unwrap();
    let gzip = root.join("new-kanjidic.xml.gz");
    const GZIP: &[u8] = &[
        31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 101, 80, 59, 14, 194, 48, 12, 189, 10, 98, 71, 134, 10,
        33, 144, 76, 174, 18, 153, 196, 133, 64, 155, 160, 52, 69, 176, 150, 131, 48, 48, 48, 50,
        114, 160, 78, 220, 130, 20, 40, 80, 49, 217, 239, 147, 103, 59, 184, 33, 187, 54, 218, 168,
        68, 224, 138, 73, 179, 23, 152, 154, 140, 229, 142, 125, 97, 156, 21, 99, 132, 14, 70, 77,
        129, 22, 84, 124, 153, 212, 236, 67, 233, 25, 225, 79, 105, 188, 44, 93, 42, 149, 103, 10,
        13, 147, 12, 147, 201, 96, 52, 28, 36, 211, 167, 189, 43, 34, 180, 27, 168, 21, 121, 82,
        161, 105, 51, 19, 11, 101, 226, 126, 62, 33, 180, 0, 115, 83, 40, 129, 69, 240, 110, 195,
        82, 185, 210, 6, 49, 67, 232, 96, 132, 151, 41, 166, 107, 99, 151, 50, 103, 178, 177, 70,
        34, 95, 122, 87, 110, 63, 74, 207, 203, 112, 216, 242, 188, 191, 38, 233, 108, 95, 212,
        213, 173, 62, 94, 234, 234, 138, 240, 118, 196, 121, 239, 199, 169, 115, 58, 6, 183, 81,
        240, 201, 130, 191, 49, 240, 115, 4, 124, 127, 249, 1, 240, 4, 157, 246, 113, 1, 0, 0,
    ];
    std::fs::write(&gzip, GZIP).unwrap();
    let proposed = preview(&app, &gzip);
    assert_eq!(proposed["kind"], "kanjidic");
    let outcome = import(&app, &proposed, Some("kanjidic2"));
    assert_eq!(outcome["operation"]["phase"], "complete", "{outcome}");
    assert!(std::fs::read_to_string(kanji.join("kanjidic.tsv"))
        .unwrap()
        .contains("\u{98df}"));
    assert_eq!(
        std::fs::read(kanji.join("krad.tsv")).unwrap(),
        b"component marker"
    );
    assert_eq!(
        std::fs::read(kanji.join("kanjivg/fixture.json")).unwrap(),
        b"diagram marker"
    );
    let before = std::fs::read(kanji.join("kanjidic.tsv")).unwrap();
    std::fs::write(&gzip, b"broken gzip").unwrap();
    assert!(call(&app, "dictionary_preview", json!({"file":gzip})).is_err());
    assert_eq!(std::fs::read(kanji.join("kanjidic.tsv")).unwrap(), before);

    // Older installations have neither publisher fields nor an archive receipt. Connecting is
    // explicit and generation-bound; a display title alone must never establish the publisher.
    let legacy =
        sakatsu_core::dict::Dictionary::import(&original, &root.join("dictionaries/legacy"))
            .unwrap();
    let meta_path = legacy.root().join("meta.json");
    let mut meta: Value = serde_json::from_slice(&std::fs::read(&meta_path).unwrap()).unwrap();
    for key in ["isUpdatable", "indexUrl", "downloadUrl", "source_sha256"] {
        meta.as_object_mut().unwrap().remove(key);
    }
    std::fs::write(&meta_path, serde_json::to_vec(&meta).unwrap()).unwrap();
    drop(legacy);
    let legacy_app = session(&root);
    let snapshot = call(&legacy_app, "dictionary_manager", json!({})).unwrap();
    let row = snapshot["dictionaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "words:legacy")
        .unwrap();
    assert!(row["source"].is_null());
    for (generation, publisher) in [
        (json!("stale"), "jitendex"),
        (row["generation"].clone(), "unknown"),
    ] {
        assert!(call(
            &legacy_app,
            "dictionary_connect_publisher",
            json!({"id":"words:legacy","generation":generation,"publisher":publisher})
        )
        .is_err());
    }
    call(
        &legacy_app,
        "dictionary_connect_publisher",
        json!({"id":"words:legacy","generation":row["generation"],"publisher":"jitendex"}),
    )
    .unwrap();
    call(
        &legacy_app,
        "dictionary_preferences",
        json!({"preferences":{"check_updates":true,"automatic":true,"notify_updates":false}}),
    )
    .unwrap();
    drop(legacy_app);
    let restarted = session(&root);
    let restored = call(&restarted, "dictionary_manager", json!({})).unwrap();
    let row = restored["dictionaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "words:legacy")
        .unwrap();
    assert_eq!(row["source"], "https://jitendex.org/static/yomitan.json");
    assert_eq!(row["consented"], true);
    assert_eq!(restored["preferences"]["automatic"], true);
    assert_eq!(restored["preferences"]["notify_updates"], false);
    // A new unrelated import at that stable ID cannot inherit the previous connection.
    let replacement =
        sakatsu_core::dict::Dictionary::import(&original, &root.join("dictionaries/legacy"))
            .unwrap();
    std::fs::write(
        replacement.root().join("meta.json"),
        serde_json::to_vec(&meta).unwrap(),
    )
    .unwrap();
    drop(replacement);
    let replaced = session(&root);
    let snapshot = call(&replaced, "dictionary_manager", json!({})).unwrap();
    let row = snapshot["dictionaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "words:legacy")
        .unwrap();
    assert!(
        row["source"].is_null(),
        "a connection cannot survive an unrelated replacement"
    );
}
