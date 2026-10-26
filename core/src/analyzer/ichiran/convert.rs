//! Ichiran's output → the normalized model.
//!
//! This is where `project notes` §4 lives: every rule here was paid for with real
//! debugging, and each has a test naming the word that taught it. The function is
//! pure — it takes parsed JSON, not a subprocess — so the whole adapter is testable
//! against the committed fixtures without Ichiran installed.
//!
//! The four rules, in the order they bite:
//!
//! 1. **A word with its own `gloss` IS a dictionary word.** Its `conj` is then a
//!    speculative *alternative parse*, not its inflection (気持ち).
//! 2. **`via` is a chain, deepest = the transformation on the dictionary verb**
//!    (食べさせられて → Conjunctive over Causative-Passive of 食べる).
//! 3. **A `via` branch carrying its own `reading` is a competing parse**, not a
//!    chain link: follow one, prefer voice over potential (言わせて).
//! 4. **A compound's senses live on its components' `conj`**, not on the compound
//!    or the components themselves (伝わってきます).

use super::raw::{self, Conj, Gloss, Segment, WordInfo};
use crate::kana::to_hiragana;
use crate::model::{Alternative, Analysis, Conjugation, LineBreak, Prop, Sense, Token};

/// Turn parsed Ichiran output into the normalized model.
///
/// `text` is the original input; tokens are grounded back into it by scanning
/// forward, so byte offsets survive the round trip through a tool that does not
/// report them.
pub fn to_analysis(text: &str, segments: &[Segment]) -> Analysis {
    to_analysis_with(text, segments, &|_| None)
}

/// A reading Ichiran declined to choose: the tied word's spelling, and where it sits.
///
/// The span is where the word **will** be grounded — every tied entry shares one spelling, so it
/// is known before the choice — or `None` where it cannot be grounded at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tie<'a> {
    pub surface: &'a str,
    pub span: Option<(usize, usize)>,
}

/// The same, with a second opinion for the readings Ichiran declines to choose between.
///
/// **Only where it declined.** Ichiran scores its candidate parses and this takes the highest;
/// where the top two score *identically* it has said, in the only way it can, that it does not
/// know. 晴々 comes back as せいせい and はればれ both at 65, and in 晴々として…良い気分 the one
/// that happens to be first in the array is the wrong one — a wrong reading, and a wrong
/// definition under it, because the dictionary is then looked up by the wrong lemma.
///
/// `resolve` is asked for a surface and answers with a reading if it has one. A tied candidate
/// matching that answer wins; anything else leaves Ichiran's order alone. It is never consulted
/// where Ichiran was confident, because the sidecar is measurably better at exactly the
/// compounds and slang where it is sure — 陰キャ, 少しずつ, 飲み会 — and overruling it there
/// would trade a good answer for a cheap one.
pub fn to_analysis_with(
    text: &str,
    segments: &[Segment],
    resolve: &dyn Fn(&str) -> Option<String>,
) -> Analysis {
    to_analysis_resolving(text, segments, &|tie: &Tie| {
        resolve(tie.surface).into_iter().collect()
    })
}

