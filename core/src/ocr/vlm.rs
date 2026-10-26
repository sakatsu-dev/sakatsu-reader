//! A vision-language model, over an OpenAI-compatible endpoint.
//!
//! # Why this one is not in-process
//!
//! `project notes` §2 runs models in-process through ONNX Runtime, and both other engines do.
//! PaddleOCR-VL-For-Manga cannot: it ships as safetensors with no ONNX export, and it is
//! autoregressive — even exported, running it would mean a generation loop with a KV cache
//! over a 1B model.
//!
//! What it *does* have is GGUF weights and upstream llama.cpp support. This core adapter therefore
//! talks over the OpenAI chat-completions shape that llama-server, LM Studio and Ollama all speak.
//! The desktop normally downloads and supervises an app-owned llama.cpp runtime; Advanced settings
//! may instead point this same adapter at an external compatible server. Process ownership stays in
//! the Tauri shell, while this platform-independent layer keeps the existing `ureq` HTTP boundary.
//!
//! ```text
//! llama-server -m PaddleOCR-VL-For-Manga-BF16.gguf \
//!              --mmproj PaddleOCR-VL-For-Manga-mmproj-BF16.gguf \
//!              --port 9999 --temp 0
//! ```
//!
//! The *general* PaddleOCR-VL publishes no GGUF, so it needs vLLM instead — same
//! OpenAI shape, so nothing here changes. Two environment variables are not optional
//! under WSL2, and both cost an hour to find:
//!
//! ```text
//! VLLM_WSL2_ENABLE_PIN_MEMORY=1 VLLM_USE_FLASHINFER_SAMPLER=0 \
//!   vllm serve PaddlePaddle/PaddleOCR-VL-1.6 --port 8000 --gpu-memory-utilization 0.5
//! ```
//!
//! - **Pinned memory** is supported on WSL2 kernels ≥ 4.19.121 but *off by default*, and
//!   vLLM's V2 model runner requires UVA, which requires it. Without the flag the engine
//!   dies at startup with `RuntimeError: UVA is not available`.
//! - **FlashInfer's sampler** JIT-compiles a CUDA kernel and needs `nvcc`, which the pip
//!   wheels do not ship. Switching it off costs nothing here: `temperature` is zero, so
//!   the top-k/top-p path it accelerates is never taken.
//!
//! # What this costs, and the failure mode it brings
//!
//! A 1B autoregressive model is seconds per capture on CPU, against PP-OCRv5's measured
//! 0.05–0.19 s. And its mistakes are a different *kind*: a CTC recogniser that is unsure
//! emits garbled characters, which look wrong. A language model that is unsure emits
//! fluent, plausible text that is not what the image says. For a tool whose whole purpose
//! is looking words up, a confident hallucination is worse than visible garbage — which is
//! why `temperature` is pinned at zero here and why this is a choice the user makes rather
//! than a default.

use std::time::Duration;

use serde_json::Value;

use super::{Ocr, OcrError, OcrImage};

/// How long to wait for a reply.
///
/// Generous on purpose. This is a 1B model that may be running on a CPU, and timing out on
/// a read that was going to succeed is worse than waiting: the capture is gone by then.
const TIMEOUT: Duration = Duration::from_secs(180);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const RESPONSE_LIMIT: u64 = 2 * 1024 * 1024;

/// The prompt the model was tuned for.
const PROMPT: &str = "OCR:";

/// The same prompt with an explicit media marker.
///
/// The model card documents this: some llama.cpp builds fail with `number of bitmaps (1)
/// does not match number of markers (0)` because nothing in the prompt says where the
/// image goes. Rather than make the user discover that and edit a setting, the first
/// failure that looks like it is retried this way.
const PROMPT_WITH_MARKER: &str = "<__media__>OCR:";

pub struct VlmOcr {
    endpoint: String,
    model: String,
    agent: ureq::Agent,
}

