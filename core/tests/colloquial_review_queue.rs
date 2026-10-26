//! The 40-case review queue: what a person has to look at, and what the app actually said.
//!
//! ```text
//! cargo test --release -p sakatsu-core --test colloquial_review_queue -- --ignored --nocapture
//! ```
//!
//! # Why a token comparison is not enough to build this from
//!
//! `colloquial_comparison` establishes where two engines draw different boundaries. That is a
//! question about segmentation, and it cannot answer the question the product is for: **what
//! assistance does the reader actually get, and is it any good?** A case where both engines agree
//! and the reader is offered nothing useful is invisible to a boundary comparison and is exactly
//! the kind of failure worth fixing.
//!
//! So every case here carries the **real output**: the grammar patterns the app found, the
//! per-token breakdown it would show, and the dictionary response it would display — headword,
//! reading, and the senses that reach the reader. Where that output is empty, the record says so,
//! because an empty explanation is a finding.
//!
//! # Five judgments, never collapsed
//!
//! Per `scripts/annotation_schema.py`'s `JUDGMENT_TYPES`, and every one starts `proposed`:
//!
//! | Judgment | Asks |
//! |---|---|
//! | `reading` | is the transcription and the reading right |
//! | `morphological_boundary` | is the segmentation right as morphology |
//! | `learner_grouping` | is it the right *unit* to show a learner |
//! | `construction_explanation` | is the explanation present and useful |
//! | `lookup` | did the dictionary answer, and with the right entry |
//!
//! **Boundary and grouping are separate on purpose.** `んです` may be two morphemes and one useful
//! unit at the same time, and a queue that offers one verdict forces a reviewer to pick which
//! question they are answering.
//!
//! # Three things this file refuses to conclude
//!
//! **Composition causing a disagreement does not make the composition wrong.** The stage trace
//! shows our `compose` joins `ん | です` where the tokenizer and Ichiran both split it. That is
//! evidence worth putting in front of a reviewer; it is not a vote, and two engines agreeing does
//! not settle what grouping serves a reader. Every such case is presented with the agreement noted
//! and the judgment left open.
//!
//! **Trace coverage is not defect prevalence.** Cases are labelled *composition-attributed* or
//! *upstream-attributed* only where the trace established it. The untraced remainder is carried as
//! untraced, with no responsibility assigned.
//!
//! **The analyzed text is not the original.** `analyze` normalises line breaks and trims, so the
//! string the engines saw is not always the string in the corpus. Both are stored, with the
//! transformations between them, so a review span and a later comparison refer to the same input.

mod common;

#[path = "common/runner.rs"]
mod runner;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use common::{dictionaries_dir, installed_dictionaries};
use runner::{Outcome, Profile, Requirement, Run};
use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, LinderaAnalyzer};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::{Analysis, Token};
use sakatsu_core::pipeline::Pipeline;
use serde_json::{json, Value};

/// Twenty each side. **Never pooled and never traded off against each other**: the captures are
/// the domain the product is measured on, the chat corpora are where examples are plentiful, and
/// a shortage on one side is reported rather than made up from the other.
const PER_CORPUS: usize = 20;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

/// Pairs the stage trace established as **produced by our composition**, not by the tokenizer.
///
/// Source: `analyzer::lindera::tests::each_stage_is_attributed_for_the_contested_groupings`. For
/// each of these the tokenizer's own output is the split form, so Lindera's joined form is
/// `compose`'s doing.
const COMPOSITION_ATTRIBUTED: &[(&str, &str)] = &[
    ("んです", "ん | です"),
    ("なん", "な | ん"),
    ("なん | です", "なんです"),
    ("そう | なん | です", "そうなんです"),
    ("んだ", "ん | だ"),
    ("である", "で | ある"),
    ("な | ので", "なので"),
];

/// Pairs the trace established as **untouched by any stage of ours** — the difference is between
/// the UniDic tokenizer and Ichiran.
const UPSTREAM_ATTRIBUTED: &[(&str, &str)] = &[
    ("と | か", "とか"),
    ("よ | ね", "よね"),
    ("何 | か", "何か"),
    ("か | な", "かな"),
    ("で | も", "でも"),
    ("か | も", "かも"),
    ("なん | か", "なんか"),
    ("の | に", "のに"),
    ("の | か", "のか"),
    ("に | は", "には"),
    ("確か | に", "確かに"),
    ("外そう | と | する", "外そうとする"),
    ("やや | ばい", "や | やばい"),
];

/// How the queue is filled, in priority order. A stratum that cannot be filled is **reported
/// short**, never topped up from another: the shape of the queue is part of what it measures.
///
/// Agreements and negative controls are not filler. A case where the engines agree and the reader
/// is still offered nothing is a finding a disagreement-driven queue would never surface, and
/// without them every judgment in the queue would be about a case selected for being broken.
const STRATA: &[(&str, usize, usize)] = &[
    // (name, capture quota, chat quota)
    ("explanatory-composition", 3, 6),
    ("particle-cluster-upstream", 3, 5),
    ("number-reading-candidate", 1, 1),
    ("reading-disagreement", 2, 1),
    ("regional-discovery", 0, 2),
    ("agreement-negative-control", 3, 4),
    ("untraced-disagreement", 0, 1),
];

#[derive(Debug, Clone)]
struct Case {
    id: String,
    corpus: &'static str,
    source: String,
    original: String,
    family: Option<String>,
    stratum_of_sample: String,
    partition: &'static str,
    /// Capture only: the transcript tier, since a draft transcript makes judgment 1 genuinely open.
    tier: &'static str,
    capture_stem: Option<String>,
    chat_ids: Option<(String, String)>,
    /// Capture only: the image beside the transcript, and its digest. `verify_case` resolves the
    /// file and compares the hash, so a case whose image has changed since review fails rather
    /// than quietly carrying a stale judgment.
    image: Option<(String, String)>,
}

fn stable(salt: &str, key: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let digest =
        sakatsu_core::hashing::hex(Sha256::digest(format!("{salt}\u{1f}{key}").as_bytes()));
    u64::from_str_radix(&digest[..16], 16).unwrap_or(0)
}

/// The transformations between the corpus text and the text the engines were handed.
///
/// **Recorded as edits, not described in prose.** `scripts/annotation_schema.py` replays them and
/// refuses any that does not land on a character boundary, so a span in a review record can be
/// resolved against either text without guessing.
fn transformations(original: &str) -> (String, Vec<Value>) {
    let analyzed = sakatsu_core::analyzer::lines::normalise_breaks(original);
    let mut edits = Vec::new();

    // Carriage returns, one edit each: \r\n loses its \r, a lone \r becomes \n.
    let bytes = original.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'\r' {
            let followed_by_newline = bytes.get(index + 1) == Some(&b'\n');
            edits.push(json!({
                "original_start": index,
                "original_end": index + 1,
                "replacement": if followed_by_newline { "" } else { "\n" },
                "kind": "carriage-return",
                "certainty": "structural",
            }));
        }
        index += 1;
    }

    // The trim, as leading and trailing edits over the normalised text mapped back to the
    // original. Both ends are recorded separately: they are different decisions about different
    // parts of the source, and a single "strip whitespace" edit would say neither.
    let trimmed = analyzed.trim();
    let lead = analyzed.len() - analyzed.trim_start().len();
    let tail = analyzed.len() - analyzed.trim_end().len();
    if lead > 0 {
        edits.push(json!({
            "original_start": 0,
            "original_end": lead,
            "replacement": "",
            "kind": "leading-whitespace",
            "certainty": "structural",
        }));
    }
    if tail > 0 {
        edits.push(json!({
            "original_start": original.len() - tail,
            "original_end": original.len(),
            "replacement": "",
            "kind": "trailing-whitespace",
            "certainty": "structural",
        }));
    }

    (trimmed.to_string(), edits)
}

/// The dictionary response the reader would actually see for one token.
///
/// **Availability and content, kept apart.** `found` says a dictionary answered; the senses say
/// what it answered with. A reviewer needs both, because an entry that exists and is the wrong
/// entry is the failure a boundary comparison is blindest to.
fn lookup_response(tokens: &[Token], index: usize, dictionaries: &[Dictionary]) -> Value {
    let contextual = sakatsu_core::lookup::in_context(tokens, index, dictionaries);
    match contextual {
        // `in_context` is the production path for a token with neighbours, and it returns the
        // ranking beside the word — so the record carries what the reader is shown *and* why that
        // sense came first, which a reviewer needs to judge whether the lookup was right.
        Ok(contextual) => {
            let view = contextual.word;
            json!({
                "found": !view.inline.is_empty() || view.name.is_some(),
                "headword": view.headword,
                "reading": view.reading,
                // The senses as the reader meets them, in the order they would be shown.
                "inline_senses": view.inline.iter().take(3).map(|sense| json!({
                    "tags": sense.tags,
                    "glosses": sense.glosses,
                    "sequence": sense.sequence,
                })).collect::<Vec<_>>(),
                "inline_sense_count": view.inline.len(),
                "promoted_from": view.inline_source,
                "also_in": view.also_in.len(),
                "name": view.name.is_some(),
                "contextual_ranking": serde_json::to_value(&contextual.ranking).unwrap_or(Value::Null),
            })
        }
        Err(error) => json!({
            "found": false,
            "error": format!("{error:?}"),
        }),
    }
}

