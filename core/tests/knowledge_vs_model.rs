//! Does the app's own knowledge carry a sentence's meaning better than FuguMT does?
//!
//! ```text
//! cargo test --release -p sakatsu-core --test knowledge_vs_model -- --ignored --nocapture
//! ```
//!
//! `--release` is not optional: this runs the translation model over a few hundred sentences.
//!
//! # The question
//!
//! The translation panel could say more from either of two places — a 61M-parameter model, or the
//! dictionary, particle senses and grammar packs already in memory. `lens_agreement.rs` measured
//! the model at its weakest (a word alone matches what a sentence lost **34.6%** of the time, a
//! particle **1.4%**) but never measured the alternative. This does.
//!
//! # It cannot be a fair fight on every axis, so it is not scored as one
//!
//! FuguMT writes an English sentence. The app's knowledge produces a **chain of glosses in
//! Japanese word order** — it is not trying to be English and marking it down for that would be
//! measuring the wrong thing. So the score is **order-free content-word overlap** against a human
//! translation, reported as recall (did it carry the meaning) and precision (how much noise came
//! with it), and a sample is printed so the rates can be read against the sentences that produced
//! them.
//!
//! The normalisation — lowercase, strip punctuation, drop function words, trim a few suffixes — is
//! crude. It stays crude on purpose and the comparison survives it, because **both sides are
//! scored by the same crude rule**: what is soft is the absolute number, not the gap between them.
//!
//! It is deliberately *not* in `translate::compare`. That module is Rust because the number it
//! computes decides how the panel is worded — one implementation, both callers. This one decides
//! nothing yet, and moving it into the library would claim a permanence it has not earned.
//!
//! # The corpus
//!
//! Two tiers, both Tatoeba-derived through Jitendex, CC BY-SA 4.0, already credited (`project notes`
//! §8) and neither written here:
//!
//! - **`common/sentences.rs`** — 50 native sentences, ten per JLPT level, each with its human
//!   translation. This is the complexity variation, and its banding is authored rather than a
//!   heuristic invented for this test.
//! - **`common::jitendex_examples()`** — a larger ungraded pool, so the rate has somewhere to
//!   settle. Fifty sentences cannot support a percentage.
//!
//! **The JLPT band is not a grading of the sentence.** `sentences.rs` says so itself: the level is
//! the level of the *pattern* the sentence was chosen for. So each band also reports measured
//! complexity — tokens, kanji, deepest conjugation chain — and whether those track the band is a
//! finding in its own right.
//!
//! # This is a measurement, not a gate
//!
//! There is no threshold to pass. It asserts only that something was compared.

mod common;
#[path = "common/sentences.rs"]
mod sentences;

use std::collections::BTreeSet;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::dict::Dictionary;
use sakatsu_core::grammar::particles::{self, Categories};
use sakatsu_core::model::{Analysis, Token};
use sakatsu_core::translate::{FuguMt, Translator};

use common::{content, per_token, Score, TokenKnowledge};
use sentences::SENTENCES;

/// How many ungraded pairs to pool. Each costs one translation, so this is the dial between a
/// coffee and a lunch.
const POOLED: usize = 300;

// ── What the app knows about a sentence ─────────────────────────────────────────────────────

/// The app's own reading of a sentence, in Japanese word order.
struct Knowledge {
    /// Dictionary glosses only, at the gloss limit given.
    glosses: String,
    /// The same, plus the authored layer: particle senses, conjugation meanings, pattern
    /// explanations. Everything the app could put on screen without a model.
    authored: String,
}

