//! Every OCR engine, old and under evaluation, on two workloads, with nothing skipped silently.
//!
//! `ocr_corpus.rs` sweeps preprocessing for the shipped engines. This asks the narrower question
//! the engine round needs — *is a new engine better than the one it would sit beside* — and holds
//! itself to an accounting the older harness does not:
//!
//! - **An inventory.** Every image in the corpus is listed with its SHA-256, its transcript tier,
//!   its partition (development, or the sealed reading holdout) and its source. The holdout is
//!   labelled and never scored.
//! - **Complete accounting.** Per engine and workload: selected = completed + failed, and every
//!   image not selected is excluded *with a reason*. A failure is counted, never dropped.
//! - **Two workloads, reported separately.** *Whole captures*: every reviewed development capture.
//!   *Bubble crops*: the regions listed in a private manifest (`target/evaluation/bubble-crops.json`,
//!   or `SAKATSU_BUBBLE_MANIFEST`), each checked against its recorded image hash.
//! - **Timing.** Release builds only (the test refuses a debug build), the first item of each
//!   engine run once untimed as warm-up, ONNX Runtime's defaults for every in-process engine.
//! - **Paired, not pooled.** An engine under trial is compared item by item with the engine it
//!   would sit beside — a VLM endpoint with manga-ocr — with an exact sign test over the items
//!   they read differently.
//!
//! It scored the 2026-10-08 round that replaced PP-OCRv5 with PP-OCRv6 small; the engines that
//! round tried and dropped are recorded in `local documentation`.
//!
//! ```text
//! $env:SAKATSU_EVAL_MODEL_DIR = "$env:SAKATSU_EVAL_DIR\models"   # new engines; falls back to the app's
//! cargo test --release -p sakatsu-core --test ocr_engines -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The results are private (they hold transcript-derived rates per image) and go to
//! `target/evaluation/ocr-engines-<hash>.json`.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sakatsu_core::ocr::score::{character_error_rate, normalize};
use sakatsu_core::ocr::{AutoOcr, MangaOcr, Ocr, OcrImage, PaddleOcr, Rect, VlmOcr};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Where an engine's files might be: the evaluation directory first, then the app's.
fn model_dir(name: &str) -> Option<PathBuf> {
    let roots = [
        std::env::var_os("SAKATSU_EVAL_MODEL_DIR").map(PathBuf::from),
        std::env::var_os("SAKATSU_MODEL_DIR").map(PathBuf::from),
        std::env::var_os("APPDATA").map(|appdata| {
            PathBuf::from(appdata)
                .join("com.sakatsu.app")
                .join("models")
        }),
    ];
    roots
        .into_iter()
        .flatten()
        .map(|root| root.join(name))
        .find(|dir| dir.is_dir())
}

fn sha256(bytes: &[u8]) -> String {
    sakatsu_core::hashing::hex(Sha256::digest(bytes))
}

fn domain(name: &str) -> &'static str {
    match name.split('_').next().unwrap_or_default() {
        "firefox" => "browser",
        "xnviewmp" => "image-viewer",
        _ => "game",
    }
}

fn decode(path: &Path) -> Result<OcrImage, String> {
    let decoded = image::open(path)
        .map_err(|error| error.to_string())?
        .to_rgba8();
    let (width, height) = decoded.dimensions();
    OcrImage::new(width, height, decoded.into_raw()).map_err(|error| error.to_string())
}

/// One item of one workload: what to read and what it says.
struct Item {
    id: String,
    domain: &'static str,
    image: OcrImage,
    truth: String,
}

struct Workload {
    name: &'static str,
    items: Vec<Item>,
    /// Items that could not be prepared, with the reason — counted against every engine.
    failed: Vec<(String, String)>,
    excluded: Vec<(String, String)>,
}

