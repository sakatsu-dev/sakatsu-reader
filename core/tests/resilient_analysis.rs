//! **One visible retry on the built-in analyzer, and nothing more.**
//!
//! `study::analyse_resilient` retries a passage once on the fallback when the primary fails
//! operationally — unavailable, timed out, malformed output, or an analysis that breaks the span
//! contract — and records what happened on the analysis itself. Everything else about it is a
//! refusal, and each refusal is pinned here:
//!
//! - empty input is not an engine failure and is not retried;
//! - a request no longer wanted when the retry would start does not start it;
//! - a primary that *is* the built-in analyzer is not retried on itself;
//! - both failing reports both, and neither is dropped;
//! - nothing is remembered: the next passage asks the primary again.
//!
//! Hermetic: the engines are stubs that count their calls.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use sakatsu_core::analyzer::{AnalyzeError, Analyzer};
use sakatsu_core::model::{
    Analysis, AnalyzerPolicy, Fallback, FallbackReason, FallbackScope, Token,
};
use sakatsu_core::study::{self, AnalysisOptions, Plan, Unanswered};

const TEXT: &str = "水を飲む";

/// A well-formed analysis of [`TEXT`], one token per word.
fn good(text: &str) -> Analysis {
    let mut tokens = Vec::new();
    let mut at = 0;
    for surface in ["水", "を", "飲む"] {
        let start = text[at..]
            .find(surface)
            .map(|offset| at + offset)
            .unwrap_or(at);
        tokens.push(Token {
            surface: surface.into(),
            dict_form: surface.into(),
            reading: surface.into(),
            start: Some(start),
            end: Some(start + surface.len()),
            ..Default::default()
        });
        at = start + surface.len();
    }
    Analysis {
        text: text.into(),
        tokens,
        execution: None,
    }
}

/// An engine that answers from a script, one entry per call, and counts its calls.
struct Scripted {
    name: &'static str,
    script: Mutex<Vec<Result<Analysis, AnalyzeError>>>,
    calls: AtomicUsize,
}

