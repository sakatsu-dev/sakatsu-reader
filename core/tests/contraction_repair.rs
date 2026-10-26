//! **Contracted verb forms Ichiran split, rejoined only where the dictionary accounts for the whole
//! span** (`lexical::repair`).
//!
//! Two halves. The positive cases and the untouched controls run the reader's own path —
//! `study::run` over committed `ichiran-cli` output, with the compiled-in advisor — so what is
//! pinned is what a reader is served. The abstentions that need a particular piece of advice are
//! driven with a scripted advisor against the stage directly, so each check can be shown to be the
//! one that refused.
//!
//! The dictionary is hermetic and small. Its records are stated facts about real words (間違う is
//! a godan u-verb read まちがう; 取る and 撮る are both read とる), not transcriptions of any
//! dictionary's text.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};

use sakatsu_core::analyzer::ichiran::{self, raw};
use sakatsu_core::analyzer::{
    validate_components, validate_selectable, AnalyzeError, Analyzer, LinderaAnalyzer, Traced,
};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::lexical::repair::{self, Abstain, Outcome};
use sakatsu_core::model::{
    Analysis, AnalyzerPolicy, Conjugation, ContractionFamily, Prop, StemEvidence, Token,
};
use sakatsu_core::study::{self, AnalysisOptions, Plan, Run};
use serde_json::json;

/// The committed fixtures this test reads, with the text each was captured from.
const FIXTURES: &[(&str, &str)] = &[
    ("machigattoru", "頭文字から間違っとるわい！！"),
    ("okonnakya", "怒んなきゃ"),
    ("yattoru", "こ、こら！ナニやっとるかぁぁ！！"),
    ("yomitoranakya", "本当の所を読みとらなきゃ。"),
];

struct FixtureIchiran;

impl Analyzer for FixtureIchiran {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.analyze_traced(text).map(|traced| traced.analysis)
    }

    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        let (name, _) = FIXTURES
            .iter()
            .find(|(_, source)| *source == text)
            .ok_or_else(|| AnalyzeError::EngineUnavailable(format!("no fixture for {text}")))?;
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ichiran")
            .join(format!("{name}.json"));
        let json = std::fs::read_to_string(&path).expect("fixture");
        let segments = raw::parse(&json).expect("fixture parses");
        Ok(Traced {
            analysis: ichiran::to_analysis(text, &segments),
            raw: Some(json),
        })
    }

    fn engine(&self) -> &str {
        "ichiran"
    }
}

/// Advice given in advance: the tokens it returns, positioned within whatever sentence it is asked
/// about. Only for the abstentions, where the point is what a *particular* piece of advice does.
struct Scripted(Vec<Token>);

impl Analyzer for Scripted {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        Ok(Analysis {
            text: text.to_string(),
            tokens: self.0.clone(),
            execution: None,
        })
    }

    fn engine(&self) -> &str {
        "scripted"
    }
}

fn dictionary(dir: &Path) -> PathBuf {
    archive(
        dir,
        "contractions.zip",
        json!([
            [
                "間違う",
                "まちがう",
                "v5u",
                "v5",
                30,
                ["to be mistaken"],
                1,
                ""
            ],
            // One word, two readings: the advisor's picks, and the caveat is drawn.
            ["怒る", "おこる", "v5r", "v5", 30, ["to get angry"], 2, ""],
            ["怒る", "いかる", "v5r", "v5", 10, ["to get angry"], 2, ""],
            // A homophone the reading query reaches; it cannot spell a kanji source.
            ["起こる", "おこる", "v5r", "v5", 30, ["to occur"], 3, ""],
            ["取る", "とる", "v5r", "v5", 40, ["to take"], 4, ""],
            ["撮る", "とる", "v5r", "v5", 30, ["to photograph"], 5, ""],
            ["なきゃ", "なきゃ", "exp", "", 10, ["must"], 6, ""],
            ["読み取る", "よみとる", "v5r", "v5", 20, ["to read"], 7, ""],
            ["為る", "する", "vs-i", "vs", 40, ["to do"], 8, ""],
            ["回る", "まわる", "v5r", "v5", 30, ["to turn"], 9, ""],
            // The auxiliary a ～とる part contracts, for the part lookup.
            ["居る", "おる", "v5r", "v5", 20, ["to be (animate)"], 10, ""]
        ]),
    )
}

