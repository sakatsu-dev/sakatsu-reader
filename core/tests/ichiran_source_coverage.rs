//! Where an Ichiran-derived analysis stops covering its own source.
//!
//! ```text
//! cargo test --release -p sakatsu-core --test ichiran_source_coverage -- --ignored --nocapture
//! ```
//!
//! # Why this exists
//!
//! `hybrid::combine` refuses to assemble a passage whose analyses do not cover their own source,
//! and that one refusal is the sole cause of **every** sentence the three-arm comparison had to
//! exclude — 17 of 259 on UD_Japanese-GSD, 20 of 300 on KWDLC, all of them Ichiran leaving source
//! covered by no token. It is not a hybrid defect. The exclusions are simply where an existing
//! standalone defect became visible, because nothing else in the tree checks the property.
//!
//! A reader on the Ichiran or the Auto path is shown those characters and cannot select them, look
//! them up or get furigana for them, and nothing says so.
//!
//! # What it separates, and what it cannot
//!
//! | Stage | What a gap there would mean |
//! |---|---|
//! | the sidecar's own response | Ichiran did not return the text, and the adapter cannot invent it |
//! | `ichiran::to_analysis` | the text came back and grounding dropped it |
//! | `Pipeline::display` | joining a compound or a name lost it after grounding |
//!
//! **The first two cannot be told apart from outside the crate.** `ichiran::server::analyze` is
//! `pub(super)`, so a test sees what the adapter produced and never the JSON behind it. What this
//! test does establish is the fork that decides where to look: whether the gap is already present
//! when `IchiranAnalyzer::analyze` returns, or appears only once the display pipeline has run.
//! Anything finer needs a fixture, and `core/tests/fixtures/ichiran/` is where those live.
//!
//! # The predicate
//!
//! `hybrid::validate` — the same function whose `Uncovered` fault produced the 37 exclusions, not a
//! re-implementation of it. A re-implementation could agree with the report while disagreeing with
//! the code that actually rejects the sentences.
//!
//! # Licences
//!
//! GSD is CC BY-SA and its text may be shown. **KWDLC is consult-never-import** (`project notes` §8),
//! so its arm reports counts and nothing else — no surface, no fragment, no character.

mod common;

#[path = "common/runner.rs"]
mod runner;

use std::collections::BTreeMap;

use runner::{Outcome, Profile, Requirement, Run};
use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer};
use sakatsu_core::hybrid::{self, SpanFault};
use sakatsu_core::pipeline::Pipeline;
use sakatsu_core::{study, Analysis};

/// The uncovered stretches an analysis has, by the production predicate.
fn gaps(analysis: &Analysis) -> Vec<(usize, usize, String)> {
    hybrid::validate(analysis)
        .into_iter()
        .filter_map(|fault| match fault {
            SpanFault::Uncovered { start, end, text } => Some((start, end, text)),
            _ => None,
        })
        .collect()
}

/// Every other way an analysis can fail its span contract, named. A gap is not the only fault, and
/// reporting only gaps would hide an ungrounded token, which covers nothing either.
fn other_faults(analysis: &Analysis) -> BTreeMap<&'static str, usize> {
    let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    for fault in hybrid::validate(analysis) {
        let name = match fault {
            SpanFault::Uncovered { .. } => continue,
            SpanFault::Ungrounded { .. } => "ungrounded token",
            SpanFault::OutOfBounds { .. } => "span out of bounds",
            SpanFault::NotACharBoundary { .. } => "span off a char boundary",
            SpanFault::SurfaceDisagrees { .. } => "surface disagrees with span",
            SpanFault::OutOfOrder { .. } => "tokens out of order or overlapping",
        };
        *kinds.entry(name).or_default() += 1;
    }
    kinds
}

/// Which stage a sentence's gap is attributable to.
#[derive(Default)]
struct Where {
    /// Present the moment `IchiranAnalyzer::analyze` returned — the sidecar or `to_analysis`.
    before_pipeline: usize,
    /// Absent then, present after `Pipeline::display` — joining lost it.
    introduced_by_pipeline: usize,
    /// Present before and gone after — the pipeline closed it, which is worth knowing.
    closed_by_pipeline: usize,
}

fn ichiran_on_the_session_route() -> (IchiranAnalyzer, bool, String) {
    let engine = IchiranAnalyzer::new();
    sakatsu_core::analyzer::ichiran::server::set_persistent(true);
    engine.warm();
    let answered = engine.analyze("これって").is_ok();
    let route = sakatsu_core::analyzer::ichiran::server::last_route();
    (
        engine,
        answered && route.is_session(),
        route.name().to_string(),
    )
}

