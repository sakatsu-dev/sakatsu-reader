//! The local PaddleOCR-VL reader: a llama.cpp server this app owns, starts and stops.
//!
//! # Why it is a child process and not an installed runtime
//!
//! This module used to install **Ollama**, and that was the wrong shape for what it does.
//! Ollama is a system-wide install with a tray application and a login-startup entry: opening
//! a reading application put an AI runtime in the notification area, closing that runtime by
//! hand did not keep it closed, and uninstalling the reader left it behind.
//!
//! What replaced it is a `llama-server.exe` unpacked into this app's own data directory,
//! started when a capture needs it and killed when the reader exits. Nothing is registered
//! with the system, nothing survives the process, and removing the app's data directory
//! removes every trace.
//!
//! Vulkan rather than CUDA, for reach rather than speed: one 33 MB build covers NVIDIA, AMD
//! and Intel with no toolkit, where the CUDA build is 238 MB plus a 373 MB runtime and serves
//! one vendor. For a 1B model answering in hundredths of a second the difference does not
//! show — measured at 0.02 s a read on an RTX 4070 Ti SUPER through Vulkan.
//!
//! # What did not change
//!
//! [`core::ocr::vlm`](sakatsu_core::ocr::vlm) is untouched. It speaks the OpenAI
//! chat-completions shape, which is what llama-server serves and what Ollama served, so the
//! engine never knew which was on the other end. Verified against both on the same image:
//! identical output.
//!
//! A custom endpoint is still supported and still never started by this app — see
//! [`is_managed_endpoint`].

use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Emitter;

/// The port the managed server listens on.
///
/// **Not 11434.** That is Ollama's, and picking it again would mean this app's server and a
/// user's own Ollama fighting over one socket — with whichever started first winning and the
/// other failing in a way neither could explain. 8140 is unassigned and unused by anything
/// this app knows about.
const PORT: u16 = 8140;
pub const ENDPOINT: &str = "http://127.0.0.1:8140";

/// What the model is called in requests.
///
/// llama-server serves whatever it was given and matches this loosely, so it is a label rather
/// than a lookup — but it is the label that appears in its logs, so it says what it is.
pub const MODEL: &str = "paddleocr-vl-manga";

/// The weights, as `catalog.rs` names them on disk.
const WEIGHTS: &str = "PaddleOCR-VL-For-Manga-BF16.gguf";
const PROJECTOR: &str = "PaddleOCR-VL-For-Manga-mmproj-BF16.gguf";
/// Where the unpacked llama.cpp build goes, under the component's directory.
pub const RUNTIME: &str = "runtime";

const PROBE_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub ready: bool,
    pub endpoint: &'static str,
    pub model: &'static str,
    pub detail: String,
    /// How long one read took on this machine, measured by setup. `None` before a machine has
    /// been timed, which is not the same as slow and must not be presented as it.
    pub seconds_per_read: Option<f64>,
}

/// The marker written once the files are in place and verified.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedConfig {
    runtime: String,
    endpoint: String,
    model: String,
    #[serde(default)]
    seconds_per_read: Option<f64>,
}

impl ManagedConfig {
    /// **`"llama"`, and an `"ollama"` marker is deliberately not current.**
    ///
    /// A machine set up before this module changed has Ollama's weights in Ollama's own blob
    /// store, which this app cannot read and should not learn to. Rejecting the marker sends
    /// that reader back through setup for a download they have effectively already done — the
    /// honest cost of dropping a runtime, and better than an install that reports itself
    /// working and then cannot start anything.
    fn is_current(&self) -> bool {
        self.runtime == "llama" && self.endpoint == ENDPOINT && self.model == MODEL
    }
}

pub fn marker_path() -> PathBuf {
    crate::vlm_dir().join("managed.json")
}

fn marker() -> Option<ManagedConfig> {
    std::fs::read_to_string(marker_path())
        .ok()
        .and_then(|text| serde_json::from_str::<ManagedConfig>(&text).ok())
        .filter(ManagedConfig::is_current)
}

