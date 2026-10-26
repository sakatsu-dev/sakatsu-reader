//! Does corpus collocation predict a compound boundary the dictionary cannot?
//!
//! ```text
//! pwsh -File scripts/fetch-jesc.ps1
//! cargo test --release -p sakatsu-core --test compound_pmi -- --ignored --nocapture
//! ```
//!
//! # The question
//!
//! `ud_gold.rs` measured a dictionary-confirmed compound join at **99.8% not-wrong and 23% recall**
//! over the gold long units the analyzer can address. The 77% it misses are not in any dictionary:
//! 在日韓国人組織, 他事業者路線, 震災発生時 — productive compounds rather than lexicalised ones.
//!
//! Whether two morphemes compound is, for those, a fact about how the language uses them together.
//! Pointwise mutual information over adjacent tokens in a large corpus is the oldest measure of
//! exactly that, and it needs **no labels, no training and no model** — the artefact is a table,
//! the same shape as the frequency lists this project already fetches.
//!
//! So this asks the cheapest possible version of the question before anything is built: does PMI
//! separate the pairs gold joins from the pairs gold splits? If it does, a table may be the whole
//! answer. If it does not, that is the evidence that a model has something to learn which a
//! statistic does not already carry.
//!
//! # What is counted, and why only some of it
//!
//! Only **kana-free** tokens. That is the same proxy for nounhood `ud_gold.rs` uses and for the
//! same reason — `Token` carries no part of speech, and それは / には / のに are excluded by
//! construction rather than by a list. It also keeps the table to a size that fits in memory.
//!
//! # The label
//!
//! Gold long units partition the sentence, so a pair (a, b) should be joined exactly when the
//! junction between them is **not** the start of a gold long unit. That is the B/I question
//! stated over the analyzer's own tokens, which is the form any rule or model would answer in.
//!
//! # This measures a feature, not a system
//!
//! A threshold sweep on one feature is not a classifier and the numbers here should not be read as
//! one. What it can say is whether the signal exists and how strong it is — and, if the answer is
//! that PMI alone separates poorly, whether that is because the pairs are simply unseen.

mod common;

use std::collections::HashMap;

use common::gold::{long_units, test_and_dev};
use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::is_kana_free;

/// Where the corpus to count lives. One sentence per line.
fn corpus_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("SAKATSU_PMI_CORPUS") {
        return std::path::PathBuf::from(path);
    }
    common::dictionaries_dir()
        .parent()
        .map(|parent| {
            parent
                .join("corpora")
                .join("jesc")
                .join("jesc-sentences.txt")
        })
        .unwrap_or_default()
}

/// Adjacent-pair counts over an interned vocabulary.
///
/// **Interned deliberately.** A `HashMap<(String, String), u32>` over a few million distinct pairs
/// spends most of its memory on duplicated surfaces; ids make the table fit in a few hundred
/// megabytes instead of failing on a laptop.
#[derive(Default)]
struct Counts {
    ids: HashMap<String, u32>,
    unigram: Vec<u64>,
    bigram: HashMap<(u32, u32), u32>,
    tokens: u64,
    pairs: u64,
}

impl Counts {
    fn id(&mut self, surface: &str) -> u32 {
        if let Some(id) = self.ids.get(surface) {
            return *id;
        }
        let id = self.unigram.len() as u32;
        self.ids.insert(surface.to_string(), id);
        self.unigram.push(0);
        id
    }

    fn observe(&mut self, surfaces: &[&str]) {
        let mut previous: Option<u32> = None;
        for surface in surfaces {
            if !is_kana_free(surface) {
                previous = None;
                continue;
            }
            let id = self.id(surface);
            self.unigram[id as usize] += 1;
            self.tokens += 1;
            if let Some(left) = previous {
                *self.bigram.entry((left, id)).or_default() += 1;
                self.pairs += 1;
            }
            previous = Some(id);
        }
    }

    /// `None` when the pair was never seen, which is a different answer from "seen and unlikely".
    fn pmi(&self, left: &str, right: &str) -> Option<f64> {
        let left = *self.ids.get(left)?;
        let right = *self.ids.get(right)?;
        let joint = *self.bigram.get(&(left, right))? as f64 / self.pairs as f64;
        let p_left = self.unigram[left as usize] as f64 / self.tokens as f64;
        let p_right = self.unigram[right as usize] as f64 / self.tokens as f64;
        Some((joint / (p_left * p_right)).ln())
    }
}

