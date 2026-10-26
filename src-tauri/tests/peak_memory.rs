//! What the reading path costs in memory, measured where the number means something.
//!
//! ```text
//! cargo test --release -p sakatsu-desktop --test peak_memory -- --ignored --nocapture
//! ```
//!
//! # Why one workload per process
//!
//! `GetProcessMemoryInfo` reports `PeakWorkingSetSize` — the largest working set the **whole
//! process** has ever held. It never falls, so a before-and-after difference around a call is not
//! that call's cost: it is zero whenever anything earlier was heavier, which is precisely when the
//! answer would have been interesting. Reading it around a call is the mistake this file exists to
//! avoid, not the technique it uses.
//!
//! So each workload runs in a process that does nothing else, and reports its peak once, at the
//! end. A process that does nothing at all is reported alongside, because a peak without its
//! baseline is a number about Windows and the Rust runtime as much as about the analyzer.
//!
//! # What is and is not counted
//!
//! In-process work only: the analyzer, the dictionaries, the pipeline. **Sidecars are separate
//! processes and are not summed in** — Ichiran under WSL, the VLM runtime, the OCR engines. Their
//! memory is real, and adding it to this figure would produce a number describing no process that
//! exists.

use std::time::Instant;

use sakatsu_core::analyzer::assist::{Assistance, AssistanceMode};
use sakatsu_core::analyzer::ichiran::{self, raw, TiePolicy};
use sakatsu_core::analyzer::{AnalyzeError, Analyzer, LinderaAnalyzer, Traced};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::{Analysis, AnalyzerPolicy};
use sakatsu_core::pipeline::Pipeline;
use sakatsu_core::study::{self, AnalysisOptions, Plan};
use sakatsu_desktop_lib::memory::{megabytes, peak_working_set};
use sha2::{Digest, Sha256};

/// Names the workload a child process should run, and nothing else.
const WORKLOAD: &str = "SAKATSU_MEMORY_WORKLOAD";

/// The passages, long enough that a page of prose is what is being weighed.
fn passage() -> String {
    const SENTENCES: &[&str] = &[
        "彼は毎日図書館で勉強している。",
        "その問題については明日話し合いましょう。",
        "天気が良ければ公園に行きます。",
        "父は昨日新しい車を買いました。",
        "駅までは歩いて十分ほどかかります。",
    ];
    SENTENCES.repeat(20).concat()
}

fn dictionaries_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_DICT_DIR") {
        return std::path::PathBuf::from(dir);
    }
    let appdata = std::env::var_os("APPDATA").expect("APPDATA (or set SAKATSU_DICT_DIR)");
    std::path::PathBuf::from(appdata)
        .join("com.sakatsu.app")
        .join("dictionaries")
}

/// One workload, in a process that has done nothing else.
///
/// Useful only as a child of [`peak_memory_of_the_reading_path`]. Run directly it does nothing,
/// because a peak read in a process that has already run three workloads is a peak for the
/// heaviest of them and not for the one that was asked about.
#[test]
#[ignore = "a child process of peak_memory_of_the_reading_path"]
fn one_workload() {
    let Some(workload) = std::env::var_os(WORKLOAD) else {
        println!("skipped: spawned by peak_memory_of_the_reading_path, not run directly");
        return;
    };
    let workload = workload.to_string_lossy().to_string();

    match workload.as_str() {
        // The floor: a process that starts and stops. Everything else is read against this.
        "baseline" => {}
        "analyzer" => {
            let analyzer = LinderaAnalyzer::new().expect("compiled in");
            let analysis = analyzer.analyze(&passage()).expect("the analyzer succeeds");
            assert!(!analysis.tokens.is_empty());
        }
        // **Opening, and one lookup. Not the cost of reading.** A shard is read from disk and
        // cached the first time a headword in it is asked for, so a process that has looked one
        // word up has filled almost nothing. Reported under its own name because the figure it
        // produces — a fraction of a megabyte — was once quoted as what the dictionaries cost.
        "dictionaries-open" => {
            let dictionaries = open_dictionaries();
            let hits = dictionaries[0].lookup("図書館").expect("a lookup succeeds");
            assert!(!hits.is_empty() || dictionaries.len() > 1);
        }
        // What a reader actually costs: every token of a passage looked up, so the shard caches
        // fill the way they do in use. This is the dictionary memory figure.
        "dictionaries-reading" => {
            let analyzer = LinderaAnalyzer::new().expect("compiled in");
            let dictionaries = open_dictionaries();
            let analysis = analyzer.analyze(&passage()).expect("the analyzer succeeds");
            let mut found = 0usize;
            for token in &analysis.tokens {
                if !sakatsu_core::model::contains_japanese(&token.surface) {
                    continue;
                }
                for dictionary in &dictionaries {
                    found += dictionary
                        .lookup(&token.surface)
                        .expect("a lookup succeeds")
                        .len();
                }
            }
            assert!(found > 0, "a passage of Japanese must reach the dictionary");
        }
        "pipeline" => {
            let analyzer = LinderaAnalyzer::new().expect("compiled in");
            let dictionaries = open_dictionaries();
            let mut analysis = analyzer.analyze(&passage()).expect("the analyzer succeeds");
            Pipeline::new(&dictionaries).display(&mut analysis);
            assert!(!analysis.tokens.is_empty());
        }
        // **First use of the Ichiran path, as the shell meets it**: the built-in analyzer built
        // where `run` builds it, then one passage whose tie asks the embedded parser for advice,
        // then the built-in's part of a Reference search. Two instances is the shell before
        // unification; one is `UNIFY_LINDERA`.
        "first-use-two-instances-legacy" => first_use(false, AssistanceMode::Legacy),
        "first-use-unified-shared-lazy" => first_use(true, AssistanceMode::SharedLazy),
        "first-use-unified-shared-eager" => first_use(true, AssistanceMode::SharedEager),
        other => panic!("unknown workload {other:?}"),
    }

    let peak = peak_working_set().expect("Windows reports a peak working set");
    println!("PEAK {workload} {peak}");
}

