//! What the reading path costs, measured the same way every time.
//!
//! ```text
//! cargo test --release -p sakatsu-core --test performance -- --ignored --nocapture
//! ```
//!
//! # Why this exists
//!
//! The only latency figures this project had were 0.061 ms for the analyzer and 0.558 ms for the
//! displayed pipeline, taken over eight short sentences with one engine, in a scratch file that
//! was deleted afterwards. They were then nearly quoted as end-to-end cost. A number that cannot
//! be reproduced is not a measurement, and a number taken on short sentences says nothing about a
//! page of prose.
//!
//! So: fixed inputs, a warmup, medians **and** p95, and short passages separated from long ones.
//!
//! # What each row is, said exactly
//!
//! **"Analysis through reading correction"** is `pipeline::Pipeline::display` — the stages that
//! rewrite the token list. It is the service boundary, and it is *not* what a reader waits for.
//! **"+ grammar, keys, furigana"** adds the derivations that need nothing but the finished list.
//!
//! Neither is the study command. That row was once labelled "through the study view" and was not:
//! it omitted the store lookup that colours each word, and the construction and serialisation of
//! the response. Both live in the shell, so the end-to-end figure does too —
//! `src-tauri/tests/study_latency.rs`. Naming a partial row for the whole is how 0.558 ms came to
//! stand for end-to-end cost in the first place, and it is the same mistake twice.
//!
//! # Errors fail a case
//!
//! An earlier version of this file timed `analyzer.analyze(text)` without inspecting the `Result`
//! and counted `filter_map(|d| d.lookup(word).ok())`. A benchmark that times failures reports its
//! fastest numbers when everything is broken. Every workload here unwraps.
//!
//! # Release only, and it is enforced
//!
//! A debug build of the analyzer is roughly an order of magnitude slower and its ratios do not
//! hold. This used to be a comment asking nicely; it is now a check, so a debug figure cannot be
//! quoted by accident.
//!
//! # What this deliberately does not measure
//!
//! **Peak memory.** `GetProcessMemoryInfo` reports a **process** high-water mark, so reading it
//! around a call here would describe the test harness rather than the call. It belongs to the
//! desktop shell, which already links the Windows APIs, and is measured there in isolated runs.
//!
//! **The sidecar and the models.** Ichiran, OCR, translation and the VLM are separate processes or
//! separate engines with their own installation state. `ichiran_server.rs` already times the
//! sidecar (714 ms against 252 ms warm); this file is the in-process reading path, which is the
//! part every reader pays on every passage.

mod common;
#[path = "common/runner.rs"]
mod runner;

use std::hint::black_box;
use std::time::{Duration, Instant};

use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::dict::KanjiDic;
use sakatsu_core::model::Analysis;
use sakatsu_core::pipeline::Pipeline;
use sakatsu_core::store::WordKey;
use sha2::{Digest, Sha256};

/// Set in a child process that should measure exactly one cold start and exit.
const ONE_COLD_START: &str = "SAKATSU_ONE_COLD_START";

/// Ordinary sentences, of the length a reader meets one at a time in a manga panel.
const SHORT: &[&str] = &[
    "彼は毎日図書館で勉強している。",
    "その問題については明日話し合いましょう。",
    "天気が良ければ公園に行きます。",
    "父は昨日新しい車を買いました。",
    "ここって禁足地なんじゃなかったっけ。",
    "へえ今度アタシも連れてってよ。",
    "それは子供っぽくないですか。",
    "駅までは歩いて十分ほどかかります。",
];

/// **Synthetic**, and reported as such wherever the figure appears.
///
/// Built by joining the short set several times over rather than by inventing long text: the point
/// is the length of the passage handed to one call, not the vocabulary. A rate from these says
/// something about passage length and nothing about how a wider vocabulary behaves.
fn long_passages() -> Vec<String> {
    (0..4)
        .map(|offset| {
            let mut passage = String::new();
            for repeat in 0..6 {
                for (index, sentence) in SHORT.iter().enumerate() {
                    if (index + offset + repeat) % 3 != 0 {
                        passage.push_str(sentence);
                    }
                }
            }
            passage
        })
        .collect()
}

