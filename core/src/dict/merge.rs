//! Merge policy: what one word looks like when several dictionaries have an opinion.
//!
//! The rules (`project notes` §6), and what each is protecting against:
//!
//! - **JMdict, via the analyzer, is primary and shows inline.** It is the source the
//!   rest of the app already agrees with — the reader's furigana and the grammar
//!   engine both came from the same analysis.
//! - **Other sources contribute only what the page has not already said.** Showing four
//!   dictionaries' worth of near-identical glosses is how a lookup becomes unreadable, and
//!   near-identical is the normal case: Jitendex is a JMdict derivative and the analyzer
//!   carries JMdict, so the two agree about almost every word a reader looks up. Senses
//!   already inline are dropped; a source with none left is not disclosed at all. See
//!   [`adds_anything_new`], and [`AlsoIn::senses`] for why a surviving sense is kept whole.
//! - **If the primary has nothing, promote the first source that does — and name it.**
//!   Names and slang are exactly the case JMdict misses, and an unattributed
//!   definition leaves the learner unable to judge it.
//! - **Examples attach to the word, not the source.** A learner wants the sentence,
//!   not a filing system.
//! - **A reading is adopted only when a source's entries agree on one.** A source
//!   whose entries disagree about how a word is read has not told us the reading, and
//!   guessing produces wrong furigana.
//! - **A name is its own answer, not another sense.** *Who is this* and *what does this
//!   mean* are different questions, and a reader looking at セイアちゃーん wants the
//!   first without losing the second. So a name-tagged hit never enters the sense list at
//!   all — see [`SourceResult::definitions`] — and arrives as a [`NameHit`] instead.
//!   [`analyzer_is_unsure`] decides when it leads; that one is subtler than it looks, and
//!   getting it wrong once made バー mean "Wah (Pakistan)".

use crate::kana::to_hiragana;
use crate::model::Token;

use super::index::{LookupHit, MatchKind};
use super::yomitan::Example as SourceExample;

/// The answer for a grouped construction, which no dictionary has a useful entry for.
///
/// **The dictionary's answers here are all wrong, and confidently so.** なん spelled as it is
/// reaches 何 — "what" — and the group's own lexical identity is だ, whose entries are the copula
/// in general. A reader who selects なん in 禁足地なんじゃなかったっけ and is told it means "what" has
/// been actively misled, which is worse than being told nothing.
///
/// So the pack answers instead, and the ordinary hits are **suppressed rather than ranked below**:
/// `also_in` would otherwise disclose the same wrong entries one click away, with the app's name
/// on them.
///
/// **Before the lookup, not after it.** Returning here first means the sources are never consulted
/// for a token whose answer cannot come from them, and — more importantly — that the answer is the
/// same on a machine with no dictionaries installed as on one with all of them. A construction
/// explained only when Jitendex happens to be present would be the worst of both.
///
/// `inline_source` names the pack, because [`WordView::inline_source`] exists precisely so a
/// promoted definition can be attributed, and this one is not the dictionary's.
///
/// **It has to read like a word, because it is standing where words stand.** The panel gives
/// a sense a chip row and a short line, and offers the word's examples under it; handed the
/// pattern's `explanation`, this filled that line with a paragraph of grammar prose, wore no
/// chips at all beside だ's `auxiliary` and `copula`, and showed no examples — while the
/// grammar card alongside printed that same paragraph a second time. Observed in the desktop
/// checkpoint on 2026-09-08 and reported as "not consistent with others visually", which it
/// was: three separate departures from the shape of every other entry.
///
/// So the pack supplies all three parts and the merge copies them across: `glosses` for the
/// line, `tags` for the chips, `examples` for the block beneath. **The prose stays in the
/// grammar panel**, which is the view that has room for it and the only one that had it
/// before this token could be selected at all.
fn construction(token: &Token) -> Option<WordView> {
    let annotation = crate::construction::resolved(token)
        .into_iter()
        .find(|annotation| annotation.lookup == crate::model::ConstructionLookup::GrammarPattern)?;
    let pattern = crate::grammar::pack::find(&annotation.grammar_pattern)?;
    let pack = crate::grammar::pack::all()
        .into_iter()
        .find(|pack| pack.get(&annotation.grammar_pattern).is_some())?;
    Some(WordView {
        headword: token.surface.clone(),
        reading: Some(token.reading.clone()),
        inline: vec![Sense {
            tags: pattern.tags.clone(),
            // The explanation is the fallback rather than the answer, and an entry reaching
            // it is a pack entry missing its glosses — caught by
            // `a_constructions_answer_reads_like_a_words` rather than shipped. Falling back
            // to *nothing* would be worse: an empty gloss list makes `WordView::is_empty`
            // true, and the token would report "no definition found" while the grammar panel
            // explains it, which is the split-answer failure that method's doc records.
            glosses: if pattern.glosses.is_empty() {
                vec![pattern.explanation.clone()]
            } else {
                pattern.glosses.clone()
            },
            sequence: None,
        }],
        inline_source: Some(pack.name.clone()),
        also_in: Vec::new(),
        // The pack's own sentences, which it has carried unused since it was written.
        inline_retrieved: Retrieved::Whole {
            query: token.surface.clone(),
        },
        examples: pattern
            .examples
            .iter()
            .map(|example| MergedExample {
                japanese: example.japanese.clone(),
                english: example.english.clone(),
                retrieved: Retrieved::Whole {
                    query: token.surface.clone(),
                },
            })
            .collect(),
        name: None,
        likely_name: false,
        romaji: None,
    })
}

/// Which query produced a source's hits, and what that query addressed.
///
/// **Recorded where the decision is taken**, in [`crate::lookup::sources`], and never inferred
/// afterwards from an entry's spelling or its glosses. Reconstruction is not equivalent: 神姫 and
/// 神 are different queries that can both return entries spelled 神, and an entry's spelling says
/// what *it* is, not what was asked for.
///
/// It is per source because that is where it varies. For 神姫 as Ichiran analyses it, Jitendex
/// answers through the component 神 while JMnedict answers the whole — in the same lookup, for the
/// same token.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Retrieved {
    /// Older cached responses did not carry retrieval provenance.
    #[default]
    Unknown,
    /// The token's own dictionary form answered this dictionary.
    Whole {
        /// What was asked — the token's dictionary form, or its surface when it has none.
        query: String,
    },
    /// The whole reached nothing here, so one of the token's components answered instead.
    ///
    /// **This is useful assistance, not a defect** — a joined word the dictionary does not have
    /// still lets the reader reach the part it does. What makes it misleading is presenting it
    /// unlabelled, as though it defined the whole. Carrying the component's own spelling is what
    /// lets a caller say "for 神: god; deity" rather than implying it of 神姫.
    Component {
        /// The component's dictionary form, as queried.
        query: String,
    },
}

impl Retrieved {
    /// What was actually asked of the dictionary.
    pub fn query(&self) -> Option<&str> {
        match self {
            Retrieved::Unknown => None,
            Retrieved::Whole { query } | Retrieved::Component { query } => Some(query),
        }
    }

    /// The component this answer describes, when it is not about the whole token.
    pub fn component(&self) -> Option<&str> {
        match self {
            Retrieved::Unknown | Retrieved::Whole { .. } => None,
            Retrieved::Component { query } => Some(query),
        }
    }
}

/// One dictionary's answer for a word.
#[derive(Debug, Clone)]
pub struct SourceResult {
    /// The dictionary's title, as its own `index.json` gives it.
    pub title: String,
    pub hits: Vec<LookupHit>,
    /// Whether these hits answer the whole token or one of its components, and under what query.
    pub retrieved: Retrieved,
}

impl SourceResult {
    /// The senses this source has — **names excluded**.
    ///
    /// A name-tagged hit is a [`NameHit`] and nothing else. Left in, it is a sense like any
    /// other: promotable into `inline` when the analyzer has nothing, and disclosable under
    /// "also in" beside itself — so ヒフミ printed *Hifumi* as the word's first definition,
    /// credited to JMnedict, and again as the name, while whatever real dictionary also had
    /// the word was demoted into the fold behind them.
    ///
    /// Decided **per hit**, not per source, which is the other half of it. A dictionary is
    /// not "a name dictionary" — an entry is a name entry. `tags.rs` records that `fem` and
    /// `masc` exist in JMdict too, so any imported dictionary can carry one, and a rule
    /// written over the source's title threw away all of that dictionary's real senses on
    /// the strength of a single entry.
    fn definitions(&self) -> Vec<Sense> {
        self.hits
            .iter()
            .filter(|hit| !super::tags::marks_a_name(&hit.entry.definition_tags))
            .filter(|hit| !is_reading_entry(&hit.entry))
            .flat_map(|hit| {
                // The sequence lives on the entry, so it is captured before the definitions are
                // flattened away from it.
                let sequence = (hit.entry.sequence != 0).then_some(hit.entry.sequence);
                hit.entry
                    .definitions
                    .iter()
                    .map(move |definition| (sequence, definition))
            })
            .filter(|(_, definition)| !definition.glosses.is_empty())
            .map(|(sequence, definition)| Sense {
                tags: definition.tags.clone(),
                glosses: definition.glosses.clone(),
                sequence,
            })
            .collect()
    }

