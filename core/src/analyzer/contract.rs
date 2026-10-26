//! Structural guarantees every finished analysis must satisfy.
//!
//! Adapters may disagree about Japanese. They may not disagree with their own source text: every
//! displayed token names an ordered source slice, and every non-whitespace source byte belongs to
//! one. This belongs beside [`super::Analyzer`] because production relies on it too.

use crate::model::{Analysis, Token};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanFault {
    Ungrounded {
        index: usize,
        surface: String,
    },
    OutOfBounds {
        index: usize,
        start: usize,
        end: usize,
    },
    NotACharBoundary {
        index: usize,
        start: usize,
        end: usize,
    },
    SurfaceDisagrees {
        index: usize,
        claimed: String,
        source: String,
    },
    OutOfOrder {
        index: usize,
        previous_end: usize,
        start: usize,
    },
    Uncovered {
        start: usize,
        end: usize,
        text: String,
    },
}

impl std::fmt::Display for SpanFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ungrounded { index, surface } => {
                write!(f, "token {index} ({surface:?}) has no source span")
            }
            Self::OutOfBounds { index, start, end } => {
                write!(f, "token {index} has invalid source span {start}..{end}")
            }
            Self::NotACharBoundary { index, start, end } => write!(
                f,
                "token {index} has source span {start}..{end} outside UTF-8 boundaries"
            ),
            Self::SurfaceDisagrees {
                index,
                claimed,
                source,
            } => write!(
                f,
                "token {index} claims {claimed:?}, but its source span spells {source:?}"
            ),
            Self::OutOfOrder {
                index,
                previous_end,
                start,
            } => write!(
                f,
                "token {index} starts at {start} before the previous token ends at {previous_end}"
            ),
            Self::Uncovered { start, end, text } => {
                write!(f, "source {start}..{end} ({text:?}) is covered by no token")
            }
        }
    }
}

pub fn validate(analysis: &Analysis) -> Vec<SpanFault> {
    let text = analysis.text.as_str();
    let mut faults = Vec::new();
    let mut cursor = 0usize;
    for (index, token) in analysis.tokens.iter().enumerate() {
        let (Some(start), Some(end)) = (token.start, token.end) else {
            faults.push(SpanFault::Ungrounded {
                index,
                surface: token.surface.clone(),
            });
            continue;
        };
        if start >= end || end > text.len() {
            faults.push(SpanFault::OutOfBounds { index, start, end });
            continue;
        }
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            faults.push(SpanFault::NotACharBoundary { index, start, end });
            continue;
        }
        if text[start..end] != token.surface {
            faults.push(SpanFault::SurfaceDisagrees {
                index,
                claimed: token.surface.clone(),
                source: text[start..end].to_string(),
            });
        }
        if start < cursor {
            faults.push(SpanFault::OutOfOrder {
                index,
                previous_end: cursor,
                start,
            });
        } else if start > cursor && !text[cursor..start].trim().is_empty() {
            faults.push(SpanFault::Uncovered {
                start: cursor,
                end: start,
                text: text[cursor..start].to_string(),
            });
        }
        cursor = cursor.max(end);
    }
    if cursor < text.len() && !text[cursor..].trim().is_empty() {
        faults.push(SpanFault::Uncovered {
            start: cursor,
            end: text.len(),
            text: text[cursor..].to_string(),
        });
    }
    faults
}

/// Where a component broke the contract, and how.
///
/// `path` names the component from the top: `[3, 1]` is the second part of the fourth token, and
/// `[3, 1, 0]` the first part of that. A path rather than an index because parts nest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentFault {
    pub path: Vec<usize>,
    pub kind: ComponentFaultKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentFaultKind {
    /// No source span at all.
    Ungrounded {
        surface: String,
    },
    /// A span outside the text, empty, or reversed.
    OutOfBounds {
        start: usize,
        end: usize,
    },
    NotACharBoundary {
        start: usize,
        end: usize,
    },
    /// The span spells something other than the component claims.
    SurfaceDisagrees {
        claimed: String,
        source: String,
    },
    /// The span reaches outside the word it is a part of.
    OutsideParent {
        start: usize,
        end: usize,
        parent_start: usize,
        parent_end: usize,
    },
    /// The span starts before the previous sibling ends: out of order, or overlapping it.
    Overlapping {
        previous_end: usize,
        start: usize,
    },
    /// A part that is a word, with no reading to set over it.
    NoReading {
        surface: String,
    },
    /// An offered part inside an offered word that has no usable span of its own, so there is
    /// nothing to check it lies within. Only [`validate_selectable`] reports it.
    ParentUngrounded,
}

