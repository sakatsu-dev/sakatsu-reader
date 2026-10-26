//! The acceptance cases and their controls, at the recovery stage.
//!
//! Recovery only: whether a supported interpretation is *generated*. Selection is a separate
//! decision and nothing here makes it.
//!
//! Run against an isolated dictionary copy whose sequence bucket is verified — `かっこ良い` is
//! filed at −1577490, so without cross-reference resolution this family answers with a pointer.

mod common;

use std::path::PathBuf;

use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::lexical;

fn verified_dictionaries() -> Vec<Dictionary> {
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
        "the copy has no sequence bucket, so cross-reference resolution is inert"
    );
    opened
}

/// The baseline these proposals are made **over**.
///
/// `study::analyse` groups now, so it is no longer the input to generation — it is the output of
/// the whole path. A test that fed it back into `lexical::candidates` would be asking the
/// candidate layer to improve on its own answer, and would report the acceptance case as absent
/// precisely because it had already succeeded.
fn studied(text: &str, dictionaries: &[Dictionary]) -> sakatsu_core::Analysis {
    let analyzer =
        sakatsu_core::analyzer::LinderaAnalyzer::new().expect("the embedded UniDic loads");
    sakatsu_core::study::analyse_without_grouping(text, &analyzer, dictionaries)
        .expect("the passage analyses")
}

fn surfaces(analysis: &sakatsu_core::Analysis) -> Vec<&str> {
    analysis
        .tokens
        .iter()
        .map(|token| token.surface.as_str())
        .collect()
}

/// **The acceptance case.** `かっこいい` arrives split, and the whole-word interpretation must be
/// proposed with evidence that can actually support a lexical unit.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn the_split_adjective_is_recovered_with_lexical_support() {
    let dictionaries = verified_dictionaries();
    let analysis = studied("かっこいい曲", &dictionaries);
    println!("  baseline {:?}", surfaces(&analysis));

    let found = lexical::candidates(&analysis, &dictionaries).expect("a valid baseline");
    let recovered = found
        .groupings()
        .find(|candidate| candidate.surface == "かっこいい")
        .expect("かっこいい was not proposed");

    println!(
        "  proposal {:?} dict_form={:?} joins {} evidence={:?}",
        recovered.surface,
        recovered.dict_form,
        recovered.joins(),
        recovered
            .evidence_kinds()
            .iter()
            .map(|kind| kind.name())
            .collect::<Vec<_>>()
    );
    assert!(
        recovered.supported_as_lexical_unit(),
        "proposed without evidence that can support a word"
    );
    // The reading is established, not assembled from a partial one.
    assert!(
        recovered.lemma_reading.established().is_some(),
        "every constituent contributed a reading, so this one is established"
    );
}

/// **The inflected acceptance case, which no whole-form lookup can reach.**
///
/// `かっこよかった` is not a headword in any dictionary and never will be — inflected forms are
/// not filed — so a generation step that required a whole-form hit before trying anything else
/// would refuse this span before asking the question. Recovery runs on the enumerated span
/// instead, and reaches 格好いい through the reading.
///
/// What grounds it is the **head constituent's** morphology, and the candidate says so: the
/// analyzer described よかった, not かっこよかった, so calling the span an i-adjective is a claim
/// about its structure. Selection is where that claim is weighed; generation's job is to make it
/// visible rather than to let it pass as a fact.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn an_inflected_span_is_recovered_without_a_whole_form_entry() {
    let dictionaries = verified_dictionaries();
    let analysis = studied("かっこよかった曲", &dictionaries);
    println!("  baseline {:?}", surfaces(&analysis));

    let found = lexical::candidates(&analysis, &dictionaries).expect("a valid baseline");
    let recovered: Vec<&sakatsu_core::lexical::Candidate> = found
        .candidates
        .iter()
        .filter(|candidate| candidate.surface == "かっこよかった")
        .collect();
    for candidate in &recovered {
        println!(
            "  {:?} → {:?} reading {:?} origin {} {:?}",
            candidate.surface,
            candidate.dict_form,
            candidate.lemma_reading.established(),
            candidate.origin.name(),
            candidate.derivation
        );
    }
    assert!(
        !recovered.is_empty(),
        "no candidate covered the inflected span at all"
    );

    // **No whole-form entry exists**, so anything reaching this span came from recovery.
    assert!(
        recovered
            .iter()
            .all(|candidate| candidate.origin == sakatsu_core::lexical::Origin::InflectionRecovery),
        "a whole-form lookup is not what reached this span"
    );

    let headword = recovered
        .iter()
        .find(|candidate| candidate.lemma_reading.established() == Some("かっこいい"))
        .expect("かっこよかった must recover かっこいい");
    assert_eq!(headword.dict_form, "格好いい");
    assert!(
        headword.supported_as_lexical_unit(),
        "recovered without evidence that can support a word"
    );
    // The source is never rewritten.
    assert_eq!(
        &analysis.text[headword.start..headword.end],
        headword.surface
    );

    // **The ground is recorded as composed**, because the analyzer described a part of this span.
    let sakatsu_core::lexical::Derivation::Inflection { ground, class, .. } = &headword.derivation
    else {
        panic!("a recovered candidate must carry its derivation: {headword:?}");
    };
    assert_eq!(*class, "adj-i");
    let sakatsu_core::lexical::InflectionGround::ComposedMorphology { head, class } = ground else {
        panic!("the analyzer did not describe this span, so the ground is composed: {ground:?}");
    };
    assert_eq!(class, "adj-i");
    assert_eq!(
        analysis.tokens[*head].surface, "よかった",
        "the head constituent is what carries the inflection"
    );
    assert!(
        !ground.is_about_the_whole_span(),
        "a composed class is a hypothesis about structure, and must not read as one about the span"
    );
    assert!(
        ground.is_grounded(),
        "…but it is still evidence, not a bare reading match"
    );

    // And the search that produced it completed.
    assert!(
        !found.lookup_failed_over(headword.start, headword.end),
        "a dictionary failed over this span: {:?}",
        found.failures
    );
}

