//! What each analyzer costs on the first analysis of a session.
//!
//! ```text
//! cargo test --release -p sakatsu-core --test analyzer_cold_start -- --ignored --nocapture
//! ```
//!
//! **The first one is the one a reader feels.** Steady-state timings are already measured in
//! `analyzer_corpus`; this is the other number, and for the sidecar it is the larger of the two
//! by a factor of ten. Run in a fresh process so the embedded dictionary is genuinely cold —
//! UniDic is 338 MB, and whether that costs anything at startup is exactly the question.
//!
//! # Measured 2026-08-27
//!
//! | | debug | release |
//! |---|---:|---:|
//! | loading UniDic | 2.15 s | **0.078 s** |
//! | first analysis | 0.0005 s | 0.0002 s |
//! | Ichiran, first analysis after a WSL idle | 7.65 s | — |
//! | Ichiran, second | 0.72 s | — |
//!
//! **Run it in release before believing the dictionary is slow.** `analyzer_to_open_with` loads
//! it on the startup path, before the window shows, so 2.15 s would be a launch regression worth
//! acting on — and it is not one: the shipped build pays 78 ms. Debug is a decompression
//! measurement, not a startup one.
mod common;

use std::time::Instant;

use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::IchiranAnalyzer;
use sha2::{Digest, Sha256};

const SAMPLE: &str = "気持ちが伝わってきます。";
const CHILD_ENGINE: &str = "SAKATSU_COLD_ENGINE";

fn require_release() {
    #[cfg(debug_assertions)]
    panic!("run this under --release; debug and release startup have different cost shapes");
}

fn executable_digest() -> String {
    let path = std::env::current_exe().expect("the test binary knows its own path");
    let bytes = std::fs::read(path).expect("the running executable can be read");
    format!(
        "{} ({} bytes, release)",
        sakatsu_core::hashing::hex(Sha256::digest(&bytes)),
        bytes.len()
    )
}

/// Windows console programs may write UTF-16LE even when their output is redirected. Treating
/// that as UTF-8 produces a line of NUL-separated letters, which technically records bytes but
/// hides the prerequisite failure a person needs to act on.
fn command_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes.iter().skip(1).step_by(2).any(|byte| *byte == 0) {
        let words: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        String::from_utf16_lossy(&words)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn command_state(program: &str, args: &[&str]) -> String {
    match std::process::Command::new(program).args(args).output() {
        Ok(output) => {
            let stdout = command_text(&output.stdout)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let stderr = command_text(&output.stderr)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "exit {:?}; stdout={stdout:?}; stderr={stderr:?}",
                output.status.code()
            )
        }
        Err(error) => format!("unavailable: {error}"),
    }
}

#[test]
fn redirected_windows_diagnostics_are_readable_in_either_console_encoding() {
    assert_eq!(
        command_text(b"Access is denied.\r\n"),
        "Access is denied.\r\n"
    );
    let utf16: Vec<u8> = "Access is denied.\r\n"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(command_text(&utf16), "Access is denied.\r\n");
}

#[test]
#[ignore = "child process of the per-engine cold-start measurement"]
fn one_cold_engine() {
    let Some(engine) = std::env::var_os(CHILD_ENGINE) else {
        println!("skipped: launched only by the parent measurement");
        return;
    };
    require_release();
    let engine = engine.to_string_lossy();
    match engine.as_ref() {
        "lindera" => {
            let started = Instant::now();
            let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");
            let constructed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let first = Instant::now();
            analyzer.analyze(SAMPLE).expect("first Lindera analysis");
            let first_ms = first.elapsed().as_secs_f64() * 1000.0;
            println!(
                "COLD_JSON {}",
                serde_json::json!({
                    "engine": "lindera",
                    "outcome": "measured",
                    "constructed_ms": constructed_ms,
                    "first_ms": first_ms,
                })
            );
        }
        "ichiran" => {
            sakatsu_core::analyzer::ichiran::server::shutdown();
            sakatsu_core::analyzer::ichiran::server::set_persistent(true);
            let started = Instant::now();
            let analyzer = IchiranAnalyzer::new();
            let constructed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let first = Instant::now();
            let result = analyzer.analyze(SAMPLE);
            let first_ms = first.elapsed().as_secs_f64() * 1000.0;
            // **Which path actually served it.** Read after the timed region, so it costs the
            // measurement nothing. Without this the number below is unattributable: a live session
            // that failed to start switches persistence off and the passage silently goes one
            // process per passage, which is the cost this measurement exists to distinguish.
            let route = sakatsu_core::analyzer::ichiran::server::last_route();
            let (by_session, one_shot) = sakatsu_core::analyzer::ichiran::server::routing_counts();
            match result {
                Ok(_) => println!(
                    "COLD_JSON {}",
                    serde_json::json!({
                        "engine": "ichiran",
                        "outcome": "measured",
                        "constructed_ms": constructed_ms,
                        "first_ms": first_ms,
                        "route": route.name(),
                        "served_by_session": by_session,
                        "served_one_shot": one_shot,
                    })
                ),
                Err(error) => println!(
                    "COLD_JSON {}",
                    serde_json::json!({
                        "engine": "ichiran",
                        "outcome": "incomplete",
                        "reason": error.to_string(),
                        "route": route.name(),
                    })
                ),
            }
            sakatsu_core::analyzer::ichiran::server::shutdown();
        }
        other => panic!("unknown child engine {other}"),
    }
}

