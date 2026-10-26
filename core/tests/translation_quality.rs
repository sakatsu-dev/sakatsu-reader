//! Does a modern instruct model actually translate better than FuguMT or CAT-Translate?
//!
//! ```text
//! # generate once, cache, then score
//! SAKATSU_LLM_URL=http://127.0.0.1:11435 \
//!   cargo test --release -p sakatsu-core --test translation_quality -- --ignored --nocapture
//! ```
//!
//! # Why this exists
//!
//! `instruct_models.rs` could not answer it. Every candidate landed in a 0.50–0.52 band on
//! content-word overlap, and a direct check showed why: **two different models translating the
//! same sentence agree with each other at only 0.67** while each agrees with the human at ~0.58.
//! Correct-but-worded-differently and wrong look nearly the same to that metric, so a two-point
//! gap between models was inside a nine-point noise floor.
//!
//! # A metric is not trusted here until it has proved it can resolve
//!
//! That is the whole lesson of the previous run, so every metric below is put through the same
//! three-point check before any model is ranked with it:
//!
//! | probe | what it must do |
//! |---|---|
//! | the reference against itself | score at the top of the scale |
//! | one good translation against another | score **high**, because both are right |
//! | the reference against an unrelated sentence | score near the bottom |
//!
//! The middle one is the discriminating test and the one content-word overlap failed. A metric
//! that cannot separate *different wording* from *different meaning* cannot rank translations,
//! and this file discards it rather than reporting a table built on it.
//!
//! # Three metrics, none of which needs a download
//!
//! - **chrF2** — character n-gram F-score, the standard companion to BLEU. Surface-level, but
//!   character n-grams forgive morphology where whole words do not.
//! - **BLEU** — corpus-level, 4-gram, with a brevity penalty. Included because CAT-Translate
//!   publishes **33.26** for JA→EN, so it is the one number here that can be checked against a
//!   claim made elsewhere.
//! - **embedding cosine** — `qwen3-embedding:0.6b`, already installed. Semantic rather than
//!   surface, which is the property BLEU and chrF lack and COMET is usually reached for to get.
//!   A local stand-in for COMET that costs nothing to obtain.
//!
//! COMET or BLEURT proper would be better and both need a multi-gigabyte download plus a Python
//! stack; `project notes` §8 says nothing large arrives without being asked, and that includes a
//! maintainer's measurement.
//!
//! # Generated once, cached, scored many times
//!
//! Inference is the expensive part and the metrics are cheap, so translations land in
//! `target/translations.tsv` and the scoring reads them back. Adding a metric later costs
//! seconds rather than another sweep.

mod common;
#[path = "common/sentences.rs"]
mod sentences;

use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;

use sakatsu_core::translate::{FuguMt, Translator};

use common::{chrf2, cosine, embed, endpoint, Bleu, EMBEDDER, TIMEOUT};
use sentences::SENTENCES;

/// Who is being compared, and how each has to be spoken to.
///
/// `narrow` marks a translation model rather than an instruction-following one — measured, not
/// assumed: CAT-Translate given "Reply with the translation only" translated the Japanese and
/// then echoed that sentence back as part of its answer.
const CANDIDATES: &[(&str, &str, bool)] = &[
    (
        "hf.co/mmnga-o/CAT-Translate-1.4b-gguf:Q4_K_M",
        "CAT-Translate-1.4b",
        true,
    ),
    ("gemma3:4b", "gemma3:4b", false),
    ("qwen3:4b-instruct", "qwen3:4b-instruct", false),
    ("qwen2.5:7b", "qwen2.5:7b", false),
    ("elyza:8b", "elyza:8b", false),
];

// ── Getting the translations ────────────────────────────────────────────────────────────────