    /// The reading this source agrees on, if it agrees.
    ///
    /// Both kinds of entry are consulted: an ordinary entry states its reading in the
    /// reading field, a name entry states it in the glossary. Excluding the latter
    /// would throw away the very disagreement this rule exists to detect — 田中 is
    /// read たなか, でんちゅう *and* ティエンジョン depending on which entry is asked,
    /// and a source that cannot settle on one has not told us the reading.
    ///
    /// In practice this rarely leaves a word bare: the analyzer's own reading takes
    /// priority, and it is only consulted when the analyzer had none.
    fn agreed_reading(&self) -> Option<String> {
        let mut readings = self
            .hits
            .iter()
            .filter_map(|hit| stated_reading(&hit.entry));
        let first = readings.next()?;
        if readings.all(|reading| reading == first) {
            Some(first)
        } else {
            None
        }
    }

    fn examples(&self) -> impl Iterator<Item = &SourceExample> {
        self.hits
            .iter()
            .flat_map(|hit| hit.entry.definitions.iter())
            .flat_map(|definition| definition.examples.iter())
    }
}

/// A sense, from whichever source it came from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(rename = "MergedSense"))]
pub struct Sense {
    pub tags: Vec<String>,
    pub glosses: Vec<String>,
    /// Which dictionary entry this sense came from, when the source states one.
    ///
    /// **The merge used to lose this, and losing it made the ordering unmeasurable.** Several
    /// entries' senses collapse into one `inline` list here, and `dict::context::RankedSense`
    /// indexes into that list — so "the ranker favoured sense 2" could never be turned back into
    /// "the ranker favoured 前【ぜん】", and the only ground truth available for sense choice is
    /// filed *by entry*: a Jitendex example sentence sits under a JMdict sequence.
    #[serde(default)]
    // A JMdict sequence is seven digits, so `i32` describes the range honestly and the wire keeps
    // the `number` the frontend has always read. `as`, not `type`: `#[ts(type = "number")]`
    // replaces the whole declaration and silently drops the `| null` the `Option` earns — the
    // same trap `model::Sense::sequence` records.
    #[cfg_attr(feature = "typescript", ts(as = "Option<i32>"))]
    pub sequence: Option<i64>,
}

/// What one other source *adds*, for the "also in …" disclosure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AlsoIn {
    pub title: String,
    /// What this source answered. Missing on historical cached responses.
    #[serde(default)]
    pub retrieved: Retrieved,
    /// **What this source adds, not everything it says.**
    ///
    /// Senses whose every gloss is already inline are dropped, because the disclosure is
    /// titled *also in* — it is framed as "what else", and reprinting the five senses the
    /// reader has just read does not represent the source more faithfully, it buries the one
    /// sense that would have.
    ///
    /// Whole senses only. See [`adds_anything_new`] for why a partly-new sense is kept
    /// intact rather than trimmed to its new glosses.
    pub senses: Vec<Sense>,
}

/// A name a word may be, from a dictionary that files names.
///
/// Kept apart from `inline` rather than merged into it, because the two answer different
/// questions — *what does this word mean* and *who is this* — and a reader looking at
/// セイアちゃーん needs the second without losing the first.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct NameHit {
    /// The name itself: "Seia".
    pub glosses: Vec<String>,
    /// The dictionary's own codes — `fem`, `surname`, `place`.
    ///
    /// Raw codes rather than prose, so the page can draw them with the same chip a sense's
    /// `n` or `adv` wears and explain them from the same tag glossary. A name is a kind of
    /// answer, not a different kind of thing, and it should not need its own vocabulary of
    /// labels to say so.
    pub tags: Vec<String>,
    /// Always named, for the same reason a promoted definition is.
    pub source: String,
    /// Whether the name answer belongs to the selected token or one of its components.
    #[serde(default)]
    pub retrieved: Retrieved,
    /// Whether this should be shown *before* the word senses.
    ///
    /// Two ways to earn it: the analyzer barely believed its own reading (see
    /// [`NAME_WINS_BELOW`]), or there are no senses for it to come before — a name that is
    /// the only answer found is the answer, and folding it away would hide the page's one
    /// piece of information.
    pub leads: bool,
}

/// Everything to show for one word.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WordView {
    pub headword: String,
    /// Adopted only from a source that agrees with itself; `None` when nothing does.
    pub reading: Option<String>,
    /// What shows without the learner having to open anything.
    pub inline: Vec<Sense>,
    /// The source of `inline` when it is **not** the primary — always named, because
    /// a promoted definition the learner cannot attribute is worse than none.
    pub inline_source: Option<String>,
    /// What the inline definitions answer. Kept separate from every other answer group because
    /// one dictionary may answer a component while another supplies a whole-token name.
    #[serde(default)]
    pub inline_retrieved: Retrieved,
    /// Quietly disclosed: the other sources that also have this word.
    pub also_in: Vec<AlsoIn>,
    /// Attached to the word, gathered from any source, in source order.
    pub examples: Vec<MergedExample>,
    /// A name this word may be. `None` when no name dictionary has it.
    #[serde(default)]
    pub name: Option<NameHit>,
    /// Nothing at all has this word and it is written wholly in katakana.
    ///
    /// In manga that is usually a name — the character the panel is about — and
    /// occasionally an effect word. Worth saying, but only as a guess: the page must not
    /// assert a meaning here, because there is none to assert.
    #[serde(default)]
    pub likely_name: bool,
    /// How to say it, when the analyzer romanised it. The only useful thing left when no
    /// dictionary knows the word at all.
    #[serde(default)]
    pub romaji: Option<String>,
}

/// One example together with the query scope of the answer that supplied it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(rename = "DictExample"))]
pub struct MergedExample {
    pub japanese: String,
    pub english: String,
    #[serde(default)]
    pub retrieved: Retrieved,
}

impl WordView {
    /// Whether there is nothing at all to show.
    ///
    /// **Every field that can carry an answer, not only the two that once could.** This is
    /// what `search_words` drops a result on, so a field left out of it is a word the
    /// reference page reports as absent while the analysis panel explains it — which is what
    /// happened to シロコ: "No dictionary has this spelling — likely a name, read shiroko" in
    /// one place and "no results" in the other, for the same word at the same moment.
    ///
    /// A name is an answer, and so is knowing that nothing has the spelling.
    pub fn is_empty(&self) -> bool {
        self.inline.is_empty()
            && self.also_in.is_empty()
            && self.name.is_none()
            && !self.likely_name
    }
}

/// Merge the analyzer's own senses with what the dictionaries say.
///
/// `sources` is consulted in order, so the caller's ordering is the promotion
/// priority — put the dictionary you trust most first.
/// Put the entries whose reading matches the token's ahead of the ones that do not.
///
/// **The ordering key runs out exactly where it is needed.** `index.rs` records that hits come
/// back by `Entry::score` and that the score saturates — 200 for 22,553 entries — so for the
/// common words a reader actually meets there is nothing left to sort by, and 前 offers 前【ぜん】
/// and 前【まえ】 in whatever order the index happens to hold them.
///
/// The analyzer has already decided how the token reads, and measured against KWDLC gold it is
/// right 98.3% of the time. 前【まえ】 and 前【ぜん】 are different sequences with different
/// readings, so that answer settles the question the score cannot.
///
/// Measured before it was written, over the 21,311 Jitendex example sentences whose filed entry
/// says which one was meant (`core/tests/sense_selection.rs`). On the 1,006 contested lookups
/// where the reading distinguishes the candidates at all, the present order gets 83.8% and this
/// gets **87.3%**. Frequency was tried first and is a fifth as good, which is why this is a
/// reading test and not a jpdb one.
///
/// **A reorder, never a filter.** Everything the dictionary returned is still returned, in the
/// same relative order within each group — the analyzer's reading is right 98.3% of the time and
/// dropping the other entries would strand the reader on the other 1.7%. It also does nothing at
/// all when the reading is unknown, or when every candidate shares it: in both cases there is no
/// evidence to act on and the dictionary's own order stands.
pub fn prefer_matching_reading(hits: &mut [LookupHit], reading: &str) {
    if reading.is_empty() || hits.len() < 2 {
        return;
    }
    let wanted = to_hiragana(reading);
    // Stable, so entries that tie keep the order the dictionary gave them.
    hits.sort_by_key(|hit| to_hiragana(&hit.entry.reading) != wanted);
}

