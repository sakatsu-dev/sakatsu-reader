//! What the Rust PP-OCR pipeline reads, line by line, for comparison with PaddleOCR's own.
//!
//! A new export is offered only after the pipeline here has been compared with PaddlePaddle's
//! own Python pipeline on the same images (`local documentation`, "PP-OCRv6 parity"). This
//! test is the Rust half: it writes every detected box and its text, so the comparison can say
//! *where* the two disagree — detection, cropping or decoding — rather than only that they do.
//!
//! ```text
//! $env:SAKATSU_PARITY_MODEL = "$env:SAKATSU_EVAL_DIR\models\ppocrv6"
//! $env:SAKATSU_PARITY_OUT = '...\target\evaluation\parity-rust-v6.json'
//! cargo test --release -p sakatsu-core --test paddle_parity -- --ignored --nocapture
//! ```
//!
//! Development captures only: the seven reading-holdout stems are excluded and counted.

mod common;

use sakatsu_core::ocr::{OcrImage, PaddleOcr};
use serde_json::json;

#[test]
#[ignore = "needs a PP-OCR export and the capture corpus"]
fn the_rust_pipeline_lines_are_written_for_comparison() {
    let model = std::env::var("SAKATSU_PARITY_MODEL").expect("SAKATSU_PARITY_MODEL");
    let out = std::env::var("SAKATSU_PARITY_OUT").expect("SAKATSU_PARITY_OUT");
    let engine = PaddleOcr::open(std::path::Path::new(&model))
        .unwrap_or_else(|error| panic!("{model} does not load: {error}"));

    let dir = common::corpus_dir();
    let holdout = common::captures::reading_holdout_stems().expect("the holdout list reads");
    let (mut images, mut excluded, mut failed) = (Vec::new(), 0usize, Vec::new());
    for path in common::corpus_images(&dir) {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        if holdout.contains(&stem) {
            excluded += 1;
            continue;
        }
        if !path.with_extension("txt").is_file() {
            continue;
        }
        let decoded = match image::open(&path) {
            Ok(decoded) => decoded.to_rgba8(),
            Err(error) => {
                failed.push(json!({"image": path.file_name().unwrap().to_string_lossy(), "reason": error.to_string()}));
                continue;
            }
        };
        let (width, height) = decoded.dimensions();
        let image = OcrImage::new(width, height, decoded.into_raw()).expect("a decoded image");
        match engine.read_lines(&image) {
            Ok(lines) => images.push(json!({
                "image": path.file_name().unwrap().to_string_lossy(),
                "lines": lines.iter().map(|line| json!({
                    "bounds": [line.bounds.0, line.bounds.1, line.bounds.2, line.bounds.3],
                    "text": line.text,
                    "confidence": line.confidence,
                })).collect::<Vec<_>>(),
            })),
            Err(error) => failed.push(json!({"image": path.file_name().unwrap().to_string_lossy(), "reason": error.to_string()})),
        }
    }
    assert!(
        !images.is_empty(),
        "no development captures at {}",
        dir.display()
    );
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&json!({
            "model": model,
            "holdout_excluded": excluded,
            "failed": failed,
            "images": images,
        }))
        .expect("serialises"),
    )
    .expect("the output is writable");
    println!(
        "PP-OCRv6: {} images read, {} failed, {excluded} holdout excluded -> {out}",
        images.len(),
        failed.len()
    );
    assert!(failed.is_empty(), "every development capture must read");
}
