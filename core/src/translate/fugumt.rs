//! FuguMT Japanese→English through ONNX Runtime.
//!
//! ## The graph, as read from the files
//!
//! ```text
//! encoder: input_ids             i64 [batch, seq]
//!          attention_mask        i64 [batch, seq]
//!       →  last_hidden_state     f32 [batch, seq, 512]
//!
//! decoder: encoder_attention_mask i64 [batch, enc_seq]
//!          input_ids              i64 [batch, dec_seq]
//!          encoder_hidden_states  f32 [batch, enc_seq, 512]
//!       →  logits                 f32 [batch, dec_seq, 32001]
//! ```
//!
//! **512 and 32001 are this model's, not the shape's.** Both are read back from the
//! tensors at runtime rather than written down here, because a hardcoded width does not
//! fail against a model that disagrees — it reshapes and quietly produces nonsense.
//!
//! Note the decoder takes **`encoder_attention_mask`** — manga-ocr's did not, and
//! leaving it out is not an option the graph allows. It also emits `present.*` cache
//! tensors meant for the `decoder_with_past` variant; they are ignored, so each step
//! re-runs the whole prefix. Slower, and far less to get wrong.
//!
//! ## Greedy decoding, deliberately
//!
//! `generation_config.json` asks for **12 beams**. `project notes` §7 overrides that, and
//! it is the single most load-bearing decision in this file: beam search on this model
//! degenerates into word salad. **This is not to be "optimised" into beam search.**
//!
//! Two ids from that same config matter as much:
//!
//! - `decoder_start_token_id` is **32000**, which is also `pad_token_id`.
//! - `bad_words_ids` is `[[32000]]` — that pad token must never be *generated*. Since
//!   it is what the decoder starts on, an unbanned greedy loop can simply re-emit it
//!   and produce nothing at all.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ort::session::{Session, SessionInputValue};
use ort::value::TensorRef;
use tokenizers::Tokenizer;

use super::{TranslateError, Translator};

/// From `generation_config.json`.
const DECODER_START: i64 = 32000;
const END_OF_TEXT: i64 = 0;
/// Also the decoder's start token, and banned from being generated.
const BANNED: i64 = 32000;
/// `max_length` from the same config. A guard as much as a limit.
const MAX_TOKENS: usize = 512;

/// What the encoder is actually given, which is not quite what the reader captured.
///
/// **Measured, not tidiness.** `core/tests/fugumt_levers.rs` over 300 sentences: folding
/// full-width ASCII wins **22 sentences to 4** against leaving it alone (sign test p < 0.001,
/// chrF2 0.545 to 0.552, BLEU 26.7 to 27.7), and a stray line break costs more than the entire
/// measured gap between this model and a 7B instruct one — cosine 0.856 to 0.827, losing 137
/// sentences to 43. Joining the lines back recovers it *exactly*: 300 ties against the baseline.
///
/// The failure it removes is tokenisation. `６人宿泊させて欲しい` came back as "I want to make you
/// stay" — the six simply gone — where `6人宿泊させて欲しい` gives "I want six people to stay".
/// `１．とりむね肉を` lost its list number the same way.
///
/// # What it deliberately does not touch
///
/// 。、ー and the kana all sit outside `FF01..=FF5E`, so they pass through untouched — this folds
/// the *Latin* range only. An earlier draft also folded em dashes onto ー; that rule fired on zero
/// of the 300 sentences, so it was measuring nothing and is not here. Nothing rewrites a word, and
/// **nothing here reaches the reader**: it is applied on the way to the encoder, and the passage on
/// screen stays exactly as it was captured.
fn for_the_model(text: &str) -> String {
    let folded: String = text
        .chars()
        .map(|c| match c {
            // Full-width ASCII down to the ordinary range: ！０Ａ become !0A.
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            _ => c,
        })
        .collect();
    // Joined with nothing, because Japanese does not separate words with spaces: a break in the
    // middle of a captured line is not a word boundary, and inserting one would invent a token.
    folded
        .split(['\n', '\r'])
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}