impl VlmOcr {
    /// Point at a server's base URL, e.g. `http://127.0.0.1:9999`.
    pub fn new(endpoint: &str, model: &str) -> Result<VlmOcr, OcrError> {
        let endpoint = endpoint.trim().trim_end_matches('/');
        let uri: ureq::http::Uri = endpoint.parse().map_err(|error| {
            OcrError::EngineUnavailable(format!("the VLM endpoint is not a valid URL: {error}"))
        })?;
        if !matches!(uri.scheme_str(), Some("http" | "https"))
            || uri.authority().is_none()
            || uri.query().is_some()
            || uri
                .authority()
                .is_some_and(|authority| authority.as_str().contains('@'))
        {
            return Err(OcrError::EngineUnavailable(
                "the VLM endpoint must be an http(s) base URL without credentials or a query"
                    .into(),
            ));
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build()
            .into();
        Ok(VlmOcr {
            endpoint: endpoint.to_string(),
            model: if model.is_empty() {
                // llama-server serves whatever it was started with and ignores this, but
                // the field is required by the shape and other servers do route on it.
                "default".to_string()
            } else {
                model.to_string()
            },
            agent,
        })
    }

    fn url(&self) -> String {
        format!("{}/v1/chat/completions", self.endpoint)
    }

    /// Whether the server answers at all.
    ///
    /// The other two engines answer "is it installed" by looking for files. This one can
    /// only answer by asking, so it asks for the model list — cheap, and does not load
    /// anything or run inference.
    pub fn is_reachable(&self) -> bool {
        let probe: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(PROBE_TIMEOUT))
            .build()
            .into();
        probe
            .get(format!("{}/v1/models", self.endpoint))
            .call()
            .is_ok()
    }

    fn ask(&self, data_url: &str, prompt: &str) -> Result<String, OcrError> {
        // This is a network call to somebody else's process that can legitimately take a
        // minute. Without a line before and after, a slow read, a hung one and a server
        // that is not there are the same event from the outside: nothing happening.
        crate::log_info!(
            "vlm",
            "asking {} for {} — {} kB of image, prompt {prompt:?}",
            self.endpoint,
            self.model,
            data_url.len() / 1024
        );
        let started = std::time::Instant::now();
        let body = serde_json::json!({
            "model": self.model,
            // Zero, always. Sampling an OCR result is asking the model to invent a
            // reading, and a plausible invention is the failure this engine is most prone
            // to and least able to signal.
            "temperature": 0,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image_url", "image_url": { "url": data_url } },
                    { "type": "text", "text": prompt },
                ],
            }],
        });

        let response = self
            .agent
            .post(self.url())
            .send_json(&body)
            .map_err(|error| {
                crate::log_error!(
                    "vlm",
                    "no answer from {} after {:.1}s: {error}",
                    self.endpoint,
                    started.elapsed().as_secs_f64()
                );
                OcrError::EngineUnavailable(format!(
                    "no answer from {} ({error}) — is the server running?",
                    self.endpoint
                ))
            })?
            .body_mut()
            .with_config()
            .limit(RESPONSE_LIMIT)
            .read_to_string()
            .map_err(|error| OcrError::Failed(format!("could not read the reply: {error}")))?;

        crate::log_info!(
            "vlm",
            "replied in {:.1}s, {} bytes",
            started.elapsed().as_secs_f64(),
            response.len()
        );
        extract(&response)
    }
}

impl Ocr for VlmOcr {
    fn recognize(&self, image: &OcrImage) -> Result<String, OcrError> {
        if image.is_empty() {
            return Err(OcrError::EmptyImage);
        }

        let data_url = format!("data:image/png;base64,{}", base64(&png(image)?));

        match self.ask(&data_url, PROMPT) {
            Ok(text) => Ok(text.trim().to_string()),
            Err(error) if looks_like_a_missing_marker(&error) => {
                crate::log_warn!(
                    "vlm",
                    "the server wanted an explicit media marker; retrying with {PROMPT_WITH_MARKER:?}"
                );
                self.ask(&data_url, PROMPT_WITH_MARKER)
                    .map(|text| text.trim().to_string())
            }
            Err(error) => Err(error),
        }
    }

    fn engine(&self) -> &str {
        "vlm"
    }
}

/// Whether a failure is the documented "the prompt did not say where the image goes" one.
fn looks_like_a_missing_marker(error: &OcrError) -> bool {
    let message = error.to_string();
    message.contains("bitmaps") || message.contains("markers")
}

