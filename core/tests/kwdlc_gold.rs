//! Three granularities at once, against gold that does **not** come from UniDic.
//!
//! ```text
//! pwsh -File scripts/fetch-kwdlc.ps1
//! cargo test -p sakatsu-core --test kwdlc_gold -- --ignored --nocapture
//! ```
//!
//! # Why this exists beside `ud_gold.rs`
//!
//! UD_Japanese-GSD cannot settle the comparison it is used for. Its lemmas and POS are
//! UniDic-derived and its short-unit-word standard *is* UniDic's, so scoring the built-in
//! analyzer (UniDic) against Ichiran (JMdict) on it asks which of them agrees with UniDic.
//! Measured, it answers exactly as that framing predicts: SUW F1 77.7 for the built-in analyzer
//! against 63.1 for Ichiran, a gap too large to believe.
//!
//! KWDLC is annotated in the **JUMAN** lineage — a third dictionary family, related to neither —
//! so it can hold the coats. It also carries a reading per morpheme, which makes it the only
//! source here able to judge 私 わたし / わたくし; GSD's readings are UniDic's own answer and
//! cannot arbitrate a dispute about UniDic.
//!
//! # The three granularities are the point
//!
//! Japanese has no single correct word boundary, which is why SUW and LUW exist as separate
//! annotation standards. KNP marks three levels on the same text and this scores against all of
//! them, because "which is the built-in analyzer closest to" is a different and more useful
//! question than "how far is it from the one we picked":
//!
//! | level | marker | what it is | 本を読んでいる |
//! |---|---|---|---|
//! | morpheme | none | JUMAN's units, finest | 本 \| を \| 読んで \| いる |
//! | 基本句 | `+` | content word plus its particles | 本を \| 読んでいる |
//! | 文節 | `*` | the phrase a reader chunks by | 本を \| 読んでいる |
//!
//! A learner-facing token is unlikely to be the finest level and unlikely to be the coarsest.
//! Nothing here decides which it should be — that is a design decision recorded in `project notes`,
//! not a number — but it prints what each choice would cost.
//!
//! # Licence
//!
//! KWDLC declares none. `project notes` §8 therefore treats it as consult-never-import, the standing
//! it gives Imabi and Tae Kim: read to measure, never copied into anything that ships. This file
//! reads it and prints statistics; no surface, reading or boundary from it is retained.

mod common;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

use common::knp::{self, level, Morpheme, Sentence};

use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::IchiranAnalyzer;

fn corpus_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_KWDLC") {
        return PathBuf::from(dir);
    }
    common::dictionaries_dir()
        .parent()
        .map(|parent| parent.join("corpora").join("kwdlc"))
        .unwrap_or_default()
}

#[derive(Default)]
struct Level {
    gold: usize,
    matched: usize,
}

#[derive(Default)]
struct Tally {
    sentences: usize,
    produced: usize,
    morpheme: Level,
    kihonku: Level,
    bunsetsu: Level,
    reading_checked: usize,
    reading_agreed: usize,
    /// Inflected morphemes the analyzer aligned with, and whether it called them inflected too.
    ///
    /// Deliberately coarse, and no longer the only thing scored. KWDLC names 活用形 in JUMAN's
    /// vocabulary (タ形, 基本形) and this app names forms in its own (Past (~ta), Non-past); this
    /// tally asks only the question both can answer without a translation table — **is this word
    /// inflected away from its dictionary form, or not.**
    ///
    /// The translation table this file once declined to write now exists as `accepted_labels`,
    /// and `conjugation_correctness_against_juman` scores against it. This stays because it is the
    /// weaker claim that needs no mapping at all: if it ever disagrees with the finer score, the
    /// mapping is what to suspect.
    /// Getting that wrong is a real fault — it is the difference between 読んだ and 読む — and
    /// getting it right is the precondition for any finer comparison.
    inflection_checked: usize,
    inflection_agreed: usize,
}

