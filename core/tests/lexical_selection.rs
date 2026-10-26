//! Automatic grouping, end to end: real analyzer, real pack, real dictionary, real decision.
//!
//! # The question this level answers and no other can
//!
//! The unit tests supply their own candidates, so they establish the rules. They cannot establish
//! that the analyzer's own output reaches those rules — whether Lindera classifies the head the
//! whitelist asks about, whether the recovered span survives to selection, whether a construction
//! occurrence and a lexical candidate over the same text agree.
//!
//! # What is asserted, and what is only reported
//!
//! Asserted: the acceptance cases are grouped, the controls are not, and every abstention names
//! a reason. Reported: the segmentation, the candidate counts, and which route carried each
//! decision. Counts are not a result — a layer that proposed more would not thereby be better —
//! so nothing here asserts one.
//!
//! Run against the isolated copy; the reader's own store is never opened.

mod common;

use std::path::PathBuf;

use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::lexical::{self, Decision, Outcome, Route};
use sakatsu_core::Analysis;

fn dictionaries() -> Vec<Dictionary> {
    let dir = PathBuf::from(
        std::env::var_os("SAKATSU_LEXICAL_DICT")
            .expect("SAKATSU_LEXICAL_DICT must point at an isolated dictionary copy"),
    );
    let opened: Vec<Dictionary> = Dictionary::discover(&dir)
        .iter()
        .map(|path| Dictionary::open(path).expect("the copy opens"))
        .collect();
    assert!(!opened.is_empty(), "no dictionary under {}", dir.display());
    assert!(
        opened.iter().any(Dictionary::has_sequence),
        "the copy has no sequence bucket, so 格好いい and 格好良い cannot be shown to be one word"
    );
    opened
}

/// The baseline these proposals are made **over**.
///
/// `study::analyse` groups now, so it is no longer the input to generation — it is the output of
/// the whole path. A test that fed it back into `lexical::candidates` would be asking the
/// candidate layer to improve on its own answer, and would report the acceptance case as absent
/// precisely because it had already succeeded.
fn studied(text: &str, dictionaries: &[Dictionary]) -> Analysis {
    let analyzer =
        sakatsu_core::analyzer::LinderaAnalyzer::new().expect("the embedded UniDic loads");
    sakatsu_core::study::analyse_without_grouping(text, &analyzer, dictionaries)
        .expect("the passage analyses")
}

struct Decided {
    analysis: Analysis,
    found: lexical::Generated,
    decisions: Vec<Decision>,
}

/// Analyse, generate and decide — the one path, so a test cannot measure a different one.
fn decided(text: &str, dictionaries: &[Dictionary]) -> Decided {
    let analysis = studied(text, dictionaries);
    let found = lexical::candidates(&analysis, dictionaries).expect("a valid baseline");
    let matches = sakatsu_core::grammar::patterns(&analysis);
    let places = lexical::occurrences(
        &analysis.tokens,
        &matches,
        lexical::select::GROUPED_CONSTRUCTIONS,
    );
    let (decisions, _) = lexical::decide(&analysis.tokens, &found, &places);

    println!(
        "  {text}\n    baseline {:?}",
        analysis
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect::<Vec<_>>()
    );
    for decision in &decisions {
        match &decision.outcome {
            Outcome::Grouped { candidate, route } => {
                let unit = &found.candidates[*candidate];
                println!(
                    "    grouped [{}..{}] {:?} → {:?} via {} ({})",
                    decision.start,
                    decision.end,
                    unit.surface,
                    unit.dict_form,
                    route.name(),
                    unit.origin.name()
                );
            }
            Outcome::Abstained {
                reason,
                alternatives,
            } => println!(
                "    abstained [{}..{}] {reason}; {} alternative(s)",
                decision.start,
                decision.end,
                alternatives.len()
            ),
        }
    }
    Decided {
        analysis,
        found,
        decisions,
    }
}

fn grouped_surfaces(decided: &Decided) -> Vec<(&str, Route)> {
    decided
        .decisions
        .iter()
        .filter_map(|decision| match &decision.outcome {
            Outcome::Grouped { candidate, route } => Some((
                decided.found.candidates[*candidate].surface.as_str(),
                *route,
            )),
            Outcome::Abstained { .. } => None,
        })
        .collect()
}

