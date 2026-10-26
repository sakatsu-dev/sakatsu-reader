//! Score the engines, and the preprocessing, against real material.
//!
//! This is the missing piece `local documentation` names as the gap that matters most:
//! every accuracy claim in that document — that the two engines are complementary, that
//! upscaling helps, that contrast helps — is unfalsifiable without a corpus. So this test
//! does not assert an accuracy target. It **prints a table**, and the table is the point.
//!
//! ```text
//! cargo test -p sakatsu-core --test ocr_corpus -- --ignored --nocapture
//! ```
//!
//! # The corpus is never committed
//!
//! Manga panels and game screenshots belong to whoever made them, and `project notes` §8 is
//! careful about exactly this. The corpus lives outside version control: point
//! `SAKATSU_OCR_CORPUS` at a directory, or drop files in `corpus/` at the repository root,
//! which is gitignored. With no corpus this skips and says how to make one, so it is green
//! on a fresh clone and in CI.
//!
//! # What a corpus looks like
//!
//! One image and one ground-truth file per sample, sharing a stem:
//!
//! ```text
//! corpus/
//!   game-menu-01.png     game-menu-01.txt
//!   manga-vertical-3.png manga-vertical-3.txt
//! ```
//!
//! The `.txt` is what a person reads in the image, exactly. Line breaks and spaces do not
//! matter — `score::normalize` strips them, because where an engine breaks a line is a
//! property of its pipeline shape rather than of its reading.

mod common;

use std::path::{Path, PathBuf};

use sakatsu_core::ocr::score::{character_error_rate, normalize};
use sakatsu_core::ocr::{
    AutoOcr, AutoReason, MangaOcr, Ocr, OcrImage, PaddleOcr, Preprocess, PreprocessOptions, VlmOcr,
};

/// Where the models live, matching the other live tests.
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

use common::corpus_dir;

struct Sample {
    name: String,
    image: OcrImage,
    truth: String,
}

/// Load every image that has a ground-truth file beside it.
///
/// An image without its `.txt` is skipped rather than treated as expecting empty text —
/// scoring against a truth nobody wrote would manufacture a number out of an oversight.
fn load(dir: &Path) -> Vec<Sample> {
    let mut samples = Vec::new();
    for path in common::corpus_images(dir) {
        let truth_path = path.with_extension("txt");
        let Ok(truth) = std::fs::read_to_string(&truth_path) else {
            eprintln!(
                "skipping {} — no {} beside it",
                path.display(),
                truth_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            );
            continue;
        };

        match image::open(&path) {
            Ok(decoded) => {
                let rgba = decoded.to_rgba8();
                let (width, height) = (rgba.width(), rgba.height());
                match OcrImage::new(width, height, rgba.into_raw()) {
                    Ok(image) => samples.push(Sample {
                        name: path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        image,
                        truth,
                    }),
                    Err(error) => eprintln!("skipping {}: {error}", path.display()),
                }
            }
            Err(error) => eprintln!("skipping {}: {error}", path.display()),
        }
    }

    samples.sort_by(|a, b| a.name.cmp(&b.name));
    samples
}

/// Every combination worth telling apart.
///
/// All eight rather than one flag at a time, because these interact: tightening changes
/// what upscaling is spent on, and upscaling changes the pixels contrast is measured over.
/// Testing them independently would answer a question nobody asked.
fn combinations() -> Vec<(String, PreprocessOptions)> {
    let mut all = Vec::new();
    for tighten in [false, true] {
        for contrast in [false, true] {
            for upscale in [false, true] {
                let mut label = String::new();
                for (on, name) in [
                    (tighten, "tighten"),
                    (contrast, "contrast"),
                    (upscale, "upscale"),
                ] {
                    if on {
                        if !label.is_empty() {
                            label.push('+');
                        }
                        label.push_str(name);
                    }
                }
                if label.is_empty() {
                    label.push_str("none");
                }
                all.push((
                    label,
                    PreprocessOptions {
                        tighten,
                        contrast,
                        upscale,
                    },
                ));
            }
        }
    }
    all
}

/// One engine's scores, kept rather than printed and forgotten.
///
/// The table below used to print each rate and drop it. Two questions need the numbers to
/// survive the loop — what the best engine per sample would score, and what the best
/// *preprocessing* per sample would — and neither can be answered a row at a time.
struct Scored {
    /// One entry per preprocessing combination: its label, and one rate per sample.
    runs: Vec<(String, Vec<f64>)>,
}