/// FuguMT, loaded from a directory of ONNX files.
pub struct FuguMt {
    encoder: Mutex<Session>,
    decoder: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl FuguMt {
    /// Load the model from `dir`, as `scripts/fetch-translation-model.ps1` lays it out.
    pub fn load(dir: &Path) -> Result<FuguMt, TranslateError> {
        let tokenizer = load_tokenizer(&dir.join("tokenizer.json"))?;
        Ok(FuguMt {
            encoder: Mutex::new(session(&dir.join("encoder_model.onnx"))?),
            decoder: Mutex::new(session(&dir.join("decoder_model.onnx"))?),
            tokenizer,
        })
    }

    /// Whether a directory holds the files [`FuguMt::load`] needs.
    pub fn is_present(dir: &Path) -> bool {
        ["encoder_model.onnx", "decoder_model.onnx", "tokenizer.json"]
            .iter()
            .all(|name| dir.join(name).is_file())
    }
}

/// Load the tokenizer, working around a flaw in the published export.
///
/// Its `tokenizer.json` carries `{"type": "Precompiled", "precompiled_charsmap": null}`,
/// and the `tokenizers` crate refuses a null charsmap outright. A Precompiled
/// normalizer *with no map* does nothing, so it is dropped rather than repaired —
/// nothing is lost that the export ever contained.
///
/// Public so `tests/onnx_probe.rs` can describe this tokenizer without keeping a second
/// copy of the workaround. It tried `Tokenizer::from_file` instead and could not even
/// report the failure: the crate **panics** on the null charsmap rather than returning
/// `Err`, so the probe's error handling never ran. Two copies of a workaround that must
/// agree is the other way to get this wrong.
pub fn load_tokenizer(path: &Path) -> Result<Tokenizer, TranslateError> {
    let json = std::fs::read_to_string(path).map_err(|error| {
        TranslateError::EngineUnavailable(format!("{}: {error}", path.display()))
    })?;

    let mut value: serde_json::Value = serde_json::from_str(&json)
        .map_err(|error| TranslateError::EngineUnavailable(format!("tokenizer.json: {error}")))?;

    let empty_precompiled = value["normalizer"]["type"] == "Precompiled"
        && value["normalizer"]["precompiled_charsmap"].is_null();
    if empty_precompiled {
        value["normalizer"] = serde_json::Value::Null;
    }

    Tokenizer::from_bytes(value.to_string().as_bytes())
        .map_err(|error| TranslateError::EngineUnavailable(format!("tokenizer.json: {error}")))
}

fn session(path: &PathBuf) -> Result<Session, TranslateError> {
    if !path.is_file() {
        return Err(TranslateError::EngineUnavailable(format!(
            "{} is missing — run scripts/fetch-translation-model.ps1",
            path.display()
        )));
    }
    crate::onnx::session_builder()
        .and_then(|mut builder| builder.commit_from_file(path))
        .map_err(|error| {
            TranslateError::EngineUnavailable(format!("could not load {}: {error}", path.display()))
        })
}

/// The highest-scoring token that is not banned.
fn best_token(logits: &[f32]) -> i64 {
    let mut best = (f32::NEG_INFINITY, END_OF_TEXT);
    for (id, &score) in logits.iter().enumerate() {
        let id = id as i64;
        if id == BANNED {
            continue;
        }
        if score > best.0 {
            best = (score, id);
        }
    }
    best.1
}

impl Translator for FuguMt {
    fn translate(&self, text: &str) -> Result<String, TranslateError> {
        let text = for_the_model(text);
        let text = text.as_str();
        if text.is_empty() {
            return Err(TranslateError::EmptyInput);
        }

        // Greedy decoding is one forward pass per token, so this scales with the length of
        // the *answer* rather than the question -- worth knowing before deciding whether a
        // GPU would help, since that shape is where transfer overhead most often eats the win.
        let _timing = crate::log::Timing::start(
            "fugumt",
            format!("translating {} characters", text.chars().count()),
        );

        let encoded = self
            .tokenizer
            .encode(text, true)
            .map_err(|error| TranslateError::Failed(error.to_string()))?;
        let source: Vec<i64> = encoded.get_ids().iter().map(|&id| i64::from(id)).collect();
        if source.is_empty() {
            return Err(TranslateError::EmptyInput);
        }
        // Every source token is real, so the mask is all ones.
        let mask: Vec<i64> = vec![1; source.len()];
        let source_shape = vec![1i64, source.len() as i64];

        // The encoder reports its own `[batch, seq, width]`, and that shape is kept rather
        // than rebuilt from a literal width and a division. The width is a property of the
        // weights, and a wrong one does not fail here: the tensor still reshapes, and the
        // damage surfaces inside the decoder loop, far from its cause. The vocabulary below
        // is taken from its shape for the same reason.
        let (hidden_shape, hidden): (Vec<i64>, Vec<f32>) = {
            let ids = TensorRef::<i64>::from_array_view((source_shape.clone(), source.as_slice()))
                .map_err(failed)?;
            let attention =
                TensorRef::<i64>::from_array_view((source_shape.clone(), mask.as_slice()))
                    .map_err(failed)?;

            let mut encoder = self.encoder.lock().unwrap_or_else(|p| p.into_inner());
            let outputs = encoder
                .run(vec![("input_ids", ids), ("attention_mask", attention)])
                .map_err(failed)?;
            let (shape, data) = outputs["last_hidden_state"]
                .try_extract_tensor::<f32>()
                .map_err(failed)?;
            if shape.len() != 3 {
                return Err(TranslateError::Failed(format!(
                    "last_hidden_state has {} dimensions, expected 3",
                    shape.len()
                )));
            }
            (shape.to_vec(), data.to_vec())
        };

        let mut ids = vec![DECODER_START];
        let mut decoder = self.decoder.lock().unwrap_or_else(|p| p.into_inner());

        for _ in 0..MAX_TOKENS {
            let decoder_ids =
                TensorRef::<i64>::from_array_view((vec![1i64, ids.len() as i64], ids.as_slice()))
                    .map_err(failed)?;
            let encoder_mask =
                TensorRef::<i64>::from_array_view((source_shape.clone(), mask.as_slice()))
                    .map_err(failed)?;
            let encoder_hidden =
                TensorRef::<f32>::from_array_view((hidden_shape.clone(), hidden.as_slice()))
                    .map_err(failed)?;

            let inputs: Vec<(&str, SessionInputValue<'_>)> = vec![
                ("input_ids", decoder_ids.into()),
                ("encoder_attention_mask", encoder_mask.into()),
                ("encoder_hidden_states", encoder_hidden.into()),
            ];
            let outputs = decoder.run(inputs).map_err(failed)?;

            let (shape, logits) = outputs["logits"]
                .try_extract_tensor::<f32>()
                .map_err(failed)?;
            let vocabulary = *shape
                .last()
                .ok_or_else(|| TranslateError::Failed("shapeless logits".into()))?
                as usize;
            let last = logits
                .len()
                .checked_sub(vocabulary)
                .ok_or_else(|| TranslateError::Failed("too few logits".into()))?;

            let next = best_token(&logits[last..]);
            if next == END_OF_TEXT {
                break;
            }
            ids.push(next);
        }

        // Drop the start token before decoding; it is structure, not output.
        let produced: Vec<u32> = ids[1..].iter().map(|&id| id as u32).collect();
        self.tokenizer
            .decode(&produced, true)
            .map(|text| text.trim().to_string())
            .map_err(|error| TranslateError::Failed(error.to_string()))
    }

