//! The capture frame's commands over the real IPC boundary.
//!
//! What is checked here is the contract — the command names and argument names the UI
//! hard-codes as strings — and the refusals. What cannot be checked here is the capture
//! itself: `read_frame` reads the live desktop through a real always-on-top window, and
//! `MockRuntime` has neither. That the frame does not photograph its own border is a
//! property of a running window, so it is verified by using it, not by a test claiming to.
//!
//! The arithmetic that *can* go wrong silently — the inset from window bounds to the
//! interior actually read — is unit-tested next to the code, in `commands::frame_tests`.

use sakatsu_core::analyzer::LinderaAnalyzer;
use sakatsu_core::ocr::{Ocr, OcrError, OcrImage};
use sakatsu_desktop_lib::commands::{AnalyzerState, OcrState, StoreState};
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;

struct StubOcr;

impl Ocr for StubOcr {
    fn recognize(&self, _image: &OcrImage) -> Result<String, OcrError> {
        Ok("認識した".into())
    }
    fn engine(&self) -> &str {
        "stub"
    }
}

fn app() -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(OcrState::with_engine(Arc::new(StubOcr)))
        // `read_frame` reads the hide-during-capture setting, so the command needs the
        // store even though nothing here has a frame to capture through.
        .manage(StoreState {
            store: None,
            error: None,
        })
        // And it warms the analyzer, because a capture is the last moment before one is
        // needed. Managed here for the same reason as the store: the command asks for it
        // before it gets as far as noticing there is no frame, so without it the refusal
        // this file checks arrives as a state error instead.
        .manage(AnalyzerState::fixed(Arc::new(
            LinderaAnalyzer::new().expect("the dictionary is compiled in"),
        )))
        .invoke_handler(tauri::generate_handler![
            sakatsu_desktop_lib::commands::close_frame,
            sakatsu_desktop_lib::commands::place_frame,
            sakatsu_desktop_lib::commands::frame_rect,
            sakatsu_desktop_lib::commands::read_frame,
            sakatsu_desktop_lib::commands::ocr_engine,
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock app")
}

fn webview(
    app: &tauri::App<tauri::test::MockRuntime>,
) -> tauri::WebviewWindow<tauri::test::MockRuntime> {
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build mock webview")
}

fn invoke(
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

/// The names and shapes the UI calls. If one of these is renamed without the UI being
/// changed too, the capture silently stops working — this is what notices.
#[test]
fn the_frame_commands_answer_by_name() {
    let app = app();
    let webview = webview(&app);

    // Closing when nothing is open is not an error: the binding both summons and
    // dismisses, and refusing here would make it fail on the first press.
    invoke(&webview, "close_frame", serde_json::json!({})).expect("close_frame answers");
}

/// Every frame command needs a frame. Asked without one they say so, rather than
/// panicking or quietly doing nothing.
///
/// `read_frame` is the load-bearing one. Reading is the *only* thing the capture binding
/// does now — it does not open a frame and it does not unhide one — so this refusal is
/// what stands between a hidden overlay and the binding putting it back on screen. If it
/// ever starts succeeding by conjuring a frame, that behaviour returns.
#[test]
fn the_frame_commands_refuse_when_there_is_no_frame() {
    let app = app();
    let webview = webview(&app);

    for (cmd, args) in [
        ("frame_rect", serde_json::json!({})),
        ("read_frame", serde_json::json!({})),
        (
            "place_frame",
            serde_json::json!({ "rect": { "x": 0.0, "y": 0.0, "width": 640.0, "height": 200.0 } }),
        ),
    ] {
        let error = invoke(&webview, cmd, args).expect_err("refuses without a frame");
        assert!(
            error.as_str().unwrap_or_default().contains("not open"),
            "{cmd} should say the frame is not open, said {error}"
        );
    }
}
