//! Suggest ground truth for corpus images that have none.
//!
//! ```text
//! cargo test -p sakatsu-core --test corpus_bootstrap -- --ignored --nocapture
//! ```
//!
//! `ocr_corpus.rs` skips any image without a `.txt` beside it, and writing those by hand is
//! the entire cost of a corpus. This runs the two fast engines over everything that lacks
//! one and leaves a `<stem>.txt.suggested` to correct, so the job becomes reading a crop and
//! fixing a candidate rather than transcribing blind.
//!
//! # It cannot write ground truth, and that is the point
//!
//! It only ever writes `.txt.suggested`. **Renaming to `.txt` is a human act**, and it is
//! the only thing that turns a machine's guess into a truth worth scoring against. A tool
//! that wrote `.txt` directly could assemble a corpus that its own engines score perfectly
//! on, and the table built from it would measure nothing at all.
//!
//! For the same reason it never touches an image that already has a `.txt`.
//!
//! # Read the image, not the suggestion
//!
//! Correcting an engine's output anchors you to it: an error you skim past is scored as a
//! success for whichever engine made it. Both readings are printed for exactly this reason —
//! two contradictory candidates cannot both be accepted passively. Where they disagree is
//! also where the corpus does its work, so the disagreement list at the end is the order
//! worth transcribing in.

mod common;

use std::path::Path;

use sakatsu_core::ocr::score::normalize;
use sakatsu_core::ocr::{MangaOcr, Ocr, OcrImage, PaddleOcr};

/// The suffix appended to the image's stem. Deliberately not `.txt`.
const SUGGESTED: &str = "txt.suggested";

fn model_root() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("SAKATSU_MODEL_DIR") {
        return Some(std::path::PathBuf::from(dir));
    }
    std::env::var_os("APPDATA").map(|appdata| {
        std::path::PathBuf::from(appdata)
            .join("com.sakatsu.app")
            .join("models")
    })
}

fn read_image(path: &Path) -> Option<OcrImage> {
    let decoded = image::open(path)
        .map_err(|error| eprintln!("skipping {}: {error}", path.display()))
        .ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    OcrImage::new(width, height, rgba.into_raw())
        .map_err(|error| eprintln!("skipping {}: {error}", path.display()))
        .ok()
}

/// One engine's reading, or a note saying why there isn't one.
fn read_with(engine: Option<&dyn Ocr>, image: &OcrImage) -> Option<String> {
    let engine = engine?;
    match engine.recognize(image) {
        Ok(text) => Some(text),
        Err(error) => {
            eprintln!("  {} failed: {error}", engine.engine());
            None
        }
    }
}

/// The file a human corrects. Comments name the engine so a wrong reading can be
/// attributed rather than just deleted.
fn suggestion(readings: &[(&str, Option<String>)], agree: bool) -> String {
    let mut out = String::new();
    out.push_str("# Correct this against the IMAGE, then rename to .txt\n");
    if !agree {
        out.push_str("# The engines disagree — this sample is worth keeping.\n");
    }
    for (name, reading) in readings {
        match reading {
            Some(text) => {
                out.push_str(&format!("# {name}\n"));
                out.push_str(text.trim());
                out.push('\n');
            }
            None => out.push_str(&format!("# {name}: no reading\n")),
        }
    }
    out
}

