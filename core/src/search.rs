//! Reference search policy, with stable resource handles for the entire request.
//!
//! The shell owns discovery, cache lifetime and IPC. This service owns query analysis,
//! entry grouping, caps, frequency ordering and character enrichment. Every result uses
//! the same borrowed dictionaries, including contextual lookup and alternative spellings.

use crate::analyzer::{AnalyzeError, Analyzer};
use crate::dict::{Dictionary, WordView};
use crate::model::Token;
use serde::Serialize;

/// Failure classification belongs to the service; the shell renders the original message.
#[derive(Debug)]
pub enum SearchError {
    Analysis(AnalyzeError),
    Dictionary(crate::dict::DictError),
    Lookup(crate::lookup::LookupError),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Analysis(error) => error.fmt(f),
            Self::Dictionary(error) => error.fmt(f),
            Self::Lookup(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for SearchError {}

/// Independently acquired handles, each held stable until the search returns.
/// This does not promise an atomic transaction across the different resource kinds.
pub struct Resources<'a> {
    pub preferred: &'a dyn Analyzer,
    pub built_in: &'a dyn Analyzer,
    pub dictionaries: &'a [Dictionary],
    pub kanji: &'a crate::dict::KanjiDic,
    pub frequency: Option<&'a crate::dict::frequency::Frequencies>,
}

/// Analyse a Reference query, spending the sidecar only where it buys something.
///
/// **The Reference page is a lookup surface, and latency is most of what it is.** With Ichiran
/// installed every Japanese search pays a `wsl -e ichiran-cli` process spawn — 0.7s warm, 7.6s
/// against a cold WSL session — while the identical English search answers out of a prebuilt
/// gloss index. That asymmetry is the whole complaint.
///
/// The split is by **query shape, not by page**, and the shape is measured rather than guessed:
/// the built-in analyzer answers first, and its answer is kept when the query turns out to be a
/// single word. Reference queries overwhelmingly are one — that is what a search box gets — and a
/// single word is exactly where Ichiran's advantages do not apply: there is no segmentation to get
/// right and no context to rank in.
///
/// **The test is `contextual_phrase`'s own**, deliberately. More than one Japanese token is
/// precisely the condition under which this function's caller runs contextual lookup, so the rich
/// analyzer is asked for exactly when context is about to be used. Anything Ichiran would segment
/// differently is, by construction, something Lindera also sees as more than one token — the
/// disagreement falls on the safe side, costing a re-analysis rather than a wrong answer.
///
/// Two things this deliberately does **not** do:
///
/// - It does not change the reading page. `analyze` keeps the preferred analyzer for the whole
///   passage, so the two pages still agree about any sentence: a phrase reaches Ichiran here too.
/// - It does not touch word identity. `WordKey::for_token` keys on `dict_form` and
///   `lemma_reading`, both analyzer output, and Reference derives no `WordKey` today. **If it ever
///   marks words again, this is the function that would let one word become two records**, and the
///   marking path must read the same analysis this one returned rather than re-deriving it.
///
/// When the preferred analyzer *is* the built-in one there is nothing to save, and nothing is
/// spent finding that out.
fn analyse_query(
    preferred: &dyn Analyzer,
    built_in: &dyn Analyzer,
    query: &str,
) -> Result<crate::model::Analysis, AnalyzeError> {
    if preferred.engine() == built_in.engine() {
        return preferred.analyze(query);
    }
    match built_in.analyze(query) {
        // One word, or none: the fast answer is the answer.
        Ok(quick) if japanese_tokens(&quick) <= 1 => Ok(quick),
        // A phrase, or the built-in analyzer could not read it. Either way the preferred one
        // decides; a failure here is not worth reporting, because the next line reports better.
        _ => preferred.analyze(query),
    }
}

/// How many of an analysis's tokens are Japanese words.
///
/// The same filter `search_words_uncached` uses to decide whether a query is a contextual phrase,
/// so the two cannot drift into disagreeing about what "one word" means.
fn japanese_tokens(analysis: &crate::model::Analysis) -> usize {
    analysis
        .tokens
        .iter()
        .filter(|token| {
            !token.surface.trim().is_empty() && token.surface.chars().any(crate::kana::is_japanese)
        })
        .count()
}

/// One reading a character has, and whether the searched word uses it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct CharReading {
    pub text: String,
    /// True for the reading this character supplies in the word searched for.
    ///
    /// False on every reading is not "none of these" — it also covers a word whose
    /// reading is not built from its characters at all. [`KanjiView::irregular`] is what
    /// separates the two.
    pub here: bool,
}

