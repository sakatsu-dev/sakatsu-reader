//! Putting back together a name the segmenter took apart.
//!
//! # The problem
//!
//! Ichiran's database is built from JMdict, which carries no names. Hand it a place name it does
//! not have and it does not fail — it finds the best reading it can for each *character*, all of
//! which are real words on their own:
//!
//! ```text
//! 善福寺川  →  善 (ぜん) 21    福 (ふく) 20    寺 (てら) 24    川 (かわ) 23
//! ```
//!
//! Four tokens, four correct-in-isolation readings, and a reading of the whole that is wrong. The
//! reader then prints ぜん・ふく・てら・かわ over a river called ぜんぷくじがわ.
//!
//! **The name was installed the whole time.** JMnedict has 善福寺川 tagged `place`, and
//! [`merge`](super::merge) already knows how to get ぜんぷくじがわ out of it. Nothing asked, because
//! lookups are per token and by then no token spanned the name.
//!
//! # Why a post-pass rather than a better segmenter
//!
//! The two standard answers are a dictionary that contains names (Sudachi's split mode C returns
//! the longest named-entity unit) or a compiled user dictionary (MeCab, NEologd). Neither is open to
//! us: `ichiran-cli` is a Lisp binary over PostgreSQL with no user-dictionary input.
//!
//! So this works on what the analyzer already returned, which has a real advantage — it is
//! analyzer-agnostic. Lindera gets it for free, and so does whatever replaces either of them.
//!
//! # What it will not do
//!
//! It does not re-segment. Yomitan's approach — longest match over the merged dictionary, ignoring
//! the analyzer — would find more names and throw away the conjugation chains, compound components
//! and grammar matches that are the reason this app runs Ichiran at all. Only runs the analyzer
//! itself was unsure about are touched, and only when a name dictionary confirms the join.

use crate::dict::{tags, Dictionary};
use crate::kana::to_hiragana;
use crate::model::{Alternative, LineBreak, Token};

/// At or above this score, the analyzer is asserting a whole word rather than offering a fragment.
///
/// **Measured by running Ichiran, not borrowed.** The pieces of a shattered name score low, and the
/// piece lengths vary — which is why this is a score test and not a "single character" test:
///
/// ```text
/// 善福寺川 → 善(21)  福(20)  寺(24)  川(23)
/// 神田川   → 神(12)  田川(33)
/// 多摩川   → 多摩(65) 川(23)
/// ```
///
/// Anything Ichiran holds as a unit scores far higher *and* arrives as a single token, so there is
/// no run to consider in the first place: 山川(208), 白子(208), 大小(208), 東京(325), 東京都(442).
/// The gap between 65 and 208 is wide, and 100 sits in it.
///
/// **Deliberately not `merge::NAME_WINS_BELOW`, which is 16.** That was measured for a different
/// phenomenon — a katakana name folded onto a rare homophone, which scores 8-9 — and reusing it here
/// looked tidy and silently excluded every real case above: 20, 21, 23, 24, 33, 65 are all above 16.
/// Two numbers, because there are two questions.
const A_WHOLE_WORD_FROM: i32 = 100;

/// The most tokens one name may be reassembled from.
///
/// Long enough for the compound place names this exists for — 善福寺川 is four, and
/// 東京都水道局 would be six — and short enough that the work per position stays five index
/// lookups rather than growing with the sentence.
const MAX_RUN: usize = 6;

/// One reading a name dictionary offers, with the category it offers it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameReading {
    /// The kana reading, from whichever field the source states it in.
    pub reading: String,
    /// The dictionary's own codes — `place`, `surname`, `fem`.
    pub tags: Vec<String>,
}

/// What a name dictionary says about a joined surface.
///
/// **Every admissible reading, not a verdict.** This used to carry one, latched from the first
/// entry that qualified, and JMnedict gives 百鬼 three entries and nine readings at identical
/// scores — so `Dictionary::lookup`'s score sort was a no-op and **the order of the term bank
/// decided what a reader saw**. It chose おきそ, from an entry tagged `fem`, while a later entry
/// carried ひゃくき, which is what the analyzer had already read the same characters as.
///
/// Choosing is now [`chosen_reading`]'s job, and it has the evidence to do it with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundName {
    /// The reading to show. Kept as a field so callers that want one answer still have one.
    pub reading: String,
    /// The chosen reading's own categories.
    pub tags: Vec<String>,
    /// Every reading the dictionaries offered, in the order they offered them.
    pub candidates: Vec<NameReading>,
}

