//! Does the particle-sense ranking earn its place? — the measurements.
//!
//! `particle_senses.rs` asks whether the ranking is *safe* (`MAX_WRONG = 0`). That is a different
//! question from whether it is *useful*, and this file asks the second one. It gates nothing; it
//! prints a report. The numbers are what a decision to keep, narrow or drop the feature should be
//! argued from.
//!
//! # Why fire rate comes first
//!
//! Three of the eighteen authored senses carry no cues at all — `wo-object`, `ga-subject`,
//! `wa-topic` — and promotion needs a strictly-positive *unique* top score (`particles::senses`).
//! Those three are the ordinary readings: the plain direct object, the plain subject, the plain
//! topic. So for を, が and は the common case cannot be promoted by construction, and the reader
//! meets the abstention panel instead. How often that happens is measurable without labelling
//! anything, and it bounds how much the ranking could possibly be worth.
//!
//! # What needs no ground truth
//!
//! Three of the measurements are label-free, which is why they run here and run first:
//!
//! - **fire rate** — how often anything is promoted, per particle, over real sentences;
//! - **the passage boundary** — `Predicate` scans to the end of the token list and `SecondWa`
//!   counts across it, while the interface passes *the whole passage*. Embedding a sentence
//!   between two unrelated neighbours must not change its answer. Any change is a defect, and
//!   saying which of the two is right is unnecessary to call it one;
//! - **the WordNet delta** — the same run with an empty table, sizing what the 1 MB file buys at
//!   corpus scale rather than at n=22.
//!
//! Accuracy needs hand-judged labels and lives in `particle_senses.rs`.

mod common;

use std::collections::BTreeMap;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::grammar::particles::{self, Categories};
use sakatsu_core::model::Token;

/// Sentences per particle. Enough for the rates to settle without making the run tiresome.
const PER_PARTICLE: usize = 1200;

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

/// What happened across a set of particle occurrences.
#[derive(Default, Clone)]
struct Tally {
    occurrences: usize,
    promoted: usize,
    /// Nothing scored above zero — no cue had anything to say.
    silent: usize,
    /// Two or more senses tied at the top. Evidence arrived and cancelled out.
    tied: usize,
    /// Which sense won, when one did.
    winners: BTreeMap<String, usize>,
}

impl Tally {
    fn note(&mut self, reading: &particles::Reading) {
        self.occurrences += 1;
        match reading.favoured {
            Some(at) => {
                self.promoted += 1;
                *self
                    .winners
                    .entry(reading.senses[at].id.clone())
                    .or_default() += 1;
            }
            None => {
                // Which kind of silence: nothing fired at all, or a tie at the top.
                let best = reading.senses.first().map(|sense| sense.score).unwrap_or(0);
                if best <= 0 {
                    self.silent += 1;
                } else {
                    self.tied += 1;
                }
            }
        }
    }
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        100.0 * part as f64 / whole as f64
    }
}

/// Every index in `tokens` whose surface is exactly `particle`.
///
/// Every occurrence, not just the first: a reader clicks the particle in front of them, and a
/// sentence with three に has three chances to be helped or misled.
fn occurrences(tokens: &[Token], particle: &str) -> Vec<usize> {
    tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.surface == particle)
        .map(|(at, _)| at)
        .collect()
}

/// Sentences containing a particle, capped, in corpus order.
fn sentences_with<'a>(corpus: &'a [String], particle: &str, cap: usize) -> Vec<&'a String> {
    corpus
        .iter()
        .filter(|text| text.contains(particle))
        .take(cap)
        .collect()
}

fn run_fire_rate(
    analyzer: &LinderaAnalyzer,
    corpus: &[String],
    table: &Categories,
) -> BTreeMap<String, Tally> {
    let mut per_particle: BTreeMap<String, Tally> = BTreeMap::new();
    for entry in particles::all() {
        let mut tally = Tally::default();
        for text in sentences_with(corpus, &entry.surface, PER_PARTICLE) {
            let Ok(analysis) = analyzer.analyze(text) else {
                continue;
            };
            for index in occurrences(&analysis.tokens, &entry.surface) {
                if let Some(reading) = particles::senses(&analysis.tokens, index, table) {
                    tally.note(&reading);
                }
            }
        }
        per_particle.insert(entry.surface.clone(), tally);
    }
    per_particle
}