/// The same knowledge, joined into the two readings the first measurement scores.
///
/// Built on [`per_token`] rather than beside it, so the two measurements cannot end up asking
/// the dictionary different questions.
fn what_the_app_knows(
    analysis: &Analysis,
    dictionary: &Dictionary,
    categories: &Categories,
    limit: usize,
) -> Knowledge {
    let words = per_token(analysis, dictionary, categories, limit);
    let glosses: Vec<&str> = words
        .iter()
        .map(|word| word.glosses.as_str())
        .filter(|text| !text.is_empty())
        .collect();

    let mut authored: Vec<String> = Vec::new();
    for word in &words {
        if !word.glosses.is_empty() {
            authored.push(word.glosses.clone());
        }
        authored.extend(word.authored.iter().cloned());
    }
    // The grammar packs, over the whole sentence rather than per word: a pattern is a thing the
    // sentence does, not a thing one token means.
    for matched in sakatsu_core::grammar::patterns(analysis) {
        if let Some(pattern) = sakatsu_core::grammar::pack::find(&matched.pattern_id) {
            authored.push(pattern.explanation.clone());
        }
    }

    Knowledge {
        glosses: glosses.join(" / "),
        authored: authored.join(" / "),
    }
}

// ── Measured complexity, so the JLPT band can be checked against something ───────────────────

#[derive(Default, Clone, Copy)]
struct Shape {
    tokens: usize,
    kanji: usize,
    deepest_chain: usize,
    sentences: usize,
}

fn shape_of(analysis: &Analysis) -> Shape {
    let deepest = analysis
        .tokens
        .iter()
        .map(|token: &Token| sakatsu_core::grammar::breakdown(token).steps.len())
        .max()
        .unwrap_or(0);
    Shape {
        tokens: analysis.tokens.iter().filter(|t| t.is_word()).count(),
        kanji: analysis
            .text
            .chars()
            .filter(|c| sakatsu_core::kana::is_kanji(*c))
            .count(),
        deepest_chain: deepest,
        sentences: 1,
    }
}

impl Shape {
    fn merge(&mut self, other: Shape) {
        self.tokens += other.tokens;
        self.kanji += other.kanji;
        self.deepest_chain += other.deepest_chain;
        self.sentences += other.sentences;
    }
    fn per_sentence(&self) -> (f64, f64, f64) {
        let n = self.sentences.max(1) as f64;
        (
            self.tokens as f64 / n,
            self.kanji as f64 / n,
            self.deepest_chain as f64 / n,
        )
    }
}

// ── The run ─────────────────────────────────────────────────────────────────────────────────

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

fn categories() -> Categories {
    let path = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .map(|dir| {
            dir.join("com.sakatsu.app")
                .join("wordnet")
                .join("categories.tsv")
        });
    match path {
        Some(path) => Categories::load(&path),
        None => Categories::default(),
    }
}

/// One sentence, scored every way.
struct Row {
    japanese: String,
    human: String,
    model: String,
    glosses: [String; 3],
    authored: [String; 3],
}

/// The gloss limits reported side by side. See `what_the_app_knows`.
const LIMITS: [usize; 3] = [1, 3, usize::MAX];
const LIMIT_NAMES: [&str; 3] = ["×1", "×3", "×all"];

fn read(
    japanese: &str,
    human: &str,
    analyzer: &LinderaAnalyzer,
    engine: &FuguMt,
    dictionary: &Dictionary,
    categories: &Categories,
) -> Option<(Row, Shape)> {
    let analysis = analyzer.analyze(japanese).ok()?;
    if analysis.tokens.is_empty() {
        return None;
    }
    let model = engine.translate(japanese).ok()?;
    let shape = shape_of(&analysis);

    let mut glosses: [String; 3] = Default::default();
    let mut authored: [String; 3] = Default::default();
    for (slot, limit) in LIMITS.iter().enumerate() {
        let known = what_the_app_knows(&analysis, dictionary, categories, *limit);
        glosses[slot] = known.glosses;
        authored[slot] = known.authored;
    }

    Some((
        Row {
            japanese: japanese.to_string(),
            human: human.to_string(),
            model,
            glosses,
            authored,
        },
        shape,
    ))
}