/// Pull the text out of a chat-completions reply.
///
/// A server that answers with an `error` object is reported as an engine failure rather
/// than as an empty read: "nothing was recognized" and "the model refused" are different
/// things and only one of them is worth reframing the capture over.
fn extract(body: &str) -> Result<String, OcrError> {
    let reply: Value =
        serde_json::from_str(body).map_err(|error| OcrError::Failed(format!("{error}: {body}")))?;

    if let Some(message) = reply
        .get("error")
        .and_then(|error| error.get("message").or(Some(error)))
    {
        return Err(OcrError::Failed(message.to_string()));
    }

    reply
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| OcrError::Failed(format!("no text in the reply: {body}")))
}

/// Encode as PNG, which is what the data URL carries.
///
/// PNG rather than JPEG: this is text at the size it was captured, and JPEG's ringing
/// around a hard glyph edge is exactly the artefact that costs a recogniser a character.
fn png(image: &OcrImage) -> Result<Vec<u8>, OcrError> {
    let source = image::RgbaImage::from_raw(image.width(), image.height(), image.rgba().to_vec())
        .ok_or_else(|| OcrError::MalformedImage("could not read the pixels".into()))?;

    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(source)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .map_err(|error| OcrError::Failed(format!("could not encode the image: {error}")))?;
    Ok(bytes)
}

/// Standard base64, with padding.
///
/// Written here rather than taken as a dependency: `base64` is already in the tree twice
/// through other crates' transitive requirements, and adding a direct one would compile a
/// third copy for twenty lines. The RFC 4648 vectors are below — this is not a place
/// subtle bugs hide, because a wrong byte breaks the image loudly rather than quietly.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let triple = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(triple >> 18) as usize & 0x3F] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3F] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 0x3F] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 0x3F] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_vectors() {
        // RFC 4648 section 10, which exercises every padding case.
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected, "encoding {input:?}");
        }
    }

    #[test]
    fn base64_handles_bytes_that_are_not_text() {
        // A PNG is binary, and the high bit is where a sloppy shift goes wrong.
        assert_eq!(base64(&[0xFF, 0xFF, 0xFF]), "////");
        assert_eq!(base64(&[0x00, 0x00, 0x00]), "AAAA");
        assert_eq!(base64(&[0x89, 0x50, 0x4E, 0x47]), "iVBORw==");
    }

    #[test]
    fn a_reply_gives_up_its_text() {
        let body =
            r#"{"choices":[{"message":{"role":"assistant","content":"気持ちが伝わってきます"}}]}"#;
        assert_eq!(extract(body).expect("reads"), "気持ちが伝わってきます");
    }

    #[test]
    fn a_server_error_is_an_engine_failure_and_not_an_empty_read() {
        let body = r#"{"error":{"message":"number of bitmaps (1) does not match number of markers (0)","code":500}}"#;
        let error = extract(body).expect_err("refuses");
        assert!(
            looks_like_a_missing_marker(&error),
            "the documented marker failure has to be recognizable to retry it, got {error}"
        );
    }

    #[test]
    fn a_reply_with_no_text_says_so_rather_than_reading_as_blank() {
        // An empty string means "nothing in the image". A malformed reply means the
        // server is not what we think it is, and silently agreeing would hide that.
        for body in [
            r#"{}"#,
            r#"{"choices":[]}"#,
            r#"{"choices":[{"message":{}}]}"#,
        ] {
            assert!(
                extract(body).is_err(),
                "{body} should not read as empty text"
            );
        }
    }

    #[test]
    fn a_trailing_slash_does_not_double_up() {
        assert_eq!(
            VlmOcr::new("http://127.0.0.1:9999/", "")
                .expect("endpoint")
                .url(),
            "http://127.0.0.1:9999/v1/chat/completions"
        );
    }

    #[test]
    fn an_empty_image_is_refused_before_any_request() {
        let engine = VlmOcr::new("http://127.0.0.1:1", "").expect("endpoint");
        let empty = OcrImage::new(0, 0, Vec::new()).expect("a zero-sized image is well-formed");
        assert!(matches!(
            engine.recognize(&empty),
            Err(OcrError::EmptyImage)
        ));
    }

    #[test]
    fn only_plain_http_base_urls_are_accepted() {
        for endpoint in [
            "file:///tmp/model",
            "not a url",
            "http://user:secret@127.0.0.1:9999",
            "http://127.0.0.1:9999?route=other",
        ] {
            assert!(VlmOcr::new(endpoint, "").is_err(), "accepted {endpoint}");
        }
        assert!(VlmOcr::new("https://models.example.test/openai", "model").is_ok());
    }
}