fn report_fire_rate(title: &str, per_particle: &BTreeMap<String, Tally>) {
    println!();
    println!("{title}");
    println!(
        "  {:<6} {:>8} {:>10} {:>10} {:>10}",
        "part", "seen", "promoted", "silent", "tied"
    );
    let mut total = Tally::default();
    for (particle, tally) in per_particle {
        println!(
            "  {:<6} {:>8} {:>9.1}% {:>9.1}% {:>9.1}%",
            particle,
            tally.occurrences,
            percent(tally.promoted, tally.occurrences),
            percent(tally.silent, tally.occurrences),
            percent(tally.tied, tally.occurrences),
        );
        total.occurrences += tally.occurrences;
        total.promoted += tally.promoted;
        total.silent += tally.silent;
        total.tied += tally.tied;
    }
    println!(
        "  {:<6} {:>8} {:>9.1}% {:>9.1}% {:>9.1}%",
        "ALL",
        total.occurrences,
        percent(total.promoted, total.occurrences),
        percent(total.silent, total.occurrences),
        percent(total.tied, total.occurrences),
    );
}

fn report_winners(per_particle: &BTreeMap<String, Tally>) {
    println!();
    println!("Which sense wins, when one does");
    for entry in particles::all() {
        let Some(tally) = per_particle.get(&entry.surface) else {
            continue;
        };
        let cueless: Vec<&str> = entry
            .senses
            .iter()
            .filter(|sense| sense.cues.is_empty())
            .map(|sense| sense.id.as_str())
            .collect();
        println!(
            "  {}  — {} senses, {} of them cueless and therefore unpromotable: {}",
            entry.surface,
            entry.senses.len(),
            cueless.len(),
            if cueless.is_empty() {
                "none".to_string()
            } else {
                cueless.join(", ")
            }
        );
        if tally.promoted == 0 {
            println!("        never promotes anything");
            continue;
        }
        for (id, count) in &tally.winners {
            println!(
                "        {:<22} {:>6}   {:>5.1}% of its promotions",
                id,
                count,
                percent(*count, tally.promoted)
            );
        }
    }
}

#[test]
#[ignore = "evaluation harness: prints a report, gates nothing"]
fn how_often_does_the_ranking_say_anything() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let corpus = common::jitendex_sentences();
    println!("corpus: {} sentences", corpus.len());

    let table = categories();
    println!(
        "WordNet: {}",
        if table.is_empty() {
            "absent — run scripts/fetch-wordnet.ps1"
        } else {
            "loaded"
        }
    );

    let with = run_fire_rate(&analyzer, &corpus, &table);
    report_fire_rate("Fire rate, with WordNet", &with);
    report_winners(&with);

    if !table.is_empty() {
        let without = run_fire_rate(&analyzer, &corpus, &Categories::default());
        report_fire_rate("Fire rate, without WordNet", &without);

        println!();
        println!("What the table buys");
        for (particle, on) in &with {
            let off = without.get(particle).cloned().unwrap_or_default();
            let on_rate = percent(on.promoted, on.occurrences);
            let off_rate = percent(off.promoted, off.occurrences);
            println!(
                "  {:<6} promoted {:>5.1}% without -> {:>5.1}% with   ({:+.1} points)",
                particle,
                off_rate,
                on_rate,
                on_rate - off_rate
            );
        }
    }
}

