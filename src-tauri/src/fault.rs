//! **Deterministic analyzer failures, for the desktop checkpoint only.** Debug builds only.
//!
//! The visible fallback has to be observed by a person, and the honest way to make Ichiran fail
//! on demand is not to damage or stop the reader's own sidecar. So `SAKATSU_ANALYZER_FAULT`
//! replaces the route with one whose primary *claims* to be Ichiran and fails in the named way —
//! the sidecar is never contacted at all:
//!
//! | value | the primary | the fallback |
//! |---|---|---|
//! | `unavailable` | engine unavailable | the built-in analyzer |
//! | `timeout` | timed out | the built-in analyzer |
//! | `malformed` | output could not be read | the built-in analyzer |
//! | `spans` | an analysis covering nothing, which the span contract refuses | the built-in analyzer |
//! | `both` | engine unavailable | **also** unavailable, so the previous passage must survive |
//! | `first-then-both` | engine unavailable | the built-in analyzer for the **first** passage only, then unavailable |
//!
//! `both` fails every request, so a window launched with it never shows a passage whose survival
//! could be observed. `first-then-both` is that sequence in one window: the first passage is read
//! by the built-in analyzer (with the passage-fallback notice), and every later one fails on both
//! engines, so the first must stay on screen and the error must name both failures.
//!
//! `scripts/desktop-checkpoint.ps1 -AnalyzerFault <value>` sets it. A release build ignores it
//! entirely: this module is not compiled there.

use std::sync::Arc;

use sakatsu_core::analyzer::{AnalyzeError, Analyzer};
use sakatsu_core::model::{Analysis, AnalyzerPolicy};

use crate::route::{Readiness, Route};

/// The environment variable the checkpoint script sets.
pub const FAULT_ENV: &str = "SAKATSU_ANALYZER_FAULT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Unavailable,
    TimedOut,
    Malformed,
    Spans,
    Both,
    FirstThenBoth,
}

/// The fault asked for, if any. An unrecognised value is ignored with a log line rather than
/// guessed at.
pub fn mode() -> Option<Mode> {
    let value = std::env::var(FAULT_ENV).ok()?;
    let mode = match value.trim().to_ascii_lowercase().as_str() {
        "unavailable" => Mode::Unavailable,
        "timeout" => Mode::TimedOut,
        "malformed" => Mode::Malformed,
        "spans" => Mode::Spans,
        "both" => Mode::Both,
        "first-then-both" => Mode::FirstThenBoth,
        other => {
            sakatsu_core::log_warn!("analyzer", "ignoring {FAULT_ENV}={other:?}");
            return None;
        }
    };
    Some(mode)
}

/// An analyzer that fails the same way on every passage.
pub struct Faulting {
    mode: Mode,
    engine: &'static str,
}

impl Analyzer for Faulting {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AnalyzeError::EmptyInput);
        }
        let said = format!("{FAULT_ENV} is set");
        match self.mode {
            Mode::Unavailable | Mode::Both | Mode::FirstThenBoth => {
                Err(AnalyzeError::EngineUnavailable(said))
            }
            Mode::TimedOut => Err(AnalyzeError::TimedOut(said)),
            Mode::Malformed => Err(AnalyzeError::BadOutput(said)),
            // No tokens over non-empty text: the span contract refuses it after display, which
            // is the path an invalid finished analysis really takes.
            Mode::Spans => Ok(Analysis {
                text: text.to_string(),
                tokens: Vec::new(),
                execution: None,
            }),
        }
    }

    fn engine(&self) -> &str {
        self.engine
    }
}

/// The built-in analyzer for a set number of passages, then unavailable.
///
/// Counts passages it was asked to read, not answers it gave: a passage it fails on still uses up
/// nothing, because it only fails once the allowance is spent. Empty input is refused without
/// counting, as the real analyzer refuses it.
pub struct ThenUnavailable {
    inner: Arc<dyn Analyzer>,
    remaining: std::sync::atomic::AtomicUsize,
}

impl ThenUnavailable {
    pub fn new(inner: Arc<dyn Analyzer>, passages: usize) -> ThenUnavailable {
        ThenUnavailable {
            inner,
            remaining: std::sync::atomic::AtomicUsize::new(passages),
        }
    }
}

impl Analyzer for ThenUnavailable {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        if text.trim().is_empty() {
            return Err(AnalyzeError::EmptyInput);
        }
        // `fetch_update` was renamed `try_update` in Rust 1.95; the workspace's floor is 1.92, so
        // the old name stays until the floor reaches 1.95.
        #[allow(deprecated)]
        let spent = self
            .remaining
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |left| left.checked_sub(1),
            )
            .is_err();
        if spent {
            return Err(AnalyzeError::EngineUnavailable(format!(
                "{FAULT_ENV}=first-then-both: the built-in analyzer's one passage is spent"
            )));
        }
        self.inner.analyze(text)
    }

    fn engine(&self) -> &str {
        self.inner.engine()
    }
}

/// The route a fault replaces the real one with: ready at once, so every passage shows the
/// behaviour, and never touching the sidecar.
pub fn route(policy: AnalyzerPolicy, built_in: Arc<dyn Analyzer>, mode: Mode) -> Route {
    let fallback: Arc<dyn Analyzer> = match mode {
        Mode::Both => Arc::new(Faulting {
            mode: Mode::Unavailable,
            engine: "lindera",
        }),
        Mode::FirstThenBoth => Arc::new(ThenUnavailable::new(built_in, 1)),
        _ => built_in,
    };
    Route {
        policy,
        primary: Arc::new(Faulting {
            mode,
            engine: "ichiran",
        }),
        fallback: Some(fallback),
        readiness: Readiness::Ready,
        generation: 0,
    }
}
