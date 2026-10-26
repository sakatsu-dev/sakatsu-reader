//! Four ways to make FuguMT more accurate without replacing it, measured.
//!
//! ```text
//! cargo test --release -p sakatsu-core --test fugumt_levers -- --ignored --nocapture
//! ```
//!
//! # Why these four
//!
//! `translation_quality.rs` found no candidate worth swapping FuguMT for: the best instruct model
//! beat it 137–104 paired (p = 0.039, which does not survive correction for five comparisons) at
//! 16× the size and an external server. So the question turned into whether the incumbent can be
//! improved in place. These are the levers that exist:
//!
//! | lever | why it might work |
//! |---|---|
//! | **sentence-at-a-time** | Marian is trained on *sentence pairs*; the app hands it whole passages |
//! | **input normalisation** | OCR gives full-width digits, half-width kana and stray line breaks |
//! | **no-repeat-ngram** | the greedy loop has only `MAX_TOKENS` between it and a stuck repetition |
//! | **decoder prefix** | the only lever *inside* this model — its graph takes a decoder prefix |
//!
//! A fifth, fine-tuning, is a project rather than a measurement: it needs PyTorch, a parallel
//! corpus in the manga/game register the app is actually read on, and a re-export to ONNX. The
//! corpus is the hard part — `corpus/` is gitignored precisely because that material is
//! copyrighted, so it cannot be the training set.
//!
//! # The instruments are the validated ones
//!
//! chrF2, BLEU and embedding cosine from `common`, each already put through the three-point check
//! in `translation_quality.rs`. **Paired sign tests against the baseline**, because every variant
//! sees the same sentences and comparing two means would throw that away.
//!
//! # This one *is* allowed to change the app
//!
//! Unlike its neighbours it is not only a measurement: a lever that wins its paired test earns a
//! change to `remembering()` or to the decode loop. One that does not is written up and dropped.

mod common;

use std::collections::BTreeSet;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::translate::{FuguMt, Knobs, Translator};

use common::{chrf2, cosine, embed, Bleu};

/// How many pairs. The same 300 the model comparison used, so the numbers sit beside each other.
const HOW_MANY: usize = 300;

// ── The input transforms ────────────────────────────────────────────────────────────────────

/// Split a passage into sentences the way the app already understands them.
///
/// `model::sentence_bounds` works on tokens and this works on text, but they agree on what ends a
/// sentence — 。！？and a newline — because that list is the app's answer to the question and
/// inventing a second one here would measure a rule the app does not hold.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if c == '\n' {
            if !current.trim().is_empty() {
                out.push(current.trim().to_string());
            }
            current.clear();
            continue;
        }
        current.push(c);
        if matches!(c, '。' | '！' | '？') {
            out.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    if out.is_empty() {
        out.push(text.trim().to_string());
    }
    out
}

/// What OCR and the web leave in Japanese text that a training corpus mostly does not.
///
/// **This mirrors `FuguMt::for_the_model`**, which is where it lives now that it won. Kept here so
/// the lever can still be measured — with the fix shipped it should score as a no-op, and that is
/// the check that the fix is really doing the work.
///
/// An earlier draft also folded em dashes onto ー. Clippy pointed out that `FF0D` is *inside* the
/// folded range and so was already unreachable, and the corpus had none of the others: the rule
/// was measuring nothing, which is why neither copy has it.
fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let mapped = match c {
            // Full-width ASCII (！ to ～) down to the ordinary range.
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            _ => c,
        };
        out.push(mapped);
    }
    // Line breaks are joined with nothing: Japanese does not separate words with spaces, so a
    // break mid-sentence from an OCR capture is not a word boundary.
    out.split('\n')
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}

/// The English subject the analysis is sure of, when it is sure of one.
///
/// Only topic-marked pronouns, and only at the head of the sentence — the narrowest case where
/// the app genuinely knows who the subject is. FuguMT's characteristic failure is inventing a
/// subject that is not there (`なあ、トム、悩みごとなんか忘れちゃって` → "I forgot about my
/// troubles"), and this cannot fix that, because nothing knows the subject when it is absent.
/// What it can do is stop the model contradicting a subject that is written down.
fn forced_subject(analyzer: &LinderaAnalyzer, japanese: &str) -> String {
    let Ok(analysis) = analyzer.analyze(japanese) else {
        return String::new();
    };
    let words: Vec<&str> = analysis
        .tokens
        .iter()
        .filter(|t| t.is_word())
        .map(|t| t.surface.as_str())
        .collect();
    // The topic marker has to be the second token, or this is not the sentence's subject.
    if words.len() < 2 || words[1] != "は" {
        return String::new();
    }
    match words[0] {
        "私" | "僕" | "俺" | "わたし" | "ぼく" => "I",
        "彼" => "He",
        "彼女" => "She",
        "あなた" | "君" | "きみ" => "You",
        "我々" | "私たち" | "僕ら" | "我々は" => "We",
        "彼ら" | "彼女ら" => "They",
        _ => "",
    }
    .to_string()
}

