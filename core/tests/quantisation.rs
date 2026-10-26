//! Do the smaller manga-ocr exports read the same text as the one we ship?
//!
//! ```text
//! pwsh -File scripts/fetch-manga-ocr-variants.ps1
//! cargo test -p sakatsu-core --test quantisation -- --ignored --nocapture
//! ```
//!
//! # Why this needs no ground truth
//!
//! `ocr_corpus.rs` cannot say anything until somebody transcribes the corpus, because it
//! asks *is this reading correct*. This asks a narrower question — **does the small export
//! read what the big one reads** — and for that the fp32 model is its own reference. The
//! corpus images are enough on their own.
//!
//! That narrowness is the point and also the limit. A variant scoring zero here is
//! faithful, not accurate: if the fp32 model misreads a crop, a variant that misreads it
//! identically scores perfectly. What this can establish is that quantising costs nothing,
//! which is the only question that has to be answered before shrinking the install from
//! 461 MB to 74 MB.
//!
//! # What it prints
//!
//! Per variant: mean character error rate against fp32, how many crops came back
//! character-for-character identical, seconds per crop, and megabytes on disk. A variant
//! that is materially smaller and identical almost everywhere is a straight win; one that
//! diverges on a tenth of the corpus is a trade to think about, and the disagreement list
//! says which crops to look at.

mod common;

use std::path::{Path, PathBuf};
use std::time::Instant;

use sakatsu_core::ocr::score::{character_error_rate, normalize};
use sakatsu_core::ocr::{MangaOcr, Ocr, OcrImage};

/// The directory the app itself installs into — the reference every variant is judged by.
const BASELINE: &str = "manga-ocr";

fn model_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SAKATSU_MODEL_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("APPDATA").map(|appdata| {
        PathBuf::from(appdata)
            .join("com.sakatsu.app")
            .join("models")
    })
}

/// Megabytes of ONNX in a model directory — the number the whole exercise is about.
fn weight_megabytes(dir: &Path) -> f64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "onnx"))
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len() as f64)
        .sum::<f64>()
        / 1e6
}

/// Every `manga-ocr-*` directory beside the baseline, sorted.
fn variants(root: &Path) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            let label = name.strip_prefix("manga-ocr-")?.to_string();
            MangaOcr::is_present(&path).then_some((label, path))
        })
        .collect();
    found.sort();
    found
}

fn read_all(engine: &MangaOcr, images: &[(String, OcrImage)]) -> (Vec<String>, f64) {
    let started = Instant::now();
    let out = images
        .iter()
        .map(|(name, image)| {
            engine.recognize(image).unwrap_or_else(|error| {
                eprintln!("  {name} failed: {error}");
                String::new()
            })
        })
        .collect();
    (out, started.elapsed().as_secs_f64())
}

#[test]
#[ignore = "needs the OCR models, the variants, and a corpus"]
fn the_smaller_exports_are_measured_against_the_one_we_ship() {
    let dir = common::corpus_dir();
    let paths = common::corpus_images(&dir);
    if paths.is_empty() {
        println!("SKIP: no images at {}", dir.display());
        return;
    }
    let Some(root) = model_root() else {
        println!("SKIP: no model directory (set SAKATSU_MODEL_DIR)");
        return;
    };

    let baseline_dir = root.join(BASELINE);
    if !MangaOcr::is_present(&baseline_dir) {
        println!(
            "SKIP: no baseline at {} — run scripts/fetch-ocr-model.ps1",
            baseline_dir.display()
        );
        return;
    }
    let found = variants(&root);
    if found.is_empty() {
        println!(
            "SKIP: no manga-ocr-* variants under {} — run scripts/fetch-manga-ocr-variants.ps1",
            root.display()
        );
        return;
    }

    let images: Vec<(String, OcrImage)> = paths
        .iter()
        .filter_map(|path| {
            let decoded = image::open(path).ok()?;
            let rgba = decoded.to_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            let name = path.file_stem()?.to_string_lossy().into_owned();
            Some((name, OcrImage::new(w, h, rgba.into_raw()).ok()?))
        })
        .collect();
    println!("{} crops from {}", images.len(), dir.display());

    let engine = MangaOcr::load(&baseline_dir).expect("baseline loads");
    let (reference, seconds) = read_all(&engine, &images);
    drop(engine);
    println!(
        "\nbaseline {BASELINE}: {:.0} MB, {:.2} s per crop",
        weight_megabytes(&baseline_dir),
        seconds / images.len() as f64
    );

    println!(
        "\n{:<12} {:>7} {:>9} {:>11} {:>10}",
        "variant", "MB", "s/crop", "identical", "CER vs fp32"
    );

    let mut diverged: Vec<(String, Vec<String>)> = Vec::new();
    for (label, path) in &found {
        let engine = match MangaOcr::load(path) {
            Ok(engine) => engine,
            Err(error) => {
                println!("{label:<12} failed to load: {error}");
                continue;
            }
        };
        let (got, seconds) = read_all(&engine, &images);
        drop(engine);

        let mut identical = 0usize;
        let mut total = 0.0f64;
        let mut differs = Vec::new();
        for (index, text) in got.iter().enumerate() {
            let want = normalize(&reference[index]);
            let have = normalize(text);
            if want == have {
                identical += 1;
            } else {
                differs.push(images[index].0.clone());
            }
            total += character_error_rate(&want, &have);
        }
        let n = got.len().max(1);
        println!(
            "{label:<12} {:>7.0} {:>9.2} {:>7}/{:<3} {:>10.4}",
            weight_megabytes(path),
            seconds / n as f64,
            identical,
            got.len(),
            total / n as f64
        );
        diverged.push((label.clone(), differs));
    }

    // Which crops a variant changed its mind about is the useful part: a variant that is
    // identical everywhere needs no further thought, and one that is not needs eyes on
    // exactly these.
    for (label, differs) in &diverged {
        if differs.is_empty() {
            continue;
        }
        println!("\n{label} differs on {} crop(s):", differs.len());
        for name in differs.iter().take(12) {
            println!("  {name}");
        }
        if differs.len() > 12 {
            println!("  … and {} more", differs.len() - 12);
        }
    }

    // No assertion, for the reason `ocr_corpus.rs` gives: there is no error rate this
    // project has earned the right to require. Read the table.
}

/// A variant directory has to be loadable by the ordinary loader, or the comparison is
/// measuring the harness rather than the model.
#[test]
fn a_variant_directory_looks_exactly_like_a_model_directory() {
    let root = common::Scratch::new("quantisation", "variant-shape");
    let dir = root.join("manga-ocr-fp16");
    std::fs::create_dir_all(&dir).expect("mkdir");
    for name in ["encoder_model.onnx", "decoder_model.onnx", "vocab.txt"] {
        std::fs::write(dir.join(name), []).expect("write");
    }
    // The plain names matter: the fetch script renames `encoder_model_fp16.onnx` on the way
    // in precisely so that `MangaOcr::load` needs no variant-awareness at all.
    assert!(MangaOcr::is_present(&dir));

    let found = variants(&root);
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].0, "fp16",
        "the label is the suffix, not the whole name"
    );

    // The baseline is not a variant of itself.
    std::fs::create_dir_all(root.join("manga-ocr")).expect("mkdir");
    for name in ["encoder_model.onnx", "decoder_model.onnx", "vocab.txt"] {
        std::fs::write(root.join("manga-ocr").join(name), []).expect("write");
    }
    assert_eq!(
        variants(&root).len(),
        1,
        "manga-ocr itself must not be listed"
    );
}
