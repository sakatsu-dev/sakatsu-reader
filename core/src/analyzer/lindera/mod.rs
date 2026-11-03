//! The built-in analyzer: Lindera with UniDic, compiled into the binary.
//!
//! This is what 屠つ uses when Ichiran is not installed, and it is why Ichiran is
//! optional rather than required. There is nothing to set up — no sidecar, no
//! database, no download — because the dictionary is embedded in the executable.
//!
//! ## What it gives up
//!
//! Ichiran is still the better analyzer, and the difference is worth being explicit
//! about rather than discovering:
//!
//! | | Ichiran | this |
//! |---|---|---|
//! | tokenizing, readings, base forms | yes | yes |
//! | dictionary senses | from the analyzer | from the dictionaries instead |
//! | conjugation | a full `via` chain — te-form *over* causative *over* 食べる | rebuilt from UniDic's suffix tokens |
//! | compounds | 伝わってきます as one word with parts | rebuilt when UniDic marks the following verb 非自立可能 |
//! | readings | from a JMdict-derived database | **measured at 95.9% agreement** — see below |
//!
//! The senses gap closes itself: the merge policy already promotes a dictionary when
//! the analyzer has nothing to say (`project notes` §6), so with Jitendex installed the
//! vocabulary panel looks much the same. The remaining grammar gap is ambiguity:
//! UniDic tells us which suffixes attach, but not every distinction Ichiran can make
//! from its richer dictionary. Composition below therefore uses only structural tags
//! UniDic actually supplies and leaves uncertain cases conservative.
//!
//! ## Why UniDic, since 2026-08-27
//!
//! This was IPADIC until then, and readings were what moved it. Measured against Ichiran over
//! the 141-capture corpus, agreement on the reading of an aligned token went from **83.3% to
//! 95.9%** — 者 inside a word said もの had been read しゃ, and that class of homograph is
//! exactly what a segmentation dictionary last updated in 2007 cannot settle.
//!
//! It is not free. The embedded dictionary is **338 MB against IPADIC's 68 MB**, and it ships
//! inside the binary. Two things came with it that were not the reason for the change but are
//! worth having: boundaries conflict with Ichiran's less often (6.3% against 7.8%), and the
//! dictionary-form reading is now recorded rather than reconstructed from the written stem.
//!
//! It also segments **shorter**. UniDic is a short-unit-word dictionary, so it splits where
//! IPADIC did not — 15.9% of Ichiran's tokens are subdivided against 12.6% before. Those are
//! compatible splits rather than different cuts, which is why the conflict number fell while
//! this one rose.

use std::sync::Mutex;

use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;

use crate::analyzer::assist::{self, Advice, Advisor, Assistance, ProviderId};
use crate::analyzer::{AnalyzeError, Analyzer};
use crate::kana::to_hiragana;
use crate::model::{
    Analysis, Conjugation, ConstructionIdentity, LexicalRole, LineBreak, Prop, Token,
};

/// The UniDic feature columns this uses, addressed by index.
///
/// **UniDic rather than IPADIC since 2026-08-27, and readings are why.** IPADIC is a
/// segmentation dictionary carrying one reading per entry, last updated in 2007; measured over
/// the 141-capture corpus it read 者 as しゃ where the word was もの, and 撮り as とり only by
/// luck. UniDic records the pronunciation of the *token in context* separately from the
/// lexeme's, which is exactly the distinction those words turn on.
///
/// The layout is not IPADIC's. Columns 0, 1, 4 and 5 happen to line up; the two that matter
/// most do not, and the values in every column differ — `五段・ラ行` became `五段-ラ行`,
/// `非自立` became `非自立可能`, and `サ変接続` moved from column 1 to column 2.
mod field {
    /// 品詞大分類 — noun, verb, particle, auxiliary, and so on.
    pub const POS: usize = 0;
    /// 品詞中分類 — distinguishes 一般 from 非自立可能, which drives compounding.
    pub const SUBPOS: usize = 1;
    /// 品詞小分類 — where UniDic files サ変可能, which IPADIC put one column earlier.
    pub const SUBPOS3: usize = 2;
    /// 活用型 — the conjugation class, e.g. 五段-ラ行.
    pub const CONJUGATION_TYPE: usize = 4;
    /// 活用形 — which form this token is in, e.g. 連用形-促音便.
    pub const CONJUGATION_FORM: usize = 5;
    /// 発音形出現形 — how this token is *pronounced*, which is not how it is *spelled* in
    /// kana. 勉強 comes back as ベンキョー and は as ワ: true of the sound, wrong as ruby.
    /// Used only as a fallback when the spelling cannot be reconstructed.
    pub const PRONUNCIATION: usize = 9;
    /// 書字形基本形 — the dictionary form as written. Not 語彙素 (column 7), which is an
    /// abstract lexeme: 撮る's is 取る, and offering that as the dictionary form would send
    /// the learner to the wrong entry.
    pub const BASE_FORM: usize = 10;
    /// 語彙素読み — the dictionary form's reading **as spelled in kana**, which IPADIC never
    /// supplied and this adapter used to reconstruct by hand. Not 発音形基本形 (column 11),
    /// which is the phonetic form: 学生 is ガクセイ here and ガクセー there.
    ///
    /// **It is the reading of [`LEMMA`], not of [`BASE_FORM`]**, and those are not always the
    /// same word. See [`spell_surface`].
    pub const LEMMA_READING: usize = 6;
    /// 語彙素 — the abstract lexeme.
    ///
    /// **Read only to find out when [`LEMMA_READING`] is describing a different word from
    /// [`BASE_FORM`]**, never offered as a dictionary form: 撮る's lexeme is 取る, and handing
    /// that to a reader would send them to the wrong entry. That is why [`BASE_FORM`] remains
    /// the dictionary form and this column only qualifies the reading.
    pub const LEMMA: usize = 7;
}

/// UniDic writes an absent field as a bare asterisk.
const ABSENT: &str = "*";

/// Lindera over the embedded UniDic dictionary.
pub struct LinderaAnalyzer {
    /// **One reader at a time, through this instance.** lindera's `segment` takes `&self`, so
    /// the lock is not what the borrow checker asks for; it is what every user of the instance has
    /// relied on since lindera 4, and with [`shared`] behind the built-in analyzer and every advice
    /// question alike, it is also what serialises them. `pieces` scopes it so nothing nested waits
    /// on it.
    ///
    /// **The segmenter itself since lindera 6**, which dropped the `Tokenizer` wrapper from the
    /// core crate. This adapter never used its character or token filters, so the segmenter's own
    /// output is exactly what the tokenizer returned.
    segmenter: Mutex<Segmenter>,
    /// Which dictionary's columns the details follow, and what resolves them.
    schema: Schema,
}

static SHARED: std::sync::OnceLock<Option<LinderaAnalyzer>> = std::sync::OnceLock::new();

/// The embedded parser, loaded once per process and shared.
///
/// **One instance, whoever asks.** Each instance loads its own copy of the embedded dictionary,
/// and the advice to the Ichiran path ([`crate::analyzer::assist`]) and the built-in analyzer are
/// the same parser over the same compiled-in dictionary. Loaded where it is first asked for: for
/// the shell, that is where the built-in analyzer has always been built.
///
/// A load that fails is remembered for the process, exactly as before: the dictionary is compiled
/// in, so a failure is a broken build, and retrying it per passage would be the same failure again.
pub fn shared() -> Option<&'static LinderaAnalyzer> {
    SHARED.get_or_init(|| LinderaAnalyzer::new().ok()).as_ref()
}

/// Whether [`shared`] has been resolved, so a request can say whether it was the one that paid.
pub fn shared_loaded() -> bool {
    SHARED.get().is_some()
}

/// [`shared`] as an analyzer a caller can hold — the shell's built-in analyzer, when it is unified.
pub fn shared_analyzer() -> Option<std::sync::Arc<dyn Analyzer>> {
    shared().map(|parser| std::sync::Arc::new(Shared(parser)) as std::sync::Arc<dyn Analyzer>)
}

/// The shared instance behind a handle that can be held as `Arc<dyn Analyzer>`.
struct Shared(&'static LinderaAnalyzer);

impl Analyzer for Shared {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.0.analyze(text)
    }

    fn analyze_in(
        &self,
        text: &str,
        assistance: &Assistance<'_>,
    ) -> Result<crate::analyzer::Traced, AnalyzeError> {
        self.0.analyze_in(text, assistance)
    }

    fn engine(&self) -> &str {
        self.0.engine()
    }
}

/// One UniDic token before attached morphology is put back together.
///
/// `Token` deliberately contains no analyzer-native fields. These stay private to the
/// adapter long enough to decide boundaries, then disappear at the normalized contract.
#[derive(Clone)]
struct Piece {
    token: Token,
    pos: String,
    subpos: String,
    /// 品詞小分類. Only サ変可能 is read from it, and only by [`is_suru_start`].
    subpos3: String,
    class: String,
    form: String,
}

/// What one dictionary entry establishes about a token — the contract every dictionary adapter
/// meets before the shared grouping logic sees it.
///
/// **Each field is either the dictionary's own data or absent** (empty). The analyzer layer never
/// consults `crate::dict` to fill a gap: a field a dictionary does not establish stays empty, and
/// the merge policy downstream may adopt one from a real dictionary, visibly, rather than this
/// layer inventing it.
///
/// - `dict_form` is the **lookup identity**, as written. A normalised spelling is never put here.
/// - `reading` is how *this token* is spelled in kana.
/// - `lemma_reading` is the dictionary form's reading — never taken from the surface reading of an
///   inflected token.
struct Established {
    pos: String,
    subpos: String,
    subpos3: String,
    class: String,
    form: String,
    dict_form: String,
    reading: String,
    lemma_reading: String,
}

/// Which dictionary's columns a token's details follow.
enum Schema {
    /// The embedded UniDic: the mapping this adapter has always made, unchanged.
    UniDic,
    /// SudachiDict, loaded from disk, with the side table that resolves its dictionary-form IDs.
    Sudachi(SudachiForms),
}

impl Schema {
    fn establish(&self, surface: &str, details: &[String]) -> Established {
        match self {
            Schema::UniDic => establish_unidic(surface, details),
            Schema::Sudachi(forms) => establish_sudachi(surface, details, forms),
        }
    }
}

/// UniDic's columns, exactly as `pieces` read them before dictionaries had adapters.
///
/// Moved here verbatim; the reader-equivalence gate holds it to that.
fn establish_unidic(surface: &str, details: &[String]) -> Established {
    let surface = surface.to_string();
    let field = |index: usize| -> &str {
        details
            .get(index)
            .map(String::as_str)
            .filter(|value| *value != ABSENT)
            .unwrap_or("")
    };

    let pos = field(field::POS).to_string();
    let subpos = field(field::SUBPOS).to_string();
    let subpos3 = field(field::SUBPOS3).to_string();
    let class = field(field::CONJUGATION_TYPE).to_string();
    let form = field(field::CONJUGATION_FORM).to_string();
    let base_form = field(field::BASE_FORM);
    let dict_form = if base_form.is_empty() {
        surface.clone()
    } else {
        base_form.to_string()
    };
    let recorded_lemma_reading = to_hiragana(field(field::LEMMA_READING));
    let recorded_lemma_reading = modern_reading(&surface, &recorded_lemma_reading)
        .map(str::to_string)
        .unwrap_or(recorded_lemma_reading);
    // **Spelled back onto the surface, not read off it.** UniDic records the
    // lexeme's kana spelling and this token's pronunciation, and neither is this
    // token's spelling: 勉強する gives ベンキョウ for the lexeme and ベンキョー for
    // the sound. The written difference between the surface and the dictionary
    // form says which kana changed, the same alignment `lemma_reading` uses in the
    // other direction.
    let reading = surface_reading(
        &surface,
        &spell_surface(
            &surface,
            &dict_form,
            &recorded_lemma_reading,
            &class,
            field(field::LEMMA),
            field(field::PRONUNCIATION),
        )
        .unwrap_or_else(|| field(field::PRONUNCIATION).to_string()),
    );
    // **Taken, not reconstructed.** IPADIC had no base-form reading, so this used
    // to rebuild one from the surface, the class and the stem — a good guess that
    // was still a guess. UniDic records it.
    let lemma_reading = if recorded_lemma_reading.is_empty() {
        lemma_reading(&surface, &reading, &dict_form, &class)
    } else {
        recorded_lemma_reading.clone()
    };
    Established {
        pos,
        subpos,
        subpos3,
        class,
        form,
        dict_form,
        reading,
        lemma_reading,
    }
}

/// SudachiDict's columns, after the display surface (`lindera-sudachidict`'s schema).
mod sudachi_field {
    pub const POS: usize = 1;
    pub const SUBPOS: usize = 2;
    /// 品詞小分類 — where SudachiDict, like UniDic, files サ変可能.
    pub const SUBPOS3: usize = 3;
    pub const CONJUGATION_TYPE: usize = 5;
    pub const CONJUGATION_FORM: usize = 6;
    /// 読み — this token's reading as spelled in katakana.
    pub const READING: usize = 7;
    /// 辞書形ID — a line of the source lexicons, or `*` for an entry that is its own dictionary
    /// form. Resolved through [`super::SudachiForms`].
    pub const DICTIONARY_FORM_ID: usize = 9;
}

/// SudachiDict's dictionary-form IDs, resolved: id → (dictionary form as written, its reading).
///
/// Built from the source lexicons in their build order — small, core, notcore — where the ID is
/// the line's index. Lindera keeps the ID as a raw string and cannot resolve it itself. Checked
/// when the table is built (`scripts/sudachidict/dictionary_forms.py`): every inflected entry with
/// an ID resolves to an entry in 終止形 — 21 across a conjugation class, classical and variant forms
/// pointing at the modern dictionary form.
pub(crate) struct SudachiForms(std::collections::HashMap<u32, (String, String)>);

impl SudachiForms {
    /// Read `dictionary_forms.tsv`: `id <TAB> form <TAB> reading`, `#` comments allowed.
    fn load(path: &std::path::Path) -> Result<SudachiForms, AnalyzeError> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            AnalyzeError::EngineUnavailable(format!("{}: {error}", path.display()))
        })?;
        let mut forms = std::collections::HashMap::new();
        for line in text
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
        {
            let mut parts = line.split('\t');
            let (Some(id), Some(form), Some(reading)) = (parts.next(), parts.next(), parts.next())
            else {
                return Err(AnalyzeError::EngineUnavailable(format!(
                    "{}: malformed line {line:?}",
                    path.display()
                )));
            };
            let id: u32 = id.parse().map_err(|_| {
                AnalyzeError::EngineUnavailable(format!("{}: bad id {id:?}", path.display()))
            })?;
            forms.insert(id, (form.to_string(), reading.to_string()));
        }
        Ok(SudachiForms(forms))
    }
}

/// SudachiDict's columns onto the contract.
///
/// - **The dictionary form is resolved, not read off the surface.** An entry whose ID is `*` is
///   its own dictionary form — unless it is inflected and not in 終止形, which the source data
///   holds for eight symbols (＋ − × ÷ …) and nothing else; there, the form is absent.
/// - **The normalised form (正規化表記) is never used.** It is a spelling normalisation (附属 →
///   付属), not a lookup identity.
/// - **The surface reading is SudachiDict's own 読み**, not reconstructed.
fn establish_sudachi(surface: &str, details: &[String], forms: &SudachiForms) -> Established {
    let field = |index: usize| -> &str {
        details
            .get(index)
            .map(String::as_str)
            .filter(|value| *value != ABSENT && !value.is_empty())
            .unwrap_or("")
    };
    let pos = field(sudachi_field::POS).to_string();
    let class = field(sudachi_field::CONJUGATION_TYPE).to_string();
    let form = field(sudachi_field::CONJUGATION_FORM).to_string();
    // **キゴウ is not a reading, and neither is a symbol's own spelling.** SudachiDict writes
    // キゴウ in the reading column of symbols (♥, 『, 〜) as a placeholder — taken literally it put
    // きごう over a heart — and writes punctuation as itself (、 reads 、). Both are absent, as they
    // are from UniDic. A symbol with a reading of its own, such as ％ パーセント, keeps it.
    let recorded = match field(sudachi_field::READING) {
        recorded
            if matches!(pos.as_str(), "補助記号" | "記号")
                && (recorded == "キゴウ" || recorded == surface) =>
        {
            ""
        }
        recorded => recorded,
    };
    // Through the same rule the UniDic path uses: kana keeps its own spelling (リング, not
    // りんぐ), an absent reading stays absent (、 has none), everything else becomes hiragana.
    let reading = surface_reading(surface, recorded);
    let id = field(sudachi_field::DICTIONARY_FORM_ID);
    let (dict_form, lemma_reading) = if id.is_empty() {
        let inflected = !form.is_empty() && !form.starts_with("終止形");
        if inflected {
            (String::new(), String::new())
        } else {
            (surface.to_string(), to_hiragana(recorded))
        }
    } else {
        id.parse::<u32>()
            .ok()
            .and_then(|id| forms.0.get(&id))
            .map(|(form, reading)| (form.clone(), to_hiragana(reading)))
            .unwrap_or_default()
    };
    Established {
        pos,
        subpos: field(sudachi_field::SUBPOS).to_string(),
        subpos3: field(sudachi_field::SUBPOS3).to_string(),
        class,
        form,
        dict_form,
        reading,
        lemma_reading,
    }
}

impl LinderaAnalyzer {
    /// Load the embedded dictionary.
    ///
    /// Fails only if the compiled-in dictionary is unreadable, which would mean a
    /// broken build rather than a missing install — but it is still reported rather
    /// than panicking, so a bad build degrades like anything else.
    pub fn new() -> Result<LinderaAnalyzer, AnalyzeError> {
        let dictionary = load_dictionary("embedded://unidic").map_err(|error| {
            AnalyzeError::EngineUnavailable(format!("embedded dictionary: {error}"))
        })?;
        // **The unknown-word ladder off, as lindera 4 had it.** lindera 6 turned on, by default,
        // a length ladder for unknown words that honours `char.def`'s LENGTH field; lindera 4
        // did not. This upgrade is held to changing nothing a reader sees
        // (`tests/reader_equivalence.rs`, 0 of 4,296 outcomes differing). Measured with the
        // ladder *on*, that workload also showed no difference — so it is kept off not because
        // it was seen to matter, but because text outside those 1,074 passages could still be
        // cut differently, and nothing measured asks for the change.
        let segmenter = Segmenter::new(Mode::Normal, dictionary, None).unknown_word_ladder(false);
        Ok(LinderaAnalyzer {
            segmenter: Mutex::new(segmenter),
            schema: Schema::UniDic,
        })
    }

    /// SudachiDict 20260723 from `dir`: Lindera's compiled dictionary, and the
    /// `dictionary_forms.tsv` side table beside it.
    ///
    /// **Its own instance, never the shared one**, and its own engine name and identity, so
    /// nothing it says is ever served as UniDic's answer or cached under UniDic's key. Lindera's
    /// defaults are kept for it — this dictionary has no earlier behaviour to preserve.
    pub fn sudachi(dir: &std::path::Path) -> Result<LinderaAnalyzer, AnalyzeError> {
        let dictionary = lindera::dictionary::load_fs_dictionary(dir).map_err(|error| {
            AnalyzeError::EngineUnavailable(format!("SudachiDict at {}: {error}", dir.display()))
        })?;
        let forms = SudachiForms::load(&dir.join("dictionary_forms.tsv"))?;
        Ok(LinderaAnalyzer {
            segmenter: Mutex::new(Segmenter::new(Mode::Normal, dictionary, None)),
            schema: Schema::Sudachi(forms),
        })
    }
}

