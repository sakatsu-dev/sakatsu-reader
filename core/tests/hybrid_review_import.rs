//! Import the completed blinded review, as one reader's usability judgments.
//!
//! # What this is, and firmly is not
//!
//! A person answered twelve blinded cases about which of two analyses reads better. That is
//! **usability feedback from one reader on twelve previously inspected development cases.** It is
//! not adjudicated Japanese gold, it cannot establish generalisation, and it is kept out of
//! linguistic scoring by identity rather than by convention — see `scripts/annotation_schema.py`,
//! where a cohort is excluded the same way `V1_COHORT` is.
//!
//! # Two identities, because the document is both prompt and response
//!
//! The batch was published content-addressed, so its filename names the **prompt**. The reader then
//! edited that same file to answer it, which legitimately changed its bytes. A single hash would
//! silently describe one and be checked against the other, so both are recorded:
//!
//! - **prompt** — `batch_sha256` from the key, the bytes that were asked;
//! - **response** — the SHA-256 of the file as it now stands, the bytes that were answered.
//!
//! # Placement numbers are decoded against each question's own list
//!
//! Q1 and Q2 both take a number and the numbers mean different things: `1` is *A is wrong* under
//! Q1 and *A is more useful* under Q2. Decoding them against a shared list would invert the
//! sharpest result in the review. Every interpreted value keeps the raw text beside it.

mod common;

#[path = "common/runner.rs"]
mod runner;

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Q1's options, in the order the batch printed them.
const Q1_OPTIONS: [&str; 5] = [
    "A is wrong",
    "B is wrong",
    "both are acceptable",
    "both are inadequate",
    "not enough context",
];

/// Q2's options. **Same numbers, different meanings** — this is the pair that must not be shared.
const Q2_OPTIONS: [&str; 5] = [
    "A is more useful",
    "B is more useful",
    "no difference",
    "both are poor",
    "not enough context",
];

/// Why Q3 is not interpreted, recorded in the artifact so the omission is legible.
const Q3_OVERRIDE: &str =
    "unassessed — the reviewer's later instruction supersedes any answer left in the document";

/// One question's answer: what was written, and what it was taken to mean.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Answer {
    /// Exactly what the reader typed, whitespace trimmed only at the ends of the block.
    raw: String,
    /// The decoded value, or `None` for unanswered, unrecognised or deliberately unassessed.
    interpreted: Option<String>,
    /// Why there is no interpreted value, when there is none.
    note: Option<String>,
}

impl Answer {
    fn unanswered() -> Answer {
        Answer {
            raw: String::new(),
            interpreted: None,
            note: Some("not answered".into()),
        }
    }

    /// Decode a placement number against one question's own option list.
    ///
    /// **Only a bare number decodes.** Free text under a multiple-choice question is kept and left
    /// uninterpreted rather than guessed at — a reader who wrote a sentence where a number was
    /// asked for meant the sentence.
    fn placed(raw: &str, options: &[&str; 5]) -> Answer {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Answer::unanswered();
        }
        match trimmed.parse::<usize>() {
            Ok(n) if (1..=options.len()).contains(&n) => Answer {
                raw: raw.to_string(),
                interpreted: Some(options[n - 1].to_string()),
                note: None,
            },
            _ => Answer {
                raw: raw.to_string(),
                interpreted: None,
                note: Some(format!(
                    "not one of the {} offered placements; kept as written",
                    options.len()
                )),
            },
        }
    }

    /// Preserved verbatim, never interpreted.
    fn unassessed(raw: &str) -> Answer {
        Answer {
            raw: raw.to_string(),
            interpreted: None,
            note: Some(Q3_OVERRIDE.into()),
        }
    }

    /// yes / no, case-insensitively — the reader wrote both `Yes` and `yES`.
    fn yes_or_no(raw: &str) -> Answer {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Answer::unanswered();
        }
        let interpreted = match trimmed.to_ascii_lowercase().as_str() {
            "yes" | "y" => Some("yes".to_string()),
            "no" | "n" => Some("no".to_string()),
            _ => None,
        };
        Answer {
            raw: raw.to_string(),
            note: interpreted
                .is_none()
                .then(|| "not yes or no; kept as written".to_string()),
            interpreted,
        }
    }
}

/// One case's answers, as read from the response document.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CaseAnswers {
    case: usize,
    focus: String,
    q1_segmentation_and_readings: Answer,
    q2_more_useful: Answer,
    q3a_as_presented: Answer,
    q3b_available: Answer,
    q4_enough_context: Answer,
    notes: String,
}

