//! **Is the reader's output identical?** The harness behind `reader_equivalence_record.rs` (which
//! records the code as it stands) and `reader_equivalence.rs` (which compares the assistance modes
//! against a recording).
//!
//! # Written against the analysis API as it stood before shared assistance
//!
//! Everything here uses entry points the code had before `study::AnalysisContext` existed —
//! `study::run`, `study::from_analysis`, `ichiran::to_analysis`. That is what lets the **preserved
//! pre-change source** (restored from its hash-verified backup into an isolated copy) compile this
//! module and the recorder, and record its own baseline. A baseline recorded by the code under test
//! would compare the code with itself.
//!
//! # What a recording holds, and what a comparison refuses
//!
//! Per passage, per engine (Ichiran's recorded raw answer, replayed; the built-in analyzer), per
//! store (empty; the deterministic populated fixture): an **explicit outcome** — `ok` with the
//! SHA-256 of the complete `StudyView`, or `error` with the error's kind and digest. A failure is
//! never a digest that merely happens to match.
//!
//! A comparison names its baseline configuration — there is no "first one" — and fails on:
//!
//! - a baseline in another schema or for another workload;
//! - any differing resource identity: workload, store fixture, kanji table, notebook,
//!   dictionary fingerprints, case counts, Ichiran coverage;
//! - a baseline configuration that is absent, or whose keys are not exactly the expected set;
//! - a compared configuration whose keys are not exactly the expected set;
//! - any differing outcome or digest.
//!
//! A dictionary that cannot be fingerprinted fails the run: an empty fingerprint would match
//! another empty fingerprint.
//!
//! # Licence
//!
//! KWDLC is consult-only and the notebook is the reader's own: passages and raw answers go only to
//! the ignored private artifacts; what is printed carries identifiers and digests.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, Traced};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::dict::KanjiDic;
use sakatsu_core::store::{Store, WordKey};
use sakatsu_core::study::{self, Resources};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The recording format. Schema 1 kept bare digests, with a failure hashed beside the successes.
pub const SCHEMA: u64 = 2;
pub const ENGINES: [&str; 2] = ["ichiran", "lindera"];
pub const STORES: [&str; 2] = ["empty", "populated"];

/// Identity fields a baseline and a comparison must share. The executable is deliberately absent:
/// the two runs are different builds by construction.
pub const IDENTITY_FIELDS: [&str; 8] = [
    "workload_id",
    "store_fixture",
    "kanji_sha256",
    "notebook_sha256",
    "dictionaries",
    "cases",
    "by_source",
    "ichiran_answered",
];

/// One passage to analyse.
pub struct Case {
    pub source: &'static str,
    pub id: String,
    pub text: String,
}

/// Which workload a run is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workload {
    /// The store passages, the fixtures, KWDLC 300, UD 259, the heteronyms and the notebook.
    Full,
    /// Verified sentences split across lines, so the joined-line questions are actually asked.
    Multiline,
}

impl Workload {
    /// `READER_EQUIVALENCE_WORKLOAD` = `full` (the default) or `multiline`.
    pub fn from_env() -> Workload {
        match std::env::var("READER_EQUIVALENCE_WORKLOAD").as_deref() {
            Err(_) | Ok("full") => Workload::Full,
            Ok("multiline") => Workload::Multiline,
            Ok(other) => {
                panic!("READER_EQUIVALENCE_WORKLOAD must be full or multiline, not {other:?}")
            }
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Workload::Full => "full",
            Workload::Multiline => "multiline",
        }
    }

    fn raw_env(self) -> &'static str {
        match self {
            Workload::Full => "READER_EQUIVALENCE_RAW",
            Workload::Multiline => "READER_EQUIVALENCE_MULTILINE_RAW",
        }
    }

    fn baseline_env(self) -> &'static str {
        match self {
            Workload::Full => "READER_EQUIVALENCE_BASELINE",
            Workload::Multiline => "READER_EQUIVALENCE_MULTILINE_BASELINE",
        }
    }

    fn stem(self) -> &'static str {
        match self {
            Workload::Full => "reader-equivalence",
            Workload::Multiline => "reader-equivalence-multiline",
        }
    }
}

pub fn sha(text: &str) -> String {
    sakatsu_core::hashing::hex(Sha256::digest(text.as_bytes()))
}