impl LinderaAnalyzer {
    /// Whether the parser reads straight through the join of two lines.
    ///
    /// **Joined with nothing between them**, which is the only way to ask: with the break still in
    /// place Lindera cuts its lattice at it and could never read across.
    ///
    /// Asked of the *composed* tokens rather than the raw lattice, and the difference decides the
    /// answer: `６人宿泊させ` / `て欲しい` segments as `させ` then `て`, whose boundary falls
    /// exactly on the join, so nothing straddles it. It is `compose` that folds the two into one
    /// conjunctive verb spanning the break — and that composed view is what `translate::passage`
    /// measured.
    ///
    /// **Not recursive**, though it calls back into `analyze`: the joined string has no newline in
    /// it, so `lines::mark` returns immediately and never probes again. The segmenter lock is
    /// already released by the time this runs.
    pub(crate) fn straddles(&self, before: &str, after: &str) -> bool {
        if before.is_empty() || after.is_empty() {
            return false;
        }
        let join = before.len();
        let Ok(analysis) = self.analyze(&format!("{before}{after}")) else {
            return false;
        };
        crate::analyzer::lines::straddled(
            join,
            analysis
                .tokens
                .iter()
                .filter_map(|token| Some((token.start?, token.end?))),
        )
    }
    /// The tokenizer's own output, converted to [`Piece`] and **nothing else**.
    ///
    /// Extracted from [`LinderaAnalyzer::analyze`] so the stages after it are observable. They
    /// were not, and that mattered: `analyze` returns
    /// `attach_terminal_kke(rejoin_fixed_expressions(compose(pieces)))`, so what a caller calls
    /// "raw Lindera" is already this project's grouping logic sitting on top of UniDic. A
    /// comparison that labelled a difference "the analyzer" could not tell the two apart, and
    /// read as exonerating the composition it was actually measuring.
    ///
    /// Behaviour is unchanged — this is the same code in a different place, and `analyze` is its
    /// only non-test caller.
    fn pieces(&self, text: &str) -> Result<Vec<Piece>, AnalyzeError> {
        // **Scoped, and that is load-bearing.** `lines::mark` needs to tokenize again to ask
        // whether the parser reads through a line break, and `self.segmenter` is a plain `Mutex`
        // — not reentrant. Holding this guard across that call deadlocks the app on the first
        // multi-line capture, and nothing in the suite used one until now.
        Ok({
            let segmenter = self
                .segmenter
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut raw = segmenter
                .segment(std::borrow::Cow::Borrowed(text))
                .map_err(|error| AnalyzeError::BadOutput(error.to_string()))?;

            raw.iter_mut()
                .map(|token| {
                    let surface = token.surface.to_string();
                    let details: Vec<String> =
                        token.details().iter().map(ToString::to_string).collect();
                    let Established {
                        pos,
                        subpos,
                        subpos3,
                        class,
                        form,
                        dict_form,
                        reading,
                        lemma_reading,
                    } = self.schema.establish(&surface, &details);
                    // UniDic knows these roles even though Lindera has no dictionary senses to
                    // carry them in. Keep only the distinctions with a measured downstream use;
                    // this is normalized evidence, not an analyzer-native POS mirror.
                    //
                    // 名詞 joined them when grouping needed the left constituent of a compound
                    // **established** rather than un-refuted: without it, a word UniDic could not
                    // classify was indistinguishable from a noun, and an adverb or a connector
                    // standing before an adjective would have been joined to it.
                    let lexical_role = match pos.as_str() {
                        "代名詞" => Some(LexicalRole::Pronoun),
                        "接尾辞" => Some(LexicalRole::Suffix),
                        "名詞" => Some(LexicalRole::Noun),
                        _ => None,
                    };

                    Piece {
                        pos,
                        subpos,
                        subpos3,
                        class: class.clone(),
                        form: form.clone(),
                        token: Token {
                            // Filled in by `lines::mark` after the whole passage is composed.
                            line_break: LineBreak::None,
                            // Lindera reports byte offsets into the text it was given, so tokens
                            // arrive already grounded — no scanning back over the source.
                            start: Some(token.byte_start),
                            end: Some(token.byte_end),
                            conjugation: conjugation(&dict_form, &lemma_reading, &class, &form),
                            // Nothing: UniDic supplies morphology, not dictionary senses. The
                            // merge policy promotes a real dictionary in their place.
                            senses: Vec::new(),
                            // A base-form reading would need the dictionary form looked up again;
                            // leaving it empty lets the merge policy adopt one from a dictionary
                            // rather than inventing it here.
                            lemma_reading,
                            lexical_role,
                            surface,
                            dict_form,
                            reading,
                            components: Vec::new(),
                            // Decided later, from the source, by `analyzer::mark_selectable`.
                            selectable: false,
                            constructions: Vec::new(),
                            alternatives: Vec::new(),
                            // UniDic scores nothing and romanizes nothing. Left absent rather than
                            // filled with a stand-in, so a rule keyed on either simply does not fire
                            // here instead of firing on a number this analyzer never reported.
                            confidence: None,
                            romaji: None,
                            // Only `dict::names` sets this, and only after the segmenter has been second-guessed.
                            reading_unverified: false,
                            reading_ambiguous: false,
                        },
                    }
                })
                .collect()
        })
    }
}

impl LinderaAnalyzer {
    /// Every instance reads the same compiled-in dictionary, so every instance gives the same
    /// answer to the same question — which is what lets one serve another's.
    pub const IDENTITY: ProviderId = ProviderId("lindera over embedded://unidic");

    /// The SudachiDict instance's identity: a different dictionary, so a different provider.
    pub const SUDACHI_IDENTITY: ProviderId = ProviderId("lindera over sudachidict-20260723");

    /// The analysis before line marking: everything but the breaks.
    fn unmarked(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        // Folded before anything is tokenized: a bare carriage return is not whitespace to
        // the former IPADIC path and could otherwise become a token of its own, inside a sentence range and
        // so onto an Anki card. See `lines::normalise_breaks`.
        let text = crate::analyzer::lines::normalise_breaks(text);
        let text = text.trim();
        if text.is_empty() {
            return Err(AnalyzeError::EmptyInput);
        }

        let pieces = self.pieces(text)?;

        Ok(Analysis {
            execution: None,
            text: text.to_string(),
            tokens: attach_terminal_kke(rejoin_fixed_expressions(compose(pieces))),
        })
    }
}

impl Analyzer for LinderaAnalyzer {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        let mut analysis = self.unmarked(text)?;
        // The lock is released above, so this may tokenize again.
        crate::analyzer::lines::mark(&mut analysis, &|before, after| {
            self.straddles(before, after)
        });
        Ok(analysis)
    }

    /// The passage, from the request if this parser has already read it.
    ///
    /// **Under this parser's identity**, so an answer is reused only where the same parser gave
    /// it: the passage an eager pass or the tie-break read is the passage a fallback receives,
    /// and nothing another kind of analyzer said can stand in for it. A question that failed was
    /// never kept, so a fallback after one runs its own analysis.
    fn analyze_in(
        &self,
        text: &str,
        assistance: &Assistance<'_>,
    ) -> Result<crate::analyzer::Traced, AnalyzeError> {
        let text = crate::analyzer::lines::normalise_breaks(text);
        match assistance.ask_of(assist::Stage::Passage, text.trim(), self) {
            Advice::Answered(analysis) => Ok(crate::analyzer::Traced {
                raw: None,
                analysis: (*analysis).clone(),
            }),
            Advice::Failed(error) => Err(error),
            // `ask_of` puts the question to this parser, which is always there.
            Advice::Unavailable => self.analyze_traced(&text),
        }
    }

    fn engine(&self) -> &str {
        match self.schema {
            Schema::UniDic => "lindera",
            Schema::Sudachi(_) => "lindera-sudachi",
        }
    }
}

impl Advisor for LinderaAnalyzer {
    fn identity(&self) -> ProviderId {
        match self.schema {
            Schema::UniDic => LinderaAnalyzer::IDENTITY,
            Schema::Sudachi(_) => LinderaAnalyzer::SUDACHI_IDENTITY,
        }
    }

    /// [`Analyzer::analyze`], with the joined-line questions asked of this parser through the
    /// request, so a passage read twice does not read its line joins twice.
    fn read(&self, text: &str, assistance: &Assistance<'_>) -> Result<Analysis, AnalyzeError> {
        let mut analysis = self.unmarked(text)?;
        crate::analyzer::lines::mark_in(&mut analysis, assistance, Some(self));
        Ok(analysis)
    }
}

/// Readings where UniDic's lexeme is not the one a modern reader meets.
///
/// **UniDic holds one 私 pronoun.** Its lexeme is `私-代名詞` and every column says ワタクシ —
/// 語彙素読み, 発音形出現形 and 発音形基本形 alike. There is no ワタシ entry for the lattice to
/// prefer, so neither cost tuning nor reading a different column reaches this: asked about 私,
/// the dictionary answers わたくし and has nothing else to offer.
///
/// That is not wrong. It is formal, and it is the wrong default for what this app reads.
/// Measured against Ichiran over 1,858 sentences, 私 was the largest single reading
/// disagreement — 129 occurrences, 24% of every reading the two analyzers differed on — and
/// Ichiran read it わたし every time.
///
/// **The token stays marked ambiguous**, because it is: わたし, わたくし, あたし and わし are all
/// live in fiction, and `dict::readings` flags it from the dictionary either way. This decides
/// which reading is offered first, not how confident the app is about it.
///
/// **日本 was excluded, and then the evidence arrived.** It sat one row below 私 at 14
/// occurrences, and it was kept out on the grounds that both readings are current and
/// broadcasters disagree — a fair call on Ichiran agreement alone, since agreeing with Ichiran
/// says nothing about who is right. KWDLC does: annotated in the JUMAN lineage, sharing a
/// dictionary with neither analyzer, it reads 日本 as にほん **34 times out of 34**. That is not
/// a preference, and the entry is here now.
///
/// The bar for a third entry is the same: an independent source, counted, not a feeling about
/// which reading sounds right.
fn modern_reading(surface: &str, recorded: &str) -> Option<&'static str> {
    match (surface, recorded) {
        ("私", "わたくし") => Some("わたし"),
        ("日本", "にっぽん") => Some("にほん"),
        _ => None,
    }
}

/// The reading that drives furigana.
///
/// UniDic always writes readings in katakana, so 食べる comes back as タベル and has to
/// be turned into たべる. But a word that is *already* kana keeps its own spelling:
/// converting コーヒー to こーひー would put the wrong furigana on the page, and
/// converting ひらがな achieves nothing.
fn surface_reading(surface: &str, reading: &str) -> String {
    if reading.is_empty() {
        return String::new();
    }
    // **Kana is its own reading; a digit is not.** This used to keep the surface whenever it held
    // no kanji, which is not the rule the comment described — １ carries no kanji and is
    // emphatically not written as it is read. UniDic supplies イチ for it and this was discarding
    // it, so a learner met every number, and every ％, with no furigana at all. Measured against
    // KWDLC: numerals were the second-largest class of reading disagreement after 日本, about
    // fifty in a 1,784-sentence sample, and every one of them was this.
    if surface.chars().all(reads_as_written) {
        return surface.to_string();
    }
    to_hiragana(reading)
}

/// A character that already spells its own reading.
///
/// Kana, plus the marks that only occur inside a kana run: the ー length mark, the ・ separator
/// and the iteration marks. Deliberately *not* digits, Latin letters or symbols — those look as
/// though they need no reading and do.
fn reads_as_written(character: char) -> bool {
    crate::kana::is_kana(character) || matches!(character, 'ー' | '・' | 'ゝ' | 'ゞ' | 'ヽ' | 'ヾ')
}

/// Spell this token's surface in kana, from the dictionary form's spelling.
///
/// The written forms say which kana changed: 伝わっ against 伝わる shares 伝わ, so the lemma's
/// つたわる loses る and gains っ to give つたわっ. An uninflected word shares everything and
/// comes back unchanged, which is the common case and the one 勉強 needs.
///
/// `None` when the alignment does not hold — an irregular whose written stem changes, say —
/// so the caller can fall back rather than be handed a confident wrong spelling.
fn spell_surface(
    surface: &str,
    dict_form: &str,
    lemma_reading: &str,
    class: &str,
    lemma: &str,
    pronunciation: &str,
) -> Option<String> {
    if lemma_reading.is_empty() {
        return None;
    }
    // **The irregulars change the stem's sound without changing its spelling**, which is
    // precisely what this reconstruction cannot see. 来る is こ / き / く under one written 来,
    // so aligning 来て against 来る takes くる, drops る and produces くて for a word that is
    // きて. する is the same shape. Both have a correct pronunciation recorded, so refusing
    // here hands the caller the right answer instead of a derived wrong one.
    if class.starts_with("カ行変格") || class.starts_with("サ行変格") {
        return None;
    }
    let spelled = align_to_surface(surface, dict_form, lemma_reading)?;

    // **The alignment above is sound only where the lexeme and the written base are the same
    // word**, and that is what this asks. 語彙素読み is the reading of 語彙素, while the ruler it
    // is measured against is 書字形基本形. For an ordinary word those are one spelling and the
    // result is right — which is the overwhelmingly common case, and the reason this is a
    // qualification on the answer above rather than a replacement for it.
    //
    // Where they part, the reading being aligned belongs to another word. 走れ is 書字形基本形
    // 走れる with 語彙素 走る, so ハシル loses one mora against 走れる's る and produces はし;
    // 突っ込ん is 突っ込む with lexeme 突き込む, giving つきこん. Both are the arithmetic working
    // perfectly on inputs that describe different words.
    if lemma.is_empty() || same_lexeme(lemma, dict_form) {
        return Some(spelled);
    }
    let sounded = to_hiragana(pronunciation);
    if sounded.is_empty() || sounded == spelled {
        return Some(spelled);
    }
    Some(if pronunciation_is_spellable(&spelled, &sounded) {
        sounded
    } else {
        spelled
    })
}

/// The written forms say which kana changed; apply that to a base reading.
///
/// 伝わっ against 伝わる shares 伝わ, so the base's つたわる loses る and gains っ to give つたわっ.
/// An uninflected word shares everything and comes back unchanged, which is the common case and
/// the one 勉強 needs. `None` when the alignment does not hold — an irregular whose written stem
/// changes, say — so the caller can fall back rather than be handed a confident wrong spelling.
///
/// Split out of [`spell_surface`] so this arithmetic is stated once, and the separate question of
/// **which base reading it should be given** is answered where the evidence for it lives.
fn align_to_surface(surface: &str, dict_form: &str, base_reading: &str) -> Option<String> {
    if surface == dict_form {
        return Some(base_reading.to_string());
    }
    let shared = surface
        .chars()
        .zip(dict_form.chars())
        .take_while(|(left, right)| left == right)
        .count();
    let surface_tail: String = to_hiragana(&surface.chars().skip(shared).collect::<String>());
    let lemma_tail: String = to_hiragana(&dict_form.chars().skip(shared).collect::<String>());
    base_reading
        .strip_suffix(&lemma_tail)
        .map(|stem| format!("{stem}{surface_tail}"))
}

