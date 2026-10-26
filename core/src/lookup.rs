//! Looking one word up across every open dictionary.
//!
//! # What this is, and why it is here rather than in the shell
//!
//! Three commands in the desktop shell reached the dictionaries: `lookup` for a selected word,
//! `lookup_in_context` for one with its sentence around it, and the Reference search for each of
//! its results. All three went through two private helpers in `commands.rs`, so the query policy —
//! which spelling is asked for, what happens when a joined token reaches nothing, which reading is
//! preferred — sat behind a Tauri boundary and could only be exercised by constructing an app.
//!
//! Every type involved was already a `core` type. So the policy moves here, next to
//! [`study`](crate::study) which consumes the same dictionaries, and the shell keeps what is
//! genuinely its own: acquiring state, the answer cache, command registration, and turning an
//! error into a string for transport.
//!
//! # The dictionaries are lent, not resolved
//!
//! Every entry point takes `&[Dictionary]`. The shell resolves `Live::now()` once and lends the
//! result, exactly as [`study`](crate::study) requires — so a lookup cannot straddle a dictionary
//! replacement halfway through its own fallback chain. Before this move, `look_up_sources` called
//! `now()` in its own loop header; that was correct for one call but made "the whole lookup saw
//! one set of dictionaries" a property of where the call happened rather than of the signature.
//!
//! # What is deliberately *not* here
//!
//! - **The cache.** `WarmupState` keys contextual answers by passage digest and is Tauri state
//!   with a foreground/speculative distinction the core has no opinion about. It wraps these
//!   functions; it is not one of them.
//! - **The construction answer.** A grouped construction is answered from the grammar pack rather
//!   than from any dictionary, and that decision already lives at the front of
//!   [`merge`](crate::dict::merge) — before the sources are consulted, so it holds identically on
//!   a machine with no dictionaries installed. Nothing here needs to know about it, which is the
//!   point of it being there.

use crate::dict::context::{contextualize_with_lookup, ContextualWordView};
use crate::dict::index::{Dictionary, MatchKind};
use crate::dict::merge::{self, SourceResult, WordView};
use crate::dict::yomitan::Entry;
use crate::dict::DictError;
use crate::model::Token;

/// What can go wrong looking a word up.
///
/// The two cases are genuinely different: a dictionary that will not answer is a resource
/// problem, and an index past the end of the passage is a caller mistake. Both used to be strings
/// built at the command boundary, and both keep **exactly** the text they had — the shell's
/// `map_err(|error| error.to_string())` is now this type's `Display`, so nothing a caller shows
/// or a test asserts on changes.
#[derive(Debug)]
pub enum LookupError {
    /// A dictionary could not be read.
    Dictionary(DictError),
    /// The selected index is not a token in the passage that was handed over.
    OutsidePassage { index: usize },
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Transparent, so `lookup` reports what the dictionary said and not a wrapper around
            // it. The degraded-store banners are the standing reminder of what nesting reads like.
            LookupError::Dictionary(error) => write!(f, "{error}"),
            LookupError::OutsidePassage { index } => {
                write!(f, "token index {index} is outside the passage")
            }
        }
    }
}

impl std::error::Error for LookupError {}

impl From<DictError> for LookupError {
    fn from(error: DictError) -> Self {
        LookupError::Dictionary(error)
    }
}

/// The reading to rank one dictionary's hits by: the reading of **what was actually asked**.
///
/// The question put to a dictionary is the dictionary form (弾く), so the reading that can tell its
/// entries apart is the dictionary form's (ひく), not the surface's (ひいている). Ranking by the
/// surface reading matched no entry for any inflected word, so the dictionary's own order stood —
/// and for 弾いている that order puts 弾く【はじく】, "to flip; to snap", ahead of 弾く【ひく】, "to
/// play (an instrument)". Found while taking README screenshots on 2026-10-19.
///
/// When a component answered instead of the whole, that component's own reading, for the same
/// reason. An uninflected word reads the same either way, so for it nothing changes.
fn reading_for<'a>(token: &'a Token, asked: &str) -> &'a str {
    let form = |part: &Token| {
        if part.dict_form.is_empty() {
            part.surface.clone()
        } else {
            part.dict_form.clone()
        }
    };
    std::iter::once(token)
        .chain(token.components.iter())
        .find(|part| form(part) == asked)
        .map(|part| {
            if !part.dict_form.is_empty() && !part.lemma_reading.is_empty() {
                part.lemma_reading.as_str()
            } else {
                part.reading.as_str()
            }
        })
        .unwrap_or(token.reading.as_str())
}

