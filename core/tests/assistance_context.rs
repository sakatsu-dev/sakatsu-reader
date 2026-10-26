//! **One request, one advisor, one memory — and the reader sees nothing different.**
//!
//! The embedded parser advises the Ichiran path four ways (`analyzer::assist`), and the dictionary
//! stages ask overlapping questions (`dict::evidence`). Both now go through one request-owned
//! context. These are the controls that the context changes how often a question is asked and
//! never what a reader is shown:
//!
//! - a repeated question reaches the parser once, and the four kinds never mix;
//! - every mode serves identical output, traced or not, on a cache hit or miss, and on a fallback;
//! - only answers are remembered: a failure is counted, never kept, and the next attempt runs —
//!   including an eager pass that failed before a fallback that then succeeds;
//! - the callers' own retention keeps its scope: one conversion, one repair pass;
//! - a passage read inside the context nests its joined-line questions without a borrow panic or a
//!   lock wait;
//! - a reused answer is the same parser's, never a fault double's;
//! - another dictionary generation never answers from the cache;
//! - the advisor cannot start an Ichiran conversion.
//!
//! Hermetic: Lindera is compiled in, Ichiran is the committed fixtures, and the dictionary is built
//! here — the term bank `analysis_path_identity.rs` uses, so the stages after the analyzer act.

mod common;

use std::cell::{Cell, RefCell};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sakatsu_core::analyzer::assist::{
    Advice, Advisor, Assistance, AssistanceMode, ProviderId, Stage,
};
use sakatsu_core::analyzer::ichiran::{self, raw, TiePolicy};
use sakatsu_core::analyzer::lindera::{self, LinderaAnalyzer};
use sakatsu_core::analyzer::{AnalyzeError, Analyzer, IchiranAnalyzer, Remembering, Traced};
use sakatsu_core::dict::evidence::{Evidence, EvidenceCache};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::{Analysis, AnalyzerPolicy};
use sakatsu_core::study::{self, AnalysisContext, AnalysisOptions, Plan, Unanswered};
use serde_json::json;

const MODES: [AssistanceMode; 3] = [
    AssistanceMode::Legacy,
    AssistanceMode::SharedLazy,
    AssistanceMode::SharedEager,
];

/// Every committed fixture, with the text the capture script says it was captured from.
fn fixtures() -> Vec<(String, String)> {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/capture-ichiran-fixtures.sh");
    let text = std::fs::read_to_string(&script).expect("the capture script");
    let found: Vec<(String, String)> = text
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("capture ")?;
            let (name, quoted) = rest.split_once(char::is_whitespace)?;
            let body = quoted.trim().strip_prefix('\'')?.split('\'').next()?;
            Some((name.to_string(), body.to_string()))
        })
        .collect();
    assert!(found.len() >= 20, "the capture script lists the fixtures");
    found
}

fn segments_of(name: &str) -> Vec<raw::Segment> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ichiran")
        .join(format!("{name}.json"));
    let json = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
    raw::parse(&json).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn text_of(name: &str) -> String {
    fixtures()
        .into_iter()
        .find(|(fixture, _)| fixture == name)
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("no fixture {name}"))
}

/// Ichiran served from the committed fixtures, converted **inside the request** — the adapter's
/// own `analyze_in`, minus the sidecar.
struct FixtureIchiran;

impl FixtureIchiran {
    fn segments(text: &str) -> Result<Vec<raw::Segment>, AnalyzeError> {
        let name = fixtures()
            .into_iter()
            .find(|(_, source)| source == text)
            .map(|(name, _)| name)
            .ok_or_else(|| AnalyzeError::EngineUnavailable(format!("no fixture for {text}")))?;
        Ok(segments_of(&name))
    }
}