/// Whether two UniDic spellings name the same word, for the purpose of trusting 語彙素読み.
///
/// Only a width or case difference is forgiven, because neither is a difference of word: the
/// letter H arrives as 語彙素 Ｈ against 書字形基本形 h, and reading that as two words would send
/// an ordinary initial to the pronunciation column and bring back えっち.
fn same_lexeme(lemma: &str, dict_form: &str) -> bool {
    if lemma == dict_form {
        return true;
    }
    let fold = |text: &str| -> String {
        // **語彙素 carries a sense disambiguator on a homograph**, written after a hyphen: 私 is
        // 私-代名詞, 引く is 引く-他動詞, ガラス is ガラス-glass. That is UniDic telling two entries
        // apart, not a second spelling, and reading it as one sent 私 to the pronunciation column
        // and brought back わたくし — undoing a substitution `modern_reading` made on 129 measured
        // occurrences.
        text.split('-')
            .next()
            .unwrap_or(text)
            .chars()
            // `U+FF01`-`U+FF5E` is the ASCII range's wide twin, offset by `0xFEE0` — the fold
            // `translate::fugumt` and `ocr::score` already use.
            .map(|c| match c {
                '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
                _ => c,
            })
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    fold(lemma) == fold(dict_form)
}

/// Whether the pronunciation can be read as a *spelling*, where the lexeme reading cannot.
///
/// 発音形出現形 records how the token sounded, and these are the three ways that is true of the
/// sound and wrong as ruby. **Each refusal is here because a sweep found it**: over the 26,007
/// Tatoeba sentences Jitendex bundles, the lexeme reading and the pronunciation disagree on 176
/// distinct kanji-bearing tokens whose lexeme differs from their written base, and these are every
/// class among them where the disagreement is *not* the mismatch this is repairing.
///
/// **Two cases in that sweep are still wrong afterwards, and are recorded rather than papered
/// over.** しかめ面 becomes しかめずら where しかめつら is standard, and the loan 再見 becomes
/// つあいちえん. Both are single occurrences in 26,007 sentences, both are voicing or small-kana
/// differences too narrow for any of the tests below to separate from a real one, and neither is
/// worth a spelling-specific exception — which is the thing this whole approach exists to avoid.
fn pronunciation_is_spellable(spelled: &str, sounded: &str) -> bool {
    // **A long vowel is written, not heard.** 場 is ジョウ as a spelling and ジョー as a sound;
    // 最中 is サイチュウ against サイチュー, 勉強 ベンキョウ against ベンキョー. The mark says the
    // vowel is held, not which kana holds it, so nothing here could put the う back.
    if sounded.contains('ー') {
        return false;
    }
    // **The pronunciation column cannot write ヅ or ヂ**; it normalizes both onto ズ and ジ, which
    // are the same sound and a different spelling. 近づく would come back ちかずく, 気づく きずく,
    // 続けざま つづけざま — 44 spellings in the sweep. Where the written reading has one and the
    // sound does not, the sound has lost information rather than supplied it.
    //
    // **Deliberately one-directional.** The other way round is a real pair of words: 感ずる and
    // 感じる both exist, the token spelled 感じる is かんじる, and testing this symmetrically would
    // refuse the eight ずる/じる verbs the sweep shows this getting right.
    if spelled.contains(['づ', 'ぢ']) && !sounded.contains(['づ', 'ぢ']) {
        return false;
    }
    // **Whether a compound voices its second element is a fact about its spelling.** すき腹 is
    // written すきばら and 道はた みちばた, where the pronunciation offers すきはら and みちはた.
    // Agreeing on every kana and disagreeing only about a voicing mark is that disagreement and
    // no other, and the written form is the one the reader is reading.
    if devoiced(spelled) == devoiced(sounded) {
        return false;
    }
    true
}

/// A kana string with its voicing marks removed, for comparing two spellings of one sound.
///
/// Decomposing and keeping the base character drops ゛ and ゜ without a table: が becomes か, ぱ
/// becomes は.
fn devoiced(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    text.chars().map(|c| c.nfd().next().unwrap_or(c)).collect()
}

fn is_kanji(character: char) -> bool {
    matches!(character, '\u{3400}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}')
}

/// Recover a dictionary-form reading from the surface reading.
///
/// **A fallback now.** UniDic records 語彙素読み, so this runs only where that column is
/// absent; under IPADIC, which had no such column, it was the only way to get one. The
/// reconstruction below is kept because an absent column is still a case that arrives.
///
/// It provides タベ for 食べ and 食べる as the lemma, but no タベル column. The
/// shared written prefix tells us exactly which kana tail changed: 読ん / よん against
/// 読む becomes よ + む. If that alignment is not present, this returns nothing rather
/// than guessing. 来る is the one written irregular whose class supplies the answer.
fn lemma_reading(surface: &str, reading: &str, dict_form: &str, class: &str) -> String {
    if reading.is_empty() {
        return String::new();
    }
    if class.starts_with("カ変") {
        return "くる".into();
    }
    if dict_form.chars().all(|character| !is_kanji(character)) {
        return to_hiragana(dict_form);
    }

    let shared = surface
        .chars()
        .zip(dict_form.chars())
        .take_while(|(left, right)| left == right)
        .count();
    let surface_tail: String = surface.chars().skip(shared).collect();
    let lemma_tail: String = dict_form.chars().skip(shared).collect();
    let surface_tail = to_hiragana(&surface_tail);
    let reading = to_hiragana(reading);
    reading
        .strip_suffix(&surface_tail)
        .map(|stem| format!("{stem}{}", to_hiragana(&lemma_tail)))
        .unwrap_or_default()
}

/// Put the morphology UniDic emits as adjacent tokens back into learner-sized words.
///
/// The boundary rules are structural, not a list of spellings: suffix verbs and
/// auxiliaries attach to an inflecting head; a 接続助詞 attaches the ending; and a
/// 非自立可能 verb after て/で begins another component. That last tag is UniDic saying
/// "helper", and is what lets 伝わってきます become the same two-part compound Ichiran
/// reports without swallowing ordinary neighbouring verbs.
/// A noun with the prefix and suffix UniDic hands over separately.
///
/// **This is not a katakana problem, though that is where it was found.** オバケ came back as
/// オ | バケ, and chasing it showed UniDic doing the same thing to beginner vocabulary: お茶 was
/// お | 茶 and お母さん was お | 母 | さん. Every one is the same shape — a 接頭辞 before the noun,
/// a 接尾辞-名詞的 after it — and UniDic is not wrong to record them separately, because お really
/// is the honorific 御. It is simply not how a reader meets the word.
///
/// It also settles two entries from the measured conflict list at once: 新入生, which the built-in
/// analyzer cut as 新入 | 生 where Ichiran keeps it whole, and オバケ itself.
///
/// # The reading is concatenated, and that is not free
///
/// お母さん comes out おははさん here, because 母 alone is はは and joining surfaces joins their
/// readings. **It is not a regression** — split, this analyzer put はは over the 母 of お母さん
/// just the same, so the error already existed and was merely spread across three tokens. What
/// changes is that a whole word can be looked up: `dict::readings::correct` turns おははさん into
/// おかあさん and おあにさん into おにいさん, neither of which it could do while the pieces were
/// apart, because 母 in isolation really is はは.
///
/// It does not fix all of them. お父さん stays おちちさん after the dictionary pass, and that is a
/// wrong reading on a word every beginner meets. It was wrong before this change too; joining it
/// makes it visible and fixable rather than fixing it. `reading_accuracy.rs` is where that belongs.
///
/// Scoped to `名詞的` suffixes deliberately. 感覚的 is 接尾辞-形状詞的 and behaves like an
/// adjective rather than continuing the noun, so it wants its own decision rather than being
/// swept in here.
fn compose_nominal(pieces: &[Piece], start: usize) -> Option<(Token, usize)> {
    let mut index = start;
    let mut parts: Vec<Token> = Vec::new();

    while pieces.get(index).is_some_and(|piece| piece.pos == "接頭辞") {
        if let Some(last) = parts.last() {
            if !contiguous(last.end, &pieces[index]) {
                return None;
            }
        }
        parts.push(pieces[index].token.clone());
        index += 1;
    }

    // **A run of digits belongs to [`compose_digits`], and this must not take the first one.**
    // 約80分 arrives as 約 | 8 | 0 | 分. Taking 8 as the head gave 約8 | 0 | 分 — a number cut in
    // half, and the halves carrying a reading concatenated from やく and はち that nobody says.
    // `compose_digits` runs after this one and only gets the chance if this declines.
    //
    // Asked as "would `compose_digits` claim a run here", rather than by testing for a digit,
    // because the two must agree on what a run is. A *single* digit is deliberately still allowed
    // through: 第3回 is だいさんかい, and UniDic's isolated reading for one digit is usually the
    // right one — the same line `compose_digits` draws by requiring two.
    if compose_digits(pieces, index).is_some() {
        return None;
    }

    let head = pieces.get(index).filter(|piece| piece.pos == "名詞")?;
    if let Some(last) = parts.last() {
        if !contiguous(last.end, head) {
            return None;
        }
    }
    parts.push(head.token.clone());
    index += 1;

    while pieces
        .get(index)
        .is_some_and(|piece| piece.pos == "接尾辞" && piece.subpos == "名詞的")
    {
        if !contiguous(parts.last().and_then(|part| part.end), &pieces[index]) {
            break;
        }
        parts.push(pieces[index].token.clone());
        index += 1;
    }

    // One part is a bare noun, which the ordinary path already handles correctly.
    (parts.len() > 1).then(|| (fixed_expression(parts), index))
}

/// A run of digits, joined and left unread.
///
/// **A number's reading depends on what is being counted, and UniDic does not know.** 2011 comes
/// over as four tokens reading ニ, ゼロ, イチ, イチ — each correct for the digit alone and
/// together にぜろいちいち, which is not how anyone says 2011年.
///
/// So a run of two or more is joined into the one number it is, and its reading is cleared rather
/// than concatenated. No furigana is the honest answer here; the wrong furigana is the one thing
/// this app is built not to give.
///
/// **A single digit joins only when something is counting it**, and that is a change from the
/// rule this function started with. It used to leave every lone digit alone, reasoning that １ in
/// １回 really is いち and KWDLC's gold reads bare digits exactly that way. That is still true of
/// the *digit*, and it stopped being the right unit: `ud_gold.rs` measures 4月, 8月, 12月, 2011年
/// and 17日 among the most frequent long units annotators keep whole and this analyzer cut apart,
/// and `project notes` §4a makes the long unit the score.
///
/// **A counted number keeps an assembled reading; a bare one keeps none.** That asymmetry is the
/// normalized contract's, not a preference: `analyzer_contract.rs` requires every *word* to carry
/// a display reading, and 4月 is a word where 2011 is not. Clearing it was tried first and broke
/// that contract — a word with no reading is a word with no furigana on the page.
///
/// So this concatenates, and the reading can be wrong until the dictionary sees the whole: counter
/// readings are irregular — 1日 ついたち, 20日 はつか, 7月 しちがつ — and よんがつ is what the parts
/// give for 4月. That is the same trade `compose_nominal` already makes and says so about お父さん
/// staying おちちさん: joining makes a wrong reading visible and fixable by `dict::readings`,
/// where apart it was unfixable in principle.
fn compose_digits(pieces: &[Piece], start: usize) -> Option<(Token, usize)> {
    let is_digit = |piece: &Piece| {
        !piece.token.surface.is_empty()
            && piece
                .token
                .surface
                .chars()
                .all(|c| c.is_ascii_digit() || ('０'..='９').contains(&c))
    };
    let mut index = start;
    let mut parts: Vec<Token> = Vec::new();
    while pieces.get(index).is_some_and(is_digit) {
        if let Some(last) = parts.last() {
            if !contiguous(last.end, &pieces[index]) {
                break;
            }
        }
        parts.push(pieces[index].token.clone());
        index += 1;
    }
    if parts.is_empty() {
        return None;
    }

    // **The counter belongs to the number**, and gold says so: 4月, 8月, 12月, 2011年, 17日 are
    // among the most frequent long units UD_Japanese-GSD keeps whole and this analyzer cut apart.
    // UniDic marks the counter for us — 助数詞可能, "can be a counter" — on an ordinary 名詞, which
    // is why `compose_nominal`'s 接尾辞 loop never reached it.
    //
    // This is also what makes a *single* digit worth joining, where alone it is not: 4 by itself
    // is よん and reads correctly, but 4月 is しがつ and no concatenation of parts produces that.
    // Counter readings are irregular throughout — 1日 ついたち, 20日 はつか, 7月 しちがつ — so the
    // whole is the only unit that can be looked up, exactly as with お母さん in `compose_nominal`.
    let counter = pieces.get(index).filter(|piece| {
        is_counter(piece) && contiguous(parts.last().and_then(|part| part.end), piece)
    });
    // The digits on their own, before the counter joins them, which is what gets read as a value.
    let digits: String = parts.iter().map(|part| part.surface.as_str()).collect();
    let counted =
        counter.map(|counter| (counter.token.surface.clone(), counter.token.reading.clone()));
    if let Some(counter) = counter {
        parts.push(counter.token.clone());
        index += 1;
    } else if parts.len() < 2 {
        // A lone digit with nothing counting it is left exactly as it was.
        return None;
    }

    let mut token = fixed_expression(parts);
    match &counted {
        // **The place values are assembled, not recited.** Composition runs the parts' readings
        // together, and a number's parts do not spell it: 34歳 was さん + よん + さい, 20分 was
        // にぜろふん, 1963年 いちきゅうろくさんねん. `numbers::counted` derives the value's reading
        // and applies whatever the counter does to it, and returns nothing where the digits are not
        // a quantity at all — a leading zero, or a run long enough to be an account number.
        //
        // **The old concatenation is still the fallback**, for the same reason it was the rule:
        // `analyzer_contract.rs` requires every word to carry a display reading, and 4月 is a word
        // where 2011 is not. A refusal here leaves a reading that may be wrong and can still be
        // corrected by `dict::readings` where the whole is a dictionary entry; clearing it would
        // leave the page with no furigana at all.
        Some((surface, reading)) => {
            if let Some(assembled) = crate::numbers::parse_digits(&digits)
                .and_then(|value| crate::numbers::counted(value, surface, reading))
            {
                // **Only the displayed reading.** `fixed_expression` has already copied the
                // concatenation into `lemma_reading`, and that is what `WordKey::for_token` keys
                // on, so leaving it is what keeps a word somebody marked as 34歳 findable. The
                // pair is inconsistent and known to be — see `local documentation`, which
                // carries the identity question this phase deliberately does not answer.
                token.reading = assembled;
            }
        }
        // A bare number is not a word — `Token::is_word` asks whether there is Japanese in it —
        // so it needs no reading, and has none that could be right: 2011 is にせんじゅういち as a
        // year and にぜろいちいち as four digits, and nothing here knows which.
        None => {
            token.reading = String::new();
            token.lemma_reading = String::new();
        }
    }
    Some((token, index))
}

/// である — the written copula, which UniDic hands over as an auxiliary plus a verb.
///
/// **The largest single long unit gold keeps whole and this analyzer cut apart**: 55 occurrences
/// of である and 7 of であり in `ud_gold.rs`. で is 助動詞 and never an inflecting head, ある is,
/// so `compose` reached them in two different branches and nothing could bring them together.
///
/// **The conjugation is carried across, and that is the whole difficulty.** `compound` builds its
/// result with `..Default::default()`, so joining the two naively leaves である with no
/// conjugation at all — and であった, であれば and であって are exactly what a reader needs the
/// breakdown for. So the verb is composed first, with its full derivation chain, and で is put in
/// front of the finished token rather than merged into a flat pair.
///
/// The lemma is rewritten to である rather than left as ある. ある is the existence verb, and
/// offering it as the dictionary form would send a reader looking up "to exist" for a copula —
/// the same mistake [`fixed_expression`] documents for として's と.
///
/// **The 助動詞 test is the discriminator and it is load-bearing.** ある follows plenty of things:
/// 読んである has で as 接続助詞 after a te-form, ここである has it as 格助詞. Only the copula だ
/// inflects to で as 助動詞-ダ, so that is what is asked, and the ordinary uses are untouched.
fn compose_copula(pieces: &[Piece], start: usize) -> Option<(Token, usize)> {
    let de = pieces.get(start)?;
    if de.pos != "助動詞" || de.class != "助動詞-ダ" || de.token.surface != "で" {
        return None;
    }
    if !de.form.starts_with("連用形") {
        return None;
    }

    let verb = pieces.get(start + 1)?;
    if verb.token.dict_form != "ある"
        || !is_inflecting_head(verb)
        || !contiguous(de.token.end, verb)
    {
        return None;
    }

    let (verb, after) = compose_component(pieces, start + 1, false);
    let dict_form = format!("{}{}", de.token.surface, verb.dict_form);
    let lemma_reading = format!("{}{}", de.token.reading, verb.lemma_reading);
    let conjugation = verb.conjugation.clone().map(|mut conjugation| {
        conjugation.lemma = dict_form.clone();
        conjugation.reading = lemma_reading.clone();
        // The base step carries the lemma's reading for the ladder to build from, and the lemma
        // has just been rewritten — leaving it as ある would show a derivation that starts from a
        // word the token no longer claims to be.
        for prop in &mut conjugation.props {
            if prop.is_base {
                prop.base_reading = lemma_reading.clone();
            }
        }
        conjugation
    });

    let mut token = compound(vec![de.token.clone(), verb]);
    token.dict_form = dict_form;
    token.lemma_reading = lemma_reading;
    token.conjugation = conjugation;
    Some((token, after))
}

/// The nominalising の and the copula that completes it: ので, のだ, のです.
///
/// **Measured, not guessed.** `ud_gold.rs` puts ので at 49 occurrences and のです at 12 among the
/// long units annotators keep whole and this analyzer cut apart — between them the second largest
/// family after である. UniDic hands over の as 助詞-準体助詞 and the copula as a separate 助動詞,
/// and neither is an inflecting head, so nothing in `compose` was ever going to bring them
/// together.
///
/// **The grammar packs are unaffected, which was checked rather than assumed.** `core.json`
/// matches this with `match_text: ["ので"]`, and `Rule::Text` is documented as a surface run
/// spanning *one or more whole tokens* — so it matched の | で before and matches ので now. That
/// robustness is deliberate: `pack.rs` says `match_text` exists precisely to remove the asymmetry
/// between an analyzer that merges and one that does not.
///
/// のに is deliberately absent. It is a real conjunction and also a genuine の + に, gold does not
/// list it among the splits here, and joining an ambiguous pair on the strength of the
/// unambiguous ones is how a rule starts being wrong somewhere nobody measured.
fn compose_no_clause(pieces: &[Piece], start: usize) -> Option<(Token, usize)> {
    let head = pieces.get(start)?;
    if head.pos != "助詞" || head.subpos != "準体助詞" {
        return None;
    }
    let tail = pieces.get(start + 1)?;
    if tail.pos != "助動詞" || !contiguous(head.token.end, tail) {
        return None;
    }
    if !matches!(tail.token.surface.as_str(), "で" | "だ" | "です") {
        return None;
    }
    // The reading concatenates correctly here — の is の and で is で — so unlike a number and its
    // counter this keeps the reading it assembles.
    Some((
        fixed_expression(vec![head.token.clone(), tail.token.clone()]),
        start + 2,
    ))
}

/// A run of the *same* punctuation mark: —— and …… arrive one character at a time.
///
/// **Identical characters only, and that restraint is what makes it safe.** UniDic hands 補助記号
/// over singly, so an em-dash rule and an ellipsis both come apart; joining any two adjacent marks
/// would also weld 。」 and 、「 together, which are two marks doing two jobs. A repeated mark is
/// one gesture — the writer held the key down — and gold agrees: `segmentation_gold.rs` has both
/// —— and …… as single tokens, and they are two of the eight words Ichiran found there that this
/// analyzer split.
///
/// The reading is cleared for the same reason `compose_digits` clears a bare number's: a run of
/// dashes has no pronunciation, and `Token::is_word` is false for it, so nothing downstream is
/// left wanting furigana it cannot have.
fn compose_symbol_run(pieces: &[Piece], start: usize) -> Option<(Token, usize)> {
    let head = pieces.get(start)?;
    if head.pos != "補助記号" || head.token.surface.chars().count() != 1 {
        return None;
    }
    let mark = head.token.surface.as_str();

    let mut index = start;
    let mut parts: Vec<Token> = Vec::new();
    while let Some(piece) = pieces.get(index) {
        if piece.token.surface != mark || !contiguous(parts.last().and_then(|p| p.end), piece) {
            break;
        }
        parts.push(piece.token.clone());
        index += 1;
    }
    if parts.len() < 2 {
        return None;
    }

    let mut token = fixed_expression(parts);
    token.reading = String::new();
    token.lemma_reading = String::new();
    Some((token, index))
}

/// What counts a number: 月 in 4月, 日 in 17日, 人 in 3人, 日間 in 5日間.
///
/// **UniDic tags counters two different ways and both have to be here.** 月 and 日間 arrive as
/// 名詞-普通名詞 with 助数詞可能 in the third field; 日 and 人 arrive as 接尾辞-名詞的 instead. The
/// split is not principled and it is not ours to fix — reading only the first form joined 4月 and
/// left 17日 apart, which is the kind of half-working rule that looks like a tokenizer bug.
///
/// **A symbol counter was once excluded, and ％ was why — the reason has since stopped being
/// true.** The argument was that joining costs a reading: ぱーセント is not visible in the glyph the
/// way 月 or 日 are, so a learner who could not recover it would lose it. That was correct when
/// [`compose_digits`] cleared the reading of everything it built.
///
/// It no longer does. It clears only for a **bare** number, where 2011 has no single right answer;
/// when a counter is absorbed the reading is the parts concatenated, so ５％ joins as one token
/// reading ごぱーせんと and nothing is dropped. The old note was arguing against a behaviour
/// that had been changed out from under it — which is the failure mode a comment is most prone to.
///
/// What remains true is the segmentation: §4a makes the long unit the score and gold keeps a
/// number and its counter whole, so the symbol joins like any other.
///
/// **This reaches ％ and not `%`, and the difference is UniDic's, not ours.** Only the fullwidth
/// form is tagged 助数詞可能; ASCII `%` and Latin units like `cm` are tagged something else and
/// never reach this function at all, so widening it moved them not at all. That is worth stating
/// because the change was argued for from `ud_gold.rs`, whose news text is **ASCII throughout** —
/// the evidence cited was for a character this cannot affect, and LUW F1 duly did not move.
/// `local documentation` records that. The change is kept because it is right for the
/// fullwidth form the app actually meets in manga and games, not because a gold number improved.
fn is_counter(piece: &Piece) -> bool {
    (piece.pos == "名詞" && piece.subpos3 == "助数詞可能")
        || (piece.pos == "接尾辞" && piece.subpos == "名詞的")
}

/// A run of single Latin letters, which UniDic hands over one character at a time.
///
/// ESC and USB arrive as `記号-文字` per letter, so ESCキー came back as E | S | C | キー. Nothing
/// is lost by joining them and an acronym is unreadable split apart. Only `記号-文字` is joined,
/// so ordinary Japanese punctuation and the `記号-一般` fallback are untouched.
fn compose_letters(pieces: &[Piece], start: usize) -> Option<(Token, usize)> {
    let is_letter = |piece: &Piece| piece.pos == "記号" && piece.subpos == "文字";
    let mut index = start;
    let mut parts: Vec<Token> = Vec::new();
    while pieces.get(index).is_some_and(is_letter) {
        if let Some(last) = parts.last() {
            if !contiguous(last.end, &pieces[index]) {
                break;
            }
        }
        parts.push(pieces[index].token.clone());
        index += 1;
    }
    (parts.len() > 1).then(|| (fixed_expression(parts), index))
}

fn compose(pieces: Vec<Piece>) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < pieces.len() {
        // **Before the gate, because ある is a verb and the auxiliary loop would not take it.**
        // Everything else a copula folds is an auxiliary, so it goes through `compose_component`
        // below; である is the one shape that needs its own join.
        if let Some((token, next)) = compose_copula(&pieces, index) {
            tokens.push(token);
            index = next;
            continue;
        }
        // Before `compose_no_clause`, which would otherwise take んだ and leave な stranded.
        if let Some((token, next)) = compose_explanatory_nan(&pieces, index) {
            tokens.push(token);
            index = next;
            continue;
        }
        let starts_suru = is_suru_start(&pieces, index);
        if !starts_suru
            && !is_inflecting_head(&pieces[index])
            && !is_suffix_adjective(&pieces[index])
            && !is_copula_head(&pieces[index])
        {
            // Tried before the bare push, and both return `None` unless there is genuinely
            // something to join, so an ordinary noun takes the same path it always did.
            if let Some((token, next)) = compose_symbol_run(&pieces, index) {
                tokens.push(token);
                index = next;
                continue;
            }
            if let Some((token, next)) = compose_no_clause(&pieces, index) {
                tokens.push(token);
                index = next;
                continue;
            }
            if let Some((token, next)) = compose_nominal(&pieces, index) {
                tokens.push(token);
                index = next;
                continue;
            }
            if let Some((token, next)) = compose_digits(&pieces, index) {
                tokens.push(token);
                index = next;
                continue;
            }
            if let Some((token, next)) = compose_letters(&pieces, index) {
                tokens.push(token);
                index = next;
                continue;
            }
            tokens.push(pieces[index].token.clone());
            index += 1;
            continue;
        }

        let (first, mut next) = compose_component(&pieces, index, starts_suru);
        let mut components = vec![first];
        while components.last().is_some_and(ends_in_te)
            && pieces.get(next).is_some_and(|piece| {
                contiguous(components.last().and_then(|token| token.end), piece)
                    && is_non_independent_verb(piece)
            })
        {
            let (component, after) = compose_component(&pieces, next, false);
            components.push(component);
            next = after;
        }

        if components.len() == 1 {
            let mut token = components.pop().expect("one component");
            // A contracted helper is inside one piece, so its parts have to be cut rather than
            // grouped. See [`contracted_auxiliary_components`]. Only the motion helper is also
            // annotated: it is the one construction the reader looks up by headword.
            if let Some(parts) = contracted_auxiliary_components(&pieces[index..next], &token) {
                let motion = parts.last().is_some_and(|part| part.dict_form == "行く");
                token.components = parts;
                if motion {
                    let annotated = crate::construction::annotate(
                        &mut token,
                        ConstructionIdentity::ContractedMotion,
                    );
                    debug_assert!(annotated, "guarded motion components must annotate");
                }
            }
            tokens.push(token);
        } else {
            tokens.push(compound(components));
        }
        index = next;
    }

    tokens
}

/// A copula auxiliary in its 連用形 is a head, because something is going to follow it.
///
/// **This replaces a hand-written special case, and the reason is what it left behind.** だった was
/// joined by a function that built its own `Conjugation` from string literals — lemma "だ", form
/// "Past (~ta)", pos "cop" — duplicating badly what `compose_component` already does properly. It
/// fixed だった and nothing else, so its two siblings stayed broken *and mislabelled*:
///
/// ```text
/// そうでした    でし[です:Continuative] | た[た:Non-past]
/// 学生じゃない   じゃ[だ:Continuative]   | ない[ない:Non-past]
/// ```
///
/// た is a past auxiliary being called non-past, because standing alone it has no head to fold
/// into. じゃない is the most frequent segmentation disagreement in this app's own register —
/// 116 occurrences in the subtitle corpus against 6 in Tatoeba — and it was left split to fix a
/// word that appeared once in seventeen captures.
///
/// `compose_component` never required a verb; the restriction was in `compose`'s gate alone. So a
/// copula reaching it folds its auxiliaries through the same code every verb uses, and 助動詞-タ,
/// ない and ます are handled because they were already handled.
///
/// **連用形 only.** 終止形 だ ends a sentence and 連体形 な precedes a noun; neither has anything to
/// fold, and treating them as heads would let a clause's own auxiliaries be swallowed by the
/// copula in front of them.
/// な + ん before a copula: the explanatory 〜のだ, contracted.
///
/// 禁足地なんじゃなかったっけ came back with な and ん as two separate words, neither of which a learner can
/// look up: な alone is a copula inflection and ん alone is a particle, and the thing they are
/// together — "the explanation is that…" — was on screen as two shrugs.
///
/// **Three pieces are required and only two are taken.** UniDic gives な as 助動詞-ダ in 連体形,
/// ん as 助詞-準体助詞 with lemma reading の, and then the copula that carries the tense. That third
/// piece has to *be there* — without it this is なん doing something else — but it is deliberately
/// not consumed, so じゃなかったっけ goes on to compose normally and keeps its own negative past.
///
/// **Contiguity is checked at both joins**, so nothing is welded across a line break, and the
/// spelling なん is never consulted: なんか is the pronoun 何 (代名詞) and cannot reach this at all.
///
/// The parent is built by hand rather than through [`fixed_expression`], which would set the
/// lexical identity to the surface なん and make the group look like a word called なん. It keeps
/// the copula's identity — `dict_form` だ — so a learner's saved words are keyed exactly as they
/// were, and claims no conjugation of its own: the morphology is on the components, where it is
/// true.
fn compose_explanatory_nan(pieces: &[Piece], start: usize) -> Option<(Token, usize)> {
    let na = pieces.get(start)?;
    if na.pos != "助動詞" || na.class != "助動詞-ダ" || !na.form.starts_with("連体形") {
        return None;
    }
    let nominaliser = pieces.get(start + 1)?;
    if nominaliser.pos != "助詞"
        || nominaliser.subpos != "準体助詞"
        // **The contracted form only, and this is not "identify it by spelling".** The structural
        // tests above and below still decide *whether* this is the construction; this decides
        // which of its two spellings is in scope. の is the same 準体助詞, so without this the
        // rule also takes なので — 映画館なので became 映画館 | なの | で — which is a different
        // construction, is far commoner in written Japanese, and cost 26 gold matches when it
        // fired. なの is deliberately out of scope until it is looked at on its own terms.
        || nominaliser.token.surface != "ん"
        || !contiguous(na.token.end, nominaliser)
    {
        return None;
    }
    // The copula that finishes the construction. Required, never consumed.
    let copula = pieces.get(start + 2)?;
    if copula.pos != "助動詞"
        || !matches!(copula.class.as_str(), "助動詞-ダ" | "助動詞-デス")
        || !contiguous(nominaliser.token.end, copula)
    {
        return None;
    }
    let mut token = Token {
        surface: format!("{}{}", na.token.surface, nominaliser.token.surface),
        reading: format!("{}{}", na.token.reading, nominaliser.token.reading),
        dict_form: "だ".to_string(),
        lemma_reading: "だ".to_string(),
        start: na.token.start,
        end: nominaliser.token.end,
        components: vec![na.token.clone(), nominaliser.token.clone()],
        ..Default::default()
    };
    let annotated = crate::construction::annotate(&mut token, ConstructionIdentity::ExplanatoryNan);
    debug_assert!(annotated, "guarded explanatory components must annotate");
    Some((token, start + 2))
}