#[test]
#[ignore = "a measurement, and it touches WSL"]
fn what_the_first_analysis_of_a_session_costs() {
    require_release();
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    println!("\n=== analyzer cold-start identity ===");
    println!("  executable      {}", executable_digest());
    println!(
        "  fixture sha256  {}",
        sakatsu_core::hashing::hex(Sha256::digest(SAMPLE.as_bytes()))
    );
    println!("  samples         3 fresh child processes per engine");
    println!("  OS file cache   uncontrolled");
    println!(
        "  WSL before      {}",
        command_state("wsl", &["--list", "--running"])
    );
    println!(
        "  PostgreSQL pre  not probed: when WSL is stopped, probing would warm what is measured"
    );

    for engine in ["lindera", "ichiran"] {
        let mut measured = Vec::new();
        let mut incomplete = Vec::new();
        let mut routes: Vec<String> = Vec::new();
        for _ in 0..3 {
            let output = std::process::Command::new(&exe)
                .args(["one_cold_engine", "--exact", "--ignored", "--nocapture"])
                .env(CHILD_ENGINE, engine)
                .env(sakatsu_core::analyzer::ichiran::SERVER_ENV, "true")
                .output()
                .expect("spawn analyzer child");
            assert!(
                output.status.success(),
                "{engine} child failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            let line = stdout
                .lines()
                .find_map(|line| line.strip_prefix("COLD_JSON "))
                .unwrap_or_else(|| panic!("{engine} child produced no result:\n{stdout}"));
            let value: serde_json::Value = serde_json::from_str(line).expect("child JSON");
            if let Some(route) = value["route"].as_str() {
                routes.push(route.to_string());
            }
            if value["outcome"] == "measured" {
                measured.push((
                    value["constructed_ms"].as_f64().expect("construction ms"),
                    value["first_ms"].as_f64().expect("first ms"),
                ));
            } else {
                incomplete.push(
                    value["reason"]
                        .as_str()
                        .unwrap_or("unspecified prerequisite failure")
                        .to_string(),
                );
            }
        }
        if measured.len() == 3 {
            println!("  {engine:<10} measured {measured:?}");
            // **Three samples that took different paths are not three samples of one thing.**
            // Reported rather than averaged away: a spread across a session start and two
            // fallbacks would otherwise read as ordinary run-to-run variation.
            let mut distinct: Vec<&String> = routes.iter().collect();
            distinct.sort();
            distinct.dedup();
            match distinct.as_slice() {
                [] => {}
                [only] => println!("  {engine:<10} route {only} for all three"),
                several => println!(
                    "  {engine:<10} MIXED ROUTES {several:?} -- these are not comparable samples"
                ),
            }
        } else {
            println!(
                "  {engine:<10} INCOMPLETE: {} of 3 measured; {}",
                measured.len(),
                incomplete.join(" | ")
            );
            if !routes.is_empty() {
                println!("  {engine:<10} routes {routes:?}");
            }
        }
    }
    println!(
        "  PostgreSQL post {}",
        command_state("wsl", &["-e", "pg_isready"])
    );
}

/// Does warming on intent actually absorb the wake-up?
///
/// The claim is that a cheap `wsl -e true` fired when the overlay opens pays the session
/// wake-up, so the analysis that follows a capture finds a warm session. This measures both
/// halves against the same cold start.
///
/// Measured 2026-08-27: **6.25 s without warming, 0.90 s with.**
///
/// **Check nothing else is holding the session open before believing a null result.** This
/// first reported 0.63 s against 0.60 s — no effect at all — because a stray
/// `wsl -e sleep infinity` from an earlier experiment was still running, so WSL never went
/// cold and there was nothing for the warm-up to absorb. `wsl --list --running` says whether
/// the distro is up; if it is, this test can only measure noise.
#[test]
#[ignore = "idles WSL for two minutes, twice"]
fn warming_on_intent_moves_the_wait_off_the_analysis() {
    use sakatsu_core::analyzer::Analyzer as _;

    let ichiran = IchiranAnalyzer::new();
    if ichiran.analyze(SAMPLE).is_err() {
        println!("SKIP: Ichiran did not answer");
        return;
    }

    let idle = std::time::Duration::from_secs(110);

    // Cold, with no warming: what a reader gets today.
    std::thread::sleep(idle);
    let cold = Instant::now();
    let _ = ichiran.analyze(SAMPLE);
    let without = cold.elapsed().as_secs_f64();

    // Cold, but warmed the moment intent was signalled. The sleep stands in for the seconds
    // between opening the overlay and pressing the shutter.
    std::thread::sleep(idle);
    ichiran.warm();
    std::thread::sleep(std::time::Duration::from_secs(8));
    let warmed = Instant::now();
    let _ = ichiran.analyze(SAMPLE);
    let with = warmed.elapsed().as_secs_f64();

    println!("\n=== the analysis after a 110s idle ===");
    println!("  with no warming                {without:.2}s");
    println!("  warmed when intent was shown   {with:.2}s");
    println!("  the reader waits {:.1}x less", without / with.max(1e-9));
}
