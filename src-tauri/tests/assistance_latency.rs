//! **What shared assistance costs a reader, by mode — and what one embedded parser costs a search.**
//!
//! ```text
//! cargo test --release -p sakatsu-desktop --test assistance_latency -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The shared-assistance decision is taken from this file's numbers, in
//! order: Legacy → SharedLazy (with the built-in analyzer unified with the advisor), then
//! SharedLazy → SharedEager. Each transition must keep, on every row:
//!
//! | | added median | added p95 |
//! |---|---|---|
//! | warm miss | ≤ 5 ms | ≤ 10 ms |
//! | verified hit | ≤ 1 ms | ≤ 2 ms |
//! | contended search or passage (unification) | ≤ 5 ms | ≤ 10 ms |
//!
//! # What is timed
//!
//! - **Warm misses**: a fresh `Remembering` over Ichiran's **captured** answer, through
//!   `study::analyse_resilient` and `study::from_analysis` — the shell's success path — so the
//!   sidecar's variance cannot drown a few milliseconds. Paired per round, modes rotated.
//! - **Verified hits**: the same `Remembering` asked again, with the inner analyzer counted so a
//!   hit is a hit.
//! - **A live-session sample** of misses, for realism, medians only.
//! - **Contention**: the built-in analyzer's part of a Reference search (`analyze` on one word)
//!   on one thread while passages are read on another — two instances under Legacy against one
//!   unified instance under SharedLazy.
//!
//! Wall time and the thread's CPU time are both reported. Warm-up is untimed, nothing is scored
//! inside a timed region, and release only. The passages are committed fixture sentences (Tatoeba
//! via Jitendex) and the contraction words already in `ichiran_latency.rs`; the long and multi-line
//! cases are those sentences joined, not new text.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sakatsu_core::analyzer::assist::{Assistance, AssistanceMode, Stage};
use sakatsu_core::analyzer::ichiran::{self, raw, server, TiePolicy};
use sakatsu_core::analyzer::lindera::{self, LinderaAnalyzer};
use sakatsu_core::analyzer::{AnalyzeError, Analyzer, IchiranAnalyzer, Remembering, Traced};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::dict::KanjiDic;
use sakatsu_core::model::{Analysis, AnalyzerPolicy};
use sakatsu_core::store::{Store, WordKey, WordStatus};
use sakatsu_core::study::{self, AnalysisOptions, Plan, Resources};
use sakatsu_desktop_lib::memory::thread_cpu_time;

const MODES: [AssistanceMode; 3] = [
    AssistanceMode::Legacy,
    AssistanceMode::SharedLazy,
    AssistanceMode::SharedEager,
];

/// The verified sentences the long and multi-line cases are built from.
const SENTENCES: &[&str] = &[
    "その少年はカーブの投げ方を知っている。",
    "前学期は成績がよかった。",
    "彼女は寒気を防ぐために厚いコートを着ていた。",
    "彼には時間がなかったので、家では何もしなかった",
    "輝くものすべてが金とは限らない。",
    "大口叩きは何もできない。",
    "本当の所を読みとらなきゃ。",
    "頭文字から間違っとるわい！！",
];

fn cases() -> Vec<(&'static str, String)> {
    let mut cases: Vec<(&'static str, String)> = [
        ("machigattoru (repaired)", "頭文字から間違っとるわい！！"),
        ("okonnakya (repaired)", "怒んなきゃ"),
        ("L81-kakattoru (abstained)", "かかっとる"),
        ("L83-shite-mawattoru (abstained)", "して回っとる"),
        ("yomitoranakya (answered)", "本当の所を読みとらなきゃ。"),
        ("kimochi (unaffected)", "気持ちが伝わってきます。"),
        ("nagekata (tie)", "その少年はカーブの投げ方を知っている。"),
        ("zengakki (tie)", "前学期は成績がよかった。"),
        (
            "kanki (two ties)",
            "彼女は寒気を防ぐために厚いコートを着ていた。",
        ),
    ]
    .into_iter()
    .map(|(id, text)| (id, text.to_string()))
    .collect();
    cases.push(("long (8 sentences)", SENTENCES.concat()));
    cases.push(("multi-line (8 lines)", SENTENCES.join("\n")));
    cases
}