/// Every score for one group of sentences: the model, then the two knowledge readings at each
/// gloss limit.
#[derive(Default)]
struct Tally {
    model: Score,
    glosses: [Score; 3],
    authored: [Score; 3],
    shape: Shape,
    /// Content words the human had, the model dropped, and the app's knowledge held.
    rescued: usize,
    /// Content words the human had and the model dropped, whether or not anything rescued them.
    dropped: usize,
}

impl Tally {
    fn add(&mut self, row: &Row, shape: Shape) {
        let human = content(&row.human);
        let model = content(&row.model);
        self.model.add(&human, &model);
        for slot in 0..3 {
            self.glosses[slot].add(&human, &content(&row.glosses[slot]));
            self.authored[slot].add(&human, &content(&row.authored[slot]));
        }
        self.shape.merge(shape);

        // The actionable number: what the model lost, and whether the app had it. Measured at
        // the middle gloss limit, since that is the one a feature would plausibly draw.
        let knowledge = content(&row.authored[1]);
        for word in human.difference(&model) {
            self.dropped += 1;
            if knowledge.contains(word) {
                self.rescued += 1;
            }
        }
    }

    fn print_rates(&self, indent: &str) {
        let line = |name: &str, score: &Score| {
            println!(
                "{indent}{name:<20}{:>7.2}{:>11.2}{:>7.2}",
                score.recall(),
                score.precision(),
                score.f1()
            );
        };
        line("FuguMT", &self.model);
        for (name, score) in LIMIT_NAMES.iter().zip(&self.glosses) {
            line(&format!("glosses {name}"), score);
        }
        for (name, score) in LIMIT_NAMES.iter().zip(&self.authored) {
            line(&format!("+authored {name}"), score);
        }
    }
}