    fn engine(&self) -> &str {
        "fugumt"
    }
}

impl FuguMt {
    /// Beam search, for the experiment `project notes` §7 has been waiting on.
    ///
    /// **This is not what `translate` does, and must not become it without evidence.** §7 forbids
    /// beam search in strong terms, having watched it produce `水を飲む` →
    /// `"DRINK AWN THIS WAN THROUGH THROUED THROUED…"`. Three things make that finding worth
    /// re-testing rather than simply obeying:
    ///
    /// - it was observed **under Python transformers 5.x**, never in this decoder;
    /// - that output looks like a vocabulary mismatch, not beam degeneration — beam search on a
    ///   healthy Marian model gets blander and repetitive, it does not emit phonetic all-caps;
    /// - the model ships `generation_config.json` asking for **12 beams**, written by transformers
    ///   4.31, so its own authors expect this to work.
    ///
    /// `core/tests/beam_experiment.rs` runs it and reads the output. If it wins it earns a
    /// promotion; if it loses, §7 gets stronger for having been tested in the right runtime.
    ///
    /// **`BANNED` is applied per hypothesis**, which is the likeliest place a fresh implementation
    /// goes wrong: the pad token is also the decoder's start token, and a beam free to re-emit it
    /// scores brilliantly while producing nothing.
    pub fn translate_with_beams(&self, text: &str, beams: usize) -> Result<String, TranslateError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(TranslateError::EmptyInput);
        }
        let beams = beams.max(1);