/// Where private artifacts go: `SAKATSU_EVALUATION_DIR`, or the repository's `target/evaluation`.
/// The override lets the isolated pre-change copy read the reservation and publish beside the
/// artifacts it is compared with.
pub fn evaluation_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_EVALUATION_DIR") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("target/evaluation")
}

/// Every committed fixture's name and text, read from the capture script that produced them.
fn fixture_texts() -> Vec<(String, String)> {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/capture-ichiran-fixtures.sh");
    let text = std::fs::read_to_string(&script).expect("the capture script");
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("capture ")?;
            let (name, quoted) = rest.split_once(char::is_whitespace)?;
            let body = quoted.trim().strip_prefix('\'')?.split('\'').next()?;
            Some((name.to_string(), body.to_string()))
        })
        .collect()
}

fn store_cases() -> Vec<Case> {
    super::store_fixture::WORKLOAD
        .iter()
        .map(|(id, text)| Case {
            source: "store",
            id: (*id).to_string(),
            text: (*text).to_string(),
        })
        .collect()
}

fn fixture_cases() -> Vec<Case> {
    fixture_texts()
        .into_iter()
        .map(|(name, text)| Case {
            source: "fixture",
            id: format!("fixture-{name}"),
            text,
        })
        .collect()
}

fn kwdlc_cases(evaluation: &Path) -> Vec<Case> {
    let corpus = std::env::var_os("SAKATSU_KWDLC")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            super::dictionaries_dir()
                .parent()
                .map(|parent| parent.join("corpora").join("kwdlc"))
                .unwrap_or_default()
        })
        .join("knp");
    let (reserved, audit) = super::reference::reserved_documents(evaluation);
    assert!(
        !reserved.is_empty(),
        "the reservation must be readable; {audit}"
    );
    let (all_files, faults) = super::knp::collect(&corpus);
    let development: Vec<PathBuf> = all_files
        .into_iter()
        .filter(|path| {
            !reserved.contains(
                &path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .collect();
    let sentences =
        super::knp::load_files(&development, faults).unwrap_or_else(|damaged| panic!("{damaged}"));
    let stride = sentences.len().div_ceil(2000).max(1);
    sentences
        .iter()
        .step_by(stride)
        .take(300)
        .map(|sentence| Case {
            source: "kwdlc",
            id: sentence.id(),
            text: sentence.text.clone(),
        })
        .collect()
}

fn ud_cases() -> Vec<Case> {
    let sentences = super::gold::test_and_dev();
    let stride = sentences.len().div_ceil(300).max(1);
    sentences
        .iter()
        .step_by(stride)
        .map(|sentence| Case {
            source: "ud",
            id: sentence.id.clone(),
            text: sentence.text.clone(),
        })
        .collect()
}

fn heteronym_cases(texts: &[&str]) -> Vec<Case> {
    texts
        .iter()
        .enumerate()
        .map(|(index, text)| Case {
            source: "heteronym",
            id: format!("heteronym-{index:03}"),
            text: (*text).to_string(),
        })
        .collect()
}

/// Every unique non-empty notebook line, in order, named by its first line number, and the
/// notebook's digest. **Required**: a full workload without it is a different workload.
fn notebook_cases() -> (Vec<Case>, String) {
    let path = std::env::var_os("SAKATSU_NOTEBOOK")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("APPDATA").unwrap_or_default())
                .join("com.sakatsu.app")
                .join("corpus")
                .join("captured.txt")
        });
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("the notebook at {} is required: {error}", path.display()));
    let digest = sakatsu_core::hashing::hex(Sha256::digest(&bytes));
    let text = String::from_utf8_lossy(&bytes);
    let mut seen = BTreeSet::new();
    let cases = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .filter(|(_, line)| seen.insert(line.trim().to_string()))
        .map(|(index, line)| Case {
            source: "notebook",
            id: format!("notebook-L{}", index + 1),
            text: line.to_string(),
        })
        .collect();
    (cases, digest)
}

/// What ends a sentence on its own line, stripped before stacking so the break has to be decided.
const TERMINATORS: &[char] = &['。', '！', '？', '!', '?', '…'];

