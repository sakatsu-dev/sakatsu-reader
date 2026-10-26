//! **The Phase 1 gate** (`project notes` §10).
//!
//! Real `ichiran-cli` output → the normalized model → the grammar engine, asserted
//! end to end on the four sentences the design nominates. What is being verified:
//! base-verb resolution, form names, derivation order, compound auxiliaries, the
//! absence of a phantom Potential, and word-specific formations.
//!
//! > *"If these pass in Rust, the port is sound."*
//!
//! These assert on rendered strings on purpose. A learner reads the ladder, so the
//! ladder is what the test reads too — a passing test here means the grammar view
//! says something true.

mod common;

use common::analyze;
use sakatsu_core::grammar::{self, breakdown, pack, Form};
use sakatsu_core::model::{Analysis, Token};

/// The ladder as a learner reads it: each rung's title, plus how it is built.
fn ladder(token: &Token) -> Vec<String> {
    breakdown(token)
        .steps
        .iter()
        .map(|step| {
            let mut line = step.form.title().to_string();
            if step.neg {
                line.push_str(" (negative)");
            }
            if step.fml {
                line.push_str(" (formal)");
            }
            match &step.formation {
                Some(formation) => {
                    line.push_str(&format!(
                        ": {} → {} [{}]",
                        formation.from, formation.result, formation.change
                    ));
                }
                None => line.push_str(": —"),
            }
            line
        })
        .collect()
}

/// The auxiliary patterns as written, resolved through the grammar pack — which is
/// the only place the text exists.
fn patterns(token: &Token) -> Vec<&'static str> {
    breakdown(token)
        .auxiliaries
        .iter()
        .map(|auxiliary| {
            auxiliary
                .pattern()
                .expect("the auxiliary's pattern is defined in the core pack")
                .pattern
                .as_str()
        })
        .collect()
}

/// The word the formation chain ends up building, if every rung was derivable.
///
/// This is the ladder's self-check: rebuilding from the dictionary form must land on
/// the reading actually in the sentence. If it does not, some rung is describing a
/// different word.
fn rebuilt(token: &Token) -> Option<String> {
    let steps = breakdown(token).steps;
    let last = steps.last()?;
    let result = last.formation.as_ref()?.result.clone();
    assert_eq!(
        result, token.reading,
        "the rebuilt word must match the surface reading of {}",
        token.surface
    );
    Some(result)
}

fn forms(token: &Token) -> Vec<Form> {
    breakdown(token)
        .steps
        .iter()
        .map(|step| step.form)
        .collect()
}

/// 気持ちが伝わってきます — the compound whose senses hide on its components, whose
/// auxiliary is 〜てくる, and whose lemma reading Ichiran writes as 【クる】.
#[test]
fn gate_kimochi_ga_tsutawatte_kimasu() {
    let analysis = analyze("kimochi-tsutawatte", "気持ちが伝わってきます");
    let compound = &analysis.tokens[2];

    assert_eq!(
        ladder(compound),
        [
            "Te-form (〜て): つたわる → つたわって [る → って]",
            "Non-past (formal): くる → きます [irregular: くる → きます]",
        ]
    );
    assert_eq!(patterns(compound), ["〜てくる"]);

    // 気持ち keeps its own identity: a noun, with no grammar to explain.
    let kimochi = &analysis.tokens[0];
    assert!(breakdown(kimochi).is_empty());
}

/// 食べさせられていた — §10's deepest derivation. The learner needs
/// 食べる → causative-passive → te → past, in that order, across two components.
#[test]
fn gate_tabesaserarete_ita() {
    let analysis = analyze("tabesaserareteita", "食べさせられていた");
    let compound = &analysis.tokens[0];

    assert_eq!(
        ladder(compound),
        [
            "Causative-passive: たべる → たべさせられる [drop る, then add させられる]",
            // Built on the causative-passive, not on 食べる: たべて would be a real
            // word and the wrong one.
            "Te-form (〜て): たべさせられる → たべさせられて [drop る, then add て]",
            "Past: いる → いた [drop る, then add た]",
        ]
    );
    assert_eq!(patterns(compound), ["〜ている"]);

    // The chain reproduces the word actually in the sentence.
    assert_eq!(
        rebuilt(&compound.components[0]),
        Some("たべさせられて".into())
    );
    assert_eq!(rebuilt(&compound.components[1]), Some("いた".into()));

    // Causative-passive must not be reported as a plain causative or passive.
    assert_eq!(forms(compound)[0], Form::CausativePassive);
}