/// Whether a directory holds a complete install: the server, the weights and the projector.
///
/// All three, because any one of them missing is an install that cannot serve a read, and
/// `resource_dir` uses this to decide which of two candidate directories is the real one.
pub fn is_installed_in(dir: &Path) -> bool {
    dir.join(RUNTIME).join("llama-server.exe").is_file()
        && dir.join(WEIGHTS).is_file()
        && dir.join(PROJECTOR).is_file()
}

/// Whether the reader is installed and its marker still describes this build.
pub fn installed() -> bool {
    marker().is_some() && is_installed_in(&crate::vlm_dir())
}

fn server_binary() -> PathBuf {
    crate::vlm_dir().join(RUNTIME).join("llama-server.exe")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Service {
    Stopped,
    Ready,
    Unexpected,
}

/// What a failed probe means, which is the difference between starting the server and refusing to.
///
/// **Only an actual HTTP answer proves something else owns the port.** This once tested for
/// `Io` alone and called everything else `Unexpected`, which was wrong in a way that only shows
/// on some machines: `ureq` reports a dropped connection as `Timeout` or `ConnectionFailed`,
/// never as `Io`. Where a closed port silently drops packets rather than refusing them — a
/// firewall rule is enough — the app concluded a squatter held the port and refused to start
/// its own server, on a machine where nothing was listening at all.
fn service_from_error(error: &ureq::Error) -> Service {
    match error {
        ureq::Error::StatusCode(_) => Service::Unexpected,
        _ => Service::Stopped,
    }
}

/// Ask the server whether it is up.
///
/// `/health` rather than `/v1/models`: llama-server answers it while the model is still
/// loading, with a status that distinguishes "starting" from "ready", and a read sent during
/// loading would simply queue.
fn service() -> Service {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(PROBE_TIMEOUT))
        .build()
        .into();
    match agent.get(format!("{ENDPOINT}/health")).call() {
        Ok(_) => Service::Ready,
        Err(error) => service_from_error(&error),
    }
}

/// The running server, so it can be stopped again.
///
/// **A reader that starts a process owns it.** Left unheld, the server would outlive the app
/// that spawned it — holding a port, a gigabyte of VRAM and the weights open — and the next
/// launch would find the port taken by its own orphan. Windows gives no way to bind a child's
/// lifetime to a parent without a job object, so the handle is kept and killed explicitly.
static SERVER: Mutex<Option<Child>> = Mutex::new(None);

/// Whether this process started the server, for the shutdown path.
static OURS: AtomicBool = AtomicBool::new(false);

/// What the running server's startup lines established — layer counts and device names only.
///
/// Reset at every start, so a restarted server is never described by its predecessor's lines.
static FACTS: Mutex<sakatsu_core::runtime_status::LlamaFacts> =
    Mutex::new(sakatsu_core::runtime_status::LlamaFacts {
        offloaded: None,
        devices: Vec::new(),
    });

/// The longest line the drain looks at. Anything longer is cut, never kept: the two lines worth
/// reading are under 200 bytes, and a server logging a request could print far more.
const LINE_LIMIT: usize = 1024;

/// Read the server's standard error until it closes, keeping only [`LlamaFacts`]' two shapes.
///
/// **Nothing else survives a line.** Each line is read into a buffer that is cleared before the
/// next, cut at [`LINE_LIMIT`], decoded lossily, offered to [`LlamaFacts::observe`] and dropped.
/// A prompt, an image or a request the server logs is never stored, written or forwarded.
///
/// [`LlamaFacts`]: sakatsu_core::runtime_status::LlamaFacts
/// [`LlamaFacts::observe`]: sakatsu_core::runtime_status::LlamaFacts::observe
fn drain_startup_lines(stderr: impl std::io::Read) {
    use std::io::{BufRead, BufReader};
    let mut reader = BufReader::new(stderr);
    let mut line = Vec::with_capacity(256);
    loop {
        line.clear();
        // Read up to the newline, keeping at most LINE_LIMIT bytes of it.
        let mut overflowed = false;
        loop {
            let available = match reader.fill_buf() {
                Ok(available) => available,
                Err(_) => return,
            };
            if available.is_empty() {
                // End of stream: the server has gone.
                if !line.is_empty() && !overflowed {
                    observe(&line);
                }
                return;
            }
            let (chunk, ended) = match available.iter().position(|byte| *byte == b'\n') {
                Some(end) => (&available[..end], Some(end + 1)),
                None => (available, None),
            };
            let room = LINE_LIMIT.saturating_sub(line.len());
            if chunk.len() > room {
                overflowed = true;
            }
            line.extend_from_slice(&chunk[..chunk.len().min(room)]);
            let consumed = ended.unwrap_or(available.len());
            reader.consume(consumed);
            if ended.is_some() {
                break;
            }
        }
        // An over-long line is not one of the two shapes; it is skipped, not truncated and read.
        if !overflowed {
            observe(&line);
        }
    }
}