/// What the analyzer establishes about each token, and what the unit's record asserts.
///
/// **A report, not a requirement.** The selector's rules are asserted elsewhere; this exists so
/// that a change in UniDic's classification or in a dictionary's tags is visible as a change here
/// rather than as a mysterious abstention.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn what_the_evidence_actually_says_is_recorded() {
    let dictionaries = dictionaries();
    for text in [
        "かっこいい曲",
        "かっこよかった曲",
        "映画とか買い物とかします。",
    ] {
        let analysis = studied(text, &dictionaries);
        println!(
            "
  {text}"
        );
        for token in &analysis.tokens {
            println!(
                "    {:<10} role={:<10} class={:<8} senses={:?}",
                token.surface,
                format!("{:?}", token.lexical_role),
                sakatsu_core::lexical::TokenMorphology::of_token(token)
                    .class
                    .unwrap_or("(none)"),
                token
                    .senses
                    .iter()
                    .map(|sense| sense.pos.as_str())
                    .take(3)
                    .collect::<Vec<_>>()
            );
        }
        let found = lexical::candidates(&analysis, &dictionaries).expect("a valid baseline");
        for candidate in found.candidates.iter().filter(|c| c.joins() > 1) {
            for record in &candidate.evidence {
                println!(
                    "    unit {:<14} {:<10} rules={:?} tags={:?} reading={:?}",
                    candidate.surface, record.term, record.rules, record.tags, record.reading
                );
            }
        }
    }
}

/// The raw record behind the acceptance case, in every field that could carry a class.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn where_a_grammatical_class_could_be_recorded_is_reported() {
    let dictionaries = dictionaries();
    for query in [
        "かっこいい",
        "格好いい",
        "格好良い",
        "とか",
        "可愛い",
        "高い",
    ] {
        println!(
            "
  {query}"
        );
        for dictionary in &dictionaries {
            let hits = dictionary.lookup(query).expect("the copy reads");
            for hit in hits.iter().take(3) {
                let entry = &hit.entry;
                println!(
                    "    {:<8} {:<10} seq={} rules={:?} def_tags={:?} term_tags={:?}",
                    entry.term,
                    entry.reading,
                    entry.sequence,
                    entry.rules,
                    entry.definition_tags,
                    entry.term_tags
                );
                for definition in entry.definitions.iter().take(3) {
                    println!("        sense tags={:?}", definition.tags);
                }
            }
        }
    }
}

/// **The acceptance case the whole slice exists for.**
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn the_split_adjective_is_grouped_automatically() {
    let dictionaries = dictionaries();
    let decided = decided("かっこいい曲", &dictionaries);

    assert!(
        decided
            .analysis
            .tokens
            .iter()
            .any(|token| token.surface == "かっこ"),
        "the baseline must still be the split one, or this proves nothing"
    );
    assert_eq!(
        grouped_surfaces(&decided),
        vec![("かっこいい", Route::Lexical)],
        "the split adjective was not grouped"
    );
}

/// **The inflected acceptance case**, which no whole-form lookup can reach.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn the_inflected_adjective_is_grouped_through_recovery() {
    let dictionaries = dictionaries();
    let decided = decided("かっこよかった曲", &dictionaries);

    let grouped = grouped_surfaces(&decided);
    assert_eq!(grouped, vec![("かっこよかった", Route::Lexical)]);

    // It got there by recovery, not by a whole-form entry: no dictionary files inflected forms.
    let Outcome::Grouped { candidate, .. } = &decided.decisions[0].outcome else {
        panic!("expected a grouping");
    };
    let unit = &decided.found.candidates[*candidate];
    assert_eq!(unit.origin, lexical::Origin::InflectionRecovery);
    assert_eq!(unit.dict_form, "格好いい");
    assert_eq!(unit.lemma_reading.established(), Some("かっこいい"));
}

/// The ordinary adjective control: `かわいい` is already one token, so there is nothing to group
/// and nothing to decide. **A silent pass is the success here.**
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn a_word_the_analyzer_already_joins_is_left_alone() {
    let dictionaries = dictionaries();
    let decided = decided("かわいい人", &dictionaries);
    assert!(
        decided
            .analysis
            .tokens
            .iter()
            .any(|token| token.surface == "かわいい"),
        "かわいい is already one token"
    );
    assert!(
        grouped_surfaces(&decided).is_empty(),
        "nothing needed grouping here"
    );
}