/// The text following a `**Qn` heading, up to the next heading or the end of the block.
fn answer_after<'a>(block: &'a str, marker: &str, next: &[&str]) -> &'a str {
    let Some(start) = block.find(marker) else {
        return "";
    };
    let after = &block[start..];
    // Past the question's own line.
    let Some(newline) = after.find('\n') else {
        return "";
    };
    let body = &after[newline + 1..];
    let end = next
        .iter()
        .filter_map(|stop| body.find(stop))
        .min()
        .unwrap_or(body.len());
    body[..end].trim_matches('\n')
}

/// Read every case's answers out of the completed document.
///
/// Deliberately a pure function of the text so the decoding rules can be tested without the
/// reviewer's file, which exists once and must not be edited to suit a test.
fn parse_cases(markdown: &str) -> Vec<CaseAnswers> {
    let mut cases = Vec::new();
    let mut sections: Vec<(usize, usize)> = Vec::new();
    for (offset, _) in markdown.match_indices("\n## Case ") {
        sections.push((offset + 1, 0));
    }
    for index in 0..sections.len() {
        let start = sections[index].0;
        let end = sections
            .get(index + 1)
            .map(|(next, _)| *next)
            .unwrap_or(markdown.len());
        let section = &markdown[start..end];
        let Some(number) = section.lines().next().and_then(|line| {
            line.trim_start_matches("## Case ")
                .trim()
                .parse::<usize>()
                .ok()
        }) else {
            continue;
        };
        let focus = section
            .lines()
            .find(|line| line.starts_with("**The part in question.**"))
            .map(|line| {
                line.trim_start_matches("**The part in question.**")
                    .trim()
                    .to_string()
            })
            .unwrap_or_default();
        // Everything after the questions heading; the appendix and the rule are not answers.
        let questions = match section.find("### Questions") {
            Some(at) => {
                let rest = &section[at..];
                let cut = ["<details>", "_(No sense list", "\n---\n"]
                    .iter()
                    .filter_map(|stop| rest.find(stop))
                    .min()
                    .unwrap_or(rest.len());
                &rest[..cut]
            }
            None => continue,
        };
        let stops = ["**Q1", "**Q2", "**Q3a", "**Q3b", "**Q4", "Notes:"];
        cases.push(CaseAnswers {
            case: number,
            focus,
            q1_segmentation_and_readings: Answer::placed(
                answer_after(questions, "**Q1", &stops),
                &Q1_OPTIONS,
            ),
            q2_more_useful: Answer::placed(answer_after(questions, "**Q2", &stops), &Q2_OPTIONS),
            q3a_as_presented: Answer::unassessed(answer_after(questions, "**Q3a", &stops)),
            q3b_available: Answer::unassessed(answer_after(questions, "**Q3b", &stops)),
            q4_enough_context: Answer::yes_or_no(answer_after(questions, "**Q4", &stops)),
            notes: answer_after(questions, "Notes:", &[]).trim().to_string(),
        });
    }
    cases
}

// ---------------------------------------------------------------------------------------------
// Hermetic controls. These need nothing installed and gate an ordinary run.
// ---------------------------------------------------------------------------------------------

/// A miniature response document in the batch's own shape.
#[cfg(test)]
fn fixture(answers: [&str; 5]) -> String {
    format!(
        "preamble\n\n## Case 1\n\n**The part in question.** 「テスト」 — the two cut this differently\n\n\
         ### Questions\n\n\
         **Q1 — segmentation and readings only.** *A is wrong / B is wrong / …*\n{}\n\
         **Q2 — which grouping is more useful** *A / B / no difference / …*\n{}\n\
         **Q3a — the answer as presented.** *Answer for A and for B.*\n{}\n\
         **Q3b — the answer available.** *Answer for A and for B.*\n{}\n\
         **Q4 — is there enough context** *yes / no.*\n{}\n\
         Notes:\n\n---\n",
        answers[0], answers[1], answers[2], answers[3], answers[4]
    )
}