#[test]
#[ignore = "needs scripts/fetch-kwdlc.ps1"]
fn the_analyzers_against_three_granularities() {
    let dir = corpus_dir().join("knp");
    let (files, _) = knp::collect(&dir);
    if files.is_empty() {
        println!(
            "SKIP: no corpus in {} — run scripts/fetch-kwdlc.ps1, or set SAKATSU_KWDLC",
            dir.display()
        );
        return;
    }

    // Capped by default: 16,051 sentences is more than any of this needs to be stable, and the
    // sidecar costs about 0.9 s each. Stride, so the sample stays spread across the genres the
    // corpus is ordered by rather than taking every document from the front.
    let limit = std::env::var("SAKATSU_KWDLC_SAMPLE")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(2000);

    // **A damaged corpus is not a smaller corpus.** `load` refuses rather than handing back what
    // it managed to parse, because a sentence missing a morpheme is scored as shortened text with
    // every later offset in it shifted — a measurement of something nobody analysed, finishing
    // green on a denominator quietly one smaller.
    let sentences: Vec<Sentence> = match knp::load(&dir) {
        Ok(sentences) => sentences,
        Err(damaged) => panic!("{damaged}"),
    };
    let stride = sentences.len().div_ceil(limit.max(1)).max(1);
    let sampled: Vec<&Sentence> = sentences.iter().step_by(stride).collect();
    println!(
        "{} sentences in {} files, taking every {stride} — {} to score",
        sentences.len(),
        files.len(),
        sampled.len()
    );

    measure(
        "built-in (lindera)",
        &LinderaAnalyzer::new().expect("compiled in"),
        &sampled,
    );

    let ichiran = IchiranAnalyzer::new();
    if ichiran.analyze("テスト").is_err() {
        println!("\nSKIP: Ichiran did not answer — it needs WSL and its PostgreSQL database.");
        return;
    }
    let ichiran_limit = std::env::var("SAKATSU_KWDLC_ICHIRAN")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(300)
        .min(sampled.len());
    measure("ichiran", &ichiran, &sampled[..ichiran_limit]);
    measure(
        "built-in, same subset",
        &LinderaAnalyzer::new().expect("compiled in"),
        &sampled[..ichiran_limit],
    );
    report_confidence_signal(
        &ichiran,
        &LinderaAnalyzer::new().expect("compiled in"),
        &sampled[..ichiran_limit],
    );
}

/// **Does Ichiran's score predict where the built-in analyzer is wrong?**
///
/// `Token::confidence` is Ichiran's per-word score and is `None` under the built-in analyzer,
/// which reports nothing comparable. That asymmetry is the one structural advantage the sidecar
/// has left: it can say when it is unsure and the built-in analyzer cannot, which forecloses
/// every strategy that depends on abstaining.
///
/// The tempting move is to harvest those scores offline and ship them as a doubt table. Before
/// building that, this asks whether the score carries the signal at all: bucket Ichiran's tokens
/// by score, and at each bucket measure how often the *built-in* analyzer's reading matches gold
/// on the same span. If low-scoring spans are where it goes wrong, the signal is real and worth
/// carrying. If the buckets are flat, there is nothing to harvest and the idea dies here for the
/// price of one run.
fn report_confidence_signal(
    ichiran: &dyn Analyzer,
    lindera: &dyn Analyzer,
    sentences: &[&Sentence],
) {
    // Buckets, not a correlation: the scores are an internal scale with no units, so the only
    // honest question is whether the low end differs from the high end.
    let mut buckets: [(usize, usize); 4] = [(0, 0); 4];
    let bucket_of = |score: i32| match score {
        i32::MIN..=15 => 0,
        16..=50 => 1,
        51..=150 => 2,
        _ => 3,
    };

    // **Counted, because this table is not reproducible without it.** Two runs of this harness
    // over an identical sentence set produced buckets differing by nine items while every score
    // table above matched to the digit: the sidecar's answers had changed, not the corpus. A
    // silent `continue` made a measurement of a different size each time look stable.
    let mut unanswered = 0usize;
    for sentence in sentences {
        let (Ok(theirs), Ok(ours)) = (
            ichiran.analyze(&sentence.text),
            lindera.analyze(&sentence.text),
        ) else {
            unanswered += 1;
            continue;
        };
        let gold: BTreeMap<(usize, usize), &str> = sentence
            .morphemes
            .iter()
            .map(|m| ((m.start, m.end), m.reading.as_str()))
            .collect();
        let mine: BTreeMap<(usize, usize), &str> = ours
            .tokens
            .iter()
            .filter_map(|t| Some(((t.start?, t.end?), t.reading.as_str())))
            .collect();

        for token in &theirs.tokens {
            let (Some(start), Some(end), Some(score)) = (token.start, token.end, token.confidence)
            else {
                continue;
            };
            // Only where all three have an opinion about the same span, or the comparison is
            // about alignment rather than about the reading.
            let (Some(want), Some(got)) = (gold.get(&(start, end)), mine.get(&(start, end))) else {
                continue;
            };
            if want.is_empty() || got.is_empty() {
                continue;
            }
            let slot = &mut buckets[bucket_of(score)];
            slot.1 += 1;
            if to_hiragana(want) == to_hiragana(got) {
                slot.0 += 1;
            }
        }
    }

    println!("\n=== does Ichiran's score predict the built-in analyzer's errors? ===");
    if unanswered > 0 {
        println!(
            "  {unanswered} of {} sentences went unanswered by one engine and are not counted below",
            sentences.len()
        );
    }
    println!("  ichiran score     built-in agrees with gold");
    for (label, (agreed, seen)) in [
        ("      <= 15", buckets[0]),
        ("   16 -  50", buckets[1]),
        ("   51 - 150", buckets[2]),
        ("      > 150", buckets[3]),
    ] {
        println!(
            "  {label}   {agreed:>6} / {seen:<6}  {:>6.1}%",
            percent(agreed, seen)
        );
    }
    println!(
        "  Flat buckets mean the score says nothing about where the built-in analyzer fails, and\n  \
         a doubt table harvested from it would carry noise."
    );
}