#[test]
#[ignore = "needs the translation model and the imported Jitendex; use --release"]
fn the_apps_knowledge_is_measured_against_the_model() {
    let dir = model_dir();
    if !FuguMt::is_present(&dir) {
        println!(
            "SKIP: no translation model at {} — run scripts/fetch-translation-model.ps1",
            dir.display()
        );
        return;
    }
    let imported = common::dictionaries_dir().join("jitendex");
    if !sakatsu_core::dict::index::Dictionary::exists_in(&imported) {
        println!(
            "SKIP: no imported Jitendex at {} — run scripts/fetch-dictionaries.ps1",
            imported.display()
        );
        return;
    }

    let engine = FuguMt::load(&dir).expect("load the model");
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let dictionary = Dictionary::open(&imported).expect("opens");
    let table = categories();
    let started = std::time::Instant::now();

    // **The metric is checked before anything is measured with it.** A reference scored against
    // itself must be perfect and an unrelated sentence must score near nothing; a rule that
    // cannot manage that is not measuring what it claims to, and every number below would be
    // decoration.
    {
        let human = content("Even if you deny it in your heart, you seek domination by me.");
        let mut same = Score::default();
        same.add(&human, &human);
        let mut other = Score::default();
        other.add(&human, &content("The cat sat on the mat."));
        println!("\n=== the metric, checked against itself ===");
        println!(
            "  reference vs itself      recall {:.2}  precision {:.2}",
            same.recall(),
            same.precision()
        );
        println!(
            "  reference vs a stranger  recall {:.2}  precision {:.2}",
            other.recall(),
            other.precision()
        );
        assert!(
            same.recall() == 1.0 && same.precision() == 1.0,
            "the metric cannot score a sentence against itself"
        );
        assert!(
            other.recall() < 0.2,
            "the metric calls unrelated sentences similar"
        );
    }

    // ── The banded tier ─────────────────────────────────────────────────────────────────────
    let mut bands: Vec<(&str, Tally, Vec<Row>)> = ["N5", "N4", "N3", "N2", "N1"]
        .iter()
        .map(|level| (*level, Tally::default(), Vec::new()))
        .collect();

    for sentence in SENTENCES {
        let Some((row, shape)) = read(
            sentence.text,
            sentence.english,
            &analyzer,
            &engine,
            &dictionary,
            &table,
        ) else {
            continue;
        };
        if let Some(band) = bands
            .iter_mut()
            .find(|(level, _, _)| *level == sentence.level)
        {
            band.1.add(&row, shape);
            band.2.push(row);
        }
    }

    println!(
        "\n=== by JLPT band — 10 native sentences each, human translation as the reference ==="
    );
    println!("\n  what the band actually contains, measured:");
    println!(
        "        {:>8}{:>8}{:>16}",
        "words", "kanji", "deepest chain"
    );
    for (level, tally, _) in &bands {
        let (tokens, kanji, chain) = tally.shape.per_sentence();
        println!("    {level}  {tokens:>8.1}{kanji:>8.1}{chain:>16.1}");
    }
    println!(
        "\n  (the level is the level of the *pattern* the sentence was chosen for — see\n   \
         common/sentences.rs. Whether it tracks difficulty is what the row above says.)"
    );

    println!("\n{:>26}{:>11}{:>7}", "recall", "precision", "F1");
    for (level, tally, _) in &bands {
        println!("  {level}");
        tally.print_rates("    ");
    }

    // ── The pooled tier ─────────────────────────────────────────────────────────────────────
    let corpus = common::jitendex_examples();
    let usable: Vec<&(String, String)> = corpus
        .iter()
        .filter(|(japanese, english)| {
            !japanese.is_empty() && english.split_whitespace().count() > 3
        })
        .collect();
    let step = (usable.len() / POOLED.max(1)).max(1);

    let mut pooled = Tally::default();
    let mut pooled_rows: Vec<Row> = Vec::new();
    for (japanese, english) in usable.iter().step_by(step).take(POOLED) {
        let Some((row, shape)) = read(japanese, english, &analyzer, &engine, &dictionary, &table)
        else {
            continue;
        };
        pooled.add(&row, shape);
        pooled_rows.push(row);
    }

    println!(
        "\n=== pooled — {} ungraded pairs, spread across {} ===",
        pooled.model.sentences,
        usable.len()
    );
    let (tokens, kanji, chain) = pooled.shape.per_sentence();
    println!("  words {tokens:.1}   kanji {kanji:.1}   deepest chain {chain:.1}");
    println!("\n{:>26}{:>11}{:>7}", "recall", "precision", "F1");
    pooled.print_rates("  ");

    println!("\n  **what the model lost, and whether the app had it**");
    for (name, tally) in [
        (
            "banded",
            bands.iter().fold(Tally::default(), |mut all, (_, t, _)| {
                all.rescued += t.rescued;
                all.dropped += t.dropped;
                all
            }),
        ),
        (
            "pooled",
            Tally {
                rescued: pooled.rescued,
                dropped: pooled.dropped,
                ..Default::default()
            },
        ),
    ] {
        println!(
            "    {name:<8} content words the human had that FuguMT dropped: {:<6} \
             of those the app knew: {:<6} ({:.1}%)",
            tally.dropped,
            tally.rescued,
            100.0 * tally.rescued as f64 / tally.dropped.max(1) as f64
        );
    }

    // ── Read these ──────────────────────────────────────────────────────────────────────────
    //
    // A rate with no examples beside it is a number nobody can act on. Two per band, from the
    // ends: the sentence the model did best on and the one it did worst on, so the sample is not
    // a flattering slice.
    println!("\n=== read these — the model's best and worst in each band ===");
    for (level, _, rows) in &bands {
        if rows.is_empty() {
            continue;
        }
        let scored: Vec<(f64, &Row)> = rows
            .iter()
            .map(|row| {
                let mut score = Score::default();
                score.add(&content(&row.human), &content(&row.model));
                (score.f1(), row)
            })
            .collect();
        let best = scored
            .iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .expect("rows");
        let worst = scored
            .iter()
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .expect("rows");
        for (what, (f1, row)) in [("best", best), ("worst", worst)] {
            println!("\n  {level} {what} (FuguMT F1 {f1:.2})");
            println!("    {}", row.japanese);
            println!("    human    {}", row.human);
            println!("    FuguMT   {}", row.model);
            println!("    glosses  {}", clip(&row.glosses[0], 150));
            println!("    authored {}", clip(&row.authored[1], 150));
        }
    }

    println!("\n  measured in {:.0}s", started.elapsed().as_secs_f64());
    assert!(
        pooled.model.sentences > 0,
        "nothing was compared — the corpus, the dictionary or the model is missing"
    );
}