/// The same, where the second opinion may read the tied word **in its sentence**.
///
/// `resolve` answers with readings in order of preference; the first that names one of the tied
/// entries wins, and none leaves Ichiran's order alone. The boundary is unchanged: it is asked
/// only on a tie, and it can only promote a tied entry.
pub fn to_analysis_resolving(
    text: &str,
    segments: &[Segment],
    resolve: &dyn Fn(&Tie) -> Vec<String>,
) -> Analysis {
    let mut grounder = Grounder::new(text, 0);
    let mut tokens = Vec::new();
    // Indices of pass-through tokens whose surface Ichiran rewrote, to be taken from the
    // source once their neighbours are grounded.
    let mut pass_through: Vec<usize> = Vec::new();

    for segment in segments {
        match segment {
            Segment::Unanalyzed(raw_text) => {
                // **Ichiran's echo of this text is not the text.** It normalises Japanese
                // punctuation into ASCII on the way through — 「 arrives back as `\"`, 、 as
                // `,`, 〜 as `-`, 【】 as `[]`, 『』 as `«»` — and the reader renders token
                // surfaces, so a captured 「台詞」 was displayed as "台詞". Grounding already
                // fails on these (the ASCII is not in the source), which is what makes them
                // recoverable: the span between the words on either side is the original,
                // and that is what gets shown.
                //
                // Deferred rather than filled here because the right-hand edge is the next
                // word's start, which has not been grounded yet.
                let mut token = Token {
                    surface: raw_text.clone(),
                    dict_form: raw_text.clone(),
                    ..Default::default()
                };
                if raw_text.trim().is_empty() {
                    // **A blank echo does not mean blank source.** ・ comes back as a single
                    // space, and skipping the segment here left those bytes covered by no
                    // token at all: 26 separators across 17 of 259 UD_Japanese-GSD sentences,
                    // every one of them a character the reader could see, not select, not look
                    // up and get no furigana for. It is the same normalisation the comment
                    // above describes, so it takes the same repair — but it cannot take the
                    // same grounding, because searching the source for " " would match a real
                    // space somewhere else entirely. Bracket it instead, and let
                    // `restore_pass_through` decide from the source what it really was.
                    pass_through.push(tokens.len());
                } else {
                    grounder.ground_pass_through(&mut token);
                    if token.start.is_none() {
                        pass_through.push(tokens.len());
                    }
                }
                tokens.push(token);
            }
            Segment::Candidates(candidates) => {
                // Ichiran ranks candidates by score, best first. 屠つ shows one
                // parse, so take it; the losers are not surfaced anywhere yet.
                let Some(best) = candidates.first() else {
                    continue;
                };
                for entry in &best.0 {
                    // Where this word will ground, asked only if it ties — a look ahead that
                    // does not move the cursor.
                    let here = |surface: &str| {
                        resolve(&Tie {
                            surface,
                            span: grounder.probe(surface),
                        })
                    };
                    let mut token = token_from_with(&entry.1, &here);
                    // The romaji lives on the entry, not on the word, so this is the only
                    // level that can supply it.
                    if !entry.0.trim().is_empty() {
                        token.romaji = Some(entry.0.clone());
                    }
                    grounder.ground(&mut token);
                    ground_components(&mut token, text);
                    tokens.push(token);
                }
            }
        }
    }

    restore_pass_through(text, &mut tokens, &pass_through);
    // Whatever the bracket could not turn into real source was genuine whitespace — a space
    // between two words, echoed as a space. Those carry nothing and would only clutter the
    // reader, which is what the old skip was for; it is applied here instead, where the source
    // has been consulted, rather than on the echo, which cannot tell the two cases apart.
    tokens.retain(|token| token.start.is_some() || !token.surface.trim().is_empty());

    Analysis {
        execution: None,
        text: text.to_string(),
        tokens,
    }
}

/// Give a rewritten pass-through token back the characters the reader actually captured.
///
/// The span is whatever sits between the grounded word before it and the grounded word after
/// it. Both edges are needed: without the left one a leading 「 would take the whole passage,
/// and without the right one a trailing 」 would.
///
/// Anything it cannot bracket is left exactly as Ichiran returned it. That is the honest
/// outcome for text with no grounded word on either side — a passage of pure punctuation —
/// and it is no worse than what happened before this existed.
fn restore_pass_through(text: &str, tokens: &mut [Token], indices: &[usize]) {
    for &index in indices {
        let from = tokens[..index]
            .iter()
            .rev()
            .find_map(|token| token.end)
            .unwrap_or(0);
        let to = tokens[index + 1..]
            .iter()
            .find_map(|token| token.start)
            .unwrap_or(text.len());
        if from >= to
            || to > text.len()
            || !text.is_char_boundary(from)
            || !text.is_char_boundary(to)
        {
            continue;
        }
        let slice = &text[from..to];
        if slice.trim().is_empty() {
            continue;
        }
        tokens[index].surface = slice.to_string();
        tokens[index].dict_form = slice.to_string();
        tokens[index].start = Some(from);
        tokens[index].end = Some(to);
    }
}