/// A word the analyzer already gets right needs no proposal, and that is the success.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn a_correctly_segmented_word_needs_no_grouping() {
    let dictionaries = verified_dictionaries();
    let analysis = studied("かわいい人", &dictionaries);
    assert!(
        surfaces(&analysis).contains(&"かわいい"),
        "かわいい is already one token: {:?}",
        surfaces(&analysis)
    );
    let found = lexical::candidates(&analysis, &dictionaries).expect("a valid baseline");
    assert!(
        found
            .groupings()
            .all(|candidate| candidate.surface != "かわいい"),
        "nothing needed grouping here"
    );
}

/// **The controls.** Recovery proposes nothing the source does not join.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn recovery_refuses_what_the_source_does_not_join() {
    let dictionaries = verified_dictionaries();

    // A space between the halves: 徘徊する is a word, but this is not it.
    let spaced = studied("深夜徘徊 するで", &dictionaries);
    let found = lexical::candidates(&spaced, &dictionaries).expect("a valid baseline");
    assert!(
        found
            .groupings()
            .all(|candidate| !candidate.surface.contains(' ') && candidate.surface != "徘徊する"),
        "a unit was proposed across a space"
    );
    // The space is not part of any token, so the two halves are not contiguous — and that
    // refusal is recorded rather than breaking out silently.
    assert!(
        found
            .refused
            .iter()
            .any(|refusal| refusal.reason.contains("contiguous")),
        "the gap between 徘徊 and する is recorded as a refusal: {:?}",
        found.refused
    );

    // 括弧, 格好, 各個, 確固 — かっこ is a real word, and nothing follows it to group with.
    let standalone = studied("かっこを書く", &dictionaries);
    let found = lexical::candidates(&standalone, &dictionaries).expect("a valid baseline");
    assert!(
        found.groupings().all(|candidate| {
            !(candidate.surface.starts_with("かっこ") && candidate.surface.contains("いい"))
        }),
        "nothing here supports かっこいい"
    );
}

/// Every proposal spells exactly the source it claims, and stays inside the bounds.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn proposals_stay_bounded_and_spell_their_own_source() {
    let dictionaries = verified_dictionaries();
    let analysis = studied(
        "図書館で本を読んでいる人がたくさんいますが、今日は静かです。",
        &dictionaries,
    );
    let found = lexical::candidates(&analysis, &dictionaries).expect("a valid baseline");
    for candidate in &found.candidates {
        assert!(candidate.joins() <= lexical::MOST_TOKENS);
        assert!(candidate.surface.chars().count() <= lexical::MOST_CHARS);
        assert_eq!(
            &analysis.text[candidate.start..candidate.end],
            candidate.surface,
            "a proposal does not spell its own source"
        );
    }
    println!(
        "  {} tokens produced {} candidates; {} unsearched extensions, {} source refusals",
        analysis.tokens.len(),
        found.candidates.len(),
        found.unsearched.len(),
        found.refused.len()
    );
}