impl Scripted {
    fn new(name: &'static str, script: Vec<Result<Analysis, AnalyzeError>>) -> Self {
        Scripted {
            name,
            script: Mutex::new(script),
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Analyzer for Scripted {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if text.trim().is_empty() {
            return Err(AnalyzeError::EmptyInput);
        }
        let mut script = self.script.lock().expect("script");
        if script.is_empty() {
            Ok(good(text))
        } else {
            script.remove(0)
        }
    }

    fn engine(&self) -> &str {
        self.name
    }
}

fn plan<'a>(primary: &'a Scripted, fallback: Option<&'a Scripted>) -> Plan<'a> {
    Plan {
        requested: AnalyzerPolicy::Automatic,
        primary,
        fallback: fallback.map(|engine| engine as &dyn Analyzer),
        standing_in: None,
    }
}

fn wanted() -> bool {
    true
}

#[test]
fn a_healthy_primary_is_recorded_as_itself_with_no_fallback() {
    let ichiran = Scripted::new("ichiran", vec![]);
    let lindera = Scripted::new("lindera", vec![]);
    let done = study::analyse_resilient(
        TEXT,
        &plan(&ichiran, Some(&lindera)),
        &[],
        AnalysisOptions::PRODUCTION,
        &wanted,
    )
    .expect("analyses");
    let execution = done.analysis.execution.expect("recorded");
    assert_eq!(execution.engine, "ichiran");
    assert_eq!(execution.requested, AnalyzerPolicy::Automatic);
    assert_eq!(execution.fallback, None);
    assert_eq!(lindera.calls(), 0, "no retry when nothing failed");
}

/// Each operational failure retries once on the built-in analyzer, through the same contract, and
/// the reason is recorded as a passage-scoped fallback.
#[test]
fn each_operational_failure_retries_once_and_records_why() {
    let broken = |text: &str| Analysis {
        text: text.into(),
        tokens: Vec::new(),
        execution: None,
    };
    for (failure, reason) in [
        (
            Err(AnalyzeError::EngineUnavailable("wsl is not running".into())),
            FallbackReason::Unavailable,
        ),
        (
            Err(AnalyzeError::TimedOut("no answer within 60 seconds".into())),
            FallbackReason::TimedOut,
        ),
        (
            Err(AnalyzeError::BadOutput("unexpected JSON".into())),
            FallbackReason::MalformedOutput,
        ),
        // An analysis the engine returned and the span contract refuses — found after the display
        // pipeline, which is why the retry wraps the whole path.
        (Ok(broken(TEXT)), FallbackReason::InvalidSpans),
    ] {
        let ichiran = Scripted::new("ichiran", vec![failure]);
        let lindera = Scripted::new("lindera", vec![]);
        let done = study::analyse_resilient(
            TEXT,
            &plan(&ichiran, Some(&lindera)),
            &[],
            AnalysisOptions::PRODUCTION,
            &wanted,
        )
        .unwrap_or_else(|unanswered| panic!("{reason:?}: {unanswered}"));
        assert!(sakatsu_core::analyzer::validate_analysis(&done.analysis).is_empty());
        let execution = done.analysis.execution.expect("recorded");
        assert_eq!(execution.engine, "lindera", "{reason:?}");
        let fallback = execution.fallback.expect("a fallback is recorded");
        assert_eq!(fallback.reason, reason);
        assert_eq!(fallback.scope, FallbackScope::Passage);
        assert!(
            fallback.detail.starts_with("ichiran: "),
            "{}",
            fallback.detail
        );
        assert_eq!(
            (ichiran.calls(), lindera.calls()),
            (1, 1),
            "{reason:?}: once each"
        );
    }
}

#[test]
fn empty_input_is_not_an_engine_failure_and_is_not_retried() {
    let ichiran = Scripted::new("ichiran", vec![]);
    let lindera = Scripted::new("lindera", vec![]);
    let refused = study::analyse_resilient(
        "   ",
        &plan(&ichiran, Some(&lindera)),
        &[],
        AnalysisOptions::PRODUCTION,
        &wanted,
    )
    .expect_err("nothing to analyse");
    assert!(matches!(
        refused,
        Unanswered::Failed {
            error: AnalyzeError::EmptyInput,
            ..
        }
    ));
    assert_eq!(lindera.calls(), 0);
}

#[test]
fn a_request_no_longer_wanted_does_not_start_the_retry() {
    let ichiran = Scripted::new(
        "ichiran",
        vec![Err(AnalyzeError::TimedOut("budget spent".into()))],
    );
    let lindera = Scripted::new("lindera", vec![]);
    let asked = AtomicUsize::new(0);
    let superseded = || {
        asked.fetch_add(1, Ordering::SeqCst);
        false
    };
    let refused = study::analyse_resilient(
        TEXT,
        &plan(&ichiran, Some(&lindera)),
        &[],
        AnalysisOptions::PRODUCTION,
        &superseded,
    )
    .expect_err("cancelled");
    assert!(matches!(refused, Unanswered::Cancelled { .. }));
    assert_eq!(asked.load(Ordering::SeqCst), 1, "asked before the retry");
    assert_eq!(lindera.calls(), 0, "and the retry never started");
}

/// A failed explicit Lindera request has no fallback: it is not retried on the same instance.
#[test]
fn the_built_in_analyzer_is_not_retried_on_itself() {
    let lindera = Scripted::new(
        "lindera",
        vec![Err(AnalyzeError::EngineUnavailable("broken build".into()))],
    );
    let refused = study::analyse_resilient(
        TEXT,
        &Plan {
            requested: AnalyzerPolicy::BuiltIn,
            primary: &lindera,
            fallback: None,
            standing_in: None,
        },
        &[],
        AnalysisOptions::PRODUCTION,
        &wanted,
    )
    .expect_err("no fallback");
    assert!(matches!(refused, Unanswered::Failed { .. }));
    assert_eq!(lindera.calls(), 1);
}

#[test]
fn both_failing_reports_both() {
    let ichiran = Scripted::new(
        "ichiran",
        vec![Err(AnalyzeError::BadOutput("unexpected JSON".into()))],
    );
    let lindera = Scripted::new(
        "lindera",
        vec![Err(AnalyzeError::EngineUnavailable("broken build".into()))],
    );
    let refused = study::analyse_resilient(
        TEXT,
        &plan(&ichiran, Some(&lindera)),
        &[],
        AnalysisOptions::PRODUCTION,
        &wanted,
    )
    .expect_err("both failed");
    let Unanswered::BothFailed {
        primary,
        primary_error,
        fallback,
        fallback_error,
    } = &refused
    else {
        panic!("{refused:?}");
    };
    assert_eq!(
        (primary.as_str(), fallback.as_str()),
        ("ichiran", "lindera")
    );
    assert!(matches!(primary_error, AnalyzeError::BadOutput(_)));
    assert!(matches!(fallback_error, AnalyzeError::EngineUnavailable(_)));
    let said = refused.to_string();
    assert!(
        said.contains("unexpected JSON") && said.contains("broken build"),
        "{said}"
    );
}

/// Nothing is remembered: a failure changes neither the route nor the next passage's first try.
#[test]
fn the_next_passage_asks_the_primary_again() {
    let ichiran = Scripted::new("ichiran", vec![Err(AnalyzeError::TimedOut("once".into()))]);
    let lindera = Scripted::new("lindera", vec![]);
    let the_plan = plan(&ichiran, Some(&lindera));
    let first =
        study::analyse_resilient(TEXT, &the_plan, &[], AnalysisOptions::PRODUCTION, &wanted)
            .expect("retried");
    assert_eq!(
        first.analysis.execution.expect("recorded").engine,
        "lindera"
    );
    let second =
        study::analyse_resilient(TEXT, &the_plan, &[], AnalysisOptions::PRODUCTION, &wanted)
            .expect("recovered");
    let execution = second.analysis.execution.expect("recorded");
    assert_eq!(
        execution.engine, "ichiran",
        "Ichiran is asked again and answers"
    );
    assert_eq!(execution.fallback, None);
    assert_eq!((ichiran.calls(), lindera.calls()), (2, 1));
}

/// A session-scoped stand-in — Ichiran starting, or unavailable to this session — is recorded on
/// every passage it reads, **including** when presentation will be quiet.
#[test]
fn a_standing_in_primary_records_its_session_fallback() {
    let lindera = Scripted::new("lindera", vec![]);
    let standing_in = Fallback {
        reason: FallbackReason::Unavailable,
        scope: FallbackScope::Session,
        detail: "ichiran: not installed".into(),
    };
    let done = study::analyse_resilient(
        TEXT,
        &Plan {
            requested: AnalyzerPolicy::Automatic,
            primary: &lindera,
            fallback: None,
            standing_in: Some(standing_in.clone()),
        },
        &[],
        AnalysisOptions::PRODUCTION,
        &wanted,
    )
    .expect("analyses");
    let execution = done.analysis.execution.expect("recorded");
    assert_eq!(execution.engine, "lindera");
    assert_eq!(execution.fallback, Some(standing_in));
}
