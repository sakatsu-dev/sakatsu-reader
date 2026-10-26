//! **Does the furigana the reader shows sound right?** Scored against an independent benchmark.
//!
//! ```text
//! pwsh -File scripts/fetch-joyo-yomi.ps1                  # once: MIT, pinned, to the evaluation folder (`common::joyo::dir`)
//! $env:SAKATSU_DICT_DIR = "$env:SAKATSU_EVAL_DIR\dictionaries"  # isolated copy, kanji table beside it
//! cargo test --release -p sakatsu-core --test joyo_readings -- --ignored --nocapture --test-threads=1
//! # Ichiran too: record its answers once (JOYO_RECORD_ICHIRAN=1), then replay (JOYO_ICHIRAN_RAW=<artifact>)
//! ```
//!
//! # What is scored, and what is not
//!
//! Every development row whose target kanji occurs exactly once (`common/joyo.rs`) runs through the
//! production path — `study::run` then `study::from_analysis` — and the ruby **over the target
//! kanji** is read back from `StudyView.ruby`, which is what a learner sees. Three results, kept
//! apart:
//!
//! 1. **Pronunciation agreement** — the reading, in the benchmark's phonetic convention, against the
//!    marked segment. Lossy by design (せい ≡ せー), so this says the reading sounds right and
//!    nothing about its spelling. The marked segment spans the kanji **and its written
//!    okurigana** (固める → `<カタメ>`), so the reading is extended by the text's own kana before
//!    comparing — see `joyo::sounds_like`.
//! 2. **KANJIDIC compatibility** — a consistency check. Per-character furigana is generated from the
//!    same table, so it passes by construction; it is reported to show that, not as accuracy.
//! 3. **Furigana-spelling accuracy — unavailable.** The benchmark has no spelling annotation.
//!
//! **Ruby is never divided.** A per-character result exists only where the furigana generator gave
//! the target its own run. Where it set one reading over the whole word, the item is *unsplittable*
//! and is neither right nor wrong — the target's place inside that reading is not established.
//!
//! The holdout third is sealed and not scored. No reading behaviour changes because of this file;
//! `heteronym_readings.rs` remains the gate.

mod common;

use std::collections::{BTreeMap, HashSet};

use common::joyo::{self, Unaligned};
use sakatsu_core::analyzer::ichiran::{self, raw};
use sakatsu_core::analyzer::{AnalyzeError, Analyzer, IchiranAnalyzer, Traced};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::dict::KanjiDic;
use sakatsu_core::model::Analysis;
use sakatsu_core::study::{self, AnalysisOptions, Resources};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// What the reader showed over one target kanji.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Outcome {
    /// A reading over the kanji alone, sounding as the benchmark says.
    Right,
    /// A reading over the kanji alone, sounding otherwise.
    Wrong,
    /// One reading over a word containing the kanji. Not divided, not scored.
    Unsplittable,
    /// No reading shown over the kanji at all.
    NoReading,
    /// No token covers the target, or its surface is not the text under it.
    NotCovered,
    /// The analysis failed.
    Failed,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Outcome::Right => "right",
            Outcome::Wrong => "wrong",
            Outcome::Unsplittable => "unsplittable (whole-word ruby)",
            Outcome::NoReading => "no reading shown",
            Outcome::NotCovered => "target not covered by one token",
            Outcome::Failed => "analysis failed",
        }
    }
}

/// Ichiran's recorded answer for one sentence, converted as the adapter converts a live one.
struct Replayed(String);

impl Analyzer for Replayed {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.analyze_traced(text).map(|traced| traced.analysis)
    }
    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        let text = sakatsu_core::analyzer::lines::normalise_breaks(text);
        let text = text.trim();
        let segments = raw::parse(&self.0)
            .map_err(|error| AnalyzeError::BadOutput(format!("unexpected JSON: {error}")))?;
        Ok(Traced {
            analysis: ichiran::to_analysis(text, &segments),
            raw: None,
        })
    }
    fn engine(&self) -> &str {
        "ichiran"
    }
}