// ── The variants ────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Lever {
    Baseline,
    Split,
    Normalise,
    SplitAndNormalise,
    NoRepeat,
    Prefix,
    /// The OCR case, simulated. See [`with_a_line_break`].
    BrokenLine,
    /// The same, joined again before translating — which is what a fix would do.
    BrokenLineJoined,
}

/// A line break dropped into the middle of a sentence, the way a captured panel gives it.
///
/// **Synthetic, and labelled as such.** No sentence in this corpus contains a newline, so the
/// question the app actually faces — OCR hands over two lines of one sentence — cannot be asked of
/// it directly. Inserting the break is the nearest honest thing: the Japanese is untouched, only
/// its layout changes, which is exactly what a capture does to it.
fn with_a_line_break(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 6 {
        return text.to_string();
    }
    let at = chars.len() / 2;
    format!(
        "{}\n{}",
        chars[..at].iter().collect::<String>(),
        chars[at..].iter().collect::<String>()
    )
}

const LEVERS: &[(Lever, &str)] = &[
    (Lever::Baseline, "baseline (as shipped)"),
    (Lever::Split, "1. sentence at a time"),
    (Lever::Normalise, "2. normalised input"),
    (Lever::SplitAndNormalise, "1+2 together"),
    (Lever::NoRepeat, "4. no-repeat 3-gram"),
    (Lever::Prefix, "3. forced subject"),
    (Lever::BrokenLine, "OCR line break, as-is"),
    (Lever::BrokenLineJoined, "OCR line break, joined"),
];

fn run(
    lever: Lever,
    japanese: &str,
    engine: &FuguMt,
    analyzer: &LinderaAnalyzer,
) -> (String, bool) {
    let plain = Knobs::default();
    match lever {
        Lever::Baseline => (engine.translate(japanese).unwrap_or_default(), false),
        Lever::NoRepeat => {
            let knobs = Knobs {
                no_repeat: 3,
                ..Knobs::default()
            };
            (
                engine.translate_tuned(japanese, &knobs).unwrap_or_default(),
                true,
            )
        }
        Lever::Prefix => {
            let prefix = forced_subject(analyzer, japanese);
            let fired = !prefix.is_empty();
            let knobs = Knobs {
                prefix,
                ..Knobs::default()
            };
            (
                engine.translate_tuned(japanese, &knobs).unwrap_or_default(),
                fired,
            )
        }
        Lever::BrokenLine => (
            engine
                .translate_tuned(&with_a_line_break(japanese), &plain)
                .unwrap_or_default(),
            true,
        ),
        Lever::BrokenLineJoined => {
            let joined = with_a_line_break(japanese).replace('\n', "");
            (
                engine.translate_tuned(&joined, &plain).unwrap_or_default(),
                true,
            )
        }
        Lever::Split | Lever::Normalise | Lever::SplitAndNormalise => {
            let text = if matches!(lever, Lever::Normalise | Lever::SplitAndNormalise) {
                normalise(japanese)
            } else {
                japanese.to_string()
            };
            let split = matches!(lever, Lever::Split | Lever::SplitAndNormalise);
            let parts = if split {
                sentences(&text)
            } else {
                vec![text.clone()]
            };
            let fired = parts.len() > 1 || text != japanese;
            let joined = parts
                .iter()
                .filter_map(|part| engine.translate_tuned(part, &plain).ok())
                .collect::<Vec<_>>()
                .join(" ");
            (joined, fired)
        }
    }
}

