//! Unloading a model through the real handler, racing work that uses it.
//!
//! **Its own test binary, and one test.** The lifecycle gates are process-wide: holding the
//! translation gate here would refuse translations in an unrelated test running beside it in the
//! same process, and a second test in this file would race this one for the same gate.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sakatsu_core::translate::{TranslateError, Translator};
use sakatsu_desktop_lib::commands::{OcrState, TranslatorState};
use sakatsu_desktop_lib::lifecycle::{gates, Gate};
use tauri::test::{mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;

/// A translator that takes a while, and says when it has finished.
struct Slow {
    finished: Arc<AtomicBool>,
}

impl Translator for Slow {
    fn translate(&self, text: &str) -> Result<String, TranslateError> {
        std::thread::sleep(Duration::from_millis(400));
        self.finished.store(true, Ordering::SeqCst);
        Ok(format!("slow: {text}"))
    }

    fn engine(&self) -> &str {
        "slow"
    }
}

fn call(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    tauri::test::get_ipc_response(
        webview,
        InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().expect("valid url"),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| {
        body.deserialize::<serde_json::Value>()
            .expect("response deserializes")
    })
}

fn request(generation: u64) -> serde_json::Value {
    serde_json::json!({
        "owner": "lifecycle-ipc",
        "operation": "translate_passage",
        "generation": generation,
        "priority": "foreground",
    })
}

#[test]
fn an_unload_and_the_work_on_its_engine_never_overlap() {
    let finished = Arc::new(AtomicBool::new(false));
    let app = mock_builder()
        .manage(TranslatorState::with_engine(Arc::new(Slow {
            finished: Arc::clone(&finished),
        })))
        .manage(OcrState::new(
            std::path::PathBuf::new(),
            std::path::PathBuf::new(),
            &sakatsu_core::store::Settings::default(),
        ))
        .invoke_handler(tauri::generate_handler![
            sakatsu_desktop_lib::commands::translate,
            sakatsu_desktop_lib::commands::unload_translator,
            sakatsu_desktop_lib::commands::unload_ocr,
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build mock webview");

    // 1. A translation already running is waited for; the unload releases the model after it.
    let in_flight = {
        let webview = webview.clone();
        std::thread::spawn(move || {
            call(
                &webview,
                "translate",
                serde_json::json!({ "text": "水", "request": request(1) }),
            )
        })
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while gates().active(Gate::Translation) == 0 {
        assert!(
            Instant::now() < deadline,
            "the translation never entered its gate"
        );
        std::thread::yield_now();
    }
    let released = call(&webview, "unload_translator", serde_json::json!({}))
        .expect("the unload waits for the running translation, then succeeds");
    assert!(
        finished.load(Ordering::SeqCst),
        "the model was released only after the translation using it had finished"
    );
    assert_eq!(
        released,
        serde_json::json!(true),
        "something was loaded, and it was released"
    );
    assert_eq!(
        in_flight
            .join()
            .unwrap()
            .expect("the translation completed"),
        serde_json::json!("slow: 水")
    );

    // Remembered translations survive an unload: the cache is kept on purpose.
    assert_eq!(
        call(
            &webview,
            "translate",
            serde_json::json!({ "text": "水", "request": request(2) }),
        )
        .expect("answered from the cache"),
        serde_json::json!("slow: 水")
    );
    // The slot really is empty: new text has to load a model again, and in this fixture there is
    // none on disk, so the stand-in answers instead of the slow engine.
    let reloaded = call(
        &webview,
        "translate",
        serde_json::json!({ "text": "火", "request": request(5) }),
    )
    .expect_err("no model on disk in the fixture");
    assert!(
        reloaded
            .as_str()
            .unwrap_or_default()
            .contains("unavailable"),
        "the slow engine was not still loaded: {reloaded}"
    );

    // 2. While an unload holds the gate, new work is refused and says so — never silently dropped.
    let held = gates()
        .exclusive(&[Gate::Translation], Duration::ZERO)
        .expect("nothing is translating now");
    let refused = call(
        &webview,
        "translate",
        serde_json::json!({ "text": "木", "request": request(6) }),
    )
    .expect_err("a translation cannot start while the gate is held");
    let refused = refused.as_str().unwrap_or_default().to_string();
    assert!(
        refused.contains("busy"),
        "refused as busy, not as cancelled: {refused}"
    );
    assert_ne!(
        refused, "request cancelled",
        "the interface would have dropped it silently"
    );
    drop(held);
    let reopened = call(
        &webview,
        "translate",
        serde_json::json!({ "text": "木", "request": request(7) }),
    )
    .expect_err("the stand-in still has no model");
    assert!(
        !reopened.as_str().unwrap_or_default().contains("busy"),
        "the gate reopened"
    );

    // 3. Unloading recognition with nothing loaded releases nothing, and says so.
    assert_eq!(
        call(&webview, "unload_ocr", serde_json::json!({})).expect("registered"),
        serde_json::json!(false)
    );
}