/// **The controls.** Nothing is grouped that the source or the evidence does not support.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn grouping_refuses_what_nothing_supports() {
    let dictionaries = dictionaries();

    // かっこ standing alone: a real word, with nothing after it to join.
    let standalone = decided("かっこを書く", &dictionaries);
    assert!(
        grouped_surfaces(&standalone)
            .iter()
            .all(|(surface, _)| !surface.contains("かっこ")),
        "かっこ was joined to something"
    );

    // A space between the halves. 徘徊する is a word; this is not it.
    let spaced = decided("深夜徘徊 するで", &dictionaries);
    assert!(
        grouped_surfaces(&spaced)
            .iter()
            .all(|(surface, _)| !surface.contains(' ')),
        "a unit was grouped across a space"
    );

    // An ordinary sentence with nothing the whitelist names.
    let plain = decided(
        "図書館で本を読んでいる人がたくさんいますが、今日は静かです。",
        &dictionaries,
    );
    assert!(
        grouped_surfaces(&plain).is_empty(),
        "grouped something in a sentence with no supported structure: {:?}",
        grouped_surfaces(&plain)
    );
}

/// **The construction route**, on the occurrences §3 validated.
///
/// Lindera splits `とか` into `と` + `か`, so each endpoint is a two-token run the matcher already
/// placed. Grouping consumes those spans; it does not search the sentence for `とか`.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn both_enumerative_endpoints_are_grouped_from_their_validated_occurrences() {
    let dictionaries = dictionaries();
    let decided = decided("映画とか買い物とかします。", &dictionaries);

    let grouped = grouped_surfaces(&decided);
    assert_eq!(
        grouped,
        vec![("とか", Route::Construction), ("とか", Route::Construction)],
        "both endpoints must be grouped, and only the endpoints"
    );

    // The two units are different stretches of text, and neither claims the list.
    let spans: Vec<(usize, usize)> = decided
        .decisions
        .iter()
        .map(|decision| (decision.start, decision.end))
        .collect();
    assert_eq!(spans.len(), 2);
    assert_ne!(spans[0], spans[1]);

    // **The listed item is untouched.** 買い物 keeps its own token and its own entry.
    assert!(
        decided
            .analysis
            .tokens
            .iter()
            .any(|token| token.surface == "買い物"),
        "the listed item must survive as itself"
    );
    for (start, end) in &spans {
        let item = decided
            .analysis
            .tokens
            .iter()
            .find(|token| token.surface == "買い物")
            .expect("買い物");
        assert!(
            item.end.unwrap() <= *start || item.start.unwrap() >= *end,
            "a grouping swallowed the listed item"
        );
    }
}

/// **A construction that is not the enumerative one is not grouped.**
///
/// The hearsay `～とか` has no match rule in the pack at all, so it produces no occurrence and
/// there is nothing for grouping to consume. This asserts the consequence rather than the
/// mechanism: a `と` followed by a `か` that the matcher did not place is left alone.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn a_toka_the_matcher_did_not_place_is_not_grouped() {
    let dictionaries = dictionaries();
    // One とか only: the enumerative rule needs two endpoints, so it cannot fire here.
    let decided = decided("明日は雨が降るとか聞きました。", &dictionaries);
    assert!(
        grouped_surfaces(&decided)
            .iter()
            .all(|(surface, _)| *surface != "とか"),
        "a とか the enumerative rule never matched was grouped anyway"
    );
}

/// Every grouping spells exactly the source it claims, stays inside the passage, and covers a
/// contiguous run of whole tokens.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn every_grouping_is_a_whole_run_of_its_own_source() {
    let dictionaries = dictionaries();
    for text in [
        "かっこいい曲",
        "かっこよかった曲",
        "映画とか買い物とかします。",
        "図書館で本を読んでいる人がたくさんいますが、今日は静かです。",
    ] {
        let decided = decided(text, &dictionaries);
        for decision in &decided.decisions {
            let Outcome::Grouped { candidate, .. } = &decision.outcome else {
                continue;
            };
            let unit = &decided.found.candidates[*candidate];
            assert_eq!(
                &decided.analysis.text[unit.start..unit.end],
                unit.surface,
                "{text}: a grouping does not spell its own source"
            );
            assert_eq!(
                (decision.start, decision.end),
                (unit.start, unit.end),
                "{text}: the decision's region is not the unit's span"
            );
            assert!(
                unit.joins() >= 2,
                "{text}: a single token is not a grouping"
            );
            let expected: Vec<usize> = (unit.tokens[0]..=*unit.tokens.last().unwrap()).collect();
            assert_eq!(unit.tokens, expected, "{text}: the tokens are not a run");
        }
    }
}
