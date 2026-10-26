//! manga-ocr through ONNX Runtime.
//!
//! An encoder–decoder: a ViT reads the image, a BERT-style decoder writes the text.
//! Both halves run in-process via `ort`, so there is no Python and no sidecar
//! (`project notes` §2).
//!
//! ## The graph, as read from the files rather than assumed
//!
//! ```text
//! encoder: pixel_values          f32 [batch, 3, 224, 224]
//!       →  last_hidden_state     f32 [batch, 197, 768]
//!
//! decoder: input_ids             i64 [batch, seq]
//!          encoder_hidden_states f32 [batch, 197, 768]
//!       →  logits                f32 [batch, seq, 6144]
//! ```
//!
//! 197 is 196 patches (224/16 squared) plus the ViT class token. 6144 is the
//! vocabulary size, and it matches `vocab.txt` exactly.
//!
//! **The decoder takes no cache inputs.** There are no `past_key_values`, so each step
//! re-runs the whole prefix. That is quadratic in output length, which for a line of
//! manga dialogue is nothing — and it makes the loop simple enough to be obviously
//! correct.
//!
//! ## Decoding
//!
//! Greedy, plus the `no_repeat_ngram_size: 3` constraint from the model's own
//! generation config. The config also asks for `num_beams: 4`; beam search is left
//! out for now as considerably more code for a small gain on short lines. Unlike the
//! translation case (`project notes` §7), where beam search actively degenerates, here it
//! is only a refinement — but it *is* the model's default, so output may differ
//! slightly from the reference implementation on ambiguous crops.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ort::session::{Session, SessionInputValue};
use ort::value::TensorRef;

use super::{Ocr, OcrError, OcrImage};

/// The side length the model expects, from `preprocessor_config.json`.
const IMAGE_SIZE: u32 = 224;

/// Token ids, from `generation_config.json` and confirmed against `vocab.txt`:
/// id 2 is `[CLS]`, id 3 is `[SEP]`.
const DECODER_START: i64 = 2;
const END_OF_TEXT: i64 = 3;

/// From `generation_config.json`. A hard stop as much as a limit: without one, a
/// decoder that starts repeating never finishes.
const MAX_TOKENS: usize = 300;

/// From `generation_config.json`. Bans a token that would repeat an existing 3-gram,
/// which is what stops greedy decoding looping on a blurry crop.
const NO_REPEAT_NGRAM: usize = 3;

/// manga-ocr, loaded from a directory of ONNX files.
pub struct MangaOcr {
    /// `run` needs `&mut`, and [`Ocr::recognize`] takes `&self`, so the sessions sit
    /// behind mutexes. Recognition is serialized, which is right anyway: two
    /// concurrent inferences would only fight over the same CPU.
    encoder: Mutex<Session>,
    decoder: Mutex<Session>,
    /// Token id → token text. Indexed by position, so `vocab[3]` is `[SEP]`.
    vocab: Vec<String>,
}

impl MangaOcr {
    /// Load the model from `dir`, as `scripts/fetch-ocr-model.ps1` lays it out.
    pub fn load(dir: &Path) -> Result<MangaOcr, OcrError> {
        let vocab_path = dir.join("vocab.txt");
        let vocab: Vec<String> = std::fs::read_to_string(&vocab_path)
            .map_err(|error| {
                OcrError::EngineUnavailable(format!("{}: {error}", vocab_path.display()))
            })?
            .lines()
            .map(|line| line.trim_end_matches('\r').to_string())
            .collect();
        if vocab.is_empty() {
            return Err(OcrError::EngineUnavailable(format!(
                "{} is empty",
                vocab_path.display()
            )));
        }

        Ok(MangaOcr {
            encoder: Mutex::new(session(&dir.join("encoder_model.onnx"))?),
            decoder: Mutex::new(session(&dir.join("decoder_model.onnx"))?),
            vocab,
        })
    }

    /// Whether a directory holds the files [`MangaOcr::load`] needs.
    pub fn is_present(dir: &Path) -> bool {
        ["encoder_model.onnx", "decoder_model.onnx", "vocab.txt"]
            .iter()
            .all(|name| dir.join(name).is_file())
    }
}

/// Turn token ids into text.
///
/// WordPiece, so `##` marks a continuation and is dropped. Every remaining space is
/// removed as well: the tokenizer inserts them between pieces and Japanese does not use
/// them, which is exactly what the reference implementation does.
///
/// A free function rather than a method so it can be tested without a loaded model.
fn detokenize(vocab: &[String], ids: &[i64]) -> String {
    let mut text = String::new();
    for &id in ids {
        let Some(token) = vocab.get(id as usize) else {
            continue;
        };
        // Special tokens are structure, not content.
        if token.starts_with('[') && token.ends_with(']') {
            continue;
        }
        text.push_str(token.strip_prefix("##").unwrap_or(token));
    }
    text.retain(|character| !character.is_whitespace());
    text
}