/// What each dictionary has for this token, in the order the dictionaries were opened.
///
/// **Precedence is the caller's, expressed as the order of the slice.** Nothing is re-ranked
/// here: the shell opens the preferred ordinary dictionary, then the name dictionary, then the
/// rest, and the merge policy in [`merge`](crate::dict::merge) reads that order as meaning
/// something. Sorting inside this function would silently overrule a decision made where the
/// dictionaries were discovered.
///
/// The hits are kept rather than merged immediately, because the contextual caller needs to know
/// *how* a source was reached — an exact spelling or reading, or the katakana fold of last resort
/// — and that distinction does not survive into [`WordView`].
pub fn sources(token: &Token, dictionaries: &[Dictionary]) -> Result<Vec<SourceResult>, DictError> {
    let query = if token.dict_form.is_empty() {
        token.surface.clone()
    } else {
        token.dict_form.clone()
    };

    // **A joined token that reaches nothing falls back to the part that does.**
    //
    // Composition builds learner-sized words — `compose_nominal` puts 河童 and the 接尾辞 様
    // together — and the dictionary does not have every one it builds. 河童様 is not an entry;
    // 河童 is. Split, the reader could click it and get the kappa. Joined, and without this, they
    // got nothing, which is worse than either.
    //
    // Longest part first, so the answer is the most specific one available: for 河童様 that is
    // 河童 rather than the honorific, and for a compound built on a prefix it is the noun rather
    // than the お.
    //
    // Only when the whole reached nothing, so a token the dictionary *does* know is never
    // answered with one of its pieces.
    let fallbacks: Vec<String> = {
        let mut parts: Vec<String> = token
            .components
            .iter()
            .map(|part| {
                if part.dict_form.is_empty() {
                    part.surface.clone()
                } else {
                    part.dict_form.clone()
                }
            })
            .filter(|part| !part.is_empty() && *part != query)
            .collect();
        parts.sort_by_key(|part| std::cmp::Reverse(part.chars().count()));
        parts
    };

    let mut sources = Vec::new();
    for dictionary in dictionaries {
        // **What answered is recorded here, as it happens.** A caller cannot work it out later:
        // 神姫 and 神 are different queries that both return entries spelled 神, so the entry is
        // no evidence of the question. Each dictionary is asked for the whole first and falls back
        // independently, which is why this is per source and not per token.
        let mut hits = dictionary.lookup(&query)?;
        let mut asked = query.as_str();
        let mut retrieved = merge::Retrieved::Whole {
            query: query.clone(),
        };
        if hits.is_empty() {
            for part in &fallbacks {
                hits = dictionary.lookup(part)?;
                if !hits.is_empty() {
                    asked = part.as_str();
                    retrieved = merge::Retrieved::Component {
                        query: part.clone(),
                    };
                    break;
                }
            }
        }
        // The score the dictionary sorts by saturates, so for a common word it decides nothing.
        // The analyzer's reading does — see `dict::merge::prefer_matching_reading`.
        // Before the caller ever sees them, so a promoted answer is a meaning rather than a
        // pointer to one — and so both lookup paths get it, not only the contextual one.
        resolve_cross_references(&mut hits, crate::dict::evidence::Source::direct(dictionary))?;
        merge::prefer_matching_reading(&mut hits, reading_for(token, asked));
        if !hits.is_empty() {
            sources.push(SourceResult {
                title: dictionary.meta().title.clone(),
                hits,
                retrieved,
            });
        }
    }
    Ok(sources)
}

