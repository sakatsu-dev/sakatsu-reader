//! Inflection recovery against the **installed** dictionary records.
//!
//! # What only this level can check
//!
//! The unit tests supply their own records, so they establish the mechanism and nothing about the
//! data. The data is where this case lives: **every `かっこいい` record carries `rules: []`**, so
//! the acceptance family runs entirely on the absent-class path, while 可愛い carries `adj-i` and
//! cannot. A fixture can assert either; only the installed index can show which one is real.
//!
//! # Two kinds of test here, and the difference matters
//!
//! Most of these **supply** the analyzer's class, which makes them tests of the installed
//! dictionary given an assumed adjective analysis. That is a real question — it is the one the
//! records answer — but it is not the question of whether the reader's own pipeline supplies that
//! evidence.
//!
//! So [`morphology_comes_from_the_analyzer_not_from_the_test`] and its companions derive the
//! morphology from **actual Lindera output** and pass whatever is there, including nothing.
//! Missing evidence stays missing, and where it is missing the outcome is
//! [`Recovered::Unresolved`] rather than an accepted proposal.
//!
//! # What "absent" has to mean
//!
//! A matching reading establishes a possible word, not its grammatical class. So where a record
//! asserts nothing, the claim has to come from somewhere else — the analyzer's own morphology —
//! and a proposal resting on a reading alone is reported as unresolved rather than accepted.
//!
//! Run against the isolated copy; the reader's own store is never opened.

mod common;

use std::path::PathBuf;

use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::lexical::candidate::ClassAgreement;
use sakatsu_core::lexical::inflect::{
    reading_lookup, recover_adjective, Recovered, Recovery, SameWordRelationship, TokenMorphology,
};
use sakatsu_core::model::Token;

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
        "the copy has no sequence bucket, so かっこよい and かっこいい cannot be shown to be one word"
    );
    opened
}

/// Every dictionary answered, so this outcome is the whole outcome.
fn complete(recovery: &Recovery, what: &str) {
    assert!(
        recovery.complete(),
        "{what}: a dictionary failed, so nothing here is established: {:?}",
        recovery.failures
    );
}

/// The one interpretation this surface has, or a failure naming what came back instead.
fn sole<'a>(recovery: &'a Recovery, what: &str) -> &'a sakatsu_core::lexical::Proposal {
    let Recovered::Proposal(lemma) = &recovery.outcome else {
        panic!("{what} was not recovered: {:?}", recovery.outcome);
    };
    assert_eq!(
        lemma.interpretations.len(),
        1,
        "{what}: expected one interpretation, got {:?}",
        lemma.identities()
    );
    &lemma.interpretations[0]
}

/// **The acceptance family, on the installed data.**
///
/// Every form goes through the absent-class path, because that is what the records say — so the
/// analyzer's class is what grounds them, and this test supplies it deliberately.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn the_acceptance_family_recovers_through_the_absent_rules_path() {
    let dictionaries = dictionaries();
    let mut look = reading_lookup(&dictionaries);

    for surface in [
        "かっこよかった",
        "かっこよくない",
        "かっこよくなかった",
        "かっこよく",
        "かっこよければ",
    ] {
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: surface,
                // The analyzer's own class, which is what carries the claim here. Supplied, and
                // `morphology_comes_from_the_analyzer_not_from_the_test` is where that is not.
                class: Some("adj-i"),
            },
            &mut look,
        );
        complete(&recovery, surface);
        let proposal = sole(&recovery, surface);
        let variant = proposal
            .variant
            .as_ref()
            .expect("the sibling record that establishes the headword reading");
        println!(
            "  {surface:<20} → {:<10} form={:<13} via {} ({} {}) class={}/{} entry={:?} seq={:?}",
            proposal.lemma_reading,
            proposal.form.name(),
            variant.entry.term,
            variant.relationship.name(),
            variant.relationship.target(),
            proposal.agreement.name(),
            proposal.entry_supports_transformation.name(),
            proposal.entry.term,
            proposal.entry.id.sequence
        );
        assert_eq!(
            proposal.lemma_reading, "かっこいい",
            "{surface}: the headword reading is the いい form"
        );
        assert!(
            proposal.suppletive,
            "{surface}: reached かっこよい, and the same word records かっこいい"
        );
        assert_eq!(
            proposal.agreement,
            ClassAgreement::Absent,
            "{surface}: the installed records assert no class, which is the path under test"
        );
        assert_eq!(
            proposal.entry_supports_transformation,
            ClassAgreement::Absent,
            "{surface}: the record says nothing about the transformation either"
        );
        assert_eq!(
            proposal.analyzer_supports_transformation,
            ClassAgreement::Compatible,
            "{surface}: so the analyzer's class is the only thing grounding this"
        );
        assert!(
            proposal.entry.rules.is_empty(),
            "{surface}: 格好良い carries no rules — if this changes, the path changes too"
        );

        // **The relationship is evidence, not commentary.** Both records are kept, and what ties
        // them to one word is a value.
        assert_eq!(variant.entry.reading, "かっこいい");
        assert!(
            matches!(
                variant.relationship,
                SameWordRelationship::Sequence(_) | SameWordRelationship::CrossReference(_)
            ),
            "{surface}: the two spellings must be tied by the data"
        );
        assert_eq!(
            variant.relationship.target(),
            proposal
                .entry
                .id
                .same_word_as
                .or(proposal.entry.id.sequence)
                .expect("the supporting record is filed somewhere"),
            "{surface}: the sibling is the same word as the record that reached it"
        );
    }
}

