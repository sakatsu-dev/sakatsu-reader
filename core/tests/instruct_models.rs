//! Would an instruct model be useful here, and does the app's knowledge help it?
//!
//! ```text
//! # triage every candidate on the 50 banded sentences
//! cargo test --release -p sakatsu-core --test instruct_models -- --ignored --nocapture triage
//!
//! # then the survivors, on 300
//! SAKATSU_LLM_MODELS="qwen3:4b-instruct,elyza:8b" \
//!   cargo test --release -p sakatsu-core --test instruct_models -- --ignored --nocapture pooled
//! ```
//!
//! # What this is for
//!
//! `knowledge_vs_model.rs` established two things. The app's dictionary-and-grammar reading of a
//! sentence recovers *more* of a human translation than FuguMT does, at a precision of 0.05; and
//! the best rule for surfacing that at runtime shows two or three words at 6% precision beside 91%
//! of sentences. Every noise case in that second run was a **wrong sense** of a word whose right
//! sense sat in the same dictionary entry — あまり as "remainder", 額 as "forehead".
//!
//! Sense selection in context is the whole gap. A gloss list cannot do it. An instruct model can
//! be *asked* to, which FuguMT structurally cannot: its ONNX graph takes source tokens and a
//! decoder prefix, and there is nowhere to put a prompt.
//!
//! So each model is asked twice — once plainly, and once with every candidate sense the dictionary
//! offers. **The delta between those is the question.** A probe before this was written found that
//! `qwen3:4b-instruct`, `elyza:8b` and `qwen2.5:7b` all read 額 as *frame* with no help at all,
//! where FuguMT read it as *forehead*, so a delta of zero is a real possible answer: not "the app's
//! knowledge makes a model better" but "a better model does not need it".
//!
//! # Why it talks to a server
//!
//! `project notes` §2 runs models in-process through ONNX Runtime, and `core/src/ocr/vlm.rs` is the
//! standing exception with the reasoning already written down: an autoregressive model of this
//! size cannot go through `ort` here without a KV cache that does not exist. This borrows that
//! decision wholesale — the same OpenAI chat-completions shape, the same `temperature: 0`, and
//! the same view that a confident hallucination is worse than visible garbage.
//!
//! # This is a measurement, not a gate
//!
//! No threshold. FuguMT's numbers are already on record for comparison: pooled F1 **0.54**, and
//! per band 0.68 at N5 falling to 0.40 at N1.

mod common;
#[path = "common/sentences.rs"]
mod sentences;

use std::collections::BTreeMap;
use std::io::Write;
use std::time::{Duration, Instant};

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::dict::Dictionary;
use sakatsu_core::grammar::particles::Categories;
use sakatsu_core::model::Analysis;
use sakatsu_core::translate::{FuguMt, Translator};

use common::{content, per_token, Score};
use sentences::SENTENCES;

/// Say it to the terminal **and** to a file, and flush both.
///
/// **A long run has to be readable while it runs.** The first attempt at this sweep let cargo hold
/// stdout: the five small models finished in ten minutes and their numbers were then invisible for
/// an hour behind a 27B that had to be killed, taking the whole report with it. The CDP harnesses
/// in `scratchpad/` have always written incrementally for exactly this reason and the habit did
/// not make it across to Rust. `target/instruct-models.txt` survives an interrupted run.
macro_rules! say {
    ($($arg:tt)*) => {{
        let line = format!($($arg)*);
        println!("{line}");
        let _ = std::io::stdout().flush();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/instruct-models.txt");
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{line}");
        }
    }};
}

/// Where the server is. Ollama's default; llama-server and LM Studio speak the same shape.
fn endpoint() -> String {
    std::env::var("SAKATSU_LLM_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".into())
}

/// Generous, for the same reason `vlm.rs` is generous: this may be a CPU, and timing out on a
/// call that was going to succeed wastes the whole run.
const TIMEOUT: Duration = Duration::from_secs(180);

