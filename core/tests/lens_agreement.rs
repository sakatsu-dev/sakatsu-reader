//! How often do the comparison strip's two lenses agree?
//!
//! ```text
//! cargo test -p sakatsu-core --test lens_agreement -- --ignored --nocapture
//! ```
//!
//! # What this measures, and why it is the number that matters
//!
//! The strip asks two questions about a selected word: what it says **alone**, and what the
//! sentence loses **without** it. The first is the question a learner asks and the one this model
//! is worst at — it was trained on sentences and a bare particle phrase is not one. The second is
//! robust, because its input is still a sentence.
//!
//! Showing both only helps if they usually corroborate. If they agree most of the time, the
//! fragment can be shown plainly and the agreement note means something. If they rarely do, the
//! strip is presenting a coin flip beside a real answer and should lead with the ablation and say
//! so. **This number decides the wording**, which is why the rule it measures
//! ([`sakatsu_core::translate::lenses_agree`]) is the same one the panel applies rather than a
//! reimplementation that could quietly differ.
//!
//! # The corpus
//!
//! Jitendex example sentences — Tatoeba-derived, CC BY-SA 4.0, already on disk, already attributed,
//! and with nothing to do with this feature. The same independent corpus `pack_precision.rs` uses,
//! through the same shared loader.
//!
//! # This is a measurement, not a gate
//!
//! There is no threshold to pass. It prints a rate and a sample of both outcomes so the sample can
//! be read — a rate with no examples beside it is a number nobody can act on.

mod common;

use std::collections::BTreeSet;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::model::Token;
use sakatsu_core::translate::{lenses_agree, variants, word_diff, FuguMt, Translator};

fn model_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_MT_DIR") {
        return std::path::PathBuf::from(dir);
    }
    let appdata = std::env::var_os("APPDATA").expect("APPDATA (or set SAKATSU_MT_DIR)");
    std::path::PathBuf::from(appdata)
        .join("com.sakatsu.app")
        .join("models")
        .join("fugumt-ja-en")
}

/// How many sentences to read. Each costs one translation for the sentence plus two per word
/// studied, so this is the dial between a coffee and a lunch.
const SENTENCES: usize = 120;

/// Words worth studying: the ones a learner would actually tick.
///
/// Punctuation and lone kana that are not particles teach nothing, and including them would pad
/// the denominator with questions nobody asks.
fn worth_studying(token: &Token) -> bool {
    let surface = token.surface.trim();
    if surface.chars().count() > 12 {
        return false;
    }
    surface.chars().any(sakatsu_core::kana::is_japanese)
        && !surface
            .chars()
            .all(|c| matches!(c, '。' | '、' | '！' | '？' | '「' | '」' | '・' | '　'))
}

#[test]
#[ignore = "needs the translation model and scripts/fetch-dictionaries.ps1"]
fn the_two_lenses_are_measured_against_an_independent_corpus() {
    let dir = model_dir();
    assert!(
        FuguMt::is_present(&dir),
        "no model at {} — run scripts/fetch-translation-model.ps1",
        dir.display()
    );
    let engine = FuguMt::load(&dir).expect("load the model");
    let analyzer = LinderaAnalyzer::new().expect("built-in analyzer");

    let corpus = common::jitendex_sentences();
    assert!(
        !corpus.is_empty(),
        "no corpus — run scripts/fetch-dictionaries.ps1"
    );

    // Spread across the corpus rather than taking the first N: the banks are ordered, and the
    // opening of the first is not a sample of the language.
    let step = (corpus.len() / SENTENCES).max(1);

    let mut asked = 0usize;
    let mut agreed = 0usize;
    // Split by whether the app has an authored, cited sense for this word. Particles are what a
    // learner most wants to study *and* the hardest thing to translate in isolation, so a single
    // pooled rate would hide the case the feature exists for.
    let mut particle_asked = 0usize;
    let mut particle_agreed = 0usize;
    let mut empty_runs = 0usize;
    let mut agreeing: Vec<String> = Vec::new();
    let mut differing: Vec<String> = Vec::new();
    let started = std::time::Instant::now();

    for source in corpus.iter().step_by(step).take(SENTENCES) {
        let analysis = match analyzer.analyze(source) {
            Ok(analysis) => analysis,
            Err(_) => continue,
        };
        let tokens = &analysis.tokens;
        if tokens.is_empty() {
            continue;
        }
        let Ok(full) = engine.translate(source) else {
            continue;
        };

        for (index, token) in tokens.iter().enumerate() {
            if !worth_studying(token) {
                continue;
            }
            let picked: BTreeSet<usize> = std::iter::once(index).collect();
            let built = variants(tokens, &picked)
                .expect("an enumerated token is a valid one-sentence selection")
                .expect("one selected token is a comparison question");
            if built.without.is_empty() || built.runs.is_empty() {
                continue;
            }
            let (Ok(alone), Ok(without)) = (
                engine.translate(&built.runs.join(" ")),
                engine.translate(&built.without),
            ) else {
                continue;
            };

            let pieces = word_diff(&full, &without);
            if !pieces.iter().any(|piece| piece.gone) {
                // Removing the word changed nothing in the English at all. Not a disagreement —
                // the ablation simply had nothing to say — and counting it either way would be a
                // claim this cannot support.
                empty_runs += 1;
                continue;
            }

            asked += 1;
            let authored = sakatsu_core::grammar::particles::entry(&token.surface).is_some();
            if authored {
                particle_asked += 1;
            }
            let lost: String = pieces
                .iter()
                .filter(|p| p.gone)
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>()
                .join(" … ");
            let line = format!("{}  alone={alone:?}  lost={lost:?}", token.surface);
            if lenses_agree(&alone, &pieces) {
                agreed += 1;
                if authored {
                    particle_agreed += 1;
                }
                if agreeing.len() < 8 {
                    agreeing.push(line);
                }
            } else if differing.len() < 8 {
                differing.push(line);
            }
        }
    }

    let rate = 100.0 * agreed as f64 / asked.max(1) as f64;
    println!("\n  sentences read      {SENTENCES}");
    println!("  comparisons made    {asked}");
    println!("  removing the word changed nothing  {empty_runs}  (not counted either way)");
    println!("  **the two lenses agreed  {agreed}  ({rate:.1}%)**");
    println!("  measured in {:.0}s", started.elapsed().as_secs_f64());

    let other_asked = asked - particle_asked;
    let other_agreed = agreed - particle_agreed;
    println!(
        "
  split by whether the app has an authored sense for the word:"
    );
    println!(
        "    particles with a written sense   {particle_agreed:>4} / {particle_asked:<4} ({:.1}%)",
        100.0 * particle_agreed as f64 / particle_asked.max(1) as f64
    );
    println!(
        "    everything else                  {other_agreed:>4} / {other_asked:<4} ({:.1}%)",
        100.0 * other_agreed as f64 / other_asked.max(1) as f64
    );

    println!("\n  where they agreed — the fragment is what the sentence lost:");
    for line in &agreeing {
        println!("    {line}");
    }
    println!("\n  where they did not — read these before trusting the fragment alone:");
    for line in &differing {
        println!("    {line}");
    }

    assert!(
        asked > 0,
        "nothing was compared — the corpus or the model is missing"
    );
}