        let encoded = self
            .tokenizer
            .encode(text, true)
            .map_err(|error| TranslateError::Failed(error.to_string()))?;
        let source: Vec<i64> = encoded.get_ids().iter().map(|&id| i64::from(id)).collect();
        if source.is_empty() {
            return Err(TranslateError::EmptyInput);
        }
        let mask: Vec<i64> = vec![1; source.len()];
        let source_shape = vec![1i64, source.len() as i64];

        let (hidden_shape, hidden): (Vec<i64>, Vec<f32>) = {
            let ids = TensorRef::<i64>::from_array_view((source_shape.clone(), source.as_slice()))
                .map_err(failed)?;
            let attention =
                TensorRef::<i64>::from_array_view((source_shape.clone(), mask.as_slice()))
                    .map_err(failed)?;
            let mut encoder = self.encoder.lock().unwrap_or_else(|p| p.into_inner());
            let outputs = encoder
                .run(vec![("input_ids", ids), ("attention_mask", attention)])
                .map_err(failed)?;
            let (shape, data) = outputs["last_hidden_state"]
                .try_extract_tensor::<f32>()
                .map_err(failed)?;
            (shape.to_vec(), data.to_vec())
        };

        /// One hypothesis: what it has written, and its summed log probability.
        struct Beam {
            ids: Vec<i64>,
            score: f64,
            done: bool,
        }

        let mut live = vec![Beam {
            ids: vec![DECODER_START],
            score: 0.0,
            done: false,
        }];
        let mut decoder = self.decoder.lock().unwrap_or_else(|p| p.into_inner());

        for _ in 0..MAX_TOKENS {
            if live.iter().all(|beam| beam.done) {
                break;
            }
            let mut next: Vec<Beam> = Vec::new();

            for beam in &live {
                if beam.done {
                    next.push(Beam {
                        ids: beam.ids.clone(),
                        score: beam.score,
                        done: true,
                    });
                    continue;
                }

                let decoder_ids = TensorRef::<i64>::from_array_view((
                    vec![1i64, beam.ids.len() as i64],
                    beam.ids.as_slice(),
                ))
                .map_err(failed)?;
                let encoder_mask =
                    TensorRef::<i64>::from_array_view((source_shape.clone(), mask.as_slice()))
                        .map_err(failed)?;
                let encoder_hidden =
                    TensorRef::<f32>::from_array_view((hidden_shape.clone(), hidden.as_slice()))
                        .map_err(failed)?;
                let inputs: Vec<(&str, SessionInputValue<'_>)> = vec![
                    ("input_ids", decoder_ids.into()),
                    ("encoder_attention_mask", encoder_mask.into()),
                    ("encoder_hidden_states", encoder_hidden.into()),
                ];
                let outputs = decoder.run(inputs).map_err(failed)?;
                let (shape, logits) = outputs["logits"]
                    .try_extract_tensor::<f32>()
                    .map_err(failed)?;
                let vocabulary = *shape.last().unwrap_or(&0) as usize;
                let last = logits.len().saturating_sub(vocabulary);
                let step = &logits[last..];

                // Log-softmax, so scores across hypotheses of different content are comparable.
                let top = step.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let sum: f64 = step.iter().map(|&x| ((x - top) as f64).exp()).sum();
                let normaliser = sum.ln();

                // **Top-k by partial selection, not a full sort.** The vocabulary is 32,001 wide
                // and this runs once per hypothesis per token: sorting all of it turned a
                // seven-sentence experiment into a fifteen-minute one. Only the best `beams`
                // entries are ever used, and `select_nth_unstable_by` finds them in linear time.
                let mut ranked: Vec<(usize, f32)> = step
                    .iter()
                    .copied()
                    .enumerate()
                    // Per hypothesis, every time. See the doc comment.
                    .filter(|(id, _)| *id as i64 != BANNED)
                    .collect();
                let wanted = beams.min(ranked.len());
                if wanted < ranked.len() {
                    ranked.select_nth_unstable_by(wanted, |a, b| b.1.total_cmp(&a.1));
                    ranked.truncate(wanted);
                }
                ranked.sort_by(|a, b| b.1.total_cmp(&a.1));

                for (id, score) in ranked.into_iter() {
                    let logprob = (score - top) as f64 - normaliser;
                    let mut ids = beam.ids.clone();
                    let finished = id as i64 == END_OF_TEXT;
                    if !finished {
                        ids.push(id as i64);
                    }
                    next.push(Beam {
                        ids,
                        score: beam.score + logprob,
                        done: finished,
                    });
                }
            }

            // Length-normalised, or the search simply prefers whichever hypothesis stopped first.
            next.sort_by(|a, b| {
                let left = a.score / (a.ids.len() as f64);
                let right = b.score / (b.ids.len() as f64);
                right.total_cmp(&left)
            });
            next.truncate(beams);
            live = next;
        }

