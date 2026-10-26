//! The `Analyzer` trait — the linchpin of the architecture.
//!
//! Japanese morphological analysis is done through this one interface, which
//! yields the normalized [`Analysis`](crate::model::Analysis). Ichiran backs it
//! now (via a sidecar); an embeddable engine (Vibrato/Lindera/Sudachi) can back
//! it later without the grammar layer, dictionaries, or UI changing at all. See
//! `project notes` §2 and §4.

pub mod assist;
mod contract;
pub mod ichiran;
pub mod lindera;
pub mod lines;
pub mod remembering;

pub use contract::{
    mark_selectable, validate as validate_analysis, validate_components, validate_selectable,
    ComponentFault, ComponentFaultKind, SpanFault,
};
pub use ichiran::IchiranAnalyzer;
pub use lindera::LinderaAnalyzer;
pub use remembering::Remembering;

use crate::model::Analysis;

/// Anything that turns Japanese text into the normalized analysis model.
///
/// Implementations must be `Send + Sync` so the desktop shell can hold one behind
/// shared state and call it from command handlers.
pub trait Analyzer: Send + Sync {
    /// Analyse a passage of Japanese text.
    ///
    /// Returns an [`AnalyzeError`] rather than panicking so a flaky sidecar (or a
    /// missing model) degrades to a surfaced error, never a crash.
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError>;

    /// A short identifier for the backing engine, e.g. "ichiran" or "vibrato".
    /// Used in diagnostics and to invalidate caches when the engine changes.
    fn engine(&self) -> &str;

    /// Get ready to be asked, if there is anything to get ready.
    ///
    /// **For the sidecar, and it is worth seven seconds.** Windows tears down the WSL distro
    /// session between commands, and the first `wsl.exe` call after it has gone cold pays the
    /// wake-up — measured at 4.6 s for `wsl -e true` and 7.6 s for a first analysis, against
    /// 0.7 s once the session is up. A reader captures, reads for a minute, captures again:
    /// exactly the rhythm that meets a cold session every single time.
    ///
    /// Called when somebody signals intent — the capture overlay opening, a capture starting —
    /// so the wake-up happens while they are framing the shot rather than after they have
    /// asked for the answer. Nothing here is on a path anybody waits for, and a failure is not
    /// worth reporting: the analysis that follows will say so far better.
    ///
    /// The default does nothing, which is correct for an in-process analyzer. Lindera has no
    /// session to wake and reads in fractions of a millisecond.
    fn warm(&self) {}

    /// The same analysis, with what the engine said before it was converted.
    ///
    /// **For diagnosis, and it must never be a second path.** An implementation that overrides
    /// this makes [`Analyzer::analyze`] delegate to it, so the analysis a trace reports is the
    /// analysis a reader would have received, by construction rather than by two bodies staying
    /// in step. `study::run` is the only caller that asks for it.
    ///
    /// The default has no raw output to give: an in-process analyzer has no such stage, and a
    /// wrapper that answers from a cache ([`Remembering`]) did not run the engine at all. Both are
    /// reported as `raw: None` rather than as an empty string, which would read as an engine that
    /// said nothing.
    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        Ok(Traced {
            raw: None,
            analysis: self.analyze(text)?,
        })
    }

    /// [`Analyzer::analyze_traced`], inside one request's [`assist::Assistance`].
    ///
    /// **The path `study::run` takes**, so the embedded parser's advice is asked through the
    /// request rather than around it. The default ignores the request, which is right for an
    /// analyzer that asks for no advice and serves none: a stub, a fault, a test double. The
    /// Ichiran adapter converts through it; the built-in analyzer answers its passage from it when
    /// the same parser has already read that passage.
    fn analyze_in(
        &self,
        text: &str,
        _assistance: &assist::Assistance<'_>,
    ) -> Result<Traced, AnalyzeError> {
        self.analyze_traced(text)
    }
}

/// One analysis and, where the engine has one, its unconverted output.
#[derive(Debug, Clone)]
pub struct Traced {
    /// The engine's own output, verbatim. `None` where there is no such stage or it was not run.
    pub raw: Option<String>,
    /// Exactly what [`Analyzer::analyze`] returns for the same text.
    pub analysis: Analysis,
}

/// Why an analysis could not be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalyzeError {
    /// The input had nothing to analyse.
    EmptyInput,
    /// The backing engine is not installed or reachable (e.g. Ichiran sidecar).
    EngineUnavailable(String),
    /// The engine ran but its output could not be understood.
    BadOutput(String),
    /// The engine did not answer within its time budget.
    TimedOut(String),
    /// The finished analysis did not satisfy the span contract.
    InvalidSpans(String),
}

impl std::fmt::Display for AnalyzeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnalyzeError::EmptyInput => write!(f, "no text to analyze"),
            AnalyzeError::EngineUnavailable(m) => write!(f, "analyzer unavailable: {m}"),
            AnalyzeError::BadOutput(m) => write!(f, "analyzer output could not be read: {m}"),
            AnalyzeError::TimedOut(m) => write!(f, "analyzer timed out: {m}"),
            AnalyzeError::InvalidSpans(m) => {
                write!(f, "analyzer output failed the span contract: {m}")
            }
        }
    }
}

impl std::error::Error for AnalyzeError {}

/// A placeholder analyzer that grounds the trait and lets the desktop shell come
/// up before the Ichiran adapter exists. It splits on spacing and hands back the
/// text as a single token — enough to prove the wiring end to end, never shipped.
pub struct StubAnalyzer;

impl Analyzer for StubAnalyzer {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(AnalyzeError::EmptyInput);
        }
        Ok(Analysis {
            execution: None,
            text: trimmed.to_string(),
            tokens: vec![crate::model::Token {
                surface: trimmed.to_string(),
                dict_form: trimmed.to_string(),
                start: Some(0),
                end: Some(trimmed.len()),
                ..Default::default()
            }],
        })
    }

    fn engine(&self) -> &str {
        "stub"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_grounds_the_trait_and_reports_empty_input() {
        let analyzer = StubAnalyzer;
        assert_eq!(analyzer.engine(), "stub");

        let analysis = analyzer.analyze(" 水を飲む ").expect("analyze");
        assert_eq!(analysis.text, "水を飲む");
        assert_eq!(analysis.tokens.len(), 1);
        assert_eq!(analysis.tokens[0].surface, "水を飲む");

        assert_eq!(analyzer.analyze("   "), Err(AnalyzeError::EmptyInput));
    }

    #[test]
    fn an_analyzer_can_be_held_as_a_trait_object() {
        // The desktop shell will hold `Box<dyn Analyzer>` behind shared state;
        // prove the trait is object-safe and Send + Sync.
        let analyzer: Box<dyn Analyzer> = Box::new(StubAnalyzer);
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn Analyzer>();
        assert!(analyzer.analyze("先生").is_ok());
    }
}