fn measure(name: &str, analyzer: &dyn Analyzer, sentences: &[&Sentence]) {
    let mut tally = Tally::default();
    // Which gold phrases the analyzer splits, for reading by hand.
    let mut split: BTreeMap<String, usize> = BTreeMap::new();
    // Readings the two disagree on, keyed surface → (gold, analyzer).
    let mut readings: BTreeMap<(String, String, String), usize> = BTreeMap::new();

    for sentence in sentences {
        let Ok(analysis) = analyzer.analyze(&sentence.text) else {
            continue;
        };
        tally.sentences += 1;

        let morpheme: BTreeSet<(usize, usize)> =
            knp::spans_of(&sentence.morphemes, level::morpheme)
                .into_iter()
                .collect();
        let kihonku: BTreeSet<(usize, usize)> = knp::spans_of(&sentence.morphemes, level::kihonku)
            .into_iter()
            .collect();
        let bunsetsu: BTreeSet<(usize, usize)> =
            knp::spans_of(&sentence.morphemes, level::bunsetsu)
                .into_iter()
                .collect();
        let reading_at: BTreeMap<(usize, usize), &str> = sentence
            .morphemes
            .iter()
            .map(|m| ((m.start, m.end), m.reading.as_str()))
            .collect();
        let at: BTreeMap<(usize, usize), &Morpheme> = sentence
            .morphemes
            .iter()
            .map(|m| ((m.start, m.end), m))
            .collect();

        tally.morpheme.gold += morpheme.len();
        tally.kihonku.gold += kihonku.len();
        tally.bunsetsu.gold += bunsetsu.len();

        for token in &analysis.tokens {
            let (Some(start), Some(end)) = (token.start, token.end) else {
                continue;
            };
            tally.produced += 1;
            if morpheme.contains(&(start, end)) {
                tally.morpheme.matched += 1;
                // **The reading question GSD cannot answer.** Only where the spans agree
                // exactly, so this compares two accounts of the same word.
                if let Some(gold) = reading_at.get(&(start, end)) {
                    if !token.reading.is_empty() && !gold.is_empty() {
                        tally.reading_checked += 1;
                        if to_hiragana(gold) == to_hiragana(&token.reading) {
                            tally.reading_agreed += 1;
                        } else {
                            *readings
                                .entry((
                                    token.surface.clone(),
                                    (*gold).to_string(),
                                    token.reading.clone(),
                                ))
                                .or_default() += 1;
                        }
                    }
                }
            }
            if let Some(morpheme) = at.get(&(start, end)) {
                // 基本形 is the dictionary form; anything else is inflected away from it.
                let gold_inflected =
                    !morpheme.conjugation_form.is_empty() && morpheme.conjugation_form != "基本形";
                let ours_inflected = token.conjugation.is_some()
                    || (!token.dict_form.is_empty() && token.dict_form != token.surface);
                tally.inflection_checked += 1;
                if gold_inflected == ours_inflected {
                    tally.inflection_agreed += 1;
                }
            }
            if kihonku.contains(&(start, end)) {
                tally.kihonku.matched += 1;
            }
            if bunsetsu.contains(&(start, end)) {
                tally.bunsetsu.matched += 1;
            }
        }

        for &(start, end) in &kihonku {
            let pieces = analysis
                .tokens
                .iter()
                .filter(|token| {
                    token.start.is_some_and(|s| s >= start) && token.end.is_some_and(|e| e <= end)
                })
                .count();
            if pieces > 1 {
                if let Some(surface) = sentence.text.get(start..end) {
                    *split.entry(surface.to_string()).or_default() += 1;
                }
            }
        }
    }

    report(name, &tally, &split, &readings);
}