pub fn merge(token: &Token, sources: &[SourceResult]) -> WordView {
    // **A construction is answered by the grammar pack, not by the dictionary**, and it is
    // answered the same way whether or not one is installed. See [`construction`].
    if let Some(view) = construction(token) {
        return view;
    }
    let primary: Vec<Sense> = token
        .senses
        .iter()
        .filter(|sense| !sense.gloss.trim().is_empty())
        .map(|sense| Sense {
            // The analyzer writes its codes as "[v5m,vt]"; keep them as codes, and
            // let `TagGlossary` turn them into prose at the point of display.
            tags: split_pos(&sense.pos),
            // "to spread; to travel; to circulate" is one sense said several ways.
            glosses: sense
                .gloss
                .split(';')
                .map(str::trim)
                .filter(|gloss| !gloss.is_empty())
                .map(ToString::to_string)
                .collect(),
            // The analyzer's own sequence, when it has one. Ichiran states it; Lindera has no
            // dictionary and so no opinion, and its senses are empty anyway.
            sequence: sense.sequence,
        })
        .filter(|sense| !sense.glosses.is_empty())
        .collect();

    // Sources that actually have something to say about the word.
    let mut contentful: Vec<(&SourceResult, Vec<Sense>)> = sources
        .iter()
        .map(|source| (source, source.definitions()))
        .filter(|(_, senses)| !senses.is_empty())
        .collect();
    // A dictionary answering the selected word outranks a different dictionary answering only
    // a component. The stable sort retains configured dictionary order within either scope.
    contentful.sort_by_key(|(source, _)| !matches!(source.retrieved, Retrieved::Whole { .. }));

    let whole = Retrieved::Whole {
        query: if token.dict_form.is_empty() {
            token.surface.clone()
        } else {
            token.dict_form.clone()
        },
    };
    let (inline, inline_source, inline_retrieved, disclosed) = if !primary.is_empty() {
        // The analyzer has the word: it is primary, and every source is a disclosure.
        (primary, None, whole, contentful.as_slice())
    } else if let Some(((promoted, senses), rest)) = contentful.split_first() {
        // The primary has nothing — a name, or slang. Promote and name the first
        // source that does have it.
        (
            senses.clone(),
            Some(promoted.title.clone()),
            promoted.retrieved.clone(),
            rest,
        )
    } else {
        (Vec::new(), None, Retrieved::Unknown, [].as_slice())
    };

    // Compared once rather than per source.
    let inline_for_compare: Vec<Vec<String>> = inline.iter().map(said).collect();
    let already = already_said(&inline_for_compare);

    // A name leads when the analyzer barely believed its own answer — or when it is the
    // only answer there is. The second case is not a judgement call: with no senses above
    // it, a name folded behind a "1 name reading" disclosure is the page hiding the one
    // thing it found. It is the ordinary case under Lindera, which has no senses of its own
    // for anything.
    let unsure = analyzer_is_unsure(token, sources);
    let name = name_hit(sources, unsure || inline.is_empty());

    WordView {
        headword: if token.dict_form.is_empty() {
            token.surface.clone()
        } else {
            token.dict_form.clone()
        },
        reading: adopt_reading(token, sources),
        inline,
        inline_source,
        inline_retrieved,
        also_in: disclosed
            .iter()
            // Only what this source *adds*. 水 is the case: Jitendex answers it with six
            // senses, five of them the five printed directly above, and the reader had to
            // read all five again to find "Wednesday". The old rule decided per source and
            // then printed every sense that source had — the right question, asked at the
            // wrong level.
            //
            // Nothing filters names out here any more, and nothing needs to: they never
            // became senses in the first place. See [`SourceResult::definitions`].
            .filter_map(|(source, senses)| {
                let adds: Vec<Sense> = senses
                    .iter()
                    .filter(|sense| adds_anything_new(sense, &already))
                    .cloned()
                    .collect();
                (!adds.is_empty()).then(|| AlsoIn {
                    title: source.title.clone(),
                    retrieved: source.retrieved.clone(),
                    senses: adds,
                })
            })
            .collect(),
        examples: gather_examples(sources),
        // Said when a katakana word is spelled the way *nothing* is spelled: either no
        // source has it at all, or every source that does was reached by folding its kana.
        //
        // シロコ is the second kind. Folding reaches 白子 ("soft roe"), so it is not
        // *unknown* — but nothing is spelled シロコ and no name dictionary has it. The gloss
        // stays on screen; this adds the caveat rather than replacing it, because both facts
        // are true and neither is certain.
        //
        // **Deliberately not gated on the score.** It was, and Lindera reports none — so the
        // one analyzer that never contributes senses of its own, and therefore leans hardest
        // on whatever a fold turns up, was the one analyzer that showed "soft roe" for シロコ
        // with no caveat beside it. `matched_as_written` is evidence both analyzers have, and
        // it is exactly what the sentence on screen claims: no dictionary has this spelling.
        // The score still decides `leads`, which is a stronger claim than this one.
        likely_name: crate::kana::is_katakana_word(&token.surface)
            && name.is_none()
            && !matched_as_written(sources),
        name,
        romaji: token.romaji.clone(),
    }
}

/// Below this, Ichiran does not believe its own answer.
///
/// Measured against the installed dictionaries, not guessed. Ichiran's per-word score for a
/// katakana name folded onto a rare homophone: ホシ 8, セイア 9, ユウカ 9, シロコ 9,
/// ヒフミ 9. For a word it is sure of: ネコ 32, ジョン 32, アカリ 36, さん 40, コーヒー 80,
/// サクラ 384, パソコン 840.
///
/// **Never sufficient on its own**, which cost a rewrite to learn: バー — an ordinary
/// loanword — scores 10, squarely inside the low band, and JMnedict *does* file バー, as
/// the place name "Wah (Pakistan)". A rule guarded only by "a name dictionary has it"
/// therefore told the reader that バー means Wah. [`analyzer_is_unsure`] is what actually
/// separates them.
pub(super) const NAME_WINS_BELOW: i32 = 16;

/// Whether the analyzer's answer is weak enough that a name should take the lead.
///
/// Two conditions, and the second is the one that matters:
///
/// 1. The analyzer scored this below [`NAME_WINS_BELOW`].
/// 2. **No source matched the word as it is actually written.**
///
/// バー *is* spelled バー — Jitendex matches it exactly — so however little Ichiran thinks
/// of it, the word is the word and nothing may displace it. セイア is spelled no such way:
/// every match for it is a kana fold onto something else (井蛙, read せいあ), which is
/// exactly what a name written in katakana looks like from a word dictionary's side.
///
/// Name entries are excluded from the check, or JMnedict matching セイア would count as
/// the word being found and defeat the rule it exists to trigger.
fn analyzer_is_unsure(token: &Token, sources: &[SourceResult]) -> bool {
    let low = token
        .confidence
        .is_some_and(|score| score < NAME_WINS_BELOW);
    low && !matched_as_written(sources)
}

/// Whether any source has this word under the spelling or reading it was asked about.
fn matched_as_written(sources: &[SourceResult]) -> bool {
    sources.iter().any(|source| {
        source.hits.iter().any(|hit| {
            hit.matched != MatchKind::Folded
                && !super::tags::marks_a_name(&hit.entry.definition_tags)
        })
    })
}

/// The first source that files this word as a name.
///
/// Sources are consulted in the caller's priority order, so a real dictionary gets to
/// answer first and a name dictionary only supplies what it alone knows.
fn name_hit(sources: &[SourceResult], unsure: bool) -> Option<NameHit> {
    sources.iter().find_map(|source| {
        let named: Vec<&LookupHit> = source
            .hits
            .iter()
            .filter(|hit| super::tags::marks_a_name(&hit.entry.definition_tags))
            .collect();

        let mut glosses: Vec<String> = named
            .iter()
            .flat_map(|hit| hit.entry.definitions.iter())
            .flat_map(|definition| definition.glosses.iter())
            .map(|gloss| gloss.trim().to_string())
            .filter(|gloss| !gloss.is_empty())
            .collect();
        // JMnedict files the same name under several entries — せいあ arrives twice.
        //
        // **`Vec::dedup` only removes *neighbours*.** It caught せいあ because those two
        // landed side by side, and missed 水's みず, which arrives at the eighth reading and
        // again at the fourteenth — printed twice in a list of sixteen. Sorting first would
        // fix that and cost the dictionary's own order, which is the order these are worth
        // reading in, so the set does the remembering instead.
        let mut seen = std::collections::HashSet::new();
        glosses.retain(|gloss| seen.insert(gloss.clone()));
        if glosses.is_empty() {
            return None;
        }

        // Deduped and in the dictionary's own order: an entry filed twice under `fem`
        // should not wear the chip twice.
        let mut tags: Vec<String> = Vec::new();
        for code in named
            .iter()
            .flat_map(|hit| hit.entry.definition_tags.iter())
            .filter(|code| super::tags::marks_a_name(std::slice::from_ref(code)))
        {
            if !tags.contains(code) {
                tags.push(code.clone());
            }
        }

        Some(NameHit {
            tags,
            glosses,
            source: source.title.clone(),
            retrieved: source.retrieved.clone(),
            leads: unsure,
        })
    })
}

/// What a sense actually puts on screen, normalised for comparison.
///
/// Glosses only, and for a concrete reason: this is exactly what both lists draw. The
/// disclosure renders `glosses.join("; ")` and so does the inline list, so two senses with
/// the same glosses print the same line twice.
///
/// Tags are deliberately not part of it. The analyzer emits JMdict's codes (`v1`, `vt`)
/// and Jitendex spells its own out (`1-dan`), so comparing them would make every source
/// differ from the primary and this would never hide anything.
fn said(sense: &Sense) -> Vec<String> {
    sense
        .glosses
        .iter()
        .map(|gloss| gloss.trim().to_lowercase())
        .collect()
}

/// Everything already on screen, flattened for comparison.
///
/// Compared **gloss by gloss, not sense by sense**. Asking whether a source's whole sense
/// matched an inline one exactly meant Jitendex answering セイア with `["frog in a well"]`
/// counted as new against an inline `["frog in a well", "someone with a narrow perspective"]`
/// that already contained it — a disclosure of a source saying nothing, on most words a
/// reader looks at.
fn already_said(inline: &[Vec<String>]) -> std::collections::HashSet<&String> {
    inline.iter().flatten().collect()
}

/// Whether one sense has anything the page has not said yet.
///
/// **Jitendex agreeing with the analyzer is the normal case, not the exception.** Both are
/// JMdict underneath — the analyzer carries it through Ichiran, and Jitendex is a derivative
/// of it — so a disclosure that reprinted every source reprinted the senses directly above it
/// for most words a reader looks at. A source saying *less* is dropped by the same rule: a
/// strict subset of what is already on screen has nothing to add either.
///
/// There used to be a second function asking this per source, and it is gone rather than
/// rewritten. Filtering the senses already answers it — a source with nothing left is a
/// source not worth opening — and two functions holding one rule is how they come to disagree.
///
/// **The sense is the unit, and every gloss of a surviving one is kept.** A sense is one
/// meaning said several ways, so dropping "liquid" out of `["fluid", "liquid", "juice"]`
/// because "liquid" appeared above would chop a sentence in half to save a line. Either the
/// sense has something to add and is shown whole, or it has not and is not shown.
fn adds_anything_new(sense: &Sense, already: &std::collections::HashSet<&String>) -> bool {
    said(sense).iter().any(|gloss| !already.contains(gloss))
}