fn archive(dir: &Path, name: &str, terms: serde_json::Value) -> PathBuf {
    let path = dir.join(name);
    let file = std::fs::File::create(&path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let members = [
        (
            "index.json",
            json!({ "title": "Contraction Fixture", "format": 3 }).to_string(),
        ),
        ("term_bank_1.json", terms.to_string()),
    ];
    for (name, contents) in members {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .expect("member");
        writer.write_all(contents.as_bytes()).expect("write");
    }
    writer.finish().expect("finish");
    path
}

fn open(dir: &Path) -> Vec<Dictionary> {
    let archive = dictionary(dir);
    vec![Dictionary::import(&archive, &dir.join("out")).expect("import the fixture dictionary")]
}

fn run(text: &str, dictionaries: &[Dictionary], options: AnalysisOptions) -> Run {
    study::run(text, &FixtureIchiran, dictionaries, options)
        .unwrap_or_else(|failure| panic!("{text}: {:?}", failure.error))
}

fn surfaces(analysis: &Analysis) -> Vec<&str> {
    analysis
        .tokens
        .iter()
        .map(|token| token.surface.as_str())
        .collect()
}

/// Everything outside the repaired span is exactly what the engine gave.
fn unchanged_outside(repaired: &Analysis, baseline: &Analysis, start: usize, end: usize) {
    let outside = |analysis: &Analysis| -> Vec<Token> {
        analysis
            .tokens
            .iter()
            .filter(|token| {
                token.end.is_none_or(|at| at <= start) || token.start.is_some_and(|at| at >= end)
            })
            .cloned()
            .collect()
    };
    assert_eq!(
        outside(repaired),
        outside(baseline),
        "only the repaired span changes"
    );
}

#[test]
fn a_contracted_te_oru_is_rejoined_with_source_slices_for_parts() {
    let scratch = common::Scratch::new("contraction-repair", "te-oru");
    let dictionaries = open(&scratch);
    let (_, text) = FIXTURES[0];

    let baseline = run(
        text,
        &dictionaries,
        AnalysisOptions::WITHOUT_GROUPING.without_repairs(),
    );
    assert!(
        surfaces(&baseline.analysis)
            .windows(2)
            .any(|pair| pair == ["間", "違っとる"]),
        "the fixture still shows the split this repairs: {:?}",
        surfaces(&baseline.analysis)
    );

    let done = run(text, &dictionaries, AnalysisOptions::WITHOUT_GROUPING);
    let token = done
        .analysis
        .tokens
        .iter()
        .find(|token| token.surface == "間違っとる")
        .expect("one token over the contracted form");
    assert_eq!(token.dict_form, "間違う");
    assert_eq!(token.lemma_reading, "まちがう");
    assert_eq!(token.reading, "まちがっとる");
    assert!(!token.reading_ambiguous, "間違う has one reading");
    let conjugation = token.conjugation.as_ref().expect("the form is described");
    assert_eq!(conjugation.props[0].form, "Conjunctive (~te)");

    // The parts are what the source spells, each selectable, and none names おる.
    let parts: Vec<(&str, bool)> = token
        .components
        .iter()
        .map(|part| (part.surface.as_str(), part.selectable))
        .collect();
    assert_eq!(parts, [("間違っ", true), ("とる", true)]);
    assert_eq!(
        token.components[1].dict_form, "おる",
        "the tail looks up the auxiliary it contracts, never 取る"
    );
    assert!(validate_components(&done.analysis).is_empty());
    assert!(validate_selectable(&done.analysis).is_empty());

    let notes = done.repairs.notes();
    assert_eq!(notes.len(), 1, "{:?}", done.repairs);
    let note = &notes[0];
    assert_eq!(note.family, ContractionFamily::TeOru);
    assert_eq!(note.stem, StemEvidence::Headword);
    assert_eq!((note.lemma.as_str(), note.sequence), ("間違う", Some(1)));
    assert_eq!(note.replaced, 2);
    assert_eq!(&text[note.start..note.end], "間違っとる");
    unchanged_outside(&done.analysis, &baseline.analysis, note.start, note.end);
}

#[test]
fn a_nasalised_negative_conditional_is_rejoined_and_its_reading_caveated() {
    let scratch = common::Scratch::new("contraction-repair", "nasal");
    let dictionaries = open(&scratch);
    let (_, text) = FIXTURES[1];

    let baseline = run(
        text,
        &dictionaries,
        AnalysisOptions::WITHOUT_GROUPING.without_repairs(),
    );
    assert_eq!(surfaces(&baseline.analysis), ["怒", "ん", "なきゃ"]);

    let done = run(text, &dictionaries, AnalysisOptions::WITHOUT_GROUPING);
    assert_eq!(surfaces(&done.analysis), ["怒んなきゃ"]);
    let token = &done.analysis.tokens[0];
    assert_eq!(token.dict_form, "怒る");
    assert_eq!(
        token.reading, "おこんなきゃ",
        "the advisor's reading of the word's two"
    );
    assert!(
        token.reading_ambiguous,
        "怒る is also いかる, and the reader is told a reading was chosen"
    );
    let prop = &token.conjugation.as_ref().expect("described").props[0];
    assert_eq!((prop.form.as_str(), prop.neg), ("Provisional (~eba)", true));
    let parts: Vec<&str> = token
        .components
        .iter()
        .map(|part| part.surface.as_str())
        .collect();
    assert_eq!(parts, ["怒ん", "なきゃ"]);
    assert!(token.components.iter().all(|part| part.selectable));

    let notes = done.repairs.notes();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].nasal);
    assert_eq!(notes[0].replaced, 3);
}