/// One character, as the reference page shows it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct KanjiView {
    pub character: String,
    pub on: Vec<CharReading>,
    pub kun: Vec<CharReading>,
    /// Name-only readings. Never marked as the reading used by an ordinary word.
    pub nanori: Vec<String>,
    pub meanings: Vec<String>,
    pub strokes: u8,
    pub grade: Option<u8>,
    /// Visible parts from basic KRADFILE, not necessarily traditional radicals.
    pub components: Vec<String>,
    /// The searched word's reading is not built from its characters' own readings —
    /// 明日 あした. Nothing is marked, and the page says why rather than leaving the
    /// learner to wonder which reading applies.
    pub irregular: bool,
}

/// One result: a dictionary entry, the other ways it is written, and its characters.
///
/// A wrapper in this layer rather than more fields on [`WordView`], which is `core`'s model
/// for *a word* and is shown by the reader's vocabulary panel too. What is added here is
/// about a search result — which spellings the search collapsed, and which characters to
/// offer under it — so it belongs to the search.
///
/// `#[serde(flatten)]` keeps the JSON shape a `WordView` with two more keys, so the interface
/// sees one object rather than a wrapper it has to unwrap at every use.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WordHit {
    #[serde(flatten)]
    pub word: WordView,
    /// Other spellings of this same dictionary entry, commonest first.
    pub forms: Vec<Form>,
    /// This word's characters, `here` marking the reading this word lends each one.
    pub chars: Vec<KanjiView>,
    /// Sentence-local sense evidence, only when a Japanese Reference query contains more than
    /// one analyzable word. A single word and every English query deliberately leave this empty:
    /// without neighbouring language, Reference has no context from which to make a preference.
    pub ranking: Option<crate::dict::VocabularyRanking>,
    /// The form this hit was found from — what the reader actually typed or clicked.
    ///
    /// 走った arrives and 走る is answered, and the page said nothing about the difference; on a
    /// page for learners that difference is the lesson rather than plumbing to hide.
    ///
    /// **A fact, not a decision.** Whether it is worth showing is `surfaceWorthShowing` in
    /// `ui/src/lib/kana.js`, which the selected-word header already uses — one rule, in the one
    /// place both surfaces that draw the chip can reach. This reports; the interface decides.
    /// `None` for every English query, for the same reason `ranking` is: an English query makes no
    /// claim about how a Japanese word was written.
    pub surface: Option<String>,
}

/// What decides that two hits are the same entry.
///
/// The sequence number when the dictionary gives one. `yomitan.rs` defaults a missing field
/// to `0`, so every entry in a dictionary that omits it would otherwise collapse into a
/// single result — those fall back to the surface form, which is the behaviour this replaced.
///
/// **Its absolute value.** Jitendex writes a search-only form as the negation of the entry it
/// belongs to: アレ is `-1000580` against あれ's `1000580`, イケない is `-1000730` against
/// いけない's `1000730`. Measured over the installed dictionary, 137,713 of 433,885 entries
/// carry a negative sequence and **not one** `|sequence|` group is negative-only — so a
/// negative number is never an entry in its own right, always a form of the positive one.
/// Keyed on the signed value, アレ and あれ come back as two results, which is the same bug
/// this function exists to fix wearing a different hat.
#[derive(Clone, PartialEq, Eq, Hash)]
enum HitKey {
    Entry(i64),
    Surface(String, String),
}

impl HitKey {
    fn of(entry: &crate::dict::yomitan::Entry) -> Self {
        if entry.sequence != 0 {
            HitKey::Entry(entry.sequence.abs())
        } else {
            HitKey::Surface(entry.term.clone(), entry.reading.clone())
        }
    }
}

/// How many distinct entries a pile of hits holds, for the cap.
fn distinct_entries(hits: &[crate::dict::yomitan::Entry]) -> usize {
    hits.iter()
        .map(HitKey::of)
        .collect::<std::collections::HashSet<_>>()
        .len()
}