/// Katakana to hiragana, so コーヒー and こーひー are not a disagreement.
fn to_hiragana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            other => other,
        })
        .collect()
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    100.0 * part as f64 / whole as f64
}

fn report(
    name: &str,
    tally: &Tally,
    split: &BTreeMap<String, usize>,
    readings: &BTreeMap<(String, String, String), usize>,
) {
    println!("\n=== {name}, over {} sentences ===", tally.sentences);
    println!("  produced {} tokens", tally.produced);
    println!("\n  level        gold   matched        P        R       F1");
    for (label, level) in [
        ("morpheme", &tally.morpheme),
        ("基本句", &tally.kihonku),
        ("文節", &tally.bunsetsu),
    ] {
        let precision = percent(level.matched, tally.produced);
        let recall = percent(level.matched, level.gold);
        let f1 = if precision + recall == 0.0 {
            0.0
        } else {
            2.0 * precision * recall / (precision + recall)
        };
        println!(
            "  {label:<10} {:>6}    {:>6}  {precision:>6.1}%  {recall:>6.1}%  {f1:>6.1}",
            level.gold, level.matched
        );
    }
    println!(
        "  the level with the highest F1 is the one this analyzer already targets. Which level a\n  \
         *learner* should be shown is a design decision, not this number."
    );

    println!(
        "\n  reading, where the span matched a morpheme exactly: {} / {} ({:.1}%)",
        tally.reading_agreed,
        tally.reading_checked,
        percent(tally.reading_agreed, tally.reading_checked)
    );
    println!(
        "  inflected or not, on those same spans:              {} / {} ({:.1}%)",
        tally.inflection_agreed,
        tally.inflection_checked,
        percent(tally.inflection_agreed, tally.inflection_checked)
    );
    let mut rows: Vec<_> = readings.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for ((surface, gold, got), count) in rows.iter().take(12) {
        println!("  {count:>5}x  {surface}: {gold} (gold) / {got}");
    }

    println!("\n  gold 基本句 the analyzer splits, most frequent first");
    let mut rows: Vec<_> = split.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (surface, count) in rows.iter().take(15) {
        println!("  {count:>5}x  {surface}");
    }
}