/// One word, looked up and merged.
///
/// Takes the whole token, not just a string: the merge policy needs the analyzer's own senses to
/// decide what is primary, and its reading to decide whether a dictionary's reading should be
/// adopted at all.
pub fn word(token: &Token, dictionaries: &[Dictionary]) -> Result<WordView, DictError> {
    Ok(merge::merge(token, &sources(token, dictionaries)?))
}

/// A contextual answer together with the evidence it was assembled from.
///
/// **Both from one retrieval pass.** The alternative is two, and two is not the same thing: the
/// dictionaries can be reopened between them, and a second pass costs a second query per token per
/// dictionary for a comparison that runs over whole passages.
///
/// The other alternative — reconstructing the evidence from the finished view — cannot work at
/// all. `merge` is where alternatives are *hidden*: it picks what leads, drops name hits out of
/// the senses, and folds the rest into "also in". What it discarded is exactly what a comparison
/// between two engines' candidates needs to see.
#[derive(Debug, Clone)]
pub struct Looked {
    /// Exactly what [`in_context`] returns. Production reads this and nothing else.
    pub view: ContextualWordView,
    /// Every dictionary's answer **before** the merge chose among them, each carrying whether it
    /// answered the whole token or one of its components — see [`merge::Retrieved`].
    pub evidence: Vec<SourceResult>,
}

/// Whether an entry says only *where the meaning is*, rather than what it is.
///
/// Jitendex files a search-only spelling as its own entry under the negated sequence of the word
/// it belongs to, and gives it cross-reference text instead of senses: `⟶ 抜 ぬ き 取 と る`, then
/// `抜き取る redirected from 抜きとる`. Useful for finding the word and useless for reading it.
///
/// **Every gloss must be cross-reference text, not merely the first.** A search-only form that
/// carries one real sense beside a pointer is still an entry worth showing as it stands, and
/// replacing its senses would lose the one thing it had.
pub fn is_cross_reference(entry: &Entry) -> bool {
    let mut saw_one = false;
    for definition in &entry.definitions {
        for gloss in &definition.glosses {
            saw_one = true;
            if !(gloss.contains(" redirected from ") || gloss.trim_start().starts_with('\u{27f6}'))
            {
                return false;
            }
        }
    }
    saw_one
}

/// Give a cross-reference its target's senses, from the dictionary that issued it.
///
/// **The reader selected a word and was shown an arrow.** Seven tokens across one reading notebook
/// displayed nothing but `⟶ 抜 ぬ き 取 と る` and its kind, with no real sense anywhere in the
/// list — because the analyzer offered no senses of its own, so `merge` promoted the only
/// dictionary answer there was, and that answer was a pointer. The other analyzer escapes this by
/// supplying its own senses, not by resolving anything.
///
/// **Takes the dictionary that produced the hits**, rather than looking one up by title. A title
/// is a label a dictionary gives itself, not an identity: two installed copies can share one, and
/// resolving through the wrong copy would answer out of a dictionary that never saw the query.
/// Called from inside [`sources`]'s own loop, where the right handle is already in hand — which is
/// also why every caller gets this, [`word`] and [`with_evidence`] alike, rather than whichever
/// one remembered to ask.
///
/// Only the definitions change. `SourceResult::retrieved` is decided by the caller and untouched,
/// because following a cross-reference says nothing about whether the answer was found for the
/// whole token or for a component; and the hit keeps its own `term` and `reading`, because the
/// reader selected 抜きとる and that is still the word on the page.
///
/// Four things leave a hit exactly as it was: a dictionary with no sequence bucket, a sequence
/// that resolves to nothing, a target that is itself only a pointer, and an ordinary entry. A
/// dictionary *error* is none of those and is returned, because an unreadable shard is a failure
/// and not an absence.
/// Ask **one** dictionary whether this exact string is a word, cross-references resolved.
///
/// **Whole-query only, and the handle stays with the caller.** [`sources`] answers the question a
/// reader's selection asks — "what is this token", falling back to components when the whole finds
/// nothing — and returns results labelled by dictionary *title*. Neither suits a caller proposing
/// lexical units: a component answer cannot support a grouping, and a title is not an identity,
/// because two installed copies can share one. This returns what the given dictionary says about
/// the given string, and the caller already knows which dictionary that is.
pub fn whole_word(dictionary: &Dictionary, query: &str) -> Result<Vec<WholeWord>, DictError> {
    crate::dict::evidence::Source::direct(dictionary).whole_word(query)
}