/// The analyzer's reading wins when it has one — it is the reading the rest of the
/// app is already showing. Otherwise take the first source that agrees with itself.
fn adopt_reading(token: &Token, sources: &[SourceResult]) -> Option<String> {
    let analyzer = token.lemma_reading.trim();
    if !analyzer.is_empty() {
        return Some(analyzer.to_string());
    }
    sources.iter().find_map(SourceResult::agreed_reading)
}

/// Examples belong to the word. Duplicates are dropped, since several dictionaries
/// drawing on the same corpus will offer the same sentence.
fn gather_examples(sources: &[SourceResult]) -> Vec<MergedExample> {
    let mut examples: Vec<MergedExample> = Vec::new();
    for source in sources {
        for example in source.examples() {
            if example.japanese.trim().is_empty() {
                continue;
            }
            if let Some(kept) = examples
                .iter_mut()
                .find(|kept| kept.japanese == example.japanese)
            {
                // The same sentence can arrive through a component and then the whole word.
                // Keep one copy but credit the stronger query that actually found it too.
                if matches!(source.retrieved, Retrieved::Whole { .. })
                    && !matches!(kept.retrieved, Retrieved::Whole { .. })
                {
                    kept.english = example.english.clone();
                    kept.retrieved = source.retrieved.clone();
                }
            } else {
                examples.push(MergedExample {
                    japanese: example.japanese.clone(),
                    english: example.english.clone(),
                    retrieved: source.retrieved.clone(),
                });
            }
        }
    }
    examples
}

/// The reading an entry states, wherever it states it.
///
/// An ordinary entry uses the reading field. A name entry from a dictionary that
/// leaves that field empty states it in the glossary instead — see [`is_reading_entry`].
pub(super) fn stated_reading(entry: &crate::dict::Entry) -> Option<String> {
    stated_readings(entry).into_iter().next()
}

/// **Every** reading an entry states, in the order it states them.
///
/// [`stated_reading`] takes the first and is right to, for the callers that want one answer. This
/// exists because the name pass wants the evidence rather than a verdict: JMnedict files 百鬼 with
/// six kana glosses on one entry — おきそ, きなり, なきり, ひゃくき, もぎ, ももき — and taking the
/// first discarded five of them before anything could compare them against what the analyzer had
/// already read. See `dict::names::look_up_name`.
pub(super) fn stated_readings(entry: &crate::dict::Entry) -> Vec<String> {
    if is_reading_entry(entry) {
        return entry
            .definitions
            .iter()
            .flat_map(|definition| definition.glosses.iter())
            .filter(|gloss| is_kana_only(gloss))
            .cloned()
            .collect();
    }
    Vec::from_iter(Some(entry.reading.clone()).filter(|reading| !reading.is_empty()))
}

/// Whether an entry's "definitions" are really its readings.
///
/// JMnedict-in-Yomitan encodes most name entries this way: it leaves the reading field
/// empty — so the parser fills it with the term — and puts the kana in the glossary.
/// Taken at face value, 田中 comes back as a word whose *meaning* is "たなか", which is
/// both wrong and unhelpful.
///
/// The test is deliberately narrow, because a Japanese-Japanese dictionary would have
/// perfectly good kana definitions: the reading must be **absent** (equal to the term,
/// which is how an empty field is stored), the term must contain something that is not
/// kana, and every gloss must be kana. A real definition fails at least one of those.
fn is_reading_entry(entry: &crate::dict::Entry) -> bool {
    if entry.reading != entry.term || is_kana_only(&entry.term) {
        return false;
    }
    let mut glosses = entry
        .definitions
        .iter()
        .flat_map(|definition| definition.glosses.iter())
        .peekable();
    glosses.peek().is_some() && glosses.all(|gloss| is_kana_only(gloss))
}

/// Whether a string is nothing but kana — the long-vowel mark and spaces included,
/// since a reading may be written コーヒー or with separators.
fn is_kana_only(text: &str) -> bool {
    let mut any = false;
    for character in text.chars() {
        if character.is_whitespace() || matches!(character, 'ー' | '・' | '、') {
            continue;
        }
        if !crate::kana::is_kana(character) {
            return false;
        }
        any = true;
    }
    any
}