/// No trigger, or one Ichiran answered itself: nothing changes and nothing is investigated.
#[test]
fn passages_with_nothing_to_repair_are_served_exactly_as_before() {
    let scratch = common::Scratch::new("contraction-repair", "controls");
    let dictionaries = open(&scratch);
    for (name, text) in &FIXTURES[2..] {
        let with = run(text, &dictionaries, AnalysisOptions::PRODUCTION);
        let without = run(
            text,
            &dictionaries,
            AnalysisOptions::PRODUCTION.without_repairs(),
        );
        assert_eq!(with.analysis, without.analysis, "{name}");
        assert!(
            with.repairs.investigations.is_empty(),
            "{name}: {:?}",
            with.repairs
        );
    }
    let answered = run(FIXTURES[3].1, &dictionaries, AnalysisOptions::PRODUCTION);
    assert_eq!(
        answered.repairs.answered, 1,
        "読みとらなきゃ is Ichiran's own answer"
    );
}

/// The stage is Ichiran's alone: the built-in analyzer composes contractions itself.
#[test]
fn the_built_in_analyzer_is_never_repaired() {
    let scratch = common::Scratch::new("contraction-repair", "lindera");
    let dictionaries = open(&scratch);
    let lindera = LinderaAnalyzer::new().expect("compiled in");
    let done = study::run(
        FIXTURES[0].1,
        &lindera,
        &dictionaries,
        AnalysisOptions::PRODUCTION,
    )
    .unwrap_or_else(|failure| panic!("{:?}", failure.error));
    assert_eq!(done.repairs, repair::Account::default());
}

/// The note travels on the execution record, and the passage still says Ichiran read it.
#[test]
fn the_repair_is_recorded_on_the_execution_and_is_not_a_fallback() {
    let scratch = common::Scratch::new("contraction-repair", "execution");
    let dictionaries = open(&scratch);
    let plan = Plan {
        requested: AnalyzerPolicy::Automatic,
        primary: &FixtureIchiran,
        fallback: None,
        standing_in: None,
    };
    let done = study::analyse_resilient(
        FIXTURES[0].1,
        &plan,
        &dictionaries,
        AnalysisOptions::PRODUCTION,
        &|| true,
    )
    .expect("analyses");
    let execution = done.analysis.execution.expect("recorded");
    assert_eq!(execution.engine, "ichiran");
    assert!(execution.fallback.is_none());
    assert_eq!(execution.repairs, done.repairs.notes());
    assert_eq!(execution.repairs.len(), 1);
}