impl Scored {
    /// The mean if the preprocessing were chosen per sample by an oracle.
    ///
    /// **A ceiling, not a proposal.** Nothing can pick the best transform for a crop it has
    /// not read yet. What this says is whether such a chooser would be worth building: a
    /// number close to the best fixed combination means the answer is no, and the transforms
    /// should stay one setting rather than becoming a policy.
    fn adaptive(&self) -> f64 {
        let samples = self.runs.first().map_or(0, |(_, rates)| rates.len());
        if samples == 0 {
            return 0.0;
        }
        let total: f64 = (0..samples)
            .map(|i| {
                self.runs
                    .iter()
                    .map(|(_, rates)| rates[i])
                    .fold(f64::INFINITY, f64::min)
            })
            .sum();
        total / samples as f64
    }

    /// The mean of the best fixed combination, which is what a setting can actually deliver.
    fn best_fixed(&self) -> Option<(&str, f64)> {
        self.runs
            .iter()
            .map(|(label, rates)| {
                let mean = rates.iter().sum::<f64>() / rates.len().max(1) as f64;
                (label.as_str(), mean)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The rates for one named combination, for comparing engines like for like.
    fn at(&self, label: &str) -> Option<&[f64]> {
        self.runs
            .iter()
            .find(|(name, _)| name == label)
            .map(|(_, rates)| rates.as_slice())
    }
}

/// What choosing the better engine per sample would score, against what each scores alone.
///
/// **Also a ceiling.** No router sees the ground truth, so this is not a result anyone can
/// ship — it is the size of the prize. If it sits near the better engine's own mean then
/// routing has little to win and the effort belongs elsewhere; if it sits far below, the gap
/// is what a better `AutoOcr` is playing for, and `auto`'s own row says how much of it the
/// current policy already takes.
fn routing_ceiling(named: &[(&str, &Scored)], at: &str) {
    let series: Vec<(&str, &[f64])> = named
        .iter()
        .filter_map(|(name, scored)| scored.at(at).map(|rates| (*name, rates)))
        .collect();
    if series.len() < 2 {
        return;
    }
    let samples = series[0].1.len();
    if samples == 0 || series.iter().any(|(_, rates)| rates.len() != samples) {
        return;
    }

    println!("\n=== choosing per sample, at preprocessing `{at}` ===");
    for (name, rates) in &series {
        let mean = rates.iter().sum::<f64>() / samples as f64;
        let exact = rates.iter().filter(|rate| **rate == 0.0).count();
        println!("{name:<28} {mean:>8.4}   {exact:>3}/{samples} exact");
    }
    let best: f64 = (0..samples)
        .map(|i| {
            series
                .iter()
                .map(|(_, rates)| rates[i])
                .fold(f64::INFINITY, f64::min)
        })
        .sum::<f64>()
        / samples as f64;
    let reachable = (0..samples)
        .filter(|i| series.iter().any(|(_, rates)| rates[*i] == 0.0))
        .count();
    println!(
        "{:<28} {best:>8.4}   {reachable:>3}/{samples} exact",
        "a perfect router"
    );
    println!("  the gap between the best engine above and this is what routing is playing for.");
}

/// Mean CER for one engine over one preprocessing combination.
fn score(engine: &dyn Ocr, samples: &[Sample], options: PreprocessOptions) -> (f64, Vec<f64>) {
    let mut rates = Vec::with_capacity(samples.len());
    for sample in samples {
        let prepared = match sakatsu_core::ocr::preprocess::prepare(&sample.image, options) {
            Ok(image) => image,
            Err(error) => {
                eprintln!("  {} failed to prepare: {error}", sample.name);
                rates.push(1.0);
                continue;
            }
        };
        let got = engine.recognize(&prepared).unwrap_or_else(|error| {
            eprintln!("  {} failed to read: {error}", sample.name);
            String::new()
        });
        rates.push(character_error_rate(
            &normalize(&sample.truth),
            &normalize(&got),
        ));
    }
    let mean = if rates.is_empty() {
        0.0
    } else {
        rates.iter().sum::<f64>() / rates.len() as f64
    };
    (mean, rates)
}

fn report(
    engine_name: &str,
    samples: &[Sample],
    build: impl Fn(PreprocessOptions) -> Box<dyn Ocr>,
) -> Scored {
    println!("\n=== {engine_name} — mean character error rate, lower is better ===");
    println!("{:<28} {:>8}   per sample", "preprocessing", "mean");

    let mut scored = Scored { runs: Vec::new() };
    let mut best: Option<(String, f64)> = None;
    for (label, options) in combinations() {
        let engine = build(options);
        let (mean, rates) = score(engine.as_ref(), samples, options);
        let detail: Vec<String> = rates.iter().map(|rate| format!("{rate:.3}")).collect();
        println!("{label:<28} {mean:>8.4}   {}", detail.join(" "));
        scored.runs.push((label.clone(), rates.clone()));
        // `match` rather than `Option::is_none_or`, which is Rust 1.82 and the workspace
        // pins 1.80.
        let improved = match &best {
            Some((_, previous)) => mean < *previous,
            None => true,
        };
        if improved {
            best = Some((label, mean));
        }
    }

    if let Some((label, mean)) = best {
        println!("  best: {label} at {mean:.4}");
    }
    // What a per-sample chooser could reach, against what a setting can actually deliver.
    // A small gap here is itself the finding: the transforms should stay a switch rather
    // than become a policy, because nothing can pick one for a crop it has not read yet.
    if let Some((fixed_label, fixed_mean)) = scored.best_fixed() {
        let adaptive = scored.adaptive();
        let cut = if fixed_mean > 0.0 {
            (1.0 - adaptive / fixed_mean) * 100.0
        } else {
            0.0
        };
        println!("  chosen per sample: {adaptive:.4} against {fixed_mean:.4} for a fixed `{fixed_label}` — a {cut:.0}% cut");
    }
    println!(
        "  samples, in order: {}",
        samples
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    scored
}

/// Which rule decided each sample, and what it cost.
///
/// The router's mean says how well it does; this says *how*. Scoring showed the losses to a
/// perfect chooser running one way — handed to manga-ocr where PP-OCRv5 had read better — and
/// there are two rules that can do that: a rejected line, and a vertical disagreement. Naming
/// the one that fired is the difference between knowing the router is wrong and knowing where.
fn report_auto_reasons(samples: &[Sample], manga_dir: &Path, paddle_dir: &Path) {
    let engine = AutoOcr::new(manga_dir.to_path_buf(), paddle_dir.to_path_buf());
    println!("\n=== auto — which rule fired, at preprocessing `none` ===");

    let mut rows: Vec<(&str, f64, &str)> = Vec::new();
    for sample in samples {
        let (text, reason) = match engine.recognize_explained(&sample.image) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("  {} failed to read: {error}", sample.name);
                continue;
            }
        };
        let rate = character_error_rate(&normalize(&sample.truth), &normalize(&text));
        rows.push((sample.name.as_str(), rate, reason.label()));
    }

    let mut labels: Vec<&str> = rows.iter().map(|(_, _, label)| *label).collect();
    labels.sort_unstable();
    labels.dedup();
    println!("{:<24} {:>5} {:>9}", "rule", "n", "mean CER");
    for label in labels {
        let hit: Vec<f64> = rows
            .iter()
            .filter(|(_, _, l)| *l == label)
            .map(|(_, rate, _)| *rate)
            .collect();
        let mean = hit.iter().sum::<f64>() / hit.len().max(1) as f64;
        println!("{label:<24} {:>5} {mean:>9.4}", hit.len());
    }

    // The samples this policy handled worst, named, so the next change has something to aim at.
    let mut worst = rows.clone();
    worst.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("  worst ten:");
    for (name, rate, label) in worst.iter().take(10) {
        println!("    {name:<30} {rate:>6.3}  {label}");
    }
}

/// Not `#[ignore]`d: this one runs everywhere and is how the corpus gets explained.
#[test]
fn the_corpus_location_is_discoverable() {
    let dir = corpus_dir();
    let samples = load(&dir);
    println!(
        "corpus: {} ({} sample{})",
        dir.display(),
        samples.len(),
        if samples.len() == 1 { "" } else { "s" }
    );
    if samples.is_empty() {
        println!(
            "no samples yet. Put screenshots and their transcriptions there as foo.png + foo.txt, \
             or set SAKATSU_OCR_CORPUS. The directory is gitignored: the material is copyrighted \
             and does not belong in the repository."
        );
    }
}

#[test]
#[ignore = "needs the OCR models and a corpus"]
fn score_the_engines_against_the_corpus() {
    let dir = corpus_dir();
    let samples = load(&dir);
    if samples.is_empty() {
        println!("SKIP: no corpus at {} — nothing to score", dir.display());
        return;
    }

    let Some(models) = model_root() else {
        println!("SKIP: no model directory (set SAKATSU_MODEL_DIR)");
        return;
    };

    println!("{} samples from {}", samples.len(), dir.display());

    let mut scored: Vec<(&str, Scored)> = Vec::new();

    let paddle_dir = models.join("ppocrv6");
    if PaddleOcr::is_present(&paddle_dir) {
        let run = report("pp-ocrv6", &samples, |options| {
            let engine = PaddleOcr::open(&paddle_dir).expect("paddle loads");
            Box::new(Preprocess::new(engine, options))
        });
        scored.push(("pp-ocrv6", run));
    } else {
        println!(
            "\npp-ocrv6 not installed at {} — skipped",
            paddle_dir.display()
        );
    }

    let manga_dir = models.join("manga-ocr");
    if MangaOcr::is_present(&manga_dir) {
        let run = report("manga-ocr", &samples, |options| {
            let engine = MangaOcr::load(&manga_dir).expect("manga-ocr loads");
            Box::new(Preprocess::new(engine, options))
        });
        scored.push(("manga-ocr", run));
    } else {
        println!(
            "\nmanga-ocr not installed at {} — skipped",
            manga_dir.display()
        );
    }

    // **The quantised export, scored as its own engine.**
    //
    // `quantisation.rs` could only ask whether a variant reads what fp32 reads, because there
    // was no truth to ask anything else against. With truth, the real question is answerable:
    // uint8 is a quarter of the size and 27% faster, and the only thing that ever stood in the
    // way of shipping it was not knowing whether its 14 divergences were worse or merely
    // different. If this row matches manga-ocr's, they were merely different.
    let uint8_dir = models.join("manga-ocr-uint8");
    if MangaOcr::is_present(&uint8_dir) {
        let run = report("manga-ocr-uint8", &samples, |options| {
            let engine = MangaOcr::load(&uint8_dir).expect("manga-ocr uint8 loads");
            Box::new(Preprocess::new(engine, options))
        });
        scored.push(("manga-ocr-uint8", run));
    } else {
        println!(
            "\nmanga-ocr uint8 not installed at {} — skipped",
            uint8_dir.display()
        );
    }

    // **The router, scored as an engine, which is the row that was missing.**
    //
    // The two rows above say how each engine reads. Neither says how the app reads, because
    // the app ships `AutoOcr` — and until now the one component with the most headroom was
    // the only one never measured. It is the same `Ocr` trait, so it costs a `report` call.
    //
    // Slower than either alone, deliberately: it runs PP-OCRv6 and then manga-ocr whenever the
    // first read looks vertical, empty or partly rejected, so a sweep of eight combinations is
    // the most expensive block in this file.
    if PaddleOcr::is_present(&paddle_dir) && MangaOcr::is_present(&manga_dir) {
        let run = report("auto", &samples, |options| {
            let engine = AutoOcr::new(manga_dir.clone(), paddle_dir.clone());
            Box::new(Preprocess::new(engine, options))
        });
        scored.push(("auto", run));
    }

    // What choosing per sample would be worth, against what the router actually gets. Reported
    // at `none` so the comparison is about engines rather than about transforms.
    let borrowed: Vec<(&str, &Scored)> = scored.iter().map(|(name, run)| (*name, run)).collect();
    routing_ceiling(&borrowed, "none");

    if PaddleOcr::is_present(&paddle_dir) && MangaOcr::is_present(&manga_dir) {
        report_auto_reasons(&samples, &manga_dir, &paddle_dir);
    }

    report_vlm(&samples);

    // Deliberately no assertion. There is no accuracy number this project has earned the
    // right to require yet, and a threshold invented here would be a number to satisfy
    // rather than a fact to learn from. Read the table.
}

/// Resolve a preprocessing label to the options behind it.
///
/// Looked up in [`combinations`] rather than parsed a second time, so the label the table
/// prints and the options it names cannot drift apart — the only way to add a combination is
/// to add it there, and this follows.
fn preprocess_named(label: &str) -> Result<PreprocessOptions, String> {
    combinations()
        .into_iter()
        .find(|(name, _)| name == label)
        .map(|(_, options)| options)
        .ok_or_else(|| {
            let valid: Vec<String> = combinations().into_iter().map(|(name, _)| name).collect();
            format!(
                "unknown preprocessing {label:?}; valid: {}",
                valid.join(", ")
            )
        })
}

/// Score the VLM, at **one** preprocessing combination.
///
/// Opt-in and off by default, using the variables `vlm_live.rs` already established so there
/// is one convention rather than two:
///
/// | variable | meaning |
/// |---|---|
/// | `SAKATSU_VLM_ENDPOINT` | unset ⇒ skipped entirely, and the run is exactly as before |
/// | `SAKATSU_VLM_MODEL` | passed through; llama-server ignores it, Ollama needs it |
/// | `SAKATSU_VLM_LABEL` | what this row is called |
/// | `SAKATSU_VLM_PREPROCESS` | which combination, default `none` |
///
/// **One combination, not eight.** This is a ~1B autoregressive model at seconds to tens of
/// seconds per crop against PP-OCRv6's 0.05–0.19 s; the full sweep is hours per model, and
/// the question worth answering — does a manga fine-tune beat manga-ocr on manga — needs one
/// row, not eight. The other engines keep their sweep because theirs is nearly free.
///
/// The label matters more than it looks: the harness reaches one endpoint at a time, so
/// comparing the general model against the manga fine-tune means two runs against two
/// servers. Without a name on the row, the two outputs are indistinguishable afterwards.
fn report_vlm(samples: &[Sample]) {
    let Some(endpoint) = std::env::var("SAKATSU_VLM_ENDPOINT")
        .ok()
        .filter(|url| !url.trim().is_empty())
    else {
        println!("\nVLM not scored — set SAKATSU_VLM_ENDPOINT to include it");
        return;
    };
    let model = std::env::var("SAKATSU_VLM_MODEL").unwrap_or_default();
    let label = std::env::var("SAKATSU_VLM_LABEL")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| (!model.trim().is_empty()).then(|| model.clone()))
        .unwrap_or_else(|| "vlm".to_string());

    let chosen = std::env::var("SAKATSU_VLM_PREPROCESS").unwrap_or_else(|_| "none".to_string());
    let options = match preprocess_named(chosen.trim()) {
        Ok(options) => options,
        Err(message) => {
            println!("\nVLM not scored — {message}");
            return;
        }
    };

    let engine = match VlmOcr::new(&endpoint, &model) {
        Ok(engine) => engine,
        Err(error) => {
            println!("\nVLM not scored — invalid endpoint {endpoint:?}: {error}");
            return;
        }
    };
    // Fail fast. `vlm::TIMEOUT` is 180 s, so a wrong endpoint otherwise costs three minutes
    // per sample before anything is reported.
    if !engine.is_reachable() {
        println!("\nVLM at {endpoint} is not reachable — skipped (is the server running?)");
        return;
    }

    println!("\n=== {label} — mean character error rate, lower is better ===");
    println!("preprocessing: {chosen}   endpoint: {endpoint}");

    let started = std::time::Instant::now();
    let (mean, rates) = score(&engine, samples, options);
    let elapsed = started.elapsed().as_secs_f64();

    let detail: Vec<String> = rates.iter().map(|rate| format!("{rate:.3}")).collect();
    println!("{chosen:<28} {mean:>8.4}   {}", detail.join(" "));
    // Cost is half the finding here: whether this engine is worth using at all turns on
    // seconds per capture, not only on error rate.
    println!(
        "  {elapsed:.1} s total, {:.1} s per sample over {} sample(s)",
        if samples.is_empty() {
            0.0
        } else {
            elapsed / samples.len() as f64
        },
        samples.len()
    );
    println!(
        "  samples, in order: {}",
        samples
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
}

/// Scored on its own rather than inside [`score_the_engines_against_the_corpus`], because it asks
/// a different question and paying for that table first put it out of reach: that test runs every
/// engine against all eight preprocessing combinations, so adjudication — which needs one pass —
/// sat behind roughly twenty-four. Split out, it is a single run and can be iterated on.
///
/// See [`report_adjudication`] for what the table means.
#[test]
#[ignore = "needs both OCR models and a corpus"]
fn adjudicating_between_the_engines() {
    let dir = corpus_dir();
    let samples = load(&dir);
    if samples.is_empty() {
        println!("SKIP: no corpus at {} — nothing to score", dir.display());
        return;
    }

    let Some(models) = model_root() else {
        println!("SKIP: no model directory (set SAKATSU_MODEL_DIR)");
        return;
    };

    let paddle_dir = models.join("ppocrv6");
    let manga_dir = models.join("manga-ocr");
    if !PaddleOcr::is_present(&paddle_dir) || !MangaOcr::is_present(&manga_dir) {
        println!("SKIP: adjudication needs both PP-OCRv6 and manga-ocr present");
        return;
    }

    println!("{} samples from {}", samples.len(), dir.display());
    report_adjudication(&samples, &manga_dir, &paddle_dir);
}

/// **Could a third opinion break the router's ties better than the router does?**
///
/// `AutoReason`'s own comment records the fault this is aimed at: scoring the router showed it
/// losing to a perfect chooser on 13 of 131 samples, **18 of 19 of them in the same direction** —
/// handed to manga-ocr where PP-OCRv5 had read better. That is a tie-breaking problem, and a
/// tie-breaking problem has a cheap fix and an expensive one. This measures both before either is
/// built.
///
/// # Adjudicating is not authoring
///
/// The row that matters is `vlm-adjudicated`: for each sample, keep whichever of the two
/// conventional reads sits closer to what the VLM said. **The text returned is always a CTC
/// engine's**, so a generative model that fabricates can pick wrongly but cannot put invented text
/// on screen. That is what makes this worth scoring while `OcrChoice::Vlm` is still "selectable,
/// never automatic" — the objection recorded there is about fabrication reaching a reader, and
/// adjudication does not let it.
///
/// # The controls are the point
///
/// A VLM row on its own would say nothing about whether it is *worth* a 1.8 GB dependency and a
/// running server. So the table also carries the two free policies — always keep PP-OCRv6, always
/// take manga-ocr — and the oracle above them all. If `always-paddle` already beats `auto` on this
/// subset then the router is simply miscalibrated, and that fix costs nothing.
fn report_adjudication(samples: &[Sample], manga_dir: &Path, paddle_dir: &Path) {
    let paddle = match PaddleOcr::open(paddle_dir) {
        Ok(engine) => engine,
        Err(error) => {
            println!("\nadjudication not scored — PP-OCRv6 failed to load: {error}");
            return;
        }
    };
    let manga = match MangaOcr::load(manga_dir) {
        Ok(engine) => engine,
        Err(error) => {
            println!("\nadjudication not scored — manga-ocr failed to load: {error}");
            return;
        }
    };
    let auto = AutoOcr::new(manga_dir.to_path_buf(), paddle_dir.to_path_buf());
    let judge = vlm_from_env();

    struct Row {
        truth: String,
        paddle: String,
        manga: String,
        auto: String,
        reason: AutoReason,
        judge: Option<String>,
    }

    let mut rows: Vec<Row> = Vec::new();
    for sample in samples {
        let (Ok(paddle_text), Ok(manga_text)) = (
            paddle.recognize(&sample.image),
            manga.recognize(&sample.image),
        ) else {
            eprintln!("  {} — an engine failed, skipped", sample.name);
            continue;
        };
        let Ok((auto_text, reason)) = auto.recognize_explained(&sample.image) else {
            eprintln!("  {} — auto failed, skipped", sample.name);
            continue;
        };
        rows.push(Row {
            truth: sample.truth.clone(),
            paddle: paddle_text,
            manga: manga_text,
            auto: auto_text,
            reason,
            judge: judge
                .as_ref()
                .and_then(|engine| engine.recognize(&sample.image).ok()),
        });
    }

    // **Only where a tie was actually broken.** `ConfidentHorizontal` means manga-ocr was never
    // loaded, so there were never two candidates and no policy over them could have changed
    // anything. Averaging those in would move every row by the same amount and hide the effect.
    let contested: Vec<&Row> = rows
        .iter()
        .filter(|row| row.reason != AutoReason::ConfidentHorizontal)
        .collect();

    println!("\n=== breaking the router's ties ===");
    println!(
        "  {} of {} samples had two candidates (the rest never loaded manga-ocr)",
        contested.len(),
        rows.len()
    );
    if contested.is_empty() {
        return;
    }

    let cer = |truth: &str, got: &str| character_error_rate(&normalize(truth), &normalize(got));
    let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len().max(1) as f64;

    let auto_rates: Vec<f64> = contested.iter().map(|r| cer(&r.truth, &r.auto)).collect();
    let paddle_rates: Vec<f64> = contested.iter().map(|r| cer(&r.truth, &r.paddle)).collect();
    let manga_rates: Vec<f64> = contested.iter().map(|r| cer(&r.truth, &r.manga)).collect();
    let oracle_rates: Vec<f64> = contested
        .iter()
        .map(|r| cer(&r.truth, &r.paddle).min(cer(&r.truth, &r.manga)))
        .collect();

    let n = contested.len();
    println!("\n  policy                       n   mean CER   (contested subset only)");
    println!(
        "  {:<26} {:>3} {:>10.4}",
        "always-paddle",
        n,
        mean(&paddle_rates)
    );
    println!(
        "  {:<26} {:>3} {:>10.4}",
        "always-manga",
        n,
        mean(&manga_rates)
    );
    println!(
        "  {:<26} {:>3} {:>10.4}",
        "auto (what ships)",
        n,
        mean(&auto_rates)
    );

    match &judge {
        None => println!(
            "  {:<26}  —  set SAKATSU_VLM_ENDPOINT to include it",
            "vlm-adjudicated"
        ),
        Some(_) => {
            let judged: Vec<&&Row> = contested.iter().filter(|r| r.judge.is_some()).collect();
            if judged.is_empty() {
                println!(
                    "  {:<26}  —  the judge answered on none of them",
                    "vlm-adjudicated"
                );
            } else {
                let mut adjudicated = Vec::new();
                let mut picked_shorter = 0usize;
                let mut oracle_shorter = 0usize;
                let mut changed = 0usize;
                for row in &judged {
                    let verdict = row.judge.as_deref().unwrap_or_default();
                    // Closer to the judge wins. A tie keeps PP-OCRv6, which is what `auto::pick`
                    // prefers when the two agree, because it preserves the detected line breaks.
                    let take_manga = cer(verdict, &row.manga) < cer(verdict, &row.paddle);
                    let chosen = if take_manga { &row.manga } else { &row.paddle };
                    adjudicated.push(cer(&row.truth, chosen));

                    // **The bias this policy could carry.** If the judge systematically reads
                    // *less* than the engines — the role-label omissions `corpus/RESULTS.md`
                    // records — then "closest to the judge" quietly means "shortest", and the
                    // policy would be choosing by length rather than by correctness. The oracle's
                    // own rate of taking the shorter read is the control: the gap is the bias.
                    let shorter_is_manga = row.manga.chars().count() < row.paddle.chars().count();
                    if take_manga == shorter_is_manga {
                        picked_shorter += 1;
                    }
                    if (cer(&row.truth, &row.manga) < cer(&row.truth, &row.paddle))
                        == shorter_is_manga
                    {
                        oracle_shorter += 1;
                    }
                    if normalize(chosen) != normalize(&row.auto) {
                        changed += 1;
                    }
                }
                println!(
                    "  {:<26} {:>3} {:>10.4}",
                    "vlm-adjudicated",
                    judged.len(),
                    mean(&adjudicated)
                );

                // **The control the adjudicator has to beat.** Adjudication consults the judge on
                // every contested capture, which was 115 of 141 here — so it does not buy a
                // cheaper pipeline than simply reading the VLM's own text would. It buys one
                // thing: what reaches the reader is never generated. Scoring the judge's raw
                // output over the same samples is what prices that safety, and it is the number
                // that decides whether adjudication belongs in `AutoOcr` or stays a lever.
                //
                // Not a shippable policy. It is here to be compared against, and `OcrChoice::Vlm`
                // remains selectable-never-automatic for the fabrication reason recorded there.
                let direct: Vec<f64> = judged
                    .iter()
                    .map(|row| cer(&row.truth, row.judge.as_deref().unwrap_or_default()))
                    .collect();
                println!(
                    "  {:<26} {:>3} {:>10.4}   the judge's own text — not shippable, a control",
                    "vlm-direct",
                    judged.len(),
                    mean(&direct)
                );
                println!(
                    "\n  the judge changed the answer on {changed} of {} contested samples",
                    judged.len()
                );
                let share = |count: usize| 100.0 * count as f64 / judged.len().max(1) as f64;
                println!(
                    "  took the shorter read: adjudicator {:.0}%, oracle {:.0}%   {}",
                    share(picked_shorter),
                    share(oracle_shorter),
                    if picked_shorter > oracle_shorter + judged.len() / 10 {
                        "<- leaning short, check the omission bias"
                    } else {
                        "(no length bias worth acting on)"
                    }
                );
            }
        }
    }

    println!(
        "  {:<26} {:>3} {:>10.4}   the ceiling for any policy over these two",
        "a perfect chooser",
        n,
        mean(&oracle_rates)
    );

    // Which way the router errs, named. The recorded fault was one-sided, and a fix aimed at the
    // wrong side would move the mean without touching the cause.
    let mut took_manga_wrongly = 0usize;
    let mut kept_paddle_wrongly = 0usize;
    for row in &contested {
        let paddle_rate = cer(&row.truth, &row.paddle);
        let manga_rate = cer(&row.truth, &row.manga);
        if normalize(&row.auto) == normalize(&row.manga) {
            if paddle_rate < manga_rate {
                took_manga_wrongly += 1;
            }
        } else if manga_rate < paddle_rate {
            kept_paddle_wrongly += 1;
        }
    }
    println!(
        "\n  auto's mistakes: took manga-ocr where PP-OCRv6 read better {took_manga_wrongly}, kept PP-OCRv6 where manga-ocr read better {kept_paddle_wrongly}"
    );

    // **Which rule sent it there.** Knowing the fault is one-sided is not enough to fix it —
    // `AutoReason`'s own comment says as much. Two separate tests in `auto::pick` hand a read to
    // manga-ocr and they fail differently: `paddle-incomplete` fires when PP-OCRv6 rejected a
    // line it had detected, `vertical-disagreement` when the two simply read a vertical block
    // differently. Grouping by the rule says which of them is doing the damage.
    //
    // Ordered by **regret**, not by count. Regret is what `auto` gave up against the better of
    // the two candidates, so a rule that fires often and costs little sorts below one that fires
    // rarely and costs a lot — which is the order a fix should be attempted in. A count alone
    // would rank the rules by how loud they are rather than by how wrong.
    let mut groups: Vec<(AutoReason, Vec<&Row>)> = Vec::new();
    for row in contested.iter().copied() {
        match groups.iter_mut().find(|(reason, _)| *reason == row.reason) {
            Some((_, members)) => members.push(row),
            None => groups.push((row.reason, vec![row])),
        }
    }

    let regret = |row: &Row| {
        cer(&row.truth, &row.auto) - cer(&row.truth, &row.paddle).min(cer(&row.truth, &row.manga))
    };
    let total_regret = |members: &[&Row]| members.iter().map(|row| regret(row)).sum::<f64>();
    groups.sort_by(|a, b| {
        total_regret(&b.1)
            .partial_cmp(&total_regret(&a.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!(
        "
  rule that fired            n   paddle    manga     auto   misrouted   regret"
    );
    for (reason, members) in &groups {
        let column = |pick: fn(&Row) -> &String| {
            mean(
                &members
                    .iter()
                    .map(|row| cer(&row.truth, pick(row)))
                    .collect::<Vec<_>>(),
            )
        };
        println!(
            "  {:<22} {:>4} {:>8.4} {:>8.4} {:>8.4} {:>11} {:>8.4}",
            reason.label(),
            members.len(),
            column(|row| &row.paddle),
            column(|row| &row.manga),
            column(|row| &row.auto),
            members.iter().filter(|row| regret(row) > 0.0).count(),
            total_regret(members),
        );
    }

    // **Is length the tell?** manga-ocr is built to read one block — a speech bubble. Handed a
    // capture holding several vertical columns it reads one of them and stops, which shows up as
    // a read much shorter than PP-OCRv6's. That is a hypothesis with a free test: if the samples
    // the router misroutes are the ones where manga-ocr returned far less text than PP-OCRv6 did,
    // then a length ratio separates them and no third engine is needed to break these ties.
    //
    // Printed against the samples the same rule got *right*, because a ratio on its own is not
    // evidence. The two numbers being close would say length carries no signal here and would
    // send the fix back to adjudication.
    println!(
        "
  within each rule — does manga-ocr's read run short when it is the wrong pick?"
    );
    for (reason, members) in &groups {
        let ratio = |rows: &[&Row]| {
            let values: Vec<f64> = rows
                .iter()
                .filter(|row| !row.paddle.trim().is_empty())
                .map(|row| row.manga.chars().count() as f64 / row.paddle.chars().count() as f64)
                .collect();
            if values.is_empty() {
                f64::NAN
            } else {
                mean(&values)
            }
        };
        let (misrouted, correct): (Vec<&Row>, Vec<&Row>) =
            members.iter().partition(|row| regret(row) > 0.0);
        if misrouted.is_empty() {
            continue;
        }
        println!(
            "  {:<22} misrouted n={:<3} ratio {:.2}   |   correct n={:<3} ratio {:.2}",
            reason.label(),
            misrouted.len(),
            ratio(&misrouted),
            correct.len(),
            ratio(&correct),
        );
    }
    println!("  ratio is manga-ocr's character count over PP-OCRv6's; below 1 means it read less.");
}

/// The VLM from the environment, or `None`. Reads the same variables as [`report_vlm`], so one
/// server serves both, and stays quiet because that function already explains them.
fn vlm_from_env() -> Option<VlmOcr> {
    let endpoint = std::env::var("SAKATSU_VLM_ENDPOINT")
        .ok()
        .filter(|url| !url.trim().is_empty())?;
    let model = std::env::var("SAKATSU_VLM_MODEL").unwrap_or_default();
    let engine = VlmOcr::new(&endpoint, &model).ok()?;
    engine.is_reachable().then_some(engine)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every label the table can print resolves back to the options it was built from.
    #[test]
    fn a_preprocessing_label_round_trips_through_its_own_table() {
        for (label, options) in combinations() {
            assert_eq!(
                preprocess_named(&label).expect("its own label resolves"),
                options,
                "{label} did not resolve to the options that produced it"
            );
        }
        assert_eq!(
            preprocess_named("none").expect("the default"),
            PreprocessOptions {
                tighten: false,
                contrast: false,
                upscale: false
            }
        );
    }

    /// A typo must say so and list the alternatives, not silently score `none` and produce a
    /// row labelled with something that never ran.
    #[test]
    fn an_unknown_preprocessing_label_is_refused_and_explains_itself() {
        let error = preprocess_named("upscale+contrast").expect_err("wrong order is not a label");
        assert!(error.contains("unknown preprocessing"));
        assert!(
            error.contains("contrast+upscale"),
            "the valid labels are listed: {error}"
        );
    }
}
