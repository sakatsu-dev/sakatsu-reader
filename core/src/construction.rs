//! Construction annotations shared by analyzers, grammar and lookup.
//!
//! New analysis writes these once where composition establishes the evidence. Serialized tokens
//! from older builds have no annotation, so [`resolved`] performs the same recognition in one
//! compatibility path. Consumers never maintain their own spelling test.

use crate::model::{
    Construction, ConstructionGrammar, ConstructionIdentity, ConstructionLookup, SourceSpan, Token,
};

pub const EXPLANATORY_NAN_PATTERN: &str = "explanatory-na-n";
pub const CONTRACTED_MOTION_PATTERN: &str = "te-iku";
pub const COPULA_RECALL_PATTERN: &str = "hanabira-n3-kke";

/// Attach the known annotation if its spans and component evidence are internally consistent.
pub fn annotate(token: &mut Token, identity: ConstructionIdentity) -> bool {
    let Some(annotation) = inferred_as(token, identity) else {
        return false;
    };
    token
        .constructions
        .retain(|found| found.identity != identity);
    token.constructions.push(annotation);
    true
}

/// Valid annotations, falling back to the compatibility recognizer for an older token.
pub fn resolved(token: &Token) -> Vec<Construction> {
    let valid: Vec<_> = token
        .constructions
        .iter()
        .filter(|annotation| validate(token, annotation))
        .cloned()
        .collect();
    if !valid.is_empty() {
        return valid;
    }

    [
        ConstructionIdentity::ExplanatoryNan,
        ConstructionIdentity::ContractedMotion,
        ConstructionIdentity::CopulaRecall,
    ]
    .into_iter()
    .filter_map(|identity| inferred_as(token, identity))
    .collect()
}

pub fn has(token: &Token, identity: ConstructionIdentity) -> bool {
    resolved(token)
        .iter()
        .any(|annotation| annotation.identity == identity)
}

/// Validate the annotation against the complete source text as well as its token.
pub fn validate_in(source: &str, token: &Token, annotation: &Construction) -> bool {
    validate(token, annotation)
        && source.get(annotation.source.start as usize..annotation.source.end as usize)
            == Some(token.surface.as_str())
}

pub fn canonical_pattern(id: &str) -> Option<&'static str> {
    match id {
        EXPLANATORY_NAN_PATTERN => Some(EXPLANATORY_NAN_PATTERN),
        CONTRACTED_MOTION_PATTERN => Some(CONTRACTED_MOTION_PATTERN),
        COPULA_RECALL_PATTERN => Some(COPULA_RECALL_PATTERN),
        _ => None,
    }
}

fn inferred_as(token: &Token, identity: ConstructionIdentity) -> Option<Construction> {
    let shape = match identity {
        ConstructionIdentity::ExplanatoryNan if explanatory_nan(token) => (
            ConstructionLookup::GrammarPattern,
            ConstructionGrammar::Sentence,
            EXPLANATORY_NAN_PATTERN,
        ),
        ConstructionIdentity::ContractedMotion if contracted_motion(token) => (
            ConstructionLookup::Headword,
            ConstructionGrammar::Auxiliary,
            CONTRACTED_MOTION_PATTERN,
        ),
        ConstructionIdentity::CopulaRecall if copula_recall(token) => (
            ConstructionLookup::Headword,
            ConstructionGrammar::Sentence,
            COPULA_RECALL_PATTERN,
        ),
        _ => return None,
    };
    let start = u32::try_from(token.start?).ok()?;
    let end = u32::try_from(token.end?).ok()?;
    let annotation = Construction {
        identity,
        source: SourceSpan { start, end },
        components: (0..token.components.len())
            .map(u32::try_from)
            .collect::<Result<_, _>>()
            .ok()?,
        lookup: shape.0,
        grammar: shape.1,
        grammar_pattern: shape.2.into(),
    };
    validate(token, &annotation).then_some(annotation)
}

fn explanatory_nan(token: &Token) -> bool {
    let [na, n] = token.components.as_slice() else {
        return false;
    };
    token.surface == "なん"
        && token.dict_form == "だ"
        && na.surface == "な"
        && na.dict_form == "だ"
        && n.surface == "ん"
        && n.lemma_reading == "の"
}

fn contracted_motion(token: &Token) -> bool {
    let [lead, auxiliary] = token.components.as_slice() else {
        return false;
    };
    (lead.surface.ends_with('て') || lead.surface.ends_with('で'))
        && matches!(auxiliary.dict_form.as_str(), "行く" | "いく")
        && auxiliary.lemma_reading == "いく"
        && auxiliary.conjugation.is_none()
        && !matches!(auxiliary.surface.as_str(), "行く" | "いく" | "ゆく")
}

fn copula_recall(token: &Token) -> bool {
    matches!(token.dict_form.as_str(), "だ" | "です")
        && token
            .components
            .last()
            .is_some_and(|component| component.surface == "っけ")
}

fn validate(token: &Token, annotation: &Construction) -> bool {
    let (Some(start), Some(end)) = (token.start, token.end) else {
        return false;
    };
    if annotation.source.start as usize != start
        || annotation.source.end as usize != end
        || annotation.source.start >= annotation.source.end
        || annotation.components.is_empty()
    {
        return false;
    }

    let mut surface = String::new();
    let mut previous_end = None;
    for (position, index) in annotation.components.iter().enumerate() {
        if usize::try_from(*index).ok() != Some(position) {
            return false;
        }
        let Some(component) = token.components.get(position) else {
            return false;
        };
        if position == 0 && component.start != token.start
            || previous_end.is_some() && component.start != previous_end
        {
            return false;
        }
        previous_end = component.end;
        surface.push_str(&component.surface);
    }
    previous_end == token.end && surface == token.surface
}