fn observe(line: &[u8]) {
    let text = String::from_utf8_lossy(line);
    FACTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .observe(text.trim_end_matches('\r'));
}

/// Where the managed server computes, from its own startup lines. `NotRunning` unless this
/// process started it and it is still running; `Unknown` until a recognised line arrives.
pub fn acceleration() -> sakatsu_core::runtime_status::Acceleration {
    use sakatsu_core::runtime_status::Acceleration;
    if !OURS.load(Ordering::Acquire) {
        return Acceleration::NotRunning;
    }
    let running = SERVER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
        .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
    if !running {
        return Acceleration::NotRunning;
    }
    FACTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .acceleration()
}

/// Whether a configured endpoint is the one this module installs and may start.
///
/// The distinction decides what the app is entitled to do. The managed server is something
/// setup put there, so starting it is finishing a job the reader already agreed to. A custom
/// endpoint is somebody else's process — possibly on another machine — and the app has no
/// business launching anything on its behalf.
pub fn is_managed_endpoint(endpoint: &str) -> bool {
    endpoint.trim().trim_end_matches('/') == ENDPOINT
}

/// Start the managed server, if that is what the reader is pointed at, and wait for it.
///
/// **Because "is the server running?" is not a question this app should ask its user.** The
/// whole point of the managed path is that nobody has to know a server exists; failing a
/// capture because it is not up hands back exactly the problem the path removes.
///
/// Bounded by `wait` and best-effort throughout: a false return means the read is about to
/// fail with its own message, which is more informative than anything invented here. Does
/// nothing for a custom endpoint, and nothing when the server is already up — the common
/// case, one cheap probe.
pub fn ensure_managed_runtime(endpoint: &str, wait: Duration) -> bool {
    if !is_managed_endpoint(endpoint) || !installed() {
        return false;
    }
    if service() == Service::Ready {
        return true;
    }
    if start_server().is_err() {
        return false;
    }

    let deadline = std::time::Instant::now() + wait;
    while std::time::Instant::now() < deadline {
        if service() == Service::Ready {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

fn server_command(binary: &Path, dir: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(binary);
    command
        .arg("-m")
        .arg(dir.join(WEIGHTS))
        .arg("--mmproj")
        .arg(dir.join(PROJECTOR))
        .args(["--host", "127.0.0.1"])
        .args(["--port", &PORT.to_string()])
        .args(["--alias", MODEL])
        // Zero, always. Sampling an OCR result is asking the model to invent a reading, and the
        // engine pins this too — set in both places because either one alone is a default
        // somebody can change without noticing the other.
        .args(["--temp", "0"])
        .args(["-ngl", "99"])
        // b10644 hides device/offload facts at the default verbosity (3).
        // Only the two recognised facts survive the stderr drain at level 4.
        .args(["-lv", "4"])
        // **All three handles.** Windows spawns with handle inheritance on, so a standard
        // handle left inherited is held open by a server that outlives whatever started it.
        // Standard error is a pipe this process owns, not an inherited handle: the drain reads
        // the two startup lines that say where the model runs, and nothing else is kept.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
}

/// Spawn `llama-server` against the installed weights.
///
/// `-ngl 99` offloads every layer it can: the Vulkan build falls back to the processor for
/// whatever will not fit, so asking for all of them is asking for as much as the card has
/// rather than a promise about how much that is.
fn start_server() -> Result<(), String> {
    let _lifecycle = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Recognition)
        .map_err(|e| e.to_string())?;
    let mut held = SERVER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(child) = held.as_mut() {
        // Still alive from an earlier start: nothing to do. `try_wait` is the only way to ask
        // without blocking, and `Ok(None)` is the answer that means running.
        if matches!(child.try_wait(), Ok(None)) {
            return Ok(());
        }
    }

    let binary = server_binary();
    if !binary.is_file() {
        return Err(
            "PaddleOCR-VL is not set up yet. Open Settings > Recognition and choose Set up.".into(),
        );
    }
    let dir = crate::vlm_dir();

    let mut command = server_command(&binary, &dir);

    *FACTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Default::default();
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start the local PaddleOCR-VL server: {error}"))?;
    if let Some(stderr) = child.stderr.take() {
        // Detached: it ends when the server closes its standard error, which killing it does.
        let _ = std::thread::Builder::new()
            .name("vlm-startup-lines".into())
            .spawn(move || drain_startup_lines(stderr));
    }
    *held = Some(child);
    OURS.store(true, Ordering::Release);
    sakatsu_core::log_info!("vlm", "started the local server on port {PORT}");
    Ok(())
}

/// Stop the server this process started.
///
/// Called from the window's close handler. Best-effort by design: a reader closing their app
/// is not owed an error about a child that had already gone.
pub fn stop_managed_runtime() -> bool {
    if !OURS.swap(false, Ordering::AcqRel) {
        return false;
    }
    let mut held = SERVER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(mut child) = held.take() {
        let _ = child.kill();
        let _ = child.wait();
        sakatsu_core::log_info!("vlm", "stopped the local server");
        return true;
    }
    false
}

fn current_status(detail: Option<String>) -> Status {
    let marker = marker();
    let installed = marker.is_some() && is_installed_in(&crate::vlm_dir());
    let service = service();
    let ready = installed && service == Service::Ready;
    let detail = detail.unwrap_or_else(|| {
        if ready {
            "Ready — Sakatsu Reader starts this local server when selected.".into()
        } else if !installed {
            "Not set up — about 1.8 GB, and nothing is installed outside this app.".into()
        } else if service == Service::Stopped {
            "Installed. The server starts on the first capture.".into()
        } else {
            format!("Something other than the expected server answered on port {PORT}.")
        }
    });
    Status {
        installed,
        ready,
        endpoint: ENDPOINT,
        model: MODEL,
        detail,
        seconds_per_read: marker.and_then(|config| config.seconds_per_read),
    }
}

#[tauri::command(async)]
pub fn vlm_setup_status() -> Status {
    current_status(None)
}

/// Start the server on request, so the settings panel can prove it works before a capture does.
#[tauri::command(async)]
pub fn start_vlm_runtime() -> Result<Status, String> {
    let _lifecycle = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Recognition)
        .map_err(|e| e.to_string())?;
    if !installed() {
        return Err(
            "PaddleOCR-VL is not set up yet. Open Settings > Recognition and choose Set up.".into(),
        );
    }
    match service() {
        Service::Ready => return Ok(current_status(None)),
        Service::Unexpected => {
            return Err(format!(
                "port {PORT} is in use, but the expected server did not answer there"
            ));
        }
        Service::Stopped => {}
    }
    start_server()?;
    Ok(current_status(Some(
        "Starting the local PaddleOCR-VL server…".into(),
    )))
}

/// The sentence to add when a VLM read cannot reach the endpoint it was given.
///
/// `core::ocr::vlm` reports the refusal accurately and cannot say more, because it knows only
/// a URL — it has no idea this app also ships a reader of its own. This does, so it can name
/// the one mismatch that is otherwise a genuine puzzle: **a working managed install sitting on
/// one port while Settings points at another.**
pub fn advice_for_unreachable(endpoint: &str) -> Option<String> {
    if is_managed_endpoint(endpoint) || !installed() {
        return None;
    }
    Some(format!(
        "Settings points PaddleOCR-VL at {endpoint}, which is not answering. \
         The one this app installed is at {ENDPOINT} and starts by itself — \
         choose Point it at this install in Settings > Recognition."
    ))
}

/// Record how long one read took, so Settings can say whether this machine suits the engine.
///
/// Written by the setup path once the server has answered. Separate from the marker's creation
/// on purpose: an install that finished but was never timed is still an install.
pub fn record_seconds_per_read<R: tauri::Runtime>(app: &tauri::AppHandle<R>, seconds: f64) {
    let path = marker_path();
    let Some(mut config) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<ManagedConfig>(&text).ok())
    else {
        return;
    };
    config.seconds_per_read = Some((seconds * 100.0).round() / 100.0);
    if let Ok(text) = serde_json::to_string(&config) {
        let _ = std::fs::write(&path, text);
    }
    let _ = app.emit("vlm-setup-finished", serde_json::json!({ "ok": true }));
}

