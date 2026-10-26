//! **Every entry point to the analysis path is one path.**
//!
//! `study::analyse`, `analyse_with_grouping` and `analyse_without_grouping` each used to repeat
//! the analyzer, the display pipeline and the span check, and the measurement harnesses called two
//! of the three. A stage added to one would reach the reader and not the measurement — or the
//! reverse — and nothing would say so. They are now delegates of `study::run`.
//!
//! This pins that, and pins the half a delegate cannot: that **asking for a trace changes what is
//! kept and never what is computed**. A diagnostic that described a different conversion from the
//! one a reader was served would be a measurement of nothing.
//!
//! Both engines, with nothing installed: Lindera is compiled in, and the Ichiran arm is the
//! committed fixtures through `ichiran::to_analysis` — the adapter's own conversion, tie-break and
//! line marking included. A hermetic dictionary is built so the joins, the reading correction and
//! grouping all have something to act on; with no dictionary every stage after the analyzer would
//! be a no-op and identity would be trivially true.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};

use sakatsu_core::analyzer::ichiran::{self, raw};
use sakatsu_core::analyzer::{AnalyzeError, Analyzer, LinderaAnalyzer, Traced};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::Analysis;
use sakatsu_core::study::{self, AnalysisOptions};
use serde_json::json;

/// The committed fixtures, with the text each was captured from.
const FIXTURES: &[(&str, &str)] = &[
    ("mizu-o-nomu", "水を飲む"),
    ("kimochi", "気持ち"),
    ("iwasete", "言わせて"),
    ("kimochi-tsutawatte", "気持ちが伝わってきます"),
    ("tabesaserareteita", "食べさせられていた"),
    ("sensei-nasakenai", "先生ってほんと…情けないですね"),
    ("kore-o-ki-ni", "これを機に言わせてもらいますが"),
    ("niwatori", "にはとりが鳴く"),
    ("tabenakatta", "食べなかった"),
    ("topic-particle", "私は学生"),
    ("harebare", "晴々とした気分"),
    ("nakaguro", "景観・社会・文化・報道"),
    ("interior-space", "映像もYou Tubeから削除しました"),
    ("hodoketeru", "ネクタイがほどけてるよ。"),
    ("tsuichatta", "寝癖がついちゃった。"),
    // Two the contraction repairs change, so identity is pinned over a repaired passage too.
    ("machigattoru", "頭文字から間違っとるわい！！"),
    ("okonnakya", "怒んなきゃ"),
];

/// Ichiran's conversion, served from a fixture, shaped exactly as the adapter is: `analyze`
/// delegates to `analyze_traced`, which converts once and keeps the raw output beside it.
struct FixtureIchiran;

impl FixtureIchiran {
    fn json(text: &str) -> Result<String, AnalyzeError> {
        let (name, _) = FIXTURES
            .iter()
            .find(|(_, source)| *source == text)
            .ok_or_else(|| AnalyzeError::EngineUnavailable(format!("no fixture for {text}")))?;
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ichiran")
            .join(format!("{name}.json"));
        std::fs::read_to_string(&path).map_err(|error| {
            AnalyzeError::EngineUnavailable(format!("{}: {error}", path.display()))
        })
    }
}

impl Analyzer for FixtureIchiran {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.analyze_traced(text).map(|traced| traced.analysis)
    }

    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        let json = Self::json(text)?;
        let segments = raw::parse(&json)
            .map_err(|error| AnalyzeError::BadOutput(format!("unexpected JSON: {error}")))?;
        Ok(Traced {
            analysis: ichiran::to_analysis(text, &segments),
            raw: Some(json),
        })
    }

    fn engine(&self) -> &str {
        "ichiran"
    }
}

