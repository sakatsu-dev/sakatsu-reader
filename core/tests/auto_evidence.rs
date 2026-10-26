//! What Auto had to decide on, per capture — written once, so routing rules can be compared
//! without reading every capture again.
//!
//! Retuning Auto for PP-OCRv6 (2026-10-09) needed the same evidence under several candidate
//! rules. Each rule only re-decides between two reads that do not change, so this records them:
//! PP-OCRv6's text with the evidence Auto routes on (vertical, lines detected, lines accepted),
//! every line's own confidence, manga-ocr's text, and the reviewed transcript.
//!
//! **Split before anything was scored.** Each development capture is assigned to *tune* or
//! *check* by a hash of its file name — `sha256(name)[0] % 3 == 0` is check — and the assignment
//! is written with the evidence. Candidate rules are compared on *tune* only; the one adopted is
//! scored once on *check*. The seven reading-holdout captures are excluded as always.
//!
//! ```text
//! $env:SAKATSU_EVAL_MODEL_DIR = "$env:SAKATSU_EVAL_DIR\models"
//! cargo test --release -p sakatsu-core --test auto_evidence -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The output is private — it holds transcripts — and goes to `target/evaluation`.

mod common;

use std::path::PathBuf;

use sakatsu_core::ocr::score::{character_error_rate, normalize};
use sakatsu_core::ocr::{MangaOcr, Ocr, OcrImage, PaddleOcr};
use serde_json::json;
use sha2::{Digest, Sha256};

fn model_dir(name: &str) -> Option<PathBuf> {
    [
        std::env::var_os("SAKATSU_EVAL_MODEL_DIR").map(PathBuf::from),
        std::env::var_os("SAKATSU_MODEL_DIR").map(PathBuf::from),
        std::env::var_os("APPDATA").map(|appdata| {
            PathBuf::from(appdata)
                .join("com.sakatsu.app")
                .join("models")
        }),
    ]
    .into_iter()
    .flatten()
    .map(|root| root.join(name))
    .find(|dir| dir.is_dir())
}

/// Tune or check, from the file name alone, so the split cannot depend on any result.
fn split(name: &str) -> &'static str {
    if Sha256::digest(name.as_bytes())[0] % 3 == 0 {
        "check"
    } else {
        "tune"
    }
}

#[test]
#[ignore = "needs PP-OCRv6, manga-ocr and the capture corpus"]
fn the_evidence_auto_routes_on_is_recorded_per_capture() {
    let paddle_dir = model_dir("ppocrv6")
        .filter(|d| PaddleOcr::is_present(d))
        .expect("PP-OCRv6 is installed");
    let manga_dir = model_dir("manga-ocr")
        .filter(|d| MangaOcr::is_present(d))
        .expect("manga-ocr is installed");
    let paddle = PaddleOcr::open(&paddle_dir).expect("PP-OCRv6 loads");
    let manga = MangaOcr::load(&manga_dir).expect("manga-ocr loads");

    let dir = common::corpus_dir();
    let holdout = common::captures::reading_holdout_stems().expect("the holdout list reads");
    let mut rows = Vec::new();
    for path in common::corpus_images(&dir) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        if holdout.contains(&stem) || !path.with_extension("txt").is_file() {
            continue;
        }
        let decoded = image::open(&path)
            .expect("a corpus image decodes")
            .to_rgba8();
        let (width, height) = decoded.dimensions();
        let image = OcrImage::new(width, height, decoded.into_raw()).expect("an image");
        let (text, vertical, detected, accepted) =
            paddle.routing_evidence(&image).expect("PP-OCRv6 reads");
        let lines = paddle.read_lines(&image).expect("PP-OCRv6 reads its lines");
        let manga_text = manga.recognize(&image).expect("manga-ocr reads");
        let truth = std::fs::read_to_string(path.with_extension("txt")).expect("a transcript");
        rows.push(json!({
            "image": name,
            "split": split(&name),
            "paddle": text,
            "vertical": vertical,
            "detected": detected,
            "accepted": accepted,
            "confidences": lines.iter().map(|line| line.confidence).collect::<Vec<_>>(),
            "manga": manga_text,
            // Scored here, by the same functions every harness uses, so a rule compared offline
            // is scored exactly as the app's own measurements would score it.
            "cer_paddle": character_error_rate(&normalize(&truth), &normalize(&text)),
            "cer_manga": character_error_rate(&normalize(&truth), &normalize(&manga_text)),
            "agree": normalize(&text) == normalize(&manga_text),
            "truth": truth,
        }));
    }
    let tune = rows.iter().filter(|row| row["split"] == "tune").count();
    assert!(!rows.is_empty(), "no development captures");
    let out =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/evaluation/auto-evidence.json");
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&json!({"rows": rows})).unwrap(),
    )
    .expect("the evidence writes");
    println!(
        "{} captures ({tune} tune, {} check) -> {}",
        rows.len(),
        rows.len() - tune,
        out.display()
    );
}