impl FoundName {
    /// A name with exactly one stated reading.
    ///
    /// The ordinary shape, and what the unit tests below mean when they hand `merge_names` a
    /// closure: one reading, nothing to choose between.
    pub fn one(reading: impl Into<String>, tags: Vec<String>) -> FoundName {
        let reading = reading.into();
        FoundName {
            candidates: vec![NameReading {
                reading: reading.clone(),
                tags: tags.clone(),
            }],
            reading,
            tags,
        }
    }
}

/// Ask the open dictionaries whether `surface` is a name, and how it is read.
///
/// Separate from [`merge_names`] so the policy can be tested without a dictionary on disk, and so
/// the shell has nothing to decide — it opens dictionaries, this knows what to ask them.
///
/// **Exact spellings only.** [`Dictionary::lookup`] will also reach entries by folded kana, which
/// is right for a reader typing a query and wrong here: a fold means "something is read this way",
/// and joining four characters on that basis would invent a name out of a coincidence.
///
/// **A join that is also an ordinary word is refused**, which is the `バー` lesson one level up.
/// 白 + 子 are two weak single kanji and JMnedict files 白子 as a place in Chiba — but Jitendex has
/// 白子 spelled exactly that way, meaning "soft roe". Joining it and taking the name's reading would
/// put a Chiba town where the roe was, and this very repository already has シロコ on record as the
/// case that taught it. So if any dictionary has the joined string as a *word*, this reports
/// nothing and the pieces stay as the analyzer left them.
///
/// The cost is that a shattered ordinary compound stays shattered. That is the declared scope —
/// names only — and repairing words would put this in the business of second-guessing segmentation
/// on strings the analyzer had a real opinion about.
///
/// Generic over the iterator rather than taking a slice, so the shell can pass its `Vec<Dictionary>`
/// and a test can pass a projection of the `(title, dictionary)` pairs it keeps.
pub fn look_up_name<'a>(
    dictionaries: impl IntoIterator<Item = &'a Dictionary>,
    surface: &str,
) -> Option<FoundName> {
    named(
        dictionaries
            .into_iter()
            .map(|dictionary| dictionary.lookup(surface)),
        surface,
    )
}

/// [`look_up_name`], reading through one request's evidence.
pub fn look_up_name_in(
    evidence: super::evidence::Evidence<'_>,
    surface: &str,
) -> Option<FoundName> {
    named(
        evidence.sources().map(|source| source.lookup(surface)),
        surface,
    )
}

/// The decision, over each dictionary's answer in order. **Lazy**: a word found in one dictionary
/// ends the search, so the dictionaries after it are never asked.
fn named(
    lookups: impl Iterator<Item = Result<Vec<super::index::LookupHit>, super::DictError>>,
    surface: &str,
) -> Option<FoundName> {
    let mut candidates: Vec<NameReading> = Vec::new();

    for looked in lookups {
        let Ok(hits) = looked else {
            continue;
        };
        for hit in hits {
            // Exact spellings only. `lookup` will also reach entries by folded kana, which is right
            // for a reader typing a query and wrong here: a fold means "something is read this
            // way", and joining characters on that basis invents a name out of a coincidence.
            if hit.matched != super::MatchKind::Spelling {
                continue;
            }

            if !tags::marks_a_name(&hit.entry.definition_tags) {
                // Spelled this way, and not a name: the join is a word. Nothing to do here.
                return None;
            }

            // **Every reading this entry states, and every entry.** Stopping at the first was what
            // made the term bank's order decide the answer — see [`FoundName`].
            for reading in super::merge::stated_readings(&hit.entry) {
                if reading == surface {
                    // A name whose "reading" is its own spelling tells a reader nothing, and would
                    // put the kanji back over the kanji as furigana.
                    continue;
                }
                if candidates
                    .iter()
                    .any(|seen| to_hiragana(&seen.reading) == to_hiragana(&reading))
                {
                    continue;
                }
                candidates.push(NameReading {
                    reading,
                    tags: hit.entry.definition_tags.clone(),
                });
            }
        }
    }

    let first = candidates.first()?;
    Some(FoundName {
        reading: first.reading.clone(),
        tags: first.tags.clone(),
        candidates,
    })
}