/// The corpus inventory and the whole-capture workload.
fn whole_captures(inventory: &mut Vec<Value>) -> Workload {
    let dir = common::corpus_dir();
    let holdout = common::captures::reading_holdout_stems().expect("the holdout list reads");
    let mut workload = Workload {
        name: "whole captures",
        items: Vec::new(),
        failed: Vec::new(),
        excluded: Vec::new(),
    };
    for path in common::corpus_images(&dir) {
        let id = path
            .strip_prefix(&dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let promoted = path.with_extension("txt");
        let draft = path.with_extension("txt.mine");
        let (tier, transcript) = if promoted.is_file() {
            ("promoted", Some(promoted))
        } else if draft.is_file() {
            ("draft", None)
        } else {
            ("none", None)
        };
        let partition = if holdout.contains(&stem) {
            "holdout"
        } else {
            "development"
        };
        let bytes = std::fs::read(&path).unwrap_or_default();
        let truth_sha = transcript
            .as_ref()
            .and_then(|t| std::fs::read(t).ok())
            .map(|b| sha256(&b));
        inventory.push(json!({
            "id": id, "sha256": sha256(&bytes), "transcript_tier": tier,
            "transcript_sha256": truth_sha, "partition": partition, "domain": domain(&id),
        }));
        if partition == "holdout" {
            workload
                .excluded
                .push((id, "reading holdout, sealed".into()));
            continue;
        }
        let Some(transcript) = transcript else {
            let reason = if tier == "draft" {
                "draft transcript, not reviewed"
            } else {
                "no transcript"
            };
            workload.excluded.push((id, reason.into()));
            continue;
        };
        match (decode(&path), std::fs::read_to_string(&transcript)) {
            (Ok(image), Ok(truth)) => workload.items.push(Item {
                domain: domain(&id),
                id,
                image,
                truth,
            }),
            (Err(error), _) => workload.failed.push((id, format!("image: {error}"))),
            (_, Err(error)) => workload.failed.push((id, format!("transcript: {error}"))),
        }
    }
    workload
}

/// The bubble-crop workload, from the private manifest.
fn bubble_crops(manifest: &Path) -> Option<(Workload, String)> {
    let raw = std::fs::read(manifest).ok()?;
    let digest = sha256(&raw);
    let document: Value = serde_json::from_slice(&raw).expect("the bubble manifest parses");
    let dir = common::corpus_dir();
    let holdout = common::captures::reading_holdout_stems().expect("the holdout list reads");
    let mut workload = Workload {
        name: "bubble crops",
        items: Vec::new(),
        failed: Vec::new(),
        excluded: Vec::new(),
    };
    for entry in document["excluded"].as_array().into_iter().flatten() {
        workload.excluded.push((
            entry["image"].as_str().unwrap_or_default().to_string(),
            entry["reason"].as_str().unwrap_or_default().to_string(),
        ));
    }
    for entry in document["crops"].as_array().expect("crops") {
        let name = entry["image"].as_str().expect("image").to_string();
        let rect: Vec<u32> = entry["rect"]
            .as_array()
            .expect("rect")
            .iter()
            .map(|v| v.as_u64().expect("a pixel") as u32)
            .collect();
        let id = format!("{name}#{},{},{},{}", rect[0], rect[1], rect[2], rect[3]);
        let path = dir.join(&name);
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        // A manifest naming a holdout capture would open the seal; refuse it loudly.
        assert!(
            !holdout.contains(&stem),
            "the bubble manifest names holdout capture {name}"
        );
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                workload.failed.push((id, format!("image: {error}")));
                continue;
            }
        };
        if Some(sha256(&bytes).as_str()) != entry["sha256"].as_str() {
            workload
                .failed
                .push((id, "image changed since the manifest was drawn".into()));
            continue;
        }
        let truth = match std::fs::read_to_string(path.with_extension("txt")) {
            Ok(truth) => truth,
            Err(error) => {
                workload.failed.push((id, format!("transcript: {error}")));
                continue;
            }
        };
        let cropped = decode(&path).and_then(|image| {
            image
                .crop(Rect {
                    x: rect[0],
                    y: rect[1],
                    width: rect[2],
                    height: rect[3],
                })
                .ok_or_else(|| "the rectangle is outside the image".to_string())
        });
        match cropped {
            Ok(image) => workload.items.push(Item {
                domain: domain(&name),
                id,
                image,
                truth,
            }),
            Err(error) => workload.failed.push((id, error)),
        }
    }
    Some((workload, digest))
}

/// One engine's reads of one workload.
struct Run {
    rates: BTreeMap<String, f64>,
    /// SHA-256 of each raw output, so two runs can be shown to read *identically* — equal rates
    /// would not show that — without the private text going into the artifact.
    texts: BTreeMap<String, String>,
    seconds: Vec<f64>,
    failed: Vec<(String, String)>,
}

fn run(engine: &dyn Ocr, workload: &Workload) -> Run {
    let mut result = Run {
        rates: BTreeMap::new(),
        texts: BTreeMap::new(),
        seconds: Vec::new(),
        failed: workload.failed.clone(),
    };
    // Warm-up: the first item once, untimed and unscored, so a session's first-run cost is not
    // charged to whichever capture happens to sort first.
    if let Some(first) = workload.items.first() {
        let _ = engine.recognize(&first.image);
    }
    for item in &workload.items {
        let started = Instant::now();
        match engine.recognize(&item.image) {
            Ok(text) => {
                result.seconds.push(started.elapsed().as_secs_f64());
                result.rates.insert(
                    item.id.clone(),
                    character_error_rate(&normalize(&item.truth), &normalize(&text)),
                );
                result
                    .texts
                    .insert(item.id.clone(), sha256(text.as_bytes()));
            }
            Err(error) => result.failed.push((item.id.clone(), error.to_string())),
        }
    }
    result
}