/// **A sentence must mean the same thing when it has neighbours.**
///
/// `Predicate` scans to the end of the token list and `SecondWa` counts across it, with no
/// sentence boundary anywhere — and the interface hands the whole analysed passage to
/// `particle_senses`, because a capture is a passage. So a verb in the *next* sentence can decide
/// this sentence's particle, and every は in a paragraph can make every other は contrastive.
///
/// Each sentence runs alone and again between two unrelated neighbours. Which of the two answers
/// is right does not matter: an answer that depends on unrelated text nearby is not an answer
/// about this sentence.
///
/// **Alignment is checked, not assumed.** The padded run is only compared when the sentence's own
/// tokens survive intact inside it — same surfaces, same order, at a findable offset. Where the
/// tokeniser merged across a join, the pair is skipped rather than mismatched.
#[test]
#[ignore = "evaluation harness: prints a report, gates nothing"]
fn does_the_answer_survive_having_neighbours() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let corpus = common::jitendex_sentences();
    let table = categories();

    // Ordinary, unrelated, and the same pair for every case so the comparison is like for like.
    let before = corpus.get(corpus.len() / 3).cloned().unwrap_or_default();
    let after = corpus
        .get(corpus.len() * 2 / 3)
        .cloned()
        .unwrap_or_default();
    println!("neighbour before: {before}");
    println!("neighbour after:  {after}");

    println!();
    println!(
        "  {:<6} {:>9} {:>10} {:>9} {:>9} {:>9}",
        "part", "compared", "changed", "gained", "lost", "swapped"
    );

    let (mut all_compared, mut all_changed, mut all_gained, mut all_lost, mut all_swapped) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut examples: Vec<String> = Vec::new();

    for entry in particles::all() {
        let (mut compared, mut changed, mut gained, mut lost, mut swapped) =
            (0usize, 0usize, 0usize, 0usize, 0usize);

        for text in sentences_with(&corpus, &entry.surface, PER_PARTICLE) {
            let padded = format!("{before}{text}{after}");
            let (Ok(alone), Ok(within)) = (analyzer.analyze(text), analyzer.analyze(&padded))
            else {
                continue;
            };

            // Where the sentence's own tokens sit inside the padded analysis. Found by matching
            // the surface sequence, so a merge across either join simply fails to match and the
            // pair is dropped.
            let want: Vec<&str> = alone.tokens.iter().map(|t| t.surface.as_str()).collect();
            let have: Vec<&str> = within.tokens.iter().map(|t| t.surface.as_str()).collect();
            let Some(offset) = have
                .windows(want.len().max(1))
                .position(|w| w == want.as_slice())
            else {
                continue;
            };

            for index in occurrences(&alone.tokens, &entry.surface) {
                let one = particles::senses(&alone.tokens, index, &table);
                let two = particles::senses(&within.tokens, offset + index, &table);
                let (Some(one), Some(two)) = (one, two) else {
                    continue;
                };
                compared += 1;
                let a = one.favoured.map(|at| one.senses[at].id.clone());
                let b = two.favoured.map(|at| two.senses[at].id.clone());
                if a == b {
                    continue;
                }
                changed += 1;
                match (&a, &b) {
                    (None, Some(_)) => gained += 1,
                    (Some(_), None) => lost += 1,
                    _ => swapped += 1,
                }
                if examples.len() < 12 {
                    examples.push(format!(
                        "  {} in {}\n      alone: {}\n      among neighbours: {}",
                        entry.surface,
                        text,
                        a.unwrap_or_else(|| "(nothing promoted)".into()),
                        b.unwrap_or_else(|| "(nothing promoted)".into()),
                    ));
                }
            }
        }

        println!(
            "  {:<6} {:>9} {:>9.1}% {:>9} {:>9} {:>9}",
            entry.surface,
            compared,
            percent(changed, compared),
            gained,
            lost,
            swapped
        );
        all_compared += compared;
        all_changed += changed;
        all_gained += gained;
        all_lost += lost;
        all_swapped += swapped;
    }

    println!(
        "  {:<6} {:>9} {:>9.1}% {:>9} {:>9} {:>9}",
        "ALL",
        all_compared,
        percent(all_changed, all_compared),
        all_gained,
        all_lost,
        all_swapped
    );

    if !examples.is_empty() {
        println!();
        println!("A few of them");
        for line in &examples {
            println!("{line}");
        }
    }

    println!();
    println!("`changed` is the defect rate: the same sentence, a different answer, decided by");
    println!("text that has nothing to do with it. `gained` means neighbours conjured a pick out");
    println!("of an abstention; `lost` means they destroyed one; `swapped` means they replaced");
    println!("one confident answer with a different confident answer.");
}