// ---------------------------------------------------------------------------
// Abstentions, each driven by the advice that should cause it.

fn token(text: &str, surface: &str) -> Token {
    let start = text.find(surface).expect("surface in text");
    Token {
        surface: surface.into(),
        start: Some(start),
        end: Some(start + surface.len()),
        ..Default::default()
    }
}

fn advice(text: &str, dict_form: &str, lemma_reading: &str, reading: &str, pos: &str) -> Token {
    Token {
        surface: text.into(),
        dict_form: dict_form.into(),
        lemma_reading: lemma_reading.into(),
        reading: reading.into(),
        conjugation: (!pos.is_empty()).then(|| Conjugation {
            lemma: dict_form.into(),
            reading: lemma_reading.into(),
            props: vec![Prop {
                pos: pos.into(),
                form: "Non-past".into(),
                ..Default::default()
            }],
        }),
        start: Some(0),
        end: Some(text.len()),
        ..Default::default()
    }
}

fn outcome(
    text: &str,
    pieces: &[&str],
    advisor: &dyn Analyzer,
    dictionaries: &[Dictionary],
) -> Outcome {
    let analysis = Analysis {
        text: text.into(),
        tokens: pieces.iter().map(|piece| token(text, piece)).collect(),
        execution: None,
    };
    let (out, account) = repair::repaired(&analysis, dictionaries, Some(advisor));
    let [investigation] = account.investigations.as_slice() else {
        panic!("one investigation: {account:?}");
    };
    if matches!(investigation.outcome, Outcome::Abstained(_)) {
        assert_eq!(out, analysis, "an abstention changes nothing");
    }
    investigation.outcome.clone()
}

/// **Line 83's shape.** The advisor composes して回っとる under する; that lemma cannot derive the
/// span, and neither can it stand in for the 回る inside it.
#[test]
fn a_larger_compound_lemma_does_not_stand_in_for_the_verb_inside_it() {
    let scratch = common::Scratch::new("contraction-repair", "line-83");
    let dictionaries = open(&scratch);
    let text = "して回っとる";
    let pieces = ["して", "回", "っとる"];

    // As the compiled-in advisor actually reads it: one token, lemma する, no conjugation.
    let lindera = LinderaAnalyzer::new().expect("compiled in");
    assert!(matches!(
        outcome(text, &pieces, &lindera, &dictionaries),
        Outcome::Abstained(_)
    ));
    // And with a conjugation claimed for it: する still derives nothing.
    let claimed = Scripted(vec![advice(text, "為る", "する", "してまわっとる", "vs-i")]);
    assert_eq!(
        outcome(text, &pieces, &claimed, &dictionaries),
        Outcome::Abstained(Abstain::NoDerivation)
    );
}

/// 取る and 撮る are both とる, and both derive とっとる. Neither is chosen.
#[test]
fn two_words_that_both_derive_the_span_abstain() {
    let scratch = common::Scratch::new("contraction-repair", "competition");
    let dictionaries = open(&scratch);
    let text = "とっとる";
    let advisor = Scripted(vec![advice(text, "取る", "とる", "とっとる", "v5r")]);
    assert_eq!(
        outcome(text, &["とっ", "とる"], &advisor, &dictionaries),
        Outcome::Abstained(Abstain::CompetingWords(2))
    );
}

#[test]
fn advice_that_crosses_punctuation_abstains() {
    let scratch = common::Scratch::new("contraction-repair", "barrier");
    let dictionaries = open(&scratch);
    let text = "間違っ、とる";
    let advisor = Scripted(vec![advice(
        text,
        "間違う",
        "まちがう",
        "まちがっとる",
        "v5u",
    )]);
    assert_eq!(
        outcome(text, &["間違っ", "、", "とる"], &advisor, &dictionaries),
        Outcome::Abstained(Abstain::Barrier)
    );
}