/// Which of a name's readings to show, given what the analyzer had already read.
///
/// **The dictionary's order is not evidence.** JMnedict scores its name entries identically, so the
/// sort in `Dictionary::lookup` leaves them in bank order, and taking the first is a coin toss
/// dressed as a lookup. This asks a question that has an answer instead.
///
/// The run carries the analyzer's own reading of the same characters. Where one of the dictionary's
/// readings **agrees with it**, two independent sources have said the same thing and that is the one
/// to show — for 百鬼 split as 百 + 鬼 the parts read ひゃく + き, and ひゃくき is among the six
/// readings the surname entry lists, four places after the おきそ that was being chosen.
///
/// Where nothing agrees and the dictionary offers only one reading, there is nothing to choose
/// between and it is taken. Where it offers several and none matches, **the evidence cannot choose**
/// — the first is shown because a name still needs a reading, and the token is marked ambiguous
/// with the rest recorded as alternatives, which is what those mechanisms are for.
fn chosen_reading(run: &[Token], found: &FoundName) -> (NameReading, bool) {
    let read_as: String = run.iter().map(|token| token.reading.as_str()).collect();
    let agreeing = found.candidates.iter().find(|candidate| {
        !read_as.is_empty() && to_hiragana(&candidate.reading) == to_hiragana(&read_as)
    });
    if let Some(agreeing) = agreeing {
        return (agreeing.clone(), false);
    }
    let first = found
        .candidates
        .first()
        .cloned()
        .unwrap_or_else(|| NameReading {
            reading: found.reading.clone(),
            tags: found.tags.clone(),
        });
    (first, found.candidates.len() > 1)
}

/// Whether a piece is a fragment rather than a word the analyzer is asserting.
///
/// A score below [`A_WHOLE_WORD_FROM`] is the signature — see that constant for the measurements.
/// Piece *length* is deliberately not part of this: 神田川 shatters into 神 and 田川, and 多摩川 into
/// 多摩 and 川, so a rule that demanded single characters missed two of the three rivers that
/// prompted this.
///
/// **No score at all counts as a fragment**, which is the opposite of `merge`'s rule for
/// `NAME_WINS_BELOW` and is right for a different reason. There, a missing number could not be
/// allowed to *assert* something (that a name should displace a word). Here it only decides what is
/// worth asking the dictionary about, and the dictionary has the final say — so under Lindera, which
/// reports no scores, this asks about more spans and joins exactly the ones JMnedict confirms.
fn fragment(token: &Token) -> bool {
    token
        .confidence
        .is_none_or(|score| score < A_WHOLE_WORD_FROM)
}

/// Whether a token is a single kana character, which may *bridge* two halves of a name.
///
/// 井の頭, 霞ヶ関, 阿佐ヶ谷: the kana is part of the name and there is no way to join around it.
fn bridging_kana(token: &Token) -> bool {
    let mut chars = token.surface.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if !crate::kana::is_kanji(c))
}

/// Whether a run may be offered to the dictionary at all.
///
/// **A piece must carry a kanji unless it is bridging two that do**, and that restriction is not
/// fussiness — it is the hole the first version of this had. Particles score 4 and 11, so any
/// score-based test calls them fragments, and a run of kana then needed one dictionary hit to become
/// a name. JMnedict files 741,346 entries with plenty of short kana given names among them, so
/// は + あい in 私はあいさつした was exactly one such hit away from becoming a person.
///
/// Requiring the ends to be kanji closes it: a particle is never the first or last piece of a
/// candidate. The interior stays open so the names that genuinely contain kana still join —
/// 井の頭, 霞ヶ関, 阿佐ヶ谷.
///
/// The known cost is a name that *begins* or *ends* in kana. Those are left alone rather than risking
/// the false positive, and their kana is already unambiguous to a reader anyway.
fn eligible(run: &[Token]) -> bool {
    let last = run.len() - 1;
    // **A name never spans a line break, not even a wrap.** `joined` concatenates the surfaces and
    // takes `first.start..last.end`, so a run crossing a break produces a token whose `surface` is
    // missing a newline that `text[start..end]` still has — the invariant every consumer of those
    // offsets relies on, asserted in `core/tests/analyzer_contract.rs`. The rule is about bytes,
    // which is why a `Wrap` is refused too even though a wrapped name is genuinely one name.
    if run[..last]
        .iter()
        .any(|token| token.line_break != LineBreak::None)
    {
        return false;
    }
    // **And the same rule, stated about the bytes themselves.**
    //
    // The line-break test above was the only guard, and it covers one way a run can be
    // discontiguous. Any untokenized byte between two pieces does exactly the same damage, and a
    // space is the common one: `原 俊介` arrives as `原` at 15..18 and `俊介` at 19..22 with nothing
    // claiming byte 18. Joined, that produced `surface` = `原俊介` over span 15..25, whose source
    // slice is `原 俊介` — so `text[start..end] != surface`, in two of the corpus's own captures.
    //
    // It escaped notice because `analyzer_contract.rs` asserts the invariant on **analyzer output**
    // and this pass runs afterwards, inside `Pipeline::segmentation`. The invariant held where it
    // was tested and broke where it was not.
    //
    // The rule is scoped to exactly when a claim is made. [`joined`] takes
    // `first.start .. last.end`, so the joined token claims a span **only** when both of those
    // exist — ungrounded pieces produce an ungrounded name, which asserts nothing and needs no
    // check. When a claim *is* made, every interior boundary has to be verifiably contiguous, and
    // a missing offset in the middle makes it unverifiable rather than acceptable.
    let claims_a_span = run.first().and_then(|token| token.start).is_some()
        && run.last().and_then(|token| token.end).is_some();
    if claims_a_span
        && !run
            .windows(2)
            .all(|pair| match (pair[0].end, pair[1].start) {
                (Some(end), Some(start)) => end == start,
                _ => false,
            })
    {
        return false;
    }
    run.iter().enumerate().all(|(index, token)| {
        let interior = index != 0 && index != last;
        if interior && bridging_kana(token) {
            return true;
        }
        token.surface.chars().any(crate::kana::is_kanji) && fragment(token)
    })
}