/// One result per dictionary entry: the commonest spelling, and the others beside it.
///
/// ついでに, 序に and 序でに are three spellings of one JMdict entry — sequence 1008050 — and
/// arrived as three identical results, because the old key here was `(term, reading)`.
/// JMdict's DTD is explicit about what a second kanji element means: where an entry has
/// several, "they will be orthographical variants of the same word, either using variations
/// in okurigana, or alternative and equivalent kanji". So the entry is the thing to show
/// once, and `yomitan.rs` has said so all along — "entries sharing a sequence number belong
/// together".
///
/// This is also what separates 竜/龍 from 辞典/字典. Both pairs share a reading and gloss
/// alike; only the first is one entry. A rule written over readings and senses cannot see the
/// difference, and an earlier attempt at one got 竜/龍 wrong.
///
/// Order is the order the entries first arrived in, which is the search's own ranking. The
/// corpus sort runs afterwards and is free to overturn it.
fn one_per_entry(
    hits: Vec<crate::dict::yomitan::Entry>,
    cap: usize,
) -> Vec<(crate::dict::yomitan::Entry, Vec<Form>)> {
    let mut groups: Vec<Vec<crate::dict::yomitan::Entry>> = Vec::new();
    let mut at: std::collections::HashMap<HitKey, usize> = std::collections::HashMap::new();
    for entry in hits {
        let key = HitKey::of(&entry);
        match at.get(&key) {
            Some(&index) => groups[index].push(entry),
            None => {
                if groups.len() >= cap {
                    continue;
                }
                at.insert(key, groups.len());
                groups.push(vec![entry]);
            }
        }
    }

    groups
        .into_iter()
        .map(|mut entries| {
            // Highest score leads, so the form shown is the one the dictionary treats as
            // ordinary — ついでに at 200 rather than 序に at -102, whichever came back first.
            // `total_cmp` because these are `f64` and `sort_by` needs a total order.
            entries.sort_by(|left, right| right.score.total_cmp(&left.score));
            let lead = entries.remove(0);
            // Every other *spelling*, once each. A group also holds one entry per reading —
            // 竜 arrives three times, for りゅう, たつ and りょう — and those are not
            // spellings.
            let mut forms: Vec<Form> = Vec::new();
            for entry in &entries {
                if entry.term == lead.term || forms.iter().any(|form| form.text == entry.term) {
                    continue;
                }
                forms.push(Form {
                    text: entry.term.clone(),
                    rare: entry.score < 0.0,
                });
            }
            (lead, forms)
        })
        .collect()
}

/// One alternative spelling.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(rename = "SearchForm"))]
pub struct Form {
    pub text: String,
    /// The dictionary scores this form below zero.
    ///
    /// Which is its way of saying rarely-written or an outdated character form — EDRDG's
    /// `rK` and `oK`. Jitendex leaves `term_tags` empty, so the score is the only signal
    /// there is, and it agrees with the documented tags everywhere it was checked: 付近 200
    /// against 附近 -101 (`rK`), 竜 200 against 龍 -101 (`oK`). A *positive* alternative like
    /// 鮨 at 99 is an ordinary second spelling and is not marked.
    pub rare: bool,
}

/// What a reference search turned up.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Found {
    pub words: Vec<WordHit>,
    /// Which way the query was read. The page used to work this out again from its own
    /// regex and disagree with this one about halfwidth katakana.
    pub japanese: bool,
    /// The corpus the word results were ordered by, when one is installed.
    ///
    /// Named rather than implied. BCCWJ, JPDB and Innocent Corpus counted different
    /// Japanese and disagree about what is common, so "most common first" is only a
    /// meaningful claim with the corpus attached to it.
    pub ranked_by: Option<String>,
    /// The query was English and no dictionary has a gloss index yet.
    ///
    /// Distinct from "no results", and the interface must not conflate them: one means
    /// the word is not in the dictionary, the other means nothing was looked at.
    pub needs_gloss_index: bool,
    /// More entries matched than are being returned.
    ///
    /// **A boolean and not a number, deliberately.** The count available cheaply is of dictionary
    /// *entries*, and several spellings of one word collapse into one result — so a figure taken
    /// from it would overstate what a reader would actually see. What is measured here is exactly
    /// what is claimed: one more distinct entry existed than fitted. A true total needs the
    /// sequence number the index does not store; when that bucket is built, this can become one.
    ///
    /// Always `false` for a Japanese query, which returns one hit per token and never truncates.
    pub capped: bool,
}

/// The characters of `term`, with the reading each supplies in `reading` marked.
///
/// Distinct characters in the order written — a word repeating a character shows it once.
fn kanji_of(term: &str, reading: &str, dic: &crate::dict::KanjiDic) -> Vec<KanjiView> {
    let marks = crate::dict::align(term, reading, dic);
    let irregular = marks.is_none();

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (index, character) in term.chars().enumerate() {
        let Some(kanji) = dic.get(character) else {
            continue;
        };
        if !seen.insert(character) {
            continue;
        }
        let used = marks
            .as_ref()
            .and_then(|marks| marks.get(index))
            .and_then(Option::as_deref);
        out.push(kanji_view(character, kanji, used, irregular));
    }
    out
}