fn is_copula_head(piece: &Piece) -> bool {
    piece.pos == "助動詞"
        && matches!(piece.class.as_str(), "助動詞-ダ" | "助動詞-デス")
        && piece.form.starts_with("連用形")
}

/// A derivational adjective suffix — っぽい, らしい — which inflects like the adjective it makes.
///
/// **UniDic files these under 接尾辞 rather than 形容詞, so [`is_inflecting_head`] misses them**, and
/// 子供っぽくない came back as 子供 | っぽく | ない — the negation standing alone as a separate
/// word, which is the one part of it a reader most needs attached. The piece is otherwise a
/// perfectly ordinary adjective: class 形容詞, form 連用形-一般, lemma っぽい. Only the part of
/// speech is unusual, so only the gate needed widening and [`compose_component`] handles the rest
/// through the same code every い-adjective uses.
///
/// **Separate from [`is_inflecting_head`] on purpose.** That predicate also guards
/// [`compose_copula`]'s test that ある is a real verb, and a suffix has no business satisfying
/// that. Widening it in place would have changed two decisions to fix one.
///
/// The preceding noun is untouched: 子供 is pushed before this is ever reached, so 子供っぽい is
/// still two tokens. Whether it should be one is a dictionary-join question and is left to
/// `dict::readings::merge_compounds`, which is where every other whole-word join is decided.
///
/// **This is wider than っぽい, and the news corpus is where that shows.** 接尾辞-形容詞的 also
/// holds やすい, にくい, らしい and their kin. Measured over UD_Japanese-GSD, widening the gate
/// changes **exactly one sentence in 1,035**:
///
/// ```text
/// 姉と同じ先生だったので、話やすかったです。
///   before   話 | やすかっ | た | です
///   after    話 | やすかった | です
/// ```
///
/// **It costs one gold match, and it is still right.** Gold labels やすかっ `LUWBILabel=I` —
/// continuing 話 as one long unit — and た `B`, a long unit of its own, so やすかった straddles
/// the boundary and the た match is lost: LUW matched 14548 → 14547, mis-splits 1477 → 1476,
/// over-joins 3609 → 3610.
///
/// That is not a new disagreement. **This analyzer folds auxiliaries into their head everywhere**
/// — 食べた is one token — while gold's LUW convention gives every auxiliary its own unit, which
/// is what `ud_gold.rs` counts under "auxiliaries gold attaches to a verb, unmapped" (105× た).
/// Before this, やすかっ | た left an auxiliary standing alone as though it were a word, which
/// 高くない and っぽくなかった already are not. Narrowing the rule to dodge the number would be
/// fitting to gold against §4a, so the number is recorded instead.
fn is_suffix_adjective(piece: &Piece) -> bool {
    piece.pos == "接尾辞" && piece.subpos == "形容詞的"
}

/// Cut 連れてって into 連れて + って, so the motion helper can be recognised and explained.
///
/// **Every other te-linked helper arrives as its own piece and this one does not.** 連れていく is
/// 連れ | て | いく, so `compose`'s helper loop makes 連れて and いく two components and
/// [`crate::grammar::compounds::detect`] finds 行く among them. The contraction is cut
/// 連れ | てっ | て: the て that belongs to 連れて sits **inside** the auxiliary's own piece, so no
/// grouping of pieces can produce the same two parts. They are reconstructed here instead.
///
/// **Reconstructed from real offsets, not invented text.** The boundary is the auxiliary piece's
/// own start plus its first character — the て or で — so both parts are exact substrings of the
/// source, they are contiguous, they concatenate back to the parent surface, and that last
/// property is asserted rather than assumed: if it ever fails the split is abandoned and the token
/// is left whole.
///
/// **The auxiliary is normalised to 行く / いく.** That is what `compounds::AUXILIARIES` matches to
/// reach `te-iku`, and it is what the uncontracted form already supplies, so the same sentence
/// gets the same explanation whether or not it was contracted. The *surface* stays って — the
/// omitted characters are not put back, because the reader is looking at 連れてって on the page.
///
/// No conjugation is claimed for either part. `compounds::is_te_form` accepts a surface ending in
/// て, which is the case it exists for, and inventing a derivation chain for a span UniDic never
/// labelled would be asserting more than the tagger said.
///
/// # Every contracted helper, not only the motion one (2026-10-17)
///
/// **言ってる was explained as 言う, and nothing else.** [`is_contracted_auxiliary`] holds the
/// contraction inside one word, and that word had no parts — so `compounds::detect` had nothing to
/// find 〜ている in, the conjugation said plain non-past, and a reader was told 言ってる means 言う.
/// Measured over the Jitendex sentences by `core/tests/grammar_lists.rs`: the full-form rule fired
/// on 0 of 194 〜てる, 0 of 23 〜ちゃう and 0 of 11 〜とく. So every helper with a pack entry is cut
/// here the same way, normalised to the lemma its uncontracted form supplies:
///
/// - **てる/でる → いる, てく/でく → 行く.** The contraction begins with the て, which is cut off
///   onto the verb exactly as for 連れてって: 言って + る, 遊んで + る.
/// - **ちゃう/じゃう/ちまう/じまう → しまう, とく/どく → おく.** Here the て is *fused* — ちゃ is
///   て+しま, と is て+お — so there is no character to cut. The verb keeps its own surface
///   (握っ + ちゃう), and `compounds::detect` accepts a helper that carries its own て
///   (`compounds::carries_te`) where it would otherwise demand a te-form before it.
/// - **とる/どる are left whole.** They are 〜ておる, which the packs have no entry for, so a part
///   would name a pattern nothing explains.
fn contracted_auxiliary_components(consumed: &[Piece], token: &Token) -> Option<Vec<Token>> {
    // Not first: the helper attaches to a verb, and a token starting with it is not this pattern.
    let at = consumed.iter().position(is_contracted_auxiliary)?;
    if at == 0 {
        return None;
    }
    let auxiliary = &consumed[at];
    let (lemma, lemma_reading) = uncontracted_helper(&auxiliary.token.dict_form)?;
    let first = auxiliary.token.surface.chars().next()?;
    // The て/で that opens てる and てく belongs to the verb, as in the uncontracted form.
    let cut = if matches!(first, 'て' | 'で') {
        first.len_utf8()
    } else {
        0
    };
    let boundary = auxiliary.token.start? + cut;

    let mut lead = String::new();
    let mut lead_reading = String::new();
    for piece in &consumed[..at] {
        lead.push_str(&piece.token.surface);
        lead_reading.push_str(&piece.token.reading);
    }
    lead.push_str(&auxiliary.token.surface[..cut]);
    lead_reading.push_str(auxiliary.token.reading.get(..cut).unwrap_or_default());

    let mut tail = auxiliary.token.surface[cut..].to_owned();
    let mut tail_reading = auxiliary
        .token
        .reading
        .get(cut..)
        .unwrap_or_default()
        .to_owned();
    for piece in &consumed[at + 1..] {
        tail.push_str(&piece.token.surface);
        tail_reading.push_str(&piece.token.reading);
    }

    // The parts describe the same span the parent does, or they describe nothing.
    if tail.is_empty() || format!("{lead}{tail}") != token.surface {
        return None;
    }

    Some(vec![
        Token {
            surface: lead,
            dict_form: token.dict_form.clone(),
            reading: lead_reading,
            lemma_reading: token.lemma_reading.clone(),
            start: token.start,
            end: Some(boundary),
            ..Default::default()
        },
        Token {
            surface: tail,
            dict_form: lemma.to_string(),
            reading: tail_reading,
            lemma_reading: lemma_reading.to_string(),
            start: Some(boundary),
            end: token.end,
            ..Default::default()
        },
    ])
}

/// The helper a contracted auxiliary stands for, as the lemma its uncontracted form supplies —
/// which is what `compounds::AUXILIARIES` matches. `None` for one the packs cannot explain.
fn uncontracted_helper(contracted: &str) -> Option<(&'static str, &'static str)> {
    Some(match contracted {
        "てる" | "でる" => ("いる", "いる"),
        "てく" | "でく" => ("行く", "いく"),
        "ちゃう" | "じゃう" | "ちまう" | "じまう" => ("しまう", "しまう"),
        "とく" | "どく" => ("おく", "おく"),
        _ => return None,
    })
}

fn is_inflecting_head(piece: &Piece) -> bool {
    matches!(piece.pos.as_str(), "動詞" | "形容詞") && piece.subpos != "接尾"
}

/// Whether a piece begins exactly where the last one ended.
///
/// **A gap between two pieces is a line break**, because Lindera cuts its lattice at one and then
/// drops the whitespace token. `compose` runs over the pieces of every lattice at once and has no
/// other way to know a break sat between two of them — so without this it merges across one and
/// produces a token whose `surface` is missing bytes that `text[start..end]` still has.
///
/// Not cosmetic: `６人宿泊させ` / `て欲しい` composed into a single `宿泊させて` spanning the
/// newline, breaking the grounding invariant `core/tests/analyzer_contract.rs` asserts for every
/// token and which `ui/src/lib/passage.js` relies on to find the reader's line breaks.
///
/// Ungrounded pieces carry no evidence either way and are left to merge as before.
fn contiguous(end: Option<usize>, piece: &Piece) -> bool {
    match (end, piece.token.start) {
        (Some(finish), Some(start)) => finish == start,
        _ => true,
    }
}

/// `非自立可能` where IPADIC said `非自立`.
///
/// The word "可能" is UniDic being careful: 行く is *capable* of being non-independent, and is
/// tagged so whether or not it is one here. That is why composition still checks the shape of
/// the neighbourhood rather than trusting this alone.
fn is_non_independent_verb(piece: &Piece) -> bool {
    piece.pos == "動詞" && piece.subpos == "非自立可能"
}

fn is_suru_start(pieces: &[Piece], index: usize) -> bool {
    let Some((noun, verb)) = pieces.get(index).zip(pieces.get(index + 1)) else {
        return false;
    };
    // サ変可能 is at 品詞小分類 in UniDic — 勉強 comes back as 名詞 | 普通名詞 | サ変可能 —
    // where IPADIC put サ変接続 one column earlier. Reading the old column finds 普通名詞 and
    // never matches, which would leave every する compound split in two.
    noun.pos == "名詞"
        && noun.subpos3 == "サ変可能"
        && verb.pos == "動詞"
        && verb.token.dict_form == "する"
        // **The same guard every other join already applies**, and the one site that was missing
        // it. `compose_component` builds this token's surface by concatenating the two pieces
        // while its span runs from the noun's start to the verb's end — so a space between them
        // lands in the span and not in the surface, and the token then lies about its own source.
        // A reader's capture did exactly that: a サ変可能 noun and する sat either side of a
        // space, and `study::analyse` refused the whole passage because the token claimed the two
        // joined where the source spelled them apart. Refusing the join leaves two honest tokens
        // instead of one dishonest one — whatever put the space there.
        && contiguous(noun.token.end, verb)
}

fn is_suffix_verb(piece: &Piece) -> bool {
    piece.pos == "動詞" && piece.subpos == "接尾"
}

fn is_auxiliary(piece: &Piece) -> bool {
    piece.pos == "助動詞"
}

/// The auxiliaries of casual speech, which UniDic files as conjugating words.
///
/// **UniDic names most auxiliaries and conjugates these.** ます and た arrive with a class of
/// `助動詞-マス` and `助動詞-タ`; てる, ちゃう and とる arrive as `助動詞` with a *conjugation*
/// class instead — 下一段-タ行, 五段-ワア行, 五段-ラ行. A match on the name alone therefore let
/// every one of them fall through, which is why 見ている came back as one word and 見てる as two.
///
/// **Keyed on the dictionary form, not the surface.** The first version listed surfaces, and so
/// caught only the forms it happened to name: 行っちゃう worked and 行っちゃった did not, because
/// the auxiliary there is spelled ちゃっ. UniDic gives every inflection of it the lemma ちゃう, so
/// one entry covers them all — and the て of 言ってた has the lemma てる, which is what it is.
///
/// **Measured, not guessed.** Against Ichiran the built-in analyzer cuts a span differently on
/// 6.3% of the capture corpus and 2.1% of Tatoeba sentences, and contracted speech is most of what
/// those two bodies of text differ by. ちまう and とる were added after reading manga dialogue where
/// なっちまったら and 抜けとる came apart — 〜ておる and 〜てしまう are ordinary in dialect and
/// absent from textbook Japanese.
///
/// Gated on the part of speech, because every one of these spellings is also a real word: とる is
/// 取る, でる is 出る, とく is 説く, て is the ordinary 接続助詞 of 見ている. UniDic separates each
/// pair by 品詞 — the dialectal とる is a 助動詞 and the verb 取る a 動詞 — so this cannot swallow
/// a verb, and the test asserts exactly that.
fn is_contracted_auxiliary(piece: &Piece) -> bool {
    piece.pos == "助動詞"
        && matches!(
            piece.token.dict_form.as_str(),
            "てる"
                | "でる"
                | "ちゃう"
                | "じゃう"
                | "ちまう"
                | "じまう"
                | "とる"
                | "どる"
                | "とく"
                | "どく"
                // てく is ていく contracted, and its te-form てって is what 連れてって actually contains:
                // UniDic reads that as 連れ + てっ[てく] + て, not as 連れて + って. Without this the
                // auxiliary was a head of its own and the phrase came back in three pieces, with
                // the motion standing apart from the verb it belongs to.
                | "てく"
                | "でく"
        )
}

/// ない after an adjective, which UniDic files as an adjective rather than an auxiliary.
///
/// 高くない comes back as 形容詞 | 一般 then 形容詞 | 非自立可能, where IPADIC gave 助動詞 for
/// the second. Without this the negation is dropped and 高くない is reported as 高い — a
/// breakdown that says the opposite of the sentence.
fn is_negating_adjective(piece: &Piece) -> bool {
    piece.pos == "形容詞" && piece.subpos == "非自立可能" && piece.token.dict_form == "ない"
}

/// The conjunctive particles that attach to an inflecting head.
///
/// ちゃ and じゃ are here for the same reason [`is_contracted_auxiliary`] exists: 入っちゃ駄目 is
/// ては contracted, UniDic files the ちゃ as an ordinary 接続助詞, and without it the phrase came
/// back as 入っ | ちゃ where Ichiran gives 入っちゃ. They are safe to name because 接続助詞 ちゃ
/// occurs in no other construction — the ちゃ of ちゃん or お茶 is not this part of speech.
fn is_connective(piece: &Piece) -> bool {
    piece.pos == "助詞"
        && piece.subpos == "接続助詞"
        && matches!(
            piece.token.surface.as_str(),
            "て" | "で" | "ば" | "ちゃ" | "じゃ"
        )
}

fn ends_in_te(token: &Token) -> bool {
    token.surface.ends_with('て') || token.surface.ends_with('で')
}

#[derive(Clone)]
struct Stage {
    form: String,
    pos: String,
    neg: bool,
    fml: bool,
}

/// Compose one inflecting word, stopping after its connective so a following
/// non-independent verb can become the next compound component.
fn compose_component(pieces: &[Piece], start: usize, suru: bool) -> (Token, usize) {
    let head_index = start + usize::from(suru);
    let head = &pieces[head_index];
    let noun = suru.then(|| &pieces[start]);

    let mut surface = noun.map_or_else(String::new, |piece| piece.token.surface.clone());
    surface.push_str(&head.token.surface);
    let mut reading = noun.map_or_else(String::new, |piece| piece.token.reading.clone());
    reading.push_str(&head.token.reading);

    let dict_form = if let Some(noun) = noun {
        format!("{}する", noun.token.surface)
    } else {
        head.token.dict_form.clone()
    };
    let lemma_reading = if let Some(noun) = noun {
        format!("{}する", noun.token.reading)
    } else {
        head.token.lemma_reading.clone()
    };
    let root_pos = if suru {
        "vs-i".to_string()
    } else {
        normalize_pos(&head.class, &head.token.dict_form)
    };
    let start_offset = noun
        .and_then(|piece| piece.token.start)
        .or(head.token.start);
    let mut end_offset = head.token.end;
    let mut next = head_index + 1;
    let mut consumed = suru;
    let mut voice: Option<&'static str> = None;
    let mut desiderative = false;
    let mut neg = false;
    let mut fml = false;
    let mut final_form: Option<&'static str> = None;

    while let Some(piece) = pieces.get(next) {
        // Never across a line break — see `contiguous`.
        if !contiguous(end_offset, piece) {
            break;
        }
        // **By dictionary form, not by part of speech.** IPADIC filed せる/られる as
        // 動詞 + 接尾; UniDic files them as 助動詞. Keying on the word itself is true under
        // both and says what is actually being recognised.
        let suffix_form = if is_suffix_verb(piece) || is_auxiliary(piece) {
            match piece.token.dict_form.as_str() {
                "せる" | "させる" => Some("Causative"),
                // The dictionary supplies the attached suffix but cannot settle the reading's
                // passive / potential / honorific ambiguity. It *can* settle the
                // causative-passive sequence structurally, because させ precedes it.
                "れる" | "られる" => Some("Ambiguous reru/rareru"),
                _ => None,
            }
        } else {
            None
        };

        if let Some(form) = suffix_form {
            voice = Some(match (voice, form) {
                (Some("Causative"), "Ambiguous reru/rareru") => "Causative-Passive",
                (_, found) => found,
            });
        } else if is_auxiliary(piece) || is_negating_adjective(piece) {
            // UniDic's names for the same auxiliaries IPADIC called 特殊・*.
            match piece.class.as_str() {
                _ if is_negating_adjective(piece) => neg = true,
                // **なきゃ is なければ, and it was being reported as a plain negative.** UniDic
                // gives the fused なきゃ the ナイ auxiliary's 仮定形 (as it does なけれ in なければ),
                // and taking only the negation from it reported しなきゃ as しない — dropping the
                // "if not", which is what makes it "must". The same 仮定形 test the たら case below
                // uses. なけれ+ば already arrived here as a provisional through ば.
                "助動詞-ナイ" => {
                    neg = true;
                    if piece.form.starts_with("仮定形") {
                        final_form = Some("Provisional (~eba)");
                    }
                }
                // です is a copula following the adjective, not morphology of the
                // adjective itself. Keeping it separate also preserves the boundary in
                // 〜ばいいです: the grammar point ends at いい, before the copula.
                // **ましょう is the polite volitional, and it was being reported as non-past.**
                // ます sets politeness and nothing else, so where no later auxiliary names a form
                // the fallback below supplied Non-past — 行きましょう came out 行く[Non-past].
                // Measured against KWDLC's 意志形, where the analyzer said Non-past for half of
                // them while every plain volitional — 行こう, 帰ろう, だろう, でしょう — was right.
                //
                // **Only 意志推量形 is taken from ます**, and the restraint is the point: ました
                // and ません get their form from the auxiliaries that follow (助動詞-タ, the
                // negation), and ます overriding those would break the two commonest polite forms
                // to fix a rarer one. まし and ませ therefore fall through exactly as before.
                "助動詞-マス" => {
                    fml = true;
                    if piece.form.starts_with("意志推量形") {
                        final_form = Some("Volitional");
                    }
                }
                // In 食べませんでした, でし is the です auxiliary between the already-seen
                // polite negative and past た. Attach it only in that established polite
                // chain: a free-standing copula remains its own word.
                "助動詞-デス" if fml => {}
                "助動詞-タイ" => desiderative = true,
                // **The same auxiliary, two grammar points.** UniDic files both 読んだ and
                // 降ったら under 助動詞-タ and separates them only by 活用形: 終止形 for the past,
                // 仮定形 for the conditional. Matching the auxiliary alone called every たら a
                // past tense — measured against Ichiran at 8 occurrences in 1,858 sentences,
                // every one of them wrong, and wrong in a way a learner would copy.
                "助動詞-タ" => {
                    final_form = Some(if piece.form.starts_with("仮定形") {
                        "Conditional (~tara)"
                    } else {
                        "Past (~ta)"
                    })
                }
                // ん in 食べませんでした. UniDic calls it the ぬ auxiliary and gives it a
                // class of its own, where IPADIC lumped it into 不変化型 with う.
                "助動詞-ヌ" => neg = true,
                "助動詞-ウ" => final_form = Some("Volitional"),
                // The form of the compound is the form of its *last* part: 見てる ends in a
                // 終止形 てる and is non-past, while the て of 言ってた is a 連用形 that the
                // following た then overrides with Past. Taking the auxiliary's own form gets
                // both without a rule per contraction.
                //
                // No new `Form` is invented for the progressive, deliberately. Ichiran reports
                // no conjugation at all for 見てる — its whole advantage here is the token
                // boundary — so naming an aspect this project cannot check would be claiming
                // more than either analyzer knows.
                _ if is_contracted_auxiliary(piece) => {
                    final_form = Some(if piece.form.starts_with("連用形") {
                        "Continuative"
                    } else {
                        "Non-past"
                    });
                }
                _ => break,
            }
        } else if is_connective(piece) {
            final_form = Some(if piece.token.surface == "ば" {
                "Provisional (~eba)"
            } else {
                "Conjunctive (~te)"
            });
        } else if piece.pos == "助詞" && piece.subpos == "並立助詞" && piece.token.surface == "たり"
        {
            final_form = Some("Alternative (~tari)");
        } else {
            break;
        }

        consumed = true;
        surface.push_str(&piece.token.surface);
        reading.push_str(&piece.token.reading);
        end_offset = piece.token.end;
        next += 1;

        if is_connective(piece) || piece.token.surface == "たり" {
            break;
        }
    }

    if !consumed {
        return (head.token.clone(), next);
    }

    let mut stages = Vec::new();
    if let Some(form) = voice {
        stages.push(Stage {
            form: form.into(),
            pos: root_pos.clone(),
            neg: false,
            fml: false,
        });
    }
    if desiderative {
        stages.push(Stage {
            form: "Desiderative (~tai)".into(),
            pos: if voice.is_some() {
                "v1".into()
            } else {
                root_pos.clone()
            },
            neg: false,
            fml: false,
        });
    }

    let final_form = final_form.or_else(|| (neg || fml).then_some("Non-past"));
    if let Some(form) = final_form {
        let pos = if desiderative {
            "adj-i".into()
        } else if voice.is_some() {
            "v1".into()
        } else {
            root_pos.clone()
        };
        stages.push(Stage {
            form: form.into(),
            pos,
            neg,
            fml,
        });
    } else if stages.is_empty() {
        stages.push(Stage {
            form: normalize_form(&head.form, &root_pos),
            pos: root_pos.clone(),
            neg,
            fml,
        });
    }

    let count = stages.len();
    let props = stages
        .into_iter()
        .rev()
        .enumerate()
        .map(|(depth, stage)| Prop {
            pos: stage.pos,
            form: stage.form,
            neg: stage.neg,
            fml: stage.fml,
            via_depth: depth as u32,
            base_reading: if depth + 1 == count {
                lemma_reading.clone()
            } else {
                String::new()
            },
            is_base: depth + 1 == count,
        })
        .collect();

    (
        Token {
            surface,
            dict_form: dict_form.clone(),
            reading,
            lemma_reading: lemma_reading.clone(),
            conjugation: Some(Conjugation {
                lemma: dict_form,
                reading: lemma_reading,
                props,
            }),
            start: start_offset,
            end: end_offset,
            ..Default::default()
        },
        next,
    )
}

