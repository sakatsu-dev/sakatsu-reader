//! Startup is checked with an owned fake process and an isolated app-data folder.
#![cfg(windows)]

#[path = "../../core/tests/common/scratch.rs"]
mod scratch_guard;

use sakatsu_core::analyzer::ichiran::server;
use sakatsu_core::{
    analyzer::{AnalyzeError, Analyzer},
    model::Analysis,
    store::AnalyzerChoice,
};
use sakatsu_desktop_lib::{
    commands::{AnalyzerState, BuiltInAnalyzerState, KanjiState, ParticleState},
    route::Readiness,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::Manager;

struct BuiltIn;
#[test]
fn startup_without_models_leaves_the_engine_uncached_for_setup() {
    use sakatsu_core::{runtime_status::EngineState, store::Settings};
    use sakatsu_desktop_lib::commands::OcrState;
    let scratch = scratch_guard::Scratch::new("ocr-startup", "missing-models");
    let state = OcrState::new(
        scratch.join("manga-ocr"),
        scratch.join("ppocrv6"),
        &Settings::default(),
    );
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    sakatsu_desktop_lib::commands::warm_ocr_on_startup(app.handle().clone());
    let state = app.state::<OcrState>();
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.status().state == EngineState::Loading && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_ne!(state.status().state, EngineState::Loading);
    assert!(!state.is_ready());
    assert!(
        state.status().loaded.is_none(),
        "setup must not inherit a cached stub engine"
    );
}

impl Analyzer for BuiltIn {
    fn analyze(&self, _: &str) -> Result<Analysis, AnalyzeError> {
        Ok(Analysis::default())
    }
    fn engine(&self) -> &str {
        "lindera"
    }
}

#[test]
fn startup_preload_and_on_demand_use_the_production_entrypoints() {
    const CHILD: &str = "SAKATSU_PRELOAD_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "startup_preload_and_on_demand_use_the_production_entrypoints",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let report = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        println!("{report}");
        assert!(
            result.status.success() && report.contains("1 passed"),
            "child did not execute its assertions"
        );
        return;
    }
    let root = scratch_guard::Scratch::new("preload", "startup");
    let profile = root.join("roaming");
    std::env::set_var("APPDATA", &profile);
    for name in [
        "SAKATSU_ANALYZER",
        "SAKATSU_ANALYZER_FAULT",
        "SAKATSU_ICHIRAN_CMD",
    ] {
        std::env::remove_var(name);
    }
    let marker = root.join("starts.txt");
    let answer = root.join("answer.json");
    let script = root.join("sidecar.ps1");
    let mut sample: serde_json::Value = serde_json::from_str(include_str!(
        "../../core/tests/fixtures/ichiran/mizu-o-nomu.json"
    ))
    .unwrap();
    sample[0][0][0].as_array_mut().unwrap().truncate(1);
    std::fs::write(&answer, serde_json::to_vec(&sample).unwrap()).unwrap();
    let source = r#"
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
[IO.File]::AppendAllText('%MARK%', "$PID`n")
$answer = [IO.File]::ReadAllText('%ANSWER%', [Text.Encoding]::UTF8)
if ($args -contains '-e') {
  while ($null -ne ($header = [Console]::ReadLine())) {
    $null = [Console]::ReadLine()
    [Console]::WriteLine($answer)
    [Console]::Out.Flush()
  }
} else { [Console]::WriteLine($answer) }
"#;
    std::fs::write(
        &script,
        source
            .replace("%MARK%", &marker.to_string_lossy().replace('\'', "''"))
            .replace("%ANSWER%", &answer.to_string_lossy().replace('\'', "''")),
    )
    .unwrap();
    let configured = profile.join("com.sakatsu.app/ichiran-command.json");
    std::fs::create_dir_all(configured.parent().unwrap()).unwrap();
    std::fs::write(
        configured,
        serde_json::to_vec(&serde_json::json!({
            "program": "powershell.exe", "prefix_args": ["-NoProfile", "-File", script, "-f"]
        }))
        .unwrap(),
    )
    .unwrap();
    struct SessionCleanup;
    impl Drop for SessionCleanup {
        fn drop(&mut self) {
            server::set_persistent(false);
        }
    }
    let _cleanup = SessionCleanup;
    server::set_persistent(true);
    let builtin: Arc<dyn Analyzer> = Arc::new(BuiltIn);
    let state =
        sakatsu_desktop_lib::analyzer_to_open_with(AnalyzerChoice::Auto, builtin.clone(), false);
    assert!(matches!(state.route().readiness, Readiness::Deferred));
    let app = tauri::test::mock_builder()
        .manage(state)
        .manage(BuiltInAnalyzerState(builtin))
        .manage(KanjiState::load(root.join("missing-kanji.tsv")))
        .manage(ParticleState::load(root.join("missing-categories.tsv")))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    assert!(!sakatsu_desktop_lib::upgrade_analyzer_in_the_background(
        app.handle().clone(),
        AnalyzerChoice::Auto,
        false
    ));
    assert!(!sakatsu_desktop_lib::upgrade_analyzer_in_the_background(
        app.handle().clone(),
        AnalyzerChoice::BuiltIn,
        true
    ));
    let status =
        sakatsu_desktop_lib::runtime_status::analysis_engine(&app.state::<AnalyzerState>());
    assert_eq!(
        status.state,
        sakatsu_core::runtime_status::EngineState::NotLoaded
    );
    let readiness = sakatsu_desktop_lib::commands::readiness(app.state(), app.state(), app.state());
    assert!(serde_json::to_string(&readiness)
        .unwrap()
        .contains("first use"));
    assert!(
        !marker.exists(),
        "startup or status queries launched the disabled preload"
    );

    app.state::<AnalyzerState>()
        .now()
        .analyze("\u{6c34}")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap().lines().count(),
        1,
        "first use must start on demand"
    );
    server::shutdown();
    assert!(sakatsu_desktop_lib::upgrade_analyzer_in_the_background(
        app.handle().clone(),
        AnalyzerChoice::Auto,
        true
    ));
    let deadline = Instant::now() + Duration::from_secs(12);
    while app.state::<AnalyzerState>().route().generation == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(
        app.state::<AnalyzerState>().route().readiness,
        Readiness::Ready
    ));
    app.state::<AnalyzerState>()
        .now()
        .analyze("\u{6c34}")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap().lines().count(),
        2,
        "the prepared process must serve the first passage without another start"
    );
}
