//! What each arm would actually tell a reader, for the twelve reviewed cases.
//!
//! ```text
//! cargo test --release -p sakatsu-core --test hybrid_assistance_audit -- --ignored --nocapture
//! ```
//!
//! # Why this is separate from the boundary scores
//!
//! `hybrid_reference.rs` measures agreement with an annotation standard. That is not what a reader
//! experiences, and it is not what Hybrid has to win on: always-consulting-both costs Ichiran-class
//! latency, so its case has to be **better assistance**. This records the assistance.
//!
//! # Five columns, because they answer different questions
//!
//! | Column | Question |
//! |---|---|
//! | entry existence | did *anything* answer? |
//! | retrieval scope | did it answer the **whole token** or fall back to a component? |
//! | displayed senses | what would the panel show, in what order? |
//! | applicability | is there evidence the leading sense fits **this** sentence? |
//! | uncertainty | what remains unsettled? |
//!
//! **An entry containing a sense establishes availability, not that the sense applies here.** The
//! first three columns are derived by this test from the pinned dictionaries and grammar packs. The
//! last two cannot be: no reference annotates these sentences, so they are authored analysis and are
//! marked as such in the artifact rather than presented as measurement.
//!
//! # Assistance is recomputed against the assembled passage
//!
//! A token's neighbours decide its contextual ranking, and Hybrid's assembly changes neighbours. So
//! the Hybrid column comes from [`hybrid::assistance_of`] over the assembled tokens, never from the
//! candidate the selector happened to pick.

mod common;

#[path = "common/runner.rs"]
mod runner;

use std::collections::BTreeMap;

use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, LinderaAnalyzer};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::pipeline::Pipeline;
use sakatsu_core::{hybrid, Analysis};
use serde_json::json;
use sha2::{Digest, Sha256};

/// One reviewed case: the sentence, and the stretch the reviewer was asked about.
struct Case {
    number: usize,
    focus: String,
    sentence: String,
}

/// Read the cases from the completed review, so this audit and the review describe the same twelve.
fn cases() -> Vec<Case> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation/_review-case-sentences.tsv");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the review cases must be readable at {path:?}: {error}"));
    raw.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            Some(Case {
                number: fields.next()?.trim().parse().ok()?,
                focus: fields.next()?.to_string(),
                sentence: fields.next()?.to_string(),
            })
        })
        .collect()
}

/// The assistance for whichever token carries `focus`, or the tokens that divide it.
fn at_focus(
    analysis: &Analysis,
    assistance: &[hybrid::TokenAssistance],
    focus: &str,
) -> Vec<serde_json::Value> {
    // The focus may be one token or several; take every token whose surface lies inside it, and
    // fall back to any token containing the focus when the analyzer joined more than was asked.
    let inside: Vec<usize> = analysis
        .tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| focus.contains(token.surface.as_str()) && !token.surface.is_empty())
        .map(|(index, _)| index)
        .collect();
    let chosen = if inside.is_empty() {
        analysis
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.surface.contains(focus))
            .map(|(index, _)| index)
            .collect()
    } else {
        inside
    };
    chosen
        .into_iter()
        .filter_map(|index| {
            let token = analysis.tokens.get(index)?;
            let part = assistance.get(index)?;
            Some(json!({
                "surface": token.surface,
                "reading": token.reading,
                "dictionary_form": token.dict_form,
                // Column 1 — did anything answer at all?
                "entry_exists": !part.displayed.is_empty() || part.name.is_some(),
                // Column 2 — the whole token, or one of its parts?
                "retrieval_scope": match &part.answer {
                    hybrid::Answer::Nothing => "nothing retrieved".to_string(),
                    hybrid::Answer::AnalyzerOnly => "the analyzer's own senses".to_string(),
                    hybrid::Answer::Dictionary { source } => format!("whole token, from {source}"),
                    hybrid::Answer::Component { source, component } =>
                        format!("COMPONENT 「{component}」, from {source}"),
                    hybrid::Answer::NameOnly { source } => format!("name entry only, from {source}"),
                },
                "a_dictionary_answered_the_whole_token": part.whole_answered,
                // Column 3 — what the panel would show, in order.
                "displayed_senses": part.displayed.iter().map(|sense| json!({
                    "sequence": sense.sequence,
                    "tags": sense.tags,
                    "glosses": sense.glosses.join("; "),
                })).collect::<Vec<_>>(),
                "name": part.name,
                "expression_offer": part.expression_offer.as_ref().map(|offer| json!({
                    "expression": offer.expression,
                    "pattern": offer.pattern_id,
                    "covers": [offer.start, offer.end],
                })),
                "grammar": part.grammar,
                "lookup_failed": part.lookup_failed,
                "expression_failed": part.expression_failed,
            }))
        })
        .collect()
}