/// The same question with many different neighbours, and with the mechanism printed.
///
/// The single-pair run above uses one deterministic filler pair, and one pair could be
/// pathological — a neighbour that happens to contain わかる would fire `ga-object-like` all by
/// itself. So this repeats it across twenty pairs drawn from across the corpus and reports the
/// spread. If the defect were an artifact of one unlucky neighbour, most pairs would show zero.
#[test]
#[ignore = "evaluation harness: prints a report, gates nothing"]
fn is_the_neighbour_effect_the_neighbour_or_the_rule() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let corpus = common::jitendex_sentences();
    let table = categories();

    // A sample of sentences per particle, small enough to run twenty times over.
    const SAMPLE: usize = 300;
    const PAIRS: usize = 20;

    println!();
    println!("Defect rate per neighbour pair, {PAIRS} pairs spread across the corpus");
    println!(
        "  {:<6} {:>8} {:>8} {:>8}  per-pair rates",
        "part", "min", "median", "max"
    );

    for entry in particles::all() {
        let mut rates: Vec<f64> = Vec::new();
        for pair in 0..PAIRS {
            let before = corpus
                .get(corpus.len() * (pair * 2 + 1) / (PAIRS * 2 + 1))
                .cloned()
                .unwrap_or_default();
            let after = corpus
                .get(corpus.len() * (pair * 2 + 2) / (PAIRS * 2 + 1))
                .cloned()
                .unwrap_or_default();

            let (mut compared, mut changed) = (0usize, 0usize);
            for text in sentences_with(&corpus, &entry.surface, SAMPLE) {
                let padded = format!("{before}{text}{after}");
                let (Ok(alone), Ok(within)) = (analyzer.analyze(text), analyzer.analyze(&padded))
                else {
                    continue;
                };
                let want: Vec<&str> = alone.tokens.iter().map(|t| t.surface.as_str()).collect();
                let have: Vec<&str> = within.tokens.iter().map(|t| t.surface.as_str()).collect();
                let Some(offset) = have
                    .windows(want.len().max(1))
                    .position(|w| w == want.as_slice())
                else {
                    continue;
                };
                for index in occurrences(&alone.tokens, &entry.surface) {
                    let one = particles::senses(&alone.tokens, index, &table);
                    let two = particles::senses(&within.tokens, offset + index, &table);
                    let (Some(one), Some(two)) = (one, two) else {
                        continue;
                    };
                    compared += 1;
                    let a = one.favoured.map(|at| one.senses[at].id.clone());
                    let b = two.favoured.map(|at| two.senses[at].id.clone());
                    if a != b {
                        changed += 1;
                    }
                }
            }
            rates.push(percent(changed, compared));
        }
        rates.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let shown: Vec<String> = rates.iter().map(|r| format!("{r:.0}")).collect();
        println!(
            "  {:<6} {:>7.1}% {:>7.1}% {:>7.1}%  {}",
            entry.surface,
            rates.first().copied().unwrap_or(0.0),
            rates[rates.len() / 2],
            rates.last().copied().unwrap_or(0.0),
            shown.join(" ")
        );
    }
}

/// Show the mechanism: what a neighbour actually contributes to the score.
#[test]
#[ignore = "evaluation harness: prints a report, gates nothing"]
fn what_does_a_neighbour_contribute() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let table = categories();

    let sentence = "猫が庭にいる。";
    for (label, text) in [
        ("alone", sentence.to_string()),
        (
            "followed by a sentence about understanding",
            format!("{sentence}この辞書の用例はわかりやすい。"),
        ),
        (
            "followed by a sentence with another は",
            format!("{sentence}彼は学生です。"),
        ),
    ] {
        let Ok(analysis) = analyzer.analyze(&text) else {
            continue;
        };
        println!();
        println!("{label}: {text}");
        for particle in ["が", "は"] {
            for index in occurrences(&analysis.tokens, particle) {
                let Some(reading) = particles::senses(&analysis.tokens, index, &table) else {
                    continue;
                };
                // Only the first occurrence, which is the one inside the sentence under test.
                println!(
                    "  {particle} favoured={:?}",
                    reading.favoured.map(|at| reading.senses[at].id.clone())
                );
                for sense in &reading.senses {
                    if sense.score > 0 {
                        println!(
                            "      {:<16} score={} because={:?}",
                            sense.id, sense.score, sense.because
                        );
                    }
                }
                break;
            }
        }
    }
}

