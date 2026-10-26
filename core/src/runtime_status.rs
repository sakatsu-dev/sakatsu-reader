//! What the app is running, and what the last operation of each kind actually used.
//!
//! Two kinds of fact, kept apart because they change for different reasons:
//!
//! - **Engine state** is what is configured and loaded *now*. Opening Settings or changing the
//!   recognition engine changes it; reading a passage does not.
//! - **Operation results** are what one recognition, analysis or translation request actually
//!   used. Each kind has its own record and its own request identity, so a cached translation
//!   never makes the analysis look cached, and a capture's engine is never shown beside text that
//!   came from somewhere else.
//!
//! # Acceleration is evidence, not inference
//!
//! [`Acceleration`] says only what something running reported. The in-process ONNX models report
//! the execution providers [`crate::onnx`] configured — at present none, so the processor. The
//! app-managed llama.cpp server reports through its startup lines, which [`LlamaFacts`] reads:
//! "offloaded 29/29 layers to GPU" is shown as GPU acceleration *with that layer count*, never
//! as "the whole pipeline runs on the GPU", because the multimodal projector and the sampling
//! need not. Anything without such a report is [`Acceleration::Unknown`].

use serde::{Deserialize, Serialize};

use crate::model::FallbackReason;
use crate::request::RequestIdentity;

/// Which of the three jobs an engine or a result belongs to.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineRole {
    Recognition,
    Analysis,
    Translation,
}

/// Where an engine runs.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Hosting {
    /// Inside this process.
    InProcess,
    /// A local program the app starts and stops itself.
    ManagedLocal,
    /// A service the app does not run. `external` unless its address is this computer's own.
    Endpoint { external: bool },
}

/// Whether an engine is ready to use.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    /// Nothing for this role is installed.
    NotInstalled,
    /// Installed and not loaded yet: the next use loads it.
    NotLoaded,
    /// Loading now.
    Loading,
    /// Loaded and ready.
    Ready,
}

/// What a running engine reported about where it computes.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Acceleration {
    /// Built with no GPU execution provider, so the processor does the work.
    Cpu,
    /// llama.cpp reported offloading this many of its layers to a GPU. Not a claim about the rest
    /// of the pipeline.
    GpuLayers {
        offloaded: u32,
        total: u32,
        /// The device llama.cpp named, already reduced to a safe label.
        device: Option<String>,
    },
    /// Nothing reported where it computes.
    Unknown,
    /// Nothing is running to report.
    NotRunning,
}

impl Acceleration {
    /// The short phrase the interface and the diagnostic report use.
    pub fn phrase(&self) -> String {
        match self {
            Acceleration::Cpu => "CPU".to_string(),
            Acceleration::GpuLayers {
                offloaded, total, ..
            } => {
                format!("GPU acceleration active ({offloaded}/{total} layers)")
            }
            Acceleration::Unknown => "device unknown".to_string(),
            Acceleration::NotRunning => "not running".to_string(),
        }
    }
}

/// One engine, as it stands now.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStatus {
    pub role: EngineRole,
    /// What the settings ask for, e.g. `"auto"` or `"automatic"`.
    pub configured: String,
    /// What is loaded, when something is.
    pub loaded: Option<String>,
    pub hosting: Hosting,
    pub state: EngineState,
    pub acceleration: Acceleration,
}

/// Whether a result was computed for this request or reused.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheUse {
    /// Computed for this request.
    Fresh,
    /// Every part was reused from an earlier request.
    Cached,
    /// Some parts reused, some computed.
    Mixed,
}

impl CacheUse {
    /// Combine the cache use of the parts of one request.
    pub fn combine(parts: impl IntoIterator<Item = bool>) -> CacheUse {
        let (mut cached, mut fresh) = (false, false);
        for part in parts {
            if part {
                cached = true;
            } else {
                fresh = true;
            }
        }
        match (cached, fresh) {
            (true, false) => CacheUse::Cached,
            (true, true) => CacheUse::Mixed,
            _ => CacheUse::Fresh,
        }
    }
}

/// Why an operation failed, as a code. Never a message: messages can carry paths and text.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    EmptyInput,
    MalformedInput,
    EngineUnavailable,
    EngineFailed,
    Cancelled,
}

impl FailureCode {
    pub fn code(self) -> &'static str {
        match self {
            FailureCode::EmptyInput => "empty_input",
            FailureCode::MalformedInput => "malformed_input",
            FailureCode::EngineUnavailable => "engine_unavailable",
            FailureCode::EngineFailed => "engine_failed",
            FailureCode::Cancelled => "cancelled",
        }
    }
}

impl From<&crate::ocr::OcrError> for FailureCode {
    fn from(error: &crate::ocr::OcrError) -> Self {
        use crate::ocr::OcrError;
        match error {
            OcrError::EmptyImage => FailureCode::EmptyInput,
            OcrError::MalformedImage(_) => FailureCode::MalformedInput,
            OcrError::EngineUnavailable(_) => FailureCode::EngineUnavailable,
            OcrError::Failed(_) => FailureCode::EngineFailed,
        }
    }
}