/// The candidates. Anything the server does not have is skipped with a line saying so, rather
/// than failing the run — a machine with three of these installed should still get a report.
const CANDIDATES: &[(&str, &str, Style)] = &[
    (
        "hf.co/mmnga-o/CAT-Translate-1.4b-gguf:Q4_K_M",
        "MIT, 0.9 GB, purpose-built JA↔EN",
        Style::Narrow,
    ),
    ("qwen3:4b-instruct", "Apache-2.0, 2.5 GB", Style::Instruct),
    ("gemma3:4b", "Gemma terms, 3.3 GB", Style::Instruct),
    ("qwen2.5:7b", "Apache-2.0, 4.7 GB", Style::Instruct),
    (
        "elyza:8b",
        "Llama-3 terms, 4.9 GB, Japanese-specialised",
        Style::Instruct,
    ),
];

/// Whether a model can be told anything beyond "translate this".
///
/// **Measured, not assumed.** CAT-Translate is a translation model rather than an
/// instruction-following one: given the extra sentence *Reply with the translation only* it
/// translated the Japanese and then echoed that sentence back as part of its answer. So it gets
/// the format its own model card documents — and the finding matters well beyond the prompt,
/// because a model that cannot take an instruction cannot be handed the app's knowledge either.
#[derive(Clone, Copy, PartialEq)]
enum Style {
    /// Follows instructions. Gets the tidy-output request, and can be given glosses.
    Instruct,
    /// Translates, and nothing else. Gets the format its own card documents.
    Narrow,
}

/// The ceiling: is this task solvable at all, or is a 4B simply too small?
///
/// **Opt-in via `SAKATSU_LLM_CEILING=1`, and never in the default sweep.** It answers a real
/// question — a poor 4B result means nothing until you know a large model can do it — but it is a
/// safety-stripped community fine-tune, it can never ship at 17.4 GB, and it *thinks* before every
/// answer. Measured: it spent over an hour on the 50-sentence corpus, more than the five shippable
/// candidates put together, and had to be killed. Run it deliberately or not at all.
const CEILING: (&str, &str, Style) = (
    "huihui_ai/Qwen3.6-abliterated:27b",
    "ceiling, 17.4 GB, thinks",
    Style::Instruct,
);

// ── Talking to the server ───────────────────────────────────────────────────────────────────