/// Walks the source text assigning byte spans to tokens in order.
struct Grounder<'a> {
    text: &'a str,
    cursor: usize,
    end: usize,
    /// The source folded the way the analyzer folds it, built on first need.
    ///
    /// Absent until a literal search fails, because the overwhelming majority of tokens are
    /// returned exactly as they were sent and never need it.
    folded: Option<Folded>,
}

/// The source text with the analyzer's own folding applied, and where every byte came from.
///
/// **The alternative was searching for the next occurrence of a normalized string in unnormalized
/// text, which cannot work and did not.** Ichiran returns ２０１１ as `2011`, the source contains
/// neither the other, and `find` answered `None` — 20 tokens per stage across 18 novel lines had no
/// offsets at all, and a normalized surface sat beside a residual token covering the same
/// characters. An offset is not something to search for twice; it is something to carry.
struct Folded {
    text: String,
    /// For each byte of `text`, the byte offset in the source it came from, plus a final entry
    /// for the end. Non-decreasing, so a source offset maps back by binary search.
    source: Vec<usize>,
}

impl Folded {
    fn of(text: &str) -> Folded {
        let mut folded = String::with_capacity(text.len());
        let mut source = Vec::with_capacity(text.len() + 1);
        let mut characters = text.char_indices().peekable();
        while let Some((at, original)) = characters.next() {
            let mut character = fold(original);
            // Ichiran composes a half-width voiced pair (ｶ + ﾞ → ガ). Map the composed
            // scalar back to the beginning of BOTH source characters; the next map entry
            // supplies their combined end. Standalone marks in kaomoji remain spacing marks.
            if matches!(original, '\u{FF66}'..='\u{FF9D}' | '\u{30A1}'..='\u{30FA}') {
                let mark = match characters.peek() {
                    Some((_, '\u{FF9E}')) => Some('\u{3099}'),
                    Some((_, '\u{FF9F}')) => Some('\u{309A}'),
                    _ => None,
                };
                if let Some(composed) =
                    mark.and_then(|mark| unicode_normalization::char::compose(character, mark))
                {
                    character = composed;
                    characters.next();
                }
            }
            let before = folded.len();
            folded.push(character);
            source.resize(source.len() + (folded.len() - before), at);
        }
        source.push(text.len());
        Folded {
            text: folded,
            source,
        }
    }

    /// The first folded byte that came from `offset` or later.
    fn folded_at(&self, offset: usize) -> usize {
        self.source.partition_point(|&from| from < offset)
    }
}

/// Width normalizations observed in Ichiran's echo, used for alignment only.
///
/// `U+FF01`-`U+FF5E` is the ASCII range's wide twin, offset by `0xFEE0` — the same fold
/// `translate::fugumt` and `ocr::score` already use. Japanese punctuation is *also* rewritten on the
/// way through, but that is recovered separately and by a better route: those tokens fail to ground,
/// and the span between the words on either side is the original. This is for the case where the
/// token is a word and there is nothing either side to interpolate from. Half-width kana are
/// returned as full-width kana, including voiced pairs (handled by `Folded::of`); isolated
/// half-width sound marks in kaomoji become spacing ゛/゜. Do not apply general compatibility
/// normalization: matching ① against 1 would assert a rewrite not established by this adapter.
fn fold(character: char) -> char {
    use unicode_normalization::UnicodeNormalization;
    match character {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(character as u32 - 0xFEE0).unwrap_or(character),
        '\u{FF66}'..='\u{FF9D}' => character.to_string().nfkc().next().unwrap_or(character),
        '\u{FF9E}' => '\u{309B}',
        '\u{FF9F}' => '\u{309C}',
        _ => character,
    }
}