fn session(path: &PathBuf) -> Result<Session, OcrError> {
    if !path.is_file() {
        return Err(OcrError::EngineUnavailable(format!(
            "{} is missing — run scripts/fetch-ocr-model.ps1",
            path.display()
        )));
    }
    crate::onnx::session_builder()
        .and_then(|mut builder| builder.commit_from_file(path))
        .map_err(|error| {
            OcrError::EngineUnavailable(format!("could not load {}: {error}", path.display()))
        })
}

/// Image → the encoder's `pixel_values`.
///
/// Faithful to manga-ocr's own preprocessing, which matters more than it looks: the
/// model was trained on **greyscale converted back to three channels**, so feeding it
/// colour would be feeding it something it has never seen. Then bilinear resize to
/// 224×224, scale to 0–1, and normalize with mean and standard deviation 0.5, which
/// lands the values in −1..1.
fn pixel_values(image: &OcrImage) -> Result<Vec<f32>, OcrError> {
    let source = image::RgbaImage::from_raw(image.width(), image.height(), image.rgba().to_vec())
        .ok_or_else(|| OcrError::MalformedImage("could not read the pixels".into()))?;

    let grey = image::DynamicImage::ImageRgba8(source).to_luma8();
    let resized = image::imageops::resize(
        &grey,
        IMAGE_SIZE,
        IMAGE_SIZE,
        // `resample: 2` in preprocessor_config.json is PIL's BILINEAR.
        image::imageops::FilterType::Triangle,
    );

    // NCHW, with the single grey channel repeated three times.
    let pixels = (IMAGE_SIZE * IMAGE_SIZE) as usize;
    let mut values = Vec::with_capacity(pixels * 3);
    for _ in 0..3 {
        values.extend(
            resized
                .pixels()
                .map(|pixel| f32::from(pixel[0]) / 255.0 * 2.0 - 1.0),
        );
    }
    Ok(values)
}

/// The token ids that would repeat an existing 3-gram if generated next.
fn banned_by_ngram(ids: &[i64]) -> Vec<i64> {
    if ids.len() < NO_REPEAT_NGRAM {
        return Vec::new();
    }
    let prefix = &ids[ids.len() - (NO_REPEAT_NGRAM - 1)..];
    ids.windows(NO_REPEAT_NGRAM)
        .filter(|window| &window[..NO_REPEAT_NGRAM - 1] == prefix)
        .map(|window| window[NO_REPEAT_NGRAM - 1])
        .collect()
}

/// The highest-scoring token that is not banned.
fn best_token(logits: &[f32], banned: &[i64]) -> i64 {
    let mut best = (f32::NEG_INFINITY, 0i64);
    for (id, &score) in logits.iter().enumerate() {
        let id = id as i64;
        if banned.contains(&id) {
            continue;
        }
        if score > best.0 {
            best = (score, id);
        }
    }
    best.1
}

impl Ocr for MangaOcr {
    fn recognize(&self, image: &OcrImage) -> Result<String, OcrError> {
        if image.is_empty() {
            return Err(OcrError::EmptyImage);
        }

        let _timing = crate::log::Timing::start(
            "manga-ocr",
            format!("recognising {}x{}", image.width(), image.height()),
        );

        let values = pixel_values(image)?;
        let encoded: Vec<f32> = {
            let pixels = TensorRef::<f32>::from_array_view((
                vec![1i64, 3, i64::from(IMAGE_SIZE), i64::from(IMAGE_SIZE)],
                values.as_slice(),
            ))
            .map_err(failed)?;

            let mut encoder = self.encoder.lock().unwrap_or_else(|p| p.into_inner());
            let outputs = encoder
                .run(vec![("pixel_values", pixels)])
                .map_err(failed)?;
            let (_, data) = outputs["last_hidden_state"]
                .try_extract_tensor::<f32>()
                .map_err(failed)?;
            // Copied out because the borrow ends with the session lock, and the
            // decoder needs it on every step.
            data.to_vec()
        };

        let hidden = 768i64;
        let encoder_shape = vec![1i64, encoded.len() as i64 / hidden, hidden];

        let mut ids = vec![DECODER_START];
        let mut decoder = self.decoder.lock().unwrap_or_else(|p| p.into_inner());

        for _ in 0..MAX_TOKENS {
            let input_ids =
                TensorRef::<i64>::from_array_view((vec![1i64, ids.len() as i64], ids.as_slice()))
                    .map_err(failed)?;
            let hidden_states =
                TensorRef::<f32>::from_array_view((encoder_shape.clone(), encoded.as_slice()))
                    .map_err(failed)?;

            // Named explicitly: the two tensors have different element types, so the
            // pairs need a common value type rather than an inferred one.
            let inputs: Vec<(&str, SessionInputValue<'_>)> = vec![
                ("input_ids", input_ids.into()),
                ("encoder_hidden_states", hidden_states.into()),
            ];
            let outputs = decoder.run(inputs).map_err(failed)?;

            let (shape, logits) = outputs["logits"]
                .try_extract_tensor::<f32>()
                .map_err(failed)?;
            let vocabulary = *shape
                .last()
                .ok_or_else(|| OcrError::Failed("the decoder returned a shapeless tensor".into()))?
                as usize;
            // Only the last position predicts the next token; the earlier ones are
            // predictions for tokens already decided.
            let last = logits
                .len()
                .checked_sub(vocabulary)
                .ok_or_else(|| OcrError::Failed("the decoder returned too few logits".into()))?;

            let next = best_token(&logits[last..], &banned_by_ngram(&ids));
            if next == END_OF_TEXT {
                break;
            }
            ids.push(next);
        }

        Ok(detokenize(&self.vocab, &ids))
    }