pub fn kanji_view(
    character: char,
    kanji: &crate::dict::Kanji,
    used: Option<&str>,
    irregular: bool,
) -> KanjiView {
    // The alignment names a reading in the normalised form it matched — hiragana, and
    // without the okurigana a kun reading writes after its period — so a KANJIDIC reading
    // is "the one used here" when it reduces to the same thing.
    let marked = |reading: &str, on: bool| -> bool {
        let Some(used) = used else { return false };

        if on {
            // **Equality**, not a prefix. An on reading is written whole and has no
            // okurigana to allow for, so `starts_with` marked every shorter reading that
            // happened to prefix the one in play. 504 characters carry such a pair — 世
            // is セイ *and* セ, 主 is シュ and シュウ — and 世紀 せいき highlighted both
            // as "the reading here", which is two answers to a question that has one.
            let plain: String = reading.chars().map(crate::kana::to_hiragana_char).collect();
            return !plain.is_empty() && used == plain;
        }

        // A kun reading can enter a compound carrying okurigana that is not written —
        // 焼 や.く as やき in 焼豚 — so its stem is a prefix of what was used and is still
        // the reading being pointed at.
        let stem = reading.trim_matches('-').split('.').next().unwrap_or("");
        !stem.is_empty() && used.starts_with(stem)
    };

    KanjiView {
        character: character.to_string(),
        on: kanji
            .on
            .iter()
            .map(|reading| CharReading {
                text: reading.clone(),
                here: marked(reading, true),
            })
            .collect(),
        kun: kanji
            .kun
            .iter()
            .map(|reading| CharReading {
                text: reading.clone(),
                here: marked(reading, false),
            })
            .collect(),
        nanori: kanji.nanori.clone(),
        meanings: kanji.meanings.clone(),
        strokes: kanji.strokes,
        grade: kanji.grade,
        components: kanji.components.clone(),
        irregular,
    }
}

/// How many other spellings of a word to offer.
///
/// A cap because a handful is a note and a dozen is a list: 道 has spellings nobody writing
/// Japanese would recognise, and the line reads "Also X · Y · Z" on the front of a card.
const OTHER_SPELLINGS: usize = 6;

/// The other ways this word is written, for a Japanese lookup.
///
/// **Why it looks a word up a second time.** Both Japanese paths — the plain lookup and the
/// contextual one — reach the dictionaries through `core::lookup::sources` and then discard the raw
/// hits when they merge. Threading those hits back out would change the signature of
/// `lookup_in_context_uncached`, which has three callers in two other commands, to serve one line
/// on one page. This asks again instead: the shard is already in memory from the first ask, so
/// the cost is a seek and a parse per spelling, and a Japanese search returns one hit per token.
///
/// The first dictionary that both indexes sequences and knows the word answers. Asking the rest
/// would merge two dictionaries' ideas of the same entry, and they number entries differently.
fn other_spellings(dictionaries: &[Dictionary], headword: &str) -> Vec<Form> {
    for dictionary in dictionaries {
        if !dictionary.has_sequence() {
            continue;
        }
        let Ok(hits) = dictionary.lookup(headword) else {
            continue;
        };
        let Some(lead) = hits.first() else {
            continue;
        };
        let Ok(siblings) = dictionary.same_entry(lead.entry.sequence, OTHER_SPELLINGS) else {
            continue;
        };

        let mut forms: Vec<Form> = Vec::new();
        for entry in siblings {
            // The word itself is not one of its own other spellings, and a group holds one entry
            // per *reading* too — 竜 arrives for りゅう, たつ and りょう — which are not spellings.
            if entry.term == headword || forms.iter().any(|form| form.text == entry.term) {
                continue;
            }
            forms.push(Form {
                // The same rule `one_per_entry` reads: the dictionary scores a rarely-written or
                // outdated form below zero, and Jitendex leaves `term_tags` empty, so the score is
                // the only signal there is.
                rare: entry.score < 0.0,
                text: entry.term,
            });
        }
        if !forms.is_empty() {
            return forms;
        }
    }
    Vec::new()
}

/// How many English results to offer.
///
/// Generous on purpose. An English query answers with *several words that mean this* —
/// the dictionary has no frequency data to rank true synonyms with (see
/// `dict::index::GlossRank`), so showing one and calling it the answer would claim more
/// than is known.
const ENGLISH_RESULTS: usize = 20;

/// One more than is shown, so the page can tell whether it held anything back.
///
/// The whole of the "more matched" notice is this `+ 1`. Asking for exactly what fits leaves no
/// way to distinguish "twenty matched" from "hundreds matched", and the alternative — counting the
/// index's intersection — counts entries rather than results and would overstate. One extra entry
/// read buys a claim that is exactly true.
const ENGLISH_PROBE: usize = ENGLISH_RESULTS + 1;

