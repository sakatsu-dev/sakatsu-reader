//! Held-out sentences, scored per particle, under both analyzers.
//!
//! ```text
//! cargo test -p sakatsu-core --test particle_senses -- --nocapture
//! cargo test -p sakatsu-core --test particle_senses -- --ignored --nocapture   # + Ichiran
//! ```
//!
//! # What is being measured, and what is not
//!
//! Three outcomes, and only one of them is a failure:
//!
//!   **right**     the favoured reading is the one the sentence has
//!   **abstained** nothing was favoured — designed behaviour, not a miss
//!   **wrong**     a reading was favoured and it is the wrong one
//!
//! **Wrong is the number that matters.** Abstaining costs a learner nothing: they see
//! every sense with its examples, which is what a textbook gives them. Being told with
//! emphasis that で marks a place when it marks a cause is worse than saying nothing,
//! because it is believed. So the floor here is on wrong, and the abstention rate is
//! reported rather than gated.
//!
//! A fourth outcome exists and is also not a failure: **fused**. The name is Ichiran's case —
//! it puts the particle inside a larger token (学校で as one unit) and there is nothing to click.
//!
//! **Under the built-in analyzer it means something else entirely, and the name hid that.**
//! `whether_a_fused_particle_is_still_reachable` measured it: of the 8 fused cases, **0** are
//! missing their token and **8** have the particle sitting at top level, declined by
//! `particles::senses` because `ParticleEntry::rankable` excludes が, は and を on purpose —
//! measured at 1.1%, 10.0% and 16.4% promotion, which that method calls "a shrug with two
//! paragraphs attached". The reader shows the grammar pack's card for them instead.
//!
//! So a fused row here is two unrelated situations sharing a column, and for the built-in
//! analyzer it is a deliberate abstention rather than anything lost to composition.
//!
//! # The sentences are held out
//!
//! None of them appear in `packs/particles.json`. They come from the same corpus and the
//! same harvest (`scripts/harvest-sentences.py --particle`), and were kept back rather
//! than printed as examples — measuring the cues against the sentences they illustrate
//! would be circular, which is the mistake `pack_precision.rs` was written after.
//!
//! # Tier 2 is measured as an addition
//!
//! Every case runs twice: with the WordNet table and without it. Without it must not be
//! *wrong* more often — a category is allowed to unlock a reading, never to break one.

mod common;

use std::collections::BTreeMap;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer};
use sakatsu_core::grammar::particles::{self, Categories};
use sakatsu_core::model::Analysis;

/// One held-out sentence and the reading its particle actually has.
struct Case {
    text: &'static str,
    /// Which particle to ask about.
    particle: &'static str,
    /// Which occurrence of it, counting tokens from the left. Zero for the only one.
    occurrence: usize,
    /// The sense id a reader of the English would give it.
    expect: &'static str,
    english: &'static str,
}