/// One dictionary entry, identified in a way that survives the merge.
///
/// **A headword is not an entry identity.** `WordView.headword` is set from the *token's*
/// `dict_form` or `surface` (`merge.rs:75`, `:398`), so "the headword equals the expression" says
/// something about the token and nothing about which entry supplied the senses beside it. An
/// earlier version of this file used that equality as proof of retrieval, and used "a gloss
/// contains the expression" as proof of sense identity. Neither is evidence.
///
/// `Sense.sequence` exists for exactly this — its own doc comment says the merge used to lose it
/// and that losing it "made the ordering unmeasurable". Where a source states no sequence, a
/// deterministic locator within the pinned dictionary content stands in, and where neither is
/// available the identity is **unestablished** and is reported as such rather than guessed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum EntryId {
    /// The dictionary's content fingerprint plus the entry sequence it states.
    Sequenced { fingerprint: String, sequence: i64 },
    /// No sequence stated: located deterministically inside the pinned content.
    Located {
        fingerprint: String,
        term: String,
        reading: String,
        ordinal: usize,
    },
}

impl EntryId {
    fn describe(&self) -> Value {
        match self {
            EntryId::Sequenced {
                fingerprint,
                sequence,
            } => json!({
                "kind": "sequence",
                "dictionary_fingerprint": fingerprint,
                "sequence": sequence,
            }),
            EntryId::Located {
                fingerprint,
                term,
                reading,
                ordinal,
            } => json!({
                "kind": "locator",
                "dictionary_fingerprint": fingerprint,
                "term": term,
                "reading": reading,
                "definition_ordinal": ordinal,
            }),
        }
    }
}

fn fingerprint_of(dictionary: &Dictionary) -> String {
    dictionary
        .content_fingerprint()
        .unwrap_or_else(|_| "unfingerprinted".to_string())
}

/// The identities a direct query for `expression` yields, and what each entry says.
fn target_entries(
    expression: &str,
    dictionaries: &[Dictionary],
) -> (BTreeSet<EntryId>, Vec<Value>) {
    let mut identities = BTreeSet::new();
    let mut described = Vec::new();
    for dictionary in dictionaries {
        let fingerprint = fingerprint_of(dictionary);
        let Ok(hits) = dictionary.lookup(expression) else {
            continue;
        };
        for (ordinal, hit) in hits.iter().enumerate() {
            // **Exact headword only.** A hit on a longer term that merely contains the expression
            // is a different word, and counting it would inflate "the entry exists".
            if hit.entry.term != expression {
                continue;
            }
            let identity = match hit.entry.sequence {
                0 => EntryId::Located {
                    fingerprint: fingerprint.clone(),
                    term: hit.entry.term.clone(),
                    reading: hit.entry.reading.clone(),
                    ordinal,
                },
                sequence => EntryId::Sequenced {
                    fingerprint: fingerprint.clone(),
                    sequence,
                },
            };
            described.push(json!({
                "dictionary": dictionary.meta().title,
                "identity": identity.describe(),
                "term": hit.entry.term,
                "reading": hit.entry.reading,
                "glosses": hit.entry.definitions.iter()
                    .flat_map(|definition| definition.glosses.clone())
                    .take(4)
                    .collect::<Vec<_>>(),
            }));
            identities.insert(identity);
        }
    }
    (identities, described)
}

/// The identity a merged sense carries, where it carries one.
fn sense_identity(
    sense: &sakatsu_core::dict::merge::Sense,
    fingerprints: &[String],
) -> Option<EntryId> {
    // A merged sense keeps its sequence but not its dictionary, so an identity is claimed only
    // when exactly one installed dictionary could have supplied it. With two dictionaries and a
    // shared sequence space this is the honest limit, and the alternative — picking one — would
    // invent provenance.
    let sequence = sense.sequence?;
    if fingerprints.len() != 1 {
        return Some(EntryId::Sequenced {
            fingerprint: "ambiguous-across-installed-dictionaries".into(),
            sequence,
        });
    }
    Some(EntryId::Sequenced {
        fingerprint: fingerprints[0].clone(),
        sequence,
    })
}

/// Does any sense in `senses` carry one of the target identities? `None` means unestablished.
fn contains_target(
    senses: &[sakatsu_core::dict::merge::Sense],
    targets: &BTreeSet<EntryId>,
) -> (Option<bool>, Option<usize>) {
    let target_sequences: BTreeSet<i64> = targets
        .iter()
        .filter_map(|identity| match identity {
            EntryId::Sequenced { sequence, .. } => Some(*sequence),
            EntryId::Located { .. } => None,
        })
        .collect();
    if target_sequences.is_empty() {
        // The target entries state no sequence, so nothing here can be matched to them by
        // identity. **Unestablished, not absent.**
        return (None, None);
    }
    let position = senses.iter().position(|sense| {
        sense
            .sequence
            .is_some_and(|seq| target_sequences.contains(&seq))
    });
    (Some(position.is_some()), position)
}

/// Whether the **expression** reaches the reader, observed stage by stage.
///
/// The four stages are the production path, called through production functions:
/// `lookup::sources` → `dict::merge::merge` → `lookup::in_context`. Nothing here reimplements
/// lookup or ranking; it records what each stage returned.
///
/// Every observation is independent, because collapsing them is how the previous version reported
/// a verdict distribution in which one branch was unreachable:
///
/// - an **exact-headword miss is not** "no appropriate entry exists";
/// - **first position is not** "correct" — that needs evidence the sense fits the context;
/// - **not-retrieved and identity-unestablished are different**, and only one of them is a finding
///   about the pipeline.
fn expression_evidence(
    expression: &str,
    span: (usize, usize),
    analysis: &Analysis,
    dictionaries: &[Dictionary],
) -> Value {
    let fingerprints: Vec<String> = dictionaries.iter().map(fingerprint_of).collect();
    let (targets, target_described) = target_entries(expression, dictionaries);

    // Every token the expression's span covers, not just the one it starts in.
    let components: Vec<(usize, &Token)> = analysis
        .tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| {
            let (Some(start), Some(end)) = (token.start, token.end) else {
                return false;
            };
            start < span.1 && end > span.0
        })
        .collect();

    let mut component_records = Vec::new();
    let mut in_sources = Vec::new();
    let mut in_primary = Vec::new();
    let mut in_also_in = false;
    let mut observed_position: Option<usize> = None;

    for (index, token) in &components {
        // Stage 1 — raw per-dictionary candidates, before any merge or ordering.
        let raw = sakatsu_core::lookup::sources(token, dictionaries);
        let (source_records, source_has_target) = match &raw {
            Ok(results) => {
                let mut found = false;
                let records: Vec<Value> = results
                    .iter()
                    .map(|result| {
                        let hits: Vec<Value> = result
                            .hits
                            .iter()
                            .map(|hit| {
                                let matches = targets.iter().any(|identity| match identity {
                                    EntryId::Sequenced { sequence, .. } => {
                                        hit.entry.sequence == *sequence
                                    }
                                    EntryId::Located { term, reading, .. } => {
                                        hit.entry.term == *term && hit.entry.reading == *reading
                                    }
                                });
                                if matches {
                                    found = true;
                                }
                                json!({
                                    "term": hit.entry.term,
                                    "reading": hit.entry.reading,
                                    "sequence": hit.entry.sequence,
                                    "is_target_entry": matches,
                                })
                            })
                            .collect();
                        json!({ "dictionary": result.title, "hits": hits })
                    })
                    .collect();
                (records, Some(found))
            }
            Err(_) => (Vec::new(), None),
        };

        // Stage 2 — the merged response, before contextual ordering.
        let merged = raw
            .as_ref()
            .ok()
            .map(|results| sakatsu_core::dict::merge::merge(token, results));
        let (primary_has_target, _) = merged
            .as_ref()
            .map(|view| contains_target(&view.inline, &targets))
            .unwrap_or((None, None));
        let also_in_has_target = merged.as_ref().is_some_and(|view| {
            view.also_in
                .iter()
                .any(|source| contains_target(&source.senses, &targets).0 == Some(true))
        });

        // Stage 3 — the response the reader is actually shown, in its final order.
        let contextual = sakatsu_core::lookup::in_context(&analysis.tokens, *index, dictionaries);
        let (final_has_target, position) = contextual
            .as_ref()
            .map(|view| contains_target(&view.word.inline, &targets))
            .unwrap_or((None, None));

        let ranking = contextual.as_ref().ok().map(|view| {
            json!({
                "favoured": view.ranking.favoured,
                "abstained": view.ranking.favoured.is_none(),
                "senses": view.ranking.senses.iter().enumerate().map(|(at, ranked)| json!({
                    "position": at,
                    "original_index": ranked.original_index,
                    "score": ranked.score,
                    "reasons": ranked.reasons,
                    "identity": view.word.inline.get(at)
                        .and_then(|sense| sense_identity(sense, &fingerprints))
                        .map(|identity| identity.describe()),
                })).collect::<Vec<_>>(),
            })
        });

        if source_has_target == Some(true) {
            in_sources.push(*index);
        }
        if primary_has_target == Some(true) {
            in_primary.push(*index);
        }
        if also_in_has_target {
            in_also_in = true;
        }
        if let Some(at) = position {
            observed_position = Some(observed_position.map_or(at, |best| best.min(at)));
        }

        component_records.push(json!({
            "token": index,
            "surface": token.surface,
            "start": token.start,
            "end": token.end,
            "stage_1_sources": source_records,
            "stage_1_has_target_entry": source_has_target,
            "stage_2_primary_senses": merged.as_ref().map(|view| view.inline.len()),
            "stage_2_has_target_entry": primary_has_target,
            "stage_2_also_in_has_target_entry": also_in_has_target,
            "stage_3_has_target_entry": final_has_target,
            "stage_3_target_position": position,
            "stage_3_headword": merged.as_ref().map(|view| view.headword.clone()),
            "headword_is_not_entry_identity": true,
            "ranking": ranking,
        }));
    }

    // **Separate observations, not one verdict.** Each answers a different question and the
    // repairs they point at are different.
    let retrieval = if targets.is_empty() {
        // The query found no entry under this exact headword. That is a fact about the query,
        // **not** a finding that no appropriate entry exists — a different spelling, a longer
        // expression or a grammar entry may still serve the reader.
        "exact-headword-query-found-nothing"
    } else if in_sources.is_empty() && in_primary.is_empty() {
        "target-entry-not-retrieved"
    } else if in_primary.is_empty() {
        "retrieved-into-sources-but-not-into-the-primary-response"
    } else {
        "target-entry-in-primary-response"
    };

    json!({
        "expression": expression,
        "span": [span.0, span.1],
        "components": component_records,
        "exact_headword_query": {
            "found": !targets.is_empty(),
            "entries": target_described,
            "means": "an exact-headword miss is not evidence that no appropriate entry exists",
        },
        "target_entry": {
            "retrieval": retrieval,
            "in_raw_sources_for_tokens": in_sources,
            "in_primary_response_for_tokens": in_primary,
            "in_additional_source_disclosure": in_also_in,
            "identity_established": !targets.is_empty()
                && targets.iter().any(|identity| matches!(identity, EntryId::Sequenced { .. })),
        },
        "target_sense_position": {
            "position": observed_position,
            "established": observed_position.is_some(),
            "means": "position is where the sense sits, not whether it is right; correctness is a \
                      reviewed judgment",
        },
        "correctness": {
            "state": "proposed",
            "by": "claude (agent proposal)",
            "note": "no judgment here establishes that a retrieved sense fits this context",
        },
    })
}

