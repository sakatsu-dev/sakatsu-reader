//! Deterministic publishers at the HTTP boundary. Everything above transport, including
//! production IPC, validation, generation checks, publication and adoption, remains real.
use super::*;
use serde_json::{json, Value};
use std::sync::OnceLock;

const CHILD: &str = "SAKATSU_PUBLISHER_TEST_CHILD";
const INDEX: &str = "https://publisher.example/index.json";
const DOWNLOAD: &str = "https://publisher.example/words.zip";
type Reply = Result<(u16, Option<String>, Vec<u8>), String>;
static RESPONSES: OnceLock<Mutex<BTreeMap<String, Reply>>> = OnceLock::new();
static REQUESTS: AtomicU64 = AtomicU64::new(0);
#[derive(Default)]
struct Gate {
    enabled: AtomicBool,
    entered: AtomicBool,
    ready: Mutex<bool>,
    changed: std::sync::Condvar,
}
static GATE: OnceLock<Arc<Gate>> = OnceLock::new();
struct HeldReader {
    gate: Arc<Gate>,
    bytes: std::io::Cursor<Vec<u8>>,
    first: bool,
}
impl Read for HeldReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.first {
            self.first = false;
            self.gate.entered.store(true, Ordering::Release);
            let mut ready = self.gate.ready.lock().unwrap();
            while !*ready {
                ready = self.gate.changed.wait(ready).unwrap();
            }
        }
        self.bytes.read(buffer)
    }
}
impl Gate {
    fn hold(&self) {
        *self.ready.lock().unwrap() = false;
        self.entered.store(false, Ordering::Release);
        self.enabled.store(true, Ordering::Release);
    }
    fn release(&self) {
        *self.ready.lock().unwrap() = true;
        self.enabled.store(false, Ordering::Release);
        self.changed.notify_all();
    }
    fn wait_entered(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !self.entered.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

pub(super) fn response(url: &str) -> Option<Result<ureq::http::Response<ureq::Body>, String>> {
    std::env::var_os(CHILD)?;
    REQUESTS.fetch_add(1, Ordering::Relaxed);
    let reply = RESPONSES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .get(url)
        .cloned()
        .unwrap_or_else(|| Err(format!("Unexpected fixture URL: {url}")));
    Some(reply.and_then(|(status, location, bytes)| {
        let mut builder = ureq::http::Response::builder().status(status);
        if let Some(location) = location {
            builder = builder.header("location", location);
        }
        let body = if let Some(gate) = GATE
            .get()
            .filter(|gate| url == INDEX && gate.enabled.load(Ordering::Acquire))
        {
            ureq::Body::builder().reader(HeldReader {
                gate: Arc::clone(gate),
                bytes: std::io::Cursor::new(bytes),
                first: true,
            })
        } else {
            ureq::Body::builder().data(bytes)
        };
        builder.body(body).map_err(|e| e.to_string())
    }))
}
fn reply(url: &str, bytes: Vec<u8>) {
    RESPONSES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(url.into(), Ok((200, None, bytes)));
}
fn metadata(revision: &str) -> Vec<u8> {
    json!({"title":format!("New dated title {revision}"),"revision":revision,"format":3,"isUpdatable":true,"indexUrl":INDEX,"downloadUrl":DOWNLOAD}).to_string().into_bytes()
}
fn archive(path: &Path, revision: &str, term: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in [
        ("index.json", metadata(revision)),
        (
            "term_bank_1.json",
            json!([[
                term,
                term,
                "n",
                "",
                0,
                [format!("meaning of {term}")],
                1,
                ""
            ]])
            .to_string()
            .into_bytes(),
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap();
    std::fs::read(path).unwrap()
}
fn call(
    app: &tauri::App<tauri::test::MockRuntime>,
    cmd: &str,
    args: Value,
) -> Result<Value, Value> {
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
    .map(|reply| reply.deserialize().unwrap())
}
fn wait(app: &tauri::App<tauri::test::MockRuntime>, id: Value) -> Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let snapshot = call(app, "dictionary_manager", json!({})).unwrap();
        if snapshot["operation"]["id"] == id
            && matches!(
                snapshot["operation"]["phase"].as_str(),
                Some("complete" | "failed" | "cancelled")
            )
        {
            return snapshot;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "operation did not finish: {snapshot}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn check(app: &tauri::App<tauri::test::MockRuntime>) -> Value {
    wait(app, call(app, "dictionary_check", json!({})).unwrap())
}
fn update(app: &tauri::App<tauri::test::MockRuntime>) -> Value {
    wait(
        app,
        call(app, "dictionary_update", json!({"id":"words:fixture"})).unwrap(),
    )
}

pub fn publisher_round_trip() {
    let Some(path) = std::env::var_os(CHILD) else {
        let root = Temporary::new().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["publisher_round_trip", "--exact", "--nocapture"])
            .env(CHILD, &root.0)
            .env("APPDATA", root.0.join("roaming"))
            .env("LOCALAPPDATA", root.0.join("local"))
            .env("SAKATSU_DEVICE_DATA_DIR", &root.0)
            .env("SAKATSU_DICT_DIR", root.0.join("dictionaries"))
            .env("SAKATSU_KANJI_DIR", root.0.join("kanji"))
            .output()
            .unwrap();
        let report = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        println!("{report}");
        assert!(output.status.success(), "{report}");
        assert!(report.contains("1 passed"));
        return;
    };
    let root = PathBuf::from(path);
    std::fs::create_dir_all(root.join("dictionaries")).unwrap();
    std::fs::create_dir_all(root.join("kanji")).unwrap();
    let old = root.join("old.zip");
    archive(&old, "2026.10.03.0", "old");
    Dictionary::import(&old, &root.join("dictionaries/fixture")).unwrap();
    let app = tauri::test::mock_builder()
        .manage(crate::discover_dictionaries(&root.join("dictionaries")))
        .manage(DictionaryManagerState::default())
        .manage(crate::warmup::WarmupState::default())
        .invoke_handler(crate::reader_commands!())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .unwrap();

    assert_eq!(check(&app)["operation"]["phase"], "complete");
    assert_eq!(
        REQUESTS.load(Ordering::Relaxed),
        0,
        "unapproved sources must not be contacted"
    );
    call(
        &app,
        "dictionary_source_consent",
        json!({"id":"words:fixture","endpoint":INDEX,"allowed":true}),
    )
    .unwrap();
    let new = archive(&root.join("new.zip"), "2026.10.28.0", "new");
    reply(INDEX, metadata("2026.10.28.0"));
    reply(DOWNLOAD, new.clone());
    let checked = check(&app);
    assert_eq!(
        checked["dictionaries"][0]["available_revision"],
        "2026.10.28.0"
    );
    let updated = update(&app);
    assert_eq!(updated["operation"]["phase"], "complete", "{updated}");
    assert_eq!(updated["dictionaries"][0]["id"], "words:fixture");
    assert_eq!(updated["dictionaries"][0]["revision"], "2026.10.28.0");
    assert_eq!(
        app.state::<DictionaryState>().0.now().dictionaries[0]
            .lookup("new")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(check(&app)["dictionaries"][0]["phase"], "current");

    // A moving download must agree with the check; it cannot silently install another version.
    reply(INDEX, metadata("2026.10.29.0"));
    check(&app);
    assert_eq!(update(&app)["operation"]["phase"], "failed");
    assert_eq!(
        call(&app, "dictionary_manager", json!({})).unwrap()["dictionaries"][0]["revision"],
        "2026.10.28.0"
    );
    reply(DOWNLOAD, b"not a ZIP".to_vec());
    check(&app);
    assert_eq!(update(&app)["operation"]["phase"], "failed");

    // Absence, malformed data and a failed request remain different from a current revision.
    for bytes in [
        b"<html>error</html>".to_vec(),
        vec![b' '; managed::MAX_METADATA as usize + 1],
        metadata("2026.10.01.0"),
    ] {
        reply(INDEX, bytes);
        let result = check(&app);
        assert_eq!(result["operation"]["phase"], "failed", "{result}");
        assert!(result["dictionaries"][0]["available_revision"].is_null());
    }
    RESPONSES
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .insert(INDEX.into(), Err("fixture offline".into()));
    assert!(check(&app)["operation"]["detail"]
        .as_str()
        .unwrap()
        .contains("fixture offline"));

    // Checks do not own publication: another legitimate installer can change the generation.
    reply(INDEX, metadata("2026.10.29.0"));
    reply(
        DOWNLOAD,
        archive(&root.join("offered.zip"), "2026.10.29.0", "offered"),
    );
    check(&app);
    let newer = root.join("newer.zip");
    archive(&newer, "2026.10.30.0", "newer");
    Dictionary::import(&newer, &root.join("dictionaries/fixture")).unwrap();
    commands::adopt(
        app.handle(),
        sakatsu_core::setup::catalog::find("dictionaries").unwrap(),
    );
    let result = update(&app);
    assert_eq!(result["operation"]["phase"], "failed");
    assert_eq!(result["dictionaries"][0]["revision"], "2026.10.30.0");

    let gate = GATE.get_or_init(|| Arc::new(Gate::default()));
    reply(INDEX, metadata("2026.10.31.0"));
    gate.hold();
    let held = call(&app, "dictionary_check", json!({})).unwrap();
    gate.wait_entered();
    assert!(
        call(&app, "dictionary_check", json!({})).is_err(),
        "duplicate checks must not start another request"
    );
    let newer = root.join("latest.zip");
    archive(&newer, "2026.11.01.0", "latest");
    Dictionary::import(&newer, &root.join("dictionaries/fixture")).unwrap();
    commands::adopt(
        app.handle(),
        sakatsu_core::setup::catalog::find("dictionaries").unwrap(),
    );
    gate.release();
    let stale = wait(&app, held);
    assert_eq!(stale["operation"]["phase"], "failed");
    assert!(stale["operation"]["detail"]
        .as_str()
        .unwrap()
        .contains("changed"));

    reply(INDEX, metadata("2026.11.02.0"));
    gate.hold();
    let held = call(&app, "dictionary_check", json!({})).unwrap();
    gate.wait_entered();
    call(&app, "dictionary_cancel", json!({"operationId":held})).unwrap();
    gate.release();
    let cancelled = wait(&app, held);
    assert_eq!(cancelled["operation"]["phase"], "cancelled");
    assert!(cancelled["dictionaries"][0]["available_revision"].is_null());

    // Withdrawing source consent also cancels a check that has already reached transport.
    gate.hold();
    let held = call(&app, "dictionary_check", json!({})).unwrap();
    gate.wait_entered();
    call(
        &app,
        "dictionary_source_consent",
        json!({"id":"words:fixture","endpoint":INDEX,"allowed":false}),
    )
    .unwrap();
    gate.release();
    assert_eq!(wait(&app, held)["operation"]["phase"], "cancelled");
    call(
        &app,
        "dictionary_source_consent",
        json!({"id":"words:fixture","endpoint":INDEX,"allowed":true}),
    )
    .unwrap();
    reply(
        DOWNLOAD,
        archive(&root.join("automatic.zip"), "2026.11.02.0", "automatic"),
    );
    check(&app);
    assert!(super::update(app.handle().clone(), "words:fixture".into(), true).is_err());
    call(
        &app,
        "dictionary_preferences",
        json!({"preferences":{"check_updates":true,"automatic":true}}),
    )
    .unwrap();
    let reading = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Analysis)
        .unwrap();
    let operation = super::update(app.handle().clone(), "words:fixture".into(), true).unwrap();
    let blocked = wait(&app, json!(operation));
    assert_eq!(blocked["operation"]["phase"], "failed");
    assert_eq!(blocked["dictionaries"][0]["revision"], "2026.11.01.0");
    drop(reading);
    let operation = super::update(app.handle().clone(), "words:fixture".into(), true).unwrap();
    assert_eq!(
        wait(&app, json!(operation))["operation"]["phase"],
        "complete"
    );

    // The production redirect loop validates every destination, including relative redirects.
    let redirect = "https://publisher.example/redirect";
    RESPONSES.get().unwrap().lock().unwrap().insert(
        redirect.into(),
        Ok((
            302,
            Some("http://publisher.example/plain".into()),
            Vec::new(),
        )),
    );
    assert!(request(redirect).err().unwrap().contains("HTTPS"));
    RESPONSES.get().unwrap().lock().unwrap().insert(
        redirect.into(),
        Ok((302, Some("/index.json".into()), Vec::new())),
    );
    assert!(request(redirect).is_ok());
}