/// **Missing evidence stays missing.** Without the analyzer's class, the installed records — which
/// assert none — leave the interpretation unresolved rather than accepted.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn the_acceptance_family_is_unresolved_without_independent_evidence() {
    let dictionaries = dictionaries();
    let mut look = reading_lookup(&dictionaries);

    let recovery = recover_adjective(
        &TokenMorphology {
            reading: "かっこよかった",
            class: None,
        },
        &mut look,
    );
    complete(&recovery, "かっこよかった");
    let Recovered::Unresolved(lemma) = &recovery.outcome else {
        panic!(
            "a reading match is not evidence of a class: {:?}",
            recovery.outcome
        );
    };
    println!(
        "  かっこよかった (no analyzer class) → unresolved {} over {} interpretation(s)",
        lemma.lemma_reading,
        lemma.interpretations.len()
    );
    assert_eq!(lemma.lemma_reading, "かっこいい");
    assert!(!lemma.independently_grounded());
}

/// **The morphology comes from the analyzer, not from the test.**
///
/// The tests above supply `adj-i`, which makes them questions about the dictionary given an
/// assumed analysis. This one analyses real text, reads whatever Lindera established for each
/// token, and passes it through unchanged — including when there is nothing to pass.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn morphology_comes_from_the_analyzer_not_from_the_test() {
    let dictionaries = dictionaries();
    let analyzer =
        sakatsu_core::analyzer::LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let mut look = reading_lookup(&dictionaries);

    for text in [
        "かっこよかった曲",
        "かっこいい曲",
        "かわいかった人",
        "たかかった本",
        "その本は高かった",
    ] {
        let analysis =
            sakatsu_core::study::analyse(text, &analyzer, &dictionaries).expect("it analyses");
        println!("\n  {text}");
        for token in &analysis.tokens {
            let morphology = TokenMorphology::of_token(token);
            let recovery = recover_adjective(&morphology, &mut look);
            complete(&recovery, text);
            println!(
                "    {:<10} reading={:<12} analyzer class={:<8} → {}",
                token.surface,
                token.reading,
                morphology.class.unwrap_or("(none)"),
                describe(&recovery.outcome)
            );

            // **Whatever the analyzer said is what was used.** Nothing is filled in, and a token
            // the analyzer said nothing about cannot be automatically selected on a reading alone.
            assert_eq!(
                morphology.class,
                analyzer_class(token).as_deref(),
                "{}: the morphology must be read, not invented",
                token.surface
            );
            if morphology.class.is_none() {
                if let Recovered::Proposal(lemma) = &recovery.outcome {
                    assert!(
                        lemma.interpretations.iter().all(|proposal| {
                            proposal.entry_supports_transformation == ClassAgreement::Compatible
                        }),
                        "{}: with no analyzer class, only the record may ground a proposal: {:?}",
                        token.surface,
                        lemma.lemma_reading
                    );
                }
            }
        }
    }
}