/// **UD_Japanese-GSD — the 17, traced to a stage and shown.**
#[test]
#[ignore = "needs scripts/fetch-ud-japanese.ps1, the dictionaries and Ichiran"]
fn where_gsd_sentences_lose_their_source() {
    let dictionaries = common::installed_dictionaries();
    assert!(!dictionaries.is_empty(), "the dictionaries are required");
    let (ichiran, session, route) = ichiran_on_the_session_route();
    assert!(session, "the session route is required; got {route}");
    let pipeline = Pipeline::new(&dictionaries);

    // The three-arm comparison's sample, reproduced exactly, so these are its sentences.
    let sentences = common::gold::test_and_dev();
    assert!(!sentences.is_empty(), "the treebank is required");
    let cap: usize = std::env::var("SAKATSU_UD_ICHIRAN")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(300);
    let stride = sentences.len().div_ceil(cap.max(1)).max(1);

    let mut placed = Where::default();
    let mut examined = 0usize;
    let mut analyzer_refused = 0usize;
    let mut characters: BTreeMap<char, usize> = BTreeMap::new();
    let mut leading = 0usize;
    let mut interior = 0usize;
    let mut trailing = 0usize;
    let mut faults: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut shown: Vec<String> = Vec::new();

    for sentence in sentences.iter().step_by(stride) {
        let Ok(raw) = ichiran.analyze(&sentence.text) else {
            analyzer_refused += 1;
            continue;
        };
        examined += 1;
        let before = gaps(&raw);
        let mut displayed = raw.clone();
        pipeline.display(&mut displayed);
        let after = gaps(&displayed);

        for (kind, count) in other_faults(&displayed) {
            *faults.entry(kind).or_default() += count;
        }

        match (before.is_empty(), after.is_empty()) {
            (true, true) => continue,
            (false, true) => {
                placed.closed_by_pipeline += 1;
                continue;
            }
            (false, false) => placed.before_pipeline += 1,
            (true, false) => placed.introduced_by_pipeline += 1,
        }

        let length = displayed.text.len();
        for (start, end, text) in &after {
            if *start == 0 {
                leading += 1;
            } else if *end == length {
                trailing += 1;
            } else {
                interior += 1;
            }
            for character in text.chars() {
                *characters.entry(character).or_default() += 1;
            }
        }
        if shown.len() < 20 {
            let stage = if before.is_empty() {
                "pipeline"
            } else {
                "adapter-or-sidecar"
            };
            let lost: Vec<String> = after
                .iter()
                .map(|(start, end, text)| format!("[{start}..{end}] {text:?}"))
                .collect();
            shown.push(format!("{:<19}{}  {}", stage, sentence.id, lost.join(" ")));
        }
    }

    println!("\n=== UD_Japanese-GSD: where the source goes missing ===");
    println!("  sentences examined           {examined}");
    if analyzer_refused > 0 {
        println!("  ichiran returned no analysis {analyzer_refused}");
    }
    println!("\n  attributed to a stage");
    println!(
        "    already uncovered when the analyzer returned   {}  (the sidecar's response, or \
         `to_analysis` grounding — not separable from here)",
        placed.before_pipeline
    );
    println!(
        "    uncovered only after Pipeline::display          {}",
        placed.introduced_by_pipeline
    );
    println!(
        "    uncovered before, covered after                 {}  (the pipeline closed it)",
        placed.closed_by_pipeline
    );
    println!("\n  where in the sentence the gap sits");
    println!("    leading {leading}   interior {interior}   trailing {trailing}");
    println!("\n  characters left uncovered, most frequent first");
    let mut ranked: Vec<(&char, &usize)> = characters.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (character, count) in ranked.iter().take(25) {
        println!(
            "    {count:>4}x  {character:?}  U+{:04X}",
            **character as u32
        );
    }
    if faults.is_empty() {
        println!("\n  no other span fault of any kind");
    } else {
        println!("\n  other span faults, which a gap count alone would have hidden");
        for (kind, count) in &faults {
            println!("    {count:>4}x  {kind}");
        }
    }
    println!("\n  the sentences");
    for line in &shown {
        println!("    {line}");
    }
}