fn mean(values: impl IntoIterator<Item = f64>) -> f64 {
    let values: Vec<f64> = values.into_iter().collect();
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(sorted.len() / 2).copied().unwrap_or(0.0)
}

/// Exact two-sided sign test over the items two engines read differently.
fn sign_test(wins: usize, losses: usize) -> f64 {
    let n = wins + losses;
    if n == 0 {
        return 1.0;
    }
    let k = wins.min(losses);
    let mut tail = 0.0f64;
    let mut term = 0.5f64.powi(n as i32); // C(n, 0) / 2^n
    for i in 0..=k {
        if i > 0 {
            term *= (n - i + 1) as f64 / i as f64;
        }
        tail += term;
    }
    (2.0 * tail).min(1.0)
}

/// The engines that are installed, each with the engine it is compared against.
fn engines() -> Vec<(String, Box<dyn Ocr>, Option<&'static str>)> {
    let mut found: Vec<(String, Box<dyn Ocr>, Option<&'static str>)> = Vec::new();
    let paddle = model_dir("ppocrv6").filter(|d| PaddleOcr::is_present(d));
    if let Some(dir) = &paddle {
        found.push((
            "pp-ocrv6".into(),
            Box::new(PaddleOcr::open(dir).expect("PP-OCRv6 loads")),
            None,
        ));
    }
    let manga = model_dir("manga-ocr").filter(|d| MangaOcr::is_present(d));
    if let Some(dir) = &manga {
        found.push((
            "manga-ocr".into(),
            Box::new(MangaOcr::load(dir).expect("manga-ocr loads")),
            None,
        ));
    }
    if let (Some(manga), Some(paddle)) = (&manga, &paddle) {
        found.push((
            "auto".into(),
            Box::new(AutoOcr::new(manga.clone(), paddle.clone())),
            None,
        ));
    }
    if let Ok(endpoint) = std::env::var("SAKATSU_VLM_ENDPOINT") {
        let model = std::env::var("SAKATSU_VLM_MODEL").unwrap_or_default();
        let label = std::env::var("SAKATSU_VLM_LABEL").unwrap_or_else(|_| "vlm".into());
        match VlmOcr::new(&endpoint, &model) {
            Ok(engine) if engine.is_reachable() => {
                found.push((label, Box::new(engine), Some("manga-ocr")));
            }
            _ => println!("VLM at {endpoint} is not reachable — not scored"),
        }
    }
    // `SAKATSU_OCR_ENGINES=a,b` scores only those — and always the engines they are compared
    // against, so a filtered run still carries its own baseline rather than borrowing one from
    // another run.
    if let Ok(wanted) = std::env::var("SAKATSU_OCR_ENGINES") {
        let wanted: Vec<&str> = wanted.split(',').map(str::trim).collect();
        let baselines: Vec<&str> = found
            .iter()
            .filter(|(name, _, _)| wanted.contains(&name.as_str()))
            .filter_map(|(_, _, baseline)| *baseline)
            .collect();
        found.retain(|(name, _, _)| {
            wanted.contains(&name.as_str()) || baselines.contains(&name.as_str())
        });
    }
    found
}

#[test]
#[ignore = "needs the OCR models and the capture corpus; release build"]
fn every_engine_on_both_workloads_with_complete_accounting() {
    if cfg!(debug_assertions) {
        panic!("timings from a debug build are not comparable — run with --release");
    }
    let mut inventory = Vec::new();
    let whole = whole_captures(&mut inventory);
    let manifest = std::env::var_os("SAKATSU_BUBBLE_MANIFEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/evaluation/bubble-crops.json")
        });
    let bubbles = bubble_crops(&manifest);
    let inventory_sha = sha256(serde_json::to_string(&inventory).unwrap().as_bytes());

    println!("=== inventory ===");
    println!(
        "  {} images, digest {}",
        inventory.len(),
        &inventory_sha[..16]
    );
    let mut workloads = vec![whole];
    match bubbles {
        Some((workload, digest)) => {
            println!(
                "  bubble manifest {} ({})",
                manifest.display(),
                &digest[..16]
            );
            workloads.push(workload);
        }
        None => println!(
            "  bubble crops NOT RUN — no manifest at {}",
            manifest.display()
        ),
    }
    println!("  ONNX Runtime: library defaults for every in-process engine; warm-up excluded");

    let found = engines();
    assert!(!found.is_empty(), "no OCR engine is installed");
    let mut results = Vec::new();
    for workload in &workloads {
        let selected = workload.items.len() + workload.failed.len();
        println!(
            "\n=== {} — {selected} selected, {} excluded ===",
            workload.name,
            workload.excluded.len()
        );
        let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, reason) in &workload.excluded {
            *reasons.entry(reason.as_str()).or_default() += 1;
        }
        for (reason, count) in &reasons {
            println!("  excluded: {count:>3}  {reason}");
        }
        println!(
            "  {:<22} {:>5} {:>5} {:>6} {:>9} {:>7} {:>8} {:>8}",
            "engine", "done", "fail", "exact", "mean CER", "s/item", "p50 s", "vs"
        );
        let mut runs: Vec<(String, Run, Option<&'static str>)> = Vec::new();
        for (name, engine, baseline) in &found {
            let run = run(engine.as_ref(), workload);
            // Accounting: every selected item is either a rate or a failure, never neither.
            assert_eq!(
                run.rates.len() + run.failed.len(),
                selected,
                "{name}: accounting does not close on {}",
                workload.name
            );
            runs.push((name.clone(), run, *baseline));
        }
        for (name, run, baseline) in &runs {
            let exact = run.rates.values().filter(|r| **r == 0.0).count();
            let comparison = baseline
                .and_then(|base| runs.iter().find(|(n, _, _)| n == base))
                .map(|(base, base_run, _)| {
                    let (mut wins, mut losses) = (0, 0);
                    for (id, rate) in &run.rates {
                        if let Some(other) = base_run.rates.get(id) {
                            if rate < other {
                                wins += 1;
                            } else if rate > other {
                                losses += 1;
                            }
                        }
                    }
                    (base.clone(), wins, losses, sign_test(wins, losses))
                });
            println!(
                "  {:<22} {:>5} {:>5} {:>6} {:>9.4} {:>7.3} {:>8.3} {}",
                name,
                run.rates.len(),
                run.failed.len(),
                exact,
                mean(run.rates.values().copied()),
                mean(run.seconds.iter().copied()),
                median(&run.seconds),
                comparison
                    .as_ref()
                    .map(|(base, w, l, p)| format!("{base}: better {w}, worse {l}, p={p:.3}"))
                    .unwrap_or_default()
            );
            for (id, reason) in &run.failed {
                println!("      failed {id}: {reason}");
            }
            let by_domain: BTreeMap<&str, f64> = ["browser", "image-viewer", "game"]
                .iter()
                .filter_map(|d| {
                    let rates: Vec<f64> = workload
                        .items
                        .iter()
                        .filter(|item| item.domain == *d)
                        .filter_map(|item| run.rates.get(&item.id).copied())
                        .collect();
                    (!rates.is_empty()).then(|| (*d, mean(rates)))
                })
                .collect();
            results.push(json!({
                "workload": workload.name, "engine": name, "selected": selected,
                "completed": run.rates.len(), "failed": run.failed,
                "excluded": workload.excluded.len(),
                "mean_cer": mean(run.rates.values().copied()), "exact": exact,
                "mean_seconds": mean(run.seconds.iter().copied()), "median_seconds": median(&run.seconds),
                "by_domain": by_domain,
                "comparison": comparison.map(|(base, w, l, p)| json!({"baseline": base, "better": w, "worse": l, "p": p})),
                "rates": run.rates,
                "text_sha256": run.texts,
            }));
        }
    }

    let artifact = json!({
        "inventory_sha256": inventory_sha,
        "inventory": inventory,
        "results": results,
    });
    let text = serde_json::to_string_pretty(&artifact).unwrap();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/evaluation");
    std::fs::create_dir_all(&dir).expect("target/evaluation");
    let path = dir.join(format!(
        "ocr-engines-{}.json",
        &sha256(text.as_bytes())[..16]
    ));
    std::fs::write(&path, text).expect("the artifact writes");
    println!("\n  artifact  {} (private)", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sign_test_is_exact_and_two_sided() {
        assert_eq!(sign_test(0, 0), 1.0);
        // 9 wins, 1 loss: 2 × (1 + 10) / 1024.
        assert!((sign_test(9, 1) - 22.0 / 1024.0).abs() < 1e-12);
        assert_eq!(sign_test(3, 3), 1.0_f64.min(2.0 * 42.0 / 64.0));
    }
}