fn ask(model: &str, prompt: &str) -> Option<String> {
    let url = format!("{}/v1/chat/completions", endpoint());
    let body = serde_json::json!({
        "model": model,
        "temperature": 0,
        "messages": [{ "role": "user", "content": prompt }],
    });
    let mut response = ureq::post(&url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .send_json(&body)
        .ok()?;
    let parsed: serde_json::Value =
        serde_json::from_str(&response.body_mut().read_to_string().ok()?).ok()?;
    Some(
        parsed["choices"][0]["message"]["content"]
            .as_str()?
            .to_owned(),
    )
}

/// Strip the wrapping a model puts round its answer, so the metric scores the translation.
fn tidy(raw: &str) -> String {
    let mut text = raw.to_string();
    if let (Some(open), Some(close)) = (text.find("<think>"), text.find("</think>")) {
        if close > open {
            text = format!("{}{}", &text[..open], &text[close + "</think>".len()..]);
        }
    }
    for marker in ["</s>", "<|endoftext|>", "<|im_end|>", "<end_of_turn>"] {
        text = text.replace(marker, " ");
    }
    // **A preamble on its own line is dropped whole.** The first version matched on `here's`
    // with a straight apostrophe; gemma3 writes `Here\u{2019}s` with a curly one, so the preamble
    // survived, became the first non-empty line, and was scored as that model's entire translation
    // for one sentence. Matching on the *shape* — a short first line ending in a colon with more
    // text after it — needs no list of openings and no guess about punctuation.
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() > 1 && lines[0].ends_with(':') && lines[0].chars().count() < 40 {
        text = lines[1..].join(" ");
    } else if let Some(first) = lines.first() {
        let opener = first
            .to_ascii_lowercase()
            .replace(char::from_u32(0x2019).unwrap(), "'");
        if [
            "here is",
            "here's",
            "sure",
            "translation:",
            "the translation",
        ]
        .iter()
        .any(|p| opener.starts_with(p))
        {
            if let Some((_, rest)) = text.split_once(':') {
                text = rest.to_string();
            }
        }
    }
    // Only the first non-empty line: several models offer a second, "more natural" rendering,
    // and scoring both would reward whichever model padded most.
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .trim_matches('"')
        .trim()
        .to_string()
}

fn cache_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/translations.tsv")
}

/// Every candidate's rendering of every sentence, generated once and kept.
fn translations(corpus: &[(String, String)]) -> HashMap<String, Vec<String>> {
    let path = cache_path();
    let mut have: HashMap<String, Vec<String>> = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(&path) {
        for line in text.lines() {
            let mut parts = line.split('\t');
            if let (Some(model), Some(said)) = (parts.next(), parts.next()) {
                have.entry(model.to_string())
                    .or_default()
                    .push(said.to_string());
            }
        }
        if have.values().all(|rows| rows.len() == corpus.len()) && !have.is_empty() {
            println!("  (reusing {} cached rows)", have.len() * corpus.len());
            return have;
        }
        have.clear();
    }

    let mut file = std::fs::File::create(&path).expect("write the cache");

    // FuguMT first, in-process.
    let dir = std::env::var_os("SAKATSU_MT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA"))
                .join("com.sakatsu.app")
                .join("models")
                .join("fugumt-ja-en")
        });
    if FuguMt::is_present(&dir) {
        let engine = FuguMt::load(&dir).expect("load");
        let started = Instant::now();
        let rows: Vec<String> = corpus
            .iter()
            .map(|(japanese, _)| {
                engine
                    .translate(japanese)
                    .unwrap_or_default()
                    .replace('\t', " ")
            })
            .collect();
        println!(
            "  FuguMT                  {:>6.2}s/sentence",
            started.elapsed().as_secs_f64() / corpus.len() as f64
        );
        for row in &rows {
            let _ = writeln!(file, "FuguMT\t{row}");
        }
        have.insert("FuguMT".into(), rows);
    }

    for (model, label, narrow) in CANDIDATES {
        let started = Instant::now();
        let mut rows = Vec::new();
        for (japanese, _) in corpus {
            let prompt = if *narrow {
                format!("Translate the following Japanese text into English.\n\n{japanese}")
            } else {
                format!("Translate the following Japanese text into English. Reply with the translation only.\n\n{japanese}")
            };
            rows.push(match ask(model, &prompt) {
                Some(raw) => tidy(&raw).replace('\t', " "),
                None => String::new(),
            });
        }
        let empty = rows.iter().filter(|r| r.is_empty()).count();
        println!(
            "  {label:<24}{:>6.2}s/sentence{}",
            started.elapsed().as_secs_f64() / corpus.len() as f64,
            if empty > 0 {
                format!("   {empty} failed")
            } else {
                String::new()
            }
        );
        for row in &rows {
            let _ = writeln!(file, "{label}\t{row}");
        }
        have.insert((*label).to_string(), rows);
    }
    have
}

/// Two-sided sign test: could this many wins out of this many decided sentences be a coin?
///
/// Exact binomial rather than a normal approximation — n is 50 at most and the tails are where the
/// answer lives.
fn sign_test(wins: usize, losses: usize) -> String {
    let n = wins + losses;
    if n == 0 {
        return "n/a".into();
    }
    let extreme = wins.max(losses);
    let tail: f64 = (extreme..=n).map(|k| binomial(n, k)).sum();
    let p = (2.0 * tail).min(1.0);
    if p < 0.001 {
        "<0.001".into()
    } else {
        format!("{p:.3}")
    }
}