impl std::fmt::Display for ComponentFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let at = self
            .path
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(".");
        match &self.kind {
            ComponentFaultKind::Ungrounded { surface } => {
                write!(f, "part {at} ({surface:?}) has no source span")
            }
            ComponentFaultKind::OutOfBounds { start, end } => {
                write!(f, "part {at} has invalid source span {start}..{end}")
            }
            ComponentFaultKind::NotACharBoundary { start, end } => write!(
                f,
                "part {at} has source span {start}..{end} outside UTF-8 boundaries"
            ),
            ComponentFaultKind::SurfaceDisagrees { claimed, source } => write!(
                f,
                "part {at} claims {claimed:?}, but its source span spells {source:?}"
            ),
            ComponentFaultKind::OutsideParent {
                start,
                end,
                parent_start,
                parent_end,
            } => write!(
                f,
                "part {at} spans {start}..{end}, outside its word's {parent_start}..{parent_end}"
            ),
            ComponentFaultKind::Overlapping {
                previous_end,
                start,
            } => write!(
                f,
                "part {at} starts at {start} before the previous part ends at {previous_end}"
            ),
            ComponentFaultKind::NoReading { surface } => {
                write!(f, "part {at} ({surface:?}) is a word with no reading")
            }
            ComponentFaultKind::ParentUngrounded => {
                write!(f, "part {at} is offered inside a word with no usable span")
            }
        }
    }
}

/// The contract [`validate`] states for tokens, stated for **their parts**.
///
/// **[`validate`] checks top-level tokens only**, so a part could claim any span, reach outside its
/// word, overlap its sibling or spell something else entirely, and the analysis would still pass.
/// The reader lets a person select a part and look it up, mark it Known and send it to Anki, all
/// by its span and surface — so a part that lies about its source misleads exactly as a token
/// would, one click further in.
///
/// Recursive, because parts nest. Each part must lie within the part or word it belongs to, come
/// after its previous sibling without overlapping it, spell its own source slice, and carry a
/// reading if it is a word.
///
/// **Not a tiling check.** Parts need not cover their whole word between them — a compound's
/// components and the pieces of a contracted form are not obliged to — so a gap is not a fault.
///
/// A parent with no usable span of its own is reported by whichever check found it — [`validate`]
/// for a token — and its parts are then checked against the text alone.
pub fn validate_components(analysis: &Analysis) -> Vec<ComponentFault> {
    let mut faults = Vec::new();
    for (index, token) in analysis.tokens.iter().enumerate() {
        let parent = usable_span(&analysis.text, token);
        walk_parts(
            &analysis.text,
            &token.components,
            parent,
            &mut vec![index],
            &mut faults,
        );
    }
    faults
}

/// A token's span, if it is a usable one. Otherwise `None`, and the reason is someone else's to
/// report.
fn usable_span(text: &str, token: &Token) -> Option<(usize, usize)> {
    let (start, end) = (token.start?, token.end?);
    (start < end && end <= text.len() && text.is_char_boundary(start) && text.is_char_boundary(end))
        .then_some((start, end))
}

