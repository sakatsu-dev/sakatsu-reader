//! Contract tests shared by both normalized analyzer adapters.
//!
//! Convergence does not mean identical tokenization — that would erase the reason Ichiran is the
//! richer optional analyzer. It means that whichever adapter answers, downstream code receives the
//! same guarantees: source-grounded, ordered, non-empty tokens; usable word lemmas; nested compound
//! spans; and one exact sentence partition.

mod common;

use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::model::{sentence_bounds, sentence_ranges, Analysis};

fn obeys_normalized_contract(analysis: &Analysis) {
    assert!(
        !analysis.tokens.is_empty(),
        "an analysis must say something"
    );

    let mut previous_end = 0;
    for token in &analysis.tokens {
        assert!(
            !token.surface.is_empty(),
            "empty tokens have no observable meaning"
        );
        let start = token.start.expect("every adapter grounds a token start");
        let end = token.end.expect("every adapter grounds a token end");
        assert!(start >= previous_end, "token spans stay in source order");
        assert!(start < end && end <= analysis.text.len());
        assert_eq!(&analysis.text[start..end], token.surface);
        previous_end = end;

        if token.is_word() {
            assert!(!token.dict_form.is_empty(), "a word has a lookup form");
            assert!(!token.reading.is_empty(), "a word has a display reading");
        }
        for component in &token.components {
            if let (Some(component_start), Some(component_end)) = (component.start, component.end) {
                assert!(component_start >= start && component_end <= end);
                assert_eq!(
                    &analysis.text[component_start..component_end],
                    component.surface
                );
            }
        }
    }

    let ranges = sentence_ranges(&analysis.tokens);
    let mut covered = 0;
    for range in ranges {
        assert_eq!(range.first_token, covered);
        for index in range.first_token..range.last_token_end {
            assert_eq!(
                sentence_bounds(&analysis.tokens, index),
                (range.first_token, range.last_token_end)
            );
        }
        covered = range.last_token_end;
    }
    assert_eq!(covered, analysis.tokens.len());
}

#[test]
fn lindera_and_captured_ichiran_obey_the_same_downstream_contract() {
    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    for (fixture, text) in [
        ("mizu-o-nomu", "水を飲む"),
        ("kimochi-tsutawatte", "気持ちが伝わってきます"),
        ("tabesaserareteita", "食べさせられていた"),
        ("sensei-nasakenai", "先生ってほんと…情けないですね"),
    ] {
        let built_in = lindera
            .analyze(text)
            .expect("Lindera analyzes the fixture text");
        let ichiran = common::analyze(fixture, text);

        obeys_normalized_contract(&built_in);
        obeys_normalized_contract(&ichiran);
        assert_eq!(built_in.text, ichiran.text, "both preserve the same source");
    }
}

/// The same contract, over passages that contain a line break.
///
/// **Lindera only, and not for want of trying.** The Ichiran half of the test above runs on
/// committed captures keyed by name, and there is no multi-line one — capturing it needs the
/// sidecar, so the sidecar path is covered by `core/tests/sentence_lines.rs` instead.
///
/// What this guards is the grounding invariant against text no token covers: `analysis.text` holds
/// a newline byte, every token still has to satisfy `text[start..end] == surface`, and the ranges
/// still have to partition the token list exactly. Nothing here was exercised before a line break
/// could end a sentence.
#[test]
fn a_passage_with_line_breaks_obeys_the_same_contract() {
    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    for text in [
        // Two utterances, no punctuation — two ranges.
        "優等生だね君って\n心配性だね君って",
        // One sentence the panel's edge broke — still one range.
        "６人宿泊させ\nて欲しい",
        // A paragraph gap, and a trailing break with nothing after it.
        "優等生だね君って\n\n心配性だね君って\n",
    ] {
        let analysis = lindera.analyze(text).expect("Lindera analyzes it");
        obeys_normalized_contract(&analysis);
    }
}

/// **The same contract, one stage later — where it was actually breaking.**
///
/// Everything above asserts on *analyzer output*. The reader never sees that: `Pipeline::display`
/// runs afterwards, joining compounds and names and correcting readings, and it is free to build a
/// token whose `surface` is no longer `text[start..end]`.
///
/// It did. `dict::names::merge_names` joined `原` and `俊介` across an untokenized space, taking
/// `first.start .. last.end` while concatenating only the surfaces — so the joined token claimed a
/// span whose source slice was `原 俊介` and whose surface was `原俊介`. Two promoted captures carry
/// it, and nothing caught it because the invariant was asserted one stage before the pass that
/// broke it.
///
/// Needs the dictionaries, because a pipeline without them does nothing worth checking.
#[test]
#[ignore = "needs the installed dictionaries: an empty pipeline joins nothing"]
fn the_display_pipeline_preserves_the_same_contract() {
    let dictionaries = common::installed_dictionaries();
    assert!(
        !dictionaries.is_empty(),
        "this checks what the dictionaries do to spans, so it cannot run without them"
    );
    let pipeline = sakatsu_core::pipeline::Pipeline::new(&dictionaries);
    let lindera = LinderaAnalyzer::new().expect("the dictionary is compiled in");

    for text in [
        // A name whose halves are separated by a space — the shape that broke it. Both the ASCII
        // and the ideographic space, since a Japanese source may use either.
        "原 俊介",
        "原　俊介",
        // And the contiguous form, which must still be joined rather than defensively split.
        "原俊介",
        // Ordinary passages, so the check is not only about names.
        "図書館で本を読んだ。",
        "善福寺川に行きました",
        "これってなんですか",
    ] {
        let mut analysis = lindera.analyze(text).expect("Lindera analyzes it");
        obeys_normalized_contract(&analysis);
        pipeline.display(&mut analysis);
        obeys_normalized_contract(&analysis);
    }
}