/// Ichiran's committed answer for 晴々とした気分, whose 晴々 is an equal-score tie the parser
/// is asked about — converted inside the request, as the adapter converts a live answer.
struct Replayed(String);

impl Analyzer for Replayed {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.analyze_traced(text).map(|traced| traced.analysis)
    }
    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        self.analyze_in(text, &Assistance::new(AssistanceMode::Legacy))
    }
    fn analyze_in(&self, text: &str, assistance: &Assistance<'_>) -> Result<Traced, AnalyzeError> {
        let segments = raw::parse(&self.0)
            .map_err(|error| AnalyzeError::BadOutput(format!("unexpected JSON: {error}")))?;
        Ok(Traced {
            analysis: ichiran::to_analysis_in(text, &segments, TiePolicy::SHIPPED, assistance)?,
            raw: None,
        })
    }
    fn engine(&self) -> &str {
        "ichiran"
    }
}

const TIED: &str = "晴々とした気分";

fn first_use(unified: bool, mode: AssistanceMode) {
    let built_in: std::sync::Arc<dyn Analyzer> = if unified {
        sakatsu_core::analyzer::lindera::shared_analyzer().expect("the shared parser")
    } else {
        std::sync::Arc::new(LinderaAnalyzer::new().expect("compiled in"))
    };
    let dictionaries = open_dictionaries();
    let json = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../core/tests/fixtures/ichiran/harebare.json"),
    )
    .expect("the committed fixture");
    let primary = Replayed(json);
    let plan = Plan {
        requested: AnalyzerPolicy::Automatic,
        primary: &primary,
        fallback: Some(built_in.as_ref()),
        standing_in: None,
    };
    let done = study::analyse_resilient(
        TIED,
        &plan,
        &dictionaries,
        AnalysisOptions::PRODUCTION.with_assistance(mode),
        &|| true,
    )
    .expect("answered");
    assert!(
        done.assistance.advice.executed() > 0,
        "the tie asked the parser, so the advice path was used"
    );
    let searched = built_in.analyze("気分").expect("a search reads");
    assert!(!searched.tokens.is_empty());
}

/// Shared with the application: `Dictionary::discover` decides what counts and in what order, and
/// a dictionary that will not open fails the run rather than shrinking it quietly.
fn open_dictionaries() -> Vec<Dictionary> {
    let dictionaries: Vec<Dictionary> = Dictionary::discover(&dictionaries_dir())
        .into_iter()
        .map(|path| {
            Dictionary::open(&path).unwrap_or_else(|error| {
                panic!(
                    "a required dictionary at {} would not open: {error}",
                    path.display()
                )
            })
        })
        .collect();
    assert!(
        !dictionaries.is_empty(),
        "no dictionaries at {} — run scripts/fetch-dictionaries.ps1",
        dictionaries_dir().display()
    );
    dictionaries
}