#[test]
#[ignore = "needs scripts/fetch-jesc.ps1 and scripts/fetch-ud-japanese.ps1"]
fn collocation_against_gold_compound_boundaries() {
    let sentences = test_and_dev();
    if sentences.is_empty() {
        println!("SKIP: no treebank — run scripts/fetch-ud-japanese.ps1");
        return;
    }
    let path = corpus_path();
    let Ok(corpus) = std::fs::read_to_string(&path) else {
        println!(
            "SKIP: no corpus at {} — run scripts/fetch-jesc.ps1, or set SAKATSU_PMI_CORPUS",
            path.display()
        );
        return;
    };

    let analyzer = LinderaAnalyzer::new().expect("compiled in");
    let limit = std::env::var("SAKATSU_PMI_SAMPLE")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(1_000_000);

    // ── Count ────────────────────────────────────────────────────────────────────────────────
    let started = std::time::Instant::now();
    let mut counts = Counts::default();
    let mut read = 0usize;
    for line in corpus.lines().take(limit) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(analysis) = analyzer.analyze(line) else {
            continue;
        };
        let surfaces: Vec<&str> = analysis
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect();
        counts.observe(&surfaces);
        read += 1;
    }
    println!(
        "counted {read} sentences in {:.1}s — {} distinct kana-free tokens, {} distinct adjacent pairs",
        started.elapsed().as_secs_f64(),
        counts.unigram.len(),
        counts.bigram.len(),
    );

    // ── Label every candidate pair in gold ───────────────────────────────────────────────────
    let jitendex = common::dictionaries_dir().join("jitendex");
    let dictionaries: Vec<Dictionary> = Dictionary::open(&jitendex).into_iter().collect();
    let known = |surface: &str| -> bool {
        dictionaries.iter().any(|dictionary| {
            dictionary
                .lookup(surface)
                .map(|hits| hits.iter().any(|hit| hit.entry.term == surface))
                .unwrap_or(false)
        })
    };

    // (pmi, joined-by-gold), for pairs the dictionary cannot already settle.
    let mut scored: Vec<(f64, bool)> = Vec::new();
    let (mut unseen_join, mut unseen_split) = (0usize, 0usize);
    let (mut total_join, mut total_split) = (0usize, 0usize);

    for sentence in &sentences {
        let Ok(analysis) = analyzer.analyze(&sentence.text) else {
            continue;
        };
        // Every offset that begins a gold long unit. A junction on one of these is a boundary.
        let starts: std::collections::BTreeSet<usize> = long_units(&sentence.tokens)
            .into_iter()
            .map(|(start, _)| start)
            .collect();

        let tokens = &analysis.tokens;
        for pair in tokens.windows(2) {
            let (Some(a_end), Some(b_start), Some(a_start), Some(b_end)) =
                (pair[0].end, pair[1].start, pair[0].start, pair[1].end)
            else {
                continue;
            };
            if a_end != b_start {
                continue;
            }
            if !is_kana_free(&pair[0].surface) || !is_kana_free(&pair[1].surface) {
                continue;
            }
            // The residual: what a dictionary lookup could not already decide.
            if known(&sentence.text[a_start..b_end]) {
                continue;
            }

            let joined = !starts.contains(&b_start);
            if joined {
                total_join += 1;
            } else {
                total_split += 1;
            }
            match counts.pmi(&pair[0].surface, &pair[1].surface) {
                Some(pmi) => scored.push((pmi, joined)),
                None if joined => unseen_join += 1,
                None => unseen_split += 1,
            }
        }
    }

    println!(
        "\ngold candidate pairs (kana-free, dictionary cannot settle)\n  {total_join} gold joins, {total_split} gold splits",
    );
    println!(
        "  never seen together in the corpus: {unseen_join} of the joins, {unseen_split} of the splits  ({:.1}% of all pairs)",
        100.0 * (unseen_join + unseen_split) as f64 / (total_join + total_split).max(1) as f64,
    );
    if scored.is_empty() {
        println!("nothing scored — the corpus and the gold set share no pairs");
        return;
    }

    // ── Does the score separate them? ────────────────────────────────────────────────────────
    let mean = |want: bool| -> f64 {
        let picked: Vec<f64> = scored
            .iter()
            .filter(|(_, joined)| *joined == want)
            .map(|(pmi, _)| *pmi)
            .collect();
        picked.iter().sum::<f64>() / picked.len().max(1) as f64
    };
    println!(
        "\nmean PMI  joined {:.2}   split {:.2}   (higher should mean 'one word')",
        mean(true),
        mean(false)
    );

    println!("\nthreshold sweep — join when PMI is at least the threshold");
    println!("  thresh   joins made   correct   precision   recall of scored joins");
    let scored_joins = scored.iter().filter(|(_, joined)| *joined).count();
    for step in 0..=12 {
        let threshold = step as f64;
        let made = scored.iter().filter(|(pmi, _)| *pmi >= threshold).count();
        let correct = scored
            .iter()
            .filter(|(pmi, joined)| *pmi >= threshold && *joined)
            .count();
        println!(
            "  {threshold:>5.1}   {made:>10}   {correct:>7}   {:>8.1}%   {:>8.1}%",
            100.0 * correct as f64 / made.max(1) as f64,
            100.0 * correct as f64 / scored_joins.max(1) as f64,
        );
    }

    // The base rate is what any threshold has to beat to be worth anything at all.
    println!(
        "\n  base rate (join everything scored): {:.1}% precision over {} pairs",
        100.0 * scored_joins as f64 / scored.len() as f64,
        scored.len(),
    );
}