/// Positive controls for the measurement itself, **harvested, never invented**.
///
/// A measurement that has only ever seen one outcome has not been shown to distinguish outcomes.
/// The previous version of this file reported "0 ranking failures" from a branch that could not
/// execute: `reached` was set only when the headword matched, and that same branch pinned the rank
/// to zero. Nothing caught it, because nothing asked whether the other branch was reachable.
///
/// So each control below is a **real entry in the installed dictionaries**, found by probing terms
/// that occur in the frozen corpus — no Japanese is constructed here, per the standing rule. Each
/// is pinned by the dictionary's content fingerprint. **A control class that cannot be filled is
/// reported, and the run says so**, rather than leaving an unexercised path to look like a result.
fn harvested_controls(probe_terms: &BTreeSet<String>, dictionaries: &[Dictionary]) -> Value {
    let mut same_term_many_entries: Option<Value> = None;
    let mut target_not_first: Option<Value> = None;
    let mut gloss_mentions_other_term: Option<Value> = None;
    let mut no_sequence_stated: Option<Value> = None;
    let mut single_entry_first: Option<Value> = None;

    for term in probe_terms {
        for dictionary in dictionaries {
            let fingerprint = fingerprint_of(dictionary);
            let Ok(hits) = dictionary.lookup(term) else {
                continue;
            };
            let exact: Vec<_> = hits.iter().filter(|hit| hit.entry.term == *term).collect();
            if exact.is_empty() {
                continue;
            }

            // **Identical headword, different entry provenance.** Two entries under one spelling,
            // which is exactly why a headword string cannot stand in for an entry identity.
            let sequences: BTreeSet<i64> = exact
                .iter()
                .filter_map(|hit| Some(hit.entry.sequence).filter(|s| *s != 0))
                .collect();
            if sequences.len() > 1 && same_term_many_entries.is_none() {
                same_term_many_entries = Some(json!({
                    "control": "identical headword, different entry provenance",
                    "term": term,
                    "dictionary": dictionary.meta().title,
                    "dictionary_fingerprint": fingerprint,
                    "sequences": sequences.iter().copied().collect::<Vec<_>>(),
                    "readings": exact.iter().map(|hit| hit.entry.reading.clone())
                        .collect::<BTreeSet<_>>().iter().cloned().collect::<Vec<_>>(),
                }));

                // **The correct candidate genuinely not first — executed, not merely located.**
                //
                // Finding data that *would* exercise a branch is not exercising it, and that
                // distinction is exactly what went wrong before: the position>0 branch looked
                // reasonable and could not run.
                //
                // The first attempt at this control designated the numerically larger sequence as
                // the target and measured position 0 — because **sequence order is not merge
                // order**, so "the second entry by number" is not "the entry shown second". That
                // was a defect in the control, not in the instrument, and it would have been read
                // as the latter. So the merge runs first and the target is the entry actually
                // sitting at a later position; the measurement then has to agree with where it is.
                let probe = Token {
                    surface: term.clone(),
                    dict_form: term.clone(),
                    ..Default::default()
                };
                let merged = sakatsu_core::lookup::sources(&probe, dictionaries)
                    .ok()
                    .map(|results| sakatsu_core::dict::merge::merge(&probe, &results));
                let later = merged.as_ref().and_then(|view| {
                    view.inline
                        .iter()
                        .enumerate()
                        .skip(1)
                        .find_map(|(at, sense)| sense.sequence.map(|sequence| (at, sequence)))
                });
                if let (Some(view), Some((expected_at, designated))) = (merged.as_ref(), later) {
                    let targets: BTreeSet<EntryId> = [EntryId::Sequenced {
                        fingerprint: fingerprint.clone(),
                        sequence: designated,
                    }]
                    .into_iter()
                    .collect();
                    let (found, position) = contains_target(&view.inline, &targets);
                    target_not_first = Some(json!({
                        "control": "correct candidate genuinely second",
                        "term": term,
                        "dictionary_fingerprint": fingerprint,
                        "designated_target_sequence": designated,
                        "sits_at_position": expected_at,
                        "measured_position": position,
                        "found": found,
                        "branch_exercised": position == Some(expected_at) && expected_at > 0,
                        "note": "an inappropriate sense leading is the same observation from the \
                                 reader's side; both are this control",
                    }));
                }
            }

            // **No sequence stated**, so identity must be reported unestablished rather than
            // guessed from the headword.
            if no_sequence_stated.is_none() {
                if let Some(hit) = exact.iter().find(|hit| hit.entry.sequence == 0) {
                    no_sequence_stated = Some(json!({
                        "control": "missing identity reported as unestablished",
                        "term": term,
                        "dictionary": dictionary.meta().title,
                        "dictionary_fingerprint": fingerprint,
                        "reading": hit.entry.reading,
                        "expected": "identity_established = false, retrieval not asserted",
                    }));
                }
            }

            // **A real first-place candidate still measured correctly** — the ordinary case, which
            // a measurement biased toward finding failures could get wrong just as easily.
            if single_entry_first.is_none() && sequences.len() == 1 {
                single_entry_first = Some(json!({
                    "control": "a real first-place candidate still measured correctly",
                    "term": term,
                    "dictionary_fingerprint": fingerprint,
                    "sequence": sequences.iter().next().copied(),
                    "expected_measured_position": 0,
                }));
            }

            // **An unrelated gloss mentioning the target expression.** Proves the measurement does
            // not treat "a gloss contains the string" as entry identity — which is precisely what
            // the previous version did.
            if gloss_mentions_other_term.is_none() && term.chars().count() >= 2 {
                for other in probe_terms {
                    if other == term {
                        continue;
                    }
                    let Ok(other_hits) = dictionary.lookup(other) else {
                        continue;
                    };
                    if let Some(hit) = other_hits.iter().find(|hit| {
                        hit.entry.term != *term
                            && hit.entry.definitions.iter().any(|definition| {
                                definition
                                    .glosses
                                    .iter()
                                    .any(|gloss| gloss.contains(term.as_str()))
                            })
                    }) {
                        gloss_mentions_other_term = Some(json!({
                            "control": "unrelated gloss mentioning the target expression",
                            "target_expression": term,
                            "unrelated_term": hit.entry.term,
                            "dictionary_fingerprint": fingerprint,
                            "expected": "not counted as the target entry",
                        }));
                        break;
                    }
                }
            }
        }
    }

    let filled: Vec<(&str, &Option<Value>)> = vec![
        (
            "identical headword, different entry provenance",
            &same_term_many_entries,
        ),
        ("correct candidate genuinely second", &target_not_first),
        (
            "unrelated gloss mentioning the target expression",
            &gloss_mentions_other_term,
        ),
        (
            "missing identity reported as unestablished",
            &no_sequence_stated,
        ),
        (
            "a real first-place candidate still measured correctly",
            &single_entry_first,
        ),
    ];
    let unfilled: Vec<&str> = filled
        .iter()
        .filter(|(_, found)| found.is_none())
        .map(|(name, _)| *name)
        .collect();

    json!({
        "sourced_from": "entries in the installed dictionaries, probed with terms occurring in the \
                         frozen corpus; no Japanese is constructed",
        "probe_terms": probe_terms.len(),
        "filled": filled.iter().filter_map(|(_, found)| (*found).clone()).collect::<Vec<_>>(),
        "unfilled": unfilled,
        "note": "a control class that cannot be filled leaves that path unexercised, and is \
                 reported rather than passed over",
        "unfilled_detail": {
            "missing identity reported as unestablished":
                "every entry the installed dictionaries returned for these probe terms states a \
                 sequence, so the no-sequence path has no real data here. The *reporting* of \
                 unestablished identity is still exercised by the queue, via the different route \
                 of an exact-headword query finding no entry at all — those cases carry \
                 identity_established = false.",
        },
        // Two classes the *queue* answers rather than the dictionary probe, recorded here so the
        // list is complete rather than quietly shortened.
        "observed_in_the_queue": [
            "correct entry available only in additional-source results — measured per case as \
             `in_additional_source_disclosure`",
            "correct entry retrieved with an inappropriate sense first — measured per case as \
             `target_sense_position.position` greater than 0",
        ],
    })
}