fn installed() -> Vec<String> {
    let url = format!("{}/api/tags", endpoint());
    let Ok(mut response) = ureq::get(&url).call() else {
        return Vec::new();
    };
    let Ok(body) = response.body_mut().read_to_string() else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Vec::new();
    };
    parsed["models"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|m| m["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// One completion, and what it cost.
fn ask(model: &str, prompt: &str) -> Option<(String, f64)> {
    let url = format!("{}/v1/chat/completions", endpoint());
    let body = serde_json::json!({
        "model": model,
        // Pinned, as `vlm.rs` pins it. A translation that changes between runs cannot be measured,
        // and creativity is the last thing wanted from this.
        "temperature": 0,
        "messages": [{ "role": "user", "content": prompt }],
    });
    let started = Instant::now();
    let mut response = ureq::post(&url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .send_json(&body)
        .ok()?;
    let text = response.body_mut().read_to_string().ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&text).ok()?;
    let said = parsed["choices"][0]["message"]["content"]
        .as_str()?
        .to_owned();
    Some((said, started.elapsed().as_secs_f64()))
}

/// What a model said, once the wrapping is off.
struct Reply {
    english: String,
    /// It reasoned out loud. Several of these models think by default, and the thinking is not
    /// the answer — but it is time spent, so it is counted rather than quietly discarded.
    thought: bool,
    /// It did not do as it was told: a preamble, a sign-off, or the translation in quotes.
    /// Cheap to strip and worth knowing, because a model that must be post-processed is a
    /// different integration proposition from one that answers cleanly.
    chatty: bool,
}

/// Openings that mean the model is talking about the translation rather than giving it.
const PREAMBLE: &[&str] = &[
    "here is",
    "here's",
    "sure",
    "the translation",
    "translation:",
    "certainly",
    "of course",
];

fn read_reply(raw: &str) -> Reply {
    let mut text = raw.to_string();
    let thought = text.contains("<think>");
    if let (Some(open), Some(close)) = (text.find("<think>"), text.find("</think>")) {
        if close > open {
            text = format!("{}{}", &text[..open], &text[close + "</think>".len()..]);
        }
    }
    // A narrow model served from a bare GGUF emits its end marker as text — there is no chat
    // template to swallow it. Left in, it becomes a content word and scores against the model.
    for marker in ["</s>", "<|endoftext|>", "<|im_end|>", "<end_of_turn>"] {
        text = text.replace(marker, " ");
    }
    let text = text.trim().to_string();
    let lowered = text.to_ascii_lowercase();
    let chatty = PREAMBLE.iter().any(|start| lowered.starts_with(start))
        || (text.starts_with('"') && text.ends_with('"'))
        || text.lines().filter(|line| !line.trim().is_empty()).count() > 1;

    // Strip what can be stripped, so the score measures the translation rather than the manners.
    let mut english = text.clone();
    if let Some(rest) = english.split_once(':') {
        if PREAMBLE.iter().any(|start| lowered.starts_with(start)) {
            english = rest.1.trim().to_string();
        }
    }
    let english = english.trim().trim_matches('"').trim().to_string();
    Reply {
        english,
        thought,
        chatty,
    }
}

// ── The two prompts ─────────────────────────────────────────────────────────────────────────

/// CAT-Translate's own documented instruction, used for every model so the comparison is of
/// models rather than of prompt-tuning.
fn plain(japanese: &str, style: Style) -> String {
    match style {
        // CAT-Translate's own card, verbatim. Anything appended becomes text to translate.
        Style::Narrow => {
            format!("Translate the following Japanese text into English.\n\n{japanese}")
        }
        Style::Instruct => format!(
            "Translate the following Japanese text into English. Reply with the translation only.\n\n{japanese}"
        ),
    }
}

/// The same, with every reading the dictionary offers for every word.
///
/// **The senses are offered, not chosen.** Picking one here would be the app doing the model's
/// job, and doing it badly is exactly what the 6%-precision result measured. What this asks is
/// whether a model can select from a list the app already holds.
fn with_knowledge(
    japanese: &str,
    analysis: &Analysis,
    dictionary: &Dictionary,
    categories: &Categories,
    style: Style,
) -> String {
    let words = per_token(analysis, dictionary, categories, usize::MAX);
    let mut lines = String::new();
    for (word, token) in words
        .iter()
        .zip(analysis.tokens.iter().filter(|t| t.is_word()))
    {
        if word.glosses.is_empty() && word.authored.is_empty() {
            continue;
        }
        let mut said: Vec<String> = Vec::new();
        if !word.glosses.is_empty() {
            said.push(word.glosses.clone());
        }
        said.extend(word.authored.iter().cloned());
        lines.push_str(&format!("{}: {}\n", token.surface, said.join("; ")));
    }
    if lines.is_empty() {
        return plain(japanese, style);
    }
    format!(
        "{}\n\nA dictionary offers these readings for the words in it. Several are usually wrong \
         for this sentence; choose the ones that fit.\n{lines}",
        plain(japanese, style)
    )
}

// ── Tallying ────────────────────────────────────────────────────────────────────────────────

/// One sentence as all three read it, kept for the report.
///
/// A named type because the tuple it replaced was four `String`s wide and said nothing about
/// which was which — clippy objected, and it was right.
struct Sample {
    japanese: String,
    human: String,
    plain: String,
    with_knowledge: String,
}

#[derive(Default)]
struct Run {
    score: Score,
    seconds: f64,
    thought: usize,
    chatty: usize,
    failed: usize,
    prompt_chars: usize,
    sentences: usize,
}

impl Run {
    fn per_sentence(&self) -> f64 {
        self.seconds / self.sentences.max(1) as f64
    }
}

fn sweep(
    model: &str,
    style: Style,
    corpus: &[(String, String)],
    analyzer: &LinderaAnalyzer,
    dictionary: &Dictionary,
    categories: &Categories,
) -> (Run, Run, Vec<Sample>) {
    let mut bare = Run::default();
    let mut told = Run::default();
    let mut samples = Vec::new();

    for (japanese, human) in corpus {
        let Ok(analysis) = analyzer.analyze(japanese) else {
            continue;
        };
        let reference = content(human);
        let prompts = [
            plain(japanese, style),
            with_knowledge(japanese, &analysis, dictionary, categories, style),
        ];

        let mut said: [String; 2] = Default::default();
        for (slot, prompt) in prompts.iter().enumerate() {
            let run = if slot == 0 { &mut bare } else { &mut told };
            run.prompt_chars += prompt.chars().count();
            run.sentences += 1;
            match ask(model, prompt) {
                Some((raw, secs)) => {
                    let reply = read_reply(&raw);
                    run.seconds += secs;
                    run.thought += usize::from(reply.thought);
                    run.chatty += usize::from(reply.chatty);
                    run.score.add(&reference, &content(&reply.english));
                    said[slot] = reply.english;
                }
                None => run.failed += 1,
            }
        }

        if samples.len() < 6 {
            samples.push(Sample {
                japanese: japanese.clone(),
                human: human.clone(),
                plain: said[0].clone(),
                with_knowledge: said[1].clone(),
            });
        }
    }

    (bare, told, samples)
}

fn report(model: &str, note: &str, bare: &Run, told: &Run) {
    say!("\n  {model}  ({note})");
    say!(
        "    {:<14}{:>8}{:>11}{:>7}{:>9}{:>8}{:>8}",
        "prompt",
        "recall",
        "precision",
        "F1",
        "s/sent",
        "chatty",
        "failed"
    );
    for (name, run) in [("plain", bare), ("+knowledge", told)] {
        say!(
            "    {:<14}{:>8.2}{:>11.2}{:>7.2}{:>9.1}{:>8}{:>8}",
            name,
            run.score.recall(),
            run.score.precision(),
            run.score.f1(),
            run.per_sentence(),
            run.chatty,
            run.failed,
        );
    }
    say!(
        "    knowledge delta  F1 {:+.2}   prompt {}→{} chars   thought out loud {}/{}",
        told.score.f1() - bare.score.f1(),
        bare.prompt_chars / bare.sentences.max(1),
        told.prompt_chars / told.sentences.max(1),
        told.thought,
        told.sentences,
    );
}

fn setup() -> Option<(LinderaAnalyzer, Dictionary, Categories, Vec<String>)> {
    let have = installed();
    if have.is_empty() {
        say!(
            "SKIP: nothing serving at {} — start Ollama, or set SAKATSU_LLM_URL",
            endpoint()
        );
        return None;
    }
    let imported = common::dictionaries_dir().join("jitendex");
    if !sakatsu_core::dict::index::Dictionary::exists_in(&imported) {
        say!("SKIP: no imported Jitendex at {}", imported.display());
        return None;
    }
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let dictionary = Dictionary::open(&imported).expect("opens");
    let path = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .map(|dir| {
            dir.join("com.sakatsu.app")
                .join("wordnet")
                .join("categories.tsv")
        });
    let categories = match path {
        Some(path) => Categories::load(&path),
        None => Categories::default(),
    };
    Some((analyzer, dictionary, categories, have))
}

fn banded() -> Vec<(String, String)> {
    SENTENCES
        .iter()
        .map(|s| (s.text.to_string(), s.english.to_string()))
        .collect()
}

#[test]
#[ignore = "needs a local model server and the imported Jitendex; use --release"]
fn triage_every_candidate_on_the_banded_fifty() {
    let Some((analyzer, dictionary, categories, have)) = setup() else {
        return;
    };
    let corpus = banded();
    let started = Instant::now();

    say!("\n=== triage — 50 native sentences, ten per JLPT level ===");
    say!("  FuguMT on this corpus, for comparison: F1 0.68 at N5 falling to 0.40 at N1");
    say!("  (`knowledge_vs_model.rs`, same corpus, same metric)");

    let mut candidates: Vec<(&str, &str, Style)> = CANDIDATES.to_vec();
    if std::env::var_os("SAKATSU_LLM_CEILING").is_some() {
        candidates.push(CEILING);
    }

    // **The baseline runs here, in this loop, on this corpus.** It was quoted from
    // `knowledge_vs_model.rs` at first, which meant comparing an aggregate over 50 sentences
    // against five per-band figures from another report — close enough to eyeball and not close
    // enough to conclude from. One run, one metric, one table.
    let mut kept: Vec<(String, f64)> = Vec::new();
    let fugu_dir = std::env::var_os("SAKATSU_MT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA"))
                .join("com.sakatsu.app")
                .join("models")
                .join("fugumt-ja-en")
        });
    if FuguMt::is_present(&fugu_dir) {
        let engine = FuguMt::load(&fugu_dir).expect("load the model");
        let mut baseline = Run::default();
        for (japanese, human) in &corpus {
            let started = Instant::now();
            baseline.sentences += 1;
            match engine.translate(japanese) {
                Ok(said) => {
                    baseline.seconds += started.elapsed().as_secs_f64();
                    baseline.score.add(&content(human), &content(&said));
                }
                Err(_) => baseline.failed += 1,
            }
        }
        say!(
            "
  FuguMT  (the incumbent — 61M params, 300 MB, in-process)"
        );
        say!(
            "    {:<14}{:>8}{:>11}{:>7}{:>9}",
            "prompt",
            "recall",
            "precision",
            "F1",
            "s/sent"
        );
        say!(
            "    {:<14}{:>8.2}{:>11.2}{:>7.2}{:>9.2}",
            "n/a",
            baseline.score.recall(),
            baseline.score.precision(),
            baseline.score.f1(),
            baseline.per_sentence()
        );
        kept.push(("FuguMT (baseline)".to_string(), baseline.score.f1()));
    } else {
        say!(
            "
  FuguMT — not installed, so there is no baseline in this table"
        );
    }

    // **One model at a time, in order.** Ollama evicts on every switch, and a probe measured the
    // same model at 0.6s and 27s depending on whether an interleaved call had pushed it out.
    // Interleaving here would measure the loader.
    for (model, note, style) in &candidates {
        if !have.iter().any(|name| name == model) {
            say!("\n  {model} — not installed, skipped");
            continue;
        }
        let (bare, told, samples) =
            sweep(model, *style, &corpus, &analyzer, &dictionary, &categories);
        report(model, note, &bare, &told);
        kept.push(((*model).to_string(), bare.score.f1().max(told.score.f1())));
        for sample in samples.iter().take(2) {
            say!("      {}", sample.japanese);
            say!("        human       {}", sample.human);
            say!("        plain       {}", sample.plain);
            say!("        +knowledge  {}", sample.with_knowledge);
        }
    }

    kept.sort_by(|a, b| b.1.total_cmp(&a.1));
    say!("\n  best first: ");
    for (model, f1) in &kept {
        say!("    {f1:.2}  {model}");
    }
    say!("\n  measured in {:.0}s", started.elapsed().as_secs_f64());
    assert!(!kept.is_empty(), "no candidate was installed");
}

