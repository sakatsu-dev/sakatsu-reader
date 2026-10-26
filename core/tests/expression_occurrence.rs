//! Construction occurrences, end to end: real analyzer, real pack, real dictionary.
//!
//! # What only this level can check
//!
//! The unit tests supply their own tokens and their own match, so they establish the mechanism.
//! They cannot establish that the enumerative rule **fires on what Lindera actually produces**,
//! that the element spans it leaves behind land on the とか the reader can see, or that a
//! dictionary answers for them. Each of those is a separate way for the chain to be broken while
//! every unit test stays green.
//!
//! # What is asserted, and what is only reported
//!
//! Asserted: both endpoints reach an offer of their own, the offers have distinct spans, each
//! offer stays inside its parent and never claims the parent's extent, the parent's own
//! explanation comes back with it, and the listed item between the endpoints is refused.
//!
//! Reported: the segmentation itself. How Lindera splits `映画とか` is a finding, not a
//! requirement, and pinning it here would turn an analyzer change into a failure in the wrong
//! file.
//!
//! Run against the isolated copy; the reader's own store is never opened.

mod common;

use std::path::PathBuf;

use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::expression::{offered, ExpressionOffer};
use sakatsu_core::grammar::Match;
use sakatsu_core::model::Token;

/// The imported pack's enumerative rule: `[{surface_ends とか}, {0,6}, {surface_ends とか}]`.
const ENUMERATIVE: &str = "hanabira-n4-a-b";

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
    opened
}

struct Recognised {
    tokens: Vec<Token>,
    found: Match,
}

fn recognise(text: &str) -> Recognised {
    let analyzer =
        sakatsu_core::analyzer::LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let analysis = sakatsu_core::analyzer::Analyzer::analyze(&analyzer, text).expect("it analyses");
    let matches = sakatsu_core::grammar::patterns(&analysis);
    println!(
        "  {text}\n    tokens {:?}",
        analysis
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect::<Vec<_>>()
    );
    let found = matches
        .into_iter()
        .find(|found| found.pattern_id == ENUMERATIVE)
        .unwrap_or_else(|| panic!("{ENUMERATIVE} did not fire on {text:?}"));
    println!(
        "    match tokens {}..{} bytes {:?}..{:?}, elements {:?}",
        found.first_token,
        found.last_token_end,
        found.start,
        found.end,
        found
            .elements
            .iter()
            .map(|span| (
                span.element,
                span.first_token,
                span.last_token_end,
                span.suffix()
            ))
            .collect::<Vec<_>>()
    );
    Recognised {
        tokens: analysis.tokens,
        found,
    }
}

fn offer_for(
    case: &Recognised,
    index: usize,
    dictionaries: &[Dictionary],
) -> Option<ExpressionOffer> {
    offered(
        &case.tokens,
        index,
        std::slice::from_ref(&case.found),
        dictionaries,
    )
    .expect("the dictionary is readable")
}

