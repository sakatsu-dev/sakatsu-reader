//! What the reader waits for, measured to the end of the response.
//!
//! ```text
//! cargo test --release -p sakatsu-desktop --test study_latency -- --ignored --nocapture
//! ```
//!
//! # Why this is not in `core`
//!
//! `core/tests/performance.rs` has a row it used to call "through the study view". It was not:
//! it ran the analysis, the joins, the reading correction, grammar breakdown, patterns, sentence
//! ranges, study keys and furigana — and then stopped. It omitted the store lookup that decides
//! how each word is coloured, and the construction and serialisation of the response itself.
//! That row is relabelled to say what it covers, and this file measures the rest, because
//! `StudyView` and `Store` both live on this side.
//!
//! **What "the reader waits for" means here**: everything `commands::study` does between
//! receiving text and handing back bytes, with a store that has words in it. What it still
//! excludes is the IPC hop and the browser's rendering, which are not this process's to measure.
//!
//! # Why the store is not empty
//!
//! `Store::statuses` is a lock and a hash lookup per token. Against an empty store that is
//! free, and against a real one it is not obviously free — a learner with twenty thousand marked
//! words is the case worth knowing about, so the fixture has them.

mod common;

use std::time::{Duration, Instant};

use sakatsu_core::analyzer::LinderaAnalyzer;
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::dict::KanjiDic;
use sakatsu_core::lookup;
use sakatsu_core::store::{Store, WordKey, WordStatus};
use sakatsu_core::study::{self, Resources};
use sha2::{Digest, Sha256};

/// A `#[cfg]` inside its own function, so the release build carries no check and the debug build
/// cannot be talked out of one.
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

/// **Synthetic**: the short set concatenated, so the figure is about passage length and not about
/// vocabulary. Kept identical in shape to `core/tests/performance.rs` so the two are comparable.
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

fn digest_texts<'a>(texts: impl IntoIterator<Item = &'a str>) -> String {
    let mut hash = Sha256::new();
    for text in texts {
        hash.update((text.len() as u64).to_le_bytes());
        hash.update(text.as_bytes());
    }
    sakatsu_core::hashing::hex(hash.finalize())
}

fn executable_digest() -> String {
    let path = std::env::current_exe().expect("the test binary knows its own path");
    let bytes = std::fs::read(path).expect("the running test executable can be read");
    format!(
        "{} ({} bytes, release)",
        sakatsu_core::hashing::hex(Sha256::digest(&bytes)),
        bytes.len()
    )
}

fn summarise(mut samples: Vec<Duration>) -> (f64, f64) {
    samples.sort_unstable();
    let millis = |duration: &Duration| duration.as_secs_f64() * 1000.0;
    let median = millis(&samples[samples.len() / 2]);
    let rank = ((samples.len() as f64 * 0.95).ceil() as usize).min(samples.len()) - 1;
    (median, millis(&samples[rank]))
}

fn measure<T>(inputs: &[String], repeats: usize, mut run: impl FnMut(&str) -> T) -> Vec<Duration> {
    for input in inputs {
        std::hint::black_box(run(input));
    }
    let mut samples = Vec::with_capacity(inputs.len() * repeats);
    for _ in 0..repeats {
        for input in inputs {
            let started = Instant::now();
            std::hint::black_box(run(input));
            samples.push(started.elapsed());
        }
    }
    samples
}