impl<'a> Grounder<'a> {
    fn new(text: &'a str, from: usize) -> Self {
        Grounder {
            text,
            cursor: from,
            end: text.len(),
            folded: None,
        }
    }

    /// Where a token spelled `surface` would ground from here, without grounding it.
    fn probe(&self, surface: &str) -> Option<(usize, usize)> {
        let mut ahead = Grounder {
            text: self.text,
            cursor: self.cursor,
            end: self.end,
            folded: None,
        };
        let mut token = Token {
            surface: surface.to_string(),
            ..Default::default()
        };
        ahead.ground(&mut token);
        Some((token.start?, token.end?))
    }

    /// Pass-through echoes may change spacing as well as punctuation. A failed local match
    /// must be bracketed by its neighbours, not found at a later repetition: `！(` can come
    /// back as `! (` and searching ahead would jump to a later `！ (` across entire sentences.
    fn ground_pass_through(&mut self, token: &mut Token) {
        let from = self.cursor.min(self.end);
        self.ground(token);
        if token
            .start
            .is_some_and(|start| !self.text[from..start].trim().is_empty())
        {
            self.cursor = from;
            token.start = None;
            token.end = None;
        }
    }

    /// Find `token.surface` at or after the cursor and record its span.
    ///
    /// Leaves the span as `None` only when the surface is in the source under **no** spelling —
    /// the punctuation rewrites, which callers still tolerate and recover by interpolation.
    fn ground(&mut self, token: &mut Token) {
        if token.surface.is_empty() {
            token.start = None;
            token.end = None;
            return;
        }
        let from = self.cursor.min(self.end);
        let literal = self.text[from..self.end].find(&token.surface);
        // A later exact spelling must not steal an earlier width-equivalent occurrence.
        // With ｶﾞ ガ (or Ａ A), literal-first matching skips the first word and leaves the
        // second ungrounded. An exact match at the cursor remains the allocation-free path.
        if literal != Some(0) && self.ground_folded(token, from) {
            return;
        }
        if let Some(offset) = literal {
            let start = from + offset;
            token.start = Some(start);
            token.end = Some(start + token.surface.len());
            self.cursor = start + token.surface.len();
            return;
        }
        token.start = None;
        token.end = None;
    }

    /// The same search against the folded source, mapping the answer back to source offsets.
    ///
    /// **The span and the surface both come from the source**, never from the analyzer's echo of
    /// it: a token grounded here is displayed, and displaying `2011` over a passage that reads
    /// ２０１１ is the same class of defect as showing `"` for 「.
    fn ground_folded(&mut self, token: &mut Token, from: usize) -> bool {
        let text = self.text;
        let end = self.end;
        let folded = self.folded.get_or_insert_with(|| Folded::of(text));
        let surface = Folded::of(&token.surface).text;
        // Nothing was folded, so the literal search above already answered.
        if surface == token.surface && folded.text == text {
            return false;
        }
        let (window_from, window_to) = (folded.folded_at(from), folded.folded_at(end));
        if window_from > window_to
            || !folded.text.is_char_boundary(window_from)
            || !folded.text.is_char_boundary(window_to)
        {
            return false;
        }
        let Some(offset) = folded.text[window_from..window_to].find(&surface) else {
            return false;
        };
        let at = window_from + offset;
        // `source` carries one entry per byte plus the end, so the byte just past the match has an
        // entry whatever the match is — this is what keeps a multi-byte fold from losing its tail.
        let (Some(&start), Some(&stop)) =
            (folded.source.get(at), folded.source.get(at + surface.len()))
        else {
            return false;
        };
        if start >= stop || !text.is_char_boundary(start) || !text.is_char_boundary(stop) {
            return false;
        }
        token.start = Some(start);
        token.end = Some(stop);
        token.surface = text[start..stop].to_string();
        self.cursor = stop;
        true
    }
}

