//! One identical vertical Japanese image through every OCR engine.
//!
//! This is deliberately a comparison report rather than an accuracy gate. The rendered
//! fixture checks integration and reading order, but it is too clean to stand in for manga;
//! real accuracy belongs to `ocr_corpus.rs` once its transcriptions are human-approved.
//!
//! ```text
//! $env:SAKATSU_VLM_ENDPOINT = "http://127.0.0.1:8000"
//! $env:SAKATSU_VLM_MODEL = "PaddlePaddle/PaddleOCR-VL-1.6"
//! cargo test -p sakatsu-core --test ocr_vertical_comparison -- --ignored --nocapture
//! ```

mod common;

use std::path::PathBuf;
use std::time::Instant;

use common::render_ocr_lines;
use sakatsu_core::ocr::score::character_error_rate;
use sakatsu_core::ocr::{MangaOcr, Ocr, PaddleOcr, VlmOcr};

fn models(name: &str) -> PathBuf {
    let appdata = std::env::var_os("APPDATA").expect("APPDATA");
    PathBuf::from(appdata)
        .join("com.sakatsu.app")
        .join("models")
        .join(name)
}

fn run(label: &str, engine: &dyn Ocr, image: &sakatsu_core::ocr::OcrImage, expected: &str) {
    let started = Instant::now();
    let got = engine
        .recognize(image)
        .unwrap_or_else(|error| panic!("{label} failed: {error}"));
    let elapsed = started.elapsed().as_secs_f64();
    let error = character_error_rate(expected, &got);
    println!("{label:<14} {elapsed:>6.2}s  CER {error:.3}  {got:?}");
    assert!(!got.trim().is_empty(), "{label} returned no Japanese");
}

#[test]
#[ignore = "needs all OCR models and a served VLM"]
fn the_installed_engines_read_the_same_vertical_columns() {
    let columns = ["水を飲む", "本を読む"];
    let expected = columns.join("\n");
    let image = render_ocr_lines(&columns, true);

    let manga_dir = models("manga-ocr");
    let paddle_dir = models("ppocrv6");
    if !MangaOcr::is_present(&manga_dir) || !PaddleOcr::is_present(&paddle_dir) {
        println!("SKIP: install manga-ocr and PP-OCRv6 before comparing all three engines");
        return;
    }

    let endpoint =
        std::env::var("SAKATSU_VLM_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8000".into());
    let model = std::env::var("SAKATSU_VLM_MODEL")
        .unwrap_or_else(|_| "PaddlePaddle/PaddleOCR-VL-1.6".into());
    let vlm = match VlmOcr::new(&endpoint, &model) {
        Ok(engine) => engine,
        Err(error) => {
            println!("SKIP: invalid PaddleOCR-VL endpoint {endpoint:?}: {error}");
            return;
        }
    };
    if !vlm.is_reachable() {
        println!("SKIP: no PaddleOCR-VL server is answering at {endpoint}");
        return;
    }

    let manga = MangaOcr::load(&manga_dir).expect("load manga-ocr");
    let paddle = PaddleOcr::open(&paddle_dir).expect("load PP-OCRv6");

    println!(
        "identical {}x{} vertical fixture; expected {expected:?}",
        image.width(),
        image.height()
    );
    run("manga-ocr", &manga, &image, &expected);
    run("PP-OCRv6", &paddle, &image, &expected);
    run("PaddleOCR-VL", &vlm, &image, &expected);
}