/// The derivation must agree with the advisor's reading, not merely exist.
#[test]
fn a_derivation_the_advisor_reads_differently_abstains() {
    let scratch = common::Scratch::new("contraction-repair", "reading");
    let dictionaries = open(&scratch);
    let text = "怒んなきゃ";
    let advisor = Scripted(vec![advice(text, "怒る", "おこる", "おこらなきゃ", "v5r")]);
    assert_eq!(
        outcome(text, &["怒", "ん", "なきゃ"], &advisor, &dictionaries),
        Outcome::Abstained(Abstain::NoDerivation)
    );
}

/// A verb class the advice does not share is not derived under.
#[test]
fn an_incompatible_class_abstains() {
    let scratch = common::Scratch::new("contraction-repair", "class");
    let dictionaries = open(&scratch);
    let text = "間違っとる";
    let advisor = Scripted(vec![advice(
        text,
        "間違う",
        "まちがう",
        "まちがっとる",
        "v1",
    )]);
    assert_eq!(
        outcome(text, &["間", "違っとる"], &advisor, &dictionaries),
        Outcome::Abstained(Abstain::NoDerivation)
    );
}

/// Advice that starts inside one of the engine's tokens does not name a run of whole tokens.
#[test]
fn advice_that_does_not_start_on_a_token_boundary_abstains() {
    let scratch = common::Scratch::new("contraction-repair", "aligned");
    let dictionaries = open(&scratch);
    let text = "間違っとる";
    let mut inner = advice("違っとる", "違う", "ちがう", "ちがっとる", "v5u");
    inner.start = Some("間".len());
    inner.end = Some(text.len());
    assert_eq!(
        outcome(
            text,
            &["間違", "っとる"],
            &Scripted(vec![inner]),
            &dictionaries
        ),
        Outcome::Abstained(Abstain::NotAligned)
    );
}

/// With nothing to consult, nothing is decided — and a dictionary that lacks the word is not a
/// licence either.
#[test]
fn a_word_no_dictionary_has_abstains() {
    let text = "間違っとる";
    let advisor = Scripted(vec![advice(
        text,
        "間違う",
        "まちがう",
        "まちがっとる",
        "v5u",
    )]);
    assert_eq!(
        outcome(text, &["間", "違っとる"], &advisor, &[]),
        Outcome::Abstained(Abstain::NoDerivation)
    );
}

/// The sentence limit is enforced before the advisor is asked.
#[test]
fn an_overlong_sentence_keeps_the_engines_output() {
    let long = "あ".repeat(repair::MAX_SENTENCE_CHARS);
    let text = format!("{long}間違っとる");
    let analysis = Analysis {
        text: text.clone(),
        tokens: vec![
            token(&text, &long),
            token(&text, "間"),
            token(&text, "違っとる"),
        ],
        execution: None,
    };
    let advisor = Scripted(Vec::new());
    let (_, account) = repair::repaired(&analysis, &[], Some(&advisor));
    assert_eq!(
        account.investigations[0].outcome,
        Outcome::Abstained(Abstain::SentenceTooLong)
    );
    assert_eq!(account.advised_sentences, 0);
}

/// A record that is only an arrow is not a word, even when it carries the verb's rules: its
/// "definition" says where to look, not what it means.
#[test]
fn an_unresolved_pointer_is_not_a_word() {
    let scratch = common::Scratch::new("contraction-repair", "pointer");
    let zip = archive(
        &scratch,
        "pointer.zip",
        json!([[
            "間違う",
            "まちがう",
            "v5u",
            "v5",
            30,
            ["⟶ 間違える"],
            99,
            ""
        ]]),
    );
    let dictionaries =
        vec![Dictionary::import(&zip, &scratch.join("out")).expect("import the pointer")];
    let text = "間違っとる";
    let advisor = Scripted(vec![advice(
        text,
        "間違う",
        "まちがう",
        "まちがっとる",
        "v5u",
    )]);
    assert_eq!(
        outcome(text, &["間", "違っとる"], &advisor, &dictionaries),
        Outcome::Abstained(Abstain::NoDerivation)
    );
}