fn walk_parts(
    text: &str,
    parts: &[Token],
    parent: Option<(usize, usize)>,
    path: &mut Vec<usize>,
    faults: &mut Vec<ComponentFault>,
) {
    let mut previous_end: Option<usize> = None;
    for (at, part) in parts.iter().enumerate() {
        path.push(at);
        let mut found = Vec::new();
        let usable = match (part.start, part.end) {
            (Some(start), Some(end)) if start >= end || end > text.len() => {
                found.push(ComponentFaultKind::OutOfBounds { start, end });
                None
            }
            (Some(start), Some(end))
                if !text.is_char_boundary(start) || !text.is_char_boundary(end) =>
            {
                found.push(ComponentFaultKind::NotACharBoundary { start, end });
                None
            }
            (Some(start), Some(end)) => Some((start, end)),
            _ => {
                found.push(ComponentFaultKind::Ungrounded {
                    surface: part.surface.clone(),
                });
                None
            }
        };
        if let Some((start, end)) = usable {
            if text[start..end] != part.surface {
                found.push(ComponentFaultKind::SurfaceDisagrees {
                    claimed: part.surface.clone(),
                    source: text[start..end].to_string(),
                });
            }
            if let Some((parent_start, parent_end)) = parent {
                if start < parent_start || end > parent_end {
                    found.push(ComponentFaultKind::OutsideParent {
                        start,
                        end,
                        parent_start,
                        parent_end,
                    });
                }
            }
            if let Some(previous) = previous_end {
                if start < previous {
                    found.push(ComponentFaultKind::Overlapping {
                        previous_end: previous,
                        start,
                    });
                }
            }
            previous_end = Some(previous_end.map_or(end, |previous| previous.max(end)));
        }
        if part.is_word() && part.reading.trim().is_empty() {
            found.push(ComponentFaultKind::NoReading {
                surface: part.surface.clone(),
            });
        }
        faults.extend(found.into_iter().map(|kind| ComponentFault {
            path: path.clone(),
            kind,
        }));
        walk_parts(text, &part.components, usable, path, faults);
        path.pop();
    }
}

/// The contract **for what a reader is offered**: the selectable projection of every word.
///
/// [`validate_components`] describes the retained decomposition — grammar included — and is
/// expected to report expanded parts the source does not spell. This checks something else: only
/// parts marked [`Token::selectable`], each against its **selectable** parent and its previous
/// **selectable** sibling, which is exactly the view the parts row and contextual lookup use. A
/// withheld sibling is not part of that view, so a selectable part overlapping it is not a fault
/// here, while it is one in the decomposition.
///
/// Independent of [`mark_selectable`] by construction: it re-derives every property from the
/// source and the flags as they stand, so an analysis marked by anything else — or marked wrongly
/// — is still checked. Faults are reported with the same paths as the decomposition's.
pub fn validate_selectable(analysis: &Analysis) -> Vec<ComponentFault> {
    let mut faults = Vec::new();
    for (index, token) in analysis.tokens.iter().enumerate() {
        if !token.selectable {
            continue;
        }
        let parent = usable_span(&analysis.text, token);
        walk_selectable(
            &analysis.text,
            &token.components,
            parent,
            &mut vec![index],
            &mut faults,
        );
    }
    faults
}

fn walk_selectable(
    text: &str,
    parts: &[Token],
    parent: Option<(usize, usize)>,
    path: &mut Vec<usize>,
    faults: &mut Vec<ComponentFault>,
) {
    let mut previous_end: Option<usize> = None;
    for (at, part) in parts.iter().enumerate() {
        if !part.selectable {
            continue;
        }
        path.push(at);
        let mut found = Vec::new();
        let usable = match (part.start, part.end) {
            (Some(start), Some(end)) if start >= end || end > text.len() => {
                found.push(ComponentFaultKind::OutOfBounds { start, end });
                None
            }
            (Some(start), Some(end))
                if !text.is_char_boundary(start) || !text.is_char_boundary(end) =>
            {
                found.push(ComponentFaultKind::NotACharBoundary { start, end });
                None
            }
            (Some(start), Some(end)) => Some((start, end)),
            _ => {
                found.push(ComponentFaultKind::Ungrounded {
                    surface: part.surface.clone(),
                });
                None
            }
        };
        if let Some((start, end)) = usable {
            if text[start..end] != part.surface {
                found.push(ComponentFaultKind::SurfaceDisagrees {
                    claimed: part.surface.clone(),
                    source: text[start..end].to_string(),
                });
            }
            match parent {
                Some((parent_start, parent_end)) if start < parent_start || end > parent_end => {
                    found.push(ComponentFaultKind::OutsideParent {
                        start,
                        end,
                        parent_start,
                        parent_end,
                    });
                }
                Some(_) => {}
                None => found.push(ComponentFaultKind::ParentUngrounded),
            }
            if let Some(previous) = previous_end {
                if start < previous {
                    found.push(ComponentFaultKind::Overlapping {
                        previous_end: previous,
                        start,
                    });
                }
            }
            previous_end = Some(previous_end.map_or(end, |previous| previous.max(end)));
        }
        if part.is_word() && part.reading.trim().is_empty() {
            found.push(ComponentFaultKind::NoReading {
                surface: part.surface.clone(),
            });
        }
        faults.extend(found.into_iter().map(|kind| ComponentFault {
            path: path.clone(),
            kind,
        }));
        walk_selectable(text, &part.components, usable, path, faults);
        path.pop();
    }
}