/// **Verified sentences across lines, built mechanically and invented nowhere.** The sources are the
/// store workload and every committed fixture text (Tatoeba via Jitendex, and the reported
/// capture's words). Four shapes, the ones `analyzer::lines` decides between:
///
/// - `ml-wrap-*`: one sentence cut at its character midpoint — the wrap corpus's construction
///   (`project notes` §7), and the case the straddle probe exists for;
/// - `ml-crlf-*`: the same cut as a Windows line ending, every third sentence;
/// - `ml-stack-*`: three consecutive sentences, terminators stripped, one per line — breaks
///   neither cheap signal settles;
/// - `ml-para-*`: two of them with a blank line between, which is never a wrap.
pub fn multiline_cases() -> Vec<Case> {
    let mut sources: Vec<(String, String)> = super::store_fixture::WORKLOAD
        .iter()
        .map(|(id, text)| ((*id).to_string(), (*text).to_string()))
        .collect();
    for (name, text) in fixture_texts() {
        if !sources.iter().any(|(_, seen)| *seen == text) {
            sources.push((name, text));
        }
    }

    let mut cases = Vec::new();
    for (index, (name, text)) in sources.iter().enumerate() {
        let chars: Vec<char> = text.chars().collect();
        if chars.len() < 4 {
            continue;
        }
        let half = chars.len() / 2;
        let before: String = chars[..half].iter().collect();
        let after: String = chars[half..].iter().collect();
        cases.push(Case {
            source: "multiline",
            id: format!("ml-wrap-{name}"),
            text: format!("{before}\n{after}"),
        });
        if index % 3 == 0 {
            cases.push(Case {
                source: "multiline",
                id: format!("ml-crlf-{name}"),
                text: format!("{before}\r\n{after}"),
            });
        }
    }
    let bare: Vec<String> = sources
        .iter()
        .map(|(_, text)| text.trim_end_matches(TERMINATORS).to_string())
        .filter(|text| !text.is_empty())
        .collect();
    for (index, start) in (0..bare.len().saturating_sub(2)).step_by(3).enumerate() {
        cases.push(Case {
            source: "multiline",
            id: format!("ml-stack-{index:02}"),
            text: bare[start..start + 3].join("\n"),
        });
        if index % 3 == 0 {
            cases.push(Case {
                source: "multiline",
                id: format!("ml-para-{index:02}"),
                text: bare[start..start + 2].join("\n\n"),
            });
        }
    }
    cases
}

/// The cases of a workload, and the notebook's digest where the workload has one.
pub fn cases(
    workload: Workload,
    heteronyms: &[&str],
    evaluation: &Path,
) -> (Vec<Case>, Option<String>) {
    match workload {
        Workload::Full => {
            let mut cases = store_cases();
            cases.extend(fixture_cases());
            cases.extend(kwdlc_cases(evaluation));
            cases.extend(ud_cases());
            cases.extend(heteronym_cases(heteronyms));
            let (notebook, digest) = notebook_cases();
            cases.extend(notebook);
            (cases, Some(digest))
        }
        Workload::Multiline => (multiline_cases(), None),
    }
}

pub fn workload_id(cases: &[Case]) -> String {
    sha(&cases
        .iter()
        .map(|case| format!("{}\u{1f}{}\u{1e}", case.id, case.text))
        .collect::<String>())
}

/// Every key a complete run produces.
pub fn expected_keys(cases: &[Case]) -> BTreeSet<String> {
    cases
        .iter()
        .flat_map(|case| {
            ENGINES.iter().flat_map(move |engine| {
                STORES
                    .iter()
                    .map(move |store| format!("{}|{engine}|{store}", case.id))
            })
        })
        .collect()
}

/// Each dictionary's title and content fingerprint. **A fingerprint that cannot be taken is an
/// error**, never an empty string.
pub fn fingerprints<E: std::fmt::Display>(
    dictionaries: impl IntoIterator<Item = (String, Result<String, E>)>,
) -> Result<Vec<Value>, String> {
    dictionaries
        .into_iter()
        .map(|(title, fingerprint)| match fingerprint {
            Ok(sha) if !sha.is_empty() => Ok(json!({"title": title, "sha256": sha})),
            Ok(_) => Err(format!("{title}: an empty fingerprint")),
            Err(error) => Err(format!("{title}: {error}")),
        })
        .collect()
}