/// The single words a Reference search asks the built-in analyzer about.
const SEARCHES: &[&str] = &[
    "気持ち",
    "成績",
    "投げ方",
    "寒気",
    "金",
    "大口",
    "前学期",
    "少年",
];

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

/// Ichiran's captured answer, converted inside the request as the adapter converts a live one.
struct Replayed {
    raw: String,
    asked: Arc<AtomicUsize>,
}

impl Analyzer for Replayed {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.analyze_traced(text).map(|traced| traced.analysis)
    }
    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        self.analyze_in(text, &Assistance::new(AssistanceMode::Legacy))
    }
    fn analyze_in(&self, text: &str, assistance: &Assistance<'_>) -> Result<Traced, AnalyzeError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        assistance.refuse_if_advising()?;
        let text = sakatsu_core::analyzer::lines::normalise_breaks(text);
        let text = text.trim();
        let segments = raw::parse(&self.raw)
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

fn plan_for(primary: &dyn Analyzer) -> Plan<'_> {
    Plan {
        requested: AnalyzerPolicy::Automatic,
        primary,
        fallback: None,
        standing_in: None,
    }
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn quantile(mut samples: Vec<f64>, q: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if samples.is_empty() {
        return f64::NAN;
    }
    // Nearest rank: a p95 of twenty samples is the nineteenth, not an interpolation.
    let rank = ((q * samples.len() as f64).ceil() as usize).clamp(1, samples.len());
    samples[rank - 1]
}

fn median(samples: Vec<f64>) -> f64 {
    let mut samples = samples;
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

/// Wall and thread CPU of one call.
fn timed<T>(work: impl FnOnce() -> T) -> (T, f64, f64) {
    let cpu = thread_cpu_time().expect("the platform reports thread CPU time");
    let started = Instant::now();
    let out = work();
    let wall = millis(started.elapsed());
    let spent = millis(thread_cpu_time().expect("thread CPU time") - cpu);
    (out, wall, spent)
}

struct Resources3<'a> {
    dictionaries: &'a [Dictionary],
    kanji: &'a KanjiDic,
    store: &'a Store,
}

/// One request the way the shell serves it: the resilient path, then the view.
fn serve(text: &str, primary: &dyn Analyzer, mode: AssistanceMode, with: &Resources3<'_>) {
    let done = study::analyse_resilient(
        text,
        &plan_for(primary),
        with.dictionaries,
        AnalysisOptions::PRODUCTION.with_assistance(mode),
        &|| true,
    )
    .unwrap_or_else(|failure| panic!("{text}: {failure}"));
    let view = study::from_analysis(
        done.analysis,
        Resources {
            analyzer: primary,
            dictionaries: with.dictionaries,
            kanji: with.kanji,
            store: Some(with.store),
        },
    );
    std::hint::black_box(&view);
}

/// Paired differences between two modes' samples, round by round.
fn paired(to: &[f64], from: &[f64]) -> Vec<f64> {
    to.iter()
        .zip(from)
        .map(|(after, before)| after - before)
        .collect()
}

fn verdict(median_ms: f64, p95_ms: f64, median_budget: f64, p95_budget: f64) -> &'static str {
    if median_ms <= median_budget && p95_ms <= p95_budget {
        "within"
    } else {
        "OVER"
    }
}