impl Analyzer for FixtureIchiran {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.analyze_traced(text).map(|traced| traced.analysis)
    }

    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        self.analyze_in(text, &Assistance::new(AssistanceMode::Legacy))
    }

    fn analyze_in(&self, text: &str, assistance: &Assistance<'_>) -> Result<Traced, AnalyzeError> {
        let segments = Self::segments(text)?;
        Ok(Traced {
            analysis: ichiran::to_analysis_in(text, &segments, TiePolicy::SHIPPED, assistance)?,
            raw: None,
        })
    }

    fn engine(&self) -> &str {
        "ichiran"
    }
}

/// The same fixtures through `ichiran::to_analysis`, the wrapper every caller outside a request
/// uses: a fresh `Legacy` context per conversion.
///
/// **Not the pre-change implementation.** It is today's conversion behind today's wrapper, so the
/// comparison below is between modes of the current code. Agreement with the code as it was before
/// shared assistance is established separately, against a baseline that code recorded itself
/// (`reader_equivalence.rs`, `common/equivalence.rs`).
struct LegacyWrapper;

impl Analyzer for LegacyWrapper {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        Ok(ichiran::to_analysis(text, &FixtureIchiran::segments(text)?))
    }

    fn engine(&self) -> &str {
        "ichiran"
    }
}

/// The shared parser as an advisor that counts what it is asked, and fails the questions a
/// script names: `failing(text)` returns how many more times that text should fail.
struct Scripted {
    asked: RefCell<Vec<String>>,
    fail: RefCell<Vec<(String, usize)>>,
    /// Fail every question while set.
    down: Cell<bool>,
}

impl Scripted {
    fn new() -> Scripted {
        Scripted {
            asked: RefCell::new(Vec::new()),
            fail: RefCell::new(Vec::new()),
            down: Cell::new(false),
        }
    }

    /// Fail `text` the next `times` it is asked.
    fn failing(self, text: &str, times: usize) -> Scripted {
        self.fail.borrow_mut().push((text.to_string(), times));
        self
    }

    fn asked(&self, text: &str) -> usize {
        self.asked
            .borrow()
            .iter()
            .filter(|seen| *seen == text)
            .count()
    }

    fn total(&self) -> usize {
        self.asked.borrow().len()
    }
}

fn parser() -> &'static LinderaAnalyzer {
    lindera::shared().expect("the embedded parser is compiled in")
}

impl Advisor for Scripted {
    /// The shared parser's identity, because it answers with the shared parser.
    fn identity(&self) -> ProviderId {
        LinderaAnalyzer::IDENTITY
    }

    fn read(&self, text: &str, assistance: &Assistance<'_>) -> Result<Analysis, AnalyzeError> {
        self.asked.borrow_mut().push(text.to_string());
        if self.down.get() {
            return Err(AnalyzeError::EngineUnavailable("scripted: down".into()));
        }
        let failing = {
            let mut fail = self.fail.borrow_mut();
            match fail
                .iter_mut()
                .find(|(seen, left)| seen == text && *left > 0)
            {
                Some((_, left)) => {
                    *left -= 1;
                    true
                }
                None => false,
            }
        };
        if failing {
            return Err(AnalyzeError::EngineUnavailable("scripted failure".into()));
        }
        parser().read(text, assistance)
    }
}

/// A primary that fails operationally, every time.
struct Down;

impl Analyzer for Down {
    fn analyze(&self, _: &str) -> Result<Analysis, AnalyzeError> {
        Err(AnalyzeError::EngineUnavailable(
            "scripted: the sidecar is down".into(),
        ))
    }

    fn engine(&self) -> &str {
        "ichiran"
    }
}

/// Lindera behind a wrapper that serves nothing from a request — how every fault double in the
/// shell is shaped. It must never be handed a remembered passage.
struct Opaque<'a>(&'a dyn Analyzer, AtomicUsize);

impl Analyzer for Opaque<'_> {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.1.fetch_add(1, Ordering::Relaxed);
        self.0.analyze(text)
    }

    fn engine(&self) -> &str {
        "lindera"
    }
}