/// What does the analyzer call each JUMAN form? A co-occurrence table, not a score.
///
/// ```text
/// cargo test --release -p sakatsu-core --test kwdlc_gold -- --ignored --nocapture form_vocabularies
/// ```
///
/// # Why this comes before any conjugation score
///
/// Scoring the form chain needs a mapping from JUMAN's 活用形 onto this app's labels, and the
/// header above records why writing one has been declined: a mapping I author, scored against an
/// analyzer whose labels I also author, tests my consistency rather than the Japanese.
///
/// **KWDLC weakens that objection in a way UD_Japanese-GSD cannot.** GSD's morphology is UniDic's,
/// so a GSD mapping restates the analyzer's own source. JUMAN is a separate tradition, so a
/// JUMAN→label mapping is a translation between two independent vocabularies — it can catch an
/// error that originates in UniDic's tagging, not only in this project's composition.
///
/// It is still a mapping somebody has to write, so this prints the evidence for writing it rather
/// than assuming it. Every aligned token contributes the gold form and what the analyzer called
/// it; a mapping that the data does not support will be visible as a row that splits several ways.
///
/// # Alignment
///
/// KWDLC morphemes are JUMAN short units and this app's tokens are composed, so 読んだ is one
/// token here and 読ん | だ there. A token is aligned when gold morphemes **tile its span
/// exactly**, and the form compared is the *last* morpheme's — JUMAN carries the inflection on the
/// final auxiliary, which is what 読んだ being タ形 means.
///
/// Both `props` orderings are reported. `project notes` §5 sorts a derivation deepest-first and this
/// file does not assume which end that puts the final inflection at; printing the outermost and
/// the any-match separately is what will say.
#[test]
#[ignore = "needs scripts/fetch-kwdlc.ps1"]
fn form_vocabularies_seen_together() {
    let dir = corpus_dir().join("knp");
    let (files, _) = knp::collect(&dir);
    if files.is_empty() {
        println!(
            "SKIP: no corpus in {} — run scripts/fetch-kwdlc.ps1",
            dir.display()
        );
        return;
    }

    // **A damaged corpus is not a smaller corpus.** `load` refuses rather than handing back what
    // it managed to parse, because a sentence missing a morpheme is scored as shortened text with
    // every later offset in it shifted — a measurement of something nobody analysed, finishing
    // green on a denominator quietly one smaller.
    let sentences: Vec<Sentence> = match knp::load(&dir) {
        Ok(sentences) => sentences,
        Err(damaged) => panic!("{damaged}"),
    };
    let limit = std::env::var("SAKATSU_KWDLC_SAMPLE")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(4000);
    let stride = sentences.len().div_ceil(limit.max(1)).max(1);
    let sampled: Vec<&Sentence> = sentences.iter().step_by(stride).collect();
    println!(
        "{} sentences, taking every {stride} — {} to read",
        sentences.len(),
        sampled.len()
    );

    let analyzer = LinderaAnalyzer::new().expect("compiled in");
    // gold 活用形 -> what the analyzer called it -> how often.
    let mut table: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let (mut tokens, mut aligned) = (0usize, 0usize);

    for sentence in &sampled {
        let Ok(analysis) = analyzer.analyze(&sentence.text) else {
            continue;
        };
        for token in &analysis.tokens {
            let (Some(start), Some(end)) = (token.start, token.end) else {
                continue;
            };
            tokens += 1;
            // The gold morphemes inside this token, in order.
            let inside: Vec<&Morpheme> = sentence
                .morphemes
                .iter()
                .filter(|m| m.start >= start && m.end <= end)
                .collect();
            // Tiling exactly: first starts where we start, last ends where we end, no gaps.
            let tiles = inside.first().is_some_and(|m| m.start == start)
                && inside.last().is_some_and(|m| m.end == end)
                && inside.windows(2).all(|pair| pair[0].end == pair[1].start);
            if !tiles {
                continue;
            }
            aligned += 1;

            // JUMAN carries the inflection on the final morpheme of the run.
            let gold = inside
                .last()
                .map(|m| m.conjugation_form.as_str())
                .unwrap_or("");
            let gold = if gold.is_empty() { "(none)" } else { gold };

            // **A composed token with no conjugation is a different thing from a bare one.**
            // について is a lexicalised particle and ので a conjunction; `fixed_expression` gives
            // both the whole as their dictionary form precisely so a reader is not sent to 付く,
            // and `compound` builds them with no derivation on purpose. Gold still labels the
            // final morpheme タ系連用テ形, so they land here looking like silence about a te-form.
            // Splitting the two says how much of that is a deliberate convention and how much is
            // the analyzer failing to say anything about a word it did inflect.
            let ours = match token.conjugation.as_ref() {
                None if !token.components.is_empty() => "(none, composed)".to_string(),
                None => "(none, simple)".to_string(),
                Some(conjugation) if conjugation.props.is_empty() => "(empty chain)".to_string(),
                Some(conjugation) => conjugation
                    .props
                    .iter()
                    .map(|prop| prop.form.as_str())
                    .collect::<Vec<_>>()
                    .join("+"),
            };
            *table
                .entry(gold.to_string())
                .or_default()
                .entry(ours)
                .or_default() += 1;
        }
    }

    println!(
        "\n{aligned} of {tokens} tokens align with a whole run of gold morphemes ({:.1}%)",
        percent(aligned, tokens)
    );
    println!("\n=== JUMAN 活用形 -> what this analyzer called it ===");
    println!("  a form whose row concentrates on one label is one a mapping can be written for;");
    println!("  a row that splits several ways is where the mapping would be a guess.\n");

    let mut rows: Vec<_> = table.iter().collect();
    rows.sort_by_key(|(_, seen)| std::cmp::Reverse(seen.values().sum::<usize>()));
    for (gold, seen) in rows.iter().take(24) {
        let total: usize = seen.values().sum();
        let mut calls: Vec<_> = seen.iter().collect();
        calls.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        let top: Vec<String> = calls
            .iter()
            .take(3)
            .map(|(label, count)| format!("{label} {count} ({:.0}%)", percent(**count, total)))
            .collect();
        println!("  {gold:<16} {total:>6}   {}", top.join("   |   "));
    }
}