fn text_fixture_digest<'a>(texts: impl IntoIterator<Item = &'a str>) -> String {
    let mut hash = Sha256::new();
    for text in texts {
        hash.update((text.len() as u64).to_le_bytes());
        hash.update(text.as_bytes());
    }
    sakatsu_core::hashing::hex(hash.finalize())
}

fn print_identity(label: &str, fixture: &str) {
    println!("  identity        {label}");
    println!("  executable      {}", runner::executable_digest());
    println!("  source tree     {}", runner::source_digest());
    println!("  fixture sha256  {fixture}");
    println!("  configuration   release; warmup outside timed region");
}

/// Median and p95 of a sample, in milliseconds.
///
/// **p95 as well as the median, because the median is the number that flatters.** A pipeline that
/// is usually fast and occasionally not is exactly what a reader notices, and one summary statistic
/// cannot show both.
fn summarise(mut samples: Vec<Duration>) -> (f64, f64) {
    samples.sort_unstable();
    let millis = |duration: &Duration| duration.as_secs_f64() * 1000.0;
    let median = millis(&samples[samples.len() / 2]);
    // Nearest-rank p95, clamped so a small sample still names its slowest observed case.
    let rank = ((samples.len() as f64 * 0.95).ceil() as usize).min(samples.len()) - 1;
    (median, millis(&samples[rank]))
}

/// Time one closure over `inputs`, repeated, after a warmup that is thrown away.
fn measure<T>(inputs: &[String], repeats: usize, mut run: impl FnMut(&str) -> T) -> Vec<Duration> {
    for input in inputs {
        black_box(run(input));
    }
    let mut samples = Vec::with_capacity(inputs.len() * repeats);
    for _ in 0..repeats {
        for input in inputs {
            let started = Instant::now();
            black_box(run(input));
            samples.push(started.elapsed());
        }
    }
    samples
}

fn row(label: &str, samples: Vec<Duration>) {
    let count = samples.len();
    let (median, p95) = summarise(samples);
    println!("  {label:<46} {median:>8.3} {p95:>9.3}   {count:>5}");
}

/// Refuse to produce a number nobody should quote.
/// A `#[cfg]` rather than a runtime test of `cfg!`, so the release build carries no check at all
/// and the debug build cannot be talked out of one.
fn require_release() {
    #[cfg(debug_assertions)]
    panic!(
        "run this under --release. A debug build of the analyzer is roughly an order of magnitude \
         slower and its ratios do not hold, so a debug figure is not a slower version of the real \
         one — it is a different shape. This used to be a comment asking nicely."
    );
}