#[test]
#[ignore = "needs the installed dictionaries and Ichiran"]
fn audit_the_assistance_for_the_reviewed_cases() {
    let dictionaries: Vec<Dictionary> = common::installed_dictionaries();
    assert!(!dictionaries.is_empty(), "the dictionaries are required");
    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let ichiran = IchiranAnalyzer::new();
    sakatsu_core::analyzer::ichiran::server::set_persistent(true);
    ichiran.warm();
    let session = ichiran.analyze("これって").is_ok()
        && sakatsu_core::analyzer::ichiran::server::last_route().is_session();
    assert!(
        session,
        "the session route is required for a three-arm audit"
    );

    let pipeline = Pipeline::new(&dictionaries);
    let cases = cases();
    assert_eq!(cases.len(), 12, "the review asked twelve");

    // One analysis per distinct sentence, reused across the cases that share it.
    let mut analysed: BTreeMap<String, (Analysis, Analysis, Option<Analysis>, Option<String>)> =
        BTreeMap::new();
    for case in &cases {
        if analysed.contains_key(&case.sentence) {
            continue;
        }
        let mut left = lindera.analyze(&case.sentence).expect("lindera analyses");
        pipeline.display(&mut left);
        let mut right = ichiran.analyze(&case.sentence).expect("ichiran analyses");
        pipeline.display(&mut right);
        let (assembled, refusal) = match hybrid::combine(&left, &right, &dictionaries) {
            Ok((_, assembled)) => (Some(assembled), None),
            Err(reason) => (None, Some(format!("{reason:?}"))),
        };
        analysed.insert(case.sentence.clone(), (left, right, assembled, refusal));
    }

    let mut records = Vec::new();
    for case in &cases {
        let (left, right, assembled, refusal) = &analysed[&case.sentence];
        let lindera_assistance = hybrid::assistance_of(left, &dictionaries);
        let ichiran_assistance = hybrid::assistance_of(right, &dictionaries);
        // **Recomputed against the assembled passage**, not carried from the winning candidate:
        // the neighbours differ, and neighbours decide contextual ranking.
        let hybrid_assistance = assembled
            .as_ref()
            .map(|assembled| hybrid::assistance_of(assembled, &dictionaries));

        records.push(json!({
            "case": case.number,
            "focus": case.focus,
            "sentence": case.sentence,
            "arms": {
                "lindera": at_focus(left, &lindera_assistance, &case.focus),
                "ichiran": at_focus(right, &ichiran_assistance, &case.focus),
                "hybrid": match (assembled, &hybrid_assistance) {
                    (Some(assembled), Some(assistance)) =>
                        json!(at_focus(assembled, assistance, &case.focus)),
                    _ => json!({ "not_assembled": refusal }),
                },
            },
            // Columns 4 and 5 are authored, not derived. Left null here on purpose: filling them
            // from this test would present interpretation as measurement.
            "applicability_evidence": serde_json::Value::Null,
            "remaining_uncertainty": serde_json::Value::Null,
        }));
    }

    let document = json!({
        "what_this_is": "Assistance for the twelve reviewed cases, per arm. Columns 1-3 are derived \
                         from the pinned dictionaries and grammar packs. Columns 4 and 5 are \
                         authored analysis and are null here so that interpretation is never \
                         mistaken for measurement.",
        "ruleset": hybrid::RULESET,
        "source_tree": runner::source_digest(),
        "dictionaries": dictionaries.iter().map(|dictionary| json!({
            "title": dictionary.meta().title,
            "entries": dictionary.meta().entry_count,
            "sha256": dictionary.content_fingerprint().expect("content identity"),
        })).collect::<Vec<_>>(),
        "caveat": "An entry containing a sense establishes availability, not that the sense applies \
                   in this sentence. No reference annotates these sentences.",
        "cases": records,
    });

    let rendered = serde_json::to_string_pretty(&document).expect("serializes");
    let digest = sakatsu_core::hashing::hex(Sha256::digest(rendered.as_bytes()));
    let evaluation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");
    let path = evaluation.join(format!("hybrid-assistance-audit-{}.json", &digest[..16]));
    let staging = evaluation.join(format!(
        ".hybrid-assistance-audit-{}.partial",
        &digest[..16]
    ));
    std::fs::write(&staging, &rendered).expect("write");
    std::fs::rename(&staging, &path).expect("publish");
    println!("wrote {}", path.display());

    // A readable pass over the cases the investigation turned on.
    for record in document["cases"].as_array().expect("cases") {
        let focus = record["focus"].as_str().unwrap_or("");
        if !matches!(
            focus,
            "なんで" | "怒んなきゃ" | "あなたたち" | "神姫" | "裁いてやる" | "今日は" | "2019年"
        ) {
            continue;
        }
        println!("\n### case {} — 「{focus}」", record["case"]);
        for arm in ["lindera", "ichiran", "hybrid"] {
            let tokens = &record["arms"][arm];
            if let Some(list) = tokens.as_array() {
                let summary: Vec<String> = list
                    .iter()
                    .map(|token| {
                        format!(
                            "{}【{}】 {} | {}",
                            token["surface"].as_str().unwrap_or(""),
                            token["reading"].as_str().unwrap_or(""),
                            token["retrieval_scope"].as_str().unwrap_or(""),
                            token["displayed_senses"]
                                .as_array()
                                .and_then(|senses| senses.first())
                                .and_then(|sense| sense["glosses"].as_str())
                                .unwrap_or("(nothing)")
                        )
                    })
                    .collect();
                println!("  {arm:<8} {}", summary.join("  ·  "));
            } else {
                println!("  {arm:<8} not assembled: {}", tokens["not_assembled"]);
            }
        }
    }
}