/// **The same digit means different things under Q1 and Q2.**
///
/// This is the single decoding rule most likely to be got wrong, and getting it wrong inverts the
/// review's sharpest finding: under Q1 a `1` says the first analysis is *wrong*, under Q2 it says
/// the first analysis is *better*. A shared option list would read one as the other.
#[test]
fn a_placement_number_is_decoded_against_its_own_question() {
    let cases = parse_cases(&fixture(["1", "1", "", "", "Yes"]));
    let case = &cases[0];
    assert_eq!(
        case.q1_segmentation_and_readings.interpreted.as_deref(),
        Some("A is wrong")
    );
    assert_eq!(
        case.q2_more_useful.interpreted.as_deref(),
        Some("A is more useful")
    );
    assert_ne!(
        case.q1_segmentation_and_readings.interpreted, case.q2_more_useful.interpreted,
        "the two questions must not share an option list"
    );

    // And the other placements, so the mapping is pinned end to end rather than at one point.
    let cases = parse_cases(&fixture(["3", "2", "", "", "no"]));
    assert_eq!(
        cases[0].q1_segmentation_and_readings.interpreted.as_deref(),
        Some("both are acceptable")
    );
    assert_eq!(
        cases[0].q2_more_useful.interpreted.as_deref(),
        Some("B is more useful")
    );
    assert_eq!(
        cases[0].q4_enough_context.interpreted.as_deref(),
        Some("no")
    );
}

/// **Q3 is preserved and never interpreted**, whatever the document still contains.
#[test]
fn q3_answers_are_kept_but_left_unassessed() {
    let cases = parse_cases(&fixture([
        "1",
        "2",
        "Looks good",
        "A is the useful one",
        "Yes",
    ]));
    let case = &cases[0];
    for answer in [&case.q3a_as_presented, &case.q3b_available] {
        assert!(
            answer.interpreted.is_none(),
            "a Q3 answer must not acquire an interpreted value"
        );
        assert_eq!(answer.note.as_deref(), Some(Q3_OVERRIDE));
    }
    // **Preserved, not discarded.** Overriding the interpretation is not licence to lose the text.
    assert_eq!(case.q3a_as_presented.raw.trim(), "Looks good");
    assert_eq!(case.q3b_available.raw.trim(), "A is the useful one");
}

/// **An unanswered question stays unknown**, and is never filled in with a default.
#[test]
fn a_missing_answer_is_unknown_rather_than_assumed() {
    let cases = parse_cases(&fixture(["3", "3", "", "", ""]));
    let case = &cases[0];
    assert_eq!(case.q4_enough_context, Answer::unanswered());
    assert!(case.q4_enough_context.interpreted.is_none());
    assert_eq!(case.q4_enough_context.note.as_deref(), Some("not answered"));

    // A "no" is a real answer and must not read as absence.
    let answered = parse_cases(&fixture(["3", "3", "", "", "no"]));
    assert_ne!(answered[0].q4_enough_context, Answer::unanswered());
}

/// Free text where a number was asked for is kept, not guessed at.
#[test]
fn an_unrecognised_placement_is_not_invented() {
    let cases = parse_cases(&fixture(["both seem fine", "9", "", "", "Yes"]));
    assert!(cases[0].q1_segmentation_and_readings.interpreted.is_none());
    assert_eq!(
        cases[0].q1_segmentation_and_readings.raw.trim(),
        "both seem fine"
    );
    assert!(
        cases[0].q2_more_useful.interpreted.is_none(),
        "9 is not one of five placements"
    );
}

/// Multi-line answers survive, and a case with two lines under Q3a keeps both.
#[test]
fn multi_line_answers_are_preserved_whole() {
    let document = fixture(["2", "1", "B shows nothing\nA is good", "A only", "Yes"]);
    let cases = parse_cases(&document);
    assert_eq!(
        cases[0].q3a_as_presented.raw.trim(),
        "B shows nothing\nA is good"
    );
}

// ---------------------------------------------------------------------------------------------
// The import itself.
// ---------------------------------------------------------------------------------------------

/// What the batch displayed for one alternative, as far as a reader could distinguish it.
///
/// Used to decide whether a directional answer about *segmentation* could have been about
/// segmentation at all. If both alternatives printed the same tokens with the same readings, a
/// reader choosing between them was choosing on something else.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Displayed {
    segmentation: String,
    readings: Vec<String>,
}

