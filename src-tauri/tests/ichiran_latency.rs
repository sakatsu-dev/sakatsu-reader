//! **What the Ichiran reading path costs, per named passage, measured the way the reader meets it.**
//!
//! ```text
//! cargo test --release -p sakatsu-desktop --test ichiran_latency -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `core/tests/paired_reader.rs` compares four configurations inside a harness: it excludes the
//! sidecar, stops at `study::run`, and says so. This measures what that one does not, over a small
//! set of **named** passages whose category is read from the run rather than assumed:
//!
//! | category | how it is known |
//! |---|---|
//! | repaired | the contraction repair applied (`Run::repairs`) |
//! | investigated, abstained | a trigger was investigated and nothing applied |
//! | answered whole | a tail-ending token Ichiran answered itself, not investigated |
//! | unaffected | no trigger at all |
//!
//! and for each, **separately**:
//!
//! 1. **the sidecar alone**, on the persistent-session route and on the one-shot route;
//! 2. **the complete `StudyView` over the captured raw answer** — `study::run` and then
//!    `study::from_analysis`, the success path of `study_resilient` — under the four
//!    configurations of the paired harness, as **paired per-passage differences**: repairs off → on,
//!    tie-break bare → contextual, and both;
//! 3. **end to end, in the production shape**: the scheduler's Ichiran lane, then
//!    `study_resilient` with the primary the shell builds — Ichiran inside `Remembering` — for a
//!    **cache miss** (a fresh cache each sample) and a **real cache hit** (verified: the sidecar is
//!    not asked again);
//! 4. **queue versus execution**: the wait for the lane when it is idle, and when another passage
//!    holds it.
//!
//! Warm-up is outside every timed region: the embedded parser, the dictionaries, the kanji table
//! and the session are all touched before the first sample. Nothing is scored inside a timed
//! region. Release only.
//!
//! What it still excludes is the IPC hop and the page's rendering, as `study_latency.rs` does.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sakatsu_core::analyzer::ichiran::{self, raw, server, TiePolicy};
use sakatsu_core::analyzer::{
    AnalyzeError, Analyzer, IchiranAnalyzer, LinderaAnalyzer, Remembering, Traced,
};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::dict::KanjiDic;
use sakatsu_core::model::{Analysis, AnalyzerPolicy};
use sakatsu_core::store::{Store, WordKey, WordStatus};
use sakatsu_core::study::{self, AnalysisOptions, Plan, Resources};
use sakatsu_desktop_lib::work::{scheduler, Lane};

fn require_release() {
    #[cfg(debug_assertions)]
    panic!("run this under --release: a debug build's ratios do not hold");
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

/// The named passages, with the category each is expected to fall in. The category reported is
/// the one the run shows; a mismatch is printed, not hidden.
///
/// The sentences are Tatoeba via Jitendex (see `scripts/capture-ichiran-fixtures.sh`); the
/// contraction fragments are the words as they appear in the reported capture and notebook lines
/// L81, L83 and L90, not those passages.
const CASES: &[(&str, &str, &str)] = &[
    ("machigattoru", "頭文字から間違っとるわい！！", "repaired"),
    ("okonnakya", "怒んなきゃ", "repaired"),
    ("L81-kakattoru", "かかっとる", "investigated, abstained"),
    (
        "L83-shite-mawattoru",
        "して回っとる",
        "investigated, abstained",
    ),
    ("L90-nattoru", "なっとる", "investigated, abstained"),
    (
        "yomitoranakya",
        "本当の所を読みとらなきゃ。",
        "answered whole",
    ),
    ("yattoru", "こ、こら！ナニやっとるかぁぁ！！", "unaffected"),
    (
        "nagekata",
        "その少年はカーブの投げ方を知っている。",
        "unaffected",
    ),
    ("kimochi", "気持ちが伝わってきます。", "unaffected"),
];

#[derive(Clone, Copy)]
struct Config {
    name: &'static str,
    policy: TiePolicy,
    repairs: bool,
}

const CONFIGS: [Config; 4] = [
    Config {
        name: "baseline",
        policy: TiePolicy::BareWord,
        repairs: false,
    },
    Config {
        name: "repairs-only",
        policy: TiePolicy::BareWord,
        repairs: true,
    },
    Config {
        name: "tie-break-only",
        policy: TiePolicy::Contextual,
        repairs: false,
    },
    Config {
        name: "shipped",
        policy: TiePolicy::Contextual,
        repairs: true,
    },
];

/// Ichiran's captured answer for one passage, converted under a chosen tie-break.
struct Replayed<'a> {
    raw: &'a str,
    policy: TiePolicy,
}

