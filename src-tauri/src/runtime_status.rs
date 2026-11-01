//! The shell's half of runtime status: the last result of each operation, and the engines' state
//! now. The types and the rules are in [`sakatsu_core::runtime_status`]; this file only gathers.
//!
//! **One slot per operation kind, held by that engine's state.** Recognition, analysis and
//! translation each keep their own last result with its own request identity, so a cached
//! translation cannot make an analysis look cached and a capture's engine cannot be shown against
//! text that came from elsewhere. Held by `OcrState`, `AnalyzerState` and `TranslatorState`
//! rather than in a process global, so each app — and each test's app — has its own.
//!
//! **Published under the scheduler's lock.** [`LastResult::record`] goes through
//! [`crate::work::Scheduler::publish_if_current`], so a result is stored only if its request
//! still owns its operation at the moment of storing — a newer request cannot register between
//! the check and the write. The interface checks again when it reads: it shows a result only for
//! the request it owns.

use std::sync::Mutex;

use sakatsu_core::diagnostics::{OsFacts, ReportInput, ResourceFact, ResourceState};
use sakatsu_core::model::{AnalyzerPolicy, Execution};
use sakatsu_core::request::RequestIdentity;
use sakatsu_core::runtime_status::{
    Acceleration, CacheUse, EngineRole, EngineState, EngineStatus, FailureCode, Hosting,
    OperationResult, Outcome, RuntimeStatus,
};

use crate::route::{AnalyzerState, Readiness};

/// The last result of one operation kind.
#[derive(Default)]
pub struct LastResult(Mutex<Option<OperationResult>>);

impl LastResult {
    /// Store `result` if its request is still current. Returns whether it was stored.
    pub fn record(&self, result: OperationResult) -> bool {
        let request = result.request.clone();
        crate::work::scheduler().publish_if_current(request.as_ref(), || {
            *self
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result);
        })
    }

    pub fn get(&self) -> Option<OperationResult> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// A recognition result, for the request that asked.
pub fn recognition_result(
    request: Option<&RequestIdentity>,
    produced: Result<&str, FailureCode>,
    acceleration: Acceleration,
) -> OperationResult {
    let (engine, outcome) = match produced {
        Ok(engine) => (engine.to_string(), Outcome::Succeeded),
        Err(code) => (String::new(), Outcome::Failed { code }),
    };
    OperationResult {
        role: EngineRole::Recognition,
        request: request.cloned(),
        engine,
        fallback: None,
        cache: CacheUse::Fresh,
        outcome,
        acceleration,
    }
}

/// An analysis result, from the execution record core wrote while producing it.
///
/// Neither analyzer has a graphics path — Ichiran is a Lisp program over PostgreSQL and the
/// built-in one is Lindera in this process — so the processor is a fact about the code, not an
/// inference.
pub fn analysis_result(
    request: Option<&RequestIdentity>,
    execution: Option<&Execution>,
    failed: bool,
) -> OperationResult {
    OperationResult {
        role: EngineRole::Analysis,
        request: request.cloned(),
        engine: execution
            .map(|execution| execution.engine.clone())
            .unwrap_or_default(),
        fallback: execution
            .and_then(|execution| execution.fallback.as_ref())
            .map(|fallback| fallback.reason),
        cache: CacheUse::Fresh,
        outcome: if failed {
            Outcome::Failed {
                code: FailureCode::EngineFailed,
            }
        } else {
            Outcome::Succeeded
        },
        acceleration: Acceleration::Cpu,
    }
}

/// The analyzer, as the current route describes it.
pub fn analysis_engine(analyzer: &AnalyzerState) -> EngineStatus {
    let route = analyzer.route();
    let primary = route.primary.engine().to_string();
    let state = match route.readiness {
        Readiness::Probing => EngineState::Loading,
        Readiness::Deferred => match sakatsu_core::analyzer::ichiran::server::session_ready() {
            Some(true) => EngineState::Ready,
            Some(false) => EngineState::NotLoaded,
            None => EngineState::Loading,
        },
        Readiness::Ready | Readiness::Unavailable { .. } => EngineState::Ready,
    };
    let loaded = if matches!(route.readiness, Readiness::Deferred) && state != EngineState::Ready {
        None
    } else {
        Some(primary.clone())
    };
    EngineStatus {
        role: EngineRole::Analysis,
        configured: match route.policy {
            AnalyzerPolicy::Automatic => "automatic",
            AnalyzerPolicy::Ichiran => "ichiran",
            AnalyzerPolicy::BuiltIn => "built-in",
        }
        .to_string(),
        hosting: if primary == "ichiran" {
            Hosting::ManagedLocal
        } else {
            Hosting::InProcess
        },
        // The engine standing in is ready even while Ichiran is still being checked.
        state,
        loaded,
        acceleration: Acceleration::Cpu,
    }
}

/// Everything the status display reads.
pub fn snapshot(
    ocr: &crate::commands::OcrState,
    analyzer: &AnalyzerState,
    translator: &crate::commands::TranslatorState,
) -> RuntimeStatus {
    RuntimeStatus {
        engines: vec![ocr.status(), analysis_engine(analyzer), translator.status()],
        recognition: ocr.last().get(),
        analysis: analyzer.last().get(),
        translation: translator.last().get(),
    }
}

/// The installed resources and the setup phase, reduced to the report's facts.
fn resources(snapshot: &sakatsu_core::setup::transport::SetupSnapshot) -> Vec<ResourceFact> {
    snapshot
        .components
        .iter()
        .map(|component| ResourceFact {
            id: component.id.clone(),
            version: component.installed_version.clone(),
            state: match component.transfer_state.as_str() {
                "installed" => ResourceState::Ready,
                "not_started" => ResourceState::NotInstalled,
                "transferring" => ResourceState::Downloading,
                "paused" => ResourceState::Paused,
                _ => ResourceState::Other,
            },
        })
        .collect()
}

/// The setup phase as its wire code, e.g. `complete`.
fn phase_code(snapshot: &sakatsu_core::setup::transport::SetupSnapshot) -> Option<String> {
    serde_json::to_value(snapshot.phase)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
}

/// The operating system's version, e.g. `10.0.26200`.
#[cfg(windows)]
fn os_version() -> String {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a correctly sized OSVERSIONINFOW, which is all RtlGetVersion writes to.
    let status = unsafe { RtlGetVersion(&mut info) };
    if status.is_ok() {
        format!(
            "{}.{}.{}",
            info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
        )
    } else {
        "unknown".to_string()
    }
}

#[cfg(not(windows))]
fn os_version() -> String {
    "unknown".to_string()
}

/// The report's input, gathered from the running app.
pub fn report_input(
    status: RuntimeStatus,
    setup: Option<&sakatsu_core::setup::transport::SetupSnapshot>,
    logging: bool,
) -> ReportInput {
    ReportInput {
        version: env!("CARGO_PKG_VERSION").to_string(),
        release_build: !cfg!(debug_assertions),
        os: OsFacts {
            family: std::env::consts::OS.to_string(),
            version: os_version(),
            arch: std::env::consts::ARCH.to_string(),
        },
        status,
        resources: setup.map(resources).unwrap_or_default(),
        setup: setup.and_then(phase_code),
        logging,
    }
}