#[test]
#[ignore = "needs the translation model and the imported Jitendex; use --release"]
fn the_levers_that_might_improve_fugumt_are_measured() {
    let dir = std::env::var_os("SAKATSU_MT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA"))
                .join("com.sakatsu.app")
                .join("models")
                .join("fugumt-ja-en")
        });
    if !FuguMt::is_present(&dir) {
        println!("SKIP: no translation model at {}", dir.display());
        return;
    }
    let engine = FuguMt::load(&dir).expect("load");
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");

    let pairs = common::jitendex_examples();
    let usable: Vec<&(String, String)> = pairs
        .iter()
        .filter(|(japanese, english)| {
            !japanese.is_empty() && english.split_whitespace().count() > 3
        })
        .collect();
    if usable.is_empty() {
        println!("SKIP: no corpus — run scripts/fetch-dictionaries.ps1");
        return;
    }
    let step = (usable.len() / HOW_MANY).max(1);
    let corpus: Vec<(String, String)> = usable
        .iter()
        .step_by(step)
        .take(HOW_MANY)
        .map(|(j, e)| (j.clone(), e.clone()))
        .collect();
    let humans: Vec<String> = corpus.iter().map(|(_, e)| e.clone()).collect();

    // How much of the corpus each lever can even touch. A lever that never fires cannot help, and
    // saying so is more useful than a rate indistinguishable from the baseline for that reason.
    println!("\n=== {} sentences ===", corpus.len());
    let multi = corpus
        .iter()
        .filter(|(j, _)| sentences(j).len() > 1)
        .count();
    let changed = corpus.iter().filter(|(j, _)| normalise(j) != **j).count();
    let subjects = corpus
        .iter()
        .filter(|(j, _)| !forced_subject(&analyzer, j).is_empty())
        .count();
    println!("  more than one sentence:        {multi}");
    println!("  normalisation changes them:    {changed}");
    println!("  a topic-marked subject:        {subjects}");

    let mut said: Vec<(Lever, &str, Vec<String>, usize, f64)> = Vec::new();
    for (lever, name) in LEVERS {
        let started = std::time::Instant::now();
        let mut rows = Vec::new();
        let mut fired = 0usize;
        for (japanese, _) in &corpus {
            let (text, did) = run(*lever, japanese, &engine, &analyzer);
            fired += usize::from(did);
            rows.push(text);
        }
        let secs = started.elapsed().as_secs_f64() / corpus.len() as f64;
        said.push((*lever, name, rows, fired, secs));
    }

    let human_vectors = embed(&humans);
    println!("\n=== against the human translations ===");
    println!(
        "  {:<24}{:>8}{:>8}{:>9}{:>8}{:>9}",
        "lever", "chrF2", "BLEU", "cosine", "fired", "s/sent"
    );
    let mut scored: Vec<(Lever, &str, f64, Vec<f64>)> = Vec::new();
    for (lever, name, rows, fired, secs) in &said {
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
        let per_sentence: Vec<f64> = match (&human_vectors, embed(rows)) {
            (Some(h), Some(v)) => v.iter().zip(h).map(|(x, y)| cosine(x, y)).collect(),
            _ => rows.iter().zip(&humans).map(|(c, h)| chrf2(h, c)).collect(),
        };
        let cos = per_sentence.iter().sum::<f64>() / per_sentence.len().max(1) as f64;
        println!(
            "  {:<24}{:>8.3}{:>8.1}{:>9.3}{:>8}{:>9.2}",
            name,
            chrf,
            bleu.score(),
            cos,
            fired,
            secs
        );
        scored.push((*lever, name, cos, per_sentence));
    }

    // ── Paired against the baseline ─────────────────────────────────────────────────────────
    let Some(base) = scored.iter().find(|(l, _, _, _)| *l == Lever::Baseline) else {
        return;
    };
    println!("\n=== sentence by sentence against the baseline ===");
    println!(
        "  {:<24}{:>7}{:>7}{:>8}{:>10}",
        "lever", "wins", "ties", "losses", "sign p"
    );
    for (lever, name, _, per_sentence) in &scored {
        if *lever == Lever::Baseline {
            continue;
        }
        let (mut win, mut tie, mut loss) = (0usize, 0usize, 0usize);
        for (mine, theirs) in per_sentence.iter().zip(&base.3) {
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
            name,
            sign_test(win, loss)
        );
    }

    // ── Read the ones that moved ────────────────────────────────────────────────────────────
    println!("\n=== where a lever changed the answer ===");
    let baseline_rows = &said
        .iter()
        .find(|(l, _, _, _, _)| *l == Lever::Baseline)
        .expect("baseline")
        .2;
    let mut shown = 0;
    for i in 0..corpus.len() {
        if shown >= 5 {
            break;
        }
        let differing: Vec<&(Lever, &str, Vec<String>, usize, f64)> = said
            .iter()
            .filter(|(l, _, rows, _, _)| *l != Lever::Baseline && rows[i] != baseline_rows[i])
            .collect();
        if differing.is_empty() {
            continue;
        }
        shown += 1;
        println!("\n  {}", corpus[i].0);
        println!("    human                    {}", humans[i]);
        println!("    baseline                 {}", baseline_rows[i]);
        for (_, name, rows, _, _) in differing {
            println!("    {name:<24} {}", rows[i]);
        }
    }

    let all: BTreeSet<&str> = LEVERS.iter().map(|(_, n)| *n).collect();
    assert!(!all.is_empty());
}