impl Analyzer for Replayed<'_> {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.analyze_traced(text).map(|traced| traced.analysis)
    }
    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        let segments = raw::parse(self.raw)
            .map_err(|error| AnalyzeError::BadOutput(format!("unexpected JSON: {error}")))?;
        Ok(Traced {
            analysis: ichiran::to_analysis_under(text, &segments, self.policy),
            raw: Some(self.raw.to_string()),
        })
    }
    fn engine(&self) -> &str {
        "ichiran"
    }
}

/// The sidecar, counting how often it is actually asked — how a cache hit is verified.
struct Counted {
    inner: IchiranAnalyzer,
    asked: Arc<AtomicUsize>,
}

impl Analyzer for Counted {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.inner.analyze(text)
    }
    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.inner.analyze_traced(text)
    }
    fn engine(&self) -> &str {
        self.inner.engine()
    }
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = samples.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 1 {
        samples[n / 2]
    } else {
        (samples[n / 2 - 1] + samples[n / 2]) / 2.0
    }
}

/// The plan the shell builds for an Ichiran primary, with no fallback: a measured passage that
/// silently retried on another engine would be timing something else.
fn plan_for(primary: &dyn Analyzer) -> Plan<'_> {
    Plan {
        requested: AnalyzerPolicy::Automatic,
        primary,
        fallback: None,
        standing_in: None,
    }
}

fn options(config: Config) -> AnalysisOptions {
    if config.repairs {
        AnalysisOptions::PRODUCTION
    } else {
        AnalysisOptions::PRODUCTION.without_repairs()
    }
}