        let best = live
            .iter()
            .max_by(|a, b| {
                let left = a.score / (a.ids.len() as f64);
                let right = b.score / (b.ids.len() as f64);
                left.total_cmp(&right)
            })
            .ok_or_else(|| TranslateError::Failed("no hypothesis".into()))?;

        let produced: Vec<u32> = best.ids[1..].iter().map(|&id| id as u32).collect();
        self.tokenizer
            .decode(&produced, true)
            .map(|text| text.trim().to_string())
            .map_err(|error| TranslateError::Failed(error.to_string()))
    }
}

/// Knobs on the greedy loop, for the experiment in `core/tests/fugumt_levers.rs`.
///
/// **`translate` does not use this and must not start.** `project notes` §7 fixed greedy decoding
/// after beam search was measured to produce word salad in two independent runtimes, and every
/// setting here is a change to the same loop. Anything that earns promotion gets promoted
/// deliberately, on a number, in the shipping path.
#[derive(Default, Clone)]
pub struct Knobs {
    /// Ban a token that would repeat an n-gram already produced. Zero switches it off.
    ///
    /// Standard practice in beam and greedy decoding alike, and untried here. The greedy loop has
    /// only `MAX_TOKENS` between it and a repetition that runs to the limit.
    pub no_repeat: usize,
    /// English the decoder is made to begin with, before it chooses anything of its own.
    ///
    /// The one lever that reaches *inside* this model. Its ONNX graph takes source tokens and a
    /// decoder prefix and nothing else — there is no prompt and no glossary slot — so a forced
    /// prefix is the only way the app's knowledge can influence generation at all.
    pub prefix: String,
}