/// One item's result, with what a reviewer needs to see why.
struct Scored {
    outcome: Outcome,
    /// How a right answer was matched: plain, with written okurigana, or with a boundary mora.
    matched: Option<joyo::Matched>,
    shown: Option<String>,
    attested: Option<bool>,
    ambiguous: Option<bool>,
}

/// Read the ruby over the target kanji at byte offset `at`. Never divides a whole-word reading.
fn score_one(
    item: &joyo::Item,
    at: usize,
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
    kanji_table: &KanjiDic,
) -> Scored {
    let (text, kanji, target) = (item.text.as_str(), item.kanji, item.target.as_str());
    let after_marker = item
        .pron
        .split_once('>')
        .map(|(_, after)| after)
        .unwrap_or_default();
    let scored = |outcome| Scored {
        outcome,
        matched: None,
        shown: None,
        attested: None,
        ambiguous: None,
    };
    let run = match study::run(text, analyzer, dictionaries, AnalysisOptions::PRODUCTION) {
        Ok(run) => run,
        Err(_) => return scored(Outcome::Failed),
    };
    // The analysis's text is the input trimmed; offsets are into it, so map `at` across the trim.
    let leading = text.len() - text.trim_start().len();
    let Some(at) = at.checked_sub(leading) else {
        return scored(Outcome::NotCovered);
    };
    let view = study::from_analysis(
        run.analysis,
        Resources {
            analyzer,
            dictionaries,
            kanji: kanji_table,
            store: None,
        },
    );
    let Some(index) = view.analysis.tokens.iter().position(|token| {
        matches!((token.start, token.end), (Some(start), Some(end)) if start <= at && at < end)
    }) else {
        return scored(Outcome::NotCovered);
    };
    let token = &view.analysis.tokens[index];
    let start = token.start.expect("matched above");
    if view
        .analysis
        .text
        .get(start..token.end.expect("matched above"))
        != Some(token.surface.as_str())
    {
        return scored(Outcome::NotCovered);
    }
    let ambiguous = Some(token.reading_ambiguous);
    // The kana the text writes straight after the target, for `joyo::sounds_like`.
    let following = view
        .analysis
        .text
        .get(at + kanji.len_utf8()..)
        .unwrap_or_default();
    let mut offset = start;
    for ruby in &view.ruby[index] {
        let end = offset + ruby.text.len();
        if offset <= at && at < end {
            let alone = ruby.text.chars().eq(std::iter::once(kanji));
            return match (&ruby.reading, alone) {
                (None, _) => Scored {
                    ambiguous,
                    ..scored(Outcome::NoReading)
                },
                (Some(_), false) => Scored {
                    ambiguous,
                    ..scored(Outcome::Unsplittable)
                },
                (Some(reading), true) => Scored {
                    outcome: match joyo::sounds_like(reading, following, target, after_marker) {
                        Some(_) => Outcome::Right,
                        None => Outcome::Wrong,
                    },
                    matched: joyo::sounds_like(reading, following, target, after_marker),
                    shown: Some(reading.clone()),
                    attested: Some(sakatsu_core::dict::attested(kanji, reading, kanji_table)),
                    ambiguous,
                },
            };
        }
        offset = end;
    }
    scored(Outcome::NotCovered)
}