/// JUMAN's 活用形 translated into this app's labels — the accepted answers for each.
///
/// **Every row is what the co-occurrence table showed, not what I expected.**
/// `form_vocabularies_seen_together` prints that table and this list was written from it: a form
/// whose row split several ways got several accepted answers, and a form with no row got none.
///
/// Two rows accept more than one label for a reason rather than for slack:
///
///   基本連用形       Continuative *and* Adverbial. UniDic files 読み and 高く both as 連用形 and
///                  `normalize_form` separates them by part of speech on purpose — 高く is the
///                  adverbial form of an adjective, and calling it continuative told a learner it
///                  linked clauses. JUMAN does not make that split, so both are right here.
///   ダ列タ系連用テ形   Te-form *and* Continuative. This is the で of だ, which the analyzer reaches
///                  by two routes, and the table shows both.
///
/// `None` means the form is not mapped and is **reported rather than scored** — the same
/// discipline `ud_gold.rs` applies to the auxiliaries it declines to map. Guessing here would be
/// inventing gold.
fn accepted_labels(gold: &str) -> Option<&'static [&'static str]> {
    Some(match gold {
        "基本形"
        | "デス列基本形"
        | "デアル列基本形"
        | "ダ列基本連体形"
        | "音便基本形"
        | "文語基本形"
        | "文語連体形" => &["Non-past"],
        "タ形" | "ダ列タ形" | "デス列タ形" | "デアル列タ形" => &["Past (~ta)"],
        "基本連用形" | "ダ列基本連用形" | "デアル列基本連用形" | "未然形" => {
            &["Continuative", "Adverbial"]
        }
        "タ系連用テ形" | "タ系連用チャ形" => &["Conjunctive (~te)"],
        "ダ列タ系連用テ形" | "ダ列タ系連用ジャ形" => {
            &["Conjunctive (~te)", "Continuative"]
        }
        "命令形" => &["Imperative"],
        "基本条件形" | "デアル列基本条件形" => &["Provisional (~eba)"],
        "タ系条件形" => &["Conditional (~tara)"],
        "意志形" | "デス列基本推量形" | "ダ列基本推量形" => &["Volitional"],
        "タ系連用タリ形" => &["Alternative (~tari)"],
        _ => return None,
    })
}