/// Judged by reading the translation, one at a time. Not generated.
const CASES: &[Case] = &[
    // ---------------------------------------------------------------- で
    Case {
        text: "私たちは公園でよくままごとをしたものだ。",
        particle: "で",
        occurrence: 0,
        expect: "de-place",
        english: "We often played house in the park.",
    },
    Case {
        text: "母は台所でせっせと料理しながら歌を口ずさんでいた。",
        particle: "で",
        occurrence: 0,
        expect: "de-place",
        english: "Mother hummed as she went about her cooking in the kitchen.",
    },
    Case {
        text: "小包を速達で送ると割り増し料金がいります。",
        particle: "で",
        occurrence: 0,
        expect: "de-means",
        english: "There is an extra charge for mailing packages by express.",
    },
    Case {
        text: "すぐれた材料で作られた家具は、売れ行きがよい。",
        particle: "で",
        occurrence: 0,
        expect: "de-material",
        english: "Furniture made of good materials sells well.",
    },
    Case {
        text: "彼は交通事故で軽傷を負った。",
        particle: "で",
        occurrence: 0,
        expect: "de-cause",
        english: "He was slightly injured in a traffic accident.",
    },
    Case {
        text: "その建物の一部分だけが地震で破壊された。",
        particle: "で",
        occurrence: 0,
        expect: "de-cause",
        english: "Only part of the building has been destroyed by the earthquake.",
    },
    Case {
        text: "撮影は三日間でクランクアップします。",
        particle: "で",
        occurrence: 0,
        expect: "de-limit",
        english: "The picture will finish shooting in three days.",
    },
    Case {
        text: "もうホントにこいつは優柔不断で、イジイジした奴だ。",
        particle: "で",
        occurrence: 0,
        expect: "de-copula-te",
        english: "This guy is really wishy-washy.",
    },
    // ---------------------------------------------------------------- を
    Case {
        text: "彼に長い手紙を書いた。",
        particle: "を",
        occurrence: 0,
        expect: "wo-object",
        english: "I wrote a long letter to him.",
    },
    Case {
        text: "島を出る手だてはありません。",
        particle: "を",
        occurrence: 0,
        expect: "wo-departure",
        english: "There's no way off the island.",
    },
    Case {
        text: "リスはすばしこく木を登る。",
        particle: "を",
        occurrence: 0,
        expect: "wo-path",
        english: "Squirrels are nimble in climbing trees.",
    },
    Case {
        text: "私たちは太平洋の上空を飛んでいます。",
        particle: "を",
        occurrence: 0,
        expect: "wo-path",
        english: "We are flying over the Pacific.",
    },
    // ---------------------------------------------------------------- に
    Case {
        text: "机の上に一冊の本がある。",
        particle: "に",
        occurrence: 0,
        expect: "ni-existence",
        english: "There is a book on the desk.",
    },
    Case {
        text: "さあ東京駅に着きました。",
        particle: "に",
        occurrence: 0,
        expect: "ni-destination",
        english: "Here we were at Tokyo Station.",
    },
    Case {
        text: "リオのカーニバルは二月に開催される。",
        particle: "に",
        occurrence: 0,
        expect: "ni-time",
        english: "Rio's carnival is held in February.",
    },
    Case {
        text: "目覚ましを七時にセットしよう。",
        particle: "に",
        occurrence: 0,
        expect: "ni-time",
        english: "I'll set the alarm for seven o'clock.",
    },
    Case {
        text: "ジョーンズ氏は事業を息子に譲り渡すつもりだ。",
        particle: "に",
        occurrence: 0,
        expect: "ni-recipient",
        english: "Mr Jones will hand over his business to his son.",
    },
    Case {
        text: "下着を買いに行くんだよ。",
        particle: "に",
        occurrence: 0,
        expect: "ni-purpose",
        english: "We're going to buy underwear.",
    },
    // ---------------------------------------------------------------- が
    Case {
        text: "ざあざあ雨が降った。",
        particle: "が",
        occurrence: 0,
        expect: "ga-subject",
        english: "It poured down.",
    },
    Case {
        text: "本当に裁縫が上手ですね。",
        particle: "が",
        occurrence: 0,
        expect: "ga-object-like",
        english: "You're a magician with a needle and thread.",
    },
    // ---------------------------------------------------------------- は
    Case {
        text: "私の祖父はインド人の血を引いていた。",
        particle: "は",
        occurrence: 0,
        expect: "wa-topic",
        english: "My grandfather was part Indian.",
    },
    Case {
        text: "夏場の東京は台風に見舞われやすい。",
        particle: "は",
        occurrence: 0,
        expect: "wa-topic",
        english: "Tokyo is subject to typhoons in summer.",
    },
];

/// What happened to one case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Right,
    /// Nothing favoured. The learner sees every sense, which is the fallback by design.
    Abstained,
    Wrong,
    /// The analyzer did not give the particle its own token, so the panel never asks.
    Fused,
}

fn judge(analysis: &Analysis, case: &Case, categories: &Categories) -> Outcome {
    let index = analysis
        .tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.surface == case.particle)
        .map(|(i, _)| i)
        .nth(case.occurrence);
    let Some(index) = index else {
        return Outcome::Fused;
    };
    let Some(reading) = particles::senses(&analysis.tokens, index, categories) else {
        return Outcome::Fused;
    };
    match reading.favoured {
        None => Outcome::Abstained,
        Some(at) if reading.senses[at].id == case.expect => Outcome::Right,
        Some(_) => Outcome::Wrong,
    }
}