/// **What the adapter is handed around a gap.**
///
/// The stage attribution above says the gap is present the moment `IchiranAnalyzer::analyze`
/// returns, which leaves two possibilities it cannot separate: the sidecar returned no segment for
/// the character, or it returned one and `to_analysis` failed to ground it. This narrows that as
/// far as the crate boundary allows — if no token carries the character in any form, grounded or
/// not, then nothing arrived for the adapter to lose.
///
/// The inputs are UD_Japanese-GSD sentences, CC BY-SA, quoted from the installed treebank rather
/// than composed here.
#[test]
#[ignore = "needs the dictionaries and Ichiran"]
fn what_surrounds_an_uncovered_separator() {
    let dictionaries = common::installed_dictionaries();
    assert!(!dictionaries.is_empty(), "the dictionaries are required");
    let (ichiran, session, route) = ichiran_on_the_session_route();
    assert!(session, "the session route is required; got {route}");

    // Opening clauses of test-s174 and test-s162 of UD_Japanese-GSD.
    for text in ["米中戦略・経済対話初日", "景観・社会・文化"] {
        let Ok(analysis) = ichiran.analyze(text) else {
            println!("\n  ichiran returned nothing for this line");
            continue;
        };
        println!("\n  source {text:?} ({} bytes)", text.len());
        println!("    idx  start..end  surface");
        for (index, token) in analysis.tokens.iter().enumerate() {
            let start = token
                .start
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into());
            let end = token
                .end
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into());
            println!("    {index:>3}  {start:>5}..{end:<5} {:?}", token.surface);
        }
        let carried = analysis
            .tokens
            .iter()
            .filter(|token| token.surface.contains('\u{30fb}'))
            .count();
        println!(
            "    tokens whose surface contains the separator: {carried}  \
             (0 means nothing arrived for the adapter to lose)"
        );
        for (start, end, lost) in gaps(&analysis) {
            println!("    uncovered [{start}..{end}] {lost:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The production validation boundary, over the capture corpus.
// ---------------------------------------------------------------------------------------------

// The enumeration moved to `common/captures.rs` so the recovery harness uses the same rule
// rather than a second one that skipped failures silently.
use common::captures::captures;

/// **Does every capture survive the boundary the reader's passages actually cross?**
///
/// `study::analyse` is that boundary: the analyzer, the display pipeline, then the shared span
/// contract. Nothing else in the tree asks this of the capture corpus — `segmentation_gold` calls
/// `analyzer.analyze()` and `common::through_pipeline` directly, so it never reaches the contract,
/// and the coverage arms above stop at `Pipeline::display` for the same reason.
///
/// That matters because the contract **fails closed**: analyzer selection happens once per session,
/// not per passage, so a rejected analysis gives the reader an error rather than a degraded one. A
/// character this corpus contains and the contract refuses is a passage the reader cannot read.
///
/// **Printing a fault is not a pass.** Every selected case is recorded, missing resources fail the
/// profile rather than skipping it, and the verdict comes from the runner's accounting.
///
/// Holdout captures are included and **labelled**. Validating that a span covers its source is
/// mechanical; it establishes nothing linguistic, and it is reported apart from the investigation.
#[test]
#[ignore = "needs the capture corpus, the dictionaries and Ichiran"]
fn captures_pass_the_production_validation_boundary() {
    let dictionaries = common::installed_dictionaries();
    let (captures, text_only, faults) = captures();
    let built_in = sakatsu_core::analyzer::LinderaAnalyzer::new().ok();
    let (ichiran, session, route) = ichiran_on_the_session_route();

    let engines = ["lindera", "ichiran"];
    let ids: Vec<String> = captures
        .iter()
        .flat_map(|capture| engines.map(|engine| format!("{}:{engine}", capture.id)))
        .collect();

    let mut run = Run::new(Profile::InstalledResources, ids.iter().cloned())
        .engine("lindera, ichiran")
        .needs(
            Requirement::required("the capture corpus", !captures.is_empty())
                .because(format!("{} transcripts with an image", captures.len())),
        )
        // **Enumeration is part of the measurement.** A directory that would not list, a file
        // that would not read, an empty transcript, or a holdout file that could not be parsed
        // each change which cases exist — so none of them may pass as a quiet absence.
        .needs(
            Requirement::required("the corpus enumerated without fault", faults.is_empty())
                .because(if faults.is_empty() {
                    "every discovered input was read".to_string()
                } else {
                    faults.join("; ")
                }),
        )
        .needs(
            Requirement::required("dictionaries", !dictionaries.is_empty())
                .because(format!("{} opened", dictionaries.len())),
        )
        .needs(Requirement::required(
            "the built-in analyzer",
            built_in.is_some(),
        ))
        .needs(
            Requirement::required("ichiran via the session route", session)
                .because(format!("route {route}")),
        );
    for dictionary in &dictionaries {
        run = run.dictionary(format!(
            "{} ({} entries; sha256 {})",
            dictionary.meta().title,
            dictionary.meta().entry_count,
            dictionary
                .content_fingerprint()
                .expect("resource content identity is required")
        ));
    }

    // Diagnosis, kept apart from the verdict above: what the refused passages have in common.
    let mut refused: Vec<String> = Vec::new();
    let mut characters: BTreeMap<char, usize> = BTreeMap::new();
    let mut by_tier: BTreeMap<(&str, &str, &str), usize> = BTreeMap::new();

    for capture in &captures {
        for engine in engines {
            let id = format!("{}:{engine}", capture.id);
            let analysed = match engine {
                "lindera" => built_in
                    .as_ref()
                    .map(|analyzer| study::analyse(&capture.text, analyzer, &dictionaries)),
                _ => Some(study::analyse(&capture.text, &ichiran, &dictionaries)),
            };
            let outcome = match analysed {
                None => Outcome::failed(
                    runner::Stage::Analyzer,
                    "the built-in analyzer did not load, so this case was never measured",
                ),
                Some(Ok(_)) => {
                    *by_tier
                        .entry((capture.tier, capture.partition, engine))
                        .or_default() += 1;
                    Outcome::Passed
                }
                Some(Err(error)) => {
                    // The same passage stopped one step earlier, so the report can say which
                    // characters the contract refused rather than only that it refused.
                    let raw = match engine {
                        "lindera" => built_in
                            .as_ref()
                            .and_then(|analyzer| analyzer.analyze(&capture.text).ok()),
                        _ => ichiran.analyze(&capture.text).ok(),
                    };
                    let mut lost = Vec::new();
                    if let Some(mut analysis) = raw {
                        Pipeline::new(&dictionaries).display(&mut analysis);
                        for (start, end, text) in gaps(&analysis) {
                            for character in text.chars() {
                                *characters.entry(character).or_default() += 1;
                            }
                            lost.push(format!("[{start}..{end}] {text:?}"));
                        }
                    }
                    refused.push(format!(
                        "{id}  {} {}  {error}{}",
                        capture.tier,
                        capture.partition,
                        if lost.is_empty() {
                            String::new()
                        } else {
                            format!("  uncovered {}", lost.join(" "))
                        }
                    ));
                    Outcome::failed(runner::Stage::Analyzer, error.to_string())
                }
            };
            run.record(id, outcome);
        }
    }

    println!("\n=== the capture corpus through `study::analyse` ===");
    println!("  transcripts selected  {}", captures.len());
    let promoted = captures.iter().filter(|c| c.tier == "promoted").count();
    let held = captures.iter().filter(|c| c.partition == "holdout").count();
    println!(
        "    promoted {promoted}   draft {}   of which holdout {held}",
        captures.len() - promoted
    );
    if !faults.is_empty() {
        println!(
            "
  ENUMERATION FAULTS — these fail the run"
        );
        for fault in &faults {
            println!("    {fault}");
        }
    }
    if !text_only.is_empty() {
        println!(
            "  text-only stems excluded for having no image: {}  {:?}",
            text_only.len(),
            text_only
        );
    }
    println!("\n  passed, by tier and partition");
    for ((tier, partition, engine), count) in &by_tier {
        println!("    {tier:<9} {partition:<12} {engine:<8} {count:>4}");
    }
    if refused.is_empty() {
        println!("\n  no passage was refused by the span contract");
    } else {
        println!(
            "\n  DIAGNOSIS — {} passage(s) the contract refused; the reader would see an error",
            refused.len()
        );
        for line in &refused {
            println!("    {line}");
        }
        println!("\n  characters left uncovered, most frequent first");
        let mut ranked: Vec<(&char, &usize)> = characters.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (character, count) in ranked.iter().take(25) {
            println!(
                "    {count:>4}x  {character:?}  U+{:04X}",
                **character as u32
            );
        }
    }
    println!(
        "\n  Mechanical source validation only. It establishes nothing linguistic, and a holdout \
         capture appearing here is not a reading judgment."
    );

    run.accounted_for().expect_success();
}