/// A 100x41 rendering of 日本語, for timing one read.
///
/// **Decoded before it was pasted here**, which is not a given: an earlier probe of this kind
/// was a plausible-looking base64 string that had never been through a decoder, and every read
/// failed with "Failed to load image or audio file" while the setup reported the machine slow.
/// `the_probe_image_is_a_real_png` below is the guard.
///
/// Real text rather than a blank rectangle, so the answer can be checked as well as timed.
const PROBE_PNG: &str = concat!(
    "iVBORw0KGgoAAAANSUhEUgAAAGQAAAApAQAAAADrX7FLAAABCUlEQVR42r2Rr07DYBTFf/0KWQXhj0CA6QDBY4Ah",
    "IQGDmcFM8g6QMMEDkEwhyCpJJjGgujcAhf4wQDJBm41RtrYH8bWBF4DjTm5+95zc64kfpYbf+kMX1a7oHgJMjvsD",
    "UI4v9WSX2UlQoZEUS+opMYgALNAEg0cG8NQGUK5ZfrmyPVhfleNy8JV5FedrKadxWnNqMjn3wKCDiPKEYSSq9OJW",
    "Gt/EVfrXlT7ahMkcImC6xTAWqdsytoQWcM3eNncpW3XC47P9nPU7rpnC+MGulUoMHllxtDduXFDNMkv6fl8qMeTd",
    "/fkwXsysm7lLvFw7ThnA9KzluJHU0+sdSgw+CwDBRtnB++dvfgPSPaIzLDHqxQAAAABJRU5ErkJggg=="
);