/// C(n, k) / 2^n, in logs so 50-choose-25 does not overflow on the way.
fn binomial(n: usize, k: usize) -> f64 {
    let mut log = -(n as f64) * 2.0f64.ln();
    for i in 0..k {
        log += ((n - i) as f64).ln() - ((i + 1) as f64).ln();
    }
    log.exp()
}

// ── The run ─────────────────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "needs the translation model and a local model server; use --release"]
fn a_modern_instruct_model_is_compared_against_fugumt_and_cat_translate() {
    // **How many sentences, and why it is a dial.** Fifty was enough to build the instrument and
    // far too few to use it: at fifty, a model winning 25 sentences and losing 18 lands at
    // p = 0.36, which is a coin. Detecting the ~0.03 gaps these metrics report needs a few
    // hundred pairs, so the pooled Jitendex corpus is available by name.
    let corpus: Vec<(String, String)> = match std::env::var("SAKATSU_QUALITY_POOLED")
        .ok()
        .and_then(|n| n.parse::<usize>().ok())
    {
        Some(want) => common::pooled_corpus(want),
        None => SENTENCES
            .iter()
            .map(|s| (s.text.to_string(), s.english.to_string()))
            .collect(),
    };

    println!(
        "\n=== generating, over {} native sentences ===",
        corpus.len()
    );
    let said = translations(&corpus);
    if said.is_empty() {
        println!(
            "SKIP: nothing answered — is a server running at {}?",
            endpoint()
        );
        return;
    }

    let humans: Vec<String> = corpus.iter().map(|(_, e)| e.clone()).collect();

    // Embeddings for everything at once: the human references, then each candidate's rows.
    let mut vectors: HashMap<String, Vec<Vec<f32>>> = HashMap::new();
    let embedding_works = match embed(&humans) {
        Some(rows) if !rows.is_empty() && !rows[0].is_empty() => {
            vectors.insert("__human__".into(), rows);
            for (label, rows) in &said {
                match embed(rows) {
                    Some(v) => {
                        vectors.insert(label.clone(), v);
                    }
                    // Loudly. A silently missing column reads as a metric that does not apply.
                    None => println!("  !! no embeddings for {label} — its cosine will be blank"),
                }
            }
            true
        }
        _ => {
            println!("\n  (no embeddings — {EMBEDDER} did not answer; that column is omitted)");
            false
        }
    };

    // ── Can these metrics resolve anything? ─────────────────────────────────────────────────
    //
    // The check the last run learned to do. A metric that scores two correct translations no
    // better than it scores an unrelated sentence cannot rank models, whatever table it fills in.
    println!("\n=== the metrics, checked before anything is ranked ===");
    let mut names: Vec<&String> = said.keys().collect();
    names.sort();
    let (a, b) = (names[0].clone(), names[names.len() - 1].clone());

    let mut probes: Vec<(&str, f64, f64)> = Vec::new();
    {
        // Reference against itself, and against a sentence with nothing to do with it.
        let stranger: Vec<String> = humans.iter().rev().cloned().collect();
        let self_chrf = humans.iter().map(|h| chrf2(h, h)).sum::<f64>() / humans.len() as f64;
        let far_chrf = humans
            .iter()
            .zip(&stranger)
            .map(|(h, s)| chrf2(h, s))
            .sum::<f64>()
            / humans.len() as f64;
        let pair_chrf = said[&a]
            .iter()
            .zip(&said[&b])
            .map(|(x, y)| chrf2(x, y))
            .sum::<f64>()
            / humans.len() as f64;
        probes.push(("chrF2", pair_chrf, far_chrf));
        println!(
            "  chrF2      itself {self_chrf:.2}   two good translations {pair_chrf:.2}   unrelated {far_chrf:.2}"
        );

        if embedding_works {
            let human_vectors = &vectors["__human__"];
            let self_cos = 1.0;
            let far_cos = human_vectors
                .iter()
                .zip(human_vectors.iter().rev())
                .map(|(x, y)| cosine(x, y))
                .sum::<f64>()
                / humans.len() as f64;
            let pair_cos = match (vectors.get(&a), vectors.get(&b)) {
                (Some(x), Some(y)) => {
                    x.iter().zip(y).map(|(p, q)| cosine(p, q)).sum::<f64>() / humans.len() as f64
                }
                _ => 0.0,
            };
            probes.push(("cosine", pair_cos, far_cos));
            println!(
                "  cosine     itself {self_cos:.2}   two good translations {pair_cos:.2}   unrelated {far_cos:.2}"
            );
        }
    }
    println!("  ({a} and {b} stand in for \"two good translations\")");
    for (metric, good, unrelated) in &probes {
        let spread = good - unrelated;
        println!(
            "  {metric:<10} separation between correct-but-different and unrelated: {spread:+.2}{}",
            if spread < 0.15 {
                "   ← too small to rank with"
            } else {
                ""
            }
        );
    }

    // ── The comparison ──────────────────────────────────────────────────────────────────────
    println!(
        "\n=== against the human translations, {} sentences ===",
        corpus.len()
    );
    println!(
        "  {:<24}{:>9}{:>8}{:>10}",
        "model", "chrF2", "BLEU", "cosine"
    );
    let mut table: Vec<(String, f64, f64, f64)> = Vec::new();
    for label in &names {
        let rows = &said[*label];
        let chrf = rows
            .iter()
            .zip(&humans)
            .map(|(c, h)| chrf2(h, c))
            .sum::<f64>()
            / humans.len() as f64;
        let mut bleu = Bleu::default();
        for (c, h) in rows.iter().zip(&humans) {
            bleu.add(h, c);
        }
        let cos = match (vectors.get(*label), vectors.get("__human__")) {
            (Some(v), Some(h)) => {
                v.iter().zip(h).map(|(x, y)| cosine(x, y)).sum::<f64>() / humans.len() as f64
            }
            _ => f64::NAN,
        };
        println!(
            "  {:<24}{:>9.3}{:>8.1}{:>10.3}",
            label,
            chrf,
            bleu.score(),
            cos
        );
        table.push(((*label).clone(), chrf, bleu.score(), cos));
    }
    // **Paired, because the corpus is the same for everyone.** Comparing two means over 50
    // sentences throws the pairing away and asks a much weaker question; asking "on how many
    // sentences did this beat FuguMT" uses it. The sign test is the honest companion: with 50
    // coin flips, 32 heads is not yet a result.
    if let (Some(base), Some(base_vectors)) = (said.get("FuguMT"), vectors.get("FuguMT")) {
        let human_vectors = vectors.get("__human__");
        println!("\n=== sentence by sentence against FuguMT ===");
        println!(
            "  {:<24}{:>7}{:>7}{:>8}{:>10}",
            "model", "wins", "ties", "losses", "sign p"
        );
        for label in &names {
            if *label == "FuguMT" {
                continue;
            }
            let rows = &said[*label];
            let (mut win, mut tie, mut loss) = (0usize, 0usize, 0usize);
            for i in 0..humans.len() {
                // Judged on cosine: it separates correct-but-different from wrong by +0.59 where
                // chrF2 manages +0.40, so it is the sharper of the two instruments.
                let (mine, theirs) = match (vectors.get(*label), human_vectors) {
                    (Some(v), Some(h)) => (cosine(&v[i], &h[i]), cosine(&base_vectors[i], &h[i])),
                    _ => (chrf2(&humans[i], &rows[i]), chrf2(&humans[i], &base[i])),
                };
                // A hair apart is a tie. Without a band, floating-point noise decides sentences
                // where the two gave plainly the same answer.
                if (mine - theirs).abs() < 0.01 {
                    tie += 1;
                } else if mine > theirs {
                    win += 1;
                } else {
                    loss += 1;
                }
            }
            println!(
                "  {:<24}{win:>7}{tie:>7}{loss:>8}{:>10}",
                label,
                sign_test(win, loss)
            );
        }
        println!(
            "  (a tie is |Δcosine| < 0.01, and is excluded from the test as a sign test does)"
        );
    }

    println!("\n  CAT-Translate publishes 33.26 BLEU for JA→EN over five benchmarks. This corpus");
    println!(
        "  and tokenisation are not those, so the number is a sanity check, not a replication."
    );

    // ── Read these ──────────────────────────────────────────────────────────────────────────
    println!("\n=== read these — where the models most disagree ===");
    let mut spread: Vec<(f64, usize)> = (0..corpus.len())
        .map(|i| {
            let scores: Vec<f64> = names
                .iter()
                .map(|l| chrf2(&humans[i], &said[*l][i]))
                .collect();
            let hi = scores.iter().cloned().fold(f64::MIN, f64::max);
            let lo = scores.iter().cloned().fold(f64::MAX, f64::min);
            (hi - lo, i)
        })
        .collect();
    spread.sort_by(|x, y| y.0.total_cmp(&x.0));
    for &(gap, i) in spread.iter().take(4) {
        println!("\n  {}  (chrF2 spread {gap:.2})", corpus[i].0);
        println!("    human                    {}", humans[i]);
        for label in &names {
            println!("    {:<24} {}", label, said[*label][i]);
        }
    }

    assert!(!table.is_empty(), "nothing was compared");
}