/// Every record the dictionary files under this **reading**, whatever its spelling, cross-references
/// resolved exactly as [`whole_word`] resolves them.
///
/// **Not a reader's lookup.** A selection wants the spelling-first answer [`whole_word`] gives.
/// This is for a caller that must know the whole field of candidates before claiming one is the
/// only one — `lexical::repair`, where a kana-spelled record would otherwise hide every kanji
/// homophone that derives the same span.
pub fn whole_word_by_reading(
    dictionary: &Dictionary,
    reading: &str,
) -> Result<Vec<WholeWord>, DictError> {
    crate::dict::evidence::Source::direct(dictionary).whole_word_by_reading(reading)
}

/// Resolve `hits`' cross-references, reading the targets through `source`.
///
/// **Per call, on hits the caller owns.** Resolution rewrites definitions, so it runs on top of
/// whatever `source` kept rather than being kept itself: two callers given the same lookup each
/// resolve their own copy.
pub(crate) fn resolved_through(
    source: crate::dict::evidence::Source<'_>,
    mut hits: Vec<crate::dict::index::LookupHit>,
) -> Result<Vec<WholeWord>, DictError> {
    let resolved = resolve_cross_references(&mut hits, source)?;
    Ok(hits
        .into_iter()
        .enumerate()
        .map(|(index, hit)| WholeWord {
            hit,
            same_word_as: resolved.get(&index).copied(),
        })
        .collect())
}

/// One dictionary record, and the relationship resolution actually established for it.
#[derive(Debug, Clone)]
pub struct WholeWord {
    pub hit: crate::dict::index::LookupHit,
    /// The sequence this record's senses were followed to.
    ///
    /// **Established, not inferred.** A dictionary having a sequence bucket proves only that it
    /// can be queried by sequence; it proves nothing about whether a negative sequence denotes the
    /// corresponding positive entry. This is set only where a cross-reference was followed and a
    /// target found, so a caller treating two records as one word relies on a relationship this
    /// code observed rather than on a convention it assumed for the whole dictionary.
    pub same_word_as: Option<i64>,
}

fn resolve_cross_references(
    hits: &mut [crate::dict::index::LookupHit],
    source: crate::dict::evidence::Source<'_>,
) -> Result<std::collections::BTreeMap<usize, i64>, DictError> {
    let mut resolved = std::collections::BTreeMap::new();
    if !source.dictionary.has_sequence() {
        return Ok(resolved);
    }
    for (index, hit) in hits.iter_mut().enumerate() {
        if hit.entry.sequence == 0 || !is_cross_reference(&hit.entry) {
            continue;
        }
        // `same_entry` keys on the absolute value, which is how the search-only form and the
        // word it belongs to are the same entry in the first place.
        let targets = source.same_entry(hit.entry.sequence, SAME_ENTRY_LIMIT)?;
        if let Some(target) = targets
            .into_iter()
            .find(|candidate| !is_cross_reference(candidate))
        {
            // The relationship, recorded where it was observed rather than assumed elsewhere.
            resolved.insert(index, target.sequence);
            hit.entry.definitions = target.definitions;
        }
    }
    Ok(resolved)
}

/// How many spellings of one word to read while looking for its senses.
///
/// The builder orders them commonest first, so the real entry is at or near the front; this is a
/// bound on a pathological sequence rather than a display limit.
const SAME_ENTRY_LIMIT: usize = 8;