/// Ground a compound's components inside the compound's own span, so a component
/// can never be matched against text belonging to a different word.
fn ground_components(token: &mut Token, text: &str) {
    let (Some(start), Some(end)) = (token.start, token.end) else {
        return;
    };
    let mut grounder = Grounder {
        text,
        cursor: start,
        end,
        folded: None,
    };
    for component in &mut token.components {
        grounder.ground(component);
        ground_components(component, text);
    }
}

/// Convert one word, recording how sure Ichiran was of it.
///
/// The score is stamped here rather than in each of [`token_body`]'s four exits, and only
/// when nothing has claimed it: an ambiguous surface arrives as a wrapper with no score of
/// its own, and the score that matters is the chosen alternative's, which
/// [`token_from_alternatives`] has already set by the time this sees it.
fn token_from(info: &WordInfo) -> Token {
    token_from_with(info, &|_| Vec::new())
}

fn token_from_with(info: &WordInfo, resolve: &dyn Fn(&str) -> Vec<String>) -> Token {
    let mut token = token_body(info, resolve);
    if token.confidence.is_none() {
        token.confidence = Some(info.score.round() as i32);
    }
    token
}

/// Convert one word.
fn token_body(info: &WordInfo, resolve: &dyn Fn(&str) -> Vec<String>) -> Token {
    // An ambiguous surface. Ichiran wraps the competing dictionary entries in
    // `alternative` and gives the outer entry no fields of its own, so this must be
    // unwrapped before anything else is read.
    if !info.alternative.is_empty() {
        return token_from_alternatives(&info.alternative, resolve);
    }

    let surface_reading = surface_reading(info);

    // Rule 4 — a compound. Its meaning is the sum of its parts: the compound
    // itself carries no gloss and no conj, and each component's senses sit on that
    // component's `conj`. Reading only the components' own fields yields nothing,
    // which is what once hid every compound's definition.
    if info.components.len() >= 2 {
        let components: Vec<Token> = info.components.iter().map(token_from).collect();
        // The head is the first component: 伝わってきます is a 伝わる, and the
        // auxiliary contributes a grammar pattern (〜てくる) rather than a headword.
        // Naming the pattern is the grammar layer's job, not the analyzer's.
        let (dict_form, lemma_reading) = components
            .first()
            .map(|head| (head.dict_form.clone(), head.lemma_reading.clone()))
            .unwrap_or_default();
        return Token {
            // Filled in by `analyzer::lines::mark` once the whole passage is grounded: the
            // break lives in the gap between two tokens, which no single token can see.
            line_break: LineBreak::None,
            surface: info.text.clone(),
            dict_form,
            reading: surface_reading,
            lemma_reading,
            lexical_role: None,
            senses: Vec::new(),
            conjugation: None,
            components,
            // Decided later, from the source, by `analyzer::mark_selectable`.
            selectable: false,
            constructions: Vec::new(),
            alternatives: Vec::new(),
            start: None,
            end: None,
            confidence: None,
            romaji: None,
            // Only `dict::names` sets this, and only after the segmenter has been second-guessed.
            reading_unverified: false,
            reading_ambiguous: false,
        };
    }

    // Rule 1 — the word has its own gloss, so *it* is the dictionary word. Any
    // conj entry is Ichiran reading it as an inflection of something else (気持ち
    // as the stem of 気持ちい): an alternative parse, not this word's grammar.
    // Preferring the conj here mislabels common words and invents grammar.
    if !info.gloss.is_empty() {
        let (spelling, kana) = raw::split_reading(&info.reading);
        return Token {
            // Filled in by `analyzer::lines::mark` once the whole passage is grounded: the
            // break lives in the gap between two tokens, which no single token can see.
            line_break: LineBreak::None,
            surface: info.text.clone(),
            dict_form: if spelling.is_empty() {
                info.text.clone()
            } else {
                spelling.to_string()
            },
            reading: surface_reading,
            lemma_reading: to_hiragana(kana),
            lexical_role: None,
            senses: senses_from(&info.gloss, sequence_of(info)),
            conjugation: None,
            components: Vec::new(),
            // Decided later, from the source, by `analyzer::mark_selectable`.
            selectable: false,
            constructions: Vec::new(),
            alternatives: info.conj.iter().map(alternative_from).collect(),
            start: None,
            end: None,
            confidence: None,
            romaji: None,
            // Only `dict::names` sets this, and only after the segmenter has been second-guessed.
            reading_unverified: false,
            reading_ambiguous: false,
        };
    }

    // A conjugated word: everything — lemma, senses, grammar — comes off the conj.
    if let Some((first, rest)) = info.conj.split_first() {
        let mut chain = walk(first);
        // **Ichiran states the sequence on the word, not on its conj levels** — a conj entry
        // carries only `gloss`, `prop`, `reading` and `readok`. So it is attached here, where the
        // word is still in scope, rather than threaded through `walk` and `descend` for a value
        // neither of them can see.
        for sense in &mut chain.senses {
            sense.sequence = sequence_of(info);
        }
        let mut alternatives = chain.dropped;
        alternatives.extend(rest.iter().map(alternative_from));

        return Token {
            // Filled in by `analyzer::lines::mark` once the whole passage is grounded: the
            // break lives in the gap between two tokens, which no single token can see.
            line_break: LineBreak::None,
            surface: info.text.clone(),
            dict_form: chain.lemma.clone(),
            reading: surface_reading,
            lemma_reading: chain.lemma_reading.clone(),
            lexical_role: None,
            senses: chain.senses,
            conjugation: Some(Conjugation {
                lemma: chain.lemma,
                reading: chain.lemma_reading,
                props: chain.props,
            }),
            components: Vec::new(),
            // Decided later, from the source, by `analyzer::mark_selectable`.
            selectable: false,
            constructions: Vec::new(),
            alternatives,
            start: None,
            end: None,
            confidence: None,
            romaji: None,
            // Only `dict::names` sets this, and only after the segmenter has been second-guessed.
            reading_unverified: false,
            reading_ambiguous: false,
        };
    }

    // Neither gloss nor conj: nothing to say beyond the surface.
    Token {
        surface: info.text.clone(),
        dict_form: info.text.clone(),
        reading: surface_reading,
        ..Default::default()
    }
}