    fn engine(&self) -> &str {
        "manga-ocr"
    }
}

fn failed(error: ort::Error) -> OcrError {
    OcrError::Failed(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocessing_produces_three_normalized_channels() {
        // A 2x1 image: one black pixel, one white.
        let image = OcrImage::new(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]).expect("valid");
        let values = pixel_values(&image).expect("preprocessed");

        let pixels = (IMAGE_SIZE * IMAGE_SIZE) as usize;
        assert_eq!(values.len(), pixels * 3, "NCHW with three channels");

        // Normalized to -1..1, and the three channels are identical because the model
        // was trained on greyscale widened back to RGB.
        assert!(values.iter().all(|value| (-1.0..=1.0).contains(value)));
        assert_eq!(values[..pixels], values[pixels..pixels * 2]);
        assert_eq!(values[..pixels], values[pixels * 2..]);

        // Black became -1 and white +1 somewhere in there.
        let min = values.iter().copied().fold(f32::INFINITY, f32::min);
        let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(min < -0.9, "black should be near -1, got {min}");
        assert!(max > 0.9, "white should be near +1, got {max}");
    }

    #[test]
    fn the_ngram_ban_blocks_a_repeat_and_nothing_else() {
        // "a b c a b" — generating c again would repeat the 3-gram "a b c".
        let ids = [10, 11, 12, 10, 11];
        assert_eq!(banned_by_ngram(&ids), [12]);

        // Too short to have a 3-gram yet.
        assert!(banned_by_ngram(&[10, 11]).is_empty());
        // No repeated prefix, so nothing is banned.
        assert!(banned_by_ngram(&[10, 11, 12, 13, 14]).is_empty());
    }

    #[test]
    fn the_best_token_skips_banned_ones() {
        let logits = [0.1, 5.0, 2.0];
        assert_eq!(best_token(&logits, &[]), 1);
        // With the winner banned, the runner-up takes it.
        assert_eq!(best_token(&logits, &[1]), 2);
        assert_eq!(best_token(&logits, &[1, 2]), 0);
    }

    #[test]
    fn detokenizing_joins_wordpieces_and_drops_special_tokens() {
        let vocab: Vec<String> = ["[PAD]", "[UNK]", "[CLS]", "[SEP]", "気", "##持", "##ち"]
            .iter()
            .map(ToString::to_string)
            .collect();

        // [CLS] 気 ##持 ##ち [SEP] — the wordpieces join, the specials vanish.
        assert_eq!(detokenize(&vocab, &[2, 4, 5, 6, 3]), "気持ち");
        // An id past the end of the vocabulary is skipped rather than panicking: a
        // decoder that emits nonsense should not take the app down.
        assert_eq!(detokenize(&vocab, &[4, 9999]), "気");
        assert_eq!(detokenize(&vocab, &[]), "");
    }

    #[test]
    fn a_missing_model_is_reported_as_unavailable() {
        let missing = std::env::temp_dir().join("sakatsu-no-such-model");
        assert!(!MangaOcr::is_present(&missing));

        let error = MangaOcr::load(&missing)
            .map(|_| ())
            .expect_err("no model there");
        assert!(matches!(error, OcrError::EngineUnavailable(_)), "{error:?}");
        assert!(error.to_string().contains("vocab.txt"));
    }
}