/// Join runs of weak adjacent tokens that spell a name, longest first.
///
/// The guard that matters is not the weakness test — it is that a **name dictionary has to confirm
/// the joined string**. Weakness only decides what is worth asking about; the dictionary decides.
/// That ordering is deliberate: a rule that joined tokens because they looked joinable would
/// invent compounds, and this one cannot produce a name that is not in a dictionary.
///
/// The pieces are kept in [`Token::components`], so nothing the analyzer said is lost and the
/// reader can still see how it read each character.
pub fn merge_names(tokens: &[Token], known: impl Fn(&str) -> Option<FoundName>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut at = 0;

    while at < tokens.len() {
        let mut joined_to = None;

        // Longest first, or 善福寺川 would be settled as 善福寺 the moment that also happened to
        // be a name — and it is.
        let longest = MAX_RUN.min(tokens.len() - at);
        for length in (2..=longest).rev() {
            let run = &tokens[at..at + length];
            if !eligible(run) {
                continue;
            }
            let surface: String = run.iter().map(|token| token.surface.as_str()).collect();
            if let Some(found) = known(&surface) {
                joined_to = Some((length, surface, found));
                break;
            }
        }

        match joined_to {
            Some((length, surface, found)) => {
                let run = &tokens[at..at + length];
                out.push(joined(run, surface, found));
                at += length;
            }
            None => {
                out.push(tokens[at].clone());
                at += 1;
            }
        }
    }

    mark_unverified(&mut out);
    out
}

/// Flag the runs that were left shattered, so the reader can say the reading is unconfirmed.
///
/// What survives [`merge_names`] as two or more adjacent single-kanji tokens is a compound the
/// segmenter did not know and no name dictionary could rejoin. Each reading is right for its own
/// character and the sequence is a guess.
///
/// The same fragment test the join uses, so the two cannot disagree about what a fragment is. A run
/// the dictionary could not confirm is exactly the set this should caveat.
///
/// A word the analyzer asserts — 208 and up — ends a run, because it is not a fragment of anything.
/// In practice Ichiran returns those as a single token anyway (山川 208, 白子 208), so this is
/// belt-and-braces rather than the main line of defence.
fn mark_unverified(tokens: &mut [Token]) {
    let joinable =
        |token: &Token| token.surface.chars().any(crate::kana::is_kanji) && fragment(token);

    let mut at = 0;
    while at < tokens.len() {
        let mut end = at;
        while end < tokens.len() && joinable(&tokens[end]) {
            end += 1;
        }
        if end - at >= 2 {
            for token in &mut tokens[at..end] {
                token.reading_unverified = true;
            }
        }
        // `end` is the first token that failed, so it cannot start a run either; `max` steps past
        // it and keeps this linear.
        at = end.max(at + 1);
    }
}