impl From<&crate::translate::TranslateError> for FailureCode {
    fn from(error: &crate::translate::TranslateError) -> Self {
        use crate::translate::TranslateError;
        match error {
            TranslateError::EmptyInput => FailureCode::EmptyInput,
            TranslateError::EngineUnavailable(_) => FailureCode::EngineUnavailable,
            TranslateError::Failed(_) => FailureCode::EngineFailed,
        }
    }
}

/// How one request ended.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Failed { code: FailureCode },
}

/// What one request of one kind actually used.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationResult {
    pub role: EngineRole,
    /// The request this answered. The interface shows a result only for the request it owns.
    pub request: Option<RequestIdentity>,
    /// The engine that produced it — for Auto recognition, the model Auto picked.
    pub engine: String,
    /// For analysis: why the preferred analyzer did not read this passage.
    pub fallback: Option<FallbackReason>,
    pub cache: CacheUse,
    pub outcome: Outcome,
    /// What the engine reported about where it computed, at the time of this request.
    pub acceleration: Acceleration,
}

/// Everything the status display and the diagnostic report read.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub engines: Vec<EngineStatus>,
    pub recognition: Option<OperationResult>,
    pub analysis: Option<OperationResult>,
    pub translation: Option<OperationResult>,
}

/// The facts llama.cpp's startup lines establish, and nothing else.
///
/// Read line by line from the server's standard error. Each line is matched against two exact
/// shapes from the pinned release (b10644, `llama.cpp` and `llama-model.cpp`):
///
/// ```text
/// load_tensors: offloaded 29/29 layers to GPU
/// llama_model_load_from_file_impl: using device Vulkan0 (NVIDIA GeForce RTX 3070) (0000:01:00.0) - 7000 MiB free
/// ```
///
/// From the second, only the backend name and the device description are kept; the PCI id and
/// the free memory are dropped. Every other line is discarded as soon as it has been looked at,
/// so a prompt or a request logged by the server is never retained.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LlamaFacts {
    pub offloaded: Option<(u32, u32)>,
    pub devices: Vec<String>,
}

/// How many devices to remember. A machine with more is not made more debuggable by a list.
const DEVICE_LIMIT: usize = 4;
/// How long a device label may be.
const LABEL_LIMIT: usize = 64;

impl LlamaFacts {
    /// Look at one line, keep what matches, and forget the line.
    pub fn observe(&mut self, line: &str) {
        if let Some(counts) = offloaded_counts(line) {
            self.offloaded = Some(counts);
        } else if let Some(device) = device_label(line) {
            if self.devices.len() < DEVICE_LIMIT && !self.devices.contains(&device) {
                self.devices.push(device);
            }
        }
    }

    /// What these facts establish about acceleration.
    pub fn acceleration(&self) -> Acceleration {
        match self.offloaded {
            Some((0, _)) => Acceleration::Cpu,
            Some((offloaded, total)) => Acceleration::GpuLayers {
                offloaded,
                total,
                device: self.devices.first().cloned(),
            },
            None => Acceleration::Unknown,
        }
    }
}

/// `…: offloaded N/M layers to GPU` → `(N, M)`.
fn offloaded_counts(line: &str) -> Option<(u32, u32)> {
    let line = line.trim_end();
    let body = line.strip_suffix(" layers to GPU")?;
    let counts = &body[body.rfind(": offloaded ")? + ": offloaded ".len()..];
    let (offloaded, total) = counts.split_once('/')?;
    let offloaded: u32 = offloaded.parse().ok()?;
    let total: u32 = total.parse().ok()?;
    (offloaded <= total && total > 0).then_some((offloaded, total))
}

/// `…: using device NAME (DESCRIPTION) (ID) - N MiB free` → `"NAME (DESCRIPTION)"`.
///
/// A description may itself contain parentheses ("AMD Radeon(TM) Graphics"), so the id is cut
/// from the right and the name from the left.
fn device_label(line: &str) -> Option<String> {
    let rest = &line[line.find(": using device ")? + ": using device ".len()..];
    let (described, _free) = rest.rsplit_once(" - ")?;
    let (with_description, _id) = described.rsplit_once(" (")?;
    let (name, description) = with_description.split_once(" (")?;
    let description = description.strip_suffix(')')?;
    let label = format!("{} ({})", safe_label(name), safe_label(description));
    (!name.trim().is_empty()).then_some(label)
}