/// The in-process reading path, warm, short and long.
#[test]
#[ignore = "a measurement, and slow; needs the dictionaries for the pipeline rows"]
fn what_the_reading_path_costs() {
    require_release();

    let analyzer = LinderaAnalyzer::new().expect("compiled in");

    // Shared with the application: `Dictionary::discover` decides what counts and in what order,
    // and a dictionary that will not open fails the run rather than shrinking the corpus quietly.
    let dictionaries = common::installed_dictionaries();
    assert!(
        !dictionaries.is_empty(),
        "no dictionaries at {} — run scripts/fetch-dictionaries.ps1. The analyzer rows would \
         still be measurable, but a partial report invites exactly the mistake this file exists \
         to stop: quoting one stage as the cost of the whole.",
        common::dictionaries_dir().display()
    );
    let pipeline = Pipeline::new(&dictionaries);

    let short: Vec<String> = SHORT.iter().map(|text| (*text).to_string()).collect();
    let long = long_passages();
    let short_chars: usize =
        short.iter().map(|text| text.chars().count()).sum::<usize>() / short.len();
    let long_chars: usize =
        long.iter().map(|text| text.chars().count()).sum::<usize>() / long.len();

    println!("\n=== the in-process reading path ===");
    print_identity(
        "reading-path-v1",
        &text_fixture_digest(
            short
                .iter()
                .map(String::as_str)
                .chain(long.iter().map(String::as_str)),
        ),
    );
    println!(
        "  dictionaries: {}",
        dictionaries
            .iter()
            .map(|dictionary| dictionary.meta().title.clone())
            .collect::<Vec<_>>()
            .join(", ")
    );
    for dictionary in &dictionaries {
        println!(
            "  resource        dictionary {} {}",
            dictionary.meta().title,
            dictionary
                .content_fingerprint()
                .expect("the borrowed dictionary can be fingerprinted before timing")
        );
    }
    println!(
        "  short: {} natural passages averaging {short_chars} characters · long: {} SYNTHETIC \
         passages averaging {long_chars} (the short set concatenated — a statement about passage \
         length, not vocabulary)",
        short.len(),
        long.len()
    );
    println!("\n  {:<46} {:>8} {:>9}   {:>5}", "", "median", "p95", "n");

    // Analyzer alone: what segmentation and morphology cost, with nothing looked up.
    row(
        "analyzer, short",
        measure(&short, 40, |text| {
            analyzer.analyze(text).expect("the analyzer must succeed")
        }),
    );
    row(
        "analyzer, long",
        measure(&long, 40, |text| {
            analyzer.analyze(text).expect("the analyzer must succeed")
        }),
    );

    // The service boundary: the stages that rewrite the token list. Not "the pipeline" — that
    // label is what let one stage stand in for end-to-end cost.
    let displayed = |text: &str| -> Analysis {
        let mut analysis = analyzer.analyze(text).expect("the analyzer must succeed");
        pipeline.display(&mut analysis);
        analysis
    };
    row(
        "analysis through reading correction, short",
        measure(&short, 20, displayed),
    );
    row(
        "analysis through reading correction, long",
        measure(&long, 20, displayed),
    );

    // Everything `commands::study` builds on top of the finished token list. Ruby needs KanjiDic,
    // which is a separate resource: when it is absent the row is reported without it and says so,
    // rather than being quietly cheaper than the app.
    // `KanjiDic::load` is infallible and returns an empty table when the file is absent, so the
    // emptiness — not an error — is what says whether this row includes furigana.
    let kanji_path = common::dictionaries_dir()
        .parent()
        .expect("the dictionaries directory has a parent")
        .join("kanji")
        .join("kanjidic.tsv");
    let kanji = KanjiDic::load(&kanji_path);
    let kanji = (!kanji.is_empty()).then_some(kanji);
    if kanji.is_some() {
        println!(
            "  resource        kanjidic {}",
            sakatsu_core::setup::resources::sha256_file(&kanji_path)
                .expect("the borrowed kanji table can be fingerprinted before timing")
        );
    }
    let study = |text: &str| -> usize {
        let analysis = displayed(text);
        let breakdowns: Vec<_> = analysis
            .tokens
            .iter()
            .map(sakatsu_core::grammar::breakdown)
            .collect();
        let patterns = sakatsu_core::grammar::patterns(&analysis);
        let sentences = sakatsu_core::model::sentence_ranges(&analysis.tokens);
        let keys: Vec<WordKey> = analysis.tokens.iter().map(WordKey::for_token).collect();
        let ruby = kanji.as_ref().map(|kanji| {
            analysis
                .tokens
                .iter()
                .map(|token| sakatsu_core::dict::furigana(&token.surface, &token.reading, kanji))
                .collect::<Vec<_>>()
                .len()
        });
        breakdowns.len() + patterns.len() + sentences.len() + keys.len() + ruby.unwrap_or(0)
    };
    let ruby_note = if kanji.is_some() {
        "with furigana"
    } else {
        "NO furigana — KanjiDic absent"
    };
    // **Not "the study view".** This row was called that and was not: it stops before the store
    // lookup that colours each word and before the response is built and serialised. Those live
    // in the shell, so the end-to-end figure does too — `src-tauri/tests/study_latency.rs`.
    row(
        &format!("+ grammar, keys, furigana, short ({ruby_note})"),
        measure(&short, 20, study),
    );
    row(
        &format!("+ grammar, keys, furigana, long ({ruby_note})"),
        measure(&long, 20, study),
    );

    // Lookup on its own, over words the passages actually contain.
    let words: Vec<String> = analyzer
        .analyze(&long[0])
        .expect("the analyzer must succeed")
        .tokens
        .iter()
        .filter(|token| sakatsu_core::model::contains_japanese(&token.surface))
        .map(|token| token.surface.clone())
        .collect();
    row(
        "dictionary lookup, one word",
        measure(&words, 20, |word| {
            dictionaries
                .iter()
                .map(|dictionary| {
                    dictionary
                        .lookup(word)
                        .expect("a lookup must succeed")
                        .len()
                })
                .sum::<usize>()
        }),
    );

    println!("\n  Peak memory is measured in the desktop shell, in isolated runs: the Windows");
    println!("  counter is a process high-water mark, so reading it around a call in this");
    println!("  harness would describe the harness. See src-tauri/tests/peak_memory.rs.");
    println!("  The sidecar and the models are separate engines; `ichiran_server.rs` times the");
    println!("  sidecar, and the OCR and translation engines have their own live tests.");
}