/// Long enough to judge, short enough that one line does not bury the next.
fn clip(text: &str, at: usize) -> String {
    if text.chars().count() <= at {
        return text.to_string();
    }
    let head: String = text.chars().take(at).collect();
    format!("{head}…")
}

// ── The flipped denominator ─────────────────────────────────────────────────────────────────
//
// The first measurement asked: of the meaning FuguMT lost, how much did the app have? It answered
// 25.8%, and that number **cannot be computed at runtime** — its denominator is the human
// translation, which the app will never see.
//
// What the app *can* compute live is the other set: **words it knows that the model did not say.**
// This measures that set instead, which is the one a line on screen would actually be drawn from,
// and asks the two questions that decide whether it is worth drawing:
//
// - **precision** — of the words it would print, how many are really in the human translation?
// - **length** — how many words is that, per sentence? A line of three at 40% is a feature. The
//   same 40% spread over twenty-five words is the gloss chain again in a shorter coat.
//
// Precision is still measured against one human translation, so a legitimate synonym the
// translator did not happen to use counts against the line. These numbers are a floor.

/// One way of choosing what to print, and how it did.
#[derive(Default)]
struct Line {
    /// Words printed that were in the human translation.
    hit: usize,
    /// Words printed.
    shown: usize,
    /// Human words the model dropped that this line recovered.
    recovered: usize,
    /// Human words the model dropped, full stop — the same denominator for every strategy.
    droppable: usize,
    /// Sentences where the strategy would print nothing. Not a failure: having nothing to add is
    /// an honest outcome, and a line that is silent half the time is a different proposition from
    /// one that always speaks.
    silent: usize,
    sentences: usize,
}

impl Line {
    fn add(&mut self, shown: &[String], human: &BTreeSet<String>, model: &BTreeSet<String>) {
        let printed: BTreeSet<&String> = shown.iter().collect();
        self.hit += printed.iter().filter(|word| human.contains(**word)).count();
        self.shown += printed.len();
        let dropped: BTreeSet<&String> = human.difference(model).collect();
        self.droppable += dropped.len();
        self.recovered += dropped
            .iter()
            .filter(|word| printed.contains(**word))
            .count();
        if printed.is_empty() {
            self.silent += 1;
        }
        self.sentences += 1;
    }
    fn precision(&self) -> f64 {
        self.hit as f64 / self.shown.max(1) as f64
    }
    fn per_sentence(&self) -> f64 {
        self.shown as f64 / self.sentences.max(1) as f64
    }
    fn recovered_share(&self) -> f64 {
        self.recovered as f64 / self.droppable.max(1) as f64
    }
    fn silent_share(&self) -> f64 {
        self.silent as f64 / self.sentences.max(1) as f64
    }
}

/// The strategies, in order of how much they trust the dictionary.
const STRATEGIES: [&str; 8] = [
    "everything x1",
    "everything x3",
    "everything +authored",
    "uncovered words x1",
    "uncovered words, cap 3",
    "uncovered words, cap 5",
    "content only, cap 3",
    "content only, cap 5",
];