/// **A dictionary that cannot be read is not a dictionary without the word.** Every term shard is
/// corrupted in place; the lookup fails, and the failure is what is recorded.
#[test]
fn a_failed_lookup_abstains_rather_than_reading_as_absent() {
    let scratch = common::Scratch::new("contraction-repair", "unreadable");
    let out = scratch.join("out");
    drop(Dictionary::import(&dictionary(&scratch), &out).expect("import"));
    fn corrupt(dir: &Path, inside: bool) -> usize {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        let mut broken = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                broken += corrupt(&path, inside || name == "spelling" || name == "reading");
            } else if inside {
                std::fs::write(&path, b"not json at all").expect("corrupt the shard");
                broken += 1;
            }
        }
        broken
    }
    assert!(
        corrupt(&out, false) > 0,
        "there were term shards to corrupt"
    );
    let dictionaries = vec![Dictionary::open(&out).expect("reopen")];
    let text = "間違っとる";
    let advisor = Scripted(vec![advice(
        text,
        "間違う",
        "まちがう",
        "まちがっとる",
        "v5u",
    )]);
    let found = outcome(text, &["間", "違っとる"], &advisor, &dictionaries);
    assert!(
        matches!(found, Outcome::Abstained(Abstain::LookupFailed(_))),
        "{found:?}"
    );
}

// ---------------------------------------------------------------------------
// The field of competitors: the reading index is searched, not only the spelling.

fn one_off(scratch: &Path, name: &str, terms: serde_json::Value) -> Vec<Dictionary> {
    let zip = archive(scratch, &format!("{name}.zip"), terms);
    vec![Dictionary::import(&zip, &scratch.join(name)).expect("import")]
}

/// **A kana-spelled verb must not hide a kanji one.** かかる is spelled in kana by one record and
/// 罹る is read the same way; both derive かかっとる. The spelling-first lookup of かかる stops at
/// the kana record, so without the reading index the repair would call it the only word.
#[test]
fn a_kana_spelled_verb_does_not_hide_an_equally_derivable_kanji_entry() {
    let scratch = common::Scratch::new("contraction-repair", "hidden-homophone");
    let dictionaries = one_off(
        &scratch,
        "hidden",
        json!([
            ["かかる", "かかる", "v5r", "v5", 30, ["to hang"], 21, ""],
            ["罹る", "かかる", "v5r", "v5", 10, ["to fall ill"], 22, ""]
        ]),
    );
    // The premise: the reader's lookup really does stop at the kana record.
    let spelled = sakatsu_core::lookup::whole_word(&dictionaries[0], "かかる").expect("lookup");
    assert_eq!(
        spelled.len(),
        1,
        "spelling-first finds only the kana record"
    );

    let text = "かかっとる";
    let advisor = Scripted(vec![advice(text, "かかる", "かかる", "かかっとる", "v5r")]);
    assert_eq!(
        outcome(text, &["かかっ", "とる"], &advisor, &dictionaries),
        Outcome::Abstained(Abstain::CompetingWords(2))
    );
}

/// A spelling hit that is not a verb does not end the search: なる is spelled by the adnominal,
/// and the verb read なる is found through the reading index.
#[test]
fn a_non_verb_spelling_hit_does_not_stop_the_search() {
    let scratch = common::Scratch::new("contraction-repair", "non-verb-spelling");
    let dictionaries = one_off(
        &scratch,
        "adnominal",
        json!([
            ["なる", "なる", "adj-pn", "", 30, ["that is in"], 31, ""],
            ["成る", "なる", "v5r", "v5", 40, ["to become"], 32, ""]
        ]),
    );
    let text = "なっとる";
    let advisor = Scripted(vec![advice(text, "なる", "なる", "なっとる", "v5r")]);
    let Outcome::Repaired(note) = outcome(text, &["な", "っとる"], &advisor, &dictionaries)
    else {
        panic!("the verb behind the adnominal should be found");
    };
    assert_eq!((note.lemma.as_str(), note.sequence), ("成る", Some(32)));
    assert_eq!(
        note.stem,
        StemEvidence::Reading,
        "the source spells the stem in kana"
    );
}