/// Decide, from the source alone, which tokens and parts a reader may select.
///
/// A top-level token is selectable when its span is usable and spells its surface. A part is
/// selectable when, in addition, it lies inside a *selectable* parent, starts no earlier than the
/// end of its previous selectable sibling, and — if it is a word — has a reading available. Its
/// spelling and position are verified against the source; its reading is required to exist, not
/// verified. Everything else — an expanded grammatical form with
/// no span, a part reaching outside its word, a sibling overlapping the one before it, anything
/// beneath a part that is not itself selectable — is left in place and marked not selectable.
///
/// **Nothing is removed, trimmed or re-spanned.** The decomposition stays exactly as the analyzer
/// gave it, because the grammar layer and the lookup fallback depend on it; this only says which
/// of it is a piece of the text. It never rejects an analysis either: a passage is not refused
/// because one of its grammatical parts cannot be pointed at.
///
/// Idempotent, so it can run after every stage that makes new tokens.
pub fn mark_selectable(analysis: &mut Analysis) {
    let text = analysis.text.as_str();
    for token in &mut analysis.tokens {
        let span =
            usable_span(text, token).filter(|&(start, end)| text[start..end] == token.surface);
        token.selectable = span.is_some();
        mark_parts(text, &mut token.components, span);
    }
}