#[test]
#[ignore = "needs the OCR models and a corpus"]
fn suggest_ground_truth_for_untranscribed_images() {
    let dir = common::corpus_dir();
    let images = common::corpus_images(&dir);
    if images.is_empty() {
        println!("SKIP: no images at {}", dir.display());
        return;
    }

    // Only what has no truth yet. Existing work is never reconsidered.
    let pending: Vec<_> = images
        .into_iter()
        .filter(|path| !path.with_extension("txt").is_file())
        .collect();
    println!(
        "{} image(s) without ground truth in {}",
        pending.len(),
        dir.display()
    );
    if pending.is_empty() {
        return;
    }

    let Some(models) = model_root() else {
        println!("SKIP: no model directory (set SAKATSU_MODEL_DIR)");
        return;
    };

    let manga = MangaOcr::load(&models.join("manga-ocr"))
        .map_err(|error| eprintln!("manga-ocr unavailable: {error}"))
        .ok();
    let paddle = PaddleOcr::open(&models.join("ppocrv6"))
        .map_err(|error| eprintln!("pp-ocrv6 unavailable: {error}"))
        .ok();
    if manga.is_none() && paddle.is_none() {
        println!("SKIP: neither engine is installed — nothing to suggest with");
        return;
    }

    let mut disagreed = Vec::new();
    let mut written = 0usize;

    for path in &pending {
        let Some(image) = read_image(path) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let manga_text = read_with(manga.as_ref().map(|e| e as &dyn Ocr), &image);
        let paddle_text = read_with(paddle.as_ref().map(|e| e as &dyn Ocr), &image);

        // Compared normalised: where an engine breaks a line is a property of its pipeline,
        // not of its reading, so it must not count as a disagreement.
        let agree = match (&manga_text, &paddle_text) {
            (Some(left), Some(right)) => normalize(left) == normalize(right),
            // One engine alone cannot corroborate anything, so treat it as worth a look.
            _ => false,
        };
        if !agree {
            disagreed.push(name.clone());
        }

        let body = suggestion(
            &[("manga-ocr", manga_text), ("pp-ocrv6", paddle_text)],
            agree,
        );
        let out = path.with_extension(SUGGESTED);
        match std::fs::write(&out, body) {
            Ok(()) => written += 1,
            Err(error) => eprintln!("could not write {}: {error}", out.display()),
        }
    }

    println!("\nwrote {written} suggestion(s)");
    println!(
        "\n{} sample(s) where the engines disagree — transcribe these first, they are the \
         ones that separate the engines:",
        disagreed.len()
    );
    for name in &disagreed {
        println!("  {name}");
    }
    println!("\nCorrect each against the image, then rename .{SUGGESTED} to .txt.");
}

/// The property that keeps ground truth human-made.
///
/// Asserted rather than trusted: the whole value of the corpus rests on no machine having
/// written a `.txt`, and this is a single `with_extension` away from being wrong.
#[test]
fn the_bootstrap_only_ever_writes_a_suggestion() {
    assert_ne!(SUGGESTED, "txt");
    assert!(
        SUGGESTED.ends_with(".suggested"),
        "a corrected file is renamed by a person; nothing here may produce a .txt"
    );

    let path = Path::new("corpus/manga-01.png").with_extension(SUGGESTED);
    assert_eq!(path.file_name().unwrap(), "manga-01.txt.suggested");
    assert!(!path.to_string_lossy().ends_with(".txt"));
}

#[test]
fn a_suggestion_names_the_engine_behind_each_reading_and_flags_disagreement() {
    let body = suggestion(
        &[
            ("manga-ocr", Some("ともだち".into())),
            ("pp-ocrv6", Some("とちだも".into())),
        ],
        false,
    );
    assert!(
        body.contains("# manga-ocr"),
        "an unattributed reading cannot be judged"
    );
    assert!(body.contains("# pp-ocrv6"));
    assert!(
        body.contains("disagree"),
        "the reader is told this one is worth keeping"
    );
    assert!(
        body.contains("against the IMAGE"),
        "the anchoring warning travels with the file"
    );

    // An engine that failed says so rather than silently contributing nothing.
    let partial = suggestion(
        &[("manga-ocr", Some("あ".into())), ("pp-ocrv6", None)],
        false,
    );
    assert!(partial.contains("# pp-ocrv6: no reading"));
}

/// Images already transcribed are invisible to this harness.
#[test]
fn an_image_that_already_has_ground_truth_is_left_alone() {
    let dir = common::Scratch::new("corpus-bootstrap", "skip-existing");
    std::fs::write(dir.join("done.png"), []).expect("write");
    std::fs::write(dir.join("done.txt"), "既に").expect("write");
    std::fs::write(dir.join("todo.png"), []).expect("write");

    let pending: Vec<String> = common::corpus_images(&dir)
        .into_iter()
        .filter(|path| !path.with_extension("txt").is_file())
        .filter_map(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    assert_eq!(pending, ["todo.png"]);
}