impl FuguMt {
    /// Greedy decoding with the knobs applied. See [`Knobs`].
    pub fn translate_tuned(&self, text: &str, knobs: &Knobs) -> Result<String, TranslateError> {
        let text = for_the_model(text);
        let text = text.as_str();
        if text.is_empty() {
            return Err(TranslateError::EmptyInput);
        }

        let encoded = self
            .tokenizer
            .encode(text, true)
            .map_err(|error| TranslateError::Failed(error.to_string()))?;
        let source: Vec<i64> = encoded.get_ids().iter().map(|&id| i64::from(id)).collect();
        if source.is_empty() {
            return Err(TranslateError::EmptyInput);
        }
        let mask: Vec<i64> = vec![1; source.len()];
        let source_shape = vec![1i64, source.len() as i64];

        let (hidden_shape, hidden): (Vec<i64>, Vec<f32>) = {
            let ids = TensorRef::<i64>::from_array_view((source_shape.clone(), source.as_slice()))
                .map_err(failed)?;
            let attention =
                TensorRef::<i64>::from_array_view((source_shape.clone(), mask.as_slice()))
                    .map_err(failed)?;
            let mut encoder = self.encoder.lock().unwrap_or_else(|p| p.into_inner());
            let outputs = encoder
                .run(vec![("input_ids", ids), ("attention_mask", attention)])
                .map_err(failed)?;
            let (shape, data) = outputs["last_hidden_state"]
                .try_extract_tensor::<f32>()
                .map_err(failed)?;
            (shape.to_vec(), data.to_vec())
        };

        // The forced opening, encoded without special tokens: they are structure, and the decoder
        // already has its own start token below.
        let mut ids = vec![DECODER_START];
        let forced = if knobs.prefix.trim().is_empty() {
            0
        } else {
            let encoded = self
                .tokenizer
                .encode(knobs.prefix.trim(), false)
                .map_err(|error| TranslateError::Failed(error.to_string()))?;
            let prefix: Vec<i64> = encoded.get_ids().iter().map(|&id| i64::from(id)).collect();
            ids.extend(prefix.iter().copied());
            prefix.len()
        };

        let mut decoder = self.decoder.lock().unwrap_or_else(|p| p.into_inner());
        for _ in 0..MAX_TOKENS {
            let decoder_ids =
                TensorRef::<i64>::from_array_view((vec![1i64, ids.len() as i64], ids.as_slice()))
                    .map_err(failed)?;
            let encoder_mask =
                TensorRef::<i64>::from_array_view((source_shape.clone(), mask.as_slice()))
                    .map_err(failed)?;
            let encoder_hidden =
                TensorRef::<f32>::from_array_view((hidden_shape.clone(), hidden.as_slice()))
                    .map_err(failed)?;
            let inputs: Vec<(&str, SessionInputValue<'_>)> = vec![
                ("input_ids", decoder_ids.into()),
                ("encoder_attention_mask", encoder_mask.into()),
                ("encoder_hidden_states", encoder_hidden.into()),
            ];
            let outputs = decoder.run(inputs).map_err(failed)?;
            let (shape, logits) = outputs["logits"]
                .try_extract_tensor::<f32>()
                .map_err(failed)?;
            let vocabulary = *shape
                .last()
                .ok_or_else(|| TranslateError::Failed("shapeless logits".into()))?
                as usize;
            let last = logits
                .len()
                .checked_sub(vocabulary)
                .ok_or_else(|| TranslateError::Failed("too few logits".into()))?;

            let mut step = logits[last..].to_vec();
            // The pad token is the decoder's start token, so an unbanned loop can re-emit it and
            // produce nothing. `best_token` does this for the shipping path; here the slice is
            // owned, so it is done in place.
            if let Some(slot) = step.get_mut(BANNED as usize) {
                *slot = f32::NEG_INFINITY;
            }
            if knobs.no_repeat > 1 {
                for banned in would_repeat(&ids, knobs.no_repeat) {
                    if let Some(slot) = step.get_mut(banned as usize) {
                        *slot = f32::NEG_INFINITY;
                    }
                }
            }

            let next = step
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(id, _)| id as i64)
                .unwrap_or(END_OF_TEXT);
            if next == END_OF_TEXT {
                break;
            }
            ids.push(next);
        }

        // The start token is structure. The forced prefix is *output* — it is what the model was
        // made to say — so it stays in, or the answer would be missing its own opening.
        let _ = forced;
        let produced: Vec<u32> = ids[1..].iter().map(|&id| id as u32).collect();
        self.tokenizer
            .decode(&produced, true)
            .map(|text| text.trim().to_string())
            .map_err(|error| TranslateError::Failed(error.to_string()))
    }
}

/// Tokens that would complete an n-gram this generation has already produced.
///
/// The last `n - 1` tokens are the context; anything that followed that same context earlier is
/// banned. Standard no-repeat-ngram, and cheap: the scan is over what has been generated, which
/// is at most `MAX_TOKENS`.
fn would_repeat(ids: &[i64], n: usize) -> Vec<i64> {
    if n < 2 || ids.len() < n {
        return Vec::new();
    }
    let context = &ids[ids.len() + 1 - n..];
    let mut banned = Vec::new();
    for window in ids.windows(n) {
        if &window[..n - 1] == context {
            banned.push(window[n - 1]);
        }
    }
    banned
}