/// Both searches are required: an unreadable reading index is a failed search, not a field with
/// one candidate in it.
#[test]
fn an_unreadable_reading_index_abstains_even_when_the_spelling_route_answers() {
    let scratch = common::Scratch::new("contraction-repair", "unreadable-reading");
    let out = scratch.join("out");
    drop(Dictionary::import(&dictionary(&scratch), &out).expect("import"));
    fn corrupt(dir: &Path, inside: bool) -> usize {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        let mut broken = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                broken += corrupt(&path, inside || name == "reading");
            } else if inside {
                std::fs::write(&path, b"not json at all").expect("corrupt the shard");
                broken += 1;
            }
        }
        broken
    }
    assert!(
        corrupt(&out, false) > 0,
        "there were reading shards to corrupt"
    );
    let dictionaries = vec![Dictionary::open(&out).expect("reopen")];
    // The premise: the spelling route still answers.
    assert!(
        !sakatsu_core::lookup::whole_word(&dictionaries[0], "間違う")
            .expect("the spelling index is intact")
            .is_empty()
    );

    let text = "間違っとる";
    let advisor = Scripted(vec![advice(
        text,
        "間違う",
        "まちがう",
        "まちがっとる",
        "v5u",
    )]);
    let found = outcome(text, &["間", "違っとる"], &advisor, &dictionaries);
    assert!(
        matches!(found, Outcome::Abstained(Abstain::LookupFailed(_))),
        "{found:?}"
    );
}

// ---------------------------------------------------------------------------
// What the reader is shown: the explanation, and what each part looks up.

fn studied(text: &str, dictionaries: &[Dictionary]) -> sakatsu_core::study::StudyView {
    study::study(
        text,
        sakatsu_core::study::Resources {
            analyzer: &FixtureIchiran,
            dictionaries,
            kanji: &sakatsu_core::dict::KanjiDic::default(),
            store: None,
        },
    )
    .unwrap_or_else(|error| panic!("{text}: {error}"))
}

/// One rung as the reader sees it: form id, label, negative flag, and (from, change, result).
type Rung = (String, String, bool, (String, String, String));