fn mark_parts(text: &str, parts: &mut [Token], parent: Option<(usize, usize)>) {
    let mut previous_end: Option<usize> = None;
    for part in parts {
        let span = parent.and_then(|(parent_start, parent_end)| {
            let (start, end) = usable_span(text, part)?;
            (start >= parent_start
                && end <= parent_end
                && text[start..end] == part.surface
                && previous_end.is_none_or(|previous| start >= previous)
                // A word-part is offered only with a reading available to set over it. This is
                // availability, not verification: the reading is not checked against the source.
                && (!part.is_word() || !part.reading.trim().is_empty()))
            .then_some((start, end))
        });
        part.selectable = span.is_some();
        if let Some((_, end)) = span {
            previous_end = Some(end);
        }
        mark_parts(text, &mut part.components, span);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(surface: &str, start: usize, reading: &str) -> Token {
        Token {
            surface: surface.into(),
            dict_form: surface.into(),
            reading: reading.into(),
            start: Some(start),
            end: Some(start + surface.len()),
            ..Default::default()
        }
    }

    /// かかっとる, with its two source slices as parts: the shape a repaired contraction takes.
    fn contracted(parts: Vec<Token>) -> Analysis {
        let mut whole = part("かかっとる", 0, "かかっとる");
        whole.components = parts;
        Analysis {
            execution: None,
            text: "かかっとる".into(),
            tokens: vec![whole],
        }
    }

    #[test]
    fn parts_that_spell_their_own_slices_in_order_pass() {
        let analysis = contracted(vec![part("かかっ", 0, "かかっ"), part("とる", 9, "とる")]);
        assert!(validate_components(&analysis).is_empty());
        assert!(validate(&analysis).is_empty());
    }

    /// **The case [`validate`] cannot see.** An expanded form written into a part — ておる in a
    /// source that says とる — passes the top-level check, because the token itself is honest.
    #[test]
    fn a_part_spelling_an_expanded_form_is_caught_where_validate_is_not() {
        let mut expanded = part("とる", 9, "ておる");
        expanded.surface = "ておる".into();
        let analysis = contracted(vec![part("かかっ", 0, "かかっ"), expanded]);
        assert!(validate(&analysis).is_empty(), "the top level is honest");
        let faults = validate_components(&analysis);
        assert!(
            faults.iter().any(|fault| fault.path == [0, 1]
                && matches!(fault.kind, ComponentFaultKind::SurfaceDisagrees { .. })),
            "{faults:?}"
        );
    }

    #[test]
    fn a_part_outside_its_word_is_caught() {
        let mut analysis = contracted(vec![part("かかっ", 0, "かかっ")]);
        analysis.text = "かかっとる。".into();
        analysis.tokens.push(part("。", 15, ""));
        analysis.tokens[0].components.push(part("る。", 12, "る"));
        let faults = validate_components(&analysis);
        assert!(
            faults
                .iter()
                .any(|fault| matches!(fault.kind, ComponentFaultKind::OutsideParent { .. })),
            "{faults:?}"
        );
    }

    #[test]
    fn overlapping_or_reversed_parts_are_caught() {
        let overlapping = contracted(vec![
            part("かかっ", 0, "かかっ"),
            part("っとる", 6, "っとる"),
        ]);
        let reversed = contracted(vec![part("とる", 9, "とる"), part("かかっ", 0, "かかっ")]);
        for analysis in [overlapping, reversed] {
            let faults = validate_components(&analysis);
            assert!(
                faults
                    .iter()
                    .any(|fault| matches!(fault.kind, ComponentFaultKind::Overlapping { .. })),
                "{faults:?}"
            );
        }
    }

    #[test]
    fn a_word_part_without_a_reading_is_caught() {
        let analysis = contracted(vec![part("かかっ", 0, ""), part("とる", 9, "とる")]);
        let faults = validate_components(&analysis);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(matches!(
            faults[0].kind,
            ComponentFaultKind::NoReading { .. }
        ));
    }

    /// Nesting: a fault two levels down is found, and named by its whole path.
    #[test]
    fn a_fault_in_a_nested_part_is_named_by_its_path() {
        let mut stem = part("かかっ", 0, "かかっ");
        stem.components = vec![part("かか", 0, "かか"), part("っと", 6, "っと")];
        let analysis = contracted(vec![stem, part("とる", 9, "とる")]);
        let faults = validate_components(&analysis);
        assert!(
            faults.iter().any(|fault| fault.path == [0, 0, 1]
                && matches!(fault.kind, ComponentFaultKind::OutsideParent { .. })),
            "{faults:?}"
        );
    }

    fn marked(mut analysis: Analysis) -> Analysis {
        mark_selectable(&mut analysis);
        analysis
    }

    /// **The case this exists for.** 食べてる's grammatical parts are 食べて and いる, and いる is
    /// not in the source. It stays a part — the grammar needs it — and is not offered.
    #[test]
    fn an_expanded_grammatical_part_is_kept_and_not_selectable() {
        let mut expanded = part("いる", 0, "いる");
        expanded.start = None;
        expanded.end = None;
        let mut whole = part("食べてる", 0, "たべてる");
        whole.components = vec![part("食べて", 0, "たべて"), expanded];
        let analysis = marked(Analysis {
            execution: None,
            text: "食べてる".into(),
            tokens: vec![whole],
        });
        let token = &analysis.tokens[0];
        assert!(token.selectable);
        assert_eq!(token.components.len(), 2, "the decomposition is kept whole");
        assert!(
            token.components[0].selectable,
            "食べて is a slice of the source"
        );
        assert!(!token.components[1].selectable, "いる is grammar, not text");
        assert_eq!(
            token.components[1].surface, "いる",
            "and it is not trimmed to fit"
        );
        assert_eq!(
            (token.components[1].start, token.components[1].end),
            (None, None)
        );
    }

    #[test]
    fn a_part_outside_its_word_or_overlapping_its_sibling_is_not_selectable() {
        let mut analysis = contracted(vec![
            part("かかっ", 0, "かかっ"),
            part("っとる", 6, "っとる"),
        ]);
        analysis.text = "かかっとる。".into();
        analysis.tokens.push(part("。", 15, ""));
        analysis.tokens[0].components.push(part("る。", 12, "る"));
        let analysis = marked(analysis);
        let parts = &analysis.tokens[0].components;
        assert!(parts[0].selectable);
        assert!(!parts[1].selectable, "overlaps the part before it");
        assert!(!parts[2].selectable, "reaches outside its word");
    }

    /// Beneath a part that cannot be pointed at, nothing can be either — even a sub-part whose
    /// text happens to occur, because there is no word for it to lie inside.
    #[test]
    fn nothing_beneath_an_unselectable_part_is_selectable() {
        let mut stem = part("かかって", 0, "かかって");
        stem.components = vec![part("かか", 0, "かか")];
        let analysis = marked(contracted(vec![stem, part("とる", 9, "とる")]));
        let parts = &analysis.tokens[0].components;
        assert!(
            !parts[0].selectable,
            "かかって is not what the source spells"
        );
        assert!(!parts[0].components[0].selectable);
        assert!(parts[1].selectable);
    }

    /// A word-part with no reading spells its slice and still is not offered: nothing could be set
    /// over it or said about it.
    #[test]
    fn a_word_part_without_a_reading_is_not_selectable() {
        let analysis = marked(contracted(vec![
            part("かかっ", 0, ""),
            part("とる", 9, "とる"),
        ]));
        let parts = &analysis.tokens[0].components;
        assert!(!parts[0].selectable);
        assert!(parts[1].selectable);
    }

    /// Every part `mark_selectable` offers satisfies the **projection's** contract, and a second
    /// pass changes nothing.
    #[test]
    fn what_is_marked_selectable_satisfies_the_projection_and_marking_is_idempotent() {
        let mut expanded = part("いる", 0, "いる");
        expanded.start = None;
        let mut analysis = contracted(vec![
            part("かかっ", 0, "かかっ"),
            part("っと", 6, "っと"),
            expanded,
        ]);
        analysis.tokens[0].components[1].components = vec![part("と", 9, "と")];
        let once = marked(analysis);
        let twice = marked(once.clone());
        assert_eq!(once, twice);
        assert!(
            validate_selectable(&once).is_empty(),
            "{:?}",
            validate_selectable(&once)
        );
    }

    /// **The control the two sibling rules disagree on.** A grounded sibling withheld for want of
    /// a reading overlaps the part after it. The decomposition reports the overlap; the projection
    /// — which is what a reader is offered — does not, because the withheld sibling is not in it.
    #[test]
    fn a_withheld_sibling_overlapping_an_offered_one_is_a_decomposition_fault_only() {
        let analysis = marked(contracted(vec![
            part("かかっ", 0, ""),
            part("かっと", 3, "かっと"),
            part("る", 12, "る"),
        ]));
        let parts = &analysis.tokens[0].components;
        assert!(!parts[0].selectable, "no reading, so withheld");
        assert!(parts[1].selectable, "overlaps only the withheld sibling");
        assert!(
            validate_components(&analysis)
                .iter()
                .any(|fault| fault.path == [0, 1]
                    && matches!(fault.kind, ComponentFaultKind::Overlapping { .. })),
            "the decomposition still reports it"
        );
        assert!(
            validate_selectable(&analysis).is_empty(),
            "the projection does not"
        );
    }

    /// The projection validator re-derives everything: flags set wrongly by anything other than
    /// `mark_selectable` are caught.
    #[test]
    fn the_projection_validator_catches_a_part_marked_selectable_wrongly() {
        let mut expanded = part("いる", 0, "いる");
        expanded.start = None;
        expanded.end = None;
        let mut overlapping = part("っとる", 6, "っとる");
        overlapping.selectable = true;
        expanded.selectable = true;
        let mut first = part("かかっ", 0, "かかっ");
        first.selectable = true;
        let mut analysis = contracted(vec![first, overlapping, expanded]);
        analysis.tokens[0].selectable = true;
        let faults = validate_selectable(&analysis);
        assert!(faults
            .iter()
            .any(|fault| matches!(fault.kind, ComponentFaultKind::Overlapping { .. })));
        assert!(faults
            .iter()
            .any(|fault| matches!(fault.kind, ComponentFaultKind::Ungrounded { .. })));
    }

    #[test]
    fn a_gap_between_parts_is_not_a_fault() {
        let analysis = contracted(vec![part("か", 0, "か"), part("る", 12, "る")]);
        assert!(validate_components(&analysis).is_empty());
    }
}