/// A dictionary that gives every post-analyzer stage work to do over these fixtures: a compound
/// the joins can form, a heteronym the correction pass must leave alone, a reading it may confirm,
/// and words grouping can reach.
fn dictionary(dir: &Path) -> PathBuf {
    let path = dir.join("identity.zip");
    let file = std::fs::File::create(&path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let members = [
        (
            "index.json",
            json!({ "title": "Identity Fixture", "format": 3 }).to_string(),
        ),
        (
            "term_bank_1.json",
            json!([
                ["気持ち", "きもち", "n", "", 40, ["feeling"], 1, ""],
                [
                    "伝わる",
                    "つたわる",
                    "v5r",
                    "v5",
                    30,
                    ["to be conveyed"],
                    2,
                    ""
                ],
                ["水", "みず", "n", "", 40, ["water"], 3, ""],
                ["飲む", "のむ", "v5m", "v5", 40, ["to drink"], 4, ""],
                ["先生", "せんせい", "n", "", 40, ["teacher"], 5, ""],
                [
                    "情けない",
                    "なさけない",
                    "adj-i",
                    "adj-i",
                    30,
                    ["shameful"],
                    6,
                    ""
                ],
                ["晴々", "はればれ", "adv", "", 20, ["cheerful"], 7, ""],
                ["晴々", "せいせい", "adv", "", 20, ["refreshed"], 8, ""],
                ["気分", "きぶん", "n", "", 40, ["mood"], 9, ""],
                ["学生", "がくせい", "n", "", 40, ["student"], 10, ""],
                ["削除", "さくじょ", "n vs", "", 30, ["deletion"], 11, ""],
                [
                    "間違う",
                    "まちがう",
                    "v5u",
                    "v5",
                    30,
                    ["to be mistaken"],
                    12,
                    ""
                ],
                ["怒る", "おこる", "v5r", "v5", 30, ["to get angry"], 13, ""]
            ])
            .to_string(),
        ),
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
    vec![Dictionary::import(&archive, &dir.join("out")).expect("import the identity fixture")]
}

/// Every entry point, and the traced run, over one text: they must agree completely.
fn assert_one_path(analyzer: &dyn Analyzer, text: &str, dictionaries: &[Dictionary]) {
    let label = format!("{} over {text}", analyzer.engine());

    let served = study::analyse(text, analyzer, dictionaries).expect("production analysis");
    let grouped = study::analyse_with_grouping(text, analyzer, dictionaries).expect("with account");
    let traced = study::run(
        text,
        analyzer,
        dictionaries,
        AnalysisOptions::PRODUCTION.traced(),
    )
    .unwrap_or_else(|failure| panic!("{label}: {:?}", failure.error));
    assert_eq!(
        served, grouped.analysis,
        "{label}: analyse vs analyse_with_grouping"
    );
    assert_eq!(
        served, traced.analysis,
        "{label}: tracing changed what was computed"
    );

    let bare = study::analyse_without_grouping(text, analyzer, dictionaries).expect("ungrouped");
    let bare_traced = study::run(
        text,
        analyzer,
        dictionaries,
        AnalysisOptions::WITHOUT_GROUPING.traced(),
    )
    .unwrap_or_else(|failure| panic!("{label}: {:?}", failure.error));
    assert_eq!(
        bare, bare_traced.analysis,
        "{label}: ungrouped, traced vs not"
    );

    // The stages are the stages the served analysis came through, not a re-derivation.
    let stages = traced.stages.expect("a traced run keeps its stages");
    assert_eq!(
        stages.converted,
        analyzer.analyze(text).expect("analyzer"),
        "{label}: the converted stage is what the analyzer returns"
    );
    assert_eq!(
        stages.repaired, bare,
        "{label}: the repaired stage is the ungrouped analysis"
    );
    let unrepaired = study::run(
        text,
        analyzer,
        dictionaries,
        AnalysisOptions::WITHOUT_GROUPING.without_repairs(),
    )
    .unwrap_or_else(|failure| panic!("{label}: {:?}", failure.error));
    assert_eq!(
        stages.displayed, unrepaired.analysis,
        "{label}: the displayed stage is the analysis before any repair"
    );
    assert!(
        traced.grouping.is_some() && bare_traced.grouping.is_none(),
        "{label}: grouping's account exists exactly when grouping ran"
    );
}

#[test]
fn every_entry_point_is_one_path_for_both_engines() {
    let scratch = common::Scratch::new("analysis-path-identity", "hermetic");
    let dictionaries = open(&scratch);
    let lindera = LinderaAnalyzer::new().expect("the built-in analyzer is compiled in");

    // **Teeth.** Identity over stages that all did nothing would pass whatever changed, so the
    // dictionary must give the display pipeline verdicts to reach over this corpus.
    let pipeline = sakatsu_core::pipeline::Pipeline::new(&dictionaries);
    let mut verdicts = 0;
    for analyzer in [&FixtureIchiran as &dyn Analyzer, &lindera] {
        for (_, text) in FIXTURES {
            let mut analysis = analyzer.analyze(text).expect("analyzes");
            let report = pipeline.display(&mut analysis);
            verdicts +=
                report.readings.corrected + report.readings.confirmed + report.readings.ambiguous;
        }
    }
    assert!(
        verdicts > 0,
        "the fixture dictionary must give the pipeline work to do"
    );
    // And the repairs something to repair, or identity over them would be identity over a no-op.
    let repaired: usize = FIXTURES
        .iter()
        .filter_map(|(_, text)| {
            study::run(
                text,
                &FixtureIchiran,
                &dictionaries,
                AnalysisOptions::PRODUCTION,
            )
            .ok()
        })
        .map(|run| run.repairs.notes().len())
        .sum();
    assert_eq!(repaired, 2, "the two contraction fixtures are repaired");

    for (_, text) in FIXTURES {
        assert_one_path(&FixtureIchiran, text, &dictionaries);
        assert_one_path(&lindera, text, &dictionaries);
    }
    drop(dictionaries);
}

#[test]
fn a_traced_ichiran_run_keeps_the_sidecar_output_and_an_untraced_one_does_not() {
    let (_, text) = FIXTURES[0];
    let traced = study::run(
        text,
        &FixtureIchiran,
        &[],
        AnalysisOptions::PRODUCTION.traced(),
    )
    .unwrap_or_else(|failure| panic!("{:?}", failure.error));
    let raw = traced
        .stages
        .as_ref()
        .and_then(|stages| stages.raw.as_deref())
        .expect("the adapter keeps its raw output when asked");
    assert!(
        raw.trim_start().starts_with('['),
        "the raw stage is the JSON itself"
    );

    let plain = study::run(text, &FixtureIchiran, &[], AnalysisOptions::PRODUCTION)
        .unwrap_or_else(|failure| panic!("{:?}", failure.error));
    assert!(
        plain.stages.is_none(),
        "production keeps nothing it was not asked for"
    );

    // In-process: there is no raw stage, and that is reported as none rather than as empty.
    let lindera = LinderaAnalyzer::new().expect("built in");
    let traced = study::run(text, &lindera, &[], AnalysisOptions::PRODUCTION.traced())
        .unwrap_or_else(|failure| panic!("{:?}", failure.error));
    assert!(traced.stages.expect("kept").raw.is_none());
}

/// A refused passage keeps the stages it got through, which is the case a stage replay needs most:
/// the span contract refuses *after* the display pipeline, so the error alone cannot say which
/// stage broke the passage.
#[test]
fn a_refused_run_keeps_the_stages_it_reached() {
    struct Lying;
    impl Analyzer for Lying {
        fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
            Ok(Analysis {
                execution: None,
                text: text.to_string(),
                tokens: vec![sakatsu_core::model::Token {
                    surface: "水".into(),
                    dict_form: "水".into(),
                    // Claims bytes it does not cover: the passage is 水を, the token only 水.
                    start: Some(0),
                    end: Some(3),
                    ..Default::default()
                }],
            })
        }
        fn engine(&self) -> &str {
            "lying"
        }
    }
    let failure = study::run("水を", &Lying, &[], AnalysisOptions::PRODUCTION.traced())
        .expect_err("uncovered content is refused");
    let stages = failure.stages.expect("the stages reached are kept");
    assert_eq!(stages.displayed.tokens.len(), 1);
    // A span refusal is its own typed error, so a retry can say why it happened.
    assert!(matches!(failure.error, AnalyzeError::InvalidSpans(_)));

    let untraced = study::run("水を", &Lying, &[], AnalysisOptions::PRODUCTION)
        .expect_err("refused either way");
    assert!(untraced.stages.is_none());
}