#[test]
#[ignore = "needs a local model server and the imported Jitendex; use --release"]
fn the_survivors_on_the_pooled_three_hundred() {
    let Some((analyzer, dictionary, categories, have)) = setup() else {
        return;
    };
    // Named rather than guessed: triage says which are worth four minutes each.
    let Ok(chosen) = std::env::var("SAKATSU_LLM_MODELS") else {
        say!("SKIP: set SAKATSU_LLM_MODELS to a comma-separated list — run triage first");
        return;
    };

    let pairs = common::jitendex_examples();
    let usable: Vec<&(String, String)> = pairs
        .iter()
        .filter(|(japanese, english)| {
            !japanese.is_empty() && english.split_whitespace().count() > 3
        })
        .collect();
    let step = (usable.len() / 300).max(1);
    let corpus: Vec<(String, String)> = usable
        .iter()
        .step_by(step)
        .take(300)
        .map(|(japanese, english)| (japanese.clone(), english.clone()))
        .collect();

    say!("\n=== pooled — {} ungraded pairs ===", corpus.len());
    say!("  FuguMT on this corpus: recall 0.52  precision 0.57  F1 0.54");

    let mut table: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for model in chosen.split(',').map(str::trim).filter(|m| !m.is_empty()) {
        if !have.iter().any(|name| name == model) {
            say!("\n  {model} — not installed, skipped");
            continue;
        }
        let style = CANDIDATES
            .iter()
            .find(|(name, _, _)| name == &model)
            .map(|(_, _, style)| *style)
            .unwrap_or(Style::Instruct);
        let (bare, told, samples) =
            sweep(model, style, &corpus, &analyzer, &dictionary, &categories);
        report(model, "", &bare, &told);
        table.insert(model.to_string(), (bare.score.f1(), told.score.f1()));
        for sample in samples.iter().take(3) {
            say!("      {}", sample.japanese);
            say!("        human       {}", sample.human);
            say!("        plain       {}", sample.plain);
            say!("        +knowledge  {}", sample.with_knowledge);
        }
    }

    say!("\n  against FuguMT's 0.54:");
    for (model, (bare, told)) in &table {
        say!(
            "    plain {bare:.2} ({:+.2})   +knowledge {told:.2} ({:+.2})   {model}",
            bare - 0.54,
            told - 0.54
        );
    }
    assert!(!table.is_empty(), "no named model was installed");
}