fn term_bank(extra: &[serde_json::Value]) -> serde_json::Value {
    let mut terms = vec![
        json!(["気持ち", "きもち", "n", "", 40, ["feeling"], 1, ""]),
        json!([
            "伝わる",
            "つたわる",
            "v5r",
            "v5",
            30,
            ["to be conveyed"],
            2,
            ""
        ]),
        json!(["水", "みず", "n", "", 40, ["water"], 3, ""]),
        json!(["飲む", "のむ", "v5m", "v5", 40, ["to drink"], 4, ""]),
        json!(["先生", "せんせい", "n", "", 40, ["teacher"], 5, ""]),
        json!([
            "情けない",
            "なさけない",
            "adj-i",
            "adj-i",
            30,
            ["shameful"],
            6,
            ""
        ]),
        json!(["晴々", "はればれ", "adv", "", 20, ["cheerful"], 7, ""]),
        json!(["晴々", "せいせい", "adv", "", 20, ["refreshed"], 8, ""]),
        json!(["気分", "きぶん", "n", "", 40, ["mood"], 9, ""]),
        json!(["学生", "がくせい", "n", "", 40, ["student"], 10, ""]),
        json!(["削除", "さくじょ", "n vs", "", 30, ["deletion"], 11, ""]),
        json!([
            "間違う",
            "まちがう",
            "v5u",
            "v5",
            30,
            ["to be mistaken"],
            12,
            ""
        ]),
        json!(["怒る", "おこる", "v5r", "v5", 30, ["to get angry"], 13, ""]),
    ];
    terms.extend(extra.iter().cloned());
    serde_json::Value::Array(terms)
}

