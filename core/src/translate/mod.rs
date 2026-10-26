//! Machine translation, behind a trait like everything else replaceable.
//!
//! ## Translation is opt-in, and labelled
//!
//! `project notes` §7 is emphatic about this: nothing is translated until the learner asks.
//! The point of the app is to read the Japanese, and a translation sitting on screen does
//! the reading for you. There are two ways to ask — opening the Translation section, or
//! ticking words into the comparison — and both are deliberate acts.
//!
//! **What comes back is worth different amounts in different places, and the interface says
//! which.** FuguMT is about 61M parameters. Its whole-sentence readings are the dependable
//! part; a handful of words translated alone are much weaker, and a lone particle is close
//! to guesswork — `core/tests/lens_agreement.rs` puts those at 34.6% and 1.4% against what
//! the sentence actually lost. A blanket "trust nothing" would be easier to write and less
//! use than a floor.

pub mod compare;
pub mod fugumt;
pub mod passage;
pub mod variants;

pub use compare::{lenses_agree, result_says_more, word_diff, Piece};
pub use fugumt::{FuguMt, Knobs};
pub use passage::mark_boundaries;
pub use variants::{variants, VariantSelectionError, Variants};

/// Anything that turns Japanese into English.
pub trait Translator: Send + Sync {
    fn translate(&self, text: &str) -> Result<String, TranslateError>;

    /// A short identifier, e.g. `"fugumt"`.
    fn engine(&self) -> &str;
}

/// Why a passage could not be translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslateError {
    /// Nothing to translate.
    EmptyInput,
    /// The model is not installed.
    EngineUnavailable(String),
    /// The model ran and failed.
    Failed(String),
}

impl std::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranslateError::EmptyInput => write!(f, "nothing to translate"),
            TranslateError::EngineUnavailable(message) => {
                write!(f, "translation unavailable: {message}")
            }
            TranslateError::Failed(message) => write!(f, "translation failed: {message}"),
        }
    }
}

impl std::error::Error for TranslateError {}

/// Stands in when no model is installed, and says so.
pub struct StubTranslator;

impl Translator for StubTranslator {
    fn translate(&self, text: &str) -> Result<String, TranslateError> {
        if text.trim().is_empty() {
            return Err(TranslateError::EmptyInput);
        }
        Err(TranslateError::EngineUnavailable(
            "no translation model is installed — see scripts/fetch-translation-model.ps1".into(),
        ))
    }

    fn engine(&self) -> &str {
        "stub"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_explains_itself_rather_than_returning_empty_text() {
        let error = StubTranslator.translate("水を飲む").expect_err("no engine");
        assert!(matches!(error, TranslateError::EngineUnavailable(_)));
        assert!(error.to_string().contains("model"));

        assert_eq!(
            StubTranslator.translate("   "),
            Err(TranslateError::EmptyInput)
        );
        assert_eq!(StubTranslator.engine(), "stub");
    }
}