/// Resolve an ambiguous surface: the best-scoring entry becomes the token, and the
/// rest are kept as readings the analyzer considered but did not choose.
///
/// Entries with nothing to say — no lemma and no senses — are dropped rather than
/// padding the "also parses as" list with blanks.
fn token_from_alternatives(
    alternatives: &[WordInfo],
    resolve: &dyn Fn(&str) -> Vec<String>,
) -> Token {
    let mut ranked: Vec<&WordInfo> = alternatives.iter().collect();
    // Descending by score; `sort_by` is stable, so ties keep Ichiran's order.
    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // **A tie is Ichiran saying it does not know**, and the only place a second opinion is
    // allowed to move the answer. Promoting a tied candidate rather than replacing the reading
    // keeps everything consistent: the lemma, the senses and the reading all come from one
    // entry, so the dictionary is looked up by the word that was actually chosen.
    if let Some(best) = ranked.first().copied() {
        let tied = ranked
            .iter()
            .take_while(|other| other.score == best.score)
            .count();
        if tied > 1 {
            // The first preference that names a tied entry wins; one that names nothing tied
            // gives way to the next rather than ending the search.
            //
            // Compared as hiragana: `project notes` §4 keeps katakana katakana in what is stored, so
            // the two sides can spell the same sound differently.
            //
            // `..tied` and the `> 1` above are the same rule twice. The gate is what
            // `a_confident_ichiran_is_never_overruled` actually pins — removing this slice alone
            // changes no fixture, because no captured tie has a third, lower-scored candidate
            // behind it. Kept anyway: it is one token, and it is the half that stays right if the
            // gate is ever loosened.
            let found = resolve(&best.text).into_iter().find_map(|preferred| {
                ranked[..tied].iter().position(|other| {
                    crate::kana::to_hiragana(&surface_reading(other)) == preferred
                })
            });
            if let Some(at) = found {
                let chosen = ranked.remove(at);
                ranked.insert(0, chosen);
            }
        }
    }

    let Some((best, rest)) = ranked.split_first() else {
        return Token::default();
    };

    let mut token = token_from(best);
    token.alternatives.extend(
        rest.iter()
            .map(|info| alternative_from_word(info))
            .filter(|alternative| !alternative.lemma.is_empty() || !alternative.senses.is_empty()),
    );
    token
}