/// **The word is explained as one word, base form then contraction** — from the `StudyView` a
/// reader is served, not from the stored conjugation. Neither part is laddered on its own: 間違っ
/// is not a te-form and 怒ん is not a negative conditional.
///
/// The breakdowns are also the fixture the interface renders
/// (`ui/src/qa/contraction-breakdowns.json`), compared here byte for byte so the layout test
/// draws exactly what core produces. `UPDATE_GOLDEN=1` rewrites it.
#[test]
fn the_grammar_view_explains_each_contraction_from_its_base_form() {
    let scratch = common::Scratch::new("contraction-repair", "breakdown");
    let dictionaries = open(&scratch);
    let mut shown = serde_json::Map::new();
    for (text, word, expected) in [
        (
            FIXTURES[0].1,
            "間違っとる",
            [
                (
                    "conjunctive",
                    "Conjunctive (~te)",
                    false,
                    ("まちがう", "う → って", "まちがって"),
                ),
                (
                    "contraction",
                    "Contracted ～ておる (～とる)",
                    false,
                    ("まちがっておる", "ておる → とる", "まちがっとる"),
                ),
            ],
        ),
        (
            FIXTURES[1].1,
            "怒んなきゃ",
            [
                (
                    "provisional",
                    "Provisional (~eba)",
                    true,
                    ("おこる", "る → ら, then add なければ", "おこらなければ"),
                ),
                (
                    "contraction",
                    "Contracted ～なければ (～なきゃ)",
                    false,
                    (
                        "おこらなければ",
                        "なければ → なきゃ, ら → ん",
                        "おこんなきゃ",
                    ),
                ),
            ],
        ),
    ] {
        let view = studied(text, &dictionaries);
        let at = view
            .analysis
            .tokens
            .iter()
            .position(|token| token.surface == word)
            .unwrap_or_else(|| panic!("{word} is one token"));
        let breakdown = &view.breakdowns[at];
        let steps = serde_json::to_value(&breakdown.steps).expect("serialize");
        let got: Vec<Rung> = steps
            .as_array()
            .expect("steps")
            .iter()
            .map(|step| {
                let formation = &step["formation"];
                (
                    step["form"].as_str().unwrap_or("").to_string(),
                    step["label"].as_str().unwrap_or("").to_string(),
                    step["neg"].as_bool().unwrap_or(false),
                    (
                        formation["from"].as_str().unwrap_or("").to_string(),
                        formation["change"].as_str().unwrap_or("").to_string(),
                        formation["result"].as_str().unwrap_or("").to_string(),
                    ),
                )
            })
            .collect();
        let want: Vec<Rung> = expected
            .iter()
            .map(|(form, label, neg, (from, change, result))| {
                (
                    form.to_string(),
                    label.to_string(),
                    *neg,
                    (from.to_string(), change.to_string(), result.to_string()),
                )
            })
            .collect();
        assert_eq!(got, want, "{word}");
        assert!(
            breakdown.steps.iter().all(|step| step.component.is_none()),
            "{word}: the ladder is the word's, not a part's"
        );
        shown.insert(
            word.to_string(),
            serde_json::to_value(breakdown).expect("serialize"),
        );
    }

    let golden =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/qa/contraction-breakdowns.json");
    let generated = serde_json::to_string_pretty(&shown).expect("serialize") + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &generated).expect("write golden");
        return;
    }
    let on_disk = std::fs::read_to_string(&golden)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", golden.display()));
    assert_eq!(
        on_disk.replace("\r\n", "\n"),
        generated,
        "the interface's contraction fixture is stale; rerun with UPDATE_GOLDEN=1"
    );
}

/// **What each part actually looks up**, through the reader's own contextual lookup over the
/// passage as the interface expands it for a part: the whole word replaced by its parts.
#[test]
fn each_part_looks_up_what_it_is() {
    let scratch = common::Scratch::new("contraction-repair", "part-lookup");
    let dictionaries = open(&scratch);
    for (text, word, part, gloss, not) in [
        (
            FIXTURES[0].1,
            "間違っとる",
            "とる",
            "to be (animate)",
            "to take",
        ),
        (
            FIXTURES[0].1,
            "間違っとる",
            "間違っ",
            "to be mistaken",
            "to be (animate)",
        ),
        (
            FIXTURES[1].1,
            "怒んなきゃ",
            "なきゃ",
            "must",
            "to get angry",
        ),
        (FIXTURES[1].1, "怒んなきゃ", "怒ん", "to get angry", "must"),
    ] {
        let view = studied(text, &dictionaries);
        let at = view
            .analysis
            .tokens
            .iter()
            .position(|token| token.surface == word)
            .expect("the repaired word");
        let parts = view.analysis.tokens[at].components.clone();
        let mut expanded = view.analysis.tokens[..at].to_vec();
        let index = at
            + parts
                .iter()
                .position(|candidate| candidate.surface == part)
                .expect("the part");
        expanded.extend(parts);
        expanded.extend_from_slice(&view.analysis.tokens[at + 1..]);

        let found = sakatsu_core::lookup::in_context(&expanded, index, &dictionaries)
            .unwrap_or_else(|error| panic!("{part}: {error:?}"));
        let glosses: Vec<&str> = found
            .word
            .inline
            .iter()
            .flat_map(|sense| sense.glosses.iter().map(String::as_str))
            .collect();
        assert!(glosses.contains(&gloss), "{part} shows {glosses:?}");
        assert!(
            !glosses.contains(&not),
            "{part} must not show {not:?}: {glosses:?}"
        );
    }
}