/// Time one read against the freshly installed server, and record it.
///
/// **Two reads, and the first is discarded.** It carries the cost of loading 1.7 GB of weights
/// and building the vision context, which is paid once per server rather than once per capture;
/// timing it would report every machine as slow. Measured here at 21.9 s cold and 0.02 s warm —
/// a difference of three orders of magnitude, so the distinction is not a nicety.
///
/// Best-effort throughout. A machine that cannot be timed is simply one the offer stays quiet
/// about, which is the same state as a machine nobody has timed yet.
pub fn measure_first_read<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(seconds) = time_one_read() {
        record_seconds_per_read(app, seconds);
    }
}

/// The measurement itself, without the handle needed to announce it.
///
/// Split out so it can be tested: an `AppHandle` cannot be built outside a running Tauri
/// application, and the part worth testing — that a real install answers, and how fast — has
/// nothing to do with emitting an event.
pub fn time_one_read() -> Option<f64> {
    if !ensure_managed_runtime(ENDPOINT, Duration::from_secs(120)) {
        return None;
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(180)))
        .build()
        .into();
    let body = serde_json::json!({
        "model": MODEL,
        "temperature": 0,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{PROBE_PNG}") } },
                // The prompt the engine actually sends. A friendlier one would measure a
                // request the app never makes — and would check a different answer.
                { "type": "text", "text": "OCR:" },
            ],
        }],
    });
    let ask = || {
        agent
            .post(format!("{ENDPOINT}/v1/chat/completions"))
            .send_json(&body)
            .map(|_| ())
    };

    // Discarded: it carries the cost of loading the weights and building the vision context,
    // which is paid once per server rather than once per capture. Measured here at 21.9 s cold
    // against 0.02 s warm — timing the first would call every machine slow.
    ask().ok()?;
    let started = std::time::Instant::now();
    ask().ok()?;
    let seconds = started.elapsed().as_secs_f64();
    sakatsu_core::log_info!("vlm", "timed one read at {seconds:.3}s");
    Some(seconds)
}