/// Flatten a competing entry into an [`Alternative`].
fn alternative_from_word(info: &WordInfo) -> Alternative {
    let token = token_from(info);
    Alternative {
        form: token
            .conjugation
            .as_ref()
            .and_then(|conjugation| conjugation.props.first())
            .map(|prop| prop.form.clone())
            .unwrap_or_default(),
        lemma: token.dict_form,
        reading: token.lemma_reading,
        senses: token.senses,
    }
}

/// The surface reading, which drives furigana. Deliberately **not** normalized to
/// hiragana: コーヒー must stay katakana (`project notes` §4).
///
/// **Zero-width joiners are stripped, and one of them is not hypothetical.** Ichiran writes
/// the topic particle は with a zero-width non-joiner in front of it — presumably marking that
/// it is said "wa" — in `kana` but not in `reading`. It is invisible, so it survived every
/// reading of the output, and it made the commonest particle in the language compare unequal
/// to itself: furigana draws ruby only where the reading differs from the surface, so every は
/// in every passage carried a redundant は above it, and audio and Anki lookups keyed on the
/// reading missed.
fn surface_reading(info: &WordInfo) -> String {
    let reading = if info.kana.is_empty() {
        raw::split_reading(&info.reading).1.to_string()
    } else {
        info.kana.clone()
    };
    strip_invisible(&reading)
}

/// Drop the format characters that carry no sound.
///
/// Only these three: they are the ones that turn up inside kana and mean nothing when spoken.
/// Whitespace is left alone — Ichiran separates a compound's parts with a space, and that is a
/// boundary the reading layer is entitled to see.
fn strip_invisible(reading: &str) -> String {
    reading
        .chars()
        .filter(|ch| !matches!(ch, '\u{200b}' | '\u{200c}' | '\u{200d}'))
        .collect()
}

/// Ichiran's `info` ("after the -te form of a verb") and `field` ("{sumo}") are
/// dropped: the grammar pack owns pattern explanations (`project notes` §5, one source
/// of text), and Phase 2's dictionary layer owns the richer sense type. Adding a
/// half-model for them now would only be churn.
/// Ichiran writes 0 for "no sequence", which is not a sequence.
fn sequence_of(info: &WordInfo) -> Option<i64> {
    (info.seq != 0).then_some(info.seq)
}

fn senses_from(gloss: &[Gloss], sequence: Option<i64>) -> Vec<Sense> {
    gloss
        .iter()
        .map(|entry| Sense {
            pos: entry.pos.clone(),
            gloss: entry.gloss.clone(),
            sequence,
        })
        .collect()
}

/// What walking one conj entry's chain yields.
#[derive(Default)]
struct Chain {
    props: Vec<Prop>,
    lemma: String,
    lemma_reading: String,
    senses: Vec<Sense>,
    /// Competing parses that were not followed — surfaced as "also parses as".
    dropped: Vec<Alternative>,
}