/// Per particle: right / abstained / wrong / fused. Every wrong one is named.
fn score(title: &str, results: &[(&Case, Outcome, Option<String>)]) -> usize {
    let mut by_particle: BTreeMap<&str, [usize; 4]> = BTreeMap::new();
    for (case, outcome, _) in results {
        let row = by_particle.entry(case.particle).or_default();
        row[match outcome {
            Outcome::Right => 0,
            Outcome::Abstained => 1,
            Outcome::Wrong => 2,
            Outcome::Fused => 3,
        }] += 1;
    }

    println!("\n=== {title} ===");
    println!("           right  abstained  wrong  fused");
    let mut wrong = 0;
    for (particle, row) in &by_particle {
        println!(
            "  {particle}         {:2}         {:2}     {:2}     {:2}",
            row[0], row[1], row[2], row[3]
        );
        wrong += row[2];
    }
    let right: usize = by_particle.values().map(|row| row[0]).sum();
    println!(
        "  total     {right:2}         {:2}     {wrong:2}     {:2}",
        by_particle.values().map(|row| row[1]).sum::<usize>(),
        by_particle.values().map(|row| row[3]).sum::<usize>()
    );

    for (case, outcome, favoured) in results {
        if *outcome == Outcome::Wrong {
            println!("\n  WRONG  {}", case.text);
            println!("         {}", case.english);
            println!(
                "         wanted {}, favoured {}",
                case.expect,
                favoured.as_deref().unwrap_or("?")
            );
        }
    }
    wrong
}

fn run(
    analysis_of: impl Fn(&str) -> Option<Analysis>,
    categories: &Categories,
    title: &str,
) -> usize {
    let results: Vec<(&Case, Outcome, Option<String>)> = CASES
        .iter()
        .map(|case| {
            let Some(analysis) = analysis_of(case.text) else {
                return (case, Outcome::Fused, None);
            };
            let outcome = judge(&analysis, case, categories);
            // Only computed for the report, and only when it is needed.
            let favoured = if outcome == Outcome::Wrong {
                analysis
                    .tokens
                    .iter()
                    .position(|token| token.surface == case.particle)
                    .and_then(|index| particles::senses(&analysis.tokens, index, categories))
                    .and_then(|reading| reading.favoured.map(|at| reading.senses[at].id.clone()))
            } else {
                None
            };
            (case, outcome, favoured)
        })
        .collect();
    score(title, &results)
}

/// Where the WordNet table lives once `scripts/fetch-wordnet.ps1` has run.
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

/// The number that decides whether this feature is safe to show a learner.
///
/// Zero, and it should stay zero. Not because the cues are perfect — they abstain often,
/// which the report shows — but because the whole design is that a cue only fires when it
/// is nearly conclusive. A wrong answer here means a cue is claiming more than it knows,
/// and the fix is to weaken or delete that cue, never to raise this number.
const MAX_WRONG: usize = 0;

#[test]
fn held_out_sentences_are_never_confidently_misread_under_lindera() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let table = categories();
    if table.is_empty() {
        println!("(no WordNet table; run scripts/fetch-wordnet.ps1 for the Tier 2 numbers)");
    }

    let with = run(
        |text| analyzer.analyze(text).ok(),
        &table,
        "Lindera, with categories",
    );
    let without = run(
        |text| analyzer.analyze(text).ok(),
        &Categories::default(),
        "Lindera, structural cues only",
    );

    // `>` rather than `<=`: the ceiling is zero, and a `<= 0` on an unsigned count is a
    // comparison clippy correctly points out can only mean one thing.
    if with > MAX_WRONG {
        panic!("{with} wrong with categories, ceiling is {MAX_WRONG}");
    }
    if without > MAX_WRONG {
        panic!("{without} wrong on structural cues alone, ceiling is {MAX_WRONG}");
    }
}

