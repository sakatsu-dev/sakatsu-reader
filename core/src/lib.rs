//! # sakatsu-core
//!
//! The platform-independent core of 屠つ (Sakatsu): the normalized analysis model,
//! the [`Analyzer`](analyzer::Analyzer) trait, and (as they land) the grammar
//! engine, dictionaries, and translation orchestration.
//!
//! This crate has **no** dependency on Tauri, the UI, or any OS — a desktop shell
//! and, later, a mobile app both build on it. That separation is what makes the
//! future phone app a reuse of this logic rather than a rewrite. See `project notes`.
//!
//! ## Status
//!
//! Phase 0: the model and the `Analyzer` contract, with a stub implementation.
//! Phase 1 fills in the Ichiran adapter and the grammar engine.

/// TypeScript declarations generated from these types. Behind the `typescript` feature so a
/// shipped build carries no generation machinery — see the module header for why it exists
/// beside `model.golden.json` rather than instead of it.
#[cfg(feature = "typescript")]
pub mod bindings;

pub mod analyzer;
pub mod anki;
pub mod audio;
pub mod construction;
pub mod corpus;
pub mod credits;
pub mod diagnostics;
pub mod dict;
pub mod expression;
pub mod grammar;
pub mod hashing;
pub mod hybrid;
pub mod kana;
pub mod lexical;
pub mod log;
pub mod lookup;
pub mod model;
pub mod net;
pub mod numbers;
pub mod ocr;
pub mod onnx;
pub mod persistence;
pub mod pipeline;
pub mod recovery;
pub mod request;
pub mod runtime_status;
pub mod setup;
pub mod store;
pub mod study;
pub mod terms;
pub mod translate;

pub use analyzer::{AnalyzeError, Analyzer, IchiranAnalyzer};
pub use model::{Alternative, Analysis, Conjugation, Prop, Sense, Token};

pub mod search;

pub mod content;

pub mod update;
