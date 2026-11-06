//! Selecting an engine loads it, rather than leaving the first capture to pay for it.
//!
//! # What this can check without a server
//!
//! `VlmOcr::new` parses the endpoint and builds an HTTP agent; it contacts nothing. And
//! `ensure_managed_runtime` returns immediately for an endpoint the app does not own. So a
//! dead-but-valid URL exercises the whole warm path — the decision, the background load, the
//! readiness flag — in milliseconds, with nothing installed and nothing listening.
//!
//! What it cannot check is the twenty seconds that motivated the feature. That llama-server
//! actually comes up is `vlm_setup`'s live test, and it needs the 1.8 GB model.

use sakatsu_core::store::{OcrChoice, Settings};
use sakatsu_desktop_lib::commands::{warm_ocr_in_the_background, warm_ocr_on_startup, OcrState};
use std::time::{Duration, Instant};

fn app(settings: &Settings) -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(OcrState::new(
            std::path::PathBuf::new(),
            std::path::PathBuf::new(),
            settings,
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build mock app")
}

/// Wait for the background load, or give up. Returns whether it happened.
///
/// Polled rather than joined: the warm detaches its thread on purpose, because the two callers
/// are an IPC command and a window opening and neither may wait for it. A bounded poll is what
/// a test can honestly do with that, and one second is thousands of times the few milliseconds
/// this path costs when nothing is listening.
fn settled(state: &OcrState) -> bool {
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if state.is_ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// The engine the feature exists for: chosen, and loaded without being asked.
#[test]
fn choosing_the_vision_reader_loads_it_before_a_capture_asks() {
    let settings = Settings {
        ocr: OcrChoice::Vlm,
        // Valid, and nothing is there. `is_managed_endpoint` is false for it, so the app will
        // not try to launch anything — which is the point: this is the custom-endpoint path,
        // and it must still warm.
        vlm_endpoint: "http://127.0.0.1:1".into(),
        vlm_model: "whatever".into(),
        ..Settings::default()
    };
    let app = app(&settings);
    let state = tauri::Manager::state::<OcrState>(&app);

    assert!(!state.is_ready(), "nothing is loaded before the warm");
    warm_ocr_in_the_background(app.handle().clone());
    assert!(
        settled(&state),
        "choosing the vision reader should load it in the background",
    );
}

/// **And the in-process engines are deliberately left cold.**
///
/// This is the half that costs something if it is ever quietly changed. Warming manga-ocr or
/// PP-OCR would map ~440 MB at every startup and on every settings save, to save about half
/// a second on a capture — the trade `OcrState`'s own doc rejects. The twenty seconds of
/// starting a server is what makes the vision reader different, and this pins that it is the
/// only difference being acted on.
#[test]
fn the_in_process_engines_are_still_loaded_on_first_use() {
    for choice in [OcrChoice::Auto, OcrChoice::Manga, OcrChoice::Paddle] {
        let settings = Settings {
            ocr: choice,
            ..Settings::default()
        };
        let app = app(&settings);
        let state = tauri::Manager::state::<OcrState>(&app);

        warm_ocr_in_the_background(app.handle().clone());
        assert!(
            !settled(&state),
            "{choice:?} should still be loaded on first use, not at startup",
        );
    }
}

/// **With the preload off, nothing loads until a capture asks** — not at startup, and not
/// after a settings save, even for the vision reader whose server takes graphics memory.
#[test]
fn a_disabled_preload_leaves_even_the_vision_reader_for_the_first_capture() {
    let settings = Settings {
        ocr: OcrChoice::Vlm,
        vlm_endpoint: "http://127.0.0.1:1".into(),
        vlm_model: "whatever".into(),
        preload_ocr_on_startup: false,
        ..Settings::default()
    };
    let app = app(&settings);
    let state = tauri::Manager::state::<OcrState>(&app);

    warm_ocr_on_startup(app.handle().clone());
    warm_ocr_in_the_background(app.handle().clone());
    assert!(!settled(&state), "neither warm-up should load the engine");
}