/// **How wrong does a real capture get?**
///
/// The pair test above shows the effect exists. This sizes it the way a reader meets it: a capture
/// is not one sentence, it is a panel or a paragraph, and the interface passes the whole thing to
/// `particle_senses` (`App.svelte`, `study.analysis.tokens`). So the question is not "can a
/// neighbour change an answer" but "how much of a page is wrong at once".
///
/// Passages are built from consecutive corpus sentences — a fair stand-in for a capture of several
/// sentences — and every particle answer inside the passage is compared with the answer the same
/// particle gets in its own sentence alone. The per-sentence answer is the honest one: it is what
/// the rules were written and tested against.
#[test]
#[ignore = "evaluation harness: prints a report, gates nothing"]
fn how_much_of_a_capture_is_wrong_at_once() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let corpus = common::jitendex_sentences();
    let table = categories();

    const PASSAGES: usize = 400;

    println!();
    println!("Answers that change when the sentence is read as part of a capture");
    println!(
        "  {:<10} {:>10} {:>10} {:>10}  affected passages",
        "sentences", "compared", "changed", "passages"
    );

    for length in [1usize, 2, 3, 5, 8] {
        let (mut compared, mut changed) = (0usize, 0usize);
        let (mut passages, mut affected) = (0usize, 0usize);

        for start in (0..corpus.len().min(PASSAGES * length)).step_by(length) {
            let block: Vec<&String> = corpus.iter().skip(start).take(length).collect();
            if block.len() < length {
                break;
            }
            let joined: String = block.iter().map(|s| s.as_str()).collect();
            let Ok(whole) = analyzer.analyze(&joined) else {
                continue;
            };
            passages += 1;
            let mut touched = false;

            // Walk each sentence, find where its tokens sit in the passage, and compare.
            for sentence in &block {
                let Ok(alone) = analyzer.analyze(sentence) else {
                    continue;
                };
                let want: Vec<&str> = alone.tokens.iter().map(|t| t.surface.as_str()).collect();
                let have: Vec<&str> = whole.tokens.iter().map(|t| t.surface.as_str()).collect();
                let Some(offset) = have
                    .windows(want.len().max(1))
                    .position(|w| w == want.as_slice())
                else {
                    continue;
                };
                for entry in particles::all() {
                    for index in occurrences(&alone.tokens, &entry.surface) {
                        let one = particles::senses(&alone.tokens, index, &table);
                        let two = particles::senses(&whole.tokens, offset + index, &table);
                        let (Some(one), Some(two)) = (one, two) else {
                            continue;
                        };
                        compared += 1;
                        let a = one.favoured.map(|at| one.senses[at].id.clone());
                        let b = two.favoured.map(|at| two.senses[at].id.clone());
                        if a != b {
                            changed += 1;
                            touched = true;
                        }
                    }
                }
            }
            if touched {
                affected += 1;
            }
        }

        println!(
            "  {:<10} {:>10} {:>9.1}% {:>10} {:>17.1}%",
            length,
            compared,
            percent(changed, compared),
            passages,
            percent(affected, passages)
        );
    }

    println!();
    println!(
        "One sentence is the control and must read 0.0% — a passage of one has no neighbours."
    );
    println!("Everything below it is the cost of having no sentence boundary in the cues.");
}