/// The explanation the reader would actually see over the whole passage.
fn explanation(analysis: &Analysis) -> Value {
    let patterns = sakatsu_core::grammar::patterns(analysis);
    let breakdowns: Vec<Value> = analysis
        .tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            let breakdown = sakatsu_core::grammar::breakdown(token);
            if breakdown.is_empty() {
                return None;
            }
            Some(json!({
                "token": index,
                "surface": token.surface,
                "steps": breakdown.steps.len(),
                "auxiliaries": breakdown.auxiliaries.len(),
                "detail": serde_json::to_value(&breakdown).unwrap_or(Value::Null),
            }))
        })
        .collect();
    json!({
        // An empty list here is the finding, not a gap in the record.
        "grammar_patterns": patterns.iter().map(|found| json!({
            "pattern_id": found.pattern_id,
            "tokens": found.token_count(),
            "start": found.start,
            "end": found.end,
        })).collect::<Vec<_>>(),
        "grammar_pattern_count": patterns.len(),
        "token_breakdowns": breakdowns,
        "tokens_with_no_breakdown": analysis.tokens.len() - breakdowns.len(),
    })
}

/// Whether the reader gets an explanation **of the target**, and on what evidence.
///
/// Two things were wrong with the previous version. It counted patterns over the **whole passage**,
/// so "an explanation appeared in 30 of 32 cases" included passages that explained something six
/// tokens away. And it treated **overlap as recognition**: a pattern whose span touched the target
/// was recorded as explaining it, which a pattern about one of the target's components does not.
///
/// Overlap is not replaced with an equally crude exact-span rule, because a larger construction can
/// legitimately explain a smaller target — `node` covering `なので` is a real explanation of the
/// stretch under review. So the relation is classified and the pattern is **resolved to its actual
/// pack entry**, and whether it is semantically relevant stays a `proposed`/`reviewed` judgment.
///
/// | Relation | Meaning |
/// |---|---|
/// | `covers-the-whole-target` | the match spans the target span entirely |
/// | `spans-the-target-and-more` | a larger construction containing it — may still explain it |
/// | `component-only` | the match lies inside the target, explaining a part |
/// | `overlapping-unrelated` | spans cross without containment either way |
fn target_explanation(analysis: &Analysis, span: (usize, usize)) -> Value {
    let patterns = sakatsu_core::grammar::patterns(analysis);

    let mut related = Vec::new();
    let mut elsewhere = 0usize;
    for found in &patterns {
        let (Some(start), Some(end)) = (found.start, found.end) else {
            // A match with no grounded span cannot be shown to bear on the target, and is not
            // counted as if it did.
            elsewhere += 1;
            continue;
        };
        if start >= span.1 || end <= span.0 {
            elsewhere += 1;
            continue;
        }
        let relation = if start <= span.0 && end >= span.1 {
            if start == span.0 && end == span.1 {
                "covers-the-whole-target"
            } else {
                "spans-the-target-and-more"
            }
        } else if start >= span.0 && end <= span.1 {
            "component-only"
        } else {
            "overlapping-unrelated"
        };
        // **Resolved, not just named.** A pattern id with no pack entry behind it is a dangling
        // reference, and reporting the id alone would hide that.
        let entry = sakatsu_core::grammar::pack::find(&found.pattern_id);
        related.push(json!({
            "pattern_id": found.pattern_id,
            "resolved": entry.is_some(),
            "pattern": entry.map(|p| p.pattern.clone()),
            "title": entry.map(|p| p.title.clone()),
            "jlpt": entry.and_then(|p| p.jlpt.clone()),
            "explanation": entry.map(|p| p.explanation.clone()),
            "first_token": found.first_token,
            "last_token_end": found.last_token_end,
            "start": start,
            "end": end,
            "relation": relation,
            "proposed_to_explain_the_target": matches!(
                relation,
                "covers-the-whole-target" | "spans-the-target-and-more"
            ),
            "why": match relation {
                "covers-the-whole-target" => "the match span and the target span are the same",
                "spans-the-target-and-more" =>
                    "a larger construction contains the target; it may still be the explanation a \
                     reader needs, which is a reviewed judgment rather than a span fact",
                "component-only" =>
                    "the match lies inside the target and explains a part of it, not the whole",
                _ => "the spans cross without either containing the other",
            },
            "relevance": "proposed",
        }));
    }

    // Component breakdowns are kept apart from construction explanations: a conjugation ladder on
    // one token is a different kind of help from a named grammar point over a phrase.
    let breakdowns: Vec<Value> = analysis
        .tokens
        .iter()
        .filter(|token| match (token.start, token.end) {
            (Some(start), Some(end)) => start < span.1 && end > span.0,
            _ => false,
        })
        .filter_map(|token| {
            let breakdown = sakatsu_core::grammar::breakdown(token);
            if breakdown.is_empty() {
                return None;
            }
            Some(json!({
                "surface": token.surface,
                "steps": breakdown.steps.len(),
                "auxiliaries": breakdown.auxiliaries.len(),
            }))
        })
        .collect();

    let explains_target = related
        .iter()
        .any(|found| found["proposed_to_explain_the_target"] == Value::Bool(true));
    let state = if explains_target {
        "construction-proposed-to-explain-the-target"
    } else if !related.is_empty() {
        "only-component-or-unrelated-matches-touch-the-target"
    } else if !breakdowns.is_empty() {
        "component-breakdown-only"
    } else if !patterns.is_empty() {
        "unrelated-explanation-only"
    } else {
        "no-explanation"
    };

    json!({
        "state": state,
        "matches_touching_the_target": related,
        "component_breakdowns": breakdowns,
        "patterns_elsewhere_in_passage": elsewhere,
        "means": "a span relation is not semantic relevance; that is a reviewed judgment",
    })
}

/// The five judgments, every one of them, every time — each stating what it rests on.
///
/// **A judgment with no evidence is refused by the schema, and rightly.** Five type names are not
/// five contracts until each carries its own payload, and "proposed" with nothing behind it is a
/// row in a queue that looks reviewed and is not. So each kind is handed the specific observation
/// a reviewer would start from: the engine outputs for the boundary, the explanation the app
/// produced for the construction, the actual dictionary response for the lookup.
fn judgments(evidence: &JudgmentEvidence) -> Vec<Value> {
    let rows: [(&str, &str, String); 5] = [
        (
            "reading",
            "is the transcription and the reading right",
            evidence.reading.clone(),
        ),
        (
            "morphological_boundary",
            "is the segmentation right as morphology",
            evidence.boundary.clone(),
        ),
        (
            "learner_grouping",
            "is this the right unit to put in front of a learner",
            evidence.grouping.clone(),
        ),
        (
            "construction_explanation",
            "is the explanation present, correct and useful",
            evidence.explanation.clone(),
        ),
        (
            "lookup",
            "did the dictionary answer, and with the right entry",
            evidence.lookup.clone(),
        ),
    ];
    rows.iter()
        .map(|(kind, asks, rests_on)| {
            json!({
                "id": format!("{}:{}", evidence.case_id, kind),
                "type": kind,
                "asks": asks,
                // **Nothing here is adjudicated.** A proposal made by software is not a review,
                // and the state says so on every row rather than in a header somebody may skip.
                "state": "proposed",
                "by": "claude (agent proposal)",
                "evidence": rests_on,
                "human_review": "pending",
            })
        })
        .collect()
}

/// What each judgment rests on, gathered before any of them is written.
#[derive(Debug, Default, Clone)]
struct JudgmentEvidence {
    case_id: String,
    reading: String,
    boundary: String,
    grouping: String,
    explanation: String,
    lookup: String,
}