/// Conjugation **correctness**, for both analyzers, against a reference neither of them wrote.
///
/// ```text
/// cargo test --release -p sakatsu-core --test kwdlc_gold -- --ignored --nocapture conjugation_correctness
/// ```
///
/// # Why this can exist here and not in `ud_gold.rs`
///
/// That file declines to score the form chain because UD_Japanese-GSD's morphology is UniDic's —
/// the analyzer's own source — so a mapping written onto it would test my consistency rather than
/// the Japanese. **KWDLC is JUMAN-lineage**, a separate tradition, so [`accepted_labels`] is a
/// translation between two independent vocabularies and can catch an error that starts in UniDic's
/// tagging rather than only in this project's composition.
///
/// It is still a mapping somebody wrote. What keeps that honest is that it was written from the
/// measured co-occurrence table rather than from expectation, and that unmapped forms are counted
/// and named instead of scored.
///
/// # Composed tokens are excluded, and that is not a convenience
///
/// Measured: **every** silent case on an inflected gold form is a *composed* token and not one is
/// a simple one. について is a lexicalised particle and ので a conjunction; `fixed_expression`
/// gives them the whole as their dictionary form so a reader is not sent to 付く, and `compound`
/// builds them with no derivation deliberately. Gold still labels their final morpheme
/// タ系連用テ形. Scoring those would mark the analyzer wrong for a convention it holds on purpose,
/// so they are counted separately and reported.
#[test]
#[ignore = "needs scripts/fetch-kwdlc.ps1"]
fn conjugation_correctness_against_juman() {
    let dir = corpus_dir().join("knp");
    let (files, _) = knp::collect(&dir);
    if files.is_empty() {
        println!(
            "SKIP: no corpus in {} — run scripts/fetch-kwdlc.ps1",
            dir.display()
        );
        return;
    }
    // **A damaged corpus is not a smaller corpus.** `load` refuses rather than handing back what
    // it managed to parse, because a sentence missing a morpheme is scored as shortened text with
    // every later offset in it shifted — a measurement of something nobody analysed, finishing
    // green on a denominator quietly one smaller.
    let sentences: Vec<Sentence> = match knp::load(&dir) {
        Ok(sentences) => sentences,
        Err(damaged) => panic!("{damaged}"),
    };
    let limit = std::env::var("SAKATSU_KWDLC_SAMPLE")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(4000);
    let stride = sentences.len().div_ceil(limit.max(1)).max(1);
    let sampled: Vec<&Sentence> = sentences.iter().step_by(stride).collect();
    println!(
        "{} sentences, every {stride} — {} to score",
        sentences.len(),
        sampled.len()
    );

    score_conjugation(
        "built-in (lindera)",
        &LinderaAnalyzer::new().expect("compiled in"),
        &sampled,
    );

    let ichiran = IchiranAnalyzer::new();
    if ichiran.analyze("テスト").is_err() {
        println!("\nSKIP Ichiran: it needs WSL and its PostgreSQL database.");
        return;
    }
    let cap = std::env::var("SAKATSU_KWDLC_ICHIRAN")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(300)
        .min(sampled.len());
    score_conjugation("ichiran", &ichiran, &sampled[..cap]);
    score_conjugation(
        "built-in, same subset",
        &LinderaAnalyzer::new().expect("compiled in"),
        &sampled[..cap],
    );
}

/// Whether a label claims a tense or inflection, as opposed to naming a derivation.
///
/// Causative, Passive, Potential, Desiderative and the ambiguous voice describe how a word was
/// *built*; Past, Te-form and the rest describe how it is *inflected*. Only the second kind can
/// contradict gold's 基本形, which is what the convention allowance in `score_conjugation` turns on.
fn is_tense(form: &str) -> bool {
    [
        "Non-past",
        "Past (~ta)",
        "Conjunctive (~te)",
        "Continuative",
        "Adverbial",
        "Provisional (~eba)",
        "Conditional (~tara)",
        "Alternative (~tari)",
        "Imperative",
        "Volitional",
    ]
    .contains(&form)
}