fn split_pos(pos: &str) -> Vec<String> {
    pos.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split([',', ' '])
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::index::MatchKind;
    use crate::dict::yomitan::{Definition, Entry};
    use crate::model::Sense as ModelSense;

    fn token(dict_form: &str, reading: &str, senses: &[(&str, &str)]) -> Token {
        Token {
            surface: dict_form.into(),
            dict_form: dict_form.into(),
            lemma_reading: reading.into(),
            senses: senses
                .iter()
                .map(|(pos, gloss)| ModelSense {
                    pos: (*pos).into(),
                    gloss: (*gloss).into(),
                    sequence: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    /// The reading decides where the score has nothing left to say.
    ///
    /// The third case is the one that keeps this honest: an entry whose reading does not match is
    /// moved down, never dropped. The analyzer is right about the reading 98.3% of the time, and
    /// filtering on it would strand the reader on the rest.
    #[test]
    fn a_matching_reading_leads_and_nothing_is_discarded() {
        let hits = |pairs: &[(&str, &str)]| -> Vec<LookupHit> {
            pairs
                .iter()
                .map(|(term, reading)| source("Jitendex", term, reading, &["gloss"]).hits.remove(0))
                .collect()
        };

        // 前 is まえ and ぜん; the score cannot separate them, the reading can.
        let mut found = hits(&[("前", "ぜん"), ("前", "まえ")]);
        prefer_matching_reading(&mut found, "まえ");
        assert_eq!(found[0].entry.reading, "まえ");
        assert_eq!(found.len(), 2, "the other entry is reordered, not removed");

        // Katakana and hiragana are the same reading written two ways.
        let mut found = hits(&[("前", "ぜん"), ("前", "まえ")]);
        prefer_matching_reading(&mut found, "マエ");
        assert_eq!(found[0].entry.reading, "まえ");

        // No reading, or a reading nothing matches: the dictionary's own order stands.
        let mut found = hits(&[("前", "ぜん"), ("前", "まえ")]);
        prefer_matching_reading(&mut found, "");
        assert_eq!(found[0].entry.reading, "ぜん");
        let mut found = hits(&[("前", "ぜん"), ("前", "まえ")]);
        prefer_matching_reading(&mut found, "ちがう");
        assert_eq!(found[0].entry.reading, "ぜん");
    }

    /// The な + ん group is answered by the pack, and the dictionary is not consulted for it.
    ///
    /// **The conflicting source is the point of the test.** A real install has entries reachable
    /// from this token — 何 for the spelling, the copula for the identity — and both are wrong
    /// here in a way a learner cannot detect. So they must not appear as the meaning *or* as a
    /// disclosure: `also_in` one click away would be the same wrong answer with the app's name on
    /// it.
    ///
    /// The same answer must come back with no dictionaries installed at all, which is the case
    /// the second half checks. A construction explained only when Jitendex happens to be present
    /// is a worse promise than not explaining it.
    #[test]
    fn a_construction_is_answered_by_the_pack_and_not_by_the_dictionary() {
        let group = Token {
            surface: "なん".into(),
            dict_form: "だ".into(),
            reading: "なん".into(),
            lemma_reading: "だ".into(),
            start: Some(0),
            end: Some(6),
            components: vec![
                Token {
                    surface: "な".into(),
                    dict_form: "だ".into(),
                    start: Some(0),
                    end: Some(3),
                    ..Default::default()
                },
                Token {
                    surface: "ん".into(),
                    dict_form: "ん".into(),
                    lemma_reading: "の".into(),
                    start: Some(3),
                    end: Some(6),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert!(
            crate::model::is_explanatory_nan(&group),
            "the fixture is the shape"
        );

        let entry = crate::grammar::pack::core()
            .get(crate::grammar::EXPLANATORY_NAN)
            .expect("the pack entry ships");

        // Dictionaries that would answer, and answer wrongly.
        let conflicting = [
            source("Jitendex", "何", "なん", &["what", "how many"]),
            source("Other", "だ", "だ", &["be; is"]),
        ];
        for sources in [&conflicting[..], &[]] {
            let view = merge(&group, sources);
            assert_eq!(view.headword, "なん", "shown as it is written");
            assert_eq!(view.reading.as_deref(), Some("なん"));
            assert_eq!(
                view.inline
                    .iter()
                    .map(|sense| sense.glosses.clone())
                    .collect::<Vec<_>>(),
                vec![entry.glosses.clone()],
                "the pack's glosses are the answer"
            );
            assert_eq!(
                view.inline_source.as_deref(),
                Some("core"),
                "attributed to the pack, because it is not the dictionary's"
            );
            assert!(
                view.also_in.is_empty(),
                "wrong entries are suppressed, not demoted"
            );
            assert!(view.name.is_none() && !view.likely_name);
        }

        // A token that is not the construction is untouched by any of this.
        let pronoun = token("何", "なん", &[("pn", "what")]);
        let ordinary = merge(&pronoun, &conflicting);
        assert_ne!(
            ordinary.inline_source.as_deref(),
            Some("core"),
            "an ordinary word is still the dictionary's to answer"
        );
    }

    /// A construction's answer wears the same shape as a word's.
    ///
    /// **Written from an observation, not from taste.** In the desktop checkpoint of
    /// 2026-09-08 なん was selected beside だ, and the difference was immediate: だ carried
    /// `auxiliary` and `copula` chips over "be; is" and a block of examples, while なん
    /// carried one numbered line holding a paragraph of grammar prose, no chips and no
    /// examples — and the grammar card beside it printed that same paragraph again.
    ///
    /// So this asserts the three properties that were violated, over the shipped pack rather
    /// than over a fixture: chips exist, the line is a line, and the sentences the pack
    /// already carries reach the panel. The gloss-length bound is what keeps a paragraph out
    /// of the slot — it is not a style rule, it is the difference between the sense list and
    /// the grammar view having separate jobs.
    #[test]
    fn a_constructions_answer_reads_like_a_words() {
        let group = Token {
            surface: "なん".into(),
            dict_form: "だ".into(),
            reading: "なん".into(),
            lemma_reading: "だ".into(),
            start: Some(0),
            end: Some(6),
            components: vec![
                Token {
                    surface: "な".into(),
                    dict_form: "だ".into(),
                    start: Some(0),
                    end: Some(3),
                    ..Default::default()
                },
                Token {
                    surface: "ん".into(),
                    dict_form: "ん".into(),
                    lemma_reading: "の".into(),
                    start: Some(3),
                    end: Some(6),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let view = merge(&group, &[]);
        let sense = &view.inline[0];

        assert!(
            !sense.tags.is_empty(),
            "a construction with no tags gets no chip row, and reads as a different kind of \
             object beside every entry that has one"
        );
        for code in &sense.tags {
            // The chips are drawn from the compiled-in glossary, which is what makes them work
            // on an install with no dictionaries — the condition this whole answer exists to
            // hold. An unresolvable code renders as a dead monospace pill instead.
            assert!(
                crate::terms::get(code).is_some(),
                "the tag {code:?} has no glossary entry, so its chip would be a dead code"
            );
        }

        // One line, in the slot a dictionary fills with "be; is". The explanation is 180-odd
        // characters and was what used to sit here.
        for gloss in &sense.glosses {
            let length = gloss.chars().count();
            assert!(
                length <= 60,
                "the gloss is {length} characters: {gloss:?}. The sense list holds a line; the \
                 paragraph belongs to the grammar panel, which already shows it."
            );
        }

        let pattern = crate::grammar::pack::core()
            .get(crate::grammar::EXPLANATORY_NAN)
            .expect("the pack entry ships");
        assert!(
            !sense.glosses.contains(&pattern.explanation),
            "showing the explanation here repeats verbatim what the grammar card beside it \
             already says"
        );
        assert_eq!(
            view.examples
                .iter()
                .map(|example| example.japanese.as_str())
                .collect::<Vec<_>>(),
            pattern
                .examples
                .iter()
                .map(|example| example.japanese.as_str())
                .collect::<Vec<_>>(),
            "the pack's sentences reach the panel"
        );
        assert!(
            !view.examples.is_empty(),
            "a construction with no examples is one a reader cannot see used"
        );
    }

    /// Most fixtures here answer the whole token. Component retrieval is set explicitly only in
    /// tests of merge's responsibility: preserving evidence it was handed, never inventing it.
    fn whole() -> Retrieved {
        whole_for("")
    }

    fn whole_for(query: &str) -> Retrieved {
        Retrieved::Whole {
            query: query.to_string(),
        }
    }

    fn source(title: &str, term: &str, reading: &str, glosses: &[&str]) -> SourceResult {
        SourceResult {
            retrieved: whole(),
            title: title.into(),
            hits: vec![LookupHit {
                entry: Entry {
                    term: term.into(),
                    reading: reading.into(),
                    definitions: vec![Definition {
                        glosses: glosses.iter().map(|g| (*g).to_string()).collect(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                matched: MatchKind::Spelling,
            }],
        }
    }

    /// A source with more than one sense, which `source` cannot build — it makes exactly
    /// one definition, and the disclosure rule is about choosing *between* senses.
    fn senses_source(title: &str, term: &str, reading: &str, senses: &[&[&str]]) -> SourceResult {
        let mut result = source(title, term, reading, &[]);
        result.hits[0].entry.definitions = senses
            .iter()
            .map(|glosses| Definition {
                glosses: glosses.iter().map(|g| (*g).to_string()).collect(),
                ..Default::default()
            })
            .collect();
        result
    }

    /// A source shaped the way JMnedict is: name codes in `definition_tags`.
    fn name_source(title: &str, term: &str, codes: &[&str], glosses: &[&str]) -> SourceResult {
        SourceResult {
            retrieved: whole(),
            title: title.into(),
            hits: vec![LookupHit {
                entry: Entry {
                    term: term.into(),
                    reading: term.into(),
                    definition_tags: codes.iter().map(|c| (*c).to_string()).collect(),
                    definitions: vec![Definition {
                        glosses: glosses.iter().map(|g| (*g).to_string()).collect(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                matched: MatchKind::Folded,
            }],
        }
    }

    /// A word reached only by folding katakana onto hiragana — nothing is spelled this way.
    fn folded_source(title: &str, term: &str, reading: &str, glosses: &[&str]) -> SourceResult {
        let mut result = source(title, term, reading, glosses);
        result.hits[0].matched = MatchKind::Folded;
        result
    }

    /// The セイア case, end to end: Ichiran folded a katakana name onto a rare homophone
    /// and barely believed it, and JMnedict has the name.
    #[test]
    fn a_name_leads_when_the_analyzer_barely_believes_its_own_answer() {
        let mut token = token("セイア", "せいあ", &[("[n]", "frog in a well")]);
        token.confidence = Some(9);

        let sources = [
            // 井蛙 is reached only by folding セイア to せいあ — see `analyzer_is_unsure`.
            folded_source("Jitendex", "井蛙", "せいあ", &["frog in a well"]),
            name_source("JMnedict", "せいあ", &["fem"], &["Seia"]),
        ];
        let view = merge(&token, &sources);

        let name = view.name.expect("JMnedict has the name");
        assert!(name.leads, "score 9 is below the threshold");
        assert_eq!(name.glosses, ["Seia"]);
        // The dictionary's own code, so the page can chip it like a sense's `n`.
        assert_eq!(name.tags, ["fem"]);
        assert_eq!(name.source, "JMnedict");

        // The word sense is not destroyed — it is still reachable, just no longer the
        // only thing the reader is told. (Jitendex is not disclosed separately because it
        // repeats the analyzer verbatim, which is the ordinary case for two things that
        // are JMdict underneath.)
        assert_eq!(view.inline[0].glosses, ["frog in a well"]);
        assert!(
            view.also_in.iter().all(|also| also.title != "JMnedict"),
            "a name dictionary holds only names — disclosing it would print Seia twice"
        );
    }

    /// The name source is kept out of `also_in` even when it *would* otherwise qualify.
    /// 水's real JMnedict entry: sixteen readings, with みず arriving twice — eighth and
    /// fourteenth, six apart. `Vec::dedup` only looks at neighbours, so it printed both.
    #[test]
    fn a_repeated_name_reading_is_dropped_however_far_apart_it_lands() {
        let token = token("水", "みず", &[("[n]", "water (esp. cool or cold)")]);
        let sources = [
            source("Jitendex", "水", "みず", &["water (esp. cool or cold)"]),
            name_source(
                "JMnedict",
                "みず",
                &["unclass", "fem", "surname"],
                &[
                    "みずあらい",
                    "みずつぎ",
                    "みずはい",
                    "あおい",
                    "あくあ",
                    "いお",
                    "すい",
                    "みず",
                    "みずき",
                    "みな",
                    "みなと",
                    "みなも",
                    "よしみ",
                    // Again, and not beside its twin — which is the whole point.
                    "みず",
                    "みずさき",
                    "みずざき",
                    "みずたか",
                ],
            ),
        ];

        let name = merge(&token, &sources).name.expect("JMnedict has this one");
        let repeats = name.glosses.iter().filter(|gloss| *gloss == "みず").count();
        assert_eq!(
            repeats, 1,
            "みず appears once, not once per entry filed under it"
        );
        assert_eq!(name.glosses.len(), 16, "and nothing else was lost with it");
        // The dictionary's own order is what these are worth reading in, so it is kept
        // rather than sorted away.
        assert_eq!(name.glosses.first().map(String::as_str), Some("みずあらい"));
        assert_eq!(name.glosses.last().map(String::as_str), Some("みずたか"));
    }

    #[test]
    fn a_name_dictionary_is_never_also_disclosed_as_a_definition() {
        let mut token = token("セイア", "せいあ", &[("[n]", "frog in a well")]);
        token.confidence = Some(9);

        let sources = [
            // A distinct gloss, so the dedup rule cannot be what hides it.
            folded_source("Jitendex", "井蛙", "せいあ", &["a narrow perspective"]),
            name_source("JMnedict", "せいあ", &["fem"], &["Seia"]),
        ];
        let view = merge(&token, &sources);

        assert_eq!(
            view.also_in
                .iter()
                .map(|a| a.title.as_str())
                .collect::<Vec<_>>(),
            ["Jitendex"],
            "Jitendex discloses because it adds something; JMnedict never does"
        );
    }

    /// アカリ: 明かり is a real word *and* Akari is a real name, and Ichiran is sure of the
    /// word. Offering both without reordering is the honest answer.
    #[test]
    fn a_name_is_offered_but_does_not_lead_when_the_word_is_a_real_one() {
        let mut token = token("アカリ", "あかり", &[("[n]", "light; glow")]);
        token.confidence = Some(36);

        let sources = [name_source("JMnedict", "あかり", &["fem"], &["Akari"])];
        let view = merge(&token, &sources);

        let name = view.name.expect("the name is still offered");
        assert!(!name.leads, "36 is a word Ichiran is sure of");
        assert_eq!(view.inline[0].glosses, ["light", "glow"]);
    }

    /// **The regression this rule is most likely to cause, and it is not hypothetical.**
    ///
    /// バー is an ordinary loanword Ichiran scores 10 — inside the low band — and JMnedict
    /// really does file バー, as the place name "Wah (Pakistan)". An earlier version of this
    /// rule was guarded only by "a name dictionary has it", and duly told the reader that
    /// バー means Wah. What saves it is that バー *is spelled* バー: Jitendex matches it
    /// exactly, so the word is the word regardless of the score.
    #[test]
    fn an_exact_match_is_never_displaced_however_low_the_score() {
        let mut token = token("バー", "ばー", &[("[n]", "bar; tavern")]);
        token.confidence = Some(10);

        let sources = [
            source("Jitendex", "バー", "バー", &["bar", "tavern"]),
            name_source("JMnedict", "バー", &["place"], &["Wah (Pakistan)"]),
        ];
        let view = merge(&token, &sources);

        let name = view.name.expect("JMnedict does have it");
        assert!(
            !name.leads,
            "バー is spelled バー — Wah may not take the lead"
        );
        assert!(
            !view.likely_name,
            "it is not unknown; Jitendex has it exactly"
        );
        assert_eq!(view.inline[0].glosses, ["bar", "tavern"]);
    }

    /// シロコ: folding reaches 白子 ("soft roe"), so it is not unknown — but nothing is
    /// spelled シロコ, Ichiran scored it 9, and no name dictionary has it. The gloss stays
    /// and the caveat is added beside it.
    #[test]
    fn a_folded_junk_match_is_flagged_even_though_something_was_found() {
        let mut token = token("シロコ", "しろこ", &[("[n]", "soft roe")]);
        token.confidence = Some(9);

        let sources = [SourceResult {
            retrieved: whole(),
            title: "Jitendex".into(),
            hits: vec![LookupHit {
                entry: Entry {
                    term: "白子".into(),
                    reading: "しろこ".into(),
                    definitions: vec![Definition {
                        glosses: vec!["soft roe".into()],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                // Nothing is spelled シロコ — this was only reached by folding.
                matched: MatchKind::Folded,
            }],
        }];
        let view = merge(&token, &sources);

        assert!(view.likely_name, "nothing is actually spelled シロコ");
        assert!(view.name.is_none(), "and no name dictionary has it either");
        assert_eq!(
            view.inline[0].glosses,
            ["soft roe"],
            "the gloss is not destroyed"
        );
    }

    /// Lindera reports no score at all. A missing number is not a low one.
    #[test]
    fn a_name_never_leads_without_a_score_to_justify_it() {
        let token = token("セイア", "せいあ", &[("[n]", "frog in a well")]);
        assert_eq!(token.confidence, None);

        let sources = [name_source("JMnedict", "せいあ", &["fem"], &["Seia"])];
        let view = merge(&token, &sources);

        let name = view.name.expect("still offered");
        assert!(!name.leads);
    }

    /// シロコ: no dictionary anywhere has it.
    #[test]
    fn an_unknown_katakana_word_is_called_a_likely_name() {
        let mut token = Token {
            surface: "シロコ".into(),
            dict_form: "シロコ".into(),
            romaji: Some("shiroko".into()),
            ..Default::default()
        };
        token.confidence = Some(9);

        let view = merge(&token, &[]);
        assert!(view.likely_name);
        assert_eq!(view.romaji.as_deref(), Some("shiroko"));
        assert!(view.inline.is_empty(), "nothing may be asserted about it");

        // Kanji and kana words get no such guess — the signal is katakana specifically.
        let kanji = Token {
            surface: "凪".into(),
            dict_form: "凪".into(),
            ..Default::default()
        };
        assert!(!merge(&kanji, &[]).likely_name);
    }

    #[test]
    fn a_word_a_dictionary_knows_is_never_called_an_unknown_name() {
        let token = Token {
            surface: "コーヒー".into(),
            dict_form: "コーヒー".into(),
            ..Default::default()
        };
        let sources = [source("Jitendex", "コーヒー", "コーヒー", &["coffee"])];
        assert!(!merge(&token, &sources).likely_name);
    }

    #[test]
    fn the_analyzer_is_primary_and_other_sources_are_disclosed() {
        let token = token("食べる", "たべる", &[("[v1,vt]", "to eat; to consume")]);
        // Both sources say something the primary does not. That matters: this test is about
        // the primary/disclosure *split*, and it used to give Jitendex only "to eat" — a
        // subset of the primary — so it was quietly also asserting that a source adding
        // nothing gets disclosed, which is the bug the per-source rule had.
        let sources = [
            source("Jitendex", "食べる", "たべる", &["to eat", "to dine"]),
            source("Another Dict", "食べる", "たべる", &["to have a meal"]),
        ];

        let view = merge(&token, &sources);
        assert_eq!(view.inline.len(), 1);
        assert_eq!(view.inline[0].glosses, ["to eat", "to consume"]);
        assert_eq!(view.inline[0].tags, ["v1", "vt"]);
        // Primary needs no attribution line: it is what the app already shows.
        assert_eq!(view.inline_source, None);

        // Both other sources are behind the disclosure, not inline.
        assert_eq!(
            view.also_in
                .iter()
                .map(|a| a.title.as_str())
                .collect::<Vec<_>>(),
            ["Jitendex", "Another Dict"]
        );
        assert_eq!(view.reading.as_deref(), Some("たべる"));
    }

    /// The case this was written for: Jitendex is JMdict underneath and so is the
    /// analyzer, so for most words the disclosure reprinted the senses directly above it.
    #[test]
    fn a_source_that_only_repeats_the_primary_is_not_disclosed() {
        let token = token("気持ち", "きもち", &[("[n]", "feeling; sensation; mood")]);
        let sources = [source(
            "Jitendex",
            "気持ち",
            "きもち",
            &["feeling", "sensation", "mood"],
        )];

        let view = merge(&token, &sources);
        assert_eq!(view.inline[0].glosses, ["feeling", "sensation", "mood"]);
        assert!(
            view.also_in.is_empty(),
            "a source saying exactly what is already on screen has nothing to disclose"
        );
    }

    /// This asserted the opposite until 水 made the cost of it plain.
    ///
    /// The old rule showed a disclosed source whole, on the reasoning that printing two of
    /// its three senses would misreport what it says. But the disclosure is titled *also
    /// in* — it is already framed as "what else", not "what Jitendex is" — and reprinting
    /// what the reader has just read does not represent the source, it buries the sense that
    /// would have. The honesty this was protecting lives on one level down: a sense that
    /// survives is shown with every gloss it has.
    #[test]
    fn a_source_with_something_extra_is_disclosed_with_only_that() {
        let token = token("気持ち", "きもち", &[("[n]", "feeling")]);
        let mut extra = source("Jitendex", "気持ち", "きもち", &["feeling"]);
        extra.hits[0]
            .entry
            .definitions
            .push(crate::dict::Definition {
                tags: vec!["n".into()],
                glosses: vec!["preparedness".into()],
                examples: Vec::new(),
            });

        let view = merge(&token, &[extra]);
        let disclosed = &view.also_in[0].senses;
        assert_eq!(disclosed.len(), 1, "\"feeling\" is already on screen");
        assert_eq!(disclosed[0].glosses, ["preparedness"]);
        // Its tags come with it: the row is drawn like any other sense.
        assert_eq!(disclosed[0].tags, ["n"]);
    }

    #[test]
    fn a_source_saying_less_than_the_primary_is_not_disclosed() {
        let token = token(
            "気持ち",
            "きもち",
            &[("[n]", "feeling"), ("[n]", "preparedness")],
        );
        let sources = [source("Jitendex", "気持ち", "きもち", &["feeling"])];

        let view = merge(&token, &sources);
        assert_eq!(view.inline.len(), 2);
        assert!(
            view.also_in.is_empty(),
            "a strict subset of what is already shown adds nothing either"
        );
    }

    /// The セイア case, exactly as it appeared on screen: Jitendex says less than the
    /// analyzer already did, so the disclosure had nothing to disclose.
    #[test]
    fn a_source_saying_a_subset_of_what_is_already_shown_is_not_disclosed() {
        let token = token(
            "井蛙",
            "せいあ",
            &[("[n]", "frog in a well; someone with a narrow perspective")],
        );
        let sources = [source("Jitendex", "井蛙", "せいあ", &["frog in a well"])];

        let view = merge(&token, &sources);
        assert_eq!(
            view.inline[0].glosses,
            ["frog in a well", "someone with a narrow perspective"]
        );
        assert!(
            view.also_in.is_empty(),
            "every gloss it has is already on screen: {:?}",
            view.also_in
        );
    }

    /// And the other half: one new gloss among old ones is still worth showing, because
    /// the rule is "adds nothing", not "overlaps".
    #[test]
    fn a_source_with_one_new_gloss_among_familiar_ones_is_still_disclosed() {
        let token = token("井蛙", "せいあ", &[("[n]", "frog in a well")]);
        let sources = [source(
            "Another Dict",
            "井蛙",
            "せいあ",
            &["frog in a well", "a provincial outlook"],
        )];

        let view = merge(&token, &sources);
        assert_eq!(
            view.also_in
                .iter()
                .map(|a| a.title.as_str())
                .collect::<Vec<_>>(),
            ["Another Dict"]
        );
        // **And the sense arrives whole.** One meaning said two ways: printing only "a
        // provincial outlook" because the other half was already on screen would quote half
        // a sentence. The sense is the smallest unit that can be shown without misquoting.
        assert_eq!(
            view.also_in[0].senses[0].glosses,
            ["frog in a well", "a provincial outlook"]
        );
    }

    /// 水, which is what this rule exists for.
    ///
    /// Jitendex answers it with six senses. Five are the five the analyzer already printed
    /// directly above, and the sixth is "Wednesday" — so opening the disclosure meant reading
    /// the same five again to find the one thing it had to say.
    #[test]
    fn a_disclosed_source_shows_only_what_the_page_has_not_said() {
        let token = token(
            "水",
            "みず",
            &[
                ("[n]", "water (esp. cool or cold)"),
                ("[n]", "fluid (esp. in an animal tissue); liquid"),
                ("[n]", "flood; floodwaters"),
                ("[n]", "water offered to wrestlers just prior to a bout"),
                (
                    "[n]",
                    "break granted to wrestlers engaged in a prolonged bout",
                ),
            ],
        );
        let sources = [senses_source(
            "Jitendex.org",
            "水",
            "みず",
            &[
                &["water (esp. cool or cold)"],
                &["fluid (esp. in an animal tissue)", "liquid"],
                &["flood", "floodwaters"],
                &["water offered to wrestlers just prior to a bout"],
                &["break granted to wrestlers engaged in a prolonged bout"],
                &["Wednesday"],
            ],
        )];

        let view = merge(&token, &sources);
        let disclosed = &view.also_in[0];
        assert_eq!(disclosed.title, "Jitendex.org");
        assert_eq!(
            disclosed.senses.len(),
            1,
            "the five the reader has already read are not printed again"
        );
        assert_eq!(disclosed.senses[0].glosses, ["Wednesday"]);
    }

    #[test]
    fn a_difference_of_letter_case_alone_is_not_a_difference() {
        let token = token("気持ち", "きもち", &[("[n]", "Feeling; Mood")]);
        let sources = [source(
            "Jitendex",
            "気持ち",
            "きもち",
            &["feeling", "  mood  "],
        )];

        let view = merge(&token, &sources);
        assert!(
            view.also_in.is_empty(),
            "the reader would see the same words twice, so the casing is not the point"
        );
    }

    #[test]
    fn a_word_the_primary_lacks_is_promoted_and_named() {
        // The case this rule exists for: a name. JMdict has nothing, JMnedict does.
        let token = Token {
            surface: "田中".into(),
            dict_form: "田中".into(),
            ..Default::default()
        };
        let sources = [
            source("JMnedict", "田中", "たなか", &["Tanaka (surname)"]),
            source("Another Dict", "田中", "たなか", &["a rice-field"]),
        ];

        let view = merge(&token, &sources);
        assert_eq!(view.inline[0].glosses, ["Tanaka (surname)"]);
        // Promoted definitions must say where they came from.
        assert_eq!(view.inline_source.as_deref(), Some("JMnedict"));
        // The promoted source is not also listed as a disclosure.
        assert_eq!(
            view.also_in
                .iter()
                .map(|a| a.title.as_str())
                .collect::<Vec<_>>(),
            ["Another Dict"]
        );
        // With no analyzer reading, the source's agreed reading is adopted.
        assert_eq!(view.reading.as_deref(), Some("たなか"));
    }

    #[test]
    fn a_source_that_disagrees_with_itself_does_not_supply_a_reading() {
        // Guessing here produces wrong furigana on the reader page.
        let token = Token {
            surface: "行".into(),
            dict_form: "行".into(),
            ..Default::default()
        };
        let mut disagreeing = source("Ambiguous", "行", "こう", &["a line"]);
        disagreeing.hits.push(LookupHit {
            entry: Entry {
                term: "行".into(),
                reading: "ぎょう".into(),
                definitions: vec![Definition {
                    glosses: vec!["a row".into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
            matched: MatchKind::Spelling,
        });

        let view = merge(&token, &[disagreeing]);
        assert_eq!(view.reading, None, "two readings means no adopted reading");
        // The definitions are still promoted — only the reading is withheld.
        assert_eq!(view.inline.len(), 2);
    }

    #[test]
    fn examples_attach_to_the_word_and_are_not_duplicated() {
        let token = token("危ない", "あぶない", &[("[adj-i]", "dangerous")]);
        let with_example = |title: &str, japanese: &str| SourceResult {
            retrieved: whole(),
            title: title.into(),
            hits: vec![LookupHit {
                entry: Entry {
                    term: "危ない".into(),
                    reading: "あぶない".into(),
                    definitions: vec![Definition {
                        glosses: vec!["dangerous".into()],
                        examples: vec![SourceExample {
                            japanese: japanese.into(),
                            english: "Look out!".into(),
                        }],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                matched: MatchKind::Spelling,
            }],
        };

        // Two sources drawing on the same corpus offer the same sentence.
        let view = merge(
            &token,
            &[
                with_example("Jitendex", "危ない!"),
                with_example("Another Dict", "危ない!"),
            ],
        );
        assert_eq!(view.examples.len(), 1, "the same sentence is kept once");
        assert_eq!(view.examples[0].japanese, "危ない!");
        assert_eq!(view.examples[0].retrieved, whole());

        let mut through_part = with_example("Component Dict", "危ない!");
        through_part.retrieved = Retrieved::Component {
            query: "危".into()
        };
        let view = merge(
            &token,
            &[through_part, with_example("Whole Dict", "危ない!")],
        );
        assert_eq!(view.examples.len(), 1);
        assert_eq!(
            view.examples[0].retrieved,
            whole(),
            "deduplication keeps whole-query evidence"
        );
    }

    #[test]
    fn each_answer_group_keeps_its_own_retrieval_scope() {
        let token = Token {
            surface: "神姫".into(),
            dict_form: "神姫".into(),
            components: vec![Token {
                surface: "神".into(),
                dict_form: "神".into(),
                ..Token::default()
            }],
            ..Token::default()
        };
        let component = Retrieved::Component {
            query: "神".into()
        };
        let mut definition = source("Jitendex", "神", "かみ", &["god; deity"]);
        definition.retrieved = component.clone();
        definition.hits[0].entry.definitions[0].examples = vec![SourceExample {
            japanese: "神を信じる。".into(),
            english: "Believe in God.".into(),
        }];
        let mut additional = source("Other", "神姫", "しんき", &["divine princess"]);
        additional.retrieved = Retrieved::Whole {
            query: "神姫".into(),
        };
        let name = name_source("JMnedict", "しんき", &["fem"], &["Shinki"]);

        let view = merge(&token, &[definition, additional, name]);
        assert_eq!(view.inline_retrieved, whole_for("神姫"));
        assert_eq!(view.also_in[0].retrieved, component);
        assert_eq!(
            view.name.as_ref().expect("whole-token name").retrieved,
            whole()
        );
        assert_eq!(view.examples[0].retrieved.component(), Some("神"));
        assert_eq!(
            view.headword, "神姫",
            "provenance never replaces the selection"
        );

        // In the reported passage no ordinary whole-word definition exists. The component
        // remains useful, but must be labelled as such under the whole-token heading.
        let mut component_only = source("Jitendex", "神", "かみ", &["god; deity"]);
        component_only.retrieved = Retrieved::Component {
            query: "神".into()
        };
        let view = merge(
            &token,
            &[
                component_only,
                name_source("JMnedict", "しんき", &["fem"], &["Shinki"]),
            ],
        );
        assert_eq!(view.inline_retrieved.component(), Some("神"));
        assert_eq!(view.name.as_ref().unwrap().retrieved, whole());
    }

    #[test]
    fn older_cached_answers_default_each_missing_scope_to_unknown() {
        let token = Token {
            surface: "a".into(),
            dict_form: "a".into(),
            ..Token::default()
        };
        let mut primary = source("Primary", "a", "a", &["first"]);
        primary.hits[0].entry.definitions[0].examples = vec![SourceExample {
            japanese: "a sentence".into(),
            english: "an example".into(),
        }];
        let view = merge(
            &token,
            &[
                primary,
                source("Other", "a", "a", &["second"]),
                name_source("Names", "a", &["fem"], &["A"]),
            ],
        );
        assert_eq!(view.also_in.len(), 1);
        assert!(view.name.is_some());
        assert_eq!(view.examples.len(), 1);
        let mut old = serde_json::to_value(view).unwrap();
        let object = old.as_object_mut().unwrap();
        object.remove("inline_retrieved");
        object["also_in"][0]
            .as_object_mut()
            .unwrap()
            .remove("retrieved");
        object["examples"][0]
            .as_object_mut()
            .unwrap()
            .remove("retrieved");
        object["name"].as_object_mut().unwrap().remove("retrieved");
        let restored: WordView = serde_json::from_value(old).unwrap();
        assert_eq!(restored.inline_retrieved, Retrieved::Unknown);
        assert_eq!(restored.also_in[0].retrieved, Retrieved::Unknown);
        assert_eq!(restored.examples[0].retrieved, Retrieved::Unknown);
        assert_eq!(restored.name.unwrap().retrieved, Retrieved::Unknown);
    }
    #[test]
    fn a_source_with_no_usable_definitions_is_not_disclosed_at_all() {
        let token = token("水", "みず", &[("[n]", "water")]);
        // An entry that parsed but has nothing to show — an image-only definition.
        let empty = SourceResult {
            retrieved: whole(),
            title: "Picture Dict".into(),
            hits: vec![LookupHit {
                entry: Entry {
                    term: "水".into(),
                    reading: "みず".into(),
                    definitions: vec![Definition::default()],
                    ..Default::default()
                },
                matched: MatchKind::Spelling,
            }],
        };

        let view = merge(&token, &[empty]);
        assert!(
            view.also_in.is_empty(),
            "an empty source is noise, not a disclosure"
        );
        assert!(!view.is_empty(), "the primary still has the word");
    }

    /// An entry shaped the way JMnedict-in-Yomitan writes names: no reading field, and
    /// the kana in the glossary.
    fn name_entry(term: &str, reading_as_gloss: &str, score: f64) -> LookupHit {
        LookupHit {
            entry: Entry {
                term: term.into(),
                // An empty reading field is stored as the term itself.
                reading: term.into(),
                score,
                definitions: vec![Definition {
                    glosses: vec![reading_as_gloss.into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
            matched: MatchKind::Spelling,
        }
    }

    #[test]
    fn a_name_whose_reading_hides_in_the_glossary_becomes_a_reading() {
        // 田中 as JMnedict actually stores it: たなか is not what the word *means*.
        let token = Token {
            surface: "田中".into(),
            dict_form: "田中".into(),
            ..Default::default()
        };
        let source = SourceResult {
            retrieved: whole(),
            title: "JMnedict".into(),
            hits: vec![name_entry("田中", "たなか", 10.0)],
        };

        let view = merge(&token, &[source]);
        // One reading only, so it is adopted — and as a reading, not a meaning.
        assert_eq!(view.reading.as_deref(), Some("たなか"));
        // ...and not a definition.
        assert!(
            view.inline.is_empty(),
            "a reading must not be presented as a meaning: {:?}",
            view.inline
        );
    }

    #[test]
    fn a_real_gloss_alongside_name_readings_still_shows() {
        let token = Token {
            surface: "田中".into(),
            dict_form: "田中".into(),
            ..Default::default()
        };
        let mut hits = vec![name_entry("田中", "たなか", 10.0)];
        // The Taiwanese place 田中, which has a genuine reading field and an English
        // gloss — a real entry, not a reading one.
        hits.push(LookupHit {
            entry: Entry {
                term: "田中".into(),
                reading: "ティエンジョン".into(),
                definitions: vec![Definition {
                    glosses: vec!["Tianzhong (Taiwan)".into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
            matched: MatchKind::Spelling,
        });

        let view = merge(
            &token,
            &[SourceResult {
                retrieved: whole(),
                title: "JMnedict".into(),
                hits,
            }],
        );
        assert_eq!(view.inline.len(), 1);
        assert_eq!(view.inline[0].glosses, ["Tianzhong (Taiwan)"]);
        // たなか and ティエンジョン are both stated by this source, so it has not
        // settled on a reading — exactly the case the agreement rule is for.
        assert_eq!(view.reading, None);
    }

    #[test]
    fn an_ordinary_kana_definition_is_not_mistaken_for_a_reading() {
        // A Japanese-Japanese dictionary defines words in kana, and those are meanings.
        // The reading field being *present* is what tells them apart.
        let token = Token {
            surface: "みず".into(),
            dict_form: "みず".into(),
            ..Default::default()
        };
        let source = SourceResult {
            retrieved: whole(),
            title: "JJ Dict".into(),
            hits: vec![LookupHit {
                entry: Entry {
                    term: "水".into(),
                    reading: "みず".into(),
                    definitions: vec![Definition {
                        glosses: vec!["のみもの".into()],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                matched: MatchKind::Spelling,
            }],
        };

        let view = merge(&token, &[source]);
        assert_eq!(
            view.inline[0].glosses,
            ["のみもの"],
            "a kana meaning is a meaning"
        );
        assert_eq!(view.reading.as_deref(), Some("みず"));
    }

    #[test]
    fn a_word_nothing_has_is_empty_rather_than_a_blank_row() {
        // Deliberately not katakana. A katakana word nothing has is not empty — there is
        // something to say about it, and the test below says what.
        let token = Token {
            surface: "彁彁彁".into(),
            dict_form: "彁彁彁".into(),
            ..Default::default()
        };
        let view = merge(&token, &[]);
        assert!(view.is_empty());
        assert_eq!(view.inline_source, None);
        assert_eq!(view.headword, "彁彁彁");
    }

    /// **What `search_words` drops a result on.** A view whose only content is one of the
    /// newer fields was reported as absent by the reference page while the analysis panel
    /// explained it — two answers to one question, at the same moment, for the same word.
    #[test]
    fn a_view_whose_only_answer_is_a_name_or_a_guess_is_not_empty() {
        let unknown = Token {
            surface: "シロコ".into(),
            dict_form: "シロコ".into(),
            romaji: Some("shiroko".into()),
            ..Default::default()
        };
        let guess = merge(&unknown, &[]);
        assert!(guess.likely_name);
        assert!(!guess.is_empty(), "the page has something to say about it");

        let named = Token {
            surface: "田中".into(),
            dict_form: "田中".into(),
            ..Default::default()
        };
        let sources = [name_source("JMnedict", "たなか", &["surname"], &["Tanaka"])];
        let view = merge(&named, &sources);
        assert!(view.inline.is_empty(), "a name is not a sense");
        assert!(view.name.is_some());
        assert!(!view.is_empty(), "a name is an answer");
    }

    /// **The double print.** ヒフミ under Lindera: the analyzer has no senses, so the first
    /// source with any gets promoted inline — and a name-tagged entry counted as one, so the
    /// name was printed as the word's definition *and* as its name, with the real
    /// dictionary's answer demoted into the fold behind them.
    #[test]
    fn a_name_is_never_promoted_into_the_senses() {
        // No senses: Lindera reports none for anything.
        let token = Token {
            surface: "ヒフミ".into(),
            dict_form: "ヒフミ".into(),
            ..Default::default()
        };
        let sources = [
            name_source("JMnedict", "ひふみ", &["fem"], &["Hifumi"]),
            folded_source("Another Dict", "一二三", "ひふみ", &["one two three"]),
        ];
        let view = merge(&token, &sources);

        assert_eq!(
            view.inline[0].glosses,
            ["one two three"],
            "the real dictionary answers; the name is not a definition"
        );
        assert_eq!(view.inline_source.as_deref(), Some("Another Dict"));
        let name = view.name.expect("still offered as a name");
        assert_eq!(name.glosses, ["Hifumi"]);
        assert!(
            !view
                .inline
                .iter()
                .any(|sense| sense.glosses.contains(&"Hifumi".to_string())),
            "the name was printed twice: {:?}",
            view.inline
        );
    }

    /// With nothing else found, the name is the answer and may not be folded away.
    ///
    /// The ordinary case under Lindera, which contributes no senses of its own — so
    /// "it leads only on a low score" would have meant it never leads there at all.
    #[test]
    fn a_name_leads_when_it_is_the_only_thing_found() {
        let token = Token {
            surface: "セイア".into(),
            dict_form: "セイア".into(),
            ..Default::default()
        };
        assert_eq!(token.confidence, None, "Lindera reports no score");

        let sources = [name_source("JMnedict", "せいあ", &["fem"], &["Seia"])];
        let view = merge(&token, &sources);

        let name = view.name.expect("JMnedict has it");
        assert!(name.leads, "there is nothing for it to come after");
        assert!(view.inline.is_empty());
    }

    /// The exclusion is per hit, not per source.
    ///
    /// `tags.rs` records that `fem` and `masc` exist in JMdict too, so an imported dictionary
    /// can carry a name code on one entry. Keyed on the source's title, that one entry
    /// suppressed every genuine sense the dictionary had.
    #[test]
    fn one_name_entry_does_not_silence_the_dictionary_it_is_in() {
        let token = token("あたし", "あたし", &[("[pn]", "I; me")]);
        let mut mixed = source("Another Dict", "あたし", "あたし", &["I (feminine)"]);
        // A second entry in the same dictionary, filed as a name.
        mixed.hits.push(LookupHit {
            entry: Entry {
                term: "あたし".into(),
                reading: "あたし".into(),
                definition_tags: vec!["fem".into()],
                definitions: vec![Definition {
                    glosses: vec!["Atashi".into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
            matched: MatchKind::Spelling,
        });

        let view = merge(&token, &[mixed]);
        assert_eq!(
            view.also_in
                .iter()
                .map(|a| a.title.as_str())
                .collect::<Vec<_>>(),
            ["Another Dict"],
            "one name entry must not take the whole dictionary's disclosure with it"
        );
        assert_eq!(view.also_in[0].senses[0].glosses, ["I (feminine)"]);
        assert!(
            !view.also_in[0]
                .senses
                .iter()
                .any(|sense| sense.glosses == ["Atashi"]),
            "and the name itself is still not a sense"
        );
    }

    /// **Lindera reports no score, and the caveat may not depend on one.**
    ///
    /// Identical to `a_folded_junk_match_is_flagged_even_though_something_was_found` but for
    /// the missing `confidence` — which is the whole of what the built-in analyzer differs by,
    /// and which used to be the difference between a caveat and none.
    #[test]
    fn a_folded_junk_match_is_flagged_without_a_score_to_go_on() {
        let token = token("シロコ", "しろこ", &[("[n]", "soft roe")]);
        assert_eq!(token.confidence, None);

        let sources = [folded_source("Jitendex", "白子", "しろこ", &["soft roe"])];
        let view = merge(&token, &sources);

        assert!(
            view.likely_name,
            "nothing is spelled シロコ, whatever the analyzer"
        );
        assert_eq!(
            view.inline[0].glosses,
            ["soft roe"],
            "the gloss is not destroyed"
        );
    }
}