/// How often each sense wins, across every corpus sentence containing the particle.
///
/// # Why this exists
///
/// The fixture above found two bad cues, and both were the same mistake: a cue that fires
/// on nearly every sentence. `noun.artifact` on で's means — but WordNet files every room
/// and building as an artifact, so it fired on 台所 and 部屋 as readily as on 電車. `行く
/// appears later` on に's destination — but 行く follows に in the purpose reading too.
///
/// Neither was caught by a unit test, because a unit test asks "does this fire where it
/// should" and never "what else does it fire on". Twenty-two hand-picked sentences caught
/// them by luck. This asks the question directly, over twenty-six thousand.
///
/// A cue is evidence only if it distinguishes. A sense favoured on almost every occurrence
/// of its particle is not being *found* in those sentences — it is being asserted over
/// them, and the reader has no way to tell the difference.
///
/// Reported, and gated loosely. The threshold is not a claim about correct Japanese; it is
/// the point past which a ranking has stopped carrying information.
#[test]
fn no_single_sense_wins_nearly_every_time() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let table = categories();
    let corpus = common::jitendex_sentences();

    // Enough for the rates to settle; the whole corpus through Lindera is slower than
    // this test is worth.
    const PER_PARTICLE: usize = 600;

    let mut failures = Vec::new();
    for entry in particles::all() {
        let mut favoured: BTreeMap<&str, usize> = BTreeMap::new();
        let mut abstained = 0usize;
        let mut seen = 0usize;

        for text in corpus.iter().filter(|text| text.contains(&entry.surface)) {
            if seen >= PER_PARTICLE {
                break;
            }
            let Ok(analysis) = analyzer.analyze(text) else {
                continue;
            };
            let Some(index) = analysis
                .tokens
                .iter()
                .position(|token| token.surface == entry.surface)
            else {
                continue;
            };
            let Some(reading) = particles::senses(&analysis.tokens, index, &table) else {
                continue;
            };
            seen += 1;
            match reading.favoured {
                Some(at) => {
                    *favoured
                        .entry(
                            entry
                                .senses
                                .iter()
                                .find(|sense| sense.id == reading.senses[at].id)
                                .map(|sense| sense.id.as_str())
                                .unwrap_or("?"),
                        )
                        .or_default() += 1
                }
                None => abstained += 1,
            }
        }

        if seen == 0 {
            continue;
        }
        println!("\n{} — {seen} occurrences", entry.surface);
        println!(
            "  abstained {:3}  ({:2}%)",
            abstained,
            abstained * 100 / seen
        );
        for (id, count) in &favoured {
            let share = count * 100 / seen;
            println!("  {id:16} {count:3}  ({share:2}%)");
            if share > MAX_SHARE {
                failures.push(format!("{id} is favoured on {share}% of {}", entry.surface));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "a sense this dominant is being asserted, not found:\n  {}",
        failures.join("\n  ")
    );
}

/// The point past which a ranking has stopped distinguishing anything.
///
/// Deliberately loose. Some readings really are commoner than others and a share of half
/// is unremarkable; what this catches is the cue that fires on everything, which is what
/// both of the bugs it was written after looked like.
const MAX_SHARE: usize = 60;

#[test]
#[ignore = "needs a working ichiran-cli"]
fn held_out_sentences_are_never_confidently_misread_under_ichiran() {
    let analyzer = IchiranAnalyzer::new();
    if analyzer.analyze("水").is_err() {
        println!("SKIP: ichiran-cli is not answering");
        return;
    }

    let wrong = run(
        |text| analyzer.analyze(text).ok(),
        &categories(),
        "Ichiran, with categories",
    );
    if wrong > MAX_WRONG {
        panic!("{wrong} wrong under Ichiran, ceiling is {MAX_WRONG}");
    }
}

/// A2: when the analyzer fuses a particle into a compound, is the particle still *there*?
///
/// ```text
/// cargo test --release -p sakatsu-core --test particle_senses -- --ignored --nocapture reaches
/// ```
///
/// # Why this is worth a test rather than an assumption
///
/// `local documentation` ranks it second among this project's failures: が, は and を
/// "contribute **nothing** — every occurrence comes back *fused*", so the three particles a
/// beginner meets first have no measured accuracy and the reader never offers their senses.
///
/// `judge` above decides that by looking for a **top-level** token whose surface is the particle.
/// But `project notes` §4a makes a token the longest span the dictionary answers for and keeps the
/// pieces in `Token::components` precisely so nothing is lost — so the particle may be present and
/// merely out of reach, which is a completely different problem from being gone.
///
/// This asks three things of every fused case:
///
///   1. is the particle in `components` at all;
///   2. would `particles::senses`, given those pieces, have answered;
///   3. would it have been right.
///
/// A yes to all three means the senses exist and only the lookup path is missing, and the fix is
/// to reach them — **not** to un-join anything, which §4a settles and the reader's word-status
/// grain depends on.
#[test]
#[ignore = "needs the particle pack"]
fn whether_a_fused_particle_is_still_reachable() {
    let analyzer = LinderaAnalyzer::new().expect("compiled in");
    let categories = categories();

    let (mut fused, mut in_components, mut answered, mut correct) =
        (0usize, 0usize, 0usize, 0usize);
    let mut absent: Vec<String> = Vec::new();
    let mut by_particle: BTreeMap<&str, [usize; 2]> = BTreeMap::new();
    // The second road to Fused: the token is right there and the scorer says nothing about it.
    let mut declined = 0usize;
    let mut declined_by: BTreeMap<&str, Vec<String>> = BTreeMap::new();

    for case in CASES {
        let Ok(analysis) = analyzer.analyze(case.text) else {
            continue;
        };
        // **`judge` reaches Fused by two different roads and they are not the same fault.**
        // Either no top-level token carries the particle — genuinely folded away — or one does
        // and `particles::senses` declines it. The first is a segmentation question; the second
        // is the scorer's own. `accuracy-baseline.md` attributes every fused case to the first.
        let top = analysis
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.surface == case.particle)
            .map(|(i, _)| i)
            .nth(case.occurrence);
        if let Some(index) = top {
            if particles::senses(&analysis.tokens, index, &categories).is_none() {
                declined += 1;
                declined_by
                    .entry(case.particle)
                    .or_default()
                    .push(case.text.to_string());
            }
            continue; // the particle is right there; nothing was folded away
        }
        fused += 1;
        let row = by_particle.entry(case.particle).or_default();
        row[0] += 1;

        // The pieces of every compound, flattened one level — which is the depth the composition
        // actually builds.
        let pieces: Vec<sakatsu_core::model::Token> = analysis
            .tokens
            .iter()
            .flat_map(|token| {
                if token.components.is_empty() {
                    vec![token.clone()]
                } else {
                    token.components.clone()
                }
            })
            .collect();
        let at = pieces
            .iter()
            .enumerate()
            .filter(|(_, token)| token.surface == case.particle)
            .map(|(i, _)| i)
            .nth(case.occurrence);
        let Some(at) = at else {
            absent.push(format!("  {} in {}", case.particle, case.text));
            continue;
        };
        in_components += 1;
        row[1] += 1;

        if let Some(reading) = particles::senses(&pieces, at, &categories) {
            if let Some(favoured) = reading.favoured {
                answered += 1;
                if reading.senses[favoured].id == case.expect {
                    correct += 1;
                }
            }
        }
    }

    println!("\n=== fused particles: are they reachable? ===");
    println!("  token missing (truly folded)     {fused}");
    println!("  token present, senses() declined {declined}   <- the scorer, not segmentation");
    for (particle, texts) in &declined_by {
        println!("    {particle}: {}", texts.len());
        for text in texts.iter().take(3) {
            println!("      {text}");
        }
    }
    println!("  particle is in components   {in_components}");
    println!("  senses() would answer       {answered}");
    println!("  ... and would be right      {correct}");
    println!("\n  by particle — fused / of those, in components");
    for (particle, [f, c]) in &by_particle {
        println!("    {particle:<4} {f:>3} / {c}");
    }
    if !absent.is_empty() {
        println!("\n  not in components at all:");
        for line in absent.iter().take(12) {
            println!("{line}");
        }
    }
}