/// A `#[cfg]` inside its own function, so the release build carries no check and the debug build
/// cannot be talked out of one — and so the diverging call does not make the caller unreachable.
fn require_release() {
    #[cfg(debug_assertions)]
    panic!("run this under --release: a debug build's allocations are not the shipped ones");
}

/// Peak working set of the reading path, one workload per process.
#[test]
#[ignore = "a measurement; spawns child processes and needs the dictionaries"]
fn peak_memory_of_the_reading_path() {
    require_release();
    assert!(
        peak_working_set().is_some(),
        "this platform does not report a peak working set, so there is nothing to measure — \
         reported as absent rather than as zero"
    );

    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let executable = std::fs::read(&exe).expect("the running executable can be fingerprinted");
    let fixture = passage();
    // Keep these readers alive across every child so the generations named here remain available.
    // The children use the same discovery path; taking the fingerprints in the parent keeps that
    // file traversal outside the isolated child whose peak is the thing being measured.
    let identity_dictionaries = open_dictionaries();
    println!("\n=== peak-memory identity ===");
    println!(
        "  executable      {} ({} bytes, release)",
        sakatsu_core::hashing::hex(Sha256::digest(&executable)),
        executable.len()
    );
    println!(
        "  fixture sha256  {}",
        sakatsu_core::hashing::hex(Sha256::digest(fixture.as_bytes()))
    );
    println!("  configuration   release; one workload per fresh child process");
    for dictionary in &identity_dictionaries {
        println!(
            "  resource        dictionary {} {}",
            dictionary.meta().title,
            dictionary
                .content_fingerprint()
                .expect("the generation selected for the children can be fingerprinted")
        );
    }
    let mut rows = Vec::new();
    for workload in [
        "baseline",
        "analyzer",
        "dictionaries-open",
        "dictionaries-reading",
        "pipeline",
        "first-use-two-instances-legacy",
        "first-use-unified-shared-lazy",
        "first-use-unified-shared-eager",
    ] {
        let started = Instant::now();
        let output = std::process::Command::new(&exe)
            .args(["one_workload", "--exact", "--ignored", "--nocapture"])
            .env(WORKLOAD, workload)
            .output()
            .expect("spawn a fresh process");
        assert!(
            output.status.success(),
            "the {workload} workload failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text
            .lines()
            .find(|line| line.starts_with("PEAK "))
            .unwrap_or_else(|| panic!("the {workload} workload reported no peak:\n{text}"));
        let peak: u64 = line
            .split_whitespace()
            .nth(2)
            .expect("a peak")
            .parse()
            .expect("bytes");
        rows.push((workload, peak, started.elapsed()));
    }

    let baseline = rows[0].1;
    println!("\n=== peak working set, one workload per process ===");
    println!(
        "  {:<22} {:>10} {:>14}   {:>8}",
        "", "peak MB", "over floor MB", "wall"
    );
    for (workload, peak, elapsed) in &rows {
        println!(
            "  {:<22} {:>10.1} {:>14.1}   {:>7.2}s",
            workload,
            megabytes(*peak),
            megabytes(peak.saturating_sub(baseline)),
            elapsed.as_secs_f64()
        );
    }
    println!(
        "\n  \"over floor\" is measured against a process that starts and stops, so it excludes\n  \
         Windows and the Rust runtime. Sidecars are separate processes and are NOT included:\n  \
         Ichiran under WSL, the VLM runtime and the OCR engines have their own memory, and\n  \
         adding it here would describe no process that exists.\n\n  \
         Read dictionaries-reading, not dictionaries-open, for what the dictionaries cost a\n  \
         reader: the open row has filled one shard and is a figure about opening a file."
    );

    // **The reading row contains the analyzer**, because tokenising a passage is how you get
    // words to look up. Stating the difference here stops 194.9 being read as what the
    // dictionaries cost, which is the same misreading 0.3 invited from the other direction.
    let peak_of = |name: &str| {
        rows.iter()
            .find(|(workload, _, _)| *workload == name)
            .map(|(_, peak, _)| *peak)
    };
    if let (Some(analyzer), Some(reading)) = (peak_of("analyzer"), peak_of("dictionaries-reading"))
    {
        println!(
            "
  Dictionary shard caches over the analyzer alone: {:.1} MB. The reading row
               includes the analyzer, because tokenising is how a passage becomes words to look up.",
            megabytes(reading.saturating_sub(analyzer))
        );
    }

    assert!(
        rows.iter().any(|(_, peak, _)| *peak > baseline),
        "every workload matched the empty process, which means nothing was measured"
    );
}