/// The whole of what `commands::study` does, to the serialised response.
#[test]
#[ignore = "a measurement; needs the dictionaries and writes a store"]
fn what_the_reader_waits_for() {
    require_release();

    let analyzer = LinderaAnalyzer::new().expect("compiled in");
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
    let kanji_path = dictionaries_dir()
        .parent()
        .expect("the dictionaries directory has a parent")
        .join("kanji")
        .join("kanjidic.tsv");
    let kanji = KanjiDic::load(&kanji_path);
    assert!(
        !kanji.is_empty(),
        "no KanjiDic — this row must include furigana or it is the row it replaced"
    );

    // A store with a learner's worth of marked words in it, so `statuses` is doing something.
    let scratch = common::Scratch::new("study-latency", "store");
    let store = Store::open(&scratch).expect("open");
    for n in 0..20_000 {
        store
            .set_status(
                WordKey::new(format!("語{n}"), format!("ご{n}")),
                WordStatus::Known,
            )
            .expect("a mark must succeed");
    }

    let short: Vec<String> = SHORT.iter().map(|text| (*text).to_string()).collect();
    let long = long_passages();

    println!("\n=== measurement identity ===");
    println!("  executable      {}", executable_digest());
    println!(
        "  fixture sha256  {}",
        digest_texts(
            short
                .iter()
                .map(String::as_str)
                .chain(long.iter().map(String::as_str))
        )
    );
    println!("  configuration   release; 20,000 marked words; warmup outside timing");
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
        "  resource        kanjidic {}",
        sakatsu_core::setup::resources::sha256_file(&kanji_path)
            .expect("the borrowed kanji table can be fingerprinted before timing")
    );

    // The production core service `commands::study` calls, ending where the command ends.
    let study = |text: &str| -> usize {
        let view = study::study(
            text,
            Resources {
                analyzer: &analyzer,
                dictionaries: &dictionaries,
                kanji: &kanji,
                store: Some(&store),
            },
        )
        .expect("the production study service must succeed");
        serde_json::to_vec(&view)
            .expect("the response serialises")
            .len()
    };

    println!("\n=== what the reader waits for, to the serialised response ===");
    println!(
        "  store: 20,000 marked words · dictionaries: {}",
        dictionaries
            .iter()
            .map(|dictionary| dictionary.meta().title.clone())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "  long passages are SYNTHETIC — the short set concatenated, so this is about passage \
         length and not vocabulary"
    );
    println!("\n  {:<40} {:>9} {:>9}   {:>6}", "", "median", "p95", "n");

    for (label, inputs) in [("short", &short), ("long", &long)] {
        let bytes = study(&inputs[0]);
        let samples = measure(inputs, 20, study);
        let count = samples.len();
        let (median, p95) = summarise(samples);
        println!(
            "  {:<40} {median:>8.3} {p95:>8.3}   {count:>6}   ({bytes} byte response)",
            format!("commands::study, {label}")
        );
    }

    println!(
        "\n  This is the whole command: analysis, joins, reading correction, grammar, patterns,\n  \
         sentence ranges, study keys, store statuses, furigana, and the serialised response.\n  \
         It excludes the IPC hop and the browser's rendering, which are not this process's."
    );
}

/// Measure the selection lookup changed by lexical roles and retrieval provenance. Analysis
/// and dictionary discovery happen before timing; each sample runs the same contextual lookup
/// and serialises the answer that reaches the reader. This is an absolute post-change cost,
/// not a before/after speed claim and not the full UI round trip.
#[test]
#[ignore = "release measurement; needs installed dictionaries"]
fn contextual_lookup_for_standalone_assistance() {
    require_release();
    let analyzer = LinderaAnalyzer::new().expect("compiled-in analyzer");
    let dictionaries: Vec<Dictionary> = Dictionary::discover(&dictionaries_dir())
        .into_iter()
        .map(|path| Dictionary::open(&path).expect("installed dictionary opens"))
        .collect();
    assert!(
        !dictionaries.is_empty(),
        "installed dictionaries are required"
    );

    let passages = [
        (
            "pronoun",
            "なんであたしがそれで怒んなきゃいかんのよ。",
            "なん",
        ),
        ("suffix", "双星神姫の私があなたたちを裁いてやるわ！", "たち"),
        (
            "component",
            "双星神姫の私があなたたちを裁いてやるわ！",
            "神姫",
        ),
    ];
    let prepared: Vec<_> = passages
        .iter()
        .map(|(label, text, surface)| {
            let analysis = study::analyse(text, &analyzer, &dictionaries)
                .expect("production analysis passes its span contract");
            let at = analysis
                .tokens
                .iter()
                .position(|token| token.surface == *surface)
                .unwrap_or_else(|| panic!("{label} token {surface} is absent"));
            (*label, analysis, at)
        })
        .collect();

    println!("\n=== standalone contextual lookup identity ===");
    println!("  executable      {}", executable_digest());
    println!(
        "  fixture sha256  {}",
        digest_texts(passages.iter().map(|(_, text, _)| *text))
    );
    println!("  configuration   release; Lindera; 3 targets; 20 interleaved repetitions");
    for dictionary in &dictionaries {
        println!(
            "  resource        dictionary {} {}",
            dictionary.meta().title,
            dictionary
                .content_fingerprint()
                .expect("fingerprint dictionary")
        );
    }

    let run = |analysis: &sakatsu_core::model::Analysis, at: usize| {
        let answer = lookup::in_context(&analysis.tokens, at, &dictionaries)
            .expect("installed dictionary lookup");
        serde_json::to_vec(&answer)
            .expect("serialize contextual answer")
            .len()
    };
    let bytes: Vec<_> = prepared
        .iter()
        .map(|(_, analysis, at)| run(analysis, *at))
        .collect();
    let mut samples: Vec<Vec<Duration>> = vec![Vec::new(); prepared.len()];
    for repetition in 0..20 {
        for offset in 0..prepared.len() {
            let index = (repetition + offset) % prepared.len();
            let (_, analysis, at) = &prepared[index];
            let started = Instant::now();
            std::hint::black_box(run(analysis, *at));
            samples[index].push(started.elapsed());
        }
    }
    for (index, (label, _, _)) in prepared.iter().enumerate() {
        let (median, p95) = summarise(std::mem::take(&mut samples[index]));
        println!(
            "  {label:<10} median {median:.3} ms  p95 {p95:.3} ms  n=20  response={} bytes",
            bytes[index]
        );
    }
}
