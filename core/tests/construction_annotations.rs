//! Learner-facing construction evidence is produced once and survives every consumer.

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::construction;
use sakatsu_core::dict::KanjiDic;
use sakatsu_core::model::{Analysis, ConstructionIdentity, Token};
use sakatsu_core::store::WordKey;
use sakatsu_core::study::{self, Resources};

const MOTION: &str = "へえ今度アタシも連れてってよ";
const NAN_AND_KKE: &str = "ここって禁足地なんじゃなかったっけ";
const SUFFIX_ADJECTIVE: &str = "それは子供っぽくない";

fn analyzer() -> LinderaAnalyzer {
    LinderaAnalyzer::new().expect("the embedded UniDic is part of the build")
}

fn annotated(analysis: &Analysis, identity: ConstructionIdentity) -> (&Token, usize) {
    analysis
        .tokens
        .iter()
        .enumerate()
        .find_map(|(index, token)| construction::has(token, identity).then_some((token, index)))
        .unwrap_or_else(|| panic!("{identity:?} was not annotated in {:?}", analysis.text))
}

fn assert_exact_evidence(source: &str, token: &Token, identity: ConstructionIdentity) {
    let annotation = construction::resolved(token)
        .into_iter()
        .find(|annotation| annotation.identity == identity)
        .expect("the requested annotation resolves");
    assert!(
        construction::validate_in(source, token, &annotation),
        "the annotation must name an exact source substring"
    );
    assert_eq!(
        source.get(annotation.source.start as usize..annotation.source.end as usize),
        Some(token.surface.as_str())
    );

    let mut previous_end = token.start;
    let mut reconstructed = String::new();
    for (position, component_index) in annotation.components.iter().copied().enumerate() {
        assert_eq!(component_index as usize, position);
        let component = &token.components[component_index as usize];
        assert_eq!(
            component.start, previous_end,
            "component spans are contiguous"
        );
        previous_end = component.end;
        reconstructed.push_str(&component.surface);
    }
    assert_eq!(previous_end, token.end);
    assert_eq!(reconstructed, token.surface);
}

#[test]
fn the_three_existing_constructions_carry_valid_shared_evidence() {
    let analyzer = analyzer();

    let motion = analyzer.analyze(MOTION).expect("motion passage analyzes");
    let (motion_token, _) = annotated(&motion, ConstructionIdentity::ContractedMotion);
    assert_exact_evidence(MOTION, motion_token, ConstructionIdentity::ContractedMotion);
    let motion_annotation = construction::resolved(motion_token)
        .into_iter()
        .find(|annotation| annotation.identity == ConstructionIdentity::ContractedMotion)
        .expect("motion annotation");
    assert_eq!(
        motion_annotation.grammar_pattern,
        construction::CONTRACTED_MOTION_PATTERN
    );
    assert!(sakatsu_core::grammar::pack::find(&motion_annotation.grammar_pattern).is_some());
    let auxiliary = motion_token.components.last().expect("motion auxiliary");
    assert!(matches!(auxiliary.dict_form.as_str(), "行く" | "いく"));
    assert_eq!(auxiliary.lemma_reading, "いく");

    let nan_kke = analyzer
        .analyze(NAN_AND_KKE)
        .expect("the explanatory passage analyzes");
    let (nan, _) = annotated(&nan_kke, ConstructionIdentity::ExplanatoryNan);
    assert_exact_evidence(NAN_AND_KKE, nan, ConstructionIdentity::ExplanatoryNan);
    let (kke, _) = annotated(&nan_kke, ConstructionIdentity::CopulaRecall);
    assert_exact_evidence(NAN_AND_KKE, kke, ConstructionIdentity::CopulaRecall);
    assert!(sakatsu_core::grammar::pack::find(construction::COPULA_RECALL_PATTERN).is_some());
    assert!(
        sakatsu_core::grammar::pack::core()
            .get(construction::COPULA_RECALL_PATTERN)
            .is_none(),
        "the recall entry comes from an imported pack, so a core-only lookup would regress"
    );

    let pattern_ids = sakatsu_core::grammar::patterns(&nan_kke)
        .into_iter()
        .map(|matched| matched.pattern_id)
        .collect::<Vec<_>>();
    for id in [
        construction::EXPLANATORY_NAN_PATTERN,
        construction::COPULA_RECALL_PATTERN,
    ] {
        assert_eq!(
            pattern_ids
                .iter()
                .filter(|found| found.as_str() == id)
                .count(),
            1,
            "{id} is consumed from its annotation exactly once"
        );
    }

    let suffix = analyzer
        .analyze(SUFFIX_ADJECTIVE)
        .expect("suffix adjective analyzes");
    assert!(
        suffix
            .tokens
            .iter()
            .any(|token| token.surface == "っぽくない"),
        "the existing suffix adjective remains separate from its noun"
    );
    assert!(
        suffix
            .tokens
            .iter()
            .all(|token| token.constructions.is_empty()),
        "the existing suffix-adjective behavior is not one of these constructions"
    );
}