#[test]
#[ignore = "a release measurement; needs Ichiran, the dictionaries and the kanji table"]
fn what_the_ichiran_path_costs_by_named_passage() {
    require_release();

    // ---- resources, exactly as `study_latency.rs` borrows them.
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
        "no dictionaries at {}",
        dictionaries_dir().display()
    );
    let kanji_path = dictionaries_dir()
        .parent()
        .expect("the dictionaries directory has a parent")
        .join("kanji")
        .join("kanjidic.tsv");
    let kanji = KanjiDic::load(&kanji_path);
    assert!(!kanji.is_empty(), "no KanjiDic at {}", kanji_path.display());
    let scratch = common::Scratch::new("ichiran-latency", "store");
    let store = Store::open(&scratch).expect("open");
    for n in 0..20_000 {
        store
            .set_status(
                WordKey::new(format!("語{n}"), format!("ご{n}")),
                WordStatus::Known,
            )
            .expect("a mark must succeed");
    }
    let lindera = LinderaAnalyzer::new().expect("compiled in");

    // ---- warm-up, untimed: the session, the embedded parser, the dictionaries, the kanji table.
    server::set_persistent(true);
    let warm = IchiranAnalyzer::new();
    warm.warm();
    warm.analyze("これって").expect("Ichiran must answer");
    let _ = study::study(
        "水を飲む",
        Resources {
            analyzer: &lindera,
            dictionaries: &dictionaries,
            kanji: &kanji,
            store: Some(&store),
        },
    );

    println!("\n=== the Ichiran reading path, by named passage ===");
    println!("  executable   {}", executable_digest());
    for dictionary in &dictionaries {
        println!(
            "  dictionary   {} ({})",
            dictionary.meta().title,
            dictionary.content_fingerprint().unwrap_or_default()
        );
    }
    println!(
        "  kanjidic     {}",
        sakatsu_core::setup::resources::sha256_file(&kanji_path).unwrap_or_default()
    );
    println!("  store        20,000 marked words");
    println!("  all figures are medians in milliseconds; n is per passage");

    // ---- 1. the sidecar alone: session, then one-shot. The session's first answer is kept.
    let mut raws: Vec<String> = Vec::new();
    let mut session_ms: Vec<f64> = Vec::new();
    let mut one_shot_ms: Vec<f64> = Vec::new();
    let sidecar = IchiranAnalyzer::new();
    for (id, text, _) in CASES {
        let first = sidecar.analyze_traced(text).expect("session answer");
        assert_eq!(
            server::last_route(),
            server::Route::Session,
            "{id}: session route"
        );
        raws.push(first.raw.expect("raw output"));
        let samples: Vec<f64> = (0..5)
            .map(|_| {
                let started = Instant::now();
                sidecar.analyze_traced(text).expect("session answer");
                millis(started.elapsed())
            })
            .collect();
        session_ms.push(median(samples));
    }
    server::set_persistent(false);
    for (id, text, _) in CASES {
        let samples: Vec<f64> = (0..3)
            .map(|_| {
                let started = Instant::now();
                sidecar.analyze_traced(text).expect("one-shot answer");
                let took = millis(started.elapsed());
                assert_eq!(
                    server::last_route(),
                    server::Route::NotPersistent,
                    "{id}: one-shot route"
                );
                took
            })
            .collect();
        one_shot_ms.push(median(samples));
    }
    server::set_persistent(true);
    sidecar.analyze("これって").expect("the session again");

    // ---- 2. the complete StudyView over the captured answer, paired per passage.
    const ROUNDS: usize = 21;
    let mut categories: Vec<String> = Vec::new();
    let mut per_config: Vec<Vec<Vec<f64>>> = Vec::new(); // case → config → samples
    for (index, (_, text, _)) in CASES.iter().enumerate() {
        let raw = &raws[index];
        // Category from the shipped configuration's own account — untimed.
        let shipped = study::run(
            text,
            &Replayed {
                raw,
                policy: TiePolicy::SHIPPED,
            },
            &dictionaries,
            AnalysisOptions::PRODUCTION,
        )
        .unwrap_or_else(|failure| panic!("{text}: {:?}", failure.error));
        categories.push(if !shipped.repairs.notes().is_empty() {
            "repaired".to_string()
        } else if !shipped.repairs.investigations.is_empty() {
            "investigated, abstained".to_string()
        } else if shipped.repairs.answered > 0 {
            "answered whole".to_string()
        } else {
            "unaffected".to_string()
        });

        let mut samples = vec![Vec::with_capacity(ROUNDS); CONFIGS.len()];
        for round in 0..ROUNDS {
            for offset in 0..CONFIGS.len() {
                let at = (offset + round) % CONFIGS.len();
                let config = CONFIGS[at];
                let replayed = Replayed {
                    raw,
                    policy: config.policy,
                };
                let started = Instant::now();
                let run = study::run(text, &replayed, &dictionaries, options(config))
                    .unwrap_or_else(|failure| panic!("{text}: {:?}", failure.error));
                let view = study::from_analysis(
                    run.analysis,
                    Resources {
                        analyzer: &replayed,
                        dictionaries: &dictionaries,
                        kanji: &kanji,
                        store: Some(&store),
                    },
                );
                let took = millis(started.elapsed());
                std::hint::black_box(&view);
                // The first round is discarded: warm-up for this passage's dictionary pages.
                if round > 0 {
                    samples[at].push(took);
                }
            }
        }
        per_config.push(samples);
    }

    // ---- 3 and 4. end to end in the production shape, and queue versus execution.
    let mut miss_ms = Vec::new();
    let mut hit_ms = Vec::new();
    let mut queue_idle_ms = Vec::new();
    let mut hit_verified = Vec::new();
    for (_, text, _) in CASES {
        let mut misses = Vec::new();
        let mut hits = Vec::new();
        let mut idle = Vec::new();
        let mut verified = true;
        for _ in 0..5 {
            // A fresh cache, so this is a miss: the sidecar is asked.
            let asked = Arc::new(AtomicUsize::new(0));
            let primary = Remembering::new(Counted {
                inner: IchiranAnalyzer::new(),
                asked: asked.clone(),
            });
            let plan = plan_for(&primary);

            let queued = Instant::now();
            let work = scheduler()
                .start(Some(Lane::Ichiran), None)
                .expect("the lane");
            idle.push(millis(queued.elapsed()));
            let started = Instant::now();
            let view =
                study::study_resilient(text, &plan, &dictionaries, &kanji, Some(&store), &|| {
                    work.checkpoint().is_ok()
                })
                .expect("a view");
            misses.push(millis(started.elapsed()));
            std::hint::black_box(&view);
            drop(work);
            let asked_after_miss = asked.load(Ordering::SeqCst);

            // The same passage again, through the same cache: a hit.
            let work = scheduler()
                .start(Some(Lane::Ichiran), None)
                .expect("the lane");
            let started = Instant::now();
            let view =
                study::study_resilient(text, &plan, &dictionaries, &kanji, Some(&store), &|| {
                    work.checkpoint().is_ok()
                })
                .expect("a view");
            hits.push(millis(started.elapsed()));
            std::hint::black_box(&view);
            drop(work);
            verified &= asked_after_miss >= 1 && asked.load(Ordering::SeqCst) == asked_after_miss;
        }
        miss_ms.push(median(misses));
        hit_ms.push(median(hits));
        queue_idle_ms.push(median(idle));
        hit_verified.push(verified);
    }

    // Queue under contention: another passage holds the lane while this one waits.
    let mut contended_wait = Vec::new();
    let mut contended_execution = Vec::new();
    for round in 0..5 {
        let (held, holding) = std::sync::mpsc::channel();
        let holder_text = CASES[round % CASES.len()].1.to_string();
        let dictionaries_ref = &dictionaries;
        let kanji_ref = &kanji;
        let store_ref = &store;
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let primary = Remembering::new(IchiranAnalyzer::new());
                let plan = plan_for(&primary);
                let work = scheduler()
                    .start(Some(Lane::Ichiran), None)
                    .expect("the lane");
                held.send(()).expect("signal");
                let view = study::study_resilient(
                    &holder_text,
                    &plan,
                    dictionaries_ref,
                    kanji_ref,
                    Some(store_ref),
                    &|| work.checkpoint().is_ok(),
                )
                .expect("a view");
                std::hint::black_box(&view);
                drop(work);
            });
            holding.recv().expect("the holder has the lane");
            let text = CASES[(round + 1) % CASES.len()].1;
            let primary = Remembering::new(IchiranAnalyzer::new());
            let plan = plan_for(&primary);
            let queued = Instant::now();
            let work = scheduler()
                .start(Some(Lane::Ichiran), None)
                .expect("the lane");
            contended_wait.push(millis(queued.elapsed()));
            let started = Instant::now();
            let view =
                study::study_resilient(text, &plan, &dictionaries, &kanji, Some(&store), &|| {
                    work.checkpoint().is_ok()
                })
                .expect("a view");
            contended_execution.push(millis(started.elapsed()));
            std::hint::black_box(&view);
            drop(work);
        });
    }

    // ---- report
    println!(
        "\n  {:<22} {:<24} {:>9} {:>9} │ {:>8} {:>8} {:>8} {:>8} │ {:>8} {:>8} {:>8} │ {:>8} {:>8} {:>6} {:>7}",
        "passage", "category (expected)", "session", "one-shot",
        "base", "repairs", "tie", "shipped",
        "Δrepair", "Δtie", "Δboth",
        "miss", "hit", "hit ok", "queue"
    );
    for (index, (id, _, expected)) in CASES.iter().enumerate() {
        let samples = &per_config[index];
        let med: Vec<f64> = samples.iter().map(|s| median(s.clone())).collect();
        let paired = |to: usize, from: usize| {
            median(
                samples[to]
                    .iter()
                    .zip(&samples[from])
                    .map(|(after, before)| after - before)
                    .collect(),
            )
        };
        let category = &categories[index];
        let shown = if category == expected {
            category.clone()
        } else {
            format!("{category} (EXPECTED {expected})")
        };
        println!(
            "  {:<22} {:<24} {:>9.1} {:>9.1} │ {:>8.2} {:>8.2} {:>8.2} {:>8.2} │ {:>+8.3} {:>+8.3} {:>+8.3} │ {:>8.1} {:>8.2} {:>6} {:>7.3}",
            id,
            shown,
            session_ms[index],
            one_shot_ms[index],
            med[0],
            med[1],
            med[2],
            med[3],
            paired(3, 2),
            paired(3, 1),
            paired(3, 0),
            miss_ms[index],
            hit_ms[index],
            if hit_verified[index] { "yes" } else { "NO" },
            queue_idle_ms[index],
        );
    }
    println!("\n  columns: session / one-shot = the sidecar alone, per route (n = 5 / 3)");
    println!("           base … shipped = the complete StudyView over the captured answer, per configuration (n = {})", ROUNDS - 1);
    println!("           Δrepair = shipped − tie-break-only · Δtie = shipped − repairs-only · Δboth = shipped − baseline (paired per round)");
    println!("           miss / hit = scheduler lane + study_resilient with Remembering(Ichiran), fresh cache / same cache (n = 5); hit ok = the sidecar was not asked again");
    println!("           queue = the wait for an idle lane (n = 5)");
    println!(
        "           configurations: {}",
        CONFIGS
            .iter()
            .map(|config| config.name)
            .collect::<Vec<_>>()
            .join(" · ")
    );
    println!(
        "\n  queue under contention (another passage holds the lane), n = 5: wait {:.1} ms · execution after the wait {:.1} ms",
        median(contended_wait),
        median(contended_execution)
    );

    assert!(
        hit_verified.iter().all(|ok| *ok),
        "a cache hit asked the sidecar again"
    );
}

fn executable_digest() -> String {
    use sha2::{Digest, Sha256};
    std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| sakatsu_core::hashing::hex(Sha256::digest(&bytes))[..16].to_string())
        .unwrap_or_else(|| "unknown".into())
}