/// Walk a conj entry and its `via` chain into a flat, depth-tagged prop list.
///
/// Rule 2: depth increases inwards, so the **deepest** prop is the transformation
/// applied to the dictionary verb. That prop is marked `is_base` and carries the
/// verb's reading, which is what lets the grammar layer show *how* a form is built
/// instead of only naming it. Rule 3 picks a single path at a fork.
fn walk(conj: &Conj) -> Chain {
    let mut chain = Chain::default();
    descend(conj, 0, &mut chain);

    // Mark the deepest props: the base transformation on the dictionary verb.
    if let Some(deepest) = chain.props.iter().map(|prop| prop.via_depth).max() {
        let base_reading = chain.lemma_reading.clone();
        for prop in chain.props.iter_mut().filter(|p| p.via_depth == deepest) {
            prop.is_base = true;
            prop.base_reading = base_reading.clone();
        }
    }
    chain
}

fn descend(conj: &Conj, depth: u32, chain: &mut Chain) {
    for prop in &conj.prop {
        chain.props.push(Prop {
            pos: prop.pos.clone(),
            form: prop.form.clone(),
            neg: prop.neg,
            fml: prop.fml,
            via_depth: depth,
            base_reading: String::new(),
            is_base: false,
        });
    }

    // A reading at this level names the lemma. Deeper levels win, so the lemma
    // ends up being the dictionary word at the bottom of the chain.
    if let Some(reading) = &conj.reading {
        let (spelling, kana) = raw::split_reading(reading);
        if !spelling.is_empty() {
            chain.lemma = spelling.to_string();
            // Rule: normalize the *lemma* reading to hiragana — Ichiran writes
            // 来る's as クる — but never the surface reading.
            chain.lemma_reading = to_hiragana(kana);
        }
    }
    if !conj.gloss.is_empty() {
        chain.senses = senses_from(&conj.gloss, None);
    }

    if conj.via.is_empty() {
        return;
    }

    // Rule 3 — at a fork, follow exactly one branch. Surfacing both makes the
    // grammar view show a phantom form: 言わせて returns 言う (Causative) and
    // 言わす (Potential), and showing the second invents a Potential that is not
    // there. The rest become "also parses as".
    let chosen = choose_branch(&conj.via);
    for (index, branch) in conj.via.iter().enumerate() {
        if index != chosen {
            chain.dropped.push(alternative_from(branch));
        }
    }
    descend(&conj.via[chosen], depth + 1, chain);
}

/// Pick which competing `via` branch to follow: a voice transformation beats a
/// potential reading. Ties go to Ichiran's order, so the choice is deterministic.
///
/// Order matters within the voice test only in that causative-passive contains
/// both words — any of the three outranks potential, which is all this needs.
fn choose_branch(via: &[Conj]) -> usize {
    via.iter()
        .enumerate()
        .max_by_key(|(index, branch)| {
            // max_by_key returns the *last* maximum; negate the index so ties
            // resolve to the earliest branch instead.
            (branch_rank(branch), std::cmp::Reverse(*index))
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn branch_rank(conj: &Conj) -> u8 {
    let mentions = |needle: &str| {
        conj.prop
            .iter()
            .any(|prop| prop.form.to_ascii_lowercase().contains(needle))
    };
    if mentions("causative") || mentions("passive") {
        2 // a voice transformation
    } else if mentions("potential") {
        0 // the phantom
    } else {
        1
    }
}

/// A parse Ichiran offered that 屠つ did not adopt.
fn alternative_from(conj: &Conj) -> Alternative {
    let chain = walk(conj);
    Alternative {
        lemma: chain.lemma,
        reading: chain.lemma_reading,
        // The outermost prop names the form this parse would have produced.
        form: chain
            .props
            .first()
            .map(|prop| prop.form.clone())
            .unwrap_or_default(),
        senses: chain.senses,
    }
}