fn spans_of(analysis: &Analysis) -> Vec<Value> {
    analysis
        .tokens
        .iter()
        .map(|token| {
            json!({
                "start": token.start,
                "end": token.end,
                "surface": token.surface,
                "reading": token.reading,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_case(
    case: &Case,
    stratum: &str,
    focus: Option<&Value>,
    lindera: &LinderaAnalyzer,
    ichiran: &IchiranAnalyzer,
    dictionaries: &[Dictionary],
) -> Option<Value> {
    let (analyzed, edits) = transformations(&case.original);
    if analyzed.is_empty() {
        return None;
    }
    let pipeline = Pipeline::new(dictionaries);

    let mut lindera_display = lindera.analyze(&analyzed).ok()?;
    let lindera_raw_spans = spans_of(&lindera_display);
    pipeline.display(&mut lindera_display);

    let ichiran_display = ichiran.analyze(&analyzed).ok().map(|mut analysis| {
        let raw = spans_of(&analysis);
        pipeline.display(&mut analysis);
        (raw, analysis)
    });

    // The token the disputed span falls in, for the lookup. Where there is no dispute — an
    // agreement control — the longest word in the passage stands in, so the control exercises the
    // same machinery the disagreements do.
    let target = focus
        .and_then(|found| found.get("start").and_then(Value::as_u64))
        .map(|start| start as usize)
        .and_then(|start| {
            lindera_display.tokens.iter().position(|token| {
                token.start.is_some_and(|at| at <= start) && token.end.is_some_and(|at| at > start)
            })
        })
        .or_else(|| {
            lindera_display
                .tokens
                .iter()
                .enumerate()
                .filter(|(_, token)| token.is_word())
                .max_by_key(|(_, token)| token.surface.chars().count())
                .map(|(index, _)| index)
        })?;

    let attribution = focus
        .and_then(|found| {
            let left = found.get("lindera")?.as_str()?;
            let right = found.get("ichiran")?.as_str()?;
            let text = found.get("text").and_then(Value::as_str).unwrap_or("");
            // A counted number's reading is produced by `compose_digits`, which the stage trace
            // covers: the tokenizer emits the digits and the counter separately for both 2019年
            // and 1週間. So these are composition-attributed as much as the boundary cases are,
            // and leaving them `untraced` understated what the trace established.
            if found.get("kind").and_then(Value::as_str) == Some("reading")
                && text.chars().any(|c| c.is_ascii_digit())
            {
                return Some("composition-attributed");
            }
            let pair = (left, right);
            if COMPOSITION_ATTRIBUTED.contains(&pair) {
                Some("composition-attributed")
            } else if UPSTREAM_ATTRIBUTED.contains(&pair) {
                Some("upstream-attributed")
            } else {
                Some("untraced")
            }
        })
        .unwrap_or("not-applicable");

    // The span under review: the disputed stretch, or the target token for a control.
    let (span_start, span_end) = focus
        .and_then(|found| {
            Some((
                found.get("start")?.as_u64()? as usize,
                found.get("end")?.as_u64()? as usize,
            ))
        })
        .filter(|(start, end)| end > start && *end <= analyzed.len())
        .or_else(|| {
            let token = &lindera_display.tokens[target];
            Some((token.start?, token.end?))
        })?;

    // **The source record carries the fields for its own kind and no others.** The schema
    // refuses a capture that carries a dataset revision, and a `null` is still carrying one — a
    // case cannot be half a capture and half a chat utterance, and emitting both sets with the
    // wrong one nulled would let a verifier resolve against whichever it happened to read.
    let mut source = serde_json::Map::new();
    source.insert("kind".into(), json!(case.corpus));
    source.insert("name".into(), json!(case.source));
    source.insert("partition".into(), json!(case.partition));
    source.insert("sampled_as".into(), json!(case.stratum_of_sample));
    source.insert("sampled_family".into(), json!(case.family));
    match case.corpus {
        "capture" => {
            // What `verify_case` resolves and hashes.
            let (image, digest) = case
                .image
                .clone()
                .unwrap_or_else(|| (String::new(), String::new()));
            source.insert("image".into(), json!(image));
            source.insert("image_sha256".into(), json!(digest));
            source.insert("transcript_tier".into(), json!(case.tier));
            source.insert("capture_stem".into(), json!(case.capture_stem));
        }
        _ => {
            let (dialogue, utterance) = case.chat_ids.clone().unwrap_or_default();
            source.insert("dataset".into(), json!(case.source));
            // The revision the utterance was taken at, not whatever the repository holds now.
            source.insert("revision".into(), json!(revision_of(&case.source)));
            source.insert("dialogue_id".into(), json!(dialogue));
            source.insert("utterance_id".into(), json!(utterance));
        }
    }
    let source = Value::Object(source);

    let evidence = match attribution {
        "composition-attributed" => {
            "The stage trace shows the UniDic tokenizer produces the split form here and our \
             `compose` joins it, so Ichiran and the tokenizer agree against our output. **That is \
             supporting evidence, not a vote.** Whether the joined unit is the right thing to show \
             a learner is the open question, and two engines splitting it does not answer it."
        }
        "upstream-attributed" => {
            "The stage trace shows no pass of ours touches this stretch, so the difference is \
             between the UniDic tokenizer and Ichiran."
        }
        "untraced" => {
            "Not covered by the stage trace. **No responsibility is assigned**: this is an \
             observed disagreement whose origin has not been established."
        }
        _ => {
            "The engines agree here. Selected as a control, to ask whether agreement means the \
              reader is served."
        }
    };

    let original_digest = {
        use sha2::{Digest, Sha256};
        sakatsu_core::hashing::hex(Sha256::digest(case.original.as_bytes()))
    };

    // The expression under review, as the reader would need it — the whole disputed stretch, not
    // the token its first byte happens to fall in.
    let expression = analyzed
        .get(span_start..span_end)
        .unwrap_or_default()
        .to_string();
    let reader = json!({
        "target_token": lindera_display.tokens[target].surface,
        // Passage-level, kept because it is what the app produces; **not** evidence about the
        // target, which is the next field.
        "explanation_over_passage": explanation(&lindera_display),
        "explanation_of_target": target_explanation(&lindera_display, (span_start, span_end)),
        "lookup_of_target_token": lookup_response(&lindera_display.tokens, target, dictionaries),
        // The question a token-local lookup cannot reach.
        "expression_candidates": expression_evidence(
            &expression,
            (span_start, span_end),
            &lindera_display,
            dictionaries,
        ),
    });
    let pattern_count = reader["explanation_over_passage"]["grammar_pattern_count"]
        .as_u64()
        .unwrap_or(0);
    let target_state = reader["explanation_of_target"]["state"]
        .as_str()
        .unwrap_or("?")
        .to_string();
    let lookup_found = reader["lookup_of_target_token"]["found"]
        .as_bool()
        .unwrap_or(false);
    let lookup_headword = reader["lookup_of_target_token"]["headword"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let lookup_senses = reader["lookup_of_target_token"]["inline_sense_count"]
        .as_u64()
        .unwrap_or(0);
    let candidate_verdict = reader["expression_candidates"]["target_entry"]["retrieval"]
        .as_str()
        .unwrap_or("?")
        .to_string();
    // **Reported beside the retrieval state, never folded into it.** "not retrieved" and "we
    // could not tell" are different findings, and only the first is about the pipeline.
    let identity_established = reader["expression_candidates"]["target_entry"]
        ["identity_established"]
        .as_bool()
        .unwrap_or(false);
    let sense_position =
        reader["expression_candidates"]["target_sense_position"]["position"].as_u64();
    let ichiran_reading = ichiran_display.as_ref().and_then(|(_, analysis)| {
        analysis
            .tokens
            .iter()
            .find(|token| token.start == Some(span_start))
            .map(|token| format!("{:?}", token.reading))
    });

    Some(json!({
        "id": format!("{}/{}", case.corpus, case.id),
        // **Named, not inferred from which fields happen to be present.** The verifier used to
        // decide what a record was by testing for `span`, which meant a full case that lost its
        // span was silently dropped from validation instead of failing it.
        "record_kind": "full",
        "stratum": stratum,
        "attribution": attribution,
        "attribution_evidence": evidence,
        "source": source,
        // The span under review, naming which of the two texts it indexes. Engine offsets index
        // the analysis text, so that is what it says.
        "span": {
            "on": "analysis",
            "start": span_start,
            "end": span_end,
            "surface": analyzed.get(span_start..span_end).unwrap_or_default(),
        },
        "provenance": {
            // **'unknown' is a legitimate value and is the honest one here.** A promoted
            // transcript was written by a person; a draft may be an OCR engine's output lightly
            // corrected, which the preliminary batch's own finding raises.
            "creation_method": if case.tier == "promoted" {
                "human-transcribed (per corpus/REVIEW.md convention)"
            } else if case.corpus == "chat" {
                "corpus-published utterance at a pinned revision"
            } else {
                "unknown"
            },
            "human_review": "pending",
            "selected_by": "colloquial_review_queue.rs",
        },
        // **Three fields, because they are three different strings.** A span means nothing
        // without knowing which of them it indexes.
        "text": {
            "original": case.original,
            // The schema requires a case to carry its own digest, so a pinned record can be
            // matched without trusting the text beside it.
            "original_sha256": original_digest,
            "analysis": analyzed,
            "transformations": edits,
            "note": "engine offsets index `analysis`; the corpus holds `original`",
        },
        "focus": focus.cloned().unwrap_or(Value::Null),
        "engines": {
            "lindera_raw": lindera_raw_spans,
            "lindera_display": spans_of(&lindera_display),
            "ichiran_raw": ichiran_display.as_ref().map(|(raw, _)| raw.clone()),
            "ichiran_display": ichiran_display.as_ref().map(|(_, analysis)| spans_of(analysis)),
        },
        // What the reader would actually be shown. The point of the queue.
        "what_the_reader_gets": reader,
        "judgments": judgments(&JudgmentEvidence {
            case_id: format!("{}/{}", case.corpus, case.id),
            reading: match case.tier {
                "promoted" => format!(
                    "Promoted transcript. Lindera reads the target token {:?}; Ichiran {}.",
                    lindera_display.tokens[target].reading,
                    ichiran_reading.as_deref().unwrap_or("did not answer")
                ),
                "not-applicable" => format!(
                    "Corpus-published utterance at a pinned revision. Lindera reads the target \
                     token {:?}; Ichiran {}.",
                    lindera_display.tokens[target].reading,
                    ichiran_reading.as_deref().unwrap_or("did not answer")
                ),
                _ => "Draft transcript: never promoted, so the transcription itself is open."
                    .to_string(),
            },
            boundary: evidence.to_string(),
            // **Separate from the boundary on purpose.** A stretch can be two morphemes and one
            // useful unit at once, so the reviewer is shown the same split with the other
            // question attached rather than being asked to answer both at once.
            grouping: format!(
                "Lindera shows this as {:?}; Ichiran as {:?}. Morphology and useful unit are \
                 different questions and this row asks only the second.",
                focus.and_then(|f| f.get("lindera")).and_then(Value::as_str).unwrap_or("(agreement)"),
                focus.and_then(|f| f.get("ichiran")).and_then(Value::as_str).unwrap_or("(agreement)")
            ),
            explanation: format!(
                "For the target span: {target_state}. ({pattern_count} grammar pattern(s) over the \
                 whole passage — a pattern elsewhere is not assistance with this construction.)"
            ),
            lookup: format!(
                "Token-local lookup {} for {:?}: headword {:?}, {} sense(s) — and a headword is \
                 the token's dict-form, not an entry identity. For the expression {:?}: \
                 `{candidate_verdict}`; entry identity {}; target sense position {}. A successful \
                 lookup of one component does not establish that the expression was available to \
                 select, and a position is not a correctness judgment.",
                if lookup_found { "answered" } else { "did not answer" },
                lindera_display.tokens[target].surface,
                lookup_headword,
                lookup_senses,
                expression,
                if identity_established { "established" } else { "UNESTABLISHED" },
                sense_position.map_or("unestablished".to_string(), |at| at.to_string())
            ),
        }),
        "review_state": "proposed",
    }))
}

fn chat_cases() -> Vec<Case> {
    let path = root().join("scratch/colloquial/chat-sample.jsonl");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .map(|record| Case {
            id: record["case_id"].as_str().unwrap_or_default().to_string(),
            corpus: "chat",
            source: record["source"].as_str().unwrap_or_default().to_string(),
            original: record["text"].as_str().unwrap_or_default().to_string(),
            family: record["family"].as_str().map(str::to_string),
            stratum_of_sample: record["stratum"].as_str().unwrap_or("?").to_string(),
            partition: "development",
            tier: "not-applicable",
            capture_stem: None,
            chat_ids: Some((
                record["dialogue_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                record["utterance_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            )),
            image: None,
        })
        .collect()
}

/// Promoted capture transcripts on the **development** side only.
///
/// The seven captures `corpus/reading-gold.json` is built on were read by the first comparison run
/// and are excluded here: that exposure is recorded and not repeated.
fn capture_cases() -> (Vec<Case>, usize) {
    const NOT_CAPTURES: [&str; 2] = ["ocr-to-text-corpus.txt", "run-2026-08-25.txt"];
    let corpus = root().join("corpus");
    let holdout: BTreeSet<String> = std::fs::read(corpus.join("reading-gold.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|document| {
            Some(
                document["cases"]
                    .as_array()?
                    .iter()
                    .filter_map(|case| case["source_image"].as_str())
                    .filter_map(|image| Path::new(image).file_stem())
                    .map(|stem| stem.to_string_lossy().to_string())
                    .collect(),
            )
        })
        .unwrap_or_default();

    let mut cases = Vec::new();
    let mut excluded = 0usize;
    let mut stack = vec![corpus.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if path.extension().and_then(|e| e.to_str()) != Some("txt")
                || NOT_CAPTURES.contains(&name.as_str())
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if holdout.contains(&stem) {
                excluded += 1;
                continue;
            }
            let application = stem
                .rsplit_once('_')
                .map(|(app, _)| app)
                .unwrap_or("?")
                .to_string();
            let image = ["png", "jpg", "jpeg", "webp"]
                .iter()
                .map(|extension| path.with_extension(extension))
                .find(|candidate| candidate.is_file())
                .and_then(|candidate| {
                    let bytes = std::fs::read(&candidate).ok()?;
                    use sha2::{Digest, Sha256};
                    Some((
                        candidate.file_name()?.to_string_lossy().to_string(),
                        sakatsu_core::hashing::hex(Sha256::digest(&bytes)),
                    ))
                });
            cases.push(Case {
                id: stem.clone(),
                corpus: "capture",
                source: application,
                original: text,
                family: None,
                stratum_of_sample: "promoted-transcript".into(),
                partition: "development",
                tier: "promoted",
                capture_stem: Some(stem),
                chat_ids: None,
                image,
            });
        }
    }
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    (cases, excluded)
}

/// The preliminary game-capture reviews, counted toward the capture allocation.
///
/// Eight draft-tier game captures with agent proposals already recorded in
/// `scratch/colloquial/review-batch-proposals.json`. They are **eligible**: capture-domain, not in
/// the reading holdout, and already carrying per-capture proposals. They are also draft tier, so
/// judgment 1 — transcription correctness — is genuinely open on every one, and the batch's own
/// `draft_provenance` finding raises the possibility that a draft records an OCR engine's output
/// rather than an independent reading. That makes them *more* useful in this queue, not less, and
/// the record says so rather than presenting them as settled.
fn preliminary_reviews() -> Vec<Value> {
    let path = root().join("scratch/colloquial/review-batch-proposals.json");
    let Ok(raw) = std::fs::read(&path) else {
        return Vec::new();
    };
    let Ok(document) = serde_json::from_slice::<Value>(&raw) else {
        return Vec::new();
    };
    document["captures"]
        .as_array()
        .map(|captures| {
            captures
                .iter()
                .map(|capture| {
                    json!({
                        "id": format!("capture/{}", capture["stem"].as_str().unwrap_or("?")),
                        // Carries v2 proposals but no engine output and no annotated span. It is
                        // a review record, and the difference has to be stated rather than left
                        // to be deduced from a missing field.
                        "record_kind": "preliminary",
                        "missing_evidence": [
                            "engine output (predates the comparison)",
                            "annotated span",
                            "reader output: explanation and lookup",
                        ],
                        "stratum": "preliminary-game-capture",
                        "attribution": "not-applicable",
                        "attribution_evidence":
                            "Reviewed before the engine comparison, by reading the capture image.",
                        "source": {
                            "kind": "capture",
                            "name": capture["stem"].as_str().unwrap_or("?")
                                .rsplit_once('_').map(|(app, _)| app).unwrap_or("?"),
                            "partition": "development",
                            "transcript_tier": "draft",
                            "capture_stem": capture["stem"].clone(),
                        },
                        "preliminary": {
                            "why_selected": capture["why"].clone(),
                            "draft_matches_image": capture["draft_matches_image"].clone(),
                            "register": capture["register"].clone(),
                            "patterns": capture["patterns"].clone(),
                        },
                        "judgments": judgments(&JudgmentEvidence {
                            case_id: format!("capture/{}", capture["stem"].as_str().unwrap_or("?")),
                            reading:
                                "Draft tier: the transcript was never promoted, so the \
                                 transcription is unreviewed and this judgment is open in a way a \
                                 promoted transcript's is not. (Matching an OCR engine's output is \
                                 not evidence either way about how the draft was made, and is not \
                                 offered as any.)"
                                    .to_string(),
                            boundary: format!(
                                "Preliminary review recorded: draft_matches_image {}.",
                                capture["draft_matches_image"]
                            ),
                            grouping: format!(
                                "Register recorded as {}. 4 of the 8 preliminary captures are UI \
                                 labels, an announcement or prose rather than dialogue, so the \
                                 useful-unit question may not arise here at all.",
                                capture["register"]
                            ),
                            explanation: format!(
                                "Patterns matched in the draft: {}. Two of six family matches \
                                 across the construction-selected captures were judged genuine, so \
                                 a match here is not a construction. No engine output was \
                                 collected for this record, so the explanation question is open \
                                 and unevidenced.",
                                capture["patterns"]
                            ),
                            lookup:
                                "Not run: this case predates the engine comparison and carries no \
                                 engine output. The lookup question is open and unevidenced."
                                    .to_string(),
                        }),
                        "review_state": "proposed",
                        "counted_toward": "capture allocation",
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Independent evidence for a counted-number reading, and the path that produced ours.
///
/// **Ichiran disagreeing is not evidence that Ichiran is right.** It is one engine's opinion, and
/// a repair justified by it would be a repair justified by the thing under comparison. So this
/// asks the installed dictionaries what reading they record for the spelled-out form, and
/// separately records what `numbers::counted` actually returns and what the `COUNTERS` table holds
/// — the fallback taken, not the fallback inferred from a table that may not be the code path.
fn counted_number_evidence(dictionaries: &[Dictionary]) -> Value {
    // (spelling to look up, the counter, what the app produced, what Ichiran produced)
    let probes = [
        ("九年", "年", "きゅうねん", "くねん"),
        // **The 週間 family, not one example of it.** A single reading establishes one reading; a
        // repair to an alternation table needs the shape of the alternation. 週間 begins with し,
        // so if it behaves like the other /s/-initial counters the table already carries, the
        // sokuon appears at 1, 8 and 10 and nowhere else.
        ("一週間", "週間", "いちしゅうかん", "いっしゅうかん"),
        ("二週間", "週間", "にしゅうかん", "にしゅうかん"),
        ("三週間", "週間", "さんしゅうかん", "さんしゅうかん"),
        ("八週間", "週間", "はちしゅうかん", "?"),
        ("十週間", "週間", "じゅうしゅうかん", "?"),
        // A longer number, to check the rule is about the **final** digit rather than the whole
        // number — the distinction that decides whether two table rows are the right repair.
        ("十一週間", "週間", "?", "?"),
        // Controls: /s/-initial counters the table already carries, so a reader can see the
        // difference between "the table has no entry" and "the dictionary disagrees with us".
        ("一歳", "歳", "いっさい", "いっさい"),
        ("八歳", "歳", "はっさい", "はっさい"),
        ("十歳", "歳", "じゅっさい", "じゅっさい"),
        ("二歳", "歳", "にさい", "にさい"),
        ("四年", "年", "よねん", "よねん"),
        ("一回", "回", "いっかい", "いっかい"),
    ];

    let mut findings = Vec::new();
    for (spelling, counter, ours, ichiran) in probes {
        let mut recorded: BTreeSet<String> = BTreeSet::new();
        for dictionary in dictionaries {
            if let Ok(hits) = dictionary.lookup(spelling) {
                for hit in hits {
                    if hit.entry.term == spelling && !hit.entry.reading.is_empty() {
                        recorded.insert(hit.entry.reading.clone());
                    }
                }
            }
        }

        // What the production function actually returns for this shape, and for the digit form
        // the corpus case used — because "absent from COUNTERS" describes the table, not the path.
        let digits = spelling.chars().count() - counter.chars().count();
        // The numeral the spelling names, so `counted` is asked the same question the dictionary
        // was. Written out rather than parsed, because a wrong parse here would look like a wrong
        // reading there.
        let value: u64 = match spelling.chars().next() {
            Some('二') => 2,
            Some('三') => 3,
            Some('四') => 4,
            Some('八') => 8,
            Some('九') => 9,
            Some('十') if spelling.chars().nth(1) == Some('一') => 11,
            Some('十') => 10,
            _ => 1,
        };
        let produced = sakatsu_core::numbers::counted(value, counter, "");
        findings.push(json!({
            "spelling": spelling,
            "counter": counter,
            "leading_digits": digits,
            "our_reading_in_corpus": ours,
            "ichiran_reading_in_corpus": ichiran,
            "dictionary_recorded_readings": recorded.iter().cloned().collect::<Vec<_>>(),
            "dictionary_count": recorded.len(),
            "numbers_counted_returns": produced,
            "note": "dictionary readings are independent of both engines; an empty list means the \
                     installed dictionaries do not carry this spelling and settles nothing",
        }));
    }

    json!({
        "why": "Ichiran is one engine's opinion, and a table edit justified by it would be \
                justified by the thing under comparison.",
        "caveat": "`numbers::counted` is called with an empty counter reading here, which is not \
                   how `compose_digits` calls it. The value shows what the table alone yields; the \
                   corpus reading above is what the production path produced.",
        "open_question": "Whether 9-before-\u{306d}\u{3093} is one instance of a wider alternation \
                          the per-counter table shape cannot express. If it is, adding two rows is \
                          the wrong repair.",
        "probes": findings,
    })
}

/// The pinned revision each chat corpus was fetched at, from the licence dossier.
///
/// Carried into every case so `verify_case` can resolve the utterance in the revision it was
/// actually taken from, rather than in whatever the repository holds now.
fn revision_of(source: &str) -> String {
    let path = root().join("scratch/colloquial/chat/licence-dossier.json");
    std::fs::read(&path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|document| {
            document
                .get("sources")?
                .get(source)?
                .get("revision")?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn disagreements() -> Vec<Value> {
    let path = root().join("target/evaluation/colloquial-disagreements.json");
    std::fs::read(&path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|document| document["disagreements"].as_array().cloned())
        .unwrap_or_default()
}

fn stratum_of(found: &Value) -> &'static str {
    let kind = found["kind"].as_str().unwrap_or("");
    let left = found["lindera"].as_str().unwrap_or("");
    let right = found["ichiran"].as_str().unwrap_or("");
    let text = found["text"].as_str().unwrap_or("");
    let family = found["family"].as_str().unwrap_or("");

    if kind == "reading" && (text.contains('年') || text.contains("週間")) {
        return "number-reading-candidate";
    }
    if kind == "reading" {
        return "reading-disagreement";
    }
    if family == "regional" {
        return "regional-discovery";
    }
    if COMPOSITION_ATTRIBUTED.contains(&(left, right)) {
        return "explanatory-composition";
    }
    if UPSTREAM_ATTRIBUTED.contains(&(left, right)) {
        return "particle-cluster-upstream";
    }
    "untraced-disagreement"
}

#[test]
#[ignore = "needs the dictionaries, the Ichiran sidecar and a completed comparison"]
fn the_forty_case_review_queue() {
    let dictionaries = installed_dictionaries();
    let found = disagreements();
    let chat = chat_cases();
    let (captures, excluded_holdout) = capture_cases();
    let preliminary = preliminary_reviews();

    let ichiran = IchiranAnalyzer::new();
    sakatsu_core::analyzer::ichiran::server::set_persistent(true);
    ichiran.warm();
    let probe = ichiran.analyze("これって");
    let route = sakatsu_core::analyzer::ichiran::server::last_route();
    let ready = probe.is_ok() && route.is_session();

    let lindera = match LinderaAnalyzer::new() {
        Ok(analyzer) => analyzer,
        Err(error) => panic!("the embedded dictionary would not load: {error:?}"),
    };

    // Index the disagreements by case, so a selected case arrives with its own evidence.
    let mut by_case: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for record in &found {
        if record["kind"].as_str() == Some("uncovered-whitespace") {
            continue;
        }
        by_case
            .entry(record["case"].as_str().unwrap_or_default().to_string())
            .or_default()
            .push(record.clone());
    }

    let mut expected: Vec<String> = Vec::new();
    let mut selections: Vec<(String, &'static str, Case, Option<Value>)> = Vec::new();
    let mut shortages: Vec<Value> = Vec::new();

    for (corpus, pool) in [("capture", &captures), ("chat", &chat)] {
        // The preliminary reviews already occupy part of the capture allocation.
        let already = if corpus == "capture" {
            preliminary.len()
        } else {
            0
        };
        let mut taken_here = already;
        let mut claimed: BTreeSet<String> = BTreeSet::new();

        for (name, capture_quota, chat_quota) in STRATA {
            let quota = if corpus == "capture" {
                *capture_quota
            } else {
                *chat_quota
            };
            if quota == 0 {
                continue;
            }
            let mut candidates: Vec<(&Case, Option<Value>)> = Vec::new();

            if *name == "agreement-negative-control" {
                for case in pool.iter() {
                    let key = format!("{corpus}/{}", case.id);
                    if by_case.contains_key(&key) || claimed.contains(&case.id) {
                        continue;
                    }
                    candidates.push((case, None));
                }
            } else {
                for case in pool.iter() {
                    if claimed.contains(&case.id) {
                        continue;
                    }
                    let key = format!("{corpus}/{}", case.id);
                    let Some(records) = by_case.get(&key) else {
                        continue;
                    };
                    if let Some(record) = records.iter().find(|record| stratum_of(record) == *name)
                    {
                        candidates.push((case, Some(record.clone())));
                    }
                }
            }

            candidates.sort_by_key(|(case, _)| stable(name, &case.id));
            let mut filled = 0usize;
            for (case, record) in candidates {
                if filled >= quota || taken_here >= PER_CORPUS {
                    break;
                }
                claimed.insert(case.id.clone());
                expected.push(format!("{corpus}/{}", case.id));
                selections.push((format!("{corpus}/{}", case.id), name, case.clone(), record));
                filled += 1;
                taken_here += 1;
            }
            if filled < quota {
                shortages.push(json!({
                    "corpus": corpus,
                    "stratum": name,
                    "wanted": quota,
                    "got": filled,
                }));
            }
        }
        if taken_here < PER_CORPUS {
            shortages.push(json!({
                "corpus": corpus,
                "stratum": "(total)",
                "wanted": PER_CORPUS,
                "got": taken_here,
            }));
        }
    }

    let mut run = Run::new(Profile::InstalledResources, expected.clone())
        .engine("lindera-display + ichiran-display, with explanation and lookup")
        .needs(
            Requirement::required("dictionaries", !dictionaries.is_empty())
                .because(format!("under {}", dictionaries_dir().display())),
        )
        .needs(
            Requirement::required("completed comparison", !found.is_empty())
                .because(format!("{} disagreement records", found.len())),
        )
        .needs(
            Requirement::required("ichiran via the session route", ready)
                .because(format!("route {}", route.name())),
        );
    for dictionary in &dictionaries {
        run = run.dictionary(format!(
            "{} ({} entries)",
            dictionary.meta().title,
            dictionary.meta().entry_count
        ));
    }

    // Terms to probe the dictionaries with, taken from the corpus the queue is drawn from rather
    // than composed for the purpose. Surfaces and dictionary forms both, since an entry is filed
    // under the latter and the reader meets the former.
    let mut probe_terms: BTreeSet<String> = BTreeSet::new();
    for (_, _, case, _) in &selections {
        if let Ok(analysis) = lindera.analyze(&case.original) {
            for token in &analysis.tokens {
                if token.is_word() {
                    probe_terms.insert(token.surface.clone());
                    if !token.dict_form.is_empty() {
                        probe_terms.insert(token.dict_form.clone());
                    }
                }
            }
        }
    }

    let mut queue: Vec<Value> = preliminary.clone();
    for (id, stratum, case, record) in &selections {
        match build_case(
            case,
            stratum,
            record.as_ref(),
            &lindera,
            &ichiran,
            &dictionaries,
        ) {
            Some(built) => {
                run.record(id.clone(), Outcome::Passed);
                queue.push(built);
            }
            None => run.record(
                id.clone(),
                Outcome::incomplete("the case could not be analysed, so it carries no evidence"),
            ),
        }
    }

    let document = json!({
        "schema": 2,
        "note": "AGENT PROPOSALS. Every judgment is `proposed`; none has been adjudicated by a \
                 person and none may be promoted without that.",
        "allocation": {
            "capture": PER_CORPUS,
            "chat": PER_CORPUS,
            "preliminary_counted_toward_capture": preliminary.len(),
            "capture_holdout_sources_excluded": excluded_holdout,
            "never_traded_between_corpora": true,
        },
        // **Not 40 equivalently verified cases**, and the record says so rather than leaving a
        // total to imply it.
        "accounting": {
            "records": queue.len(),
            "full": queue.iter().filter(|case| case["record_kind"] == "full").count(),
            "preliminary": queue.iter().filter(|case| case["record_kind"] == "preliminary").count(),
            "judgments": queue.iter()
                .map(|case| case["judgments"].as_array().map(Vec::len).unwrap_or(0))
                .sum::<usize>(),
            "note": "preliminary records carry v2 proposals but no engine output and no annotated \
                     span; they are not verified to the same standard as a full case",
        },
        "attribution_caveats": [
            "Composition causing a disagreement does not make the composition wrong. Agreement \
             between Ichiran and the UniDic tokenizer is supporting evidence, not a vote.",
            "Trace coverage is not defect prevalence. Cases are labelled composition-attributed or \
             upstream-attributed only where the stage trace established it; the rest are untraced \
             and carry no assigned responsibility.",
            "Engine offsets index the `analysis` text, not the `original`. Both are stored with the \
             transformations between them.",
        ],
        "shortages": shortages,
        "independent_evidence": {
            "counted_numbers": counted_number_evidence(&dictionaries),
            "measurement_controls": harvested_controls(&probe_terms, &dictionaries),
        },
        "cases": queue,
    });

    let out = root().join("target/evaluation/colloquial-review-queue.json");
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_vec_pretty(&document) {
        Ok(body) => match std::fs::write(&out, body) {
            Ok(()) => println!("\n  wrote {}", out.display()),
            Err(error) => println!("\n  could not write {}: {error}", out.display()),
        },
        Err(error) => println!("\n  could not serialise the queue: {error}"),
    }

    println!("\n=== colloquial review queue ===");
    println!(
        "  {} cases: {} preliminary (capture, draft tier) + {} built",
        queue.len(),
        preliminary.len(),
        selections.len()
    );
    println!("  {excluded_holdout} capture holdout source(s) excluded from selection");
    let mut by_stratum: BTreeMap<&str, usize> = BTreeMap::new();
    for case in &queue {
        *by_stratum
            .entry(case["stratum"].as_str().unwrap_or("?"))
            .or_default() += 1;
    }
    for (stratum, count) in &by_stratum {
        println!("    {stratum:<28} {count:>3}");
    }
    if shortages.is_empty() {
        println!("  no shortages");
    } else {
        println!("  SHORTAGES:");
        for shortage in &shortages {
            println!(
                "    {} {} wanted {} got {}",
                shortage["corpus"], shortage["stratum"], shortage["wanted"], shortage["got"]
            );
        }
    }

    run.accounted_for().expect_success();
}

#[cfg(test)]
mod tests {
    use super::*;
    use sakatsu_core::dict::merge::Sense;

    fn sense(sequence: Option<i64>, gloss: &str) -> Sense {
        Sense {
            tags: Vec::new(),
            glosses: vec![gloss.to_string()],
            sequence,
        }
    }

    fn sequenced(sequence: i64) -> BTreeSet<EntryId> {
        [EntryId::Sequenced {
            fingerprint: "fp".into(),
            sequence,
        }]
        .into_iter()
        .collect()
    }

    /// **The missing-identity control, as a fixture.** The installed dictionaries happen to state a
    /// sequence on every entry these probes reach, so the harvested controls cannot fill this class
    /// — but "no real example is to hand" is not a reason to leave the path unexercised, and the
    /// path is the one that decides whether a case reads *unestablished* or *not retrieved*.
    #[test]
    fn a_target_with_no_stated_sequence_is_unestablished_not_absent() {
        let targets: BTreeSet<EntryId> = [EntryId::Located {
            fingerprint: "fp".into(),
            term: "とか".into(),
            reading: "とか".into(),
            ordinal: 0,
        }]
        .into_iter()
        .collect();
        let senses = vec![sense(Some(1234567), "and the like")];

        let (found, position) = contains_target(&senses, &targets);
        assert_eq!(
            found, None,
            "with no sequence to match on, the answer is 'we could not tell', not 'absent'"
        );
        assert_eq!(position, None);
    }

    /// The ordinary case: a stated sequence is matched by identity, and its position is reported.
    #[test]
    fn a_sequenced_target_is_found_at_its_actual_position() {
        let senses = vec![
            sense(Some(111), "first"),
            sense(Some(222), "second"),
            sense(Some(333), "third"),
        ];
        assert_eq!(
            contains_target(&senses, &sequenced(111)),
            (Some(true), Some(0))
        );
        assert_eq!(
            contains_target(&senses, &sequenced(222)),
            (Some(true), Some(1)),
            "a target that is not first must report where it actually is"
        );
        assert_eq!(
            contains_target(&senses, &sequenced(999)),
            (Some(false), None)
        );
    }

    /// **A gloss is prose, not an identity.** The predecessor of this code matched a target by
    /// asking whether a gloss contained the expression, which makes any entry that happens to
    /// mention とか look like the とか entry.
    #[test]
    fn a_gloss_mentioning_the_expression_is_not_the_entry() {
        let senses = vec![sense(Some(111), "used with とか to list examples")];
        assert_eq!(
            contains_target(&senses, &sequenced(222)),
            (Some(false), None),
            "mentioning the expression is not being its entry"
        );
    }

    /// A sense carrying no sequence cannot claim an identity, and does not borrow one.
    #[test]
    fn a_sense_without_a_sequence_has_no_identity() {
        assert!(sense_identity(&sense(None, "x"), &["fp".to_string()]).is_none());
        assert!(sense_identity(&sense(Some(1), "x"), &["fp".to_string()]).is_some());
    }

    /// With two dictionaries installed, a bare sequence does not say which one it came from, and
    /// the identity says so rather than picking one.
    #[test]
    fn provenance_is_marked_ambiguous_rather_than_guessed() {
        let two = vec!["a".to_string(), "b".to_string()];
        match sense_identity(&sense(Some(7), "x"), &two) {
            Some(EntryId::Sequenced { fingerprint, .. }) => assert_eq!(
                fingerprint, "ambiguous-across-installed-dictionaries",
                "a shared sequence space must not be attributed to one dictionary"
            ),
            other => panic!("expected an ambiguous sequenced identity, got {other:?}"),
        }
    }
}
