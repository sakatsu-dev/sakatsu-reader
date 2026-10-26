//! PP-OCRv6 against the real models.
//!
//! Ignored by default: needs `scripts/fetch-paddle-ocr.ps1` (~30 MB) and CI has no
//! models. Run:
//!
//! ```text
//! cargo test -p sakatsu-core --test paddle_ocr_live -- --ignored --nocapture
//! ```
//!
//! Images are rendered here rather than committed, so the test carries no fixture and no
//! licence question. Unlike the manga-ocr test, these exercise the part that is genuinely
//! new: this is a *pipeline*, so what is being checked is that detection finds the lines,
//! that they come back in the right order, and that a vertical column is turned before
//! being read rather than fed in sideways.

use std::path::PathBuf;

use sakatsu_core::ocr::{Ocr, OcrImage, PaddleOcr};

mod common;
use common::render_ocr_lines;

fn model_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let appdata = std::env::var_os("APPDATA").expect("APPDATA (or set SAKATSU_MODEL_DIR)");
    PathBuf::from(appdata)
        .join("com.sakatsu.app")
        .join("models")
        .join("ppocrv6")
}

fn engine() -> PaddleOcr {
    let dir = model_dir();
    PaddleOcr::open(&dir).unwrap_or_else(|error| {
        panic!(
            "no models at {} ({error}) — run scripts/fetch-paddle-ocr.ps1",
            dir.display()
        )
    })
}

#[test]
#[ignore = "needs scripts/fetch-paddle-ocr.ps1"]
fn it_reads_a_single_line() {
    let engine = engine();
    assert_eq!(engine.engine(), "pp-ocrv6");

    for expected in ["水を飲む", "気持ちが伝わってきます", "先生"] {
        let image = render_ocr_lines(&[expected], false);
        let started = std::time::Instant::now();
        let text = engine.recognize(&image).expect("recognize");
        println!(
            "{:>5}x{:<4} → {text:?}   (expected {expected:?}, {:.2}s)",
            image.width(),
            image.height(),
            started.elapsed().as_secs_f64()
        );
        assert_eq!(text.trim(), expected, "read {text:?}");
    }
}

/// The reason this engine exists: a text box has several lines, and the recogniser only
/// reads one at a time. Detection has to find them and reading order has to hold.
#[test]
#[ignore = "needs scripts/fetch-paddle-ocr.ps1"]
fn it_reads_several_lines_in_order() {
    let engine = engine();
    // Deliberately strings this model reads cleanly. What is under test is the pipeline —
    // that detection finds both lines and reading order puts them the right way round —
    // not the recogniser's accuracy, which has its own failure modes and would make this
    // test fail for reasons that say nothing about the code.
    let lines = ["気持ちが伝わってきます", "本を読んでいます"];
    let image = render_ocr_lines(&lines, false);

    let text = engine.recognize(&image).expect("recognize");
    println!("multi-line →\n{text}");

    let read: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(
        read.len(),
        lines.len(),
        "found {} lines: {read:?}",
        read.len()
    );
    for (got, want) in read.iter().zip(lines.iter()) {
        assert_eq!(got, want);
    }
}

/// The trap. A column of Japanese is a tall narrow box, and a recogniser trained on
/// horizontal lines reads it as noise unless the pipeline turns it first.
#[test]
#[ignore = "needs scripts/fetch-paddle-ocr.ps1"]
fn it_turns_a_vertical_column_before_reading_it() {
    let engine = engine();
    let expected = "水を飲む";
    let image = render_ocr_lines(&[expected], true);
    println!("vertical image is {}x{}", image.width(), image.height());
    assert!(image.height() > image.width(), "the render is a column");

    let text = engine.recognize(&image).expect("recognize");
    println!("vertical → {text:?} (expected {expected:?})");

    // Every character should survive, whatever order detection reported the boxes in.
    for character in expected.chars() {
        assert!(
            text.contains(character),
            "{character:?} missing from {text:?} — a column read sideways is noise"
        );
    }
}

#[test]
#[ignore = "needs scripts/fetch-paddle-ocr.ps1"]
fn it_reads_vertical_columns_from_right_to_left() {
    let engine = engine();
    let columns = ["水を飲む", "本を読む"];
    let image = render_ocr_lines(&columns, true);

    let text = engine.recognize(&image).expect("recognize");
    println!("vertical columns →\n{text}");

    let read: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(
        read, columns,
        "vertical Japanese columns must read from right to left"
    );
}

#[test]
#[ignore = "needs scripts/fetch-paddle-ocr.ps1"]
fn an_empty_image_is_refused_before_any_inference() {
    let engine = engine();
    let empty = OcrImage::new(0, 0, Vec::new()).expect("valid");
    assert!(engine.recognize(&empty).is_err());
}