/// One token standing for a whole name.
fn joined(run: &[Token], surface: String, found: FoundName) -> Token {
    let (chosen, ambiguous) = chosen_reading(run, &found);
    // Every reading that was not chosen, kept where the reader can be shown them rather than
    // discarded. `form` carries the dictionary's own category for that reading — `surname`, `fem` —
    // because which name it is is exactly what distinguishes them.
    let alternatives: Vec<Alternative> = found
        .candidates
        .iter()
        .filter(|candidate| candidate.reading != chosen.reading)
        .map(|candidate| Alternative {
            lemma: surface.clone(),
            reading: candidate.reading.clone(),
            form: candidate.tags.join(", "),
            senses: Vec::new(),
        })
        .collect();
    Token {
        // **The run's own trailing break, not `None`.** A name rejoined at the end of a line
        // would otherwise swallow the sentence boundary that followed it.
        line_break: run.last().map_or(LineBreak::None, |token| token.line_break),
        dict_form: surface.clone(),
        surface,
        lemma_reading: chosen.reading.clone(),
        reading: chosen.reading,
        // The joined name is a new lexical unit; a constituent's role does not describe it.
        lexical_role: None,
        // Left for `lookup` to fill. Putting the name's own tags here would make them look like
        // senses, and the interface has a place for a name already — `WordView::name`.
        senses: Vec::new(),
        // A name is not inflected, and claiming a conjugation for one would put a grammar
        // explanation under a river.
        conjugation: None,
        // What the analyzer actually said, kept rather than discarded.
        components: run.to_vec(),
        // Decided later, from the source, by `analyzer::mark_selectable`.
        selectable: false,
        // A dictionary name join is not one of the grammar constructions.
        constructions: Vec::new(),
        alternatives,
        start: run.first().and_then(|token| token.start),
        end: run.last().and_then(|token| token.end),
        // **Deliberately `None`.** This reading did not come from the analyzer, so there is no
        // analyzer confidence to report, and inventing one would feed a fabricated number to
        // `analyzer_is_unsure`. The name still leads on the merged token, by the other route
        // `NameHit::leads` allows: there are no word senses for it to come before.
        confidence: None,
        // Romanisation belongs to the piece the analyzer romanised, not to a join it never saw.
        romaji: None,
        // A dictionary confirmed *a* reading, which is the point of the join — but where it
        // confirmed several and nothing chose between them, saying so is the honest claim. This
        // used to be flatly `false` on the grounds that a dictionary had spoken, and for 百鬼 the
        // dictionary had spoken nine times.
        reading_unverified: false,
        reading_ambiguous: ambiguous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(surface: &str, reading: &str, confidence: Option<i32>) -> Token {
        Token {
            surface: surface.to_string(),
            dict_form: surface.to_string(),
            reading: reading.to_string(),
            lemma_reading: reading.to_string(),
            confidence,
            ..Token::default()
        }
    }

    /// A name is never rejoined across a line break — not even a wrapped one.
    ///
    /// [`joined`] concatenates the run's surfaces and takes `first.start .. last.end`, so a run
    /// crossing a break yields a token whose `surface` is missing a newline that
    /// `text[start..end]` still has. That is the invariant `core/tests/analyzer_contract.rs`
    /// asserts for every token, and it is about bytes rather than meaning — which is why a `Wrap`
    /// is refused too, even though a name broken over two columns is genuinely one name.
    ///
    /// Latent before `line_break` existed: `merge_names` saw only a slice, and a slice has no
    /// gap in it to look at.
    #[test]
    fn a_name_is_not_rejoined_across_a_line_break() {
        let confirms = |surface: &str| {
            (surface == "善福寺川").then(|| FoundName::one("ぜんぷくじがわ", Vec::new()))
        };

        let together = vec![piece("善福", "", Some(1)), piece("寺川", "", Some(1))];
        assert_eq!(
            merge_names(&together, confirms).len(),
            1,
            "the two halves join when nothing separates them"
        );

        for separated in [LineBreak::Break, LineBreak::Wrap] {
            let mut run = together.clone();
            run[0].line_break = separated;
            assert_eq!(
                merge_names(&run, confirms).len(),
                2,
                "{separated:?} between them must stop the join"
            );
        }
    }

    /// A name is never rejoined across a gap in the source, whatever put the gap there.
    ///
    /// The line-break rule above covers one way a run can be discontiguous. **A space does the same
    /// damage and was not covered**: `原 俊介` reached `merge_names` as two tokens with an
    /// untokenized byte between them, and joined into `surface` = `原俊介` over a span whose source
    /// slice is `原 俊介`. Two of the corpus's own captures carry it, and the engine comparison
    /// found them by checking `text[start..end] == surface` after the display pipeline — which
    /// `analyzer_contract.rs` checks only on analyzer output, one stage earlier.
    #[test]
    fn a_name_is_not_rejoined_across_a_gap_in_the_source() {
        let confirms = |surface: &str| {
            (surface == "原俊介").then(|| FoundName::one("はらしゅんすけ", Vec::new()))
        };
        let grounded = |surface: &str, start: usize| {
            let mut token = piece(surface, "", Some(1));
            token.start = Some(start);
            token.end = Some(start + surface.len());
            token
        };

        // 原 at 0..3 and 俊介 immediately after it: one name, and it must still join.
        let touching = vec![grounded("原", 0), grounded("俊介", 3)];
        assert_eq!(
            merge_names(&touching, confirms).len(),
            1,
            "a genuinely contiguous name still joins"
        );

        // Anything at all between them, and the joined surface would no longer be its own slice.
        // The ideographic space is the one a Japanese source is most likely to use.
        for gap in [" ", "\u{3000}", "\t", "\n"] {
            let separated = vec![grounded("原", 0), grounded("俊介", 3 + gap.len())];
            assert_eq!(
                merge_names(&separated, confirms).len(),
                2,
                "{gap:?} between the pieces must stop the join"
            );
        }
    }

    /// Contiguity is required exactly where a span is claimed, and nowhere else.
    ///
    /// [`joined`] takes `first.start .. last.end`, so ungrounded pieces produce an ungrounded name
    /// that asserts nothing — there is no claim to verify. A name whose *ends* are grounded does
    /// claim a span, and an interior with no offsets makes that claim unverifiable rather than
    /// acceptable.
    #[test]
    fn contiguity_is_required_only_where_a_span_is_claimed() {
        let confirms = |surface: &str| {
            (surface == "原俊介").then(|| FoundName::one("はらしゅんすけ", Vec::new()))
        };

        let ungrounded = vec![piece("原", "", Some(1)), piece("俊介", "", Some(1))];
        assert_eq!(
            merge_names(&ungrounded, confirms).len(),
            1,
            "an ungrounded name asserts no span and needs no contiguity"
        );

        let mut first = piece("原", "", Some(1));
        first.start = Some(0);
        first.end = None;
        let mut second = piece("俊介", "", Some(1));
        second.start = None;
        second.end = Some(9);
        assert_eq!(
            merge_names(&[first, second], confirms).len(),
            2,
            "a claimed span with an unverifiable interior must not be joined"
        );
    }

    /// The reported case, with Ichiran's real scores.
    fn shattered_river() -> Vec<Token> {
        vec![
            piece("善", "ぜん", Some(21)),
            piece("福", "ふく", Some(20)),
            piece("寺", "てら", Some(24)),
            piece("川", "かわ", Some(23)),
        ]
    }

    /// Only 善福寺川 and 善福寺 are names, which is also true of the real dictionary.
    fn river_index(surface: &str) -> Option<FoundName> {
        match surface {
            "善福寺川" => Some(FoundName::one("ぜんぷくじがわ", vec!["place".into()])),
            "善福寺" => Some(FoundName::one("ぜんぷくじ", vec!["place".into()])),
            _ => None,
        }
    }

    #[test]
    fn a_shattered_place_name_is_put_back_together() {
        let merged = merge_names(&shattered_river(), river_index);

        assert_eq!(merged.len(), 1, "four characters became one name");
        assert_eq!(merged[0].surface, "善福寺川");
        assert_eq!(merged[0].reading, "ぜんぷくじがわ");
        assert_eq!(merged[0].confidence, None, "the analyzer did not say this");
    }

    /// **The reason the search runs longest-first.** 善福寺 is a name too — the temple — so a
    /// shortest-first or greedy-from-two search would settle on it and leave 川 stranded, which is
    /// a different place read a different way.
    #[test]
    fn the_longest_name_wins_over_a_shorter_one_inside_it() {
        let merged = merge_names(&shattered_river(), river_index);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].surface, "善福寺川", "not 善福寺 followed by 川");
    }

    /// What the analyzer said is kept, not thrown away.
    #[test]
    fn the_pieces_survive_as_components() {
        let merged = merge_names(&shattered_river(), river_index);
        let parts: Vec<&str> = merged[0]
            .components
            .iter()
            .map(|token| token.surface.as_str())
            .collect();
        assert_eq!(parts, ["善", "福", "寺", "川"]);
        assert_eq!(
            merged[0].components[0].reading, "ぜん",
            "including its reading"
        );
    }

    /// A word the analyzer is sure of is never joined to anything.
    ///
    /// 東京 scores 325 against the shattered characters' 20-24, and it is two characters, so it
    /// fails both halves of [`weak`]. Without this, 東京 + 都 would be a candidate wherever some
    /// dictionary files the join as a name.
    #[test]
    fn a_word_the_analyzer_is_sure_of_is_left_alone() {
        let tokens = vec![
            piece("東京", "とうきょう", Some(325)),
            piece("都", "と", Some(30)),
        ];
        let merged = merge_names(&tokens, |surface| {
            (surface == "東京都").then(|| FoundName::one("とうきょうと", vec!["place".into()]))
        });

        assert_eq!(merged.len(), 2, "a strong piece makes the run ineligible");
        assert_eq!(merged[0].surface, "東京");
    }

    /// Weakness alone never joins anything — the dictionary has to confirm it.
    #[test]
    fn weak_neighbours_that_spell_nothing_are_left_apart() {
        let tokens = vec![piece("水", "みず", Some(12)), piece("火", "ひ", Some(11))];
        let merged = merge_names(&tokens, |_| None);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].surface, "水");
    }

    /// The two rivers a single-character rule would have missed, with their measured scores.
    ///
    /// 神田川 shatters into 神(12) + 田川(33) and 多摩川 into 多摩(65) + 川(23). Both are in
    /// JMnedict; neither is made of single characters. This is why [`fragment`] tests the score and
    /// not the length.
    #[test]
    fn a_name_shattered_into_uneven_pieces_is_still_rejoined() {
        let rivers = |surface: &str| match surface {
            "神田川" => Some(FoundName::one("かんだがわ", vec!["surname".into()])),
            "多摩川" => Some(FoundName::one("たまがわ", vec!["place".into()])),
            _ => None,
        };

        let kanda = merge_names(
            &[
                piece("神", "かみ", Some(12)),
                piece("田川", "たがわ", Some(33)),
            ],
            rivers,
        );
        assert_eq!(kanda.len(), 1);
        assert_eq!(kanda[0].reading, "かんだがわ");

        let tama = merge_names(
            &[
                piece("多摩", "たま", Some(65)),
                piece("川", "かわ", Some(23)),
            ],
            rivers,
        );
        assert_eq!(tama.len(), 1);
        assert_eq!(tama[0].reading, "たまがわ");
    }

    /// A token with no score is treated as a fragment, which is what makes this work under Lindera.
    ///
    /// The opposite of `merge`'s rule for `NAME_WINS_BELOW`, and for a stated reason: there a
    /// missing number could not be allowed to *assert* that a name displaces a word, whereas here it
    /// only decides what to ask the dictionary about, and the dictionary answers.
    #[test]
    fn a_scoreless_token_is_asked_about_because_the_dictionary_decides() {
        let unscored = vec![
            piece("善福寺", "ぜんぷくじ", None),
            piece("川", "かわ", None),
        ];
        let merged = merge_names(&unscored, |surface| {
            (surface == "善福寺川").then(|| FoundName::one("ぜんぷくじがわ", vec!["place".into()]))
        });
        assert_eq!(
            merged.len(),
            1,
            "Lindera reports no scores and still gets the river"
        );
        assert_eq!(merged[0].reading, "ぜんぷくじがわ");

        // And the dictionary really is the one deciding: same tokens, nothing in the index.
        assert_eq!(merge_names(&unscored, |_| None).len(), 2);
    }

    /// **The hole the first version of this had.** Particles score 4 — far below
    /// [`NAME_WINS_BELOW`] — so a weakness test that consulted only the score called them weak, and
    /// a run of kana needed just one dictionary hit to become a name. JMnedict has 741,346 entries
    /// and short kana given names among them, so this was not hypothetical.
    #[test]
    fn a_run_of_kana_is_never_a_name_however_low_it_scores() {
        let tokens = vec![piece("に", "に", Some(4)), piece("は", "は", Some(4))];
        let merged = merge_names(&tokens, |surface| {
            (surface == "には").then(|| FoundName::one("には", vec!["place".into()]))
        });
        assert_eq!(merged.len(), 2, "neither end carries a kanji");
    }

    /// The same, for the shape that actually occurs: a particle against a real word.
    #[test]
    fn a_particle_beside_a_word_is_not_joined_to_it() {
        let tokens = vec![piece("は", "は", Some(4)), piece("愛", "あい", Some(9))];
        let merged = merge_names(&tokens, |surface| {
            (surface == "は愛").then(|| FoundName::one("はあい", vec!["fem".into()]))
        });
        assert_eq!(merged.len(), 2, "a run may not begin with kana");
    }

    /// Kana *inside* a name still joins, because there is no way around it.
    ///
    /// 井の頭 and 霞ヶ関 are the names this allows for. The ends carry kanji, so a particle can
    /// never open or close the run.
    #[test]
    fn kana_bridging_two_kanji_is_part_of_the_name() {
        let tokens = vec![
            piece("井", "い", Some(13)),
            piece("の", "の", Some(4)),
            piece("頭", "あたま", Some(15)),
        ];
        let merged = merge_names(&tokens, |surface| {
            (surface == "井の頭").then(|| FoundName::one("いのかしら", vec!["place".into()]))
        });
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].surface, "井の頭");
        assert_eq!(merged[0].reading, "いのかしら");
    }

    /// A name found in the middle of a sentence leaves its neighbours untouched.
    #[test]
    fn the_rest_of_the_sentence_is_unchanged() {
        let mut tokens = vec![piece("東京", "とうきょう", Some(325))];
        tokens.extend(shattered_river());
        tokens.push(piece("に", "に", Some(4)));

        let merged = merge_names(&tokens, river_index);
        let surfaces: Vec<&str> = merged.iter().map(|token| token.surface.as_str()).collect();
        assert_eq!(surfaces, ["東京", "善福寺川", "に"]);
    }

    /// The span is the run's, so a click still highlights the right characters.
    #[test]
    fn the_merged_token_spans_the_whole_run() {
        let mut tokens = shattered_river();
        for (index, token) in tokens.iter_mut().enumerate() {
            token.start = Some(index * 3);
            token.end = Some(index * 3 + 3);
        }
        let merged = merge_names(&tokens, river_index);
        assert_eq!(merged[0].start, Some(0));
        assert_eq!(merged[0].end, Some(12), "through the end of the last piece");
    }

    /// A run longer than [`MAX_RUN`] is not considered whole, and its head is still tried.
    #[test]
    fn the_run_length_is_bounded() {
        let tokens: Vec<Token> = "一二三四五六七"
            .chars()
            .map(|c| piece(&c.to_string(), "x", Some(5)))
            .collect();
        let asked = std::cell::RefCell::new(Vec::new());
        let merged = merge_names(&tokens, |surface| {
            asked.borrow_mut().push(surface.chars().count());
            None
        });
        assert_eq!(merged.len(), 7, "nothing was a name");
        assert!(
            asked.borrow().iter().all(|&length| length <= MAX_RUN),
            "never asked about a run longer than the cap"
        );
    }

    /// An empty token list is not a special case anywhere.
    #[test]
    fn nothing_in_nothing_out() {
        assert!(merge_names(&[], river_index).is_empty());
    }

    /// A run nothing could rejoin says so rather than presenting its guess as a reading.
    #[test]
    fn a_run_that_could_not_be_rejoined_is_marked_unverified() {
        let merged = merge_names(&shattered_river(), |_| None);

        assert_eq!(merged.len(), 4, "no name dictionary had it");
        assert!(
            merged.iter().all(|token| token.reading_unverified),
            "every piece of the run carries the caveat"
        );
    }

    /// A run that *was* rejoined carries no caveat — it has a source now.
    #[test]
    fn a_rejoined_name_is_not_marked() {
        let merged = merge_names(&shattered_river(), river_index);
        assert!(
            !merged[0].reading_unverified,
            "JMnedict confirmed this reading"
        );
    }

    /// One single-kanji word on its own is not a broken compound.
    #[test]
    fn a_lone_character_is_never_marked() {
        let tokens = vec![
            piece("川", "かわ", Some(23)),
            piece("を", "を", Some(4)),
            piece("渡る", "わたる", Some(50)),
        ];
        let merged = merge_names(&tokens, |_| None);
        assert!(
            merged.iter().all(|token| !token.reading_unverified),
            "川 beside a particle is just the word 川"
        );
    }

    /// A character the analyzer is sure of is not part of a broken compound.
    ///
    /// This is where the mark is stricter than the join: joining had the dictionary as a backstop,
    /// and nothing checks this. 山(300) beside 川(250) is two words.
    #[test]
    fn confident_neighbours_are_not_marked() {
        let tokens = vec![
            piece("山", "やま", Some(300)),
            piece("川", "かわ", Some(250)),
        ];
        let merged = merge_names(&tokens, |_| None);
        assert!(merged.iter().all(|token| !token.reading_unverified));
    }

    /// With no scores at all, structure alone decides — the Lindera case.
    #[test]
    fn a_scoreless_run_of_single_kanji_is_marked() {
        let tokens = vec![piece("善", "ぜん", None), piece("福", "ふく", None)];
        let merged = merge_names(&tokens, |_| None);
        assert!(merged.iter().all(|token| token.reading_unverified));
    }

    /// **A join that is also an ordinary word is left alone**, and this was caught by the live test
    /// rather than reasoned out in advance.
    ///
    /// 白 (12) + 子 (11) are two weak single kanji; JMnedict files 白子 as a place in Chiba; and
    /// Jitendex has 白子 spelled exactly that way meaning "soft roe". Joining it put a town where
    /// the roe was. `look_up_name` refuses the join by returning `None` for anything a dictionary
    /// has as a word, so the policy here never sees it — which is why this test drives the closure
    /// the way `look_up_name` behaves rather than the way JMnedict alone would.
    #[test]
    fn a_join_that_is_also_an_ordinary_word_is_not_made_a_name() {
        let tokens = vec![piece("白", "しろ", Some(12)), piece("子", "こ", Some(11))];

        // What `look_up_name` does for 白子: a word dictionary has it, so it reports nothing.
        let merged = merge_names(&tokens, |_| None);
        assert_eq!(
            merged.len(),
            2,
            "白子 is a word; the pieces stay as they were"
        );

        // And the shape that would have shipped the bug, for contrast.
        let if_only_jmnedict_were_asked = merge_names(&tokens, |surface| {
            (surface == "白子").then(|| FoundName::one("しらこ", vec!["place".into()]))
        });
        assert_eq!(
            if_only_jmnedict_were_asked.len(),
            1,
            "the policy joins whatever the closure calls a name — the guard belongs in the lookup"
        );
    }
}
