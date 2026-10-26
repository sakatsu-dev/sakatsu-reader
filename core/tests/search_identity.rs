//! Moving the search path into `core` changed nothing about what it puts on the wire.
//!
//! # Why a frozen copy of the old code lives here
//!
//! `commands::search_words_uncached` and the query policy under it — grouping by dictionary entry,
//! the cap, alternative spellings, the kanji panel, the frequency sort — were private helpers in the
//! desktop shell. They moved to `core::search` so the policy could be called without constructing a
//! Tauri app. An extraction is only worth doing if it is provably behaviour-preserving, and "I read
//! both and they look the same" is the standard this branch has been burned by more than once.
//!
//! So [`frozen`] is the pre-move code, transcribed from `commands.rs` at `912f9a3` and **not to be
//! tidied** — reformatting it would defeat its only purpose. Its own DTOs are kept too, so the
//! comparison is over the **serialized bytes** rather than over two names for one type.
//!
//! # The intended differences, and only those
//!
//! Four Tauri state acquisitions become lent values — `state.0.now()`, `kanji.0.now()`,
//! `analyzer.0.now()`, `frequency.0.read()`. That is the change the extraction is *for*.
//!
//! Three calls reach helpers that moved in **earlier** slices and are pinned by their own identity
//! tests: `with_names` (`pipeline_identity.rs`) and the two lookup entry points
//! (`lookup_identity.rs`). The frozen copy calls the current ones rather than re-freezing them,
//! because re-proving another test's subject here would say nothing new and would rot twice.
//! **What this file is about is the search policy on top of them.**
//!
//! # What is compared
//!
//! The whole serialized response, for every query — so a difference in ordering, in which spelling
//! led its group, in the cap flag, or in a field nothing here thought to name still fails.
//!
//! The corpus is chosen so each named case actually occurs, because a corpus that only produces
//! ordinary words compares two implementations on the one case they cannot differ about:
//!
//! | Case | Query |
//! |---|---|
//! | Japanese single word | 市場 |
//! | Japanese phrase, so the contextual path runs | 市場で野菜を買いました |
//! | English with a gloss index | market |
//! | English with **no** gloss index | market, against dictionaries that have none |
//! | Grouping and alternative spellings | one entry with three spellings |
//! | The cap, and the extra group that reports it | a gloss shared by 21 entries |
//! | Frequency ties, and words the corpus never saw | a corpus that ranks two of them |
//! | Kanji details | a word whose characters are in the table |
//! | Empty and blank | "" and "   " |
//!
//! # It runs with nothing installed
//!
//! The fixture dictionaries are built here and UniDic is embedded, so this gates an ordinary
//! `cargo test` run — the pattern `pipeline_identity.rs` set and `study_identity.rs` and
//! `lookup_identity.rs` followed.
//!
//! **This module is deliberately temporary.** It goes when the migration is committed; keeping it
//! would resurrect the duplication the extraction removed.

use std::io::Write;
use std::path::{Path, PathBuf};

use sakatsu_core::analyzer::LinderaAnalyzer;
use sakatsu_core::dict::frequency::{Direction, Frequencies};
use sakatsu_core::dict::{Dictionary, KanjiDic};

/// The pre-move implementation, transcribed. Do not tidy.
#[allow(dead_code)]
mod frozen {
    use sakatsu_core::analyzer::{AnalyzeError, Analyzer};
    use sakatsu_core::dict::merge::WordView;
    use sakatsu_core::dict::Dictionary;
    use sakatsu_core::model::Token;
    use serde::Serialize;

    /// How many other spellings of one entry are worth showing beside it.
    const OTHER_SPELLINGS: usize = 6;

    /// How many English results fit on a page.
    const ENGLISH_RESULTS: usize = 20;

    /// One more than fits, so the extra group is what says the ceiling was reached.
    const ENGLISH_PROBE: usize = ENGLISH_RESULTS + 1;

    /// One reading a character has, and whether the searched word uses it.
    #[derive(Debug, Clone, Serialize)]
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