/// Compound expressions UniDic splits and the grammar pack expects whole.
///
/// **A measured list, not a curated one.** Every entry satisfies three conditions at once:
/// UD_Japanese-GSD marks it one long unit and `ud_gold.rs` reports it near the top of the rejoin
/// shortlist; a grammar pack carries it as a pattern; and the matcher currently fails to fire on
/// it, because UniDic hands it over in pieces.
///
/// The third condition is why this is worth doing rather than merely tidy. Split, 教師として comes
/// back as 教師 | と | して and `particle-to` fires on that と — so a learner is not only denied
/// "として, in the capacity of", they are told the と is the quotative particle. A confident wrong
/// answer is the failure this app is built to avoid, and it was measurable the whole time.
///
/// The pack expects these whole because it was authored that way: `DEVELOPMENT.md` records that match
/// rules are written against observed **Ichiran** output, and Ichiran merges these on its own.
/// The rules are not wrong; the tokens under them changed.
///
/// **What is deliberately absent.** `ので` and `ために` — their rules live in `core.json`, were
/// written against split tokens, and already fire; nothing here needs to reach them. And the
/// copula forms gold also keeps whole, である, ではない and のです, because they interact with the
/// auxiliary folding in `compose_component` and merging them by surface would quietly take the
/// conjugation off a copula. Those want the conjugation work, not this.
///
/// Longest first, so によって is tried before による.
/// **Order does not matter here, and it used to.**
///
/// [`rejoin_fixed_expressions`] took the *first* entry that matched, so a shorter entry standing
/// above one it is a prefix of won and left the tail behind: `による` above `によると` turned
/// によると into による | と, and `に対し` above `に対して` did the same to the て. Both became live
/// hazards the moment the second group below was added, because `による` was already here.
///
/// That was first answered by sorting longest-first and adding a test to keep it sorted — which
/// polices a hazard instead of removing it, and leaves a rule a future entry can still break. The
/// matcher now takes the **longest** match, so the ordering is free. The list stays grouped by
/// length because it reads better that way, and nothing depends on it.
///
/// The second group is from `ud_gold.rs`: において, における, に対する, に関する, によると and
/// ために are among the long units annotators keep whole and this analyzer cut apart.
///
/// **そういえば, 次第に, 非常に and だが come from the domain instead** —
/// `segmentation_gold.rs` scores both analyzers against hand-segmented captures of manga and game
/// text, and these are five of the eight words Ichiran found there that this analyzer cut in half.
/// That is a different and better source than GSD for this app: news gold says である and ので,
/// and a reader of manga meets neither.
///
/// **どうして is the one that got away, and it is out because this mechanism cannot express it.**
/// It is どう + して, and by the time `rejoin_fixed_expressions` sees the tokens, して has already
/// been composed into して来た as a te-linked helper — so no run of *whole* tokens spells it.
/// Recovering it would mean running fixed expressions before composition, ahead of every
/// て-compound in the language, which is a large risk for one occurrence. Left split, and recorded
/// here so the next person does not rediscover it as a typo.
///
/// **`といった` and `だけでなく` are deliberately absent**, though gold lists both. といった is not
/// safely one unit — 「…」と言った is a quotative particle and a conjugated verb, and joining the
/// spelling would swallow that reading wherever someone is being quoted. だけでなく ends in an
/// inflected 形容詞. Neither is data; both are judgements, and they want measuring separately.
const FIXED_EXPRESSIONS: &[&str] = &[
    // Fifteen bytes.
    "そういえば",
    // Twelve.
    "によると",
    "に対して",
    "によって",
    "にとって",
    "について",
    "における",
    "において",
    "に対する",
    "に関する",
    "に対し",
    "次第に",
    "非常に",
    "という",
    "として",
    "による",
    "により",
    "ために",
    // Six.
    "だが",
];

/// Put the pieces of a [`FIXED_EXPRESSIONS`] entry back together.
///
/// Built on [`compound`] so there is one definition of how several tokens become one, then two
/// fields are overridden: `compound` takes the head's dictionary form, which is right for
/// 伝わってきます and wrong here. として's head is と, and offering と as the dictionary form would
/// send the reader to the particle — the same mistake in the panel that the matcher was making
/// in the grammar view.
fn fixed_expression(components: Vec<Token>) -> Token {
    let mut token = compound(components);
    token.dict_form = token.surface.clone();
    token.lemma_reading = token.reading.clone();
    token
}

/// Merge any run of tokens that spells a fixed expression.
///
/// Runs after [`compose`], so it sees learner-sized words rather than raw UniDic pieces, and it
/// never merges across a gap: the offsets have to meet, which is the same guard `compose` uses to
/// avoid reading through a line break.
/// っけ after a copula is part of the same word: そうだっけ, そうだったっけ, じゃなかったっけ.
///
/// **Keyed on the copula's lemma, not on the surface だ.** UniDic hands the four forms over
/// differently — だ is 助動詞-ダ 終止形, だった is already folded by [`compose_component`], でした is
/// 助動詞-デス, and じゃなかった is the negative past of the same lemma — but every one of them
/// arrives with `dict_form` だ or です. Matching the surface would have caught one of the four and
/// left じゃなかったっけ, which is the form the captures actually contain.
///
/// It also settles the negative case for free. 行ったっけ is 行った + っけ with lemma 行く, so a verb
/// followed by っけ is excluded by the same test rather than by a separate rule — っけ attaches to
/// plenty of predicates and only the copula is being joined here.
///
/// **Why a pass over tokens rather than a branch in [`compose`].** だったっけ needs だっ + た folded
/// first, which `compose_component` already does; reaching っけ from inside `compose` would mean
/// duplicating that. This runs after, in the same shape as [`rejoin_fixed_expressions`].
///
/// **The copula is extended rather than compounded, and that is load-bearing.** [`compound`]
/// builds with `..Default::default()` and so returns a token with no conjugation at all — the
/// same trap [`compose_copula`] documents for である. Tense and negation are exactly what a reader
/// needs from だったっけ against だっけ, so the copula keeps its own fields and っけ joins its
/// components.
fn attach_terminal_kke(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    for token in tokens {
        let joins = token.surface == "っけ"
            && out.last().is_some_and(|copula| {
                matches!(copula.dict_form.as_str(), "だ" | "です")
                    // Contiguous only: a gap is a line break or whitespace, and っけ belonging to
                    // the next utterance must not be pulled back onto this one.
                    && copula.end.is_some()
                    && copula.end == token.start
            });
        if !joins {
            out.push(token);
            continue;
        }
        let copula = out.pop().expect("checked by is_some_and");
        out.push(with_terminal_kke(copula, token));
    }
    out
}

/// Put っけ on the end of a copula, keeping everything the copula already knew.
fn with_terminal_kke(copula: Token, kke: Token) -> Token {
    let surface = format!("{}{}", copula.surface, kke.surface);
    let reading = format!("{}{}", copula.reading, kke.reading);
    let end = kke.end;
    // One level of parts, like every other composition here: a copula that was itself folded
    // (だっ + た) contributes its pieces rather than nesting them inside a wrapper.
    let mut components = if copula.components.is_empty() {
        vec![copula.clone()]
    } else {
        copula.components.clone()
    };
    components.push(kke);
    let mut token = Token {
        surface,
        reading,
        end,
        components,
        ..copula
    };
    let annotated = crate::construction::annotate(&mut token, ConstructionIdentity::CopulaRecall);
    debug_assert!(annotated, "guarded copula recall components must annotate");
    token
}

fn rejoin_fixed_expressions(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut index = 0;

    while index < tokens.len() {
        // **The longest match wins, rather than the first one listed.**
        //
        // This took whichever entry matched first, which made the order of the list load-bearing:
        // `による` above `によると` matched the shorter one and left the と stranded, and `に対し`
        // above `に対して` did the same to the て. Both hazards were introduced the moment the
        // second group of entries was added.
        //
        // That was first answered by sorting the list longest-first and writing a test to keep it
        // sorted — policing the hazard rather than removing it, and leaving a rule that a future
        // entry could still break in a way only a test would explain. Choosing the longest match
        // here makes the ordering irrelevant, so there is nothing left to get wrong.
        let mut taken: Option<usize> = None;
        for expression in FIXED_EXPRESSIONS {
            let mut surface = String::new();
            let mut count = 0usize;
            for token in &tokens[index..] {
                if count > 0 {
                    let previous_end = tokens[index + count - 1].end;
                    if previous_end.is_none() || previous_end != token.start {
                        break;
                    }
                }
                surface.push_str(&token.surface);
                count += 1;
                if surface.len() >= expression.len() {
                    break;
                }
            }
            // Two or more: a single token already spelling the expression is not split and
            // needs nothing done to it.
            if count > 1 && surface == *expression && taken.is_none_or(|best| count > best) {
                taken = Some(count);
            }
        }

        match taken {
            Some(count) => {
                out.push(fixed_expression(tokens[index..index + count].to_vec()));
                index += count;
            }
            None => {
                out.push(tokens[index].clone());
                index += 1;
            }
        }
    }

    out
}

fn compound(components: Vec<Token>) -> Token {
    let head = components.first().expect("compound has a head");
    Token {
        surface: components
            .iter()
            .map(|part| part.surface.as_str())
            .collect(),
        dict_form: head.dict_form.clone(),
        reading: components
            .iter()
            .map(|part| part.reading.as_str())
            .collect(),
        lemma_reading: head.lemma_reading.clone(),
        start: head.start,
        end: components.last().and_then(|part| part.end),
        components,
        ..Default::default()
    }
}

/// The token's own conjugation, when UniDic says it has one.
fn conjugation(
    dict_form: &str,
    lemma_reading: &str,
    class: &str,
    form: &str,
) -> Option<Conjugation> {
    if class.is_empty() && form.is_empty() {
        return None;
    }
    Some(Conjugation {
        lemma: dict_form.to_string(),
        reading: lemma_reading.to_string(),
        props: vec![Prop {
            pos: normalize_pos(class, dict_form),
            form: normalize_form(form, &normalize_pos(class, dict_form)),
            neg: false,
            fml: false,
            via_depth: 0,
            base_reading: lemma_reading.to_string(),
            is_base: true,
        }],
    })
}

/// Translate UniDic conjugation classes into the codes the normalized model and formation
/// engine share with Ichiran.
///
/// UniDic writes what IPADIC wrote with `・` using `-`, names the irregulars in full
/// (`カ行変格` for `カ変・クル`), and splits `一段` into `上一段-*` and `下一段-*`.
///
/// **`dict_form` is here for one word.** IPADIC had a class of its own for 行く's irregular
/// て-form, `五段・カ行促音便` → `v5k-s`; UniDic files 行く as plain `五段-カ行` and records the
/// irregularity elsewhere. Without the special case 行って would be generated as 行いて.
fn normalize_pos(class: &str, dict_form: &str) -> String {
    if dict_form == "行く" || dict_form == "逝く" || dict_form == "往く" {
        return "v5k-s".to_string();
    }
    let code = match class {
        value if value.starts_with("上一段") || value.starts_with("下一段") => "v1",
        value if value.starts_with("カ行変格") => "vk",
        value if value.starts_with("サ行変格") => "vs-i",
        value if value.starts_with("形容詞") => "adj-i",
        value if value.starts_with("五段-カ行") => "v5k",
        value if value.starts_with("五段-ガ行") => "v5g",
        value if value.starts_with("五段-サ行") => "v5s",
        value if value.starts_with("五段-タ行") => "v5t",
        value if value.starts_with("五段-ナ行") => "v5n",
        value if value.starts_with("五段-バ行") => "v5b",
        value if value.starts_with("五段-マ行") => "v5m",
        value if value.starts_with("五段-ラ行") => "v5r",
        // UniDic writes the ワ行 class as 五段-ワア行, for the あ that appears in 買わない.
        value if value.starts_with("五段-ワ") => "v5u",
        other => return other.to_string(),
    };
    code.to_string()
}