/// One word with the sentence-local evidence available to Analysis, **and** the retrieval it came
/// from.
///
/// [`in_context`] is this with the evidence dropped, so the two cannot drift.
pub fn with_evidence(
    tokens: &[Token],
    index: usize,
    dictionaries: &[Dictionary],
) -> Result<Looked, LookupError> {
    let token = tokens
        .get(index)
        .ok_or(LookupError::OutsidePassage { index })?;
    let sources = sources(token, dictionaries)?;
    let word = merge::merge(token, &sources);
    // Analyzer-provided inline senses are already about this selected lemma. A definition
    // promoted from a dictionary is trustworthy for ranking only when that very source was
    // reached by its spelling or reading, not by the katakana-fold last resort.
    // `map_or(true, …)` rather than `is_none_or`, which needs Rust 1.82; this workspace's
    // MSRV is 1.80. Same reasoning as `grammar::pack`'s spelt-out check.
    let trustworthy_lookup = word.inline_source.as_deref().is_none_or(|promoted| {
        sources
            .iter()
            .find(|source| source.title == promoted)
            .is_some_and(|source| {
                source
                    .hits
                    .iter()
                    .any(|hit| matches!(hit.matched, MatchKind::Spelling | MatchKind::Reading))
            })
    });
    Ok(Looked {
        view: contextualize_with_lookup(word, tokens, index, trustworthy_lookup),
        evidence: sources,
    })
}

/// One word with the sentence-local evidence available to Analysis.
///
/// Deliberately separate from [`word`]. A Reference query has no selected position and no
/// neighbouring tokens, so applying this ordering there would make a contextual claim without
/// context. The returned view is already reordered, so summary, detail pane and Anki all see the
/// same canonical answer.
///
/// **A wrapper, kept so the production path is provably unchanged.** It is
/// [`with_evidence`] with the evidence dropped — not a second implementation that has to be kept
/// in step, which is the usual way a "diagnostics" branch quietly stops matching what ships.
pub fn in_context(
    tokens: &[Token],
    index: usize,
    dictionaries: &[Dictionary],
) -> Result<ContextualWordView, LookupError> {
    with_evidence(tokens, index, dictionaries).map(|looked| looked.view)
}

#[cfg(test)]
mod tests {
    use super::reading_for;
    use crate::model::Token;

    fn token(surface: &str, dict_form: &str, reading: &str, lemma_reading: &str) -> Token {
        Token {
            surface: surface.into(),
            dict_form: dict_form.into(),
            reading: reading.into(),
            lemma_reading: lemma_reading.into(),
            ..Token::default()
        }
    }

    /// **The case that showed "to flip" for a guitar being played.** The dictionary is asked for
    /// 弾く, so its entries have to be told apart by 弾く's reading, not by 弾いている's.
    #[test]
    fn an_inflected_word_is_ranked_by_its_dictionary_forms_reading() {
        let hiku = token("弾いている", "弾く", "ひいている", "ひく");
        assert_eq!(reading_for(&hiku, "弾く"), "ひく");
    }

    #[test]
    fn an_uninflected_word_reads_the_same_either_way() {
        let walk = token("散歩", "散歩", "さんぽ", "さんぽ");
        assert_eq!(reading_for(&walk, "散歩"), "さんぽ");
        // No dictionary form: the surface was asked, so the surface reading answers.
        let bare = token("ギター", "", "ぎたー", "");
        assert_eq!(reading_for(&bare, "ギター"), "ぎたー");
        // A dictionary form with no lemma reading falls back to the reading it has.
        let partial = token("帰る", "帰る", "かえる", "");
        assert_eq!(reading_for(&partial, "帰る"), "かえる");
    }

    /// When a component answered for a joined word, it is that component's reading that can tell
    /// the component's entries apart.
    #[test]
    fn a_component_that_answered_is_ranked_by_its_own_reading() {
        let mut joined = token("河童様", "河童様", "かっぱさま", "かっぱさま");
        joined.components = vec![
            token("河童", "河童", "かっぱ", "かっぱ"),
            token("様", "様", "さま", "さま"),
        ];
        assert_eq!(reading_for(&joined, "河童"), "かっぱ");
        assert_eq!(reading_for(&joined, "様"), "さま");
        // Something nobody asked about falls back to the token's own reading.
        assert_eq!(reading_for(&joined, "猫"), "かっぱさま");
    }
}