fn displayed(section: &str, from: &str, to: &str) -> Displayed {
    let start = section.find(from).map(|at| at + from.len()).unwrap_or(0);
    let end = section[start..]
        .find(to)
        .map(|at| start + at)
        .unwrap_or(section.len());
    let block = &section[start..end];
    let segmentation = block
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string();
    let mut readings = Vec::new();
    let mut rest = block;
    while let Some(open) = rest.find('\u{3010}') {
        let after = &rest[open + '\u{3010}'.len_utf8()..];
        match after.find('\u{3011}') {
            Some(close) => {
                readings.push(after[..close].to_string());
                rest = &after[close..];
            }
            None => break,
        }
    }
    Displayed {
        segmentation,
        readings,
    }
}

/// Whether a reader's answer pointed at one alternative rather than at neither.
fn is_directional(answer: &Answer) -> bool {
    matches!(
        answer.interpreted.as_deref(),
        Some("A is wrong" | "B is wrong" | "A is more useful" | "B is more useful")
    )
}

/// Import the completed review, unblind it, and publish the result beside the response.
///
/// Ignored because it reads an artifact that exists once. The decoding rules it applies are gated
/// by the hermetic controls above, which need nothing.
#[test]
#[ignore = "reads the completed review artifact in target/evaluation"]
fn import_the_completed_review() {
    let evaluation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");

    let response_path = evaluation.join("hybrid-review-batch-6da1aa26cd39bf6a.md");
    let response = std::fs::read(&response_path)
        .unwrap_or_else(|error| panic!("the completed review must be readable: {error}"));
    let response_sha = sakatsu_core::hashing::hex(Sha256::digest(&response));
    let response_text = String::from_utf8(response).expect("the review is UTF-8");

    // The key carries the prompt's identity and the unblinding.
    let key_path = evaluation.join("hybrid-review-key-b7134e3a998a6f7d.json");
    let key: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&key_path)
            .unwrap_or_else(|error| panic!("the key must be readable: {error}")),
    )
    .expect("the key is JSON");
    let prompt_sha = key["batch_sha256"]
        .as_str()
        .expect("the key names the prompt")
        .to_string();
    assert_ne!(
        prompt_sha, response_sha,
        "prompt and response hashing the same would mean the document was never answered"
    );

    let cases = parse_cases(&response_text);
    assert_eq!(cases.len(), 12, "twelve cases were asked");

    let unblind: BTreeMap<u64, &serde_json::Value> = key["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|case| (case["case"].as_u64().expect("case number"), case))
        .collect();

    // Sections again, to read what each alternative actually displayed.
    let sections: Vec<&str> = response_text.split("\n## Case ").skip(1).collect();

    let mut records = Vec::new();
    let mut aligned = 0usize;
    let mut disagreed = 0usize;
    let mut abstained_but_preferred = Vec::new();
    let mut no_preference = 0usize;

    for (index, case) in cases.iter().enumerate() {
        let entry = unblind
            .get(&(case.case as u64))
            .unwrap_or_else(|| panic!("case {} is in the key", case.case));
        let a_engine = entry["shown_as_a"].as_str().unwrap_or("?");
        let b_engine = entry["shown_as_b"].as_str().unwrap_or("?");
        let choice = entry["selector_choice"].as_str().unwrap_or("?");
        let section = sections[index];

        let shown_a = displayed(section, "### Analysis A", "### Analysis B");
        let shown_b = displayed(section, "### Analysis B", "### Questions");
        let indistinguishable = shown_a == shown_b;

        // **Derived, not hardcoded.** A directional answer about segmentation cannot be about
        // segmentation when both alternatives printed the same tokens and the same readings.
        let caveat = (indistinguishable
            && (is_directional(&case.q1_segmentation_and_readings)
                || is_directional(&case.q2_more_useful)))
        .then(|| {
            "both alternatives displayed identical segmentation and readings, so a directional \
             answer here cannot establish a segmentation preference; it is recorded as an \
             assistance preference"
                .to_string()
        });

        // Which engine the reader's usefulness answer points at, if either.
        let preferred_engine = match case.q2_more_useful.interpreted.as_deref() {
            Some("A is more useful") => Some(a_engine),
            Some("B is more useful") => Some(b_engine),
            _ => None,
        };
        let selector_engine = if choice.starts_with("PreferLindera") {
            Some("lindera")
        } else if choice.starts_with("PreferIchiran") {
            Some("ichiran")
        } else {
            None
        };
        let alignment = match (selector_engine, preferred_engine) {
            (Some(selector), Some(reader)) if selector == reader => {
                aligned += 1;
                "the selector chose the analysis the reader preferred"
            }
            (Some(_), Some(_)) => {
                disagreed += 1;
                "the selector chose the analysis the reader did not prefer"
            }
            (None, Some(reader)) => {
                abstained_but_preferred.push((case.case, reader.to_string()));
                "the selector did not decide; the reader preferred one"
            }
            (Some(_), None) => "the selector decided; the reader saw no difference",
            (None, None) => {
                no_preference += 1;
                "neither decided"
            }
        };

        records.push(serde_json::json!({
            "case": case.case,
            "focus": case.focus,
            "shown_as": { "a": a_engine, "b": b_engine },
            "selector_choice": choice,
            "category": entry["category"].clone(),
            "displayed_alternatives_indistinguishable": indistinguishable,
            "caveat": caveat,
            "answers": {
                "q1_segmentation_and_readings": {
                    "raw": case.q1_segmentation_and_readings.raw,
                    "interpreted": case.q1_segmentation_and_readings.interpreted,
                    "note": case.q1_segmentation_and_readings.note,
                },
                "q2_more_useful": {
                    "raw": case.q2_more_useful.raw,
                    "interpreted": case.q2_more_useful.interpreted,
                    "note": case.q2_more_useful.note,
                },
                "q3a_as_presented": {
                    "raw": case.q3a_as_presented.raw,
                    "interpreted": serde_json::Value::Null,
                    "note": case.q3a_as_presented.note,
                },
                "q3b_available": {
                    "raw": case.q3b_available.raw,
                    "interpreted": serde_json::Value::Null,
                    "note": case.q3b_available.note,
                },
                "q4_enough_context": {
                    "raw": case.q4_enough_context.raw,
                    "interpreted": case.q4_enough_context.interpreted,
                    "note": case.q4_enough_context.note,
                },
            },
            "notes": case.notes,
            "alignment": alignment,
        }));
    }

    let document = serde_json::json!({
        "what_this_is": "One reader's usability judgments on twelve previously inspected \
                         development cases. NOT adjudicated Japanese gold, and not evidence of \
                         generalisation.",
        "cohort": "hybrid-usability-review",
        "scoring_policy": "excluded from independent-review scoring by cohort identity; see \
                           scripts/annotation_schema.py",
        "identities": {
            "prompt_sha256": prompt_sha,
            "response_sha256": response_sha,
            "response_bytes": response_text.len(),
            "key": key_path.file_name().unwrap_or_default().to_string_lossy(),
            "ruleset": sakatsu_core::hybrid::RULESET,
            "source_tree": runner::source_digest(),
        },
        "unassessed": {
            "q3a_and_q3b": Q3_OVERRIDE,
            "review_duration": "not reported by the reviewer; left unknown",
        },
        "alignment_summary": {
            "selector_and_reader_agreed": aligned,
            "selector_and_reader_disagreed": disagreed,
            "selector_abstained_while_reader_preferred": abstained_but_preferred
                .iter()
                .map(|(case, engine)| serde_json::json!({ "case": case, "reader_preferred": engine }))
                .collect::<Vec<_>>(),
            "neither_expressed_a_preference": no_preference,
            "limit": "twelve development cases already inspected during development; this cannot \
                      establish generalisation, and alignment is not correctness",
        },
        "cases": records,
    });

    let rendered = serde_json::to_string_pretty(&document).expect("serializes");
    let digest = sakatsu_core::hashing::hex(Sha256::digest(rendered.as_bytes()));
    let path = evaluation.join(format!("hybrid-review-import-{}.json", &digest[..16]));
    if path.exists() {
        println!("already imported at {}", path.display());
    } else {
        let staging = evaluation.join(format!(".hybrid-review-import-{}.partial", &digest[..16]));
        std::fs::write(&staging, &rendered).expect("write");
        std::fs::rename(&staging, &path).expect("publish");
    }

    println!("\nprompt   {}", &prompt_sha[..16]);
    println!("response {}", &response_sha[..16]);
    println!("import   {}", path.display());
    println!(
        "\nagreed {aligned}, disagreed {disagreed}, abstained-while-preferred {}, neither {no_preference}",
        abstained_but_preferred.len()
    );
    for (case, engine) in &abstained_but_preferred {
        println!("  case {case}: the reader preferred the {engine} analysis");
    }
    for record in document["cases"].as_array().expect("cases") {
        if let Some(caveat) = record["caveat"].as_str() {
            println!("  case {}: {caveat}", record["case"]);
        }
    }
}