/// One outcome, recorded explicitly.
pub fn record(
    outcome: &Result<study::Run, study::RunFailure>,
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
    kanji: &KanjiDic,
    store: Option<&Store>,
    keys: &mut HashSet<WordKey>,
) -> Value {
    match outcome {
        Ok(run) => {
            for token in &run.analysis.tokens {
                keys.insert(WordKey::for_token(token));
                for part in &token.components {
                    keys.insert(WordKey::for_token(part));
                }
            }
            let view = study::from_analysis(
                run.analysis.clone(),
                Resources {
                    analyzer,
                    dictionaries,
                    kanji,
                    store,
                },
            );
            json!({
                "outcome": "ok",
                "digest": sha(&serde_json::to_string(&view).expect("a view serializes")),
            })
        }
        Err(failure) => {
            let debug = format!("{:?}", failure.error);
            let kind = debug.split('(').next().unwrap_or_default().to_string();
            json!({
                "outcome": "error",
                "kind": kind,
                "digest": sha(&format!("error: {}", failure.error)),
            })
        }
    }
}

/// A configuration: a name, and how it runs one passage.
pub type Runner<'a> =
    &'a dyn Fn(&str, &dyn Analyzer, &[Dictionary]) -> Result<study::Run, study::RunFailure>;

/// A run over one workload, ready to be published and compared.
pub struct Recorded {
    pub workload: Workload,
    pub value: Value,
    pub expected: BTreeSet<String>,
    pub uncovered: Vec<String>,
}