/// これを機に言わせてもらいますが — the 〜てもらう compound built on the 言わせて fork.
/// The fork must resolve to 言う (Causative) here exactly as it does alone.
#[test]
fn gate_kore_o_ki_ni_iwasete_moraimasu_ga() {
    let analysis = analyze("kore-o-ki-ni", "これを機に言わせてもらいますが");
    let compound = analysis
        .tokens
        .iter()
        .find(|token| token.is_compound())
        .expect("言わせてもらいます is a compound");

    assert_eq!(
        ladder(compound),
        [
            "Causative: いう → いわせる [う → わ, then add せる]",
            // 言わせて is the te-form of 言わせる (ichidan), not of 言う. Building it
            // from 言う would give 言って — a real word, and not this one.
            "Te-form (〜て): いわせる → いわせて [drop る, then add て]",
            "Non-past (formal): もらう → もらいます [う → い, then add ます]",
        ]
    );
    assert_eq!(patterns(compound), ["〜てもらう"]);
    assert_eq!(rebuilt(&compound.components[0]), Some("いわせて".into()));

    // The phantom: Ichiran also parses 言わせて as a Potential of 言わす. It must not
    // appear anywhere in the grammar of this sentence.
    for token in &analysis.tokens {
        assert!(
            !forms(token).contains(&Form::Potential),
            "phantom Potential in {}",
            token.surface
        );
    }
}

/// 先生ってほんと…情けないですね — an い-adjective with です, and an ellipsis that
/// Ichiran refuses to analyse.
#[test]
fn gate_sensei_tte_honto_nasakenai_desu_ne() {
    let analysis = analyze("sensei-nasakenai", "先生ってほんと…情けないですね");

    let nasakenai = &analysis.tokens[4];
    assert_eq!(nasakenai.dict_form, "情けない");
    assert_eq!(
        ladder(nasakenai),
        ["Non-past (formal): なさけない → なさけないです [add です]"]
    );
    assert!(
        patterns(nasakenai).is_empty(),
        "です is not an auxiliary verb"
    );

    // Punctuation has no grammar, and 先生 is a plain noun.
    assert!(breakdown(&analysis.tokens[3]).is_empty());
    assert!(breakdown(&analysis.tokens[0]).is_empty());
}

/// Negation is part of the form, not decoration: a ladder that drops it tells the
/// learner the opposite of what the sentence says.
#[test]
fn gate_negation_reaches_the_ladder() {
    let analysis = analyze("tabenakatta", "食べなかった");
    assert_eq!(
        ladder(&analysis.tokens[0]),
        ["Past (negative): たべる → たべなかった [drop る, then add なかった]"]
    );
}

/// Every pattern the matcher reports, as `id @ token range` plus its written form.
fn found(analysis: &Analysis) -> Vec<String> {
    grammar::patterns(analysis)
        .iter()
        .map(|found| {
            let pattern = pack::core()
                .get(&found.pattern_id)
                .expect("a reported pattern is in the pack");
            format!(
                "{} [{}] @{}..{}",
                found.pattern_id, pattern.pattern, found.first_token, found.last_token_end
            )
        })
        .collect()
}

/// The matcher on real output. Ichiran merges には and では into single tokens, so
/// these are the surfaces the rules must actually be written against.
#[test]
fn gate_patterns_are_found_in_a_real_sentence() {
    let analysis = analyze(
        "kare-niwa",
        "彼には時間がなかったので、家では何もしなかった",
    );

    assert_eq!(
        found(&analysis),
        [
            "particle-niwa [には] @1..2",
            "particle-ga [が] @3..4",
            "node [ので] @5..6",
            "particle-dewa [では] @8..9",
        ]
    );

    // Spans point back at the real bytes, so the reader can highlight them.
    let niwa = &grammar::patterns(&analysis)[0];
    let (start, end) = (niwa.start.unwrap(), niwa.end.unwrap());
    assert_eq!(&analysis.text[start..end], "には");
}

/// 〜なければならない spans two tokens and is matched on the first one's
/// *conjugation* — negative provisional — rather than on how it happens to be spelt.
/// That is what makes the rule work for any verb.
#[test]
fn gate_a_multi_token_pattern_matches_on_conjugation() {
    let analysis = analyze("ikanakereba", "行かなければならない");
    assert_eq!(
        found(&analysis),
        ["nakereba-naranai [〜なければならない] @0..2"]
    );

    // And the first token's own grammar is still explained.
    assert_eq!(
        ladder(&analysis.tokens[0]),
        ["Provisional (〜ば) (negative): いく → いかなければ [く → か, then add なければ]"]
    );
}

/// ことができる arrives as one token, not こと + が + できる. A rule written for the
/// three-token reading would never fire — which is why the rules are written against
/// observed output.
#[test]
fn gate_a_merged_structure_is_matched_as_one_token() {
    let analysis = analyze("miru-koto", "見ることができる");
    assert_eq!(found(&analysis), ["koto-ga-dekiru [ことができる] @1..2"]);
}