/// Letters, digits, spaces and a little punctuation, at most [`LABEL_LIMIT`] characters.
///
/// Applied to anything a device or engine reports about itself before it is kept. No slash,
/// backslash or colon survives, so a path or an address cannot pass through intact.
pub fn safe_label(text: &str) -> String {
    text.chars()
        .filter(|c| {
            c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-' | '+' | '(' | ')')
        })
        .take(LABEL_LIMIT)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFLOADED: &str = "load_tensors: offloaded 29/29 layers to GPU";
    const DEVICE: &str = "llama_model_load_from_file_impl: using device Vulkan0 (NVIDIA GeForce RTX 3070) (0000:01:00.0) - 7000 MiB free";

    #[test]
    fn the_offload_line_gives_the_layer_counts() {
        let mut facts = LlamaFacts::default();
        facts.observe(OFFLOADED);
        assert_eq!(facts.offloaded, Some((29, 29)));
    }

    #[test]
    fn the_device_line_keeps_the_name_and_description_only() {
        let mut facts = LlamaFacts::default();
        facts.observe(DEVICE);
        assert_eq!(
            facts.devices,
            vec!["Vulkan0 (NVIDIA GeForce RTX 3070)".to_string()]
        );
        let kept = format!("{facts:?}");
        assert!(
            !kept.contains("0000:01:00.0"),
            "the PCI id is dropped: {kept}"
        );
        assert!(!kept.contains("7000"), "the free memory is dropped: {kept}");
    }

    #[test]
    fn a_description_with_parentheses_survives() {
        let mut facts = LlamaFacts::default();
        facts.observe(
            "x: using device Vulkan0 (AMD Radeon(TM) Graphics) (unknown id) - 512 MiB free",
        );
        assert_eq!(
            facts.devices,
            vec!["Vulkan0 (AMD Radeon(TM) Graphics)".to_string()]
        );
    }

    #[test]
    fn all_layers_offloaded_is_gpu_acceleration_with_counts_not_a_whole_pipeline_claim() {
        let mut facts = LlamaFacts::default();
        facts.observe(DEVICE);
        facts.observe(OFFLOADED);
        let acceleration = facts.acceleration();
        assert_eq!(
            acceleration,
            Acceleration::GpuLayers {
                offloaded: 29,
                total: 29,
                device: Some("Vulkan0 (NVIDIA GeForce RTX 3070)".into()),
            }
        );
        assert_eq!(
            acceleration.phrase(),
            "GPU acceleration active (29/29 layers)"
        );
    }

    #[test]
    fn some_layers_is_still_reported_as_counts() {
        let mut facts = LlamaFacts::default();
        facts.observe("load_tensors: offloaded 12/29 layers to GPU");
        assert_eq!(
            facts.acceleration().phrase(),
            "GPU acceleration active (12/29 layers)"
        );
    }

    #[test]
    fn no_layers_offloaded_is_the_processor() {
        let mut facts = LlamaFacts::default();
        facts.observe("load_tensors: offloaded 0/29 layers to GPU");
        assert_eq!(facts.acceleration(), Acceleration::Cpu);
    }

    #[test]
    fn nothing_recognised_is_unknown_never_an_inferred_gpu() {
        let mut facts = LlamaFacts::default();
        facts.observe(DEVICE);
        facts.observe("main: server is listening on http://127.0.0.1:8140");
        assert_eq!(facts.acceleration(), Acceleration::Unknown);
    }

    #[test]
    fn prompts_and_other_lines_leave_nothing_behind() {
        let mut facts = LlamaFacts::default();
        // What a server might log about a request: text, a path, binary-looking noise.
        for line in [
            "srv  log_server_r: request: POST /v1/chat/completions 127.0.0.1 200",
            "prompt: \u{3053}\u{308c}\u{306f}\u{79d8}\u{5bc6}\u{3067}\u{3059} C:\\Users\\someone\\secret.png",
            "\u{fffd}\u{fffd}PNG\r\n\u{1a}\n data:image/png;base64,iVBORw0KGgo=",
            "using device but not in the shape: offloaded lots of layers",
            "offloaded 3/2 layers to GPU without the prefix",
        ] {
            facts.observe(line);
        }
        assert_eq!(
            facts,
            LlamaFacts::default(),
            "nothing but the two shapes is kept"
        );
    }

    #[test]
    fn impossible_counts_are_ignored() {
        let mut facts = LlamaFacts::default();
        facts.observe("load_tensors: offloaded 30/29 layers to GPU");
        facts.observe("load_tensors: offloaded 1/0 layers to GPU");
        assert_eq!(facts.offloaded, None);
    }

    #[test]
    fn a_safe_label_cannot_carry_a_path_or_an_address() {
        assert_eq!(
            safe_label("C:\\Users\\someone\\model.gguf"),
            "CUserssomeonemodel.gguf"
        );
        assert_eq!(
            safe_label("http://192.168.1.10:8140/v1"),
            "http192.168.1.108140v1"
        );
        assert!(safe_label(&"x".repeat(500)).len() <= LABEL_LIMIT);
    }

    #[test]
    fn cache_use_combines_parts() {
        assert_eq!(CacheUse::combine([]), CacheUse::Fresh);
        assert_eq!(CacheUse::combine([false, false]), CacheUse::Fresh);
        assert_eq!(CacheUse::combine([true, true]), CacheUse::Cached);
        assert_eq!(CacheUse::combine([true, false]), CacheUse::Mixed);
    }
}