/// Run every configuration over the workload. `on_run` sees every run before it is digested, so a
/// caller can account for it. Ichiran is asked live only when no raw recording is named, and then
/// the answers are kept privately for every later run.
pub fn drive(
    workload: Workload,
    heteronyms: &[&str],
    configurations: &[(&str, Runner<'_>)],
    replayed: &dyn Fn(String) -> Box<dyn Analyzer>,
    built_in: &dyn Analyzer,
    on_run: &mut dyn FnMut(&str, &str, &study::Run),
) -> Recorded {
    let evaluation = evaluation_dir();
    let dictionaries = super::installed_dictionaries();
    assert!(!dictionaries.is_empty(), "no dictionaries installed");
    let kanji_path = super::dictionaries_dir()
        .parent()
        .expect("the dictionaries directory has a parent")
        .join("kanji")
        .join("kanjidic.tsv");
    let kanji = KanjiDic::load(&kanji_path);
    assert!(!kanji.is_empty(), "no KanjiDic at {}", kanji_path.display());
    let kanji_digest = sakatsu_core::hashing::hex(Sha256::digest(
        std::fs::read(&kanji_path).expect("the kanji table"),
    ));

    let (cases, notebook_digest) = cases(workload, heteronyms, &evaluation);
    let workload_id = workload_id(&cases);
    let expected = expected_keys(&cases);

    // ---- raw answers: replayed from a recording, or asked for once and recorded.
    let raws: BTreeMap<String, String> = match std::env::var_os(workload.raw_env()) {
        Some(path) => {
            let recorded: Value =
                serde_json::from_slice(&std::fs::read(&path).expect("the raw artifact"))
                    .expect("the raw artifact parses");
            assert_eq!(
                recorded["workload_id"].as_str(),
                Some(workload_id.as_str()),
                "the recorded raw answers are for a different workload"
            );
            recorded["raws"]
                .as_object()
                .expect("raws")
                .iter()
                .map(|(id, raw)| (id.clone(), raw.as_str().expect("a raw answer").to_string()))
                .collect()
        }
        None => {
            let ichiran = IchiranAnalyzer::new();
            sakatsu_core::analyzer::ichiran::server::set_persistent(true);
            ichiran.warm();
            assert!(
                ichiran.analyze("これって").is_ok(),
                "Ichiran must answer to record"
            );
            let mut raws = BTreeMap::new();
            for case in &cases {
                match ichiran.analyze_traced(&case.text) {
                    Ok(Traced { raw: Some(raw), .. }) => {
                        raws.insert(case.id.clone(), raw);
                    }
                    Ok(_) => panic!("{}: Ichiran gave no raw output", case.id),
                    Err(error) => panic!("{}: Ichiran did not answer: {error}", case.id),
                }
            }
            let path = super::reference::publish(
                &evaluation,
                &format!("{}-raw", workload.stem()),
                "json",
                &json!({"workload": workload.name(), "workload_id": workload_id, "raws": raws})
                    .to_string(),
            )
            .expect("publish the raw answers");
            println!("  raw answers  {} (private)", path.display());
            raws
        }
    };
    // **Every passage has Ichiran's answer**, so the expected key set is the whole workload.
    let unanswered: Vec<&str> = cases
        .iter()
        .filter(|case| !raws.contains_key(&case.id))
        .map(|case| case.id.as_str())
        .collect();
    assert!(unanswered.is_empty(), "no raw answer for {unanswered:?}");

    // ---- outcomes, per configuration.
    let scratch = super::Scratch::new("reader-equivalence", "store");
    let populated = super::store_fixture::populated(&scratch);
    let mut configurations_out = serde_json::Map::new();
    let mut store_keys_seen = HashSet::new();
    for (name, run) in configurations {
        let mut records = BTreeMap::new();
        for case in &cases {
            let ichiran = replayed(raws[&case.id].clone());
            for (engine, analyzer) in [("ichiran", ichiran.as_ref()), ("lindera", built_in)] {
                for (store_name, store) in [("empty", None), ("populated", Some(&populated))] {
                    let outcome = run(&case.text, analyzer, &dictionaries);
                    if let Ok(done) = &outcome {
                        on_run(name, engine, done);
                    }
                    let mut keys = HashSet::new();
                    let entry = record(&outcome, analyzer, &dictionaries, &kanji, store, &mut keys);
                    if store_name == "populated" {
                        store_keys_seen.extend(keys);
                    }
                    records.insert(format!("{}|{engine}|{store_name}", case.id), entry);
                }
            }
        }
        let produced: BTreeSet<String> = records.keys().cloned().collect();
        assert_eq!(
            produced, expected,
            "{name}: the run did not produce exactly the expected keys"
        );
        configurations_out.insert((*name).to_string(), json!(records));
    }

    let uncovered: Vec<String> = super::store_fixture::MARKED
        .iter()
        .filter(|(term, reading, _, _)| !store_keys_seen.contains(&WordKey::new(*term, *reading)))
        .map(|(term, reading, _, why)| format!("{term}/{reading} ({why})"))
        .collect();

    let dictionary_identities = fingerprints(
        dictionaries
            .iter()
            .map(|d| (d.meta().title.clone(), d.content_fingerprint())),
    )
    .unwrap_or_else(|error| panic!("a dictionary could not be fingerprinted: {error}"));
    let identity = json!({
        "executable": super::reference::executable_identity(),
        "dictionaries": dictionary_identities,
        "kanji_sha256": kanji_digest,
        "workload_id": workload_id,
        "store_fixture": super::store_fixture::digest(),
        "notebook_sha256": notebook_digest,
        "cases": cases.len(),
        "by_source": cases.iter().fold(BTreeMap::<&str, usize>::new(), |mut counts, case| {
            *counts.entry(case.source).or_default() += 1;
            counts
        }),
        "ichiran_answered": raws.len(),
    });
    Recorded {
        workload,
        value: json!({
            "schema": SCHEMA,
            "workload": workload.name(),
            "identity": identity,
            "configurations": configurations_out,
        }),
        expected,
        uncovered,
    }
}

/// What a comparison found: per configuration, how many keys differ and how many were errors.
#[derive(Debug, PartialEq, Eq)]
pub struct Compared {
    pub lines: Vec<String>,
    pub problems: Vec<String>,
}

/// Compare every configuration in `recorded` with `baseline`'s configuration named `reference`.
/// Pure, so its refusals are tested directly.
pub fn compare(
    baseline: &Value,
    reference: &str,
    recorded: &Value,
    expected: &BTreeSet<String>,
) -> Compared {
    let mut lines = Vec::new();
    let mut problems = Vec::new();
    if baseline["schema"].as_u64() != Some(SCHEMA) {
        problems.push(format!(
            "the baseline is schema {}, not {SCHEMA}; record it again",
            baseline["schema"]
        ));
        return Compared { lines, problems };
    }
    if baseline["workload"] != recorded["workload"] {
        problems.push(format!(
            "the baseline is the {} workload, this run is {}",
            baseline["workload"], recorded["workload"]
        ));
    }
    for field in IDENTITY_FIELDS {
        let (was, now) = (&baseline["identity"][field], &recorded["identity"][field]);
        if was.is_null() && now.is_null() && field != "notebook_sha256" {
            problems.push(format!("identity {field} is missing from both"));
        } else if was != now {
            problems.push(format!(
                "identity {field} differs: baseline {was}, now {now}"
            ));
        }
    }
    let Some(reference_records) = baseline["configurations"][reference].as_object() else {
        problems.push(format!(
            "the baseline has no configuration named {reference:?}"
        ));
        return Compared { lines, problems };
    };
    let keys_of = |records: &serde_json::Map<String, Value>| -> BTreeSet<String> {
        records.keys().cloned().collect()
    };
    let missing_or_extra = |label: &str, keys: &BTreeSet<String>, problems: &mut Vec<String>| {
        let missing = expected.difference(keys).count();
        let extra = keys.difference(expected).count();
        if missing > 0 || extra > 0 {
            problems.push(format!(
                "{label}: {missing} expected keys missing and {extra} unexpected"
            ));
        }
    };
    missing_or_extra(
        &format!("baseline {reference}"),
        &keys_of(reference_records),
        &mut problems,
    );

    let Some(configurations) = recorded["configurations"].as_object() else {
        problems.push("the run recorded no configurations".into());
        return Compared { lines, problems };
    };
    for (name, records) in configurations {
        let Some(records) = records.as_object() else {
            problems.push(format!("{name}: not a record map"));
            continue;
        };
        missing_or_extra(name, &keys_of(records), &mut problems);
        let differing: Vec<&String> = expected
            .iter()
            .filter(|key| {
                let (was, now) = (reference_records.get(*key), records.get(*key));
                was.is_none() || now.is_none() || was != now
            })
            .collect();
        let errors = records
            .values()
            .filter(|entry| entry["outcome"] != "ok")
            .count();
        lines.push(format!(
            "{name:<14} {} of {} outcomes differ from {reference}; {} errors recorded",
            differing.len(),
            expected.len(),
            errors
        ));
        for key in differing.iter().take(40) {
            lines.push(format!("    differs  {key}"));
        }
        if !differing.is_empty() {
            problems.push(format!("{name}: {} outcomes differ", differing.len()));
        }
    }
    Compared { lines, problems }
}

/// Publish a run, print it, and — when a baseline is named — compare it, failing on any problem.
pub fn finish(recorded: Recorded) {
    let evaluation = evaluation_dir();
    let path = super::reference::publish(
        &evaluation,
        &format!("{}-outcomes", recorded.workload.stem()),
        "json",
        &recorded.value.to_string(),
    )
    .expect("publish the outcomes");
    println!(
        "\n=== reader output ({} workload), complete StudyView outcomes ===",
        recorded.workload.name()
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&recorded.value["identity"]).unwrap_or_default()
    );
    println!("  outcomes     {}", path.display());
    if let Some(accounts) = recorded.value.get("accounts") {
        println!(
            "\n=== questions asked, per configuration and engine ===\n{}",
            serde_json::to_string_pretty(accounts).unwrap_or_default()
        );
    }
    if !recorded.uncovered.is_empty() {
        println!("  STORE FIXTURE KEYS NOT REACHED: {:?}", recorded.uncovered);
    }

    let mut problems = Vec::new();
    if let Some(baseline_path) = std::env::var_os(recorded.workload.baseline_env()) {
        let reference = std::env::var("READER_EQUIVALENCE_BASELINE_CONFIG").unwrap_or_else(|_| {
            panic!("a baseline was named; name its configuration in READER_EQUIVALENCE_BASELINE_CONFIG")
        });
        let baseline: Value =
            serde_json::from_slice(&std::fs::read(&baseline_path).expect("the baseline artifact"))
                .expect("the baseline parses");
        let compared = compare(&baseline, &reference, &recorded.value, &recorded.expected);
        println!(
            "\n  against {} ({reference}):",
            Path::new(&baseline_path).display()
        );
        for line in &compared.lines {
            println!("  {line}");
        }
        problems.extend(compared.problems);
    }
    // The fixture must touch what it claims to — on the workload built for it.
    if recorded.workload == Workload::Full {
        assert!(
            recorded.uncovered.is_empty(),
            "the store fixture reaches nothing for {:?}",
            recorded.uncovered
        );
    }
    assert!(problems.is_empty(), "the comparison failed: {problems:#?}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> BTreeSet<String> {
        ["a|ichiran|empty", "a|lindera|empty"]
            .iter()
            .map(|key| key.to_string())
            .collect()
    }

    fn ok(digest: &str) -> Value {
        json!({"outcome": "ok", "digest": digest})
    }

    fn run(records: Value) -> Value {
        json!({
            "schema": SCHEMA,
            "workload": "full",
            "identity": {
                "workload_id": "w", "store_fixture": "s", "kanji_sha256": "k",
                "notebook_sha256": "n", "dictionaries": [{"title": "J", "sha256": "d"}],
                "cases": 1, "by_source": {"fixture": 1}, "ichiran_answered": 1,
            },
            "configurations": {"pre-change": records},
        })
    }

    fn agreeing() -> Value {
        json!({"a|ichiran|empty": ok("1"), "a|lindera|empty": ok("2")})
    }

    #[test]
    fn identical_runs_compare_clean() {
        let compared = compare(&run(agreeing()), "pre-change", &run(agreeing()), &keys());
        assert!(compared.problems.is_empty(), "{:?}", compared.problems);
    }

    #[test]
    fn a_missing_key_an_extra_key_and_a_changed_outcome_are_each_refused() {
        let missing = json!({"a|ichiran|empty": ok("1")});
        assert!(
            !compare(&run(agreeing()), "pre-change", &run(missing), &keys())
                .problems
                .is_empty()
        );

        let mut extra = agreeing();
        extra["b|ichiran|empty"] = ok("3");
        assert!(
            !compare(&run(agreeing()), "pre-change", &run(extra), &keys())
                .problems
                .is_empty()
        );

        // Same digest, but an error now: the outcome is part of what is compared.
        let failed = json!({
            "a|ichiran|empty": {"outcome": "error", "kind": "BadOutput", "digest": "1"},
            "a|lindera|empty": ok("2"),
        });
        assert!(
            !compare(&run(agreeing()), "pre-change", &run(failed), &keys())
                .problems
                .is_empty()
        );

        // A baseline that is itself incomplete is refused, not silently narrowed.
        let short = json!({"a|ichiran|empty": ok("1")});
        assert!(
            !compare(&run(short), "pre-change", &run(agreeing()), &keys())
                .problems
                .is_empty()
        );
    }

    #[test]
    fn the_baseline_configuration_must_be_named_and_present() {
        let compared = compare(&run(agreeing()), "current", &run(agreeing()), &keys());
        assert!(compared
            .problems
            .iter()
            .any(|p| p.contains("no configuration named")));
    }

    #[test]
    fn every_resource_identity_is_checked() {
        for field in IDENTITY_FIELDS {
            let mut changed = run(agreeing());
            changed["identity"][field] = json!("something else");
            let compared = compare(&run(agreeing()), "pre-change", &changed, &keys());
            assert!(
                compared.problems.iter().any(|p| p.contains(field)),
                "{field} not checked"
            );
        }
        let mut old = run(agreeing());
        old["schema"] = json!(1);
        assert!(!compare(&old, "pre-change", &run(agreeing()), &keys())
            .problems
            .is_empty());
    }

    #[test]
    fn a_dictionary_that_cannot_be_fingerprinted_fails() {
        let failed = fingerprints([("J".to_string(), Err::<String, _>("unreadable"))]);
        assert!(failed.is_err());
        let empty = fingerprints([("J".to_string(), Ok::<_, String>(String::new()))]);
        assert!(
            empty.is_err(),
            "an empty fingerprint would match another empty one"
        );
        assert!(fingerprints([("J".to_string(), Ok::<_, String>("d".into()))]).is_ok());
    }

    #[test]
    fn the_multiline_workload_breaks_lines_and_invents_nothing() {
        let cases = multiline_cases();
        assert!(cases.len() >= 30, "{}", cases.len());
        assert!(cases.iter().all(|case| case.text.contains('\n')));
        let sources: Vec<String> = fixture_texts().into_iter().map(|(_, text)| text).collect();
        for case in &cases {
            // Every line is a piece of a verified sentence.
            for line in case
                .text
                .split('\n')
                .map(|line| line.trim_end_matches('\r'))
            {
                if line.is_empty() {
                    continue;
                }
                assert!(
                    sources.iter().any(|source| source.contains(line))
                        || super::super::store_fixture::WORKLOAD
                            .iter()
                            .any(|(_, text)| text.contains(line)),
                    "{}: {line} is not from a verified sentence",
                    case.id
                );
            }
        }
    }
}