/// **The acceptance span, through the analyzer.**
///
/// `かっこいい` is the case this layer exists for, and what the analyzer supplies for it is a
/// finding in its own right rather than something to assume. Recorded here so that a change in
/// segmentation or morphology is visible.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn what_the_analyzer_supplies_for_the_acceptance_span_is_recorded() {
    let dictionaries = dictionaries();
    let analyzer =
        sakatsu_core::analyzer::LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let mut look = reading_lookup(&dictionaries);

    let analysis = sakatsu_core::study::analyse("かっこよかった曲", &analyzer, &dictionaries)
        .expect("it analyses");
    let surfaces: Vec<&str> = analysis
        .tokens
        .iter()
        .map(|token| token.surface.as_str())
        .collect();
    println!("  baseline {surfaces:?}");

    // Whichever token carries the inflected reading, recovery is run on the analyzer's own
    // evidence for it — and the outcome is reported, not asserted into existence.
    for token in &analysis.tokens {
        let morphology = TokenMorphology::of_token(token);
        let recovery = recover_adjective(&morphology, &mut look);
        complete(&recovery, &token.surface);
        println!(
            "    {:<12} reading={:<12} class={:<8} → {}",
            token.surface,
            token.reading,
            morphology.class.unwrap_or("(none)"),
            describe(&recovery.outcome)
        );
        // No token may be carried to かっこいい on a reading alone.
        if let Recovered::Proposal(lemma) = &recovery.outcome {
            if lemma.lemma_reading == "かっこいい" {
                assert!(
                    lemma.independently_grounded(),
                    "{}: かっこいい was accepted with nothing but a matching reading",
                    token.surface
                );
            }
        }
    }
}

/// The analyzer's class, read the long way, so the helper cannot quietly agree with itself.
fn analyzer_class(token: &Token) -> Option<String> {
    let conjugation = token.conjugation.as_ref()?;
    let prop = conjugation
        .props
        .iter()
        .find(|prop| prop.is_base)
        .or_else(|| conjugation.props.last())?;
    let pos = prop.pos.trim();
    (!pos.is_empty()).then(|| pos.to_string())
}

fn describe(outcome: &Recovered) -> String {
    match outcome {
        Recovered::Proposal(lemma) => format!(
            "proposal {} ({} interpretation(s), grounded)",
            lemma.lemma_reading,
            lemma.interpretations.len()
        ),
        Recovered::Unresolved(lemma) => format!(
            "unresolved {} (reading only: {:?})",
            lemma.lemma_reading,
            lemma
                .interpretations
                .iter()
                .map(|proposal| proposal.entry.term.as_str())
                .collect::<Vec<_>>()
        ),
        Recovered::Ambiguous(groups) => format!(
            "ambiguous {:?}",
            groups
                .iter()
                .map(|group| group.lemma_reading.as_str())
                .collect::<Vec<_>>()
        ),
        Recovered::Contradicted {
            form,
            analyzer_class,
            ..
        } => format!(
            "contradicted ({analyzer_class} is not an adjective: {})",
            form.name()
        ),
        Recovered::Unsupported => "unsupported (not an inflected i-adjective)".to_string(),
        Recovered::Unvalidated { proposed_reading } => {
            format!("unvalidated ({proposed_reading} reaches nothing)")
        }
        Recovered::Unavailable { proposed_reading } => {
            format!("unavailable ({proposed_reading} could not be checked)")
        }
    }
}

/// **The counterexample, on the installed data.**
///
/// 可愛い carries `adj-i`, so it cannot reach the absent path — and かわよかった belongs to
/// かわよい, a word of its own, rather than to かわいい.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn the_regular_adjective_stays_regular_on_the_installed_records() {
    let dictionaries = dictionaries();
    let mut look = reading_lookup(&dictionaries);

    let recovery = recover_adjective(
        &TokenMorphology {
            reading: "かわいかった",
            class: Some("adj-i"),
        },
        &mut look,
    );
    complete(&recovery, "かわいかった");
    let Recovered::Proposal(lemma) = &recovery.outcome else {
        panic!("かわいかった was not recovered: {:?}", recovery.outcome);
    };
    println!(
        "  かわいかった → {} over {:?}",
        lemma.lemma_reading,
        lemma
            .interpretations
            .iter()
            .map(|proposal| (proposal.entry.term.as_str(), proposal.entry.id.sequence))
            .collect::<Vec<_>>()
    );
    assert_eq!(lemma.lemma_reading, "かわいい");
    for proposal in &lemma.interpretations {
        assert!(!proposal.suppletive, "かわいい inflects regularly");
        assert!(proposal.variant.is_none());
    }
    assert!(
        lemma
            .interpretations
            .iter()
            .any(|proposal| proposal.agreement == ClassAgreement::Compatible),
        "可愛い asserts adj-i, so this case cannot hide behind the absent path"
    );

    // **And かわよかった never reaches かわいい.**
    //
    // The installed data corrected an assumption here: かわよい *is* an entry — Jitendex 2845536,
    // `adj-i` — so かわよかった is a real inflection of its own word. The control is therefore the
    // *direction*, not the existence: what must not happen is the suppletive stem carrying it to
    // かわいい, which is what a rule keyed on "the stem ends in よ" did.
    let recovery = recover_adjective(
        &TokenMorphology {
            reading: "かわよかった",
            class: Some("adj-i"),
        },
        &mut look,
    );
    complete(&recovery, "かわよかった");
    let proposal = sole(&recovery, "かわよかった");
    println!(
        "  かわよかった → {} suppletive={} seq={:?}",
        proposal.lemma_reading, proposal.suppletive, proposal.entry.id.sequence
    );
    assert_eq!(
        proposal.lemma_reading, "かわよい",
        "かわよかった is a form of かわよい"
    );
    assert_ne!(
        proposal.lemma_reading, "かわいい",
        "and must never be carried to かわいい, which inflects regularly"
    );
    assert!(
        !proposal.suppletive,
        "no suppletive relationship is recorded between かわよい and かわいい"
    );
    assert!(proposal.variant.is_none());
}