fn archive(dir: &Path, name: &str, terms: serde_json::Value) -> PathBuf {
    let path = dir.join(format!("{name}.zip"));
    let file = std::fs::File::create(&path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    for (member, contents) in [
        (
            "index.json",
            json!({ "title": "Assistance Fixture", "format": 3 }).to_string(),
        ),
        ("term_bank_1.json", terms.to_string()),
    ] {
        writer
            .start_file(member, zip::write::SimpleFileOptions::default())
            .expect("member");
        writer.write_all(contents.as_bytes()).expect("write");
    }
    writer.finish().expect("finish");
    path
}

fn open(dir: &Path) -> Vec<Dictionary> {
    let archive = archive(dir, "assistance", term_bank(&[]));
    vec![Dictionary::import(&archive, &dir.join("out")).expect("import the fixture dictionary")]
}

fn plan<'a>(primary: &'a dyn Analyzer, fallback: Option<&'a dyn Analyzer>) -> Plan<'a> {
    Plan {
        requested: AnalyzerPolicy::Automatic,
        primary,
        fallback,
        standing_in: None,
    }
}

// ---------------------------------------------------------------------------------------------
// Memoisation

#[test]
fn a_repeated_question_reaches_the_parser_once_and_the_four_kinds_never_mix() {
    let scripted = Scripted::new();
    let assistance = Assistance::with_provider(AssistanceMode::SharedLazy, Some(&scripted));
    let first = assistance.ask(Stage::Word, "晴々");
    let again = assistance.ask(Stage::Word, "晴々");
    assert_eq!(
        scripted.total(),
        1,
        "the second question is answered from memory"
    );
    assert_eq!(first.answered(), again.answered());
    let word = *assistance.account().stage(Stage::Word);
    assert_eq!((word.requested, word.executed, word.remembered), (2, 1, 1));

    // One string, four kinds of question: each is its own.
    for stage in [Stage::Passage, Stage::Sentence, Stage::JoinedLines] {
        assert!(assistance.ask(stage, "晴々").answered().is_some());
    }
    assert_eq!(
        scripted.total(),
        4,
        "a stage never answers another's question"
    );

    // The legacy mode remembers nothing and still counts.
    let scripted = Scripted::new();
    let legacy = Assistance::with_provider(AssistanceMode::Legacy, Some(&scripted));
    legacy.ask(Stage::Word, "晴々");
    legacy.ask(Stage::Word, "晴々");
    assert_eq!(scripted.total(), 2);
    let word = *legacy.account().stage(Stage::Word);
    assert_eq!((word.requested, word.executed, word.remembered), (2, 2, 0));
}

#[test]
fn a_failure_is_counted_and_never_remembered() {
    let scripted = Scripted::new().failing("晴々", 1);
    let assistance = Assistance::with_provider(AssistanceMode::SharedLazy, Some(&scripted));
    assert!(matches!(
        assistance.ask(Stage::Word, "晴々"),
        Advice::Failed(_)
    ));
    assert!(
        assistance.ask(Stage::Word, "晴々").answered().is_some(),
        "the next attempt runs, and succeeds"
    );
    assert_eq!(scripted.asked("晴々"), 2);
    let word = *assistance.account().stage(Stage::Word);
    assert_eq!((word.executed, word.failed, word.remembered), (2, 1, 0));

    // No provider at all is counted as such.
    let none = Assistance::with_provider(AssistanceMode::SharedLazy, None);
    assert!(matches!(none.ask(Stage::Word, "晴々"), Advice::Unavailable));
    assert_eq!(none.account().stage(Stage::Word).unavailable, 1);
}

// ---------------------------------------------------------------------------------------------
// Equivalence

#[test]
fn every_mode_serves_what_the_legacy_wrapper_serves() {
    let scratch = common::Scratch::new("assistance-context", "modes");
    let dictionaries = open(&scratch);
    let lindera = LinderaAnalyzer::new().expect("compiled in");
    let mut advised = 0;
    let mut reads_saved = 0;

    for (name, text) in fixtures() {
        for (analyzer, wrapper) in [
            (
                &FixtureIchiran as &dyn Analyzer,
                &LegacyWrapper as &dyn Analyzer,
            ),
            (&lindera, &lindera),
        ] {
            let label = format!("{name} under {}", analyzer.engine());
            // The legacy wrapper's answer, through the same run.
            let reference = study::run(&text, wrapper, &dictionaries, AnalysisOptions::PRODUCTION)
                .unwrap_or_else(|failure| panic!("{label}: {:?}", failure.error));
            let mut legacy: Option<(usize, usize)> = None;
            for mode in MODES {
                for options in [
                    AnalysisOptions::PRODUCTION,
                    AnalysisOptions::PRODUCTION.traced(),
                    AnalysisOptions::WITHOUT_GROUPING,
                ] {
                    let options = options.with_assistance(mode);
                    let run = study::run(&text, analyzer, &dictionaries, options)
                        .unwrap_or_else(|failure| panic!("{label} {mode:?}: {:?}", failure.error));
                    if options.grouping {
                        assert_eq!(
                            run.analysis, reference.analysis,
                            "{label}: {mode:?} (trace {}) changed the reader's output",
                            options.trace
                        );
                        assert_eq!(run.repairs.notes(), reference.repairs.notes(), "{label}");
                    }
                    let asked = run.assistance.advice.executed();
                    let read = run.assistance.evidence.executed();
                    if options == AnalysisOptions::PRODUCTION.with_assistance(mode) {
                        match (mode, legacy) {
                            (AssistanceMode::Legacy, _) => legacy = Some((asked, read)),
                            (_, Some((legacy_asked, legacy_read))) => {
                                // The eager pass may ask one question nobody needed; nothing
                                // else may cost more than it did.
                                let eager = usize::from(mode == AssistanceMode::SharedEager);
                                assert!(
                                    asked <= legacy_asked + eager,
                                    "{label}: {mode:?} asked the parser {asked} times, legacy {legacy_asked}"
                                );
                                assert!(
                                    read <= legacy_read,
                                    "{label}: {mode:?} read the dictionary {read} times, legacy {legacy_read}"
                                );
                                reads_saved += legacy_read - read;
                            }
                            (_, None) => unreachable!("legacy runs first"),
                        }
                        advised += asked;
                    }
                }
            }
        }
    }
    // **Teeth.** Equivalence over passages nobody advised would hold whatever the memo did.
    assert!(advised > 0, "the fixtures put questions to the parser");
    assert!(
        reads_saved > 0,
        "and the shared modes read the dictionary less"
    );
    drop(dictionaries);
}

#[test]
fn a_cache_hit_and_a_miss_serve_the_same_and_a_hit_asks_nothing() {
    let text = text_of("harebare");
    let remembering = Remembering::new(FixtureIchiran);
    let options = AnalysisOptions::PRODUCTION.with_assistance(AssistanceMode::SharedLazy);
    let miss = study::run(&text, &remembering, &[], options).expect("a miss");
    let hit = study::run(&text, &remembering, &[], options).expect("a hit");
    assert_eq!(miss.analysis, hit.analysis);
    assert!(
        miss.assistance.advice.executed() > 0,
        "the tie is advised on a miss"
    );
    assert_eq!(
        hit.assistance.advice.executed(),
        0,
        "a remembered analysis asks no advice"
    );
    let wrapped =
        study::run(&text, &LegacyWrapper, &[], AnalysisOptions::PRODUCTION).expect("wrapped");
    assert_eq!(hit.analysis, wrapped.analysis);
}

// ---------------------------------------------------------------------------------------------
// Failure, then success

#[test]
fn an_eager_pass_that_failed_does_not_stop_the_fallback_reading_for_itself() {
    let text = text_of("harebare");
    let fallback = LinderaAnalyzer::new().expect("compiled in");
    let options = AnalysisOptions::PRODUCTION.with_assistance(AssistanceMode::SharedEager);

    // What the legacy path serves when Ichiran is down.
    let legacy = study::analyse_resilient(
        &text,
        &plan(&Down, Some(&fallback)),
        &[],
        AnalysisOptions::PRODUCTION.with_assistance(AssistanceMode::Legacy),
        &|| true,
    )
    .expect("the fallback answers");

    // The eager pass fails; the primary fails; the fallback must read for itself, and succeed.
    let scripted = Scripted::new().failing(&text, 1);
    let context = AnalysisContext::with_provider(AssistanceMode::SharedEager, Some(&scripted));
    let done = study::analyse_resilient_in(
        &text,
        &plan(&Down, Some(&fallback)),
        &[],
        options,
        &|| true,
        &context,
    )
    .expect("the fallback answers after an eager failure");
    assert_eq!(done.analysis, legacy.analysis);
    let passage = *done.assistance.advice.stage(Stage::Passage);
    assert_eq!(passage.failed, 1, "the eager pass failed");
    assert_eq!(passage.remembered, 0, "nothing was kept from it");
    assert_eq!(passage.executed, 2, "the fallback read the passage itself");

    // And where the eager pass succeeded, the fallback is served what it read.
    let scripted = Scripted::new();
    let context = AnalysisContext::with_provider(AssistanceMode::SharedEager, Some(&scripted));
    let reused = study::analyse_resilient_in(
        &text,
        &plan(&Down, Some(&fallback)),
        &[],
        options,
        &|| true,
        &context,
    )
    .expect("answered");
    assert_eq!(reused.analysis, legacy.analysis);
    let passage = *reused.assistance.advice.stage(Stage::Passage);
    assert_eq!((passage.executed, passage.remembered), (1, 1));
}

#[test]
fn a_reused_passage_is_the_same_parser_s_and_never_a_fault_double_s() {
    let text = text_of("harebare");
    let lindera = LinderaAnalyzer::new().expect("compiled in");
    let opaque = Opaque(&lindera, AtomicUsize::new(0));
    let scripted = Scripted::new();
    let context = AnalysisContext::with_provider(AssistanceMode::SharedEager, Some(&scripted));
    let options = AnalysisOptions::PRODUCTION.with_assistance(AssistanceMode::SharedEager);
    let done = study::analyse_resilient_in(
        &text,
        &plan(&Down, Some(&opaque)),
        &[],
        options,
        &|| true,
        &context,
    )
    .expect("answered");
    assert_eq!(
        opaque.1.load(Ordering::Relaxed),
        1,
        "the double ran its own analysis"
    );
    assert_eq!(
        done.assistance.advice.stage(Stage::Passage).remembered,
        0,
        "and was handed nothing"
    );
    assert_eq!(
        done.analysis.tokens,
        study::run(&text, &lindera, &[], AnalysisOptions::PRODUCTION)
            .expect("lindera")
            .analysis
            .tokens
    );

    // An advisor of another identity is never served the parser's passage either.
    struct Elsewhere;
    impl Advisor for Elsewhere {
        fn identity(&self) -> ProviderId {
            ProviderId("another parser")
        }
        fn read(&self, text: &str, assistance: &Assistance<'_>) -> Result<Analysis, AnalyzeError> {
            parser().read(text, assistance)
        }
    }
    let eager = Assistance::with_provider(AssistanceMode::SharedEager, Some(&Elsewhere));
    eager.prefetch(&text);
    let _ = lindera.analyze_in(&text, &eager);
    let passage = *eager.account().stage(Stage::Passage);
    assert_eq!(
        (passage.executed, passage.remembered),
        (2, 0),
        "an answer under another identity is not the built-in's"
    );
}

#[test]
fn a_failed_tie_does_not_silence_the_next_conversion_and_retention_keeps_its_scope() {
    // 寒気 and コート both tie in this fixture, so one conversion asks for the passage once and
    // for two words.
    let name = "kanki";
    let text = text_of(name);
    let segments = segments_of(name);
    let reference = ichiran::to_analysis(&text, &segments);

    // The passage fails every time it is asked within the first conversion.
    let scripted = Scripted::new().failing(&text, 1);
    let assistance = Assistance::with_provider(AssistanceMode::SharedLazy, Some(&scripted));
    let first = ichiran::to_analysis_in(&text, &segments, TiePolicy::SHIPPED, &assistance)
        .expect("converted");
    assert_eq!(
        scripted.asked(&text),
        1,
        "one conversion settles a failed passage once, for both of its ties"
    );
    let words = assistance.account().stage(Stage::Word).requested;
    assert!(words >= 2, "both ties asked for the bare word ({words})");

    // A second conversion in the same request asks again — the failure was not kept — and now
    // gets the reader's answer.
    let second = ichiran::to_analysis_in(&text, &segments, TiePolicy::SHIPPED, &assistance)
        .expect("converted");
    assert_eq!(scripted.asked(&text), 2);
    assert_eq!(second, reference);
    let _ = first;

    // A bare word that failed on one conversion is asked again on the next.
    let scripted = Scripted::new().failing("寒気", 1);
    let assistance = Assistance::with_provider(AssistanceMode::SharedLazy, Some(&scripted));
    let _ = ichiran::to_analysis_in(&text, &segments, TiePolicy::BareWord, &assistance);
    let again = ichiran::to_analysis_in(&text, &segments, TiePolicy::BareWord, &assistance)
        .expect("converted");
    assert_eq!(scripted.asked("寒気"), 2, "the failed word was asked again");
    assert_eq!(
        again,
        ichiran::to_analysis_under(&text, &segments, TiePolicy::BareWord)
    );
}

#[test]
fn a_repair_sentence_that_failed_stays_failed_for_one_pass_only() {
    let name = "machigattoru";
    let text = text_of(name);
    let scratch = common::Scratch::new("assistance-context", "repair-retention");
    let dictionaries = open(&scratch);
    let converted = ichiran::to_analysis(&text, &segments_of(name));
    let mut displayed = converted.clone();
    sakatsu_core::pipeline::Pipeline::new(&dictionaries).display(&mut displayed);

    let scripted = Scripted::new().failing(&text, 1);
    let assistance = Assistance::with_provider(AssistanceMode::SharedLazy, Some(&scripted));
    let (_, failed) = sakatsu_core::lexical::repair::repaired_in(
        &displayed,
        Evidence::direct(&dictionaries),
        &assistance,
    );
    assert!(failed.notes().is_empty(), "no advice, no repair");
    let asked_once = scripted.asked(&text);
    assert_eq!(asked_once, 1, "one pass asks a failed sentence once");

    let (_, second) = sakatsu_core::lexical::repair::repaired_in(
        &displayed,
        Evidence::direct(&dictionaries),
        &assistance,
    );
    assert_eq!(scripted.asked(&text), 2, "the next pass asks again");
    assert_eq!(second.notes().len(), 1, "and repairs");
    drop(dictionaries);
}

#[test]
fn a_dictionary_fault_then_a_success_is_a_success() {
    let scratch = common::Scratch::new("assistance-context", "dictionary-fault");
    let dictionaries = open(&scratch);
    let terms = dictionaries[0].root().join("terms.ndjson");
    let away = dictionaries[0].root().join("terms.ndjson.away");
    let cache = EvidenceCache::new(true);
    let source = Evidence::through(&dictionaries, &cache)
        .sources()
        .next()
        .expect("one dictionary");

    std::fs::rename(&terms, &away).expect("take the entries away");
    let failed = source.lookup("水");
    std::fs::rename(&away, &terms).expect("put them back");
    assert!(failed.is_err(), "an unreadable entry file is a failure");

    let found = source.lookup("水").expect("the next read succeeds");
    assert_eq!(found[0].entry.reading, "みず");
    let again = source.lookup("水").expect("remembered");
    assert_eq!(found, again);
    let lookup = cache.account().lookup;
    assert_eq!(
        (
            lookup.requested,
            lookup.executed,
            lookup.failed,
            lookup.remembered
        ),
        (3, 2, 1, 1)
    );
    drop(dictionaries);
}

#[test]
fn another_generation_never_answers_from_the_cache() {
    let scratch = common::Scratch::new("assistance-context", "generations");
    let first = archive(&scratch, "first", term_bank(&[]));
    let second = archive(
        &scratch,
        "second",
        serde_json::Value::Array(vec![json!([
            "水",
            "すい",
            "n",
            "",
            40,
            ["water (Wed.)"],
            3,
            ""
        ])]),
    );
    let out = scratch.join("out");
    let old = Dictionary::import(&first, &out).expect("first generation");
    let new = Dictionary::import(&second, &out).expect("second generation");
    assert_ne!(old.root(), new.root(), "a re-import is another generation");
    assert_eq!(old.installed_as(), new.installed_as());

    let cache = EvidenceCache::new(true);
    let olds = [old];
    let news = [new];
    let read = |dictionaries: &[Dictionary]| {
        Evidence::through(dictionaries, &cache)
            .sources()
            .next()
            .expect("one")
            .lookup("水")
            .expect("reads")[0]
            .entry
            .reading
            .clone()
    };
    assert_eq!(read(&olds), "みず");
    assert_eq!(read(&news), "すい", "the new generation's own answer");
    assert_eq!(read(&olds), "みず");
    assert_eq!(cache.account().lookup.remembered, 1);

    // Each operation is its own key: the reading index is never answered by a lookup.
    let source = Evidence::through(&news, &cache)
        .sources()
        .next()
        .expect("one");
    let by_reading = source.lookup_reading("すい").expect("reads");
    assert_eq!(by_reading.len(), 1);
    assert!(source.lookup_reading("みず").expect("reads").is_empty());
    drop((olds, news));
}

// ---------------------------------------------------------------------------------------------
// Structure

#[test]
fn a_passage_read_inside_the_request_nests_its_line_joins_without_waiting() {
    // `lines.rs`'s own wrap case: a break neither cheap signal settles, so the join is asked.
    let text = "６人宿泊させ\nて欲しい";
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let lindera = LinderaAnalyzer::new().expect("compiled in");
        let assistance = Assistance::new(AssistanceMode::SharedEager);
        assistance.prefetch(text);
        let served = lindera
            .analyze_in(text, &assistance)
            .map(|traced| traced.analysis);
        let advised = assistance.ask(Stage::Passage, text);
        let _ = send.send((
            served,
            advised.answered().cloned(),
            lindera.analyze(text),
            assistance.account(),
        ));
    });
    let (served, advised, plain, account) = receive
        .recv_timeout(Duration::from_secs(120))
        .expect("no borrow panic and no lock wait");
    let plain = plain.expect("analyzes");
    assert_eq!(served.expect("served"), plain);
    assert_eq!(advised.expect("advised"), plain);
    let joins = *account.stage(Stage::JoinedLines);
    assert!(joins.executed >= 1, "the join was asked inside the passage");
    assert_eq!(account.stage(Stage::Passage).executed, 1);
    assert!(account.stage(Stage::Passage).remembered >= 2);
}