/// Translate UniDic's 活用形 into the form vocabulary the model is defined in.
///
/// This is the analyzer's job, not the grammar layer's: `project notes` §2 has the grammar
/// engine consume normalized tokens and nothing analyzer-specific, and
/// [`Form::classify`](crate::grammar::Form::classify) reads English labels because
/// that is what the model speaks. Passing 仮定形 through untranslated meant every
/// form-based match rule silently never fired under this analyzer, and the grammar
/// ladder named every form "Other form".
///
/// UniDic describes *stems* — composition above promotes recognized neighbouring
/// auxiliaries into properties, while an isolated token can only report its own stem.
/// An unrecognized label is passed through for `classify` to call unclassified rather
/// than being forced into a more confident answer than the dictionary supplied.
fn normalize_form(form: &str, pos: &str) -> String {
    // UniDic suffixes almost every label with the sound change that produced it —
    // 連用形-促音便, 連用形-撥音便, 終止形-一般 — and the stem it names is the part before the
    // dash. Matching on the whole string would leave every one of them unclassified, which is
    // the failure this function exists because of.
    let stem = form.split('-').next().unwrap_or(form);
    let normalized = match stem {
        // UniDic splits IPADIC's 基本形 into the terminal and attributive uses of the same
        // shape. Both are non-past; nothing downstream distinguishes them yet.
        "終止形" | "連体形" => "Non-past",
        "仮定形" | "已然形" => "Provisional (~eba)",
        "意志推量形" => "Volitional",
        // **The same UniDic label, two names in English.** 高く is the *adverbial* form of an
        // adjective; 読み is the *continuative* of a verb. UniDic files both as 連用形 and only
        // the part of speech separates them, so calling every one continuative told a learner
        // that 高く links clauses. Measured against Ichiran at 12 occurrences in 1,858 sentences,
        // where it says Adverbial and this said Continuative.
        "連用形" if pos == "adj-i" => "Adverbial",
        "連用形" => "Continuative",
        "未然形" => "Continuative",
        "命令形" => "Imperative",
        // Kept for the labels UniDic shares with IPADIC's vocabulary rather than renaming.
        "体言接続" => "Attributive",
        // An i-adjective's 語幹 is the stem 〜そう and 〜すぎる attach to (楽し|そう), the form
        // Ichiran reports as "Adjective Stem". Left raw, it read as unclassified, and the same
        // word got a stem under one analyzer and nothing under the other.
        "語幹" if pos == "adj-i" => "Adjective Stem",
        "語幹" => return form.to_string(),
        _ => return form.to_string(),
    };
    normalized.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- the dictionary adapter contract ----------------------------------------------------
    //
    // Rows copied from SudachiDict 20260723's own source lexicon (`small_lex.csv`), as
    // `lindera-sudachidict` returns them after the surface: display form, POS ×4, conjugation
    // type and form, reading, normalised form, dictionary-form ID, then split data. None of the
    // Japanese below was written for the test.

    fn sudachi_row(fields: &[&str]) -> Vec<String> {
        fields.iter().map(|field| field.to_string()).collect()
    }

    fn sudachi_forms() -> SudachiForms {
        // The one entry these rows reference: line 684363 of the lexicons, 読む ヨム.
        SudachiForms(
            [(684_363, ("読む".to_string(), "ヨム".to_string()))]
                .into_iter()
                .collect(),
        )
    }

    /// An inflected verb takes its dictionary form and that form's reading from the entry its
    /// ID names — never its own reading, and never the normalised form column.
    #[test]
    fn a_sudachi_inflection_resolves_its_dictionary_form_through_its_id() {
        let row = sudachi_row(&[
            "読み",
            "動詞",
            "一般",
            "*",
            "*",
            "五段-マ行",
            "連用形-一般",
            "ヨミ",
            "読む",
            "684363",
            "A",
            "*",
            "*",
            "*",
            "*",
        ]);
        let established = establish_sudachi("読み", &row, &sudachi_forms());
        assert_eq!(established.dict_form, "読む");
        assert_eq!(established.lemma_reading, "よむ");
        assert_eq!(
            established.reading, "よみ",
            "the token's own reading, as Sudachi spells it"
        );
        assert_eq!(
            (
                established.pos.as_str(),
                established.class.as_str(),
                established.form.as_str()
            ),
            ("動詞", "五段-マ行", "連用形-一般")
        );
    }

    /// An uninflected entry marked `*` is its own dictionary form.
    #[test]
    fn a_sudachi_entry_marked_as_its_own_form_keeps_its_surface() {
        let row = sudachi_row(&[
            "本",
            "名詞",
            "普通名詞",
            "一般",
            "*",
            "*",
            "*",
            "ホン",
            "本",
            "*",
            "A",
            "*",
            "*",
            "*",
            "012274",
        ]);
        let established = establish_sudachi("本", &row, &sudachi_forms());
        assert_eq!(
            (
                established.dict_form.as_str(),
                established.lemma_reading.as_str()
            ),
            ("本", "ほん")
        );
    }

    /// **The normalised form is never the lookup identity.** 1佐 normalises to 一佐; the entry a
    /// reader is sent to is the one written.
    #[test]
    fn the_sudachi_normalised_form_is_never_substituted_for_the_dictionary_form() {
        let row = sudachi_row(&[
            "1佐",
            "名詞",
            "普通名詞",
            "一般",
            "*",
            "*",
            "*",
            "イッサ",
            "一佐",
            "*",
            "A",
            "*",
            "*",
            "*",
            "*",
        ]);
        let established = establish_sudachi("1佐", &row, &sudachi_forms());
        assert_eq!(established.dict_form, "1佐");
    }

    /// **Absent, not manufactured.** `+` is filed as an inflected verb (タス, 連体形) with no
    /// dictionary-form ID — one of eight such symbols in the source data. Its surface is not a
    /// dictionary form of anything, and its reading is not a lemma reading.
    #[test]
    fn an_inflected_sudachi_entry_without_an_id_has_no_dictionary_form() {
        let row = sudachi_row(&[
            "+",
            "動詞",
            "一般",
            "*",
            "*",
            "五段-サ行",
            "連体形-一般",
            "タス",
            "+",
            "*",
            "A",
            "*",
            "*",
            "*",
            "*",
        ]);
        let established = establish_sudachi("+", &row, &sudachi_forms());
        assert_eq!(established.dict_form, "");
        assert_eq!(established.lemma_reading, "");
        assert_eq!(established.reading, "たす");
    }

    /// **A placeholder is not a reading.** SudachiDict writes キゴウ for ♥ and 、 for 、; neither
    /// is something to print over the character. Found by agreement with Ichiran dropping to 76%.
    #[test]
    fn sudachi_symbol_placeholders_are_absent_readings() {
        let heart = sudachi_row(&[
            "♥",
            "補助記号",
            "一般",
            "*",
            "*",
            "*",
            "*",
            "キゴウ",
            "♥",
            "*",
            "A",
            "*",
            "*",
            "*",
            "*",
        ]);
        assert_eq!(establish_sudachi("♥", &heart, &sudachi_forms()).reading, "");
        let comma = sudachi_row(&[
            "、",
            "補助記号",
            "読点",
            "*",
            "*",
            "*",
            "*",
            "、",
            "、",
            "*",
            "A",
            "*",
            "*",
            "*",
            "*",
        ]);
        assert_eq!(
            establish_sudachi("、", &comma, &sudachi_forms()).reading,
            ""
        );
    }

    /// A katakana word keeps its own spelling as its reading, as it does from UniDic.
    #[test]
    fn a_sudachi_katakana_word_reads_as_written() {
        let ring = sudachi_row(&[
            "リング",
            "名詞",
            "普通名詞",
            "一般",
            "*",
            "*",
            "*",
            "リング",
            "リング",
            "*",
            "A",
            "*",
            "*",
            "*",
            "014161/022804",
        ]);
        assert_eq!(
            establish_sudachi("リング", &ring, &sudachi_forms()).reading,
            "リング"
        );
    }

    /// An ID the side table does not hold leaves the form absent rather than guessing.
    #[test]
    fn an_unresolvable_sudachi_id_leaves_the_form_absent() {
        let row = sudachi_row(&[
            "読み",
            "動詞",
            "一般",
            "*",
            "*",
            "五段-マ行",
            "連用形-一般",
            "ヨミ",
            "読む",
            "684363",
            "A",
            "*",
            "*",
            "*",
            "*",
        ]);
        let empty = SudachiForms(std::collections::HashMap::new());
        let established = establish_sudachi("読み", &row, &empty);
        assert_eq!(
            (
                established.dict_form.as_str(),
                established.lemma_reading.as_str()
            ),
            ("", "")
        );
    }

    fn analyzer() -> LinderaAnalyzer {
        LinderaAnalyzer::new().expect("the dictionary is compiled in")
    }

    /// **Which stage actually produces each contested grouping.**
    ///
    /// **Where the reported groupings are decided — and it is not this project's composition.**
    ///
    /// Three failures were reported from the reader. Two of them are groupings, and the natural
    /// suspicion falls on our own `compose`/`rejoin_fixed_expressions` pass, because
    /// [`LinderaAnalyzer::analyze`] returns all of it and a caller cannot see the seam. For these
    /// three, that suspicion is wrong: the tokenizer decides every one, and composition leaves
    /// each untouched.
    ///
    /// That matters for what a fix would have to be. Rejoining `なん`+`で` here would be adding a
    /// fixed expression, not correcting a mistake in the joining we already do — and, as
    /// `reported_examples.rs` shows, it would not touch the retrieval failure that produced the
    /// reader-visible wrong answer anyway.
    ///
    /// Short expressions only, and no captured text: `corpus/` and `scratch/` are gitignored
    /// because that material is copyrighted, so the sentences live in `reported_examples.rs`,
    /// which reads them from disk. UniDic is embedded, so this needs nothing installed.
    #[test]
    fn the_reported_groupings_are_the_tokenizer_s_and_composition_leaves_them_alone() {
        let analyzer = analyzer();
        let surfaces = |text: &str| -> (Vec<String>, Vec<String>) {
            let pieces = analyzer.pieces(text).expect("tokenizes");
            let before: Vec<String> = pieces
                .iter()
                .map(|piece| piece.token.surface.clone())
                .collect();
            let after: Vec<String> = attach_terminal_kke(rejoin_fixed_expressions(compose(pieces)))
                .iter()
                .map(|token| token.surface.clone())
                .collect();
            (before, after)
        };

        for (text, expected) in [
            // Split by the tokenizer. `なん` is tagged a pronoun, so nothing downstream has a
            // reason to treat the pair as one word — and `なんで` is not in `FIXED_EXPRESSIONS`.
            ("なんで", vec!["なん", "で"]),
            // Both spellings of the pronoun split from the pluralizing suffix identically, which
            // is what lets the kanji-written record stand in for the kana the report showed.
            ("あなたたち", vec!["あなた", "たち"]),
            ("貴方たち", vec!["貴方", "たち"]),
            // **One token, from the tokenizer.** Any "name reading" the reader sees for this is
            // decided later, in retrieval and merge — not here.
            ("神姫", vec!["神姫"]),
        ] {
            let (before, after) = surfaces(text);
            assert_eq!(before, expected, "the tokenizer's own cut of {text}");
            assert_eq!(
                after, before,
                "composition changed {text}, so this project owns the grouping after all"
            );
        }

        // The control that makes the claim mean something: composition *does* act, on a shape it
        // is meant to. Without this, the assertions above would also pass if `compose` were a
        // no-op or never ran.
        let (before, after) = surfaces("そうなんです");
        assert_ne!(
            before, after,
            "composition is live — it is simply not what groups the three reported cases"
        );
    }

    /// The colloquial engine comparison attributed 633 of 644 disagreements to "the analyzer",
    /// which read as exonerating this project's own grouping logic. It does not:
    /// [`LinderaAnalyzer::analyze`] returns
    /// `attach_terminal_kke(rejoin_fixed_expressions(compose(pieces)))`, so everything after the
    /// tokenizer is ours and the comparison could not see the seam.
    ///
    /// This walks the representative cases through every stage and pins where each grouping is
    /// introduced, so responsibility is established rather than inferred. Run it with
    /// `--nocapture` to read the trace; the assertions below are what gate it.
    ///
    /// It needs nothing installed — UniDic is embedded — so it gates an ordinary test run.
    #[test]
    fn each_stage_is_attributed_for_the_contested_groupings() {
        let analyzer = analyzer();
        let stages = |text: &str| -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
            let pieces = analyzer.pieces(text).expect("tokenizes");
            let tokenizer: Vec<String> = pieces
                .iter()
                .map(|piece| piece.token.surface.clone())
                .collect();
            let composed = compose(pieces);
            let after_compose: Vec<String> =
                composed.iter().map(|token| token.surface.clone()).collect();
            let rejoined = rejoin_fixed_expressions(composed);
            let after_rejoin: Vec<String> =
                rejoined.iter().map(|token| token.surface.clone()).collect();
            let finished = attach_terminal_kke(rejoined);
            let after_kke: Vec<String> =
                finished.iter().map(|token| token.surface.clone()).collect();
            (tokenizer, after_compose, after_rejoin, after_kke)
        };

        println!("\n=== what each stage does to the contested groupings ===");
        for text in [
            "そうなんです",
            "行くんです",
            "そうなんだ",
            "重要である",
            "映画とかドラマ",
            "いいですよね",
            "でも行く",
            "なんか変",
            "行くのに",
            "行くかな",
            "行くかも",
            "何か食べる",
            "雨なので",
            "行くのか",
            "東京には",
            "確かに",
            "外そうとする",
            "なんとかしない",
            "ややばい",
            "2019年",
            "1週間",
        ] {
            let (tokenizer, composed, rejoined, kke) = stages(text);
            println!("  {text}");
            println!("    tokenizer  {}", tokenizer.join(" | "));
            println!("    compose    {}", composed.join(" | "));
            println!("    rejoin     {}", rejoined.join(" | "));
            println!("    kke        {}", kke.join(" | "));
        }

        // **なん is ours.** The tokenizer separates な and ん; `compose_explanatory_nan` joins
        // them. The comparison recorded four chat cases of Lindera's `なん` against Ichiran's
        // `な` | `ん` and filed them under "analyzer".
        let (tokenizer, composed, _, _) = stages("そうなんです");
        assert!(
            tokenizer.iter().any(|surface| surface == "ん"),
            "the tokenizer separates ん: {tokenizer:?}"
        );
        assert!(
            composed.iter().any(|surface| surface == "なん"),
            "and compose joins it into なん: {composed:?}"
        );

        // **とか is not ours.** Neither stage touches it, so a disagreement about it really is
        // upstream — which is the distinction the comparison could not draw for itself.
        let (tokenizer, composed, rejoined, kke) = stages("映画とかドラマ");
        assert_eq!(
            tokenizer, composed,
            "compose leaves とか alone: {tokenizer:?} -> {composed:?}"
        );
        assert_eq!(rejoined, kke, "and so do the later stages: {rejoined:?}");

        // **よね is not ours either.**
        let (tokenizer, _, _, kke) = stages("いいですよね");
        assert_eq!(
            tokenizer, kke,
            "no stage regroups よね: {tokenizer:?} -> {kke:?}"
        );

        // **The number readings are ours**, and this is where the two proposed defects live:
        // the tokenizer emits the digits and the counter separately, and `compose_digits` is
        // what decides the reading. So neither is a tokenizer fault.
        for text in ["2019年", "1週間"] {
            let (tokenizer, composed, _, _) = stages(text);
            assert!(
                tokenizer.len() > composed.len(),
                "compose_digits joins {text}: {tokenizer:?} -> {composed:?}"
            );
        }
    }

    /// An adjective's 連用形 is adverbial; a verb's is continuative.
    ///
    /// Both halves matter. UniDic gives the two the same label, so a fix keyed on the label
    /// alone renames the verbs too — which is why 読み is asserted here beside 高く.
    #[test]
    fn an_adjectives_renyou_is_adverbial_and_a_verbs_is_continuative() {
        let analyzer = analyzer();
        let forms = |text: &str| -> Vec<String> {
            analyzer
                .analyze(text)
                .expect("analyzes")
                .tokens
                .iter()
                .filter_map(|token| token.conjugation.as_ref())
                .flat_map(|conj| conj.props.iter().map(|prop| prop.form.clone()))
                .collect()
        };

        let adjective = forms("空が高くなった。");
        assert!(
            adjective.iter().any(|form| form == "Adverbial"),
            "高く is the adverbial form of 高い, got {adjective:?}"
        );

        let verb = forms("本を読み、家に帰る。");
        assert!(
            verb.iter().any(|form| form == "Continuative"),
            "読み is the continuative of 読む, got {verb:?}"
        );
    }

    /// Digits and symbols carry readings; kana does not; a number does not either.
    ///
    /// Four rules meeting in one function, and each of the others is the reason this one cannot
    /// simply be widened:
    ///
    /// - a lone digit with nothing counting it reads — ４ is よん;
    /// - ％ counts, and ５％ is one token whose reading is still ごぱーせんと;
    /// - kana does **not** get rewritten, or コーヒー becomes こーひー;
    /// - a number gets **no** reading, because 2011 is にせんじゅういち as a year and にぜろいちいち
    ///   only as four separate digits, and nothing here knows which is meant.
    ///
    /// **This used to assert that １ in １回 reads いち, and now asserts that １回 is one token.**
    /// The digit's reading was never wrong; the unit was. `ud_gold.rs` measures number-and-counter
    /// among the most frequent long units annotators keep whole, and §4a makes the long unit the
    /// score. The reading is not lost by joining — it is deferred to the dictionary, which is the
    /// only party that knows 1日 is ついたち.
    #[test]
    fn digits_and_symbols_read_but_kana_and_numbers_are_left_alone() {
        let analyzer = analyzer();
        let reading_of = |text: &str, surface: &str| -> String {
            analyzer
                .analyze(text)
                .expect("analyzes")
                .tokens
                .iter()
                .find(|token| token.surface == surface)
                .unwrap_or_else(|| panic!("{surface} is a token of {text}"))
                .reading
                .clone()
        };

        // A digit with no counter after it is still a digit, and still reads.
        assert_eq!(reading_of("４と５。", "４"), "よん");
        // ％ counts ５ and joins it, and the reading survives the join concatenated — which is
        // what let `is_counter` stop excluding symbols. Read the reading, not just the surface:
        // an empty one here would mean `compose_digits` took the bare-number branch by mistake.
        assert_eq!(reading_of("５％の人。", "５％"), "ごぱーせんと");

        // With a counter it is one token, unread here and resolved by the dictionary later.
        let surfaces_of = |text: &str| -> Vec<String> {
            analyzer
                .analyze(text)
                .expect("analyzes")
                .tokens
                .iter()
                .map(|token| token.surface.clone())
                .collect()
        };
        // **And the ASCII form does not join, which is pinned so nobody assumes it does.** UniDic
        // tags only ％ as 助数詞可能; `%` never reaches `is_counter`. This cost a gold measurement
        // once already — `ud_gold.rs` is ASCII news, so it cannot see the fullwidth rule at all.
        assert_eq!(surfaces_of("5%の人。"), vec!["5", "%", "の", "人", "。"]);
        for (text, whole) in [
            ("１回だけ。", "１回"),
            ("4月に行く。", "4月"),
            // 日 and 人 are 接尾辞-名詞的 where 月 is 名詞-助数詞可能. Both tagging shapes are
            // counters and `is_counter` reads both; these two cases are what pin that.
            ("17日に会う。", "17日"),
            ("3人が来た。", "3人"),
            ("2011年に。", "2011年"),
        ] {
            let got = surfaces_of(text);
            assert!(
                got.iter().any(|token| token == whole),
                "{text} should contain {whole}, got {got:?}"
            );
        }
        // **A counted number carries a reading, because the contract requires one of every word.**
        // It may be the parts concatenated rather than the irregular truth — see the note on
        // `compose_digits` — but `analyzer_contract.rs` does not permit a word with no furigana,
        // and clearing this is what broke it.
        assert!(
            !reading_of("4月に行く。", "4月").is_empty(),
            "a counted number is a word and every word carries a reading"
        );

        // Kana spells itself; rewriting it puts wrong furigana on the page.
        assert_eq!(reading_of("コーヒーを飲む。", "コーヒー"), "コーヒー");

        // A multi-digit run with no counter is still one token and deliberately unread.
        assert_eq!(reading_of("2011と2012。", "2011"), "");

        // And the evidence-backed override, now that KWDLC has spoken.
        assert_eq!(reading_of("日本に行く。", "日本"), "にほん");
    }

    /// A noun keeps its prefix and suffix, and a letter run stays a word.
    ///
    /// Found chasing オバケ in the capture corpus, which came back as オ | バケ. The same UniDic
    /// shape splits beginner vocabulary — お茶 was お | 茶 — so the cases below are mostly not
    /// katakana at all.
    ///
    /// **Readings are asserted only where concatenation is right.** お茶 is おちゃ because お is
    /// お and 茶 is ちゃ; お母さん is *not* asserted here, because 母 alone is はは and only the
    /// dictionary pass can know better. See [`compose_nominal`].
    #[test]
    fn a_noun_keeps_its_prefix_and_suffix() {
        let analyzer = analyzer();
        let surfaces = |text: &str| -> Vec<String> {
            analyzer
                .analyze(text)
                .expect("analyzes")
                .tokens
                .iter()
                .map(|token| token.surface.clone())
                .collect()
        };

        for (text, whole) in [
            ("お茶を飲む。", "お茶"),
            ("お母さんが来た。", "お母さん"),
            ("オバケが出る。", "オバケ"),
            ("新入生の歓迎。", "新入生"),
            ("大成功だった。", "大成功"),
            ("ESCキーを押す。", "ESC"),
            ("USBを挿す。", "USB"),
        ] {
            let got = surfaces(text);
            assert!(
                got.iter().any(|token| token == whole),
                "{text} should contain {whole}, got {got:?}"
            );
        }

        // **A prefix must not eat the first digit of a number.** 約80分 arrives as 約 | 8 | 0 | 分
        // and came back 約8 | 0 | 分 — the number cut in half, with やく and はち concatenated into
        // a reading for 約8 that nobody says. Found by the JESC corpus, which is what a register
        // this app is actually read in turns up that news and Tatoeba text does not.
        // The counter joins too, so the whole reads 約 | 80分 | 前 — which is what Ichiran gives
        // for this line and what gold's long units want.
        let number = surfaces("約80分前に着く。");
        assert!(
            number.iter().any(|token| token == "80分"),
            "80分 should survive as one number and counter, got {number:?}"
        );
        assert!(
            !number.iter().any(|token| token == "約8"),
            "約 must not swallow the leading digit, got {number:?}"
        );

        // A bare noun is untouched, and the reading of a safe join is asserted.
        let plain = analyzer.analyze("本を読む。").expect("analyzes");
        assert!(plain.tokens.iter().any(|token| token.surface == "本"));
        let tea = analyzer.analyze("お茶を飲む。").expect("analyzes");
        let tea = tea
            .tokens
            .iter()
            .find(|token| token.surface == "お茶")
            .expect("お茶");
        assert_eq!(tea.reading, "おちゃ");
    }

    /// Casual contractions are one word, and the words they are spelled like are not.
    ///
    /// **The controls carry this test.** Every surface in [`is_contracted_auxiliary`] is also a
    /// real word — でる is 出る, とく is 説く, て is the ordinary 接続助詞 of 見ている — and the
    /// rule separates them by 品詞 alone. If that gate is ever dropped the first three assertions
    /// still pass and the last four fail, which is the only reason they are here.
    #[test]
    fn casual_contractions_are_one_word_and_real_verbs_are_not_swallowed() {
        let analyzer = analyzer();
        let surfaces = |text: &str| -> Vec<String> {
            analyzer
                .analyze(text)
                .expect("analyzes")
                .tokens
                .iter()
                .map(|token| token.surface.clone())
                .collect()
        };

        for (text, whole) in [
            ("彼は本を見てる。", "見てる"),
            ("遊んでる。", "遊んでる"),
            ("言ってた。", "言ってた"),
            ("行っちゃう。", "行っちゃう"),
            ("食べとく。", "食べとく"),
            ("入っちゃ駄目。", "入っちゃ"),
        ] {
            let got = surfaces(text);
            assert!(
                got.iter().any(|token| token == whole),
                "{text} should contain {whole}, got {got:?}"
            );
        }

        // The same spellings as ordinary words, which UniDic marks a different part of speech.
        // とる is the one that most needs the part-of-speech gate: the dialectal 抜けとる is a
        // 助動詞 and this is the verb 取る, spelled the same in kana.
        for (text, word) in [
            ("水が出る。", "出る"),
            ("話を説く。", "説く"),
            ("写真を取る。", "取る"),
        ] {
            let got = surfaces(text);
            assert!(
                got.iter().any(|token| token == word),
                "{word} is a verb, not an auxiliary, got {got:?}"
            );
        }

        // And the uncontracted form still composes the way it always did.
        let got = surfaces("彼は本を見ている。");
        assert!(
            got.iter().any(|token| token == "見ている"),
            "見ている is unchanged, got {got:?}"
        );
    }

    /// A contracted helper is explained as the helper it stands for, and なきゃ as a provisional.
    ///
    /// **One word was not enough.** The test above holds 見てる together; this one is the half it
    /// did not ask — what the reader is then told. Each word had no parts, so the 〜ている rule had
    /// nothing to find, and 見てる was explained as plain 見る. Measured over the Jitendex
    /// sentences: 0 of 194 〜てる, 0 of 23 〜ちゃう and 0 of 11 〜とく. The same sentences as
    /// above, asked through the real detector.
    #[test]
    fn a_contracted_helper_names_the_pattern_it_contracts() {
        let analyzer = analyzer();
        let helpers = |text: &str, whole: &str| -> Vec<&'static str> {
            let analysis = analyzer.analyze(text).expect("analyzes");
            let token = analysis
                .tokens
                .iter()
                .find(|token| token.surface == whole)
                .unwrap_or_else(|| panic!("{text} should contain {whole}"));
            crate::grammar::compounds::detect(token)
                .iter()
                .map(|found| found.pattern_id)
                .collect()
        };
        for (text, whole, pattern) in [
            ("彼は本を見てる。", "見てる", "te-iru"),
            ("遊んでる。", "遊んでる", "te-iru"),
            ("言ってた。", "言ってた", "te-iru"),
            ("行っちゃう。", "行っちゃう", "te-shimau"),
            ("食べとく。", "食べとく", "te-oku"),
        ] {
            assert_eq!(helpers(text, whole), vec![pattern], "{whole}");
        }
        // The uncontracted forms still name theirs, through the ordinary compound.
        assert_eq!(helpers("彼は本を見ている。", "見ている"), vec!["te-iru"]);

        // なきゃ is the negative provisional — "if (it) does not get through" — not a plain
        // negative. A Tatoeba sentence from the Jitendex examples ("It's pointless if English
        // speakers can't understand it").
        let analysis = analyzer
            .analyze("英語話者に通じなきゃ意味ないぞ。")
            .expect("analyzes");
        let word = analysis
            .tokens
            .iter()
            .find(|token| token.surface == "通じなきゃ")
            .expect("通じなきゃ is one word");
        let prop = &word.conjugation.as_ref().expect("conjugated").props[0];
        assert_eq!((prop.form.as_str(), prop.neg), ("Provisional (~eba)", true));
    }

    /// たら is the conditional, not the past, and both are 助動詞-タ.
    ///
    /// The past case is asserted beside it because that is what the fix could have broken: the
    /// two differ only by 活用形, so a rule keyed on the auxiliary alone gets one of them wrong
    /// whichever way it guesses.
    #[test]
    fn tara_is_conditional_where_ta_is_past() {
        let analyzer = analyzer();

        let conditional = analyzer
            .analyze("雨が降ったら中止です。")
            .expect("analyzes");
        let forms: Vec<String> = conditional
            .tokens
            .iter()
            .filter_map(|token| token.conjugation.as_ref())
            .flat_map(|conj| conj.props.iter().map(|prop| prop.form.clone()))
            .collect();
        assert!(
            forms.iter().any(|form| form.contains("Conditional")),
            "降ったら is a conditional, got {forms:?}"
        );
        assert!(
            !forms.iter().any(|form| form.contains("Past")),
            "降ったら is not a past tense, got {forms:?}"
        );

        let past = analyzer.analyze("彼は本を読んだ。").expect("analyzes");
        let forms: Vec<String> = past
            .tokens
            .iter()
            .filter_map(|token| token.conjugation.as_ref())
            .flat_map(|conj| conj.props.iter().map(|prop| prop.form.clone()))
            .collect();
        assert!(
            forms.iter().any(|form| form.contains("Past")),
            "読んだ is a past tense, got {forms:?}"
        );
    }

    /// Guards [`rejoin_fixed_expressions`], and the control is the important half.
    ///
    /// Merging by surface is the kind of fix that quietly eats correct output: と is a real
    /// particle far more often than it is the first half of として, so the second case here
    /// fails if the rejoin ever grows greedy enough to swallow it.
    ///
    /// The third asserts the point of the exercise rather than its mechanism. Split, 教師として
    /// produced a `particle-to` match on the と — the app telling a learner the quotative
    /// particle was there. Whole, that misfire is gone.
    #[test]
    fn fixed_expressions_are_rejoined_without_swallowing_ordinary_particles() {
        let analyzer = analyzer();

        let joined = analyzer
            .analyze("彼は教師として働いている。")
            .expect("analyzes");
        let surfaces: Vec<&str> = joined.tokens.iter().map(|t| t.surface.as_str()).collect();
        assert!(
            surfaces.contains(&"として"),
            "として should be one token, got {surfaces:?}"
        );

        let control = analyzer
            .analyze("彼はそう言うと部屋を出た。")
            .expect("analyzes");
        let surfaces: Vec<&str> = control.tokens.iter().map(|t| t.surface.as_str()).collect();
        assert!(
            surfaces.contains(&"と"),
            "a quotative と is not part of an expression, got {surfaces:?}"
        );

        let packs = crate::grammar::pack::all();
        let fired: Vec<String> = crate::grammar::matcher::find_across(&joined.tokens, &packs)
            .iter()
            .map(|found| found.pattern_id.clone())
            .collect();
        assert!(
            !fired.iter().any(|id| id == "particle-to"),
            "the と of として is not the quotative particle, but {fired:?} says it is"
        );
    }

    /// である is one word, it keeps its conjugation, and ordinary ある is left alone.
    ///
    /// **The conjugation assertions are the point.** `compound` drops it — it builds with
    /// `..Default::default()` — so the obvious way to write [`compose_copula`] produces a token
    /// that looks right in the reader and has nothing to say in the grammar panel. であった is
    /// where that shows.
    ///
    /// The controls carry the rest: 読んである and 本がある both put ある after something ending
    /// in で or preceded by it, and neither is the copula. If the 助動詞 gate is ever dropped they
    /// are what fails.
    /// A suffix adjective inflects, and its negation belongs to it.
    ///
    /// 子供っぽくない came back as three tokens with ない standing alone, so the reader was offered
    /// "not" as a separate word from the adjective it negates. っぽい is 接尾辞-形容詞的 in UniDic
    /// and every other field says ordinary adjective, so only the head gate needed widening.
    ///
    /// **All four tenses, because the negation is the point.** Affirmative and past already
    /// worked; っぽくない and っぽくなかった are what changed, and a rule that fixed the first and
    /// not the second would look right in the one example anybody checks.
    ///
    /// らしい is in the positives deliberately: it is the same UniDic class, so the fix reaches it,
    /// and a test that only ever named っぽい would hide how wide the rule actually is. The
    /// controls are an ordinary い-adjective and an independent ない, neither of which may move.
    #[test]
    fn a_suffix_adjective_carries_its_own_negation_and_tense() {
        let analyzer = analyzer();
        let find = |text: &str, surface: &str| -> Token {
            analyzer
                .analyze(text)
                .expect("analyzes")
                .tokens
                .into_iter()
                .find(|token| token.surface == surface)
                .unwrap_or_else(|| panic!("{surface} is a token of {text}"))
        };

        for (text, surface, lemma, reading, form, negated) in [
            (
                "それは子供っぽい。",
                "っぽい",
                "っぽい",
                "っぽい",
                "Non-past",
                false,
            ),
            (
                "それは子供っぽくない。",
                "っぽくない",
                "っぽい",
                "っぽくない",
                "Non-past",
                true,
            ),
            (
                "それは水っぽかった。",
                "っぽかった",
                "っぽい",
                "っぽかった",
                "Past (~ta)",
                false,
            ),
            (
                "それは子供っぽくなかった。",
                "っぽくなかった",
                "っぽい",
                "っぽくなかった",
                "Past (~ta)",
                true,
            ),
            (
                "彼は男らしくない。",
                "らしくない",
                "らしい",
                "らしくない",
                "Non-past",
                true,
            ),
        ] {
            let token = find(text, surface);
            assert_eq!(token.dict_form, lemma, "{text}: the suffix keeps its lemma");
            assert_eq!(token.reading, reading, "{text}: the written reading");
            let props = &token
                .conjugation
                .as_ref()
                .unwrap_or_else(|| panic!("{text}: {surface} must inflect"))
                .props;
            assert!(
                props.iter().any(|prop| prop.form == form),
                "{text}: expected {form}, got {:?}",
                props.iter().map(|prop| &prop.form).collect::<Vec<_>>()
            );
            assert_eq!(
                props.iter().any(|prop| prop.neg),
                negated,
                "{text}: negation must be part of the word"
            );
            let (start, end) = (token.start.expect("start"), token.end.expect("end"));
            assert_eq!(
                &text[start..end],
                surface,
                "{text}: the span is the surface"
            );
        }

        // **The noun in front is deliberately still separate.** Whether 子供っぽい is one word is a
        // dictionary question, and this rule does not answer it.
        let surfaces: Vec<String> = analyzer
            .analyze("それは子供っぽくない。")
            .expect("analyzes")
            .tokens
            .iter()
            .map(|token| token.surface.clone())
            .collect();
        assert!(
            surfaces.iter().any(|surface| surface == "子供"),
            "the noun stays its own token, got {surfaces:?}"
        );

        // Controls: an ordinary い-adjective and an independent ない must be untouched.
        let ordinary = find("山が高くない。", "高くない");
        assert_eq!(ordinary.dict_form, "高い");
        let independent = find("本がない。", "ない");
        assert_eq!(
            independent.dict_form, "ない",
            "ない with nothing to negate is its own word"
        );
    }

    /// Contracted てく is the motion helper, and the verb keeps it.
    ///
    /// 連れてって came back as 連れ | てっ | て: the verb, the motion and the connective as three
    /// separate words, with the auxiliary standing on its own as though it were a word a learner
    /// could look up. UniDic already says what it is — 助動詞, class 五段-カ行, lemma てく — and
    /// [`is_contracted_auxiliary`] already collects exactly this family, so てく joins とく, てる and
    /// ちゃう rather than getting a rule of its own.
    ///
    /// **Keyed on the lemma UniDic assigns, never on the letters てく.** A surface rule would
    /// claim every 出てくる and 見てくれ; this only fires where the tagger has already decided the
    /// piece is the てく auxiliary, so the negative controls below need no special casing.
    ///
    /// **The form comes from what follows, which is why both are tested.** 連れてって ends in the
    /// connective and is Conjunctive; 歩いてった ends in た and is Past. A rule that read the
    /// contraction itself would have called both the same thing.
    ///
    /// **The parts are cut, not grouped, and that is why they exist at all.** UniDic puts the て
    /// of 連れて inside the auxiliary's own piece, so [`contracted_auxiliary_components`]
    /// reconstructs 連れて and って from source offsets. Without them
    /// [`crate::grammar::compounds::detect`] has nothing to look at and the ていく note never
    /// reaches a reader who met the contracted form — which was the point of joining it.
    #[test]
    fn contracted_teku_is_part_of_the_verb_it_follows() {
        let analyzer = analyzer();
        let find = |text: &str, surface: &str| -> Token {
            analyzer
                .analyze(text)
                .expect("analyzes")
                .tokens
                .into_iter()
                .find(|token| token.surface == surface)
                .unwrap_or_else(|| panic!("{surface} is a token of {text}"))
        };

        for (text, surface, lemma, form) in [
            (
                "公園に連れてって。",
                "連れてって",
                "連れる",
                "Conjunctive (~te)",
            ),
            ("駅まで歩いてった。", "歩いてった", "歩く", "Past (~ta)"),
            ("もう出てく。", "出てく", "出る", "Non-past"),
        ] {
            let token = find(text, surface);
            assert_eq!(token.dict_form, lemma, "{text}: the head verb is the lemma");
            let props = &token
                .conjugation
                .as_ref()
                .unwrap_or_else(|| panic!("{text}: {surface} must carry a conjugation"))
                .props;
            assert!(
                props.iter().any(|prop| prop.form == form),
                "{text}: expected {form}, got {:?}",
                props.iter().map(|prop| &prop.form).collect::<Vec<_>>()
            );
            let (start, end) = (token.start.expect("start"), token.end.expect("end"));
            assert_eq!(
                &text[start..end],
                surface,
                "{text}: the span is the surface"
            );
        }

        // **The parts, and the explanation they unlock.** てく is a helper like ていく, and a reader
        // meeting 連れてって needs the same note as one meeting 連れていく. `compounds::detect` finds
        // it by looking for 行く among a compound's components after a te-form, so the parts are
        // not decoration — without them there is nothing for it to find.
        for (text, surface, lead, tail) in [
            ("公園に連れてって。", "連れてって", "連れて", "って"),
            ("駅まで歩いてった。", "歩いてった", "歩いて", "った"),
            ("もう出てく。", "出てく", "出て", "く"),
        ] {
            let token = find(text, surface);
            let parts: Vec<&str> = token
                .components
                .iter()
                .map(|part| part.surface.as_str())
                .collect();
            assert_eq!(
                parts,
                vec![lead, tail],
                "{text}: cut where the te-form ends"
            );
            assert_eq!(
                token.components[1].dict_form, "行く",
                "{text}: the helper is normalised to what `compounds` matches"
            );
            assert_eq!(
                token.components[1].lemma_reading, "いく",
                "{text}: read いく"
            );
            // Exact substrings of the source, contiguous, and adding back up to the whole.
            let (first, second) = (&token.components[0], &token.components[1]);
            assert_eq!(first.end, second.start, "{text}: the parts are contiguous");
            assert_eq!(
                first.start, token.start,
                "{text}: they start where the token does"
            );
            assert_eq!(second.end, token.end, "{text}: and end where it does");
            assert_eq!(
                format!("{}{}", first.surface, second.surface),
                token.surface,
                "{text}: the parts are the whole"
            );
            assert_eq!(
                &text[first.start.expect("s")..first.end.expect("e")],
                lead,
                "{text}: the lead is a real span of the source"
            );
            assert_eq!(
                crate::grammar::compounds::detect(&token)
                    .iter()
                    .map(|found| found.pattern_id)
                    .collect::<Vec<_>>(),
                vec!["te-iku"],
                "{text}: the ていく explanation is reachable"
            );
        }

        // The uncontracted form reaches the same place by the ordinary helper path, and must.
        let uncontracted = find("公園に連れていく。", "連れていく");
        assert_eq!(uncontracted.dict_form, "連れる");
        assert_eq!(
            uncontracted
                .components
                .iter()
                .map(|part| part.surface.as_str())
                .collect::<Vec<_>>(),
            vec!["連れて", "いく"],
            "the te-linked helper still supplies its parts"
        );
        assert_eq!(
            crate::grammar::compounds::detect(&uncontracted)
                .iter()
                .map(|found| found.pattern_id)
                .collect::<Vec<_>>(),
            vec!["te-iku"],
            "contracted and uncontracted get the same explanation"
        );

        // Controls: the other te-linked helpers are untouched, and 来る is not てく.
        for (text, surface, lemma) in [
            ("水を持ってくる。", "持ってくる", "持つ"),
            ("少し作ってください。", "作ってください", "作る"),
            ("駅まで歩いて来た。", "歩いて来た", "歩く"),
        ] {
            let token = find(text, surface);
            assert_eq!(token.dict_form, lemma, "{text}: unchanged by the てく rule");
        }
    }

    /// な + ん is one selectable group, and the spelling なん is never what identifies it.
    ///
    /// **The controls carry this test.** なんか is the pronoun 何 and 何を is the same word spelled
    /// in kanji; both would match any rule written on letters, and neither may reach this one.
    /// 静かな部屋 has the same な with no ん after it, 食べるんだ has the same ん with no な before it,
    /// and そうなの has の as a sentence-final particle rather than the nominaliser. Each is a
    /// piece of the construction standing alone, and each must stay alone.
    ///
    /// **The following copula is required but not consumed**, so じゃなかったっけ keeps its own
    /// negative past and its own components — asserted here, because a grouping rule that quietly
    /// ate the tense would look correct in the reader and be wrong in the panel.
    #[test]
    fn explanatory_nan_is_one_group_and_the_spelling_is_not_evidence() {
        let analyzer = analyzer();
        let tokens =
            |text: &str| -> Vec<Token> { analyzer.analyze(text).expect("analyzes").tokens };
        let find = |text: &str, surface: &str| -> Token {
            tokens(text)
                .into_iter()
                .find(|token| token.surface == surface)
                .unwrap_or_else(|| panic!("{surface} is a token of {text}"))
        };

        for text in [
            "彼は学生なんだ。",
            "ここは静かなんです。",
            "彼は学生なんじゃない。",
            "ここって禁足地なんじゃなかったっけ",
            // **だろう is in scope deliberately, and it is the one that costs a gold match.**
            // UD's dev-s234 groups ん forward into んだろ; this groups it back onto the copula,
            // because なん is the chunk a reader sees and can select. The disagreement is recorded
            // as a pinned exception in `ud_gold.rs`, not smoothed away here.
            "彼は平気なんだろう。",
            "何故平気なんだろ。",
        ] {
            let token = find(text, "なん");
            assert!(
                crate::model::is_explanatory_nan(&token),
                "{text}: the shared recogniser must agree"
            );
            assert_eq!(
                token.dict_form, "だ",
                "{text}: the copula's identity, not なん"
            );
            assert_eq!(token.lemma_reading, "だ", "{text}: and its reading");
            assert_eq!(token.reading, "なん", "{text}: read as written");
            assert!(
                token.conjugation.is_none(),
                "{text}: the parent claims no morphology of its own"
            );
            let parts: Vec<&str> = token
                .components
                .iter()
                .map(|part| part.surface.as_str())
                .collect();
            assert_eq!(parts, vec!["な", "ん"], "{text}: both roles stay visible");
            assert_eq!(
                token.components[1].lemma_reading, "の",
                "{text}: ん is の contracted, and that is what says so"
            );
            // Exact source slices, contiguous, adding back up to the group.
            let (start, end) = (token.start.expect("start"), token.end.expect("end"));
            assert_eq!(&text[start..end], "なん", "{text}: the span is the group");
            assert_eq!(token.components[0].end, token.components[1].start);
            assert_eq!(
                format!(
                    "{}{}",
                    token.components[0].surface, token.components[1].surface
                ),
                token.surface
            );
            // The group keys as the copula, so nothing a learner saved is re-keyed by it.
            let key = crate::store::WordKey::for_token(&token);
            assert_eq!(
                (key.term.as_str(), key.reading.as_str()),
                ("だ", "だ"),
                "{text}: the displayed headword must not become the identity"
            );
        }

        // The copula after it is required, and keeps everything of its own.
        let after = find("ここって禁足地なんじゃなかったっけ", "じゃなかったっけ");
        let props = &after.conjugation.as_ref().expect("morphology").props;
        assert!(props.iter().any(|prop| prop.form == "Past (~ta)"));
        assert!(props.iter().any(|prop| prop.neg), "still negative");
        assert_eq!(
            after
                .components
                .iter()
                .map(|part| part.surface.as_str())
                .collect::<Vec<_>>(),
            vec!["じゃなかった", "っけ"]
        );

        // Every control: a piece of the construction on its own is left on its own.
        for (text, absent) in [
            ("なんか変だ。", "the pronoun なん"),
            ("何を食べる。", "the pronoun 何"),
            ("静かな部屋だ。", "な with no ん"),
            ("毎日食べるんだ。", "ん with no な"),
            ("それはそうなの。", "の uncontracted"),
        ] {
            assert!(
                !tokens(text).iter().any(crate::model::is_explanatory_nan),
                "{text}: {absent} is not this construction"
            );
        }

        // A gap between the pieces is a different utterance, however it is spelled.
        assert!(
            !tokens("学生な んだ")
                .iter()
                .any(crate::model::is_explanatory_nan),
            "whitespace between them blocks the group"
        );
    }

    /// **A する compound never closes over a space, and every token still spells its own source.**
    ///
    /// A `名詞-普通名詞-サ変可能` noun followed by `する` matched the compound rule, and
    /// `compose_component` concatenated the two surfaces — while the span it gave the result ran
    /// from the noun's start to the verb's end, taking the space in between. The token then read
    /// `徘徊する` where `text[start..end]` spelled `徘徊 する`, which is the grounding invariant
    /// `analyzer_contract.rs` asserts and which `study::analyse` now refuses outright: the reader
    /// got an error for the whole passage instead of a reading of it. The capture that showed
    /// this is line 32 of the notebook traced in `reported_examples.rs`; it is named by line
    /// number here and quoted only in that test's ignored artifact.
    ///
    /// The space is not hypothetical punctuation: a reader's capture has one between a サ変可能
    /// noun and its する. **How it got there does not matter and is not claimed here** — whether
    /// the source held it, a recogniser inserted it, or a transcriber typed it, a token still may
    /// not spell something its own span does not. The capture stays in the ignored diagnostic
    /// artifact; 勉強 is this module's own documented サ変可能 example and carries the same shape.
    ///
    /// Both halves are checked, because refusing the join is only right if the pieces survive:
    /// a rule that dropped the space by dropping `する` would pass a coverage test and lose a verb.
    #[test]
    fn a_suru_compound_does_not_close_over_a_space() {
        let analyzer = analyzer();
        let text = "毎日勉強 するの";
        let analysis = analyzer.analyze(text).expect("analyzes");

        for token in &analysis.tokens {
            let (Some(start), Some(end)) = (token.start, token.end) else {
                panic!("every token is grounded: {:?}", token.surface);
            };
            assert_eq!(
                &text[start..end],
                token.surface,
                "token {:?} does not spell its own source",
                token.surface
            );
        }

        let surfaces: Vec<&str> = analysis
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect();
        assert!(
            surfaces.contains(&"勉強"),
            "the noun survives the refused join: {surfaces:?}"
        );
        assert!(
            surfaces.iter().any(|surface| surface.starts_with("する")),
            "and so does the verb: {surfaces:?}"
        );

        // The control: with no space between them, the compound still forms.
        let joined = analyzer.analyze("毎日勉強するの").expect("analyzes");
        assert!(
            joined
                .tokens
                .iter()
                .any(|token| token.dict_form == "勉強する"),
            "an adjacent noun and する must still compose: {:?}",
            joined
                .tokens
                .iter()
                .map(|token| (&token.surface, &token.dict_form))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn lexical_roles_survive_only_on_tokens_unidic_establishes() {
        let analyzer = analyzer();
        let analysis = analyzer
            .analyze("なんであなたたちに子供っぽくない話をする。")
            .expect("analyzes");

        let nan = analysis
            .tokens
            .iter()
            .find(|token| token.surface == "なん")
            .expect("なん is a token");
        assert_eq!(nan.lexical_role, Some(LexicalRole::Pronoun));

        let suffix = analysis
            .tokens
            .iter()
            .find(|token| token.surface == "たち")
            .expect("the plural suffix is a token");
        assert_eq!(suffix.lexical_role, Some(LexicalRole::Suffix));

        let composed = analysis
            .tokens
            .iter()
            .find(|token| token.surface == "っぽくない")
            .expect("the suffix adjective is composed as an inflecting token");
        assert_eq!(
            composed.lexical_role, None,
            "a composed lexical unit must not inherit its head piece's suffix role"
        );
    }

    /// っけ joins the copula it follows, and only the copula.
    ///
    /// **The four positives are four different UniDic shapes**, which is why the rule keys on the
    /// lemma: だ is 終止形, だった arrives already folded from だっ + た, でした is a different
    /// auxiliary entirely, and じゃなかった is the negative past. A surface test would have caught
    /// だっけ alone.
    ///
    /// **Tense and negation are asserted, not just the boundary.** Joining through [`compound`]
    /// would have produced the right surface with no conjugation at all, and だったっけ against
    /// だっけ is exactly the distinction a reader opens the panel for.
    ///
    /// The controls are the reason this is narrow: 行ったっけ and 食べたっけ are verbs taking the
    /// same particle, and a gap of any kind is a different utterance.
    #[test]
    fn the_terminal_kke_joins_a_copula_and_leaves_verbs_alone() {
        let analyzer = analyzer();
        let tokens =
            |text: &str| -> Vec<Token> { analyzer.analyze(text).expect("analyzes").tokens };
        let find = |text: &str, surface: &str| -> Token {
            tokens(text)
                .into_iter()
                .find(|token| token.surface == surface)
                .unwrap_or_else(|| panic!("{surface} is a token of {text}"))
        };

        for (text, surface, lemma, form, negated, parts) in [
            (
                "何時だっけ。",
                "だっけ",
                "だ",
                "Non-past",
                false,
                vec!["だ", "っけ"],
            ),
            (
                "昨日は雨だったっけ。",
                "だったっけ",
                "だ",
                "Past (~ta)",
                false,
                vec!["だった", "っけ"],
            ),
            (
                "彼は学生でしたっけ。",
                "でしたっけ",
                "です",
                "Past (~ta)",
                false,
                vec!["でした", "っけ"],
            ),
            (
                "紅茶は好きじゃなかったっけ。",
                "じゃなかったっけ",
                "だ",
                "Past (~ta)",
                true,
                vec!["じゃなかった", "っけ"],
            ),
        ] {
            let token = find(text, surface);
            assert_eq!(
                token.dict_form, lemma,
                "{text}: the copula's lemma survives"
            );
            let conjugation = token
                .conjugation
                .as_ref()
                .unwrap_or_else(|| panic!("{text}: {surface} must keep its conjugation"));
            let props = &conjugation.props;
            assert!(
                props.iter().any(|prop| prop.form == form),
                "{text}: expected {form}, got {:?}",
                props.iter().map(|prop| &prop.form).collect::<Vec<_>>()
            );
            assert_eq!(
                props.iter().any(|prop| prop.neg),
                negated,
                "{text}: negation must survive the join"
            );
            assert_eq!(
                token
                    .components
                    .iter()
                    .map(|part| part.surface.as_str())
                    .collect::<Vec<_>>(),
                parts,
                "{text}: the pieces stay reachable"
            );
            // The span has to still describe the source, or the reader highlights the wrong text.
            let (start, end) = (token.start.expect("start"), token.end.expect("end"));
            assert_eq!(
                &text[start..end],
                surface,
                "{text}: the span is the surface"
            );
        }

        // A verb takes っけ too, and must not be swallowed by a rule written for the copula.
        for text in ["もう行ったっけ。", "もう食べたっけ。"] {
            let surfaces: Vec<String> = tokens(text)
                .iter()
                .map(|token| token.surface.clone())
                .collect();
            assert!(
                surfaces.iter().any(|surface| surface == "っけ"),
                "{text}: っけ after a verb stays its own token, got {surfaces:?}"
            );
        }

        // A sentence boundary between them is not one word, however adjacent they look.
        let split = tokens("彼は学生だ。っけ");
        assert!(
            split.iter().any(|token| token.surface == "だ")
                && split.iter().any(|token| token.surface == "っけ"),
            "punctuation between them must block the join"
        );

        // And a copula with nothing after it is left exactly as it was.
        let plain = find("それはそうだ。", "だ");
        assert_eq!(plain.dict_form, "だ");
        assert!(plain.components.is_empty(), "nothing was joined to it");
    }

    #[test]
    fn the_written_copula_is_one_word_and_keeps_its_derivation() {
        let analyzer = analyzer();
        let token_of = |text: &str, surface: &str| -> Token {
            let analysis = analyzer.analyze(text).expect("analyzes");
            match analysis.tokens.iter().find(|t| t.surface == surface) {
                Some(token) => token.clone(),
                None => {
                    let got: Vec<&str> =
                        analysis.tokens.iter().map(|t| t.surface.as_str()).collect();
                    panic!("{surface} is not a token of {text}: {got:?}")
                }
            }
        };

        let plain = token_of("それは事実である。", "である");
        assert_eq!(
            plain.dict_form, "である",
            "ある is the existence verb, not this"
        );

        // The inflected forms are the reason the derivation has to survive the join.
        let past = token_of("それは事実であった。", "であった");
        let conjugation = past.conjugation.as_ref().expect("であった is conjugated");
        assert_eq!(conjugation.lemma, "である");
        assert!(
            conjugation
                .props
                .iter()
                .any(|prop| prop.form.starts_with("Past")),
            "であった is a past copula, got {:?}",
            conjugation.props
        );
        assert!(
            conjugation
                .props
                .iter()
                .all(|prop| !prop.is_base || prop.base_reading == "である"),
            "the derivation must build from である, not from ある: {:?}",
            conjugation.props
        );

        let continuative = token_of("重要であり必要だ。", "であり");
        assert_eq!(
            continuative
                .conjugation
                .as_ref()
                .expect("であり is conjugated")
                .lemma,
            "である"
        );

        // Controls: neither of these is the copula, and both would break if the 助動詞 gate went.
        let existence = analyzer.analyze("本がある。").expect("analyzes");
        let surfaces: Vec<&str> = existence
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect();
        assert!(
            surfaces.contains(&"ある"),
            "the existence verb stands alone, got {surfaces:?}"
        );

        let te_aru = analyzer.analyze("書いてある。").expect("analyzes");
        let surfaces: Vec<&str> = te_aru
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect();
        assert!(
            !surfaces.iter().any(|token| token.contains("である")),
            "the で of 書いて is a 接続助詞, not the copula, got {surfaces:?}"
        );
    }

    /// The eight words Ichiran kept whole and this analyzer cut in half, in the app's own register.
    ///
    /// **Every case here comes from `segmentation_gold.rs`, not from GSD.** News gold's list is
    /// である and ので; hand-segmented manga and game captures produced these instead, and a reader
    /// of this app meets the second list rather than the first. That is the whole argument for
    /// annotating a little of the domain by hand.
    ///
    /// Three mechanisms, and the controls below each say which one is being checked.
    #[test]
    fn the_words_the_sidecar_kept_whole_are_kept_whole_here() {
        let analyzer = analyzer();
        let surfaces = |text: &str| -> Vec<String> {
            analyzer
                .analyze(text)
                .expect("analyzes")
                .tokens
                .iter()
                .map(|token| token.surface.clone())
                .collect()
        };

        for (text, whole) in [
            // Fixed expressions: four strings, no new code. どうして is
            // deliberately absent — see the note on FIXED_EXPRESSIONS for why.
            ("そういえば忘れてた。", "そういえば"),
            ("次第に失われる。", "次第に"),
            ("非常に美しい。", "非常に"),
            ("だが今は違う。", "だが"),
            // A repeated mark is one gesture.
            ("光は——消えた。", "——"),
            ("そう……ですか。", "……"),
            // The copula folding what follows it. だった was once a hand-written special case
            // and these two were its broken siblings — でし | た and じゃ | ない, with た called
            // Non-past because it had no head. じゃない is the most frequent disagreement in this
            // app's own register, and it was left split while だった, seen once, was fixed.
            ("清らかだった。", "だった"),
            ("そうでした。", "でした"),
            ("学生じゃない。", "じゃない"),
        ] {
            let got = surfaces(text);
            assert!(
                got.iter().any(|token| token == whole),
                "{text} should contain {whole}, got {got:?}"
            );
        }

        // **The controls.** A run of *different* marks is two marks doing two jobs, and welding
        // them is what an "any two adjacent symbols" rule would do. だ in its terminal form does
        // not take た, so a following clause's past must not be swallowed.
        let quoted = surfaces("「そうだ」と言った。");
        assert!(
            !quoted
                .iter()
                .any(|token| token.chars().count() > 1
                    && token.chars().all(|c| "「」。、".contains(c))),
            "different marks must not be joined, got {quoted:?}"
        );
        let terminal = surfaces("これは本だと言った。");
        assert!(
            terminal.iter().any(|token| token == "だ"),
            "a terminal だ stands alone, got {terminal:?}"
        );

        // **The forms that must not become heads.** 終止形 だ ends a clause and 連体形 な precedes
        // a noun; if either folded what came after it, the copula would swallow the next clause's
        // auxiliaries. The terminal case is above; this is the attributive one.
        let attributive = surfaces("静かな場所だった。");
        assert!(
            attributive.iter().any(|token| token == "な"),
            "an attributive な stands alone before its noun, got {attributive:?}"
        );

        // The derivation has to survive, not just the surface — that is what the deleted
        // special case got wrong about its siblings.
        let analysis = analyzer.analyze("学生じゃない。").expect("analyzes");
        let negated = analysis
            .tokens
            .iter()
            .find(|token| token.surface == "じゃない")
            .expect("じゃない is one token");
        let chain = negated
            .conjugation
            .as_ref()
            .expect("じゃない is conjugated");
        assert_eq!(chain.lemma, "だ", "the copula is the lemma");
        assert!(
            chain.props.iter().any(|prop| prop.neg),
            "じゃない is a negation, got {:?}",
            chain.props
        );
    }

    /// The compound particles gold keeps whole arrive whole, prefixes included.
    ///
    /// **These are the cases that make longest-match matter.** による and に対し are both in the
    /// list and both prefix によると and に対して, so a first-match rule joins the shorter one and
    /// strands the tail. This is what proves the matcher picks the longer, and it no longer
    /// depends on anything about how the list happens to be ordered.
    #[test]
    fn compound_particles_survive_their_own_prefixes() {
        let analyzer = analyzer();
        let surfaces = |text: &str| -> Vec<String> {
            analyzer
                .analyze(text)
                .expect("analyzes")
                .tokens
                .iter()
                .map(|token| token.surface.clone())
                .collect()
        };

        for (text, whole) in [
            ("新聞によると事故があった。", "によると"),
            ("彼に対して失礼だ。", "に対して"),
            ("会議において決まった。", "において"),
            ("日本における問題。", "における"),
            ("それに関する資料。", "に関する"),
            ("健康のために走る。", "ために"),
        ] {
            let got = surfaces(text);
            assert!(
                got.iter().any(|token| token == whole),
                "{text} should contain {whole}, got {got:?}"
            );
        }
    }

    /// Guards the override in [`modern_reading`], and guards it from both sides.
    ///
    /// The reading must change, and the *lemma* reading with it — they come from the same UniDic
    /// column, so an override applied to one and not the other would put わたし over the word and
    /// わたくし in the panel beside it.
    ///
    /// It also pins the shape of the fix: 学生 is here to fail if the override is ever widened
    /// into something that rewrites readings generally. One word, from one measured disagreement.
    #[test]
    fn the_pronoun_watashi_is_offered_before_the_formal_reading() {
        let analysis = analyzer().analyze("私は学生です。").expect("analyzes");
        let watashi = analysis
            .tokens
            .iter()
            .find(|token| token.surface == "私")
            .expect("私 is a token");
        assert_eq!(watashi.reading, "わたし");
        assert_eq!(watashi.lemma_reading, "わたし");

        let gakusei = analysis
            .tokens
            .iter()
            .find(|token| token.surface == "学生")
            .expect("学生 is a token");
        assert_eq!(gakusei.reading, "がくせい");
    }

    #[test]
    fn unidic_form_labels_are_translated_into_the_model_vocabulary() {
        use crate::grammar::Form;

        // The one that matters most: without it no provisional rule can ever fire.
        assert_eq!(
            Form::classify(&normalize_form("仮定形-一般", "")),
            Form::Provisional
        );
        assert_eq!(
            Form::classify(&normalize_form("終止形-一般", "")),
            Form::NonPast
        );
        assert_eq!(
            Form::classify(&normalize_form("連体形-一般", "")),
            Form::NonPast
        );
        assert_eq!(
            Form::classify(&normalize_form("命令形", "")),
            Form::Imperative
        );
        assert_eq!(
            Form::classify(&normalize_form("連用形-一般", "")),
            Form::Continuative
        );

        // **The suffix is the point.** UniDic names the sound change that produced the form —
        // 連用形-促音便 for 伝わっ, 連用形-撥音便 for 遊ん — and a match on the whole string
        // would leave every one of them unclassified. The stem is what carries the grammar.
        assert_eq!(
            Form::classify(&normalize_form("連用形-促音便", "")),
            Form::Continuative
        );
        assert_eq!(
            Form::classify(&normalize_form("終止形-撥音便", "")),
            Form::NonPast
        );

        // Unknown labels pass through rather than being forced into a wrong bucket.
        assert_eq!(normalize_form("珍しい活用", ""), "珍しい活用");
    }

    /// The conjugation classes the formation engine shares with Ichiran.
    ///
    /// UniDic writes with `-` what IPADIC wrote with `・`, names the irregulars in full, and
    /// splits 一段 in two — so every arm of this mapping had to be rewritten, and a silent
    /// miss would pass the class through as a code nothing downstream recognises.
    #[test]
    fn unidic_conjugation_classes_become_the_shared_codes() {
        assert_eq!(normalize_pos("下一段-バ行", "食べる"), "v1");
        assert_eq!(normalize_pos("上一段-ミ行", "見る"), "v1");
        assert_eq!(normalize_pos("カ行変格", "来る"), "vk");
        assert_eq!(normalize_pos("サ行変格", "する"), "vs-i");
        assert_eq!(normalize_pos("形容詞", "高い"), "adj-i");
        assert_eq!(normalize_pos("五段-ラ行", "伝わる"), "v5r");
        assert_eq!(normalize_pos("五段-ワア行", "買う"), "v5u");

        // 行く is the one word UniDic does not distinguish and the formation engine must:
        // its て-form is 行って, not 行いて, and only `v5k-s` says so.
        assert_eq!(normalize_pos("五段-カ行", "行く"), "v5k-s");
        assert_eq!(normalize_pos("五段-カ行", "書く"), "v5k");
    }

    #[test]
    fn a_provisional_verb_is_reported_as_one() {
        let analysis = analyzer().analyze("行けばいい").expect("analyze");
        let provisional = analysis.tokens.iter().any(|token| {
            token
                .conjugation
                .iter()
                .flat_map(|c| c.props.iter())
                .any(|p| {
                    crate::grammar::Form::classify(&p.form) == crate::grammar::Form::Provisional
                })
        });
        assert!(
            provisional,
            "行け is a provisional form and must be reported as one"
        );
    }

    #[test]
    fn it_tokenizes_without_any_setup_at_all() {
        // The whole point: no sidecar, no database, no download.
        let analysis = analyzer().analyze("水を飲む").expect("analyze");
        let surfaces: Vec<&str> = analysis
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect();
        assert_eq!(surfaces, ["水", "を", "飲む"]);
        assert_eq!(analysis.text, "水を飲む");
    }

    #[test]
    fn tokens_arrive_grounded_in_the_source() {
        let analysis = analyzer()
            .analyze("水を食べさせられていた")
            .expect("analyze");
        for token in &analysis.tokens {
            let (start, end) = (token.start.expect("start"), token.end.expect("end"));
            assert_eq!(&analysis.text[start..end], token.surface);
            for component in &token.components {
                let (start, end) = (
                    component.start.expect("component start"),
                    component.end.expect("component end"),
                );
                assert_eq!(&analysis.text[start..end], component.surface);
            }
        }
    }

    #[test]
    fn attached_tense_is_one_word_with_a_usable_lemma_reading() {
        let analysis = analyzer().analyze("食べた").expect("analyze");
        let verb = &analysis.tokens[0];
        assert_eq!(analysis.tokens.len(), 1);
        assert_eq!(verb.surface, "食べた");
        assert_eq!(verb.dict_form, "食べる", "原形 becomes the dictionary form");
        assert_eq!(verb.lemma_reading, "たべる");
        assert!(verb.components.is_empty());

        let steps = crate::grammar::ladder(verb);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].form, crate::grammar::Form::Past);
        assert_eq!(steps[0].pos, "v1");
        assert_eq!(
            steps[0]
                .formation
                .as_ref()
                .map(|formed| formed.result.as_str()),
            Some("たべた")
        );
    }

    #[test]
    fn negative_and_formal_tenses_are_folded_into_the_head() {
        let cases = [
            ("行かなかった", crate::grammar::Form::Past, true, false),
            ("食べます", crate::grammar::Form::NonPast, false, true),
            ("食べませんでした", crate::grammar::Form::Past, true, true),
            ("高くない", crate::grammar::Form::NonPast, true, false),
            // **ましょう carries its own form and ます must not flatten it.** This read
            // NonPast until 助動詞-マス began taking 意志推量形 from the auxiliary.
            (
                "行きましょう",
                crate::grammar::Form::Volitional,
                false,
                true,
            ),
            // The two controls that matter: both are ます followed by something that names the
            // form itself, and an over-broad reading of the auxiliary would break them.
            ("行きました", crate::grammar::Form::Past, false, true),
            ("行きません", crate::grammar::Form::NonPast, true, true),
        ];
        for (text, form, neg, fml) in cases {
            let analysis = analyzer().analyze(text).expect("analyze");
            let word = &analysis.tokens[0];
            assert_eq!(word.surface, text, "{text} is one composed inflecting word");
            let step = crate::grammar::ladder(word)
                .into_iter()
                .next()
                .expect("one step");
            assert_eq!((step.form, step.neg, step.fml), (form, neg, fml), "{text}");
        }
    }

    #[test]
    fn causative_passive_and_te_form_become_a_real_derivation_chain() {
        let analysis = analyzer().analyze("食べさせられて").expect("analyze");
        assert_eq!(analysis.tokens.len(), 1);
        let token = &analysis.tokens[0];
        assert_eq!(token.surface, "食べさせられて");
        assert_eq!(token.dict_form, "食べる");
        assert_eq!(token.lemma_reading, "たべる");

        let steps = crate::grammar::ladder(token);
        assert_eq!(
            steps.iter().map(|step| step.form).collect::<Vec<_>>(),
            [
                crate::grammar::Form::CausativePassive,
                crate::grammar::Form::Conjunctive,
            ]
        );
        assert_eq!(
            steps
                .iter()
                .filter_map(|step| step.formation.as_ref())
                .map(|formed| formed.result.as_str())
                .collect::<Vec<_>>(),
            ["たべさせられる", "たべさせられて"]
        );
    }

    #[test]
    fn plain_rareru_is_composed_without_claiming_an_ambiguous_voice() {
        let analysis = analyzer().analyze("食べられる").expect("analyze");
        assert_eq!(analysis.tokens.len(), 1);
        assert_eq!(analysis.tokens[0].surface, "食べられる");
        let steps = crate::grammar::ladder(&analysis.tokens[0]);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].form, crate::grammar::Form::Unclassified);
    }

    #[test]
    fn a_te_linked_helper_is_an_ichiran_shaped_compound() {
        let analysis = analyzer().analyze("伝わってきます").expect("analyze");
        assert_eq!(analysis.tokens.len(), 1);
        let compound = &analysis.tokens[0];
        assert_eq!(compound.surface, "伝わってきます");
        assert_eq!(compound.dict_form, "伝わる");
        assert_eq!(
            compound
                .components
                .iter()
                .map(|part| (part.surface.as_str(), part.dict_form.as_str()))
                .collect::<Vec<_>>(),
            [("伝わって", "伝わる"), ("きます", "くる")]
        );
        let auxiliary = crate::grammar::breakdown(compound).auxiliaries;
        assert_eq!(auxiliary.len(), 1);
        assert_eq!(auxiliary[0].pattern_id, "te-kuru");
    }

    #[test]
    fn a_suru_noun_keeps_its_dictionary_headword_through_a_compound() {
        let analysis = analyzer().analyze("勉強しています").expect("analyze");
        assert_eq!(analysis.tokens.len(), 1);
        let compound = &analysis.tokens[0];
        assert_eq!(compound.dict_form, "勉強する");
        assert_eq!(compound.components[0].surface, "勉強して");
        assert_eq!(compound.components[0].dict_form, "勉強する");
        assert_eq!(compound.components[0].lemma_reading, "べんきょうする");
    }

    #[test]
    fn katakana_readings_become_furigana_but_kana_words_keep_their_spelling() {
        let analysis = analyzer().analyze("水").expect("analyze");
        // タベル-style katakana turned into the hiragana a reader wants above kanji.
        assert_eq!(analysis.tokens[0].reading, "みず");

        // コーヒー is already written as it is read; こーひー would be wrong.
        let katakana = analyzer().analyze("コーヒー").expect("analyze");
        assert_eq!(katakana.tokens[0].reading, "コーヒー");

        // Segmentation may split ひらがな into several tokens, so this asserts the rule rather
        // than a particular segmentation: a token that is already kana reads as itself.
        let hiragana = analyzer().analyze("ひらがな").expect("analyze");
        for token in &hiragana.tokens {
            assert_eq!(
                token.reading, token.surface,
                "a kana token should read as it is written"
            );
        }
    }

    #[test]
    fn senses_are_left_to_the_dictionaries() {
        // UniDic has no glosses, and pretending otherwise would be worse than silence:
        // the merge policy promotes a real dictionary when the analyzer has nothing.
        let analysis = analyzer().analyze("水").expect("analyze");
        assert!(analysis.tokens[0].senses.is_empty());
    }

    #[test]
    fn empty_input_is_refused_like_any_other_analyzer() {
        assert_eq!(analyzer().analyze("   "), Err(AnalyzeError::EmptyInput));
        assert_eq!(analyzer().engine(), "lindera");
    }

    #[test]
    fn it_can_be_held_as_a_trait_object_like_the_sidecar_one() {
        let analyzer: Box<dyn Analyzer> = Box::new(analyzer());
        assert!(analyzer.analyze("先生").is_ok());
    }
}

#[cfg(test)]
mod irregular_reading_tests {
    use super::*;
    use crate::analyzer::Analyzer;

    /// **来る's stem changes sound without changing spelling, and reconstruction cannot see it.**
    ///
    /// 来 is written the same in 来る, 来て and 来ない while being read く, き and こ. Aligning
    /// the written forms — which is right for 伝わっ against 伝わる — takes くる, drops る and
    /// offers くて for a word that is きて. Caught on a real capture, 付いて来て.
    #[test]
    fn the_irregular_verbs_keep_the_reading_unidic_recorded() {
        let analyzer = LinderaAnalyzer::new().expect("the dictionary is compiled in");

        for (text, wrong) in [("付いて来て", "くて"), ("持って来た", "くた")] {
            let analysis = analyzer.analyze(text).expect("analyse");
            let reading: String = analysis
                .tokens
                .iter()
                .map(|token| token.reading.as_str())
                .collect();
            assert!(
                !reading.contains(wrong),
                "{text} came back as {reading:?}, which contains the reconstructed {wrong:?}"
            );
            assert!(
                reading.contains("き"),
                "{text} should be read with き: got {reading:?}"
            );
        }
    }
}