#[test]
#[ignore = "a release measurement; needs Ichiran, the dictionaries and the kanji table"]
fn what_shared_assistance_costs_by_mode() {
    require_release();

    let dictionaries: Vec<Dictionary> = Dictionary::discover(&dictionaries_dir())
        .into_iter()
        .map(|path| Dictionary::open(&path).expect("a required dictionary opens"))
        .collect();
    assert!(!dictionaries.is_empty(), "no dictionaries");
    let kanji_path = dictionaries_dir()
        .parent()
        .expect("parent")
        .join("kanji")
        .join("kanjidic.tsv");
    let kanji = KanjiDic::load(&kanji_path);
    assert!(!kanji.is_empty(), "no KanjiDic at {}", kanji_path.display());
    let scratch = common::Scratch::new("assistance-latency", "store");
    let store = Store::open(&scratch).expect("open");
    for n in 0..20_000 {
        store
            .set_status(
                WordKey::new(format!("語{n}"), format!("ご{n}")),
                WordStatus::Known,
            )
            .expect("mark");
    }
    let with = Resources3 {
        dictionaries: &dictionaries,
        kanji: &kanji,
        store: &store,
    };

    // ---- untimed: the session, the shared parser, and each passage's captured answer.
    server::set_persistent(true);
    let sidecar = IchiranAnalyzer::new();
    sidecar.warm();
    sidecar.analyze("これって").expect("Ichiran must answer");
    assert!(lindera::shared().is_some(), "the shared parser loads");
    let cases = cases();
    let raws: Vec<String> = cases
        .iter()
        .map(|(id, text)| {
            sidecar
                .analyze_traced(text)
                .unwrap_or_else(|error| panic!("{id}: {error}"))
                .raw
                .expect("raw")
        })
        .collect();

    println!("\n=== shared assistance, by mode ===");
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
    println!("  store        20,000 marked words · all figures in milliseconds");

    // ---- questions asked, per mode (untimed).
    println!(
        "\n  {:<32} {:>26} {:>26} {:>26}",
        "questions: parser executed/asked · dictionary read/asked",
        "legacy",
        "shared-lazy",
        "shared-eager"
    );
    for (index, (id, text)) in cases.iter().enumerate() {
        let mut cells = Vec::new();
        for mode in MODES {
            let replayed = Replayed {
                raw: raws[index].clone(),
                asked: Arc::new(AtomicUsize::new(0)),
            };
            let run = study::analyse_resilient(
                text,
                &plan_for(&replayed),
                &dictionaries,
                AnalysisOptions::PRODUCTION.with_assistance(mode),
                &|| true,
            )
            .expect("answered");
            let advice = run.assistance.advice;
            let asked: usize = Stage::ALL
                .iter()
                .map(|stage| advice.stage(*stage).requested)
                .sum();
            cells.push(format!(
                "{}/{} · {}/{}",
                advice.executed(),
                asked,
                run.assistance.evidence.executed(),
                run.assistance.evidence.requested()
            ));
        }
        println!(
            "  {:<32} {:>26} {:>26} {:>26}",
            id, cells[0], cells[1], cells[2]
        );
    }

    // ---- warm misses and verified hits, paired per round with the modes rotated.
    const ROUNDS: usize = 31;
    let mut miss_wall: Vec<[Vec<f64>; 3]> = Vec::new();
    let mut miss_cpu: Vec<[Vec<f64>; 3]> = Vec::new();
    let mut hit_wall: Vec<[Vec<f64>; 3]> = Vec::new();
    let mut hits_verified = true;
    for (index, (_, text)) in cases.iter().enumerate() {
        let mut wall: [Vec<f64>; 3] = Default::default();
        let mut cpu: [Vec<f64>; 3] = Default::default();
        let mut hits: [Vec<f64>; 3] = Default::default();
        // One cache per mode for the hits, primed untimed.
        let counters: Vec<Arc<AtomicUsize>> =
            (0..3).map(|_| Arc::new(AtomicUsize::new(0))).collect();
        let kept: Vec<Remembering<Replayed>> = counters
            .iter()
            .map(|asked| {
                Remembering::new(Replayed {
                    raw: raws[index].clone(),
                    asked: asked.clone(),
                })
            })
            .collect();
        for (slot, mode) in MODES.into_iter().enumerate() {
            serve(text, &kept[slot], mode, &with);
        }
        let primed: Vec<usize> = counters.iter().map(|c| c.load(Ordering::SeqCst)).collect();

        for round in 0..ROUNDS {
            for offset in 0..MODES.len() {
                let slot = (offset + round) % MODES.len();
                let mode = MODES[slot];
                let fresh = Remembering::new(Replayed {
                    raw: raws[index].clone(),
                    asked: Arc::new(AtomicUsize::new(0)),
                });
                let ((), w, c) = timed(|| serve(text, &fresh, mode, &with));
                let ((), h, _) = timed(|| serve(text, &kept[slot], mode, &with));
                if round > 0 {
                    wall[slot].push(w);
                    cpu[slot].push(c);
                    hits[slot].push(h);
                }
            }
        }
        for (slot, counter) in counters.iter().enumerate() {
            hits_verified &= primed[slot] == 1 && counter.load(Ordering::SeqCst) == 1;
        }
        miss_wall.push(wall);
        miss_cpu.push(cpu);
        hit_wall.push(hits);
    }

    println!(
        "\n  {:<32} {:>8} {:>8} {:>8} │ {:>15} {:>15} │ {:>8} {:>8} │ {:>15} {:>15}",
        "warm miss (n = 30 per mode)",
        "legacy",
        "lazy",
        "eager",
        "Δlazy med/p95",
        "Δeager med/p95",
        "cpu leg",
        "cpu lazy",
        "hit Δlazy",
        "hit Δeager"
    );
    let mut pooled: [Vec<f64>; 4] = Default::default(); // miss Δlazy, miss Δeager, hit Δlazy, hit Δeager
    for (index, (id, _)) in cases.iter().enumerate() {
        let wall = &miss_wall[index];
        let cpu = &miss_cpu[index];
        let hits = &hit_wall[index];
        let lazy = paired(&wall[1], &wall[0]);
        let eager = paired(&wall[2], &wall[1]);
        let hit_lazy = paired(&hits[1], &hits[0]);
        let hit_eager = paired(&hits[2], &hits[1]);
        println!(
            "  {:<32} {:>8.2} {:>8.2} {:>8.2} │ {:>+7.2}/{:>+7.2} {:>+7.2}/{:>+7.2} │ {:>8.2} {:>8.2} │ {:>+7.3}/{:>+7.3} {:>+7.3}/{:>+7.3}",
            id,
            median(wall[0].clone()),
            median(wall[1].clone()),
            median(wall[2].clone()),
            median(lazy.clone()),
            quantile(lazy.clone(), 0.95),
            median(eager.clone()),
            quantile(eager.clone(), 0.95),
            median(cpu[0].clone()),
            median(cpu[1].clone()),
            median(hit_lazy.clone()),
            quantile(hit_lazy.clone(), 0.95),
            median(hit_eager.clone()),
            quantile(hit_eager.clone(), 0.95),
        );
        pooled[0].extend(lazy);
        pooled[1].extend(eager);
        pooled[2].extend(hit_lazy);
        pooled[3].extend(hit_eager);
    }
    let summary = |name: &str, samples: &[f64], median_budget: f64, p95_budget: f64| {
        let (m, p) = (median(samples.to_vec()), quantile(samples.to_vec(), 0.95));
        println!(
            "  {:<44} median {:>+7.3}  p95 {:>+7.3}  budget {}/{}  {}",
            name,
            m,
            p,
            median_budget,
            p95_budget,
            verdict(m, p, median_budget, p95_budget)
        );
    };
    println!("\n  pooled paired differences, every passage and round:");
    summary("warm miss  legacy → shared-lazy", &pooled[0], 5.0, 10.0);
    summary(
        "warm miss  shared-lazy → shared-eager",
        &pooled[1],
        5.0,
        10.0,
    );
    summary("verified hit  legacy → shared-lazy", &pooled[2], 1.0, 2.0);
    summary(
        "verified hit  shared-lazy → shared-eager",
        &pooled[3],
        1.0,
        2.0,
    );
    println!(
        "  hits verified (the inner analyzer asked once, at priming): {}",
        if hits_verified { "yes" } else { "NO" }
    );

    // ---- live-session misses, for realism.
    println!("\n  live session, fresh cache each sample (n = 5 per mode), medians:");
    for (id, text) in &cases {
        let mut samples: [Vec<f64>; 3] = Default::default();
        for round in 0..5 {
            for offset in 0..MODES.len() {
                let slot = (offset + round) % MODES.len();
                let primary = Remembering::new(IchiranAnalyzer::new());
                let ((), w, _) = timed(|| serve(text, &primary, MODES[slot], &with));
                samples[slot].push(w);
            }
        }
        println!(
            "  {:<32} {:>8.1} {:>8.1} {:>8.1}",
            id,
            median(samples[0].clone()),
            median(samples[1].clone()),
            median(samples[2].clone())
        );
    }

    // ---- contention: searches on one thread while passages are read on another.
    let own = LinderaAnalyzer::new().expect("a second instance, as the shell builds today");
    let unified = lindera::shared_analyzer().expect("the shared parser");
    let long_raws: Vec<(String, String)> = cases
        .iter()
        .zip(&raws)
        .filter(|((id, _), _)| {
            id.contains("tie") || id.starts_with("long") || id.starts_with("multi")
        })
        .map(|((_, text), raw)| (text.clone(), raw.clone()))
        .collect();
    let mut arms: [(Vec<f64>, Vec<f64>); 2] = Default::default(); // (search, passage) per arm
    for block in 0..6 {
        let arm = block % 2;
        let (searcher, mode): (&dyn Analyzer, AssistanceMode) = if arm == 0 {
            (&own, AssistanceMode::Legacy)
        } else {
            (unified.as_ref(), AssistanceMode::SharedLazy)
        };
        let done = std::sync::atomic::AtomicBool::new(false);
        let mut searches = Vec::new();
        let mut passages = Vec::new();
        std::thread::scope(|scope| {
            let reader = scope.spawn(|| {
                let mut took = Vec::new();
                for round in 0..4 {
                    for (text, raw) in &long_raws {
                        let primary = Remembering::new(Replayed {
                            raw: raw.clone(),
                            asked: Arc::new(AtomicUsize::new(0)),
                        });
                        let started = Instant::now();
                        serve(text, &primary, mode, &with);
                        if round > 0 {
                            took.push(millis(started.elapsed()));
                        }
                    }
                }
                done.store(true, Ordering::SeqCst);
                took
            });
            let mut at = 0;
            while !done.load(Ordering::SeqCst) {
                let word = SEARCHES[at % SEARCHES.len()];
                at += 1;
                let started = Instant::now();
                std::hint::black_box(searcher.analyze(word).expect("a search reads"));
                searches.push(millis(started.elapsed()));
            }
            passages = reader.join().expect("the reader finishes");
        });
        arms[arm].0.extend(searches);
        arms[arm].1.extend(passages);
    }
    println!("\n  contention: the built-in's part of a search (analyze one word) while passages are read");
    println!(
        "  {:<34} {:>10} {:>10} {:>8} │ {:>10} {:>10} {:>8}",
        "", "search med", "search p95", "n", "passage med", "p95", "n"
    );
    for (name, (searches, passages)) in [
        ("two instances, legacy", &arms[0]),
        ("one instance, shared-lazy", &arms[1]),
    ] {
        println!(
            "  {:<34} {:>10.3} {:>10.3} {:>8} │ {:>10.2} {:>10.2} {:>8}",
            name,
            median(searches.clone()),
            quantile(searches.clone(), 0.95),
            searches.len(),
            median(passages.clone()),
            quantile(passages.clone(), 0.95),
            passages.len()
        );
    }
    let added = |index: usize, q: f64| {
        let pick = |samples: &Vec<f64>| {
            if q == 0.5 {
                median(samples.clone())
            } else {
                quantile(samples.clone(), q)
            }
        };
        let (a, b) = if index == 0 {
            (&arms[0].0, &arms[1].0)
        } else {
            (&arms[0].1, &arms[1].1)
        };
        pick(b) - pick(a)
    };
    for (index, name) in [
        (0, "contended search  two → one instance"),
        (1, "contended passage two → one instance"),
    ] {
        let (m, p) = (added(index, 0.5), added(index, 0.95));
        println!(
            "  {:<44} median {:>+7.3}  p95 {:>+7.3}  budget 5/10  {}",
            name,
            m,
            p,
            verdict(m, p, 5.0, 10.0)
        );
    }
    println!(
        "\n  the contention rows compare unpaired distributions (an arm per block, alternated), so\n  \
         they are differences of medians and of p95s, not paired differences."
    );

    assert!(hits_verified, "a cache hit asked the inner analyzer again");
}

fn executable_digest() -> String {
    use sha2::{Digest, Sha256};
    std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| sakatsu_core::hashing::hex(Sha256::digest(&bytes))[..16].to_string())
        .unwrap_or_else(|| "unknown".into())
}