/// Gloss openings that describe a word's grammatical job rather than say what it means.
///
/// **This filter was invented for the experiment**, not lifted from the app, and it is the one
/// piece of this measurement that is a guess rather than a reading of what the app does. The
/// first run pointed straight at it: the commonest thing the line printed was
/// `indicates / sentence / topic`, which is a true and useful thing to know about は and is never
/// a word in an English translation.
const META_GLOSS: [&str; 7] = [
    "indicates",
    "indicating",
    "particle",
    "auxiliary",
    "marks",
    "used as",
    "used to",
];

/// Whether a gloss is saying what a word *means*, as opposed to what it *does*.
fn says_a_meaning(gloss: &str) -> bool {
    let lowered = gloss.trim().to_ascii_lowercase();
    !META_GLOSS.iter().any(|meta| lowered.starts_with(meta))
}

/// What each strategy would print for one sentence.
///
/// **The `uncovered words` family is the interesting one.** Rather than diffing the whole gloss
/// bag against the English, it asks per word: did *any* gloss of this word survive into the
/// model's output? If none did, the model said nothing about that word, and its leading gloss is
/// worth offering. That is a claim about a token rather than about a string, and it is the
/// difference between "the model phrased this another way" and "the model dropped this".
fn lines_for(
    analysis: &Analysis,
    dictionary: &Dictionary,
    categories: &Categories,
    model: &BTreeSet<String>,
) -> ([Vec<String>; 8], Vec<String>) {
    let unsaid = |text: &str| -> Vec<String> {
        content(text)
            .into_iter()
            .filter(|word| !model.contains(word))
            .collect()
    };

    let one = per_token(analysis, dictionary, categories, 1);
    let three = per_token(analysis, dictionary, categories, 3);
    let joined = |words: &[TokenKnowledge]| {
        words
            .iter()
            .map(|word| word.glosses.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };

    let everything_one = unsaid(&joined(&one));
    let everything_three = unsaid(&joined(&three));
    let everything_authored =
        unsaid(&what_the_app_knows(analysis, dictionary, categories, 1).authored);

    // Per word: was this one covered at all?
    let mut uncovered: Vec<String> = Vec::new();
    let mut seen = BTreeSet::new();
    for word in &one {
        if word.every_gloss.is_empty() {
            continue;
        }
        let covered = content(&word.every_gloss)
            .iter()
            .any(|gloss| model.contains(gloss));
        if covered {
            continue;
        }
        for gloss in content(&word.glosses) {
            if seen.insert(gloss.clone()) {
                uncovered.push(gloss);
            }
        }
    }

    // The same again, with the two things the first run showed were drowning it: the particles,
    // which the app has its own authored list of, and the glosses that describe a job rather than
    // state a meaning.
    let mut content_only: Vec<String> = Vec::new();
    let mut readable: Vec<String> = Vec::new();
    let mut kept = BTreeSet::new();
    for (word, token) in one
        .iter()
        .zip(analysis.tokens.iter().filter(|t| t.is_word()))
    {
        if word.every_gloss.is_empty() || particles::entry(&token.surface).is_some() {
            continue;
        }
        if !says_a_meaning(&word.glosses) {
            continue;
        }
        let covered = content(&word.every_gloss)
            .iter()
            .any(|gloss| model.contains(gloss));
        if covered {
            continue;
        }
        let mut fresh = false;
        for gloss in content(&word.glosses) {
            if kept.insert(gloss.clone()) {
                content_only.push(gloss);
                fresh = true;
            }
        }
        if fresh {
            readable.push(word.glosses.clone());
        }
    }

    let cap = |source: &[String], n: usize| source.iter().take(n).cloned().collect::<Vec<_>>();
    let (three, five) = (cap(&uncovered, 3), cap(&uncovered, 5));
    let (content_three, content_five) = (cap(&content_only, 3), cap(&content_only, 5));
    (
        [
            everything_one,
            everything_three,
            everything_authored,
            uncovered,
            three,
            five,
            content_three,
            content_five,
        ],
        // **The glosses as written, for the sample only.** Everything scored above is normalised
        // — lowercased and stemmed — which is right for counting and wrong for reading: it prints
        // `lik` for *liking* and `indicat` for *indicates*. Judging the idea by that would be
        // judging the stemmer. The rates use stems; the sample shows words.
        cap(&readable, 3),
    )
}

#[test]
#[ignore = "needs the translation model and the imported Jitendex; use --release"]
fn what_the_app_would_print_beside_a_translation_is_measured() {
    let dir = model_dir();
    if !FuguMt::is_present(&dir) {
        println!("SKIP: no translation model at {}", dir.display());
        return;
    }
    let imported = common::dictionaries_dir().join("jitendex");
    if !sakatsu_core::dict::index::Dictionary::exists_in(&imported) {
        println!("SKIP: no imported Jitendex at {}", imported.display());
        return;
    }

    let engine = FuguMt::load(&dir).expect("load the model");
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let dictionary = Dictionary::open(&imported).expect("opens");
    let table = categories();
    let started = std::time::Instant::now();

    let corpus = common::jitendex_examples();
    let usable: Vec<&(String, String)> = corpus
        .iter()
        .filter(|(japanese, english)| {
            !japanese.is_empty() && english.split_whitespace().count() > 3
        })
        .collect();
    let step = (usable.len() / POOLED.max(1)).max(1);

    // The same two tiers the first measurement uses, so the two reports read together.
    let banded: Vec<(String, String)> = SENTENCES
        .iter()
        .map(|s| (s.text.to_string(), s.english.to_string()))
        .collect();
    let pooled: Vec<(String, String)> = usable
        .iter()
        .step_by(step)
        .take(POOLED)
        .map(|(japanese, english)| (japanese.clone(), english.clone()))
        .collect();

    let mut samples: Vec<(String, String, String, Vec<String>)> = Vec::new();

    for (tier, corpus) in [("banded, 50 native", &banded), ("pooled, 300", &pooled)] {
        let mut lines: [Line; 8] = Default::default();

        for (japanese, english) in corpus {
            let Ok(analysis) = analyzer.analyze(japanese) else {
                continue;
            };
            if analysis.tokens.is_empty() {
                continue;
            }
            let Ok(translated) = engine.translate(japanese) else {
                continue;
            };
            let human = content(english);
            let model = content(&translated);
            let (built, readable) = lines_for(&analysis, &dictionary, &table, &model);
            for (slot, shown) in built.iter().enumerate() {
                lines[slot].add(shown, &human, &model);
            }
            // The filtered, capped line is the one a feature would draw, so that is what gets
            // sampled — showing the unfiltered one would be sampling a strategy nobody would ship.
            if samples.len() < 10 && !readable.is_empty() {
                samples.push((
                    japanese.clone(),
                    english.clone(),
                    translated.clone(),
                    readable,
                ));
            }
        }

        println!("\n=== what the app would print beside the translation — {tier} ===");
        println!(
            "  {:<26}{:>11}{:>9}{:>12}{:>9}",
            "strategy", "precision", "words", "of dropped", "silent"
        );
        for (name, line) in STRATEGIES.iter().zip(&lines) {
            println!(
                "  {:<26}{:>11.2}{:>9.1}{:>11.0}%{:>8.0}%",
                name,
                line.precision(),
                line.per_sentence(),
                100.0 * line.recovered_share(),
                100.0 * line.silent_share(),
            );
        }
    }

    println!("\n=== read these — the capped line, as it would appear ===");
    for (japanese, human, model, shown) in samples.iter().take(8) {
        println!("\n  {japanese}");
        println!("    human   {human}");
        println!("    FuguMT  {model}");
        println!("    beside  {}", shown.join(" / "));
    }

    println!("\n  measured in {:.0}s", started.elapsed().as_secs_f64());
}