/// Ichiran's raw answers: replayed from `JOYO_ICHIRAN_RAW`, or asked once under
/// `JOYO_RECORD_ICHIRAN=1` and published privately. `None` when neither is asked for.
fn ichiran_answers(
    texts: &[(String, String)],
    evaluation: &std::path::Path,
) -> Option<BTreeMap<String, String>> {
    if let Some(path) = std::env::var_os("JOYO_ICHIRAN_RAW") {
        let recorded: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("the raw artifact")).expect("JSON");
        assert_eq!(
            recorded["data_sha256"].as_str(),
            Some(joyo::DATA_SHA256),
            "the recorded answers are for a different revision"
        );
        return Some(
            recorded["raws"]
                .as_object()
                .expect("raws")
                .iter()
                .map(|(key, raw)| (key.clone(), raw.as_str().expect("a raw answer").to_string()))
                .collect(),
        );
    }
    if std::env::var("JOYO_RECORD_ICHIRAN").as_deref() != Ok("1") {
        return None;
    }
    // **Resumable, and bounded per run.** About 8,500 answers take over an hour, longer than one
    // background job may run. Answers are checkpointed beside the artifacts every 200, a run
    // continues from the checkpoint, and each run stops itself after `JOYO_RECORD_BUDGET_SECS`
    // (default 1,200) — failing with how far it got, so a partial recording is never scored.
    let checkpoint = evaluation.join("joyo-ichiran-raw.partial.json");
    let mut raws: BTreeMap<String, String> = std::fs::read(&checkpoint)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(|kept| kept["data_sha256"].as_str() == Some(joyo::DATA_SHA256))
        .and_then(|kept| {
            kept["raws"].as_object().map(|raws| {
                raws.iter()
                    .filter_map(|(key, raw)| Some((key.clone(), raw.as_str()?.to_string())))
                    .collect()
            })
        })
        .unwrap_or_default();
    let resumed = raws.len();
    let save = |raws: &BTreeMap<String, String>| {
        let body =
            json!({"revision": joyo::REVISION, "data_sha256": joyo::DATA_SHA256, "raws": raws});
        let pending = checkpoint.with_extension("json.pending");
        std::fs::write(&pending, body.to_string()).expect("write the checkpoint");
        std::fs::rename(&pending, &checkpoint).expect("replace the checkpoint");
    };
    let budget = std::time::Duration::from_secs(
        std::env::var("JOYO_RECORD_BUDGET_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1200),
    );
    let started = std::time::Instant::now();
    let ichiran = IchiranAnalyzer::new();
    ichiran::server::set_persistent(true);
    ichiran.warm();
    assert!(
        ichiran.analyze("これって").is_ok(),
        "Ichiran must answer to record"
    );
    let mut unanswered = Vec::new();
    let mut since_save = 0;
    for (key, text) in texts {
        if raws.contains_key(key) {
            continue;
        }
        if started.elapsed() > budget {
            save(&raws);
            panic!(
                "recording incomplete: {} of {} answered ({} resumed, {} this run); rerun to continue",
                raws.len(),
                texts.len(),
                resumed,
                raws.len() - resumed
            );
        }
        match ichiran.analyze_traced(text) {
            Ok(Traced { raw: Some(raw), .. }) => {
                raws.insert(key.clone(), raw);
                since_save += 1;
                if since_save >= 200 {
                    save(&raws);
                    since_save = 0;
                }
            }
            Ok(_) => unanswered.push(format!("{key}: no raw output")),
            Err(error) => unanswered.push(format!("{key}: {error}")),
        }
    }
    save(&raws);
    assert!(
        unanswered.is_empty(),
        "Ichiran did not answer: {unanswered:?}"
    );
    let path = common::reference::publish(
        evaluation,
        "joyo-ichiran-raw",
        "json",
        &json!({"revision": joyo::REVISION, "data_sha256": joyo::DATA_SHA256, "raws": raws})
            .to_string(),
    )
    .expect("publish the raw answers");
    println!(
        "  ichiran raw  {} (private; {} answers, {resumed} resumed from the checkpoint)",
        path.display(),
        raws.len()
    );
    Some(raws)
}

fn rate(part: usize, whole: usize) -> String {
    if whole == 0 {
        "—".to_string()
    } else {
        format!(
            "{part}/{whole} ({:.1}%)",
            100.0 * part as f64 / whole as f64
        )
    }
}