/// A reading alone does not establish a class, and the report says which evidence carried it.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn a_class_that_disagrees_rejects_the_record_on_installed_data() {
    let dictionaries = dictionaries();
    let mut look = reading_lookup(&dictionaries);

    // 可愛い is adj-i. Offering the analyzer's verb class must reject it rather than pool — and
    // the rejection now comes from the transformation itself, which a verb analysis contradicts
    // before any record is consulted.
    let recovery = recover_adjective(
        &TokenMorphology {
            reading: "かわいかった",
            class: Some("v5r"),
        },
        &mut look,
    );
    let Recovered::Contradicted { analyzer_class, .. } = &recovery.outcome else {
        panic!(
            "a v5r analysis cannot validate an i-adjective transformation: {:?}",
            recovery.outcome
        );
    };
    assert_eq!(analyzer_class, "v5r");
}

/// Ordinary adjectives that *do* carry rules, as the independent control on installed data.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn ordinary_adjectives_recover_with_a_compatible_class() {
    let dictionaries = dictionaries();
    let mut look = reading_lookup(&dictionaries);

    for (surface, lemma_reading) in [
        ("たかかった", "たかい"),
        ("おいしくない", "おいしい"),
        ("さむければ", "さむい"),
        ("あたらしく", "あたらしい"),
    ] {
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: surface,
                class: Some("adj-i"),
            },
            &mut look,
        );
        complete(&recovery, surface);
        let Recovered::Proposal(lemma) = &recovery.outcome else {
            panic!("{surface} was not recovered: {:?}", recovery.outcome);
        };
        println!(
            "  {surface:<14} → {:<12} over {:?}",
            lemma.lemma_reading,
            lemma
                .interpretations
                .iter()
                .map(|proposal| (proposal.entry.term.as_str(), proposal.agreement.name()))
                .collect::<Vec<_>>()
        );
        assert_eq!(lemma.lemma_reading, lemma_reading);
        assert!(lemma.interpretations.iter().all(|p| !p.suppletive));
        assert!(
            lemma
                .interpretations
                .iter()
                .any(|proposal| proposal.agreement == ClassAgreement::Compatible),
            "{surface}: these records do assert adj-i"
        );
    }
}

/// **Every supporting word survives, and lookup order does not choose one.**
///
/// On the installed data a lemma reading routinely reaches several records; before, the first one
/// found became the answer and the rest were dropped. Reversing the order the dictionaries are
/// consulted in must give the same set.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn the_interpretation_set_does_not_depend_on_lookup_order() {
    let forward = dictionaries();
    let mut reversed = dictionaries();
    reversed.reverse();

    for surface in [
        "たかかった",
        "かっこよかった",
        "かわいかった",
        "おいしくない",
    ] {
        let mut look = reading_lookup(&forward);
        let first = recover_adjective(
            &TokenMorphology {
                reading: surface,
                class: Some("adj-i"),
            },
            &mut look,
        );
        let mut look = reading_lookup(&reversed);
        let second = recover_adjective(
            &TokenMorphology {
                reading: surface,
                class: Some("adj-i"),
            },
            &mut look,
        );
        complete(&first, surface);
        complete(&second, surface);
        assert_eq!(
            first.outcome, second.outcome,
            "{surface}: the outcome depends on the order the dictionaries answered in"
        );
        if let Recovered::Proposal(lemma) = &first.outcome {
            println!(
                "  {surface:<14} {} interpretation(s): {:?}",
                lemma.interpretations.len(),
                lemma
                    .interpretations
                    .iter()
                    .map(|proposal| proposal.entry.term.as_str())
                    .collect::<Vec<_>>()
            );
        }
    }
}