/// One cold construction, in a process that has done nothing else.
///
/// Only useful as a child of [`cold_start_per_engine`]; on its own it measures one process's first
/// analyzer and prints it. Guarded by an environment variable so an ordinary `--ignored` run does
/// not report a "cold" start in a process that has already built three analyzers.
#[test]
#[ignore = "a child process of cold_start_per_engine"]
fn one_cold_start() {
    if std::env::var_os(ONE_COLD_START).is_none() {
        println!("skipped: this test is spawned by cold_start_per_engine, not run directly");
        return;
    }
    require_release();
    let started = Instant::now();
    let analyzer = LinderaAnalyzer::new().expect("compiled in");
    let constructed = started.elapsed();
    // Some builds load the embedded dictionary lazily, so pay for one analysis before calling the
    // construction cost complete.
    let first = Instant::now();
    black_box(
        analyzer
            .analyze(SHORT[0])
            .expect("the analyzer must succeed"),
    );
    println!(
        "COLD lindera {:.3} {:.3}",
        constructed.as_secs_f64() * 1000.0,
        first.elapsed().as_secs_f64() * 1000.0
    );
}

/// Cold initialisation, per engine, in fresh processes.
///
/// **A second cold start in the same process is not cold.** The old figure was one observation
/// taken alongside everything else in this file, which measured a process that had already paid
/// for the allocator, the loader and the OS file cache. So each observation gets its own process.
///
/// Each child has fresh process state. The OS file cache is neither controlled nor measured;
/// the first observation is reported separately, but must not be called a cold-cache sample.
///
/// Only Lindera is covered here. Ichiran is a WSL sidecar with its own installation state, and
/// `ichiran_server.rs` times it; adding a row for it that silently returns zero when WSL is absent
/// is the kind of partial report this file exists to prevent.
#[test]
#[ignore = "a measurement; spawns child processes"]
fn cold_start_per_engine() {
    require_release();
    println!("\n=== cold-start measurement identity ===");
    print_identity(
        "lindera-cold-start-v1",
        &text_fixture_digest(SHORT.iter().copied()),
    );
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let mut observations = Vec::new();
    for _ in 0..5 {
        let output = std::process::Command::new(&exe)
            .args(["one_cold_start", "--exact", "--ignored", "--nocapture"])
            .env(ONE_COLD_START, "1")
            .output()
            .expect("spawn a fresh process");
        assert!(
            output.status.success(),
            "a cold-start child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text
            .lines()
            .find(|line| line.starts_with("COLD lindera "))
            .unwrap_or_else(|| panic!("a child reported no cold start:\n{text}"));
        let mut fields = line.split_whitespace().skip(2);
        let construction: f64 = fields.next().expect("construction").parse().expect("ms");
        let first: f64 = fields.next().expect("first analysis").parse().expect("ms");
        observations.push((construction, first));
    }

    let (first_child, subsequent) = observations.split_first().expect("five observations");
    let median = |mut values: Vec<f64>| {
        values.sort_by(f64::total_cmp);
        values[values.len() / 2]
    };
    println!("\n=== cold initialisation, fresh process per observation ===");
    println!(
        "  lindera, first child: construction {:.1} ms, first analysis {:.3} ms \
         (one observation; OS file cache uncontrolled)",
        first_child.0, first_child.1
    );
    println!(
        "  lindera, subsequent fresh children: construction {:.1} ms, first analysis {:.3} ms \
         (median of {}; OS file cache uncontrolled)",
        median(subsequent.iter().map(|(c, _)| *c).collect()),
        median(subsequent.iter().map(|(_, f)| *f).collect()),
        subsequent.len()
    );
    println!(
        "  ichiran: not measured here — a WSL sidecar with its own installation state, timed by \
         ichiran_server.rs. A row that returns zero when WSL is absent would be worse than none."
    );
}

/// Resource replacement, leg by leg.
///
/// # Why the legs are measured and not subtracted
///
/// An earlier version of this reported a reclamation cost by **subtracting one whole import from
/// another**: import with the old generation held, import again after releasing it, call the
/// difference reclamation. Three things were wrong with that. The subtraction carried the noise of
/// two full index builds, which dominate. The second import could reclaim **two** generations,
/// because by then both earlier ones were unleased. And nothing in the arrangement distinguished
/// the rename from the removal.
///
/// So `Dictionary::import_timed` reports each leg from inside, and this prints them unmixed. They
/// are unrelated quantities: build is proportional to the archive, publish is a rename plus a
/// small pointer write whatever the dictionary weighs, and reclaim is a `remove_dir_all` over
/// however many superseded generations nobody holds.
///
/// # The trigger, which the numbers alone would not show
///
/// A generation is reclaimed when a publish finds it **unleased**, not simply on the next publish.
/// The `reclaimed` column is the evidence: it reads 0 while a reader is open — the normal state of
/// a running app — and only becomes non-zero once the readers are dropped.
#[test]
#[ignore = "a measurement; writes a synthetic dictionary"]
fn what_replacing_a_dictionary_costs() {
    require_release();
    use sakatsu_core::dict::index::Dictionary;

    let scratch = common::Scratch::new("performance", "replace");
    let out = scratch.join("out");

    // Large enough that a `remove_dir_all` over it is a measurable cost rather than a rounding
    // error, and small enough to build in a second.
    const ENTRIES: usize = 20_000;
    let archive = scratch.join("bulk.zip");
    {
        let file = std::fs::File::create(&archive).expect("create zip");
        let mut writer = zip::ZipWriter::new(file);
        let bank: Vec<serde_json::Value> = (0..ENTRIES)
            .map(|n| {
                serde_json::json!([
                    format!("語{n}"),
                    format!("ご{n}"),
                    "n",
                    "",
                    10,
                    [format!("word number {n}")],
                    n + 1,
                    ""
                ])
            })
            .collect();
        for (member, contents) in [
            (
                "index.json",
                serde_json::json!({ "title": "Bulk", "format": 3 }).to_string(),
            ),
            ("term_bank_1.json", serde_json::json!(bank).to_string()),
        ] {
            writer
                .start_file(member, zip::write::SimpleFileOptions::default())
                .expect("member");
            std::io::Write::write_all(&mut writer, contents.as_bytes()).expect("write");
        }
        writer.finish().expect("finish");
    }

    println!("  fixture entries {}", ENTRIES);
    print_identity(
        "dictionary-replacement-v1",
        &sakatsu_core::setup::resources::sha256_file(&archive)
            .expect("the synthetic archive can be fingerprinted before timing"),
    );

    println!("\n=== replacing a dictionary of {ENTRIES} entries ===");
    println!(
        "  {:<34} {:>9} {:>9} {:>9} {:>9}   {:>9}",
        "", "build", "validate", "publish", "reclaim", "reclaimed"
    );
    let row = |label: &str, legs: sakatsu_core::dict::index::ImportLegs| {
        let ms = |elapsed: Duration| elapsed.as_secs_f64() * 1000.0;
        println!(
            "  {:<34} {:>8.1} {:>8.1} {:>8.2} {:>8.1}   {:>9}",
            label,
            ms(legs.build),
            ms(legs.validate),
            ms(legs.publish),
            ms(legs.reclaim),
            legs.reclaimed
        );
    };

    let (first, legs) = Dictionary::import_timed(&archive, &out).expect("first import");
    row("first install, nothing to replace", legs);

    let (second, legs) = Dictionary::import_timed(&archive, &out).expect("second import");
    row("replace, previous generation HELD", legs);
    assert_eq!(
        legs.reclaimed, 0,
        "a leased generation must not be reclaimed; that is the guarantee, not an accident"
    );

    // One reader released, so exactly one generation becomes eligible.
    drop(first);
    let (third, legs) = Dictionary::import_timed(&archive, &out).expect("third import");
    row("replace, one generation FREE", legs);
    assert_eq!(
        legs.reclaimed, 1,
        "releasing one reader must free exactly one generation"
    );

    drop(second);
    drop(third);
    let (fourth, legs) = Dictionary::import_timed(&archive, &out).expect("fourth import");
    row("replace, two generations FREE", legs);
    assert_eq!(
        legs.reclaimed, 2,
        "and releasing two must free two — which is what the old subtraction was silently timing"
    );
    drop(fourth);

    println!(
        "\n  Reclamation is triggered by a publish finding an UNLEASED generation, not by the\n  \
         next publish. The `reclaimed` column is the evidence: it is 0 while a reader is open,\n  \
         which is the normal state of a running app, and the cost is paid whenever that reader\n  \
         is finally dropped."
    );
}

/// What marking one word costs, as the store grows.
///
/// # Why measure this before deciding anything about storage
///
/// `set_status` rewrites the **whole** sorted `words.json` and `fsync`s it, under a lock the
/// reader also takes. At roughly eighty bytes a record that is over a megabyte of I/O per click at
/// twenty thousand words, and the obvious conclusion — replace JSON with a database — is exactly
/// the kind of plausible-on-inspection change this branch keeps disproving by measuring.
///
/// So the figure is recorded and nothing is replaced. A learner with twenty thousand marked words
/// is already an unusual learner, and the cost has to be visibly bad at a realistic size before a
/// binary store is worth what it costs in inspectability — `store/mod.rs`'s own header says the
/// API is narrow precisely so that swap stays available if it is ever justified.
#[test]
#[ignore = "a measurement; writes a store"]
fn what_marking_a_word_costs_as_the_store_grows() {
    require_release();
    use sakatsu_core::store::{Store, WordKey, WordStatus};

    let scratch = common::Scratch::new("performance", "words");

    println!("\n=== marking one word, by how many are already marked ===");
    print_identity(
        "word-status-write-v1",
        &text_fixture_digest(["1000", "5000", "20000"]),
    );
    println!(
        "  {:<12} {:>10} {:>10} {:>12}",
        "marked", "median ms", "p95 ms", "words.json"
    );

    let store = Store::open(&scratch).expect("open");
    let mut marked = 0usize;
    for target in [1_000usize, 5_000, 20_000] {
        while marked < target {
            let key = WordKey::new(format!("語{marked}"), format!("ご{marked}"));
            store
                .set_status(key, WordStatus::Known)
                .expect("a mark must succeed");
            marked += 1;
        }
        // Twenty fresh marks at this size, timed one at a time. Fresh rather than repeated,
        // because re-marking a word it already holds is a different and cheaper question.
        let mut samples = Vec::new();
        for extra in 0..20 {
            let key = WordKey::new(
                format!("測{target}_{extra}"),
                format!("そく{target}_{extra}"),
            );
            let started = Instant::now();
            store
                .set_status(key, WordStatus::Seen)
                .expect("a mark must succeed");
            samples.push(started.elapsed());
            marked += 1;
        }
        let size = std::fs::metadata(scratch.join("words.json"))
            .map(|meta| meta.len())
            .unwrap_or(0);
        let (median, p95) = summarise(samples);
        println!(
            "  {:<12} {:>10.2} {:>10.2} {:>11.1} MB",
            target,
            median,
            p95,
            size as f64 / (1024.0 * 1024.0)
        );
    }

    println!(
        "\n  Every mark rewrites and fsyncs the whole file under a lock the reader also takes.\n  \
         Recorded, not acted on: see the doc comment for why replacing JSON is not in scope.\n\n  \
         **Read the ordering before reading the trend.** These figures are not monotonic in the\n  \
         number of words — a run here read 15.8 ms at 1,000 and 4.1 ms at 5,000 — so at these\n  \
         sizes the cost is dominated by the `fsync`, not by how much is written. The megabyte per\n  \
         click is real and the latency it causes is not yet visible above that floor. Anyone\n  \
         proposing a database on the strength of the file size should run this first."
    );
}