    /// sees one object rather than a wrapper it has to unwrap at every use.
    #[derive(Debug, Clone, Serialize)]
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
        pub ranking: Option<sakatsu_core::dict::VocabularyRanking>,
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

    /// One alternative spelling.
    #[derive(Debug, Clone, Serialize)]
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

    #[derive(Clone, PartialEq, Eq, Hash)]
    enum HitKey {
        Entry(i64),
        Surface(String, String),
    }

    impl HitKey {
        fn of(entry: &sakatsu_core::dict::yomitan::Entry) -> Self {
            if entry.sequence != 0 {
                HitKey::Entry(entry.sequence.abs())
            } else {
                HitKey::Surface(entry.term.clone(), entry.reading.clone())
            }
        }
    }

    fn analyse_query(
        preferred: &dyn Analyzer,
        built_in: &dyn Analyzer,
        query: &str,
    ) -> Result<sakatsu_core::model::Analysis, AnalyzeError> {
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

    fn japanese_tokens(analysis: &sakatsu_core::model::Analysis) -> usize {
        analysis
            .tokens
            .iter()
            .filter(|token| {
                !token.surface.trim().is_empty()
                    && token.surface.chars().any(sakatsu_core::kana::is_japanese)
            })
            .count()
    }

    fn distinct_entries(hits: &[sakatsu_core::dict::yomitan::Entry]) -> usize {
        hits.iter()
            .map(HitKey::of)
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    fn one_per_entry(
        hits: Vec<sakatsu_core::dict::yomitan::Entry>,
        cap: usize,
    ) -> Vec<(sakatsu_core::dict::yomitan::Entry, Vec<Form>)> {
        let mut groups: Vec<Vec<sakatsu_core::dict::yomitan::Entry>> = Vec::new();
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

    fn kanji_of(term: &str, reading: &str, dic: &sakatsu_core::dict::KanjiDic) -> Vec<KanjiView> {
        let marks = sakatsu_core::dict::align(term, reading, dic);
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
            out.push(view(character, kanji, used, irregular));
        }
        out
    }

    fn view(
        character: char,
        kanji: &sakatsu_core::dict::Kanji,
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
                let plain: String = reading
                    .chars()
                    .map(sakatsu_core::kana::to_hiragana_char)
                    .collect();
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

    fn other_spellings(open: &[Dictionary], headword: &str) -> Vec<Form> {
        for dictionary in open {
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

    fn cap_to_a_page<T>(mut groups: Vec<T>) -> (Vec<T>, bool) {
        let capped = groups.len() > ENGLISH_RESULTS;
        groups.truncate(ENGLISH_RESULTS);
        (groups, capped)
    }

    pub fn search_words_uncached(
        analyzer: &dyn Analyzer,
        built_in: &dyn Analyzer,
        open: &[Dictionary],
        kanji: &sakatsu_core::dict::KanjiDic,
        frequency: Option<&sakatsu_core::dict::frequency::Frequencies>,
        query: String,
    ) -> Result<Found, String> {
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

        if query.chars().any(sakatsu_core::kana::is_japanese) {
            // A single word answers from the built-in analyzer; a phrase pays for the preferred
            // one. See `analyse_query` for why the test is the contextual-phrase test.
            let analysis = analyse_query(analyzer, built_in, &query).map_err(|e| e.to_string())?;
            // A searched place name deserves the same rejoining a read one gets.
            let mut analysis = analysis;
            analysis.tokens = sakatsu_core::pipeline::Pipeline::new(open)
                .segmentation(&analysis.tokens)
                .0;
            let analysis = analysis;
            let contextual_phrase = analysis
                .tokens
                .iter()
                .filter(|token| {
                    !token.surface.trim().is_empty()
                        && token.surface.chars().any(sakatsu_core::kana::is_japanese)
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
                        sakatsu_core::lookup::in_context(&analysis.tokens, index, open)
                            .map_err(|error| error.to_string())
                    })
                    .transpose()?;
                let (view, ranking) = match contextual {
                    Some(contextual) => (contextual.word, Some(contextual.ranking)),
                    None => (
                        sakatsu_core::lookup::word(token, open)
                            .map_err(|error| error.to_string())?,
                        None,
                    ),
                };
                if !view.is_empty() {
                    // Here the query *is* Japanese, so `irregular` is a claim worth keeping: it
                    // says this word's reading cannot be split across its characters.
                    let chars = kanji_of(
                        &view.headword,
                        view.reading.as_deref().unwrap_or_default(),
                        kanji,
                    );
                    // No `forms`: this path looks a token up once and gets one entry back, so
                    // there is nothing that was collapsed to report.
                    let forms = other_spellings(open, &view.headword);
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
        let indexed: Vec<&Dictionary> = open
            .iter()
            .filter(|dictionary| dictionary.has_glosses())
            .collect();

        if indexed.is_empty() {
            return Ok(Found {
                words: Vec::new(),
                japanese: false,
                needs_gloss_index: !open.is_empty(),
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
        let mut hits: Vec<sakatsu_core::dict::yomitan::Entry> = Vec::new();
        for dictionary in indexed {
            // Checked here too: the `break` below leaves only the inner loop, so each extra
            // indexed dictionary used to append past the cap — two of them meant up to forty
            // results, each paying a full merge across every dictionary.
            if distinct_entries(&hits) >= ENGLISH_PROBE {
                break;
            }
            for hit in dictionary
                .search_glosses(&query, ENGLISH_PROBE)
                .map_err(|error| error.to_string())?
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
                sakatsu_core::lookup::word(&token, open).map_err(|error| error.to_string())?;

            // `irregular` is a claim about the reading of *what was typed*, and the page prints
            // it as one. What was typed here is English and this word is a result rather than the
            // query, so there is no such claim to make. The `here` marks stay: those say which
            // reading this word lends each character, which is the whole point of showing them
            // under it.
            let chars = kanji_of(
                &word.headword,
                word.reading.as_deref().unwrap_or_default(),
                kanji,
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
        let ranked_by = frequency.map(|corpus| {
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
}

/// A scratch root removed however the test ends — a guard rather than a call at the end, which
/// only runs when the test passes.
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(name: &str) -> Scratch {
    let root = std::env::temp_dir().join(format!(
        "sakatsu-search-identity-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("scratch");
    Scratch(root)
}

/// Import one Yomitan fixture from `rows`.
fn dictionary(root: &Path, name: &str, title: &str, rows: Vec<serde_json::Value>) -> PathBuf {
    let archive = root.join(format!("{name}.zip"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    for (file, value) in [
        (
            "index.json",
            serde_json::json!({"title": title, "format": 3}),
        ),
        ("term_bank_1.json", serde_json::json!(rows)),
    ] {
        zip.start_file(file, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(value.to_string().as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let container = root.join(name);
    drop(Dictionary::import(&archive, &container).unwrap());
    container
}

/// A term-bank row: `[term, reading, tags, rules, score, glosses, sequence, term_tags]`.
fn row(term: &str, reading: &str, gloss: &str, score: i64, sequence: i64) -> serde_json::Value {
    serde_json::json!([term, reading, "n", "", score, [gloss], sequence, ""])
}

/// Every query both implementations are asked.
const QUERIES: &[&str] = &[
    "",
    "   ",
    // Japanese, one word: the built-in analyzer answers and no context is consulted.
    "\u{5e02}\u{5834}",
    // Japanese, a phrase: the contextual path runs and each token carries a ranking.
    "\u{5e02}\u{5834}\u{3067}\u{91ce}\u{83dc}\u{3092}\u{8cb7}\u{3044}\u{307e}\u{3057}\u{305f}",
    // A word the dictionaries do not have at all.
    "\u{9f52}\u{9f52}\u{9f52}",
    // English, present in the gloss index, with grouping and alternative spellings behind it.
    "market",
    // English, shared by more entries than fit on a page: the cap and its report.
    "shared",
    // English, nothing matches.
    "aardvark",
];

/// The fixture: one dictionary with sequences and spellings, one crowded gloss, one kanji table.
struct Fixture {
    dictionaries: Vec<Dictionary>,
    kanji: KanjiDic,
    frequency: Frequencies,
    analyzer: LinderaAnalyzer,
}

fn fixture(root: &Path) -> Fixture {
    let mut rows = vec![
        // One entry, three spellings, so `one_per_entry` has something to collapse and
        // `other_spellings` has something to report. The scores are the real ones' shape: the
        // ordinary form at 200, a rarely-written form below zero.
        row(
            "\u{5e02}\u{5834}",
            "\u{3057}\u{3058}\u{3087}\u{3046}",
            "market",
            200,
            1008050,
        ),
        row(
            "\u{5e02}\u{5834}\u{2010}",
            "\u{3057}\u{3058}\u{3087}\u{3046}",
            "market",
            -101,
            1008050,
        ),
        row(
            "\u{30a4}\u{30c1}\u{30d0}",
            "\u{3044}\u{3061}\u{3070}",
            "market",
            99,
            -1008050,
        ),
        row(
            "\u{91ce}\u{83dc}",
            "\u{3084}\u{3055}\u{3044}",
            "vegetables",
            200,
            1010010,
        ),
        row(
            "\u{8cb7}\u{3046}",
            "\u{304b}\u{3046}",
            "to buy",
            200,
            1010020,
        ),
    ];
    // Twenty-one entries sharing one gloss: twenty fit, and the twenty-first is what says so.
    for index in 0..21 {
        rows.push(row(
            &format!("word{index}"),
            "reading",
            "shared gloss",
            20,
            2_000_000 + index,
        ));
    }
    let container = dictionary(root, "fixture", "Search identity fixture", rows);
    let held = Dictionary::open(&container).unwrap();
    held.index_secondary().unwrap();

    let kanji_table = root.join("kanjidic.tsv");
    std::fs::write(
        &kanji_table,
        // character, on, kun, meanings, strokes, grade
        "\u{5e02}\t\u{30b7}\t\u{3044}\u{3061}\tmarket\t5\t2\n\
         \u{5834}\t\u{30b8}\u{30e7}\u{30a6}\t\u{3070}\tplace\t12\t2\n\
         \u{91ce}\t\u{30e4}\t\u{306e}\tfield\t11\t2\n",
    )
    .unwrap();

    let mut frequency = Frequencies::new("Test corpus", Direction::Rank);
    // Two ranked and the rest unmeasured, so the tie-and-absence branch of the sort runs.
    frequency.insert("word19", None, 1.0);
    frequency.insert("word3", None, 2.0);

    Fixture {
        dictionaries: vec![Dictionary::open(&container).unwrap()],
        kanji: KanjiDic::load(&kanji_table),
        frequency,
        analyzer: LinderaAnalyzer::new().unwrap(),
    }
}

/// Both implementations, over one query, as the bytes they would put on the wire.
fn both(
    fixture: &Fixture,
    dictionaries: &[Dictionary],
    frequency: Option<&Frequencies>,
    query: &str,
) -> (
    Result<serde_json::Value, String>,
    Result<serde_json::Value, String>,
) {
    let old = frozen::search_words_uncached(
        &fixture.analyzer,
        &fixture.analyzer,
        dictionaries,
        &fixture.kanji,
        frequency,
        query.to_string(),
    )
    .map(|found| serde_json::to_value(found).expect("serialize"));

    let new = sakatsu_core::search::search(
        query,
        sakatsu_core::search::Resources {
            preferred: &fixture.analyzer,
            built_in: &fixture.analyzer,
            dictionaries,
            kanji: &fixture.kanji,
            frequency,
        },
    )
    .map_err(|error| error.to_string())
    .map(|found| serde_json::to_value(found).expect("serialize"));

    (old, new)
}

/// The whole response, for every query, byte for byte.
#[test]
fn the_extracted_search_puts_the_same_bytes_on_the_wire() {
    let root = scratch("whole");
    let fixture = fixture(&root);

    let mut reached_words = false;
    let mut reached_a_cap = false;
    let mut reached_forms = false;
    let mut reached_chars = false;
    let mut reached_ranking = false;

    for query in QUERIES {
        let (old, new) = both(
            &fixture,
            &fixture.dictionaries,
            Some(&fixture.frequency),
            query,
        );
        assert_eq!(old, new, "the two implementations differ on {query:?}");

        if let Ok(value) = &new {
            let words = value["words"].as_array().cloned().unwrap_or_default();
            reached_words |= !words.is_empty();
            reached_a_cap |= value["capped"] == serde_json::json!(true);
            reached_forms |= words
                .iter()
                .any(|word| !word["forms"].as_array().map(Vec::is_empty).unwrap_or(true));
            reached_chars |= words
                .iter()
                .any(|word| !word["chars"].as_array().map(Vec::is_empty).unwrap_or(true));
            reached_ranking |= words.iter().any(|word| !word["ranking"].is_null());
        }
    }

    // **The corpus is asserted, not assumed.** A fixture that stopped producing grouped spellings
    // or stopped reaching the cap would leave those branches compared on nothing at all, and this
    // test would go on passing while covering less.
    assert!(reached_words, "no query produced a result");
    assert!(reached_a_cap, "the cap was never reached");
    assert!(reached_forms, "no alternative spellings were ever reported");
    assert!(reached_chars, "the kanji panel was never populated");
    assert!(
        reached_ranking,
        "the contextual path never ran, so its ranking was never compared"
    );
}

/// The same, with no frequency corpus: the sort is skipped and `ranked_by` is absent.
#[test]
fn the_extracted_search_agrees_with_no_corpus_installed() {
    let root = scratch("no-corpus");
    let fixture = fixture(&root);
    for query in QUERIES {
        let (old, new) = both(&fixture, &fixture.dictionaries, None, query);
        assert_eq!(old, new, "the two implementations differ on {query:?}");
    }
    let english = both(&fixture, &fixture.dictionaries, None, "market")
        .1
        .unwrap();
    assert!(
        english["ranked_by"].is_null(),
        "nothing ordered these, and the response must not claim otherwise: {english}"
    );
}

/// **The gloss index missing, which is a different answer from "no results".**
///
/// One means the word is not in the dictionary; the other means nothing was looked at. Conflating
/// them is the specific mistake `needs_gloss_index` exists to prevent, so both implementations are
/// asked with a dictionary that has no gloss index at all.
#[test]
fn the_extracted_search_agrees_when_no_dictionary_has_a_gloss_index() {
    let root = scratch("no-gloss");
    let fixture = fixture(&root);

    let bare = dictionary(
        &root,
        "bare",
        "No gloss index",
        vec![row(
            "\u{5e02}\u{5834}",
            "\u{3057}\u{3058}\u{3087}\u{3046}",
            "market",
            200,
            1,
        )],
    );
    // A fresh import builds one; removing it is what an install interrupted before the backfill
    // leaves behind, and it is the state this branch is about.
    let generation = std::fs::read_to_string(bare.join("current")).unwrap();
    std::fs::remove_dir_all(bare.join(generation.trim()).join("gloss")).unwrap();
    let unindexed = vec![Dictionary::open(&bare).unwrap()];
    assert!(unindexed[0].needs_secondary_index());

    for query in QUERIES {
        let (old, new) = both(&fixture, &unindexed, Some(&fixture.frequency), query);
        assert_eq!(old, new, "the two implementations differ on {query:?}");
    }
    let english = both(&fixture, &unindexed, None, "market").1.unwrap();
    assert_eq!(
        english["needs_gloss_index"],
        serde_json::json!(true),
        "an English query with no index says so: {english}"
    );

    // And with no dictionaries at all it is `false`: nothing is missing an index, because there is
    // nothing. The two states read alike on screen and must not read alike on the wire.
    let (old, new) = both(&fixture, &[], None, "market");
    assert_eq!(old, new);
    assert_eq!(new.unwrap()["needs_gloss_index"], serde_json::json!(false));
}