/// **Both endpoints reach their own assistance, and the item between them does not.**
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn both_enumerative_endpoints_reach_their_own_entry() {
    let dictionaries = dictionaries();
    let case = recognise("映画とか買い物とかします。");
    let (parent_start, parent_end) = (
        case.found.start.expect("a grounded match"),
        case.found.end.expect("a grounded match"),
    );

    let mut offers: Vec<(usize, ExpressionOffer)> = Vec::new();
    for index in 0..case.tokens.len() {
        let Some(offer) = offer_for(&case, index, &dictionaries) else {
            println!(
                "    {index:>2} {:<10} — no offer",
                case.tokens[index].surface
            );
            continue;
        };
        println!(
            "    {index:>2} {:<10} → {} [{}..{}] element {:?} in {:?} ({} senses)",
            case.tokens[index].surface,
            offer.expression,
            offer.start,
            offer.end,
            offer.occurrence.as_ref().map(|within| within.element),
            offer.title,
            offer.senses.len()
        );
        offers.push((index, offer));
    }

    // Two distinct occurrences, at the two rule positions the pack names as endpoints.
    let positions: std::collections::BTreeSet<usize> = offers
        .iter()
        .filter_map(|(_, offer)| offer.occurrence.as_ref().map(|within| within.element))
        .collect();
    assert_eq!(
        positions,
        [0usize, 2].into_iter().collect(),
        "both endpoints must be reachable, and only the endpoints"
    );

    let spans: std::collections::BTreeSet<(usize, usize)> = offers
        .iter()
        .map(|(_, offer)| (offer.start, offer.end))
        .collect();
    assert_eq!(
        spans.len(),
        2,
        "the two occurrences are different pieces of text"
    );

    for (index, offer) in &offers {
        let within = offer.occurrence.as_ref().expect("an occurrence");
        assert_eq!(offer.expression, "とか");
        assert!(
            !offer.senses.is_empty(),
            "token {index}: an offer with no senses is not assistance"
        );
        // **The parent's explanation is preserved**, and the offer does not become it.
        assert_eq!(offer.pattern_id, ENUMERATIVE);
        assert!(!offer.title.is_empty());
        assert_eq!((within.start, within.end), (parent_start, parent_end));
        assert!(
            parent_start <= offer.start && offer.end <= parent_end,
            "token {index}: an occurrence outside its own construction"
        );
        assert_ne!(
            (offer.start, offer.end),
            (parent_start, parent_end),
            "token {index}: the occurrence must not claim the whole list"
        );
        // The selected token is inside the offer, or holds it.
        let token = &case.tokens[*index];
        assert!(
            token.start.is_some_and(|at| at < offer.end)
                && token.end.is_some_and(|to| to > offer.start),
            "token {index}: the offer does not overlap the selection it was made for"
        );
    }

    // **The listed item is refused.** 買い物 is inside the construction and is not an endpoint;
    // it has its own entry and must not be handed とか's.
    let item = case
        .tokens
        .iter()
        .position(|token| token.surface == "買い物")
        .expect("買い物 is a token here");
    assert!(
        item >= case.found.first_token && item < case.found.last_token_end,
        "the item must be inside the match for this to be a real exclusion"
    );
    assert!(
        offer_for(&case, item, &dictionaries).is_none(),
        "the listed item between the endpoints was offered an endpoint's entry"
    );
}

/// The same construction with a longer list: the endpoints are still the endpoints, and
/// everything between them is still excluded.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn a_longer_list_offers_only_its_endpoints() {
    let dictionaries = dictionaries();
    let case = recognise("週末は映画とか買い物とか楽しいことをしたいです。");

    let mut offered_tokens = Vec::new();
    for index in 0..case.tokens.len() {
        if offer_for(&case, index, &dictionaries).is_some() {
            offered_tokens.push(case.tokens[index].surface.clone());
        }
    }
    println!("    offered on {offered_tokens:?}");
    assert!(!offered_tokens.is_empty(), "neither endpoint was reachable");
    for surface in &offered_tokens {
        assert!(
            "とか".contains(surface.as_str()) || surface.ends_with("とか"),
            "{surface} is not part of a とか and must not have been offered one"
        );
    }
}

/// **A match without element spans offers nothing**, which is what an older interface sends.
///
/// The transport is backward compatible by refusing, never by guessing: a payload that predates
/// the field arrives with no rule positions, and a scan for the characters would find the same
/// とか without the recogniser having put it there.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn a_match_from_an_older_interface_offers_nothing() {
    let dictionaries = dictionaries();
    let case = recognise("映画とか買い物とかします。");

    // Exactly what `serde` produces for a payload without the field.
    let wire = serde_json::json!({
        "pattern_id": case.found.pattern_id,
        "first_token": case.found.first_token,
        "last_token_end": case.found.last_token_end,
        "start": case.found.start,
        "end": case.found.end,
    });
    let older: Match = serde_json::from_value(wire).expect("the field defaults");
    assert!(older.elements.is_empty());

    for index in 0..case.tokens.len() {
        assert!(
            offered(
                &case.tokens,
                index,
                std::slice::from_ref(&older),
                &dictionaries
            )
            .expect("the dictionary is readable")
            .is_none(),
            "token {index} was offered an occurrence with no rule position behind it"
        );
    }
}