fn failed(error: ort::Error) -> TranslateError {
    TranslateError::Failed(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pad_token_can_never_be_generated() {
        // It is also the decoder's start token, so an unbanned greedy loop could
        // re-emit it forever and produce nothing.
        let mut logits = vec![0.0f32; 32001];
        logits[BANNED as usize] = 100.0;
        logits[42] = 1.0;
        assert_eq!(best_token(&logits), 42);
    }

    #[test]
    fn the_highest_scoring_token_wins_otherwise() {
        let mut logits = vec![0.0f32; 32001];
        logits[7] = 3.0;
        logits[9] = 5.0;
        assert_eq!(best_token(&logits), 9);
    }

    #[test]
    fn a_missing_model_is_reported_as_unavailable() {
        let missing = std::env::temp_dir().join("sakatsu-no-such-mt-model");
        assert!(!FuguMt::is_present(&missing));

        let error = FuguMt::load(&missing).map(|_| ()).expect_err("no model");
        assert!(matches!(error, TranslateError::EngineUnavailable(_)));
        assert!(error.to_string().contains("tokenizer.json"));
    }

    #[test]
    fn a_null_precompiled_normalizer_is_dropped_rather_than_refused() {
        // Exactly what the published export contains, and what the tokenizers crate
        // rejects outright.
        let dir = std::env::temp_dir().join("sakatsu-tokenizer-fix-test");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("tokenizer.json");
        std::fs::write(
            &path,
            r#"{
                "version": "1.0",
                "truncation": null,
                "padding": null,
                "added_tokens": [],
                "normalizer": { "type": "Precompiled", "precompiled_charsmap": null },
                "pre_tokenizer": { "type": "WhitespaceSplit" },
                "post_processor": null,
                "decoder": null,
                "model": { "type": "WordLevel", "vocab": { "a": 0 }, "unk_token": "a" }
            }"#,
        )
        .expect("write");

        let tokenizer = load_tokenizer(&path).expect("the null normalizer is dropped");
        assert_eq!(tokenizer.get_vocab_size(true), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// **The measured win, pinned.** `fugumt_levers.rs` put this at 22 sentences to 4
    /// (sign test p < 0.001): a full-width digit was tokenised as something the decoder then
    /// dropped, so `６人` lost its six entirely.
    #[test]
    fn full_width_latin_is_folded_because_the_model_loses_it_otherwise() {
        assert_eq!(
            for_the_model("６人宿泊させて欲しい。"),
            "6人宿泊させて欲しい。"
        );
        assert_eq!(for_the_model("１．とりむね肉"), "1.とりむね肉");
        assert_eq!(for_the_model("あ！まずい！"), "あ!まずい!");
        assert_eq!(for_the_model("ＡＢＣ"), "ABC");
    }

    /// The Latin range only. Japanese punctuation and the long-vowel mark sit outside it and carry
    /// meaning — folding 。 to a full stop would be rewriting the sentence rather than re-encoding
    /// it, and ー is a letter's length, not a dash.
    #[test]
    fn japanese_punctuation_and_the_long_vowel_mark_are_left_alone() {
        assert_eq!(for_the_model("コーヒー、そして。"), "コーヒー、そして。");
        assert_eq!(for_the_model("犬・猫"), "犬・猫");
        // Deliberately *not* folded onto ー. An earlier draft did that; it fired on none of the
        // 300 measured sentences, so it was a rule with no evidence behind it.
        assert_eq!(for_the_model("長い—線"), "長い—線");
    }

    /// **A break mid-sentence cost more than the whole measured gap to a 7B instruct model** —
    /// cosine 0.856 against 0.827, losing 137 sentences to 43. Joining with *nothing* is the fix:
    /// Japanese has no word spaces, so a space here would invent a boundary nobody wrote.
    #[test]
    fn a_captured_line_break_is_joined_with_nothing() {
        assert_eq!(
            for_the_model("気持ちが\n伝わってきます。"),
            "気持ちが伝わってきます。"
        );
        assert_eq!(
            for_the_model("気持ちが\r\n伝わってきます。"),
            "気持ちが伝わってきます。"
        );
        assert_eq!(for_the_model("  \n気持ち\n\n伝わる\n  "), "気持ち伝わる");
    }

    #[test]
    fn an_ideographic_space_becomes_an_ordinary_one_and_the_edges_are_trimmed() {
        assert_eq!(for_the_model("\u{3000}あ\u{3000}い\u{3000}"), "あ い");
    }

    #[test]
    fn nothing_at_all_stays_nothing() {
        assert_eq!(for_the_model(""), "");
        assert_eq!(for_the_model("   \n  "), "");
    }
}