/// Write the marker that says this install is complete and current.
pub fn write_marker(dir: &Path) -> Result<(), String> {
    let config = ManagedConfig {
        runtime: "llama".into(),
        endpoint: ENDPOINT.into(),
        model: MODEL.into(),
        seconds_per_read: None,
    };
    let text = serde_json::to_string(&config).map_err(|error| error.to_string())?;
    std::fs::write(dir.join("managed.json"), text).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_launch_requests_the_startup_facts_verbosity() {
        let command = server_command(Path::new("llama-server.exe"), Path::new("models"));
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert!(args.windows(2).any(|pair| pair == ["-lv", "4"]));
        assert!(!args.iter().any(|arg| arg.starts_with("--log-file")));
    }

    /// The drain keeps the two facts and nothing else — not a prompt, not a path, and not an
    /// over-long line that happens to end in the right shape.
    #[test]
    fn the_startup_drain_keeps_the_two_facts_and_nothing_else() {
        *FACTS.lock().unwrap() = Default::default();
        let mut input = Vec::new();
        input.extend_from_slice(
            b"llama_model_load_from_file_impl: using device Vulkan0 (Test GPU) (0000:01:00.0) - 512 MiB free\n",
        );
        input.extend_from_slice(b"prompt: a private passage C:\\Users\\someone\\capture.png\n");
        input.extend(std::iter::repeat_n(b'x', 5000));
        input.extend_from_slice(b": offloaded 9/9 layers to GPU\n");
        input.extend_from_slice(b"0.00.612.005 I load_tensors: offloaded 28/29 layers to GPU\r\n");
        drain_startup_lines(std::io::Cursor::new(input));

        let facts = FACTS.lock().unwrap().clone();
        assert_eq!(
            facts.offloaded,
            Some((28, 29)),
            "the over-long line was skipped, not read"
        );
        assert_eq!(facts.devices, vec!["Vulkan0 (Test GPU)".to_string()]);
        let kept = format!("{facts:?}");
        for private in ["private passage", "capture.png", "0000:01:00.0", "512"] {
            assert!(!kept.contains(private), "{private:?} survived: {kept}");
        }
    }

    fn marker_json(runtime: &str, extra: &str) -> String {
        format!(r#"{{"runtime":"{runtime}","endpoint":"{ENDPOINT}","model":"{MODEL}"{extra}}}"#)
    }

    fn parse(text: &str) -> ManagedConfig {
        serde_json::from_str(text).expect("marker should parse")
    }

    /// **An Ollama marker is not this install**, and saying so is the migration.
    ///
    /// Those weights live in Ollama's content-addressed blob store, which this app cannot read
    /// and deliberately does not learn to. Accepting the marker would report a working install
    /// that could never start a server.
    #[test]
    fn a_marker_from_the_ollama_runtime_is_not_current() {
        assert!(!parse(&marker_json("ollama", "")).is_current());
        assert!(parse(&marker_json("llama", "")).is_current());
    }

    #[test]
    fn a_marker_written_before_timing_existed_is_still_a_valid_install() {
        let config = parse(&marker_json("llama", ""));
        assert!(config.is_current());
        assert_eq!(config.seconds_per_read, None);
    }

    /// The field is `secondsPerRead` in the file and `seconds_per_read` in Rust, joined only by
    /// a rename attribute. A silent mismatch reads as "never timed", which is the state that
    /// hides the offer — so nothing would visibly break.
    #[test]
    fn a_timed_marker_carries_the_measurement_across_the_rename() {
        let config = parse(&marker_json("llama", r#","secondsPerRead":0.02"#));
        assert_eq!(config.seconds_per_read, Some(0.02));
    }

    /// Round-trips, because [`write_marker`] serialises the same struct that reads it back.
    #[test]
    fn the_marker_this_writes_is_one_it_accepts() {
        let dir = std::env::temp_dir().join(format!("sakatsu-vlm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        write_marker(&dir).expect("write");

        let text = std::fs::read_to_string(dir.join("managed.json")).expect("read");
        assert!(
            parse(&text).is_current(),
            "wrote a marker it would reject: {text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Which endpoints the app may start a server for, and which are somebody else's.
    ///
    /// The whole safety of auto-starting rests on this: a custom endpoint may be a server on
    /// another machine, and launching one because of it would be both useless and rude.
    #[test]
    fn only_this_install_counts_as_the_managed_endpoint() {
        assert!(is_managed_endpoint(ENDPOINT));
        assert!(is_managed_endpoint(&format!("{ENDPOINT}/")));
        assert!(is_managed_endpoint(&format!("  {ENDPOINT}  ")));

        // Ollama's port, which this deliberately no longer uses. A machine running Ollama for
        // its own reasons must not be mistaken for this app's server.
        assert!(!is_managed_endpoint("http://127.0.0.1:11434"));
        assert!(!is_managed_endpoint("http://127.0.0.1:8000"));
        assert!(!is_managed_endpoint("http://192.168.1.10:8140"));
        assert!(!is_managed_endpoint(""));
    }

    /// A dropped connection is not a squatter, and the difference decides whether a capture works.
    #[test]
    fn only_a_real_http_answer_means_something_else_holds_the_port() {
        assert_eq!(
            service_from_error(&ureq::Error::StatusCode(404)),
            Service::Unexpected
        );
        for silence in [
            ureq::Error::ConnectionFailed,
            ureq::Error::HostNotFound,
            ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)),
            // The one that actually bit: a firewall dropping packets to a closed port turns a
            // refusal into a timeout, and a timeout used to mean "occupied".
            ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::TimedOut)),
        ] {
            assert_eq!(
                service_from_error(&silence),
                Service::Stopped,
                "{silence:?} should leave the app free to start its own server",
            );
        }
    }

    /// The whole install, driven end to end against a real download.
    ///
    /// **`#[ignore]`d because it needs ~1.8 GB already fetched**, and because it starts a real
    /// server on the real port. Everything else in this module tests a piece; this is the one
    /// that answers "does an install work", which no amount of piece-testing does.
    ///
    /// Expects the three artifacts in the component's `work/` directory, exactly where
    /// `fetch_component` leaves them:
    ///
    /// ```powershell
    /// cargo test -p sakatsu-desktop --lib -- --ignored the_whole_install --nocapture
    /// ```
    #[test]
    #[ignore = "needs the artifacts downloaded, and starts a real server"]
    fn the_whole_install_works_from_the_downloaded_files() {
        let dir = crate::vlm_dir();
        let work = dir.join("work");

        // **Re-runnable**, because the first run publishes the weights out of `work/` and a
        // verification you can only do once is one nobody repeats. Each artifact is accepted
        // either where the download leaves it or where the install puts it.
        for name in [WEIGHTS, PROJECTOR] {
            assert!(
                work.join(name).is_file() || dir.join(name).is_file(),
                "{} is in neither {} nor {} — download the artifacts first",
                name,
                work.display(),
                dir.display()
            );
        }
        let zip = work.join("llama-vulkan.zip");
        assert!(
            zip.is_file() || dir.join(RUNTIME).join("llama-server.exe").is_file(),
            "neither {} nor an unpacked runtime in {} — download the artifacts first",
            zip.display(),
            dir.display()
        );

        // Exactly what `After::UnpackRuntime` does, in the order it does it: publish the
        // weights where they are used, unpack the runtime beside them, write the marker.
        for name in [WEIGHTS, PROJECTOR] {
            if !dir.join(name).is_file() {
                std::fs::rename(work.join(name), dir.join(name)).expect("publish the weights");
            }
            assert!(dir.join(name).is_file(), "{name} should be in place");
        }

        // Only when there is an archive to open. A finished install has none — the setup arm
        // deletes it, 95 MB of DLLs being worth more than the 33 MB zip they came from — so
        // requiring one here would mean this could never be run against a *completed* install,
        // which is the state most worth checking.
        if zip.is_file() {
            let files = crate::commands::unpack_runtime(&zip, &dir.join(RUNTIME))
                .expect("unpack the runtime");
            eprintln!("unpacked {files} runtime files");
            assert!(files > 10, "a llama.cpp release is more than {files} files");
        } else {
            eprintln!("runtime already unpacked — checking it as it stands");
        }
        assert!(
            dir.join(RUNTIME).join("llama-server.exe").is_file(),
            "the runtime must have a server in it"
        );

        write_marker(&dir).expect("marker");
        assert!(
            marker().is_some(),
            "the marker this writes must be one it accepts"
        );

        eprintln!("installed: {}", installed());
        assert!(
            installed(),
            "all three files present and the marker current"
        );

        // And it serves a read. Two, because the first carries the model load.
        assert!(
            ensure_managed_runtime(ENDPOINT, Duration::from_secs(180)),
            "the server must come up"
        );
        let status = vlm_setup_status();
        assert!(
            status.ready,
            "status should report ready: {}",
            status.detail
        );
        eprintln!("status: {}", status.detail);

        // The timing the offer in Settings is gated on. Without it `secondsPerRead` stays null
        // and the reader is never recommended, however fast the machine turns out to be.
        let seconds = time_one_read().expect("a working install must be timeable");
        eprintln!("timed one read at {seconds:.3}s");
        assert!(
            seconds > 0.0 && seconds < 120.0,
            "{seconds}s is outside anything a read can take"
        );

        stop_managed_runtime();
    }

    /// The embedded probe is a real PNG with real ink in it.
    ///
    /// **An earlier probe of this kind was a base64 string that had never been decoded.** Every
    /// request failed with "Failed to load image or audio file", and because a failure was being
    /// read as slowness, setup told a fast machine it was slow. Decoded and opened rather than
    /// pattern-matched: a truncated PNG still starts with the magic bytes, and a blank one still
    /// decodes.
    #[test]
    fn the_probe_image_is_a_real_png() {
        use base64::Engine as _;

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(PROBE_PNG)
            .expect("the probe should be valid base64");
        let image = image::load_from_memory(&bytes).expect("the probe should decode as an image");
        assert_eq!(
            image::guess_format(&bytes)
                .expect("a known format")
                .to_mime_type(),
            "image/png",
            "the request declares data:image/png",
        );

        let (width, height) = (image.width(), image.height());
        assert!(
            (16..=512).contains(&width) && (16..=512).contains(&height),
            "probe is {width}x{height}, which is not a plausible line of text",
        );

        // Ink, not a blank rectangle. A blank probe would time a read of nothing.
        let darkest = image
            .to_luma8()
            .pixels()
            .map(|pixel| pixel.0[0])
            .min()
            .unwrap_or(255);
        assert!(darkest < 128, "the probe carries no text to read");
    }

    /// An incomplete install is not an install.
    ///
    /// Each of the three files is load-bearing — the server cannot run without the binary, and
    /// cannot read an image without the projector — so any one missing has to read as absent.
    /// `resource_dir` picks between two candidate directories with this, so a partial directory
    /// that answered `true` would shadow a complete one.
    #[test]
    fn every_piece_has_to_be_there_to_count_as_installed() {
        let root = std::env::temp_dir().join(format!("sakatsu-vlm-parts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(RUNTIME)).expect("temp dir");

        let pieces = [
            root.join(RUNTIME).join("llama-server.exe"),
            root.join(WEIGHTS),
            root.join(PROJECTOR),
        ];
        for piece in &pieces {
            std::fs::write(piece, b"x").expect("write");
        }
        assert!(is_installed_in(&root), "all three present");

        for missing in &pieces {
            std::fs::remove_file(missing).expect("remove");
            assert!(
                !is_installed_in(&root),
                "{} missing should not count as installed",
                missing.display()
            );
            std::fs::write(missing, b"x").expect("restore");
        }

        let _ = std::fs::remove_dir_all(&root);
    }
}