/// How many lines a captured passage is taken to be. A manga panel, a subtitle block or a verse
/// is a handful of lines, not one and not thirty.
const LINES_PER_PASSAGE: usize = 4;

/// Terminators the app itself treats as ending a sentence — `model::ends_sentence`, minus the
/// newline, which is the thing under test here.
const ENDS: [char; 5] = ['。', '！', '？', '!', '?'];

/// [`marked`], but leaving a break alone where the parser says the line was wrapped.
///
/// **The shipped function**, not a copy of it. This lever earned a change to the app, so measuring
/// a private reimplementation of it would measure the wrong thing the first time the two drifted.
fn marked_guarded(lines: &[String]) -> String {
    sakatsu_core::translate::mark_boundaries(&lines.join(
        "
",
    ))
}

/// A capture's lines, welded the way `FuguMt::for_the_model` welds them today.
fn welded(lines: &[String]) -> String {
    lines.join("")
}

/// The same lines with the boundary the app already believes in written where the model can see
/// it. Nothing is rewritten: a terminator is added only where the line does not already end in one.
fn marked(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| {
            let line = line.trim();
            if line.ends_with(ENDS) {
                line.to_string()
            } else {
                format!("{line}。")
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

/// What a capture of punctuation-free text looks like — lyrics, subtitles, a lot of game script.
///
/// **Synthetic, and labelled as such**, in the same spirit as [`with_a_line_break`]: every sentence
/// in this corpus is a dictionary example and ends in `。`, so the question the app actually
/// faces cannot be asked of it directly. Dropping the terminator is the nearest honest thing — the
/// words are untouched and only the punctuation the reader never captured is absent. It matters
/// because with the terminators present [`welded`] and [`marked`] are the same string, and the
/// lever would measure nothing.
fn unpunctuated(line: &str) -> String {
    line.trim().trim_end_matches(ENDS).trim().to_string()
}

/// Score a set of answers against their references, returning the per-item series the sign test
/// needs alongside the three summary numbers.
fn score(candidates: &[String], references: &[String]) -> (f64, f64, f64, Vec<f64>) {
    let chrf = candidates
        .iter()
        .zip(references)
        .map(|(c, h)| chrf2(h, c))
        .sum::<f64>()
        / references.len().max(1) as f64;
    let mut bleu = Bleu::default();
    for (c, h) in candidates.iter().zip(references) {
        bleu.add(h, c);
    }
    let per_item: Vec<f64> = match (embed(references), embed(candidates)) {
        (Some(h), Some(v)) => v.iter().zip(&h).map(|(x, y)| cosine(x, y)).collect(),
        _ => candidates
            .iter()
            .zip(references)
            .map(|(c, h)| chrf2(h, c))
            .collect(),
    };
    let cos = per_item.iter().sum::<f64>() / per_item.len().max(1) as f64;
    (chrf, bleu.score(), cos, per_item)
}

fn paired(name: &str, mine: &[f64], base: &[f64]) {
    let (mut win, mut tie, mut loss) = (0usize, 0usize, 0usize);
    for (a, b) in mine.iter().zip(base) {
        if (a - b).abs() < 0.01 {
            tie += 1;
        } else if a > b {
            win += 1;
        } else {
            loss += 1;
        }
    }
    println!(
        "  {name:<28}{win:>7}{tie:>7}{loss:>8}{:>10}",
        sign_test(win, loss)
    );
}

/// **What happens to a passage, which is the only thing `translate` is ever given.**
///
/// `FuguMt::for_the_model` deletes every line break before the encoder sees the text, and
/// `project notes` §7 records why: a break dropped into the middle of a sentence cost more than the
/// entire gap to a 7B instruct model, and joining the lines back recovered it exactly.
///
/// That measurement was taken on single sentences with a break inserted synthetically, because
/// "no sentence in this corpus contains a newline". The shipped app never asks it that question:
/// `sentence_ranges` treats a newline as a terminator, so the per-sentence paths
/// (`translate_sentences`, `translate_variants`) can never hand the model a line break at all. The
/// whole-passage `translate` command is the single caller that can, and there the lines are
/// usually *separate sentences* — welding them is not recovery, it is running eight utterances
/// together. The reported symptom was one word: "schoolhousethat".
///
/// So this asks the question the other test could not, on passages rather than sentences, and adds
/// the regression check the change actually risks: does marking a boundary damage a sentence that
/// really was wrapped?
///
/// ```text
/// cargo test --release -p sakatsu-core --test fugumt_levers passages -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs the translation model and the imported Jitendex; use --release"]
fn a_captured_passage_is_several_sentences_and_the_model_should_be_told_so() {
    let dir = std::env::var_os("SAKATSU_MT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA"))
                .join("com.sakatsu.app")
                .join("models")
                .join("fugumt-ja-en")
        });
    if !FuguMt::is_present(&dir) {
        println!("SKIP: no translation model at {}", dir.display());
        return;
    }
    let engine = FuguMt::load(&dir).expect("load");
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");

    let pairs = common::jitendex_examples();
    let usable: Vec<&(String, String)> = pairs
        .iter()
        .filter(|(japanese, english)| {
            !japanese.is_empty() && english.split_whitespace().count() > 3
        })
        .collect();
    if usable.is_empty() {
        println!("SKIP: no corpus — run scripts/fetch-dictionaries.ps1");
        return;
    }
    let step = (usable.len() / HOW_MANY).max(1);
    let corpus: Vec<(String, String)> = usable
        .iter()
        .step_by(step)
        .take(HOW_MANY)
        .map(|(j, e)| (j.clone(), e.clone()))
        .collect();

    // Consecutive sentences grouped into passages, which is what a capture is. The reference is
    // what those lines mean, in order.
    let passages: Vec<(Vec<String>, String)> = corpus
        .chunks(LINES_PER_PASSAGE)
        .filter(|chunk| chunk.len() == LINES_PER_PASSAGE)
        .map(|chunk| {
            (
                chunk.iter().map(|(j, _)| j.clone()).collect(),
                chunk
                    .iter()
                    .map(|(_, e)| e.trim())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        })
        .collect();
    let references: Vec<String> = passages.iter().map(|(_, e)| e.clone()).collect();
    println!(
        "\n=== {} passages of {LINES_PER_PASSAGE} lines ===",
        passages.len()
    );

    // **What the aligned view actually does**, not a model of it. It used to split the lines by
    // hand, on the belief that `sentence_ranges` treats a newline as a terminator. It did not —
    // the arm was dead — so that row was the number for a path the app never took. This analyses
    // the passage, partitions it with the app's own rule, and translates each range's token
    // surfaces, which is `commands::translate_sentences` line for line.
    let aligned = |lines: &[String]| {
        let Ok(analysis) = analyzer.analyze(&lines.join(
            "
",
        )) else {
            return String::new();
        };
        sakatsu_core::model::sentence_ranges(&analysis.tokens)
            .into_iter()
            .filter_map(|range| {
                let japanese: String = analysis.tokens[range.first_token..range.last_token_end]
                    .iter()
                    .map(|token| token.surface.as_str())
                    .collect();
                engine.translate(&japanese).ok()
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    for (label, strip) in [
        ("as captured, with punctuation", false),
        ("punctuation absent", true),
    ] {
        let shaped: Vec<Vec<String>> = passages
            .iter()
            .map(|(lines, _)| {
                lines
                    .iter()
                    .map(|line| {
                        if strip {
                            unpunctuated(line)
                        } else {
                            line.clone()
                        }
                    })
                    .collect()
            })
            .collect();

        let welded_rows: Vec<String> = shaped
            .iter()
            .map(|lines| engine.translate(&welded(lines)).unwrap_or_default())
            .collect();
        let marked_rows: Vec<String> = shaped
            .iter()
            .map(|lines| engine.translate(&marked(lines)).unwrap_or_default())
            .collect();
        let guarded_rows: Vec<String> = shaped
            .iter()
            .map(|lines| engine.translate(&marked_guarded(lines)).unwrap_or_default())
            .collect();
        let each_rows: Vec<String> = shaped.iter().map(|lines| aligned(lines)).collect();

        let differ = shaped
            .iter()
            .filter(|lines| welded(lines) != marked(lines))
            .count();
        println!("\n--- {label} ---");
        println!(
            "  passages where marking changes the input: {differ} of {}",
            shaped.len()
        );
        println!(
            "  {:<28}{:>8}{:>8}{:>9}",
            "variant", "chrF2", "BLEU", "cosine"
        );
        let mut series = Vec::new();
        for (name, rows) in [
            ("welded (what ships)", &welded_rows),
            ("boundaries marked", &marked_rows),
            ("marked, wrap-guarded", &guarded_rows),
            ("the aligned view, for real", &each_rows),
        ] {
            let (chrf, bleu, cos, per_item) = score(rows, &references);
            println!("  {name:<28}{chrf:>8.3}{bleu:>8.1}{cos:>9.3}");
            series.push((name, per_item));
        }

        println!(
            "\n  {:<28}{:>7}{:>7}{:>8}{:>10}",
            "against welded", "wins", "ties", "losses", "sign p"
        );
        let base = series[0].1.clone();
        for (name, per_item) in series.iter().skip(1) {
            paired(name, per_item, &base);
        }
    }

    // ── The regression this change actually risks ───────────────────────────────────────────
    //
    // A sentence that really was wrapped by OCR. Marking a boundary there writes a full stop into
    // the middle of a clause, which is the harm the original lever measured from the other side.
    println!("\n=== a single sentence, wrapped by OCR ===");
    let wrapped: Vec<Vec<String>> = corpus
        .iter()
        .map(|(j, _)| {
            with_a_line_break(j)
                .split('\n')
                .map(str::to_string)
                .collect()
        })
        .filter(|lines: &Vec<String>| lines.len() == 2)
        .collect();
    let wrapped_refs: Vec<String> = corpus
        .iter()
        .zip(&wrapped)
        .map(|((_, e), _)| e.clone())
        .take(wrapped.len())
        .collect();
    println!("  {} wrapped sentences", wrapped.len());
    println!(
        "  {:<28}{:>8}{:>8}{:>9}",
        "variant", "chrF2", "BLEU", "cosine"
    );
    let welded_wrapped: Vec<String> = wrapped
        .iter()
        .map(|lines| engine.translate(&welded(lines)).unwrap_or_default())
        .collect();
    let marked_wrapped: Vec<String> = wrapped
        .iter()
        .map(|lines| engine.translate(&marked(lines)).unwrap_or_default())
        .collect();
    // The third variant, so the table has no empty cell: what the *aligned* view already does to a
    // wrapped capture, because `sentence_ranges` treats every newline as a terminator.
    let each_wrapped: Vec<String> = wrapped.iter().map(|lines| aligned(lines)).collect();
    let guarded_wrapped: Vec<String> = wrapped
        .iter()
        .map(|lines| engine.translate(&marked_guarded(lines)).unwrap_or_default())
        .collect();
    let (c4, b4, o4, s4) = score(&guarded_wrapped, &wrapped_refs);
    let (c1, b1, o1, s1) = score(&welded_wrapped, &wrapped_refs);
    let (c2, b2, o2, s2) = score(&marked_wrapped, &wrapped_refs);
    let (c3, b3, o3, s3) = score(&each_wrapped, &wrapped_refs);
    println!("  {:<28}{c1:>8.3}{b1:>8.1}{o1:>9.3}", "welded (what ships)");
    println!("  {:<28}{c2:>8.3}{b2:>8.1}{o2:>9.3}", "boundaries marked");
    println!(
        "\n  {:<28}{:>7}{:>7}{:>8}{:>10}",
        "against welded", "wins", "ties", "losses", "sign p"
    );
    println!(
        "  {:<28}{c4:>8.3}{b4:>8.1}{o4:>9.3}",
        "marked, wrap-guarded"
    );
    println!(
        "  {:<28}{c3:>8.3}{b3:>8.1}{o3:>9.3}",
        "the aligned view, for real"
    );
    paired("boundaries marked", &s2, &s1);
    paired("marked, wrap-guarded", &s4, &s1);
    paired("the aligned view, for real", &s3, &s1);

    assert!(!passages.is_empty(), "the corpus must yield passages");
}

/// Two-sided sign test — could this many wins be a coin?
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

fn binomial(n: usize, k: usize) -> f64 {
    let mut log = -(n as f64) * 2.0f64.ln();
    for i in 0..k {
        log += ((n - i) as f64).ln() - ((i + 1) as f64).ln();
    }
    log.exp()
}