/// **Would a trivial rule do better?**
///
/// The current policy promotes only on a unique positive top score, which buys near-zero wrong
/// answers at the cost of abstaining most of the time. The obvious alternative costs nothing to
/// build: always promote the sense the pack authors wrote first, which for every particle here is
/// the ordinary reading — plain object, plain subject, plain topic, and so on.
///
/// If `default-first` is right more often than `current`, the scoring is not paying for itself,
/// because a constant would beat it.
///
/// Judged against the hand-labelled cases in `particle_senses.rs`, which are the only ground truth
/// in the repository. They are reproduced here by id rather than re-labelled — the expectation
/// strings are copied from that file, so the two cannot disagree about what is correct.
#[test]
#[ignore = "evaluation harness: prints a report, gates nothing"]
fn would_always_guessing_the_common_reading_do_better() {
    // (particle, expected sense id) for each hand-judged case in `particle_senses.rs`.
    // Copied from that file's CASES; the sentences themselves are not needed here, only which
    // reading a reader of the English gave, and which particle it was.
    let labelled: &[(&str, &str)] = &LABELLED;

    println!();
    println!("The authored-first sense of each particle — what `default-first` would always say");
    let mut first_of: BTreeMap<&str, &str> = BTreeMap::new();
    for entry in particles::all() {
        let first = entry.senses.first().expect("a particle has senses");
        first_of.insert(entry.surface.as_str(), first.id.as_str());
        println!("  {:<4} -> {}", entry.surface, first.id);
    }

    println!();
    println!(
        "  {:<6} {:>8} {:>16} {:>16}",
        "part", "cases", "default-first", "share"
    );
    let (mut all_cases, mut all_right) = (0usize, 0usize);
    for entry in particles::all() {
        let mine: Vec<&(&str, &str)> = labelled
            .iter()
            .filter(|(particle, _)| *particle == entry.surface)
            .collect();
        if mine.is_empty() {
            continue;
        }
        let first = first_of[entry.surface.as_str()];
        let right = mine.iter().filter(|(_, expect)| *expect == first).count();
        println!(
            "  {:<6} {:>8} {:>15} {:>15.1}%",
            entry.surface,
            mine.len(),
            right,
            percent(right, mine.len())
        );
        all_cases += mine.len();
        all_right += right;
    }
    println!(
        "  {:<6} {:>8} {:>15} {:>15.1}%",
        "ALL",
        all_cases,
        all_right,
        percent(all_right, all_cases)
    );
    println!();
    println!(
        "`current` scores 17/22 right, 0 wrong, 5 abstained (particle_senses.rs, with WordNet)."
    );
    println!(
        "Read the two together: the question is whether being right more often is worth being"
    );
    println!("wrong sometimes, on a panel a learner is meant to trust.");
}

/// The hand-judged labels, copied from `particle_senses.rs::CASES`.
const LABELLED: [(&str, &str); 22] = [
    ("で", "de-place"),
    ("で", "de-place"),
    ("で", "de-means"),
    ("で", "de-material"),
    ("で", "de-cause"),
    ("で", "de-cause"),
    ("で", "de-limit"),
    ("で", "de-copula-te"),
    ("を", "wo-object"),
    ("を", "wo-departure"),
    ("を", "wo-path"),
    ("を", "wo-path"),
    ("に", "ni-existence"),
    ("に", "ni-destination"),
    ("に", "ni-time"),
    ("に", "ni-time"),
    ("に", "ni-recipient"),
    ("に", "ni-purpose"),
    ("が", "ga-subject"),
    ("が", "ga-object-like"),
    ("は", "wa-topic"),
    ("は", "wa-topic"),
];

/// Is `preceding_pos` alive under the analyzer most people run?
///
/// The cue matches against `Prop.pos`, which Lindera fills from a normalized conjugation class.
/// UniDic's ordinary 形状詞 carries no such class here, so the cue may be dead for everyone not
/// running the Ichiran sidecar — costing `de-copula-te` one of its only two cues, at weight 3.
/// This prints what Lindera actually reports, so the answer is observed rather than inferred.
#[test]
#[ignore = "evaluation harness: prints a report, gates nothing"]
fn is_the_na_adjective_cue_alive_under_lindera() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let table = categories();

    for text in [
        "ここは静かで、とても過ごしやすい。",
        "彼は親切で有名です。",
        "この部屋はきれいで広い。",
    ] {
        let Ok(analysis) = analyzer.analyze(text) else {
            continue;
        };
        println!();
        println!("{text}");
        for (at, token) in analysis.tokens.iter().enumerate() {
            let props: Vec<String> = token
                .conjugation
                .iter()
                .flat_map(|conjugation| conjugation.props.iter())
                .map(|prop| prop.pos.clone())
                .collect();
            println!(
                "  [{at}] {:<8} dict={:<8} conjugation={:<4} pos={:?}",
                token.surface,
                token.dict_form,
                if token.conjugation.is_some() {
                    "yes"
                } else {
                    "no"
                },
                props
            );
        }
        if let Some(index) = analysis.tokens.iter().position(|t| t.surface == "で") {
            if let Some(reading) = particles::senses(&analysis.tokens, index, &table) {
                println!("    favoured: {:?}", reading.favoured);
                for sense in &reading.senses {
                    if sense.score > 0 || sense.id == "de-copula-te" {
                        println!(
                            "    {:<16} score={} because={:?}",
                            sense.id, sense.score, sense.because
                        );
                    }
                }
            }
        }
    }
}