#[test]
#[ignore = "needs the fetched benchmark, the dictionaries and the kanji table; Ichiran only when asked"]
fn the_furigana_shown_over_a_target_kanji_is_scored_against_the_benchmark() {
    let (rows, malformed) = joyo::load().unwrap_or_else(|error| panic!("{error}"));
    let total = rows.len() + malformed.len();
    let split_digest = joyo::split_digest(&rows);
    let dev = joyo::dev(rows);
    let dictionaries = common::installed_dictionaries();
    assert!(!dictionaries.is_empty(), "no dictionaries installed");
    let kanji_path = common::dictionaries_dir()
        .parent()
        .expect("the dictionaries directory has a parent")
        .join("kanji")
        .join("kanjidic.tsv");
    let kanji_table = KanjiDic::load(&kanji_path);
    assert!(
        !kanji_table.is_empty(),
        "no KanjiDic at {}",
        kanji_path.display()
    );
    let evaluation = common::equivalence::evaluation_dir();

    // ---- alignment, accounted for in full before anything is scored.
    let mut unaligned: BTreeMap<Unaligned, usize> = BTreeMap::new();
    for (_, reason) in &malformed {
        *unaligned.entry(*reason).or_default() += 1;
    }
    let aligned: Vec<(&joyo::Item, usize)> = dev
        .iter()
        .filter_map(|row| match row.at {
            Ok(at) => Some((&row.item, at)),
            Err(reason) => {
                *unaligned.entry(reason).or_default() += 1;
                None
            }
        })
        .collect();
    let dev_unaligned = dev.len() - aligned.len();
    assert_eq!(
        aligned.len() + dev_unaligned,
        dev.len(),
        "every dev row is accounted for"
    );

    // ---- engines.
    let lindera = common::built_in().expect("compiled in");
    let texts: Vec<(String, String)> = aligned
        .iter()
        .map(|(item, _)| (item.key.clone(), item.text.clone()))
        .collect();
    let ichiran_raws = ichiran_answers(&texts, &evaluation);

    let mut summary = serde_json::Map::new();
    let mut items_out: Vec<Value> = Vec::new();
    let mut engines: Vec<&str> = vec!["lindera"];
    if ichiran_raws.is_some() {
        engines.push("ichiran");
    }
    for engine in &engines {
        let mut counts: BTreeMap<Outcome, usize> = BTreeMap::new();
        let (mut attested, mut per_character) = (0usize, 0usize);
        let (mut ambiguous_right, mut ambiguous_wrong) = (0usize, 0usize);
        let (mut via_okurigana, mut via_boundary) = (0usize, 0usize);
        let mut seen = HashSet::new();
        for (item, at) in &aligned {
            let replayed;
            let analyzer: &dyn Analyzer = match *engine {
                "ichiran" => {
                    let raws = ichiran_raws.as_ref().expect("asked for");
                    let Some(raw) = raws.get(&item.key) else {
                        panic!("{}: no recorded Ichiran answer", item.key);
                    };
                    replayed = Replayed(raw.clone());
                    &replayed
                }
                _ => &lindera,
            };
            let scored = score_one(item, *at, analyzer, &dictionaries, &kanji_table);
            *counts.entry(scored.outcome).or_default() += 1;
            if let Some(matched) = scored.matched {
                via_okurigana += usize::from(matched.okurigana > 0);
                via_boundary += usize::from(matched.boundary > 0);
            }
            if matches!(scored.outcome, Outcome::Right | Outcome::Wrong) {
                per_character += 1;
                attested += usize::from(scored.attested == Some(true));
                match (scored.outcome, scored.ambiguous) {
                    (Outcome::Right, Some(true)) => ambiguous_right += 1,
                    (Outcome::Wrong, Some(true)) => ambiguous_wrong += 1,
                    _ => {}
                }
            }
            assert!(seen.insert(item.key.clone()), "{} scored twice", item.key);
            items_out.push(json!({
                "engine": engine,
                "key": item.key,
                "outcome": scored.outcome.name(),
                "target": item.target,
                "shown": scored.shown,
                "attested": scored.attested,
                "ambiguous": scored.ambiguous,
            }));
        }
        let scored_total: usize = counts.values().sum();
        assert_eq!(
            scored_total,
            aligned.len(),
            "{engine}: every aligned item has an outcome"
        );
        let right = counts.get(&Outcome::Right).copied().unwrap_or(0);
        let wrong = counts.get(&Outcome::Wrong).copied().unwrap_or(0);

        println!("\n=== {engine}: the ruby over the target kanji, development split ===");
        for (outcome, count) in &counts {
            println!("  {:<36} {count:>6}", outcome.name());
        }
        println!(
            "  pronunciation agreement (per-character)  {}",
            rate(right, right + wrong)
        );
        println!(
            "  KANJIDIC compatibility (consistency)     {}  — by construction; not accuracy",
            rate(attested, per_character)
        );
        println!(
            "    of the right: {via_okurigana} matched with written okurigana, {via_boundary} with one boundary mora"
        );
        println!("  furigana-spelling accuracy               unavailable — no spelling gold");
        println!(
            "  reading_ambiguous on right / wrong       {} / {}",
            rate(ambiguous_right, right),
            rate(ambiguous_wrong, wrong)
        );
        summary.insert(
            engine.to_string(),
            json!({
                "outcomes": counts.iter().map(|(o, c)| (o.name().to_string(), json!(c))).collect::<serde_json::Map<_, _>>(),
                "pronunciation_right": right,
                "pronunciation_wrong": wrong,
                "right_via_okurigana": via_okurigana,
                "right_via_boundary_mora": via_boundary,
                "kanjidic_compatible": attested,
                "per_character": per_character,
                "ambiguous_on_right": ambiguous_right,
                "ambiguous_on_wrong": ambiguous_wrong,
            }),
        );
    }

    println!("\n=== coverage ===");
    println!("  benchmark rows            {total}");
    println!("  development rows          {}", dev.len());
    println!(
        "  aligned (kanji once)      {}",
        rate(aligned.len(), dev.len())
    );
    for (reason, count) in &unaligned {
        // A malformed key has no kanji to split by, so those rows are counted across both splits.
        let scope = match reason {
            Unaligned::MalformedKey => " (both splits — no kanji to split by)",
            _ => "",
        };
        println!("  unaligned: {:<24} {count}{scope}", reason.name());
    }
    println!("  holdout                   sealed, not scored");

    let fingerprints = common::equivalence::fingerprints(
        dictionaries
            .iter()
            .map(|d| (d.meta().title.clone(), d.content_fingerprint())),
    )
    .unwrap_or_else(|error| panic!("a dictionary could not be fingerprinted: {error}"));
    let identity = json!({
        "revision": joyo::REVISION,
        "data_sha256": joyo::DATA_SHA256,
        "split_digest": split_digest,
        "split": "by kanji, sha256(kanji) % 3 == 0 is holdout; holdout sealed",
        "executable": common::reference::executable_identity(),
        "dictionaries": fingerprints,
        "kanji_sha256": sakatsu_core::hashing::hex(Sha256::digest(std::fs::read(&kanji_path).expect("kanji table"))),
        "engines": engines,
        "rows": total,
        "dev_rows": dev.len(),
        "aligned": aligned.len(),
        "unaligned": unaligned.iter().map(|(r, c)| (r.name().to_string(), json!(c))).collect::<serde_json::Map<_, _>>(),
    });
    let path = common::reference::publish(
        &evaluation,
        "joyo-readings",
        "json",
        &json!({"identity": identity, "summary": summary, "items": items_out}).to_string(),
    )
    .expect("publish the results");
    println!(
        "\n  identity  {}",
        serde_json::to_string(&identity).unwrap_or_default()
    );
    println!("  artifact  {} (private)", path.display());
    assert!(
        aligned.len() > 1000,
        "too few aligned items to mean anything: {}",
        aligned.len()
    );
}