#[test]
fn controls_do_not_acquire_explanatory_nan_metadata() {
    let analyzer = analyzer();
    for text in ["映画館なので静かです。", "なんか変だ。", "何を食べる。"] {
        let analysis = analyzer.analyze(text).expect("control analyzes");
        assert!(
            analysis
                .tokens
                .iter()
                .all(|token| !construction::has(token, ConstructionIdentity::ExplanatoryNan)),
            "{text:?} is outside the explanatory なん construction"
        );
    }
}

#[test]
fn legacy_tokens_share_the_same_normalization_lookup_and_study_identity() {
    let analyzer = analyzer();
    let analysis = analyzer.analyze(NAN_AND_KKE).expect("passage analyzes");
    let (annotated_nan, nan_index) = annotated(&analysis, ConstructionIdentity::ExplanatoryNan);

    let mut wire = serde_json::to_value(annotated_nan).expect("token serializes");
    wire.as_object_mut()
        .expect("token is an object")
        .remove("constructions");
    let legacy: Token =
        serde_json::from_value(wire).expect("older token without annotations loads");
    assert!(legacy.constructions.is_empty());
    assert_eq!(
        construction::resolved(annotated_nan),
        construction::resolved(&legacy),
        "one compatibility normalizer supplies the same evidence"
    );
    assert_eq!(
        WordKey::for_token(annotated_nan),
        WordKey::for_token(&legacy)
    );

    let mut legacy_tokens = analysis.tokens.clone();
    for token in &mut legacy_tokens {
        token.constructions.clear();
    }
    assert_eq!(
        serde_json::to_value(
            sakatsu_core::lookup::in_context(&analysis.tokens, nan_index, &[])
                .expect("annotated lookup")
        )
        .expect("lookup serializes"),
        serde_json::to_value(
            sakatsu_core::lookup::in_context(&legacy_tokens, nan_index, &[])
                .expect("legacy lookup")
        )
        .expect("lookup serializes"),
        "lookup consumes the same normalized construction"
    );

    let view = study::study(
        NAN_AND_KKE,
        Resources {
            analyzer: &analyzer,
            dictionaries: &[],
            kanji: &KanjiDic::default(),
            store: None,
        },
    )
    .expect("study view");
    let count = view.analysis.tokens.len();
    assert_eq!(view.breakdowns.len(), count);
    assert_eq!(view.statuses.len(), count);
    assert_eq!(view.ruby.len(), count);
    assert!(!view.sentences.is_empty());
    assert!(view
        .patterns
        .iter()
        .any(|matched| { matched.pattern_id == construction::EXPLANATORY_NAN_PATTERN }));
    assert!(view
        .patterns
        .iter()
        .any(|matched| { matched.pattern_id == construction::COPULA_RECALL_PATTERN }));
}