fn score_conjugation(name: &str, analyzer: &dyn Analyzer, sentences: &[&Sentence]) {
    let (mut checked, mut agreed, mut composed, mut unmapped) = (0usize, 0usize, 0usize, 0usize);
    let mut by_convention = 0usize;
    let mut wrong: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut unmapped_forms: BTreeMap<String, usize> = BTreeMap::new();

    for sentence in sentences {
        let Ok(analysis) = analyzer.analyze(&sentence.text) else {
            continue;
        };
        for token in &analysis.tokens {
            let (Some(start), Some(end)) = (token.start, token.end) else {
                continue;
            };
            let inside: Vec<&Morpheme> = sentence
                .morphemes
                .iter()
                .filter(|m| m.start >= start && m.end <= end)
                .collect();
            let tiles = inside.first().is_some_and(|m| m.start == start)
                && inside.last().is_some_and(|m| m.end == end)
                && inside.windows(2).all(|pair| pair[0].end == pair[1].start);
            if !tiles {
                continue;
            }
            let Some(gold) = inside.last().map(|m| m.conjugation_form.as_str()) else {
                continue;
            };
            if gold.is_empty() || gold == "*" {
                continue;
            }
            let Some(accepts) = accepted_labels(gold) else {
                unmapped += 1;
                *unmapped_forms.entry(gold.to_string()).or_default() += 1;
                continue;
            };
            // Deliberately unconjugated — see the note on this test.
            if token.conjugation.is_none() && !token.components.is_empty() {
                composed += 1;
                continue;
            }

            checked += 1;
            let ours: Vec<&str> = token
                .conjugation
                .as_ref()
                .map(|conjugation| {
                    conjugation
                        .props
                        .iter()
                        .map(|prop| prop.form.as_str())
                        .collect()
                })
                .unwrap_or_default();
            // Any step naming the form counts: a chain carries the derivation as well as the
            // final inflection, and 食べさせられた is genuinely past *and* causative-passive.
            if ours.iter().any(|form| accepts.contains(form)) {
                agreed += 1;
            } else if accepts == ["Non-past"] && !ours.iter().any(|form| is_tense(form)) {
                // **A dictionary form is where the two analyzers keep different books, and this is
                // the allowance `analyzer_corpus.rs` already makes.** Ichiran's `conj` records the
                // derivation that *produced* a surface, so a word already in its dictionary form
                // has none at all; the built-in analyzer names that Non-past. Neither is wrong
                // about the Japanese, and scoring it cost Ichiran 216 of its 242 failures on the
                // first run of this test — a convention gap reported as a competence gap.
                //
                // The same allowance covers 読みたい, where gold says 基本形 of たい and the
                // analyzer says Desiderative: the word is in its dictionary form and the chain
                // names the derivation instead of a tense. Both describe it correctly.
                //
                // It is narrow on purpose. It applies only where gold says a dictionary form, and
                // only when the chain claims no *tense* that would contradict it — a token called
                // Past where gold says 基本形 is still counted wrong.
                by_convention += 1;
            } else {
                *wrong.entry((gold.to_string(), ours.join("+"))).or_default() += 1;
            }
        }
    }

    println!("\n=== {name} — conjugation against JUMAN ===");
    println!(
        "  exact           {agreed} / {checked}   ({:.1}%)",
        percent(agreed, checked)
    );
    println!(
        "  + by convention {by_convention}  (a dictionary form the analyzer names differently or leaves bare)"
    );
    println!(
        "  once that is set aside: {} / {checked}   ({:.1}%)   <- the comparable number",
        agreed + by_convention,
        percent(agreed + by_convention, checked)
    );
    println!("  skipped         {composed} composed tokens that carry no derivation by design");
    println!("  unmapped        {unmapped} occurrences of a form this file does not translate");
    if !unmapped_forms.is_empty() {
        let mut rows: Vec<_> = unmapped_forms.iter().collect();
        rows.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        let named: Vec<String> = rows
            .iter()
            .take(6)
            .map(|(form, count)| format!("{form} {count}"))
            .collect();
        println!("                  {}", named.join(", "));
    }

    println!("  most frequent disagreements — gold / what the analyzer said");
    let mut rows: Vec<_> = wrong.iter().collect();
    rows.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
    for ((gold, ours), count) in rows.iter().take(12) {
        let ours = if ours.is_empty() { "(nothing)" } else { ours };
        println!("  {count:>5}x  {gold} / {ours}");
    }
}