#[test]
fn the_advisor_cannot_start_an_ichiran_conversion() {
    struct Recursing;
    impl Advisor for Recursing {
        fn identity(&self) -> ProviderId {
            ProviderId("recursing")
        }
        fn read(&self, text: &str, assistance: &Assistance<'_>) -> Result<Analysis, AnalyzeError> {
            // The adapter refuses before it spawns anything: this program does not exist, and the
            // error must not say so.
            let sidecar =
                IchiranAnalyzer::with_command("no-such-ichiran-sidecar", Vec::<String>::new());
            let refused = sidecar
                .analyze_in(text, assistance)
                .expect_err("refused from inside a question");
            assert!(refused.to_string().contains("refused"), "{refused}");
            ichiran::to_analysis_in(
                text,
                &segments_of("harebare"),
                TiePolicy::SHIPPED,
                assistance,
            )
        }
    }
    let text = text_of("harebare");
    let assistance = Assistance::with_provider(AssistanceMode::SharedLazy, Some(&Recursing));
    assert!(matches!(
        assistance.ask(Stage::Passage, &text),
        Advice::Failed(AnalyzeError::EngineUnavailable(_))
    ));
    assert_eq!(assistance.account().refused, 2);
    assert!(
        !assistance.advising(),
        "the guard is released after the question"
    );
    // Outside a question the same conversion runs.
    assert!(ichiran::to_analysis_in(
        &text,
        &segments_of("harebare"),
        TiePolicy::SHIPPED,
        &assistance
    )
    .is_ok());
}

#[test]
fn a_cancelled_request_publishes_nothing_and_the_next_starts_empty() {
    let text = text_of("harebare");
    let fallback = Opaque(parser(), AtomicUsize::new(0));
    let context = AnalysisContext::new(AssistanceMode::SharedLazy);
    let outcome = study::analyse_resilient_in(
        &text,
        &plan(&Down, Some(&fallback)),
        &[],
        AnalysisOptions::PRODUCTION.with_assistance(AssistanceMode::SharedLazy),
        &|| false,
        &context,
    );
    assert!(matches!(outcome, Err(Unanswered::Cancelled { .. })));
    assert_eq!(
        fallback.1.load(Ordering::Relaxed),
        0,
        "no retry was started"
    );

    let next = AnalysisContext::new(AssistanceMode::SharedLazy);
    let account = next.account();
    assert_eq!(account.advice.executed(), 0);
    assert_eq!(account.evidence.requested(), 0);
}