/// Split what fits from the evidence that more existed.
///
/// A function rather than two lines at the call site because it is the whole of the feature and
/// the boundary is off-by-one by nature: the tests have to exercise *this*, not a copy of its
/// arithmetic that would agree with itself while the page shipped the wrong answer.
fn cap_to_a_page<T>(mut groups: Vec<T>) -> (Vec<T>, bool) {
    let capped = groups.len() > ENGLISH_RESULTS;
    groups.truncate(ENGLISH_RESULTS);
    (groups, capped)
}

pub fn search(query: &str, resources: Resources<'_>) -> Result<Found, SearchError> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(Found {
            words: Vec::new(),
            japanese: false,
            needs_gloss_index: false,
            ranked_by: None,
            capped: false,
        });
    }
    // Pinned for the whole search. An import landing mid-search could otherwise put a result
    // on the page from a dictionary that the `needs_gloss_index` line beneath it was computed
    // without, and the two would contradict each other.

    if query.chars().any(crate::kana::is_japanese) {
        // A single word answers from the built-in analyzer; a phrase pays for the preferred
        // one. See `analyse_query` for why the test is the contextual-phrase test.
        let mut analysis = analyse_query(resources.preferred, resources.built_in, &query)
            .map_err(SearchError::Analysis)?;
        // A searched place name deserves the same rejoining a read one gets.
        //
        // **Joins only, and named as such.** Reference deliberately stops here: it is a search
        // over spellings, not a passage being read, and the correction pass is about what furigana
        // to draw. The consequence is real and is recorded rather than hidden — Reference never
        // sets `reading_ambiguous`, so a word the reader marks as a guess is unmarked here. It is
        // fixed as its own change, with its own evidence; what this line owes the reader is to say
        // which pipeline it ran, instead of leaving that to whichever helper it reached for.
        analysis.tokens = crate::pipeline::Pipeline::new(resources.dictionaries)
            .segmentation(&analysis.tokens)
            .0;
        let analysis = analysis;
        let contextual_phrase = analysis
            .tokens
            .iter()
            .filter(|token| {
                !token.surface.trim().is_empty()
                    && token.surface.chars().any(crate::kana::is_japanese)
            })
            .count()
            > 1;
        let mut words: Vec<WordHit> = Vec::new();
        for (index, token) in analysis
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| !token.surface.trim().is_empty())
        {
            let contextual = contextual_phrase
                .then(|| {
                    crate::lookup::in_context(&analysis.tokens, index, resources.dictionaries)
                        .map_err(SearchError::Lookup)
                })
                .transpose()?;
            let (view, ranking) = match contextual {
                Some(contextual) => (contextual.word, Some(contextual.ranking)),
                None => (
                    crate::lookup::word(token, resources.dictionaries)
                        .map_err(SearchError::Dictionary)?,
                    None,
                ),
            };
            if !view.is_empty() {
                // Here the query *is* Japanese, so `irregular` is a claim worth keeping: it
                // says this word's reading cannot be split across its characters.
                let chars = kanji_of(
                    &view.headword,
                    view.reading.as_deref().unwrap_or_default(),
                    resources.kanji,
                );
                // No `forms`: this path looks a token up once and gets one entry back, so
                // there is nothing that was collapsed to report.
                let forms = other_spellings(resources.dictionaries, &view.headword);
                words.push(WordHit {
                    word: view,
                    forms,
                    chars,
                    ranking,
                    surface: Some(token.surface.clone()),
                });
            }
        }

        return Ok(Found {
            japanese: true,
            words,
            needs_gloss_index: false,
            ranked_by: None,
            // A Japanese query answers one hit per token. There is no cap to hit.
            capped: false,
        });
    }

    // English. Ask every dictionary that can answer, keeping each hit as its own result.
    // This is the branch the ranking is for: a Japanese query has one answer, an English
    // one has a dozen synonyms and no way to order them from the dictionary alone.
    let indexed: Vec<&Dictionary> = resources
        .dictionaries
        .iter()
        .filter(|dictionary| dictionary.has_glosses())
        .collect();

    if indexed.is_empty() {
        return Ok(Found {
            words: Vec::new(),
            japanese: false,
            needs_gloss_index: !resources.dictionaries.is_empty(),
            ranked_by: None,
            capped: false,
        });
    }

    // Grouped by dictionary entry, not by surface form.
    //
    // ついでに, 序に and 序でに are three spellings of one JMdict entry — sequence 1008050 —
    // and arrived as three identical results, because the key here was `(term, reading)`.
    // JMdict's DTD is explicit about what a second kanji element means: where an entry has
    // several "they will be orthographical variants of the same word, either using variations
    // in okurigana, or alternative and equivalent kanji". So the entry is the thing to show
    // once, and `yomitan.rs` has said so all along — "entries sharing a sequence number
    // belong together".
    //
    // This is also what tells 竜/龍 apart from 辞典/字典. Both pairs share a reading and read
    // alike; only the first pair is one entry. A rule written over readings and senses cannot
    // see the difference, and an earlier attempt at one got 竜/龍 wrong in both directions.
    let mut hits: Vec<crate::dict::yomitan::Entry> = Vec::new();
    for dictionary in indexed {
        // Checked here too: the `break` below leaves only the inner loop, so each extra
        // indexed dictionary used to append past the cap — two of them meant up to forty
        // results, each paying a full merge across every dictionary.
        if distinct_entries(&hits) >= ENGLISH_PROBE {
            break;
        }
        for hit in dictionary
            .search_glosses(&query, ENGLISH_PROBE)
            .map_err(SearchError::Dictionary)?
        {
            hits.push(hit.entry);
        }
    }

    // One result per entry: the commonest spelling leads and the rest go beside it.
    //
    // Grouped to `ENGLISH_PROBE` rather than `ENGLISH_RESULTS` so the one extra group, if it
    // exists, is what says the ceiling was reached. It is then dropped: the reader is told there
    // is more, never shown a twenty-first result they did not ask for.
    let (groups, capped) = cap_to_a_page(one_per_entry(hits, ENGLISH_PROBE));

    let mut words = Vec::new();
    for (lead, forms) in groups {
        let token = Token {
            surface: lead.term.clone(),
            dict_form: lead.term.clone(),
            reading: lead.reading.clone(),
            lemma_reading: lead.reading.clone(),
            ..Default::default()
        };
        let word =
            crate::lookup::word(&token, resources.dictionaries).map_err(SearchError::Dictionary)?;

        // `irregular` is a claim about the reading of *what was typed*, and the page prints
        // it as one. What was typed here is English and this word is a result rather than the
        // query, so there is no such claim to make. The `here` marks stay: those say which
        // reading this word lends each character, which is the whole point of showing them
        // under it.
        let chars = kanji_of(
            &word.headword,
            word.reading.as_deref().unwrap_or_default(),
            resources.kanji,
        )
        .into_iter()
        .map(|character| KanjiView {
            irregular: false,
            ..character
        })
        .collect();

        words.push(WordHit {
            word,
            forms,
            chars,
            ranking: None,
            // An English query says nothing about how a Japanese word was written.
            surface: None,
        });
    }

    // The ranking, and the only place it applies. A word the corpus never saw keeps its
    // place at the back rather than being sorted as though it had been measured and found
    // rare — `None` from `rank` means "not in this corpus", which is a different claim.
    let ranked_by = resources.frequency.map(|corpus| {
        words.sort_by(|left, right| {
            let of = |hit: &WordHit| {
                corpus.rank(
                    &hit.word.headword,
                    hit.word.reading.as_deref().unwrap_or_default(),
                )
            };
            match (of(left), of(right)) {
                (Some(a), Some(b)) => a.total_cmp(&b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                // Both unmeasured: leave them as the dictionary had them.
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
        corpus.name.clone()
    });

    Ok(Found {
        words,
        japanese: false,
        needs_gloss_index: false,
        ranked_by,
        capped,
    })
}

#[cfg(test)]
mod english_cap_tests {
    use super::*;
    use crate::dict::yomitan::Entry;

    fn entry(sequence: i64, term: &str) -> Entry {
        Entry {
            term: term.to_string(),
            reading: term.to_string(),
            sequence,
            ..Default::default()
        }
    }

    /// One distinct entry per sequence number, which is what `one_per_entry` groups by.
    fn distinct(count: i64) -> Vec<Entry> {
        (0..count)
            .map(|n| entry(1000 + n, &format!("w{n}")))
            .collect()
    }

    /// **The off-by-one is the whole feature**, so both sides of the boundary are asserted.
    ///
    /// Exactly `ENGLISH_RESULTS` distinct entries is a complete answer and must not claim
    /// otherwise; one more is the only evidence available that the ceiling was reached, and it
    /// must be counted and then dropped rather than shown as a twenty-first result.
    #[test]
    fn the_extra_entry_is_counted_and_then_thrown_away() {
        let probed = one_per_entry(distinct(ENGLISH_PROBE as i64), ENGLISH_PROBE);
        assert_eq!(
            probed.len(),
            ENGLISH_PROBE,
            "the probe reaches one past the ceiling"
        );
        let (shown, capped) = cap_to_a_page(probed);
        assert!(
            capped,
            "a twenty-first distinct entry is the evidence there is more"
        );
        assert_eq!(shown.len(), ENGLISH_RESULTS, "and it is not returned");
    }

    #[test]
    fn a_full_page_that_is_the_whole_answer_claims_nothing_more() {
        let (shown, capped) = cap_to_a_page(one_per_entry(
            distinct(ENGLISH_RESULTS as i64),
            ENGLISH_PROBE,
        ));
        assert!(
            !capped,
            "exactly a full page is a complete answer, not a truncated one"
        );
        assert_eq!(shown.len(), ENGLISH_RESULTS);
    }

    /// Spellings are why the count could not be a number. Three spellings of one entry are one
    /// result, so an entry count would have said three where a reader sees one.
    #[test]
    fn spellings_of_one_word_do_not_push_the_page_over_its_ceiling() {
        let mut hits = distinct(ENGLISH_RESULTS as i64);
        hits.push(entry(1000, "序に"));
        hits.push(entry(1000, "序でに"));

        let (shown, capped) = cap_to_a_page(one_per_entry(hits, ENGLISH_PROBE));
        assert_eq!(
            shown.len(),
            ENGLISH_RESULTS,
            "two more spellings of an entry already shown are not two more results"
        );
        assert!(!capped, "so nothing was held back");
        assert_eq!(
            shown[0].1.len(),
            2,
            "they arrive as other spellings of the first result"
        );
    }
}

#[cfg(test)]
mod query_shape_tests {
    use super::*;
    use crate::model::{Analysis, Token};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// An analyzer that splits on spaces and counts how often it was asked.
    ///
    /// The count is the assertion. What matters here is not what the preferred analyzer *says* —
    /// it is whether it was woken at all, because waking it is the `wsl -e ichiran-cli` spawn this
    /// whole split exists to avoid.
    struct Counting {
        name: &'static str,
        calls: AtomicUsize,
    }

    impl Counting {
        fn new(name: &'static str) -> Arc<Counting> {
            Arc::new(Counting {
                name,
                calls: AtomicUsize::new(0),
            })
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::Acquire)
        }
    }

    impl Analyzer for Counting {
        fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            let mut tokens = Vec::new();
            let mut at = 0usize;
            for piece in text.split(' ') {
                if !piece.is_empty() {
                    tokens.push(Token {
                        surface: piece.to_string(),
                        dict_form: piece.to_string(),
                        start: Some(at),
                        end: Some(at + piece.len()),
                        ..Default::default()
                    });
                }
                at += piece.len() + 1;
            }
            Ok(Analysis {
                execution: None,
                text: text.to_string(),
                tokens,
            })
        }
        fn engine(&self) -> &str {
            self.name
        }
    }

    fn analyse(preferred: &Arc<Counting>, built_in: &Arc<Counting>, query: &str) -> Analysis {
        let rich: Arc<dyn Analyzer> = preferred.clone();
        let fast: Arc<dyn Analyzer> = built_in.clone();
        analyse_query(rich.as_ref(), fast.as_ref(), query).expect("the stubs always answer")
    }

    #[test]
    fn a_single_word_never_wakes_the_preferred_analyzer() {
        let rich = Counting::new("ichiran");
        let fast = Counting::new("lindera");
        let analysis = analyse(&rich, &fast, "食べる");
        assert_eq!(japanese_tokens(&analysis), 1);
        assert_eq!(fast.calls(), 1, "the built-in analyzer answers");
        assert_eq!(
            rich.calls(),
            0,
            "one word has no segmentation to get right and no context to rank in, so the              sidecar spawn buys nothing and must not happen"
        );
    }

    #[test]
    fn a_phrase_is_handed_to_the_preferred_analyzer() {
        let rich = Counting::new("ichiran");
        let fast = Counting::new("lindera");
        analyse(&rich, &fast, "本を 読む");
        assert_eq!(
            fast.calls(),
            1,
            "measured, not guessed: the shape is read off a real analysis"
        );
        assert_eq!(
            rich.calls(),
            1,
            "more than one Japanese token is exactly when the caller runs contextual lookup, so              the richer analyzer has to be the one that segmented it"
        );
    }

    /// English never reaches here at all — `search_words_uncached` returns from the gloss index
    /// first — but a query with no Japanese in it must still not spend a spawn if it ever did.
    #[test]
    fn a_query_with_no_japanese_words_stays_with_the_built_in_analyzer() {
        let rich = Counting::new("ichiran");
        let fast = Counting::new("lindera");
        analyse(&rich, &fast, "library");
        assert_eq!(rich.calls(), 0);
    }

    #[test]
    fn nothing_is_spent_deciding_when_the_preferred_analyzer_is_the_built_in_one() {
        let fast = Counting::new("lindera");
        let same: Arc<dyn Analyzer> = fast.clone();
        analyse_query(same.as_ref(), same.as_ref(), "本を 読む").expect("answers");
        assert_eq!(
            fast.calls(),
            1,
            "with one analyzer there is no choice to make, and the probe would be pure waste"
        );
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    use crate::dict::yomitan;

    /// The real dictionary's numbers, so these are the app's actual behaviour rather than a
    /// story about it. Every score and sequence below was read out of the installed Jitendex.
    fn entry(term: &str, reading: &str, sequence: i64, score: f64) -> yomitan::Entry {
        yomitan::Entry {
            term: term.into(),
            reading: reading.into(),
            sequence,
            score,
            ..Default::default()
        }
    }

    /// The case this was built for: three spellings of one word arriving as three results.
    #[test]
    fn three_spellings_of_one_entry_come_back_once() {
        let hits = vec![
            entry("序に", "ついでに", 1008050, -102.0),
            entry("ついでに", "ついでに", 1008050, 200.0),
            entry("序でに", "ついでに", 1008050, -101.0),
        ];
        let out = one_per_entry(hits, 20);
        assert_eq!(out.len(), 1, "one entry, one result");
        let (lead, forms) = &out[0];
        assert_eq!(
            lead.term, "ついでに",
            "the form scored 200 leads, not the first to arrive"
        );
        assert_eq!(
            forms.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(),
            ["序でに", "序に"],
            "the other two are kept, commonest first"
        );
        assert!(forms.iter().all(|f| f.rare), "both are scored below zero");
    }

    /// 竜/龍 are one entry and 辞典/字典 are two. Reading and gloss cannot tell them apart —
    /// this is the whole reason the key is the sequence number.
    #[test]
    fn variant_kanji_merge_but_homographs_do_not() {
        let dragon = one_per_entry(
            vec![
                entry("竜", "りゅう", 1629170, 200.0),
                entry("龍", "りゅう", 1629170, -101.0),
                entry("竜", "たつ", 1629170, 98.0),
            ],
            20,
        );
        assert_eq!(dragon.len(), 1);
        assert_eq!(dragon[0].0.term, "竜");
        assert_eq!(
            dragon[0]
                .1
                .iter()
                .map(|f| (f.text.as_str(), f.rare))
                .collect::<Vec<_>>(),
            [("龍", true)],
            "竜 appearing twice for two readings is not a second spelling"
        );

        let dictionaries = one_per_entry(
            vec![
                entry("辞典", "じてん", 1595270, 200.0),
                entry("字典", "じてん", 1315210, 200.0),
            ],
            20,
        );
        assert_eq!(dictionaries.len(), 2, "different entries stay apart");
    }

    /// Jitendex negates the sequence of a search-only form: アレ is -1000580 where あれ is
    /// 1000580. Keyed on the signed number they come back as two results.
    #[test]
    fn a_negated_sequence_is_a_form_of_the_entry_it_negates() {
        let out = one_per_entry(
            vec![
                entry("あれ", "あれ", 1000580, 200.0),
                entry("アレ", "アレ", -1000580, -103.0),
                entry("彼", "あれ", 1000580, -101.0),
            ],
            20,
        );
        assert_eq!(out.len(), 1, "one entry however its forms are signed");
        assert_eq!(out[0].0.term, "あれ");
        assert_eq!(
            out[0].1.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(),
            ["彼", "アレ"],
        );
    }

    /// A positive score is an ordinary second spelling, not a rare one. 鮨 for 寿司 is the
    /// case: it is scored 99, where 龍 is scored -101.
    #[test]
    fn only_a_negative_score_is_marked_rare() {
        let out = one_per_entry(
            vec![
                entry("寿司", "すし", 1595650, 200.0),
                entry("鮨", "すし", 1595650, 99.0),
            ],
            20,
        );
        assert_eq!(out[0].1.len(), 1);
        assert!(
            !out[0].1[0].rare,
            "鮨 is an alternative spelling, not a rare one"
        );
    }

    /// `yomitan.rs` defaults a missing sequence to 0. Keyed on that alone, every entry in a
    /// dictionary that omits the field would collapse into a single result.
    #[test]
    fn entries_without_a_sequence_do_not_all_become_one_word() {
        let out = one_per_entry(
            vec![
                entry("いぬ", "いぬ", 0, 0.0),
                entry("ねこ", "ねこ", 0, 0.0),
                entry("とり", "とり", 0, 0.0),
            ],
            20,
        );
        assert_eq!(
            out.len(),
            3,
            "no sequence means fall back to the surface form"
        );
    }

    /// The cap counts entries, so a word with six spellings cannot crowd out five others.
    #[test]
    fn the_cap_counts_entries_rather_than_spellings() {
        let mut hits = vec![];
        for spelling in ["a", "b", "c", "d"] {
            hits.push(entry(spelling, "x", 111, 1.0));
        }
        hits.push(entry("other", "y", 222, 1.0));
        let out = one_per_entry(hits, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].0.term, "other");
    }
}
