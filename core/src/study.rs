//! Everything the reader needs for one passage, assembled in one place.
//!
//! # What this is, and why it is here rather than in the shell
//!
//! [`pipeline`](crate::pipeline) ends at the finished token list. Everything after it — grammar
//! breakdown, pattern matching, sentence ranges, study keys, vocabulary status, furigana — is a
//! pure function of that list, and it all lived in `commands::study` in the desktop shell. That
//! put a passage's worth of reading logic behind a Tauri boundary, where it could only be
//! exercised by constructing an app.
//!
//! Every type it produces was already a `core` type. So the work moves here and the shell keeps
//! what is genuinely its own: acquiring state, registering commands, logging, and turning an error
//! into a string for transport.
//!
//! # The resource contract, stated exactly
//!
//! [`Resources`] is **stable handles for the invocation, not a transaction.** The caller resolves
//! each live handle once and lends it, so no stage re-resolves halfway through a passage. That is
//! all it is, and it is worth being precise about what it is not:
//!
//! - It is **not an atomic snapshot across resources**. Four independent `Live::now()` calls can
//!   straddle a replacement of one of them. Nothing here needs them to be simultaneous — a
//!   passage analysed with one dictionary generation and rendered with a kanji table swapped a
//!   millisecond later is still internally consistent — and pretending otherwise would mean
//!   inventing a synchronisation scheme with no demonstrated requirement behind it.
//! - Borrowing `&Store` **does not freeze vocabulary status**. `Store` is internally locked and
//!   another thread can mark a word mid-call. What holds is that statuses are read **together**:
//!   [`Store::statuses`](crate::store::Store::statuses) takes the lock once for the whole key
//!   batch, so a response is built from one consistent read rather than one lookup per token.
//!
//! Two of the three were already right in the shell and are preserved rather than introduced: the
//! kanji table was taken once with a comment saying why, and the dictionary snapshot is the one
//! `pipeline` already requires.
//!
//! # No store is a supported state
//!
//! `store: None` is not an error path. A store that could not be opened leaves the reader working
//! with every word unmarked, which is the degradation the shell already chose — losing the ability
//! to save is not a reason to lose the ability to read.

use crate::analyzer::assist::{AdviceAccount, Advisor, Assistance, AssistanceMode};
use crate::analyzer::{AnalyzeError, Analyzer};
use crate::dict::evidence::{Evidence, EvidenceAccount, EvidenceCache};
use crate::dict::index::Dictionary;
use crate::dict::{KanjiDic, Ruby};
use crate::grammar::{self, Breakdown, Match};
use crate::model::{
    self, Analysis, AnalyzerPolicy, Execution, Fallback, FallbackReason, FallbackScope,
    SentenceRange,
};
use crate::pipeline::Pipeline;
use crate::store::{Store, WordKey, WordStatus};

use serde::Serialize;

/// The resources one invocation runs against, lent by whoever resolved them.
///
/// Borrowed rather than owned so the caller keeps control of lifetime and replacement: this type
/// never reaches for a live handle, which is what makes "the whole call saw one set of
/// dictionaries" a property of the signature instead of a promise in a comment.
#[derive(Clone, Copy)]
pub struct Resources<'a> {
    pub analyzer: &'a dyn Analyzer,
    pub dictionaries: &'a [Dictionary],
    pub kanji: &'a KanjiDic,
    /// `None` when no store could be opened. Every word is then unmarked.
    pub store: Option<&'a Store>,
}

/// Everything the study UI needs for one passage, in one round trip.
///
/// One call rather than four: the UI would otherwise have to hand tokens back to
/// get their grammar and status, and a passage is analysed as a whole anyway.
///
/// **Field order is the wire format.** `ui/src/model.ts` declares this shape and
/// `ui/src/model.golden.json` is generated from it; moving this type from the shell into `core`
/// changed where it is defined and nothing about what it serialises to.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Serialize)]
pub struct StudyView {
    pub analysis: Analysis,
    /// The exact partition the core's sentence-sensitive features use.
    ///
    /// Sent with the analysis so the reader can keep a comparison inside one sentence without
    /// copying the boundary rule into the UI.
    pub sentences: Vec<SentenceRange>,
    /// Parallel to `analysis.tokens` — one breakdown per token, empty where the word
    /// has no grammar to explain.
    pub breakdowns: Vec<Breakdown>,
    /// Grammar patterns spanning the token sequence.
    pub patterns: Vec<Match>,
    /// Parallel to `analysis.tokens` — how well the learner knows each word.
    pub statuses: Vec<WordStatus>,
    /// Parallel to `analysis.tokens` — how each word should be set, with the kana over the
    /// characters they belong to where that can be established.
    ///
    /// Computed here rather than asked for per word: the reader draws every token at once,
    /// and a round trip each would be hundreds for a page.
    pub ruby: Vec<Vec<Ruby>>,
}

/// Analyse a passage: the analyzer, then the displayed pipeline, and nothing else.
///
/// The smallest useful thing, and a strict prefix of [`study`]. Kept separate because a caller may
/// want the tokens without a passage's worth of derivations — and because the two commands
/// disagreeing about their readings is a bug this project has already had once.
/// **Takes what it needs, not a `Resources`.** It reads neither the kanji table nor the store, and
/// a caller handed a struct with two fields it ignores is a caller invited to wonder which of them
/// mattered — or worse, to fabricate an empty one to fill the gap.
pub fn analyse(
    text: &str,
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
) -> Result<Analysis, AnalyzeError> {
    run(text, analyzer, dictionaries, AnalysisOptions::PRODUCTION)
        .map(|run| run.analysis)
        .map_err(|failure| failure.error)
}

/// What a [`run`] does beyond the analyzer and the display pipeline.
///
/// **Options, not variants of the function.** Three entry points once repeated the analyzer, the
/// pipeline and the span check between them, and the harnesses called two of the three. A stage
/// added to one would have reached the reader and not the measurement — or the reverse — with
/// nothing to say so. Every entry point is now [`run`] with a value of this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisOptions {
    /// Rejoin contracted verb forms the engine split ([`crate::lexical::repair`]). **Ichiran only**:
    /// the built-in analyzer composes them itself, and a stage that ran on its output would be a
    /// second opinion on the advisor's own advice.
    pub repairs: bool,
    /// Form dictionary-backed units ([`crate::lexical`]).
    pub grouping: bool,
    /// Keep every intermediate analysis in [`Run::stages`]. Changes what is *kept*, never what is
    /// computed — `run_is_one_path_whatever_it_keeps` pins that.
    pub trace: bool,
    /// How the request asks the embedded parser for advice and remembers what the dictionaries
    /// said ([`AnalysisContext`]). Changes how often each question is asked, never its answer —
    /// `core/tests/assistance_context.rs` and the reader-equivalence measurement pin that.
    pub assistance: AssistanceMode,
}

/// The assistance the reader is served with, decided by measurement.
/// **Shared and lazy**: identical output on every recorded passage, within every latency
/// budget, and fewer dictionary reads. The eager pass stays a diagnostic option — it reads every
/// passage whether or not anything needs it, and a verified cache hit paid for that past its p95
/// budget.
pub const ASSISTANCE: AssistanceMode = AssistanceMode::SharedLazy;

impl AnalysisOptions {
    /// What the reader is served.
    pub const PRODUCTION: AnalysisOptions = AnalysisOptions {
        repairs: true,
        grouping: true,
        trace: false,
        assistance: ASSISTANCE,
    };
    /// The same, without grouping: what grouping is a change *to*.
    pub const WITHOUT_GROUPING: AnalysisOptions = AnalysisOptions {
        repairs: true,
        grouping: false,
        trace: false,
        assistance: ASSISTANCE,
    };
    /// These options without the contraction repairs: what the repairs are a change *to*, paired
    /// within one run.
    pub const fn without_repairs(self) -> AnalysisOptions {
        AnalysisOptions {
            repairs: false,
            ..self
        }
    }

    /// These options, with every stage kept.
    pub const fn traced(self) -> AnalysisOptions {
        AnalysisOptions {
            trace: true,
            ..self
        }
    }

    /// These options, with the request's assistance asked another way.
    pub const fn with_assistance(self, assistance: AssistanceMode) -> AnalysisOptions {
        AnalysisOptions { assistance, ..self }
    }
}

/// What one request knows while it runs: the embedded parser's advice and the dictionaries'
/// answers, each asked once where the mode allows.
///
/// **Two halves, on purpose.** Nothing under `analyzer/` reads a dictionary — an analyzer's output
/// must not depend on what is installed — so the half an analyzer is handed, [`Assistance`], holds
/// the advice and nothing else, and the dictionary half stays here, where the stages that read
/// dictionaries are run.
///
/// **One per request, and only one.** [`analyse_resilient`] makes one, and the primary run and
/// the operational retry share it: the passage the tie-break read is the passage the fallback
/// receives, and a lookup the first attempt made is not made again by the second. Nothing in it
/// outlives the request.
pub struct AnalysisContext<'p> {
    pub assistance: Assistance<'p>,
    pub evidence: EvidenceCache,
}

impl AnalysisContext<'static> {
    /// Advice from the shared embedded parser.
    pub fn new(mode: AssistanceMode) -> AnalysisContext<'static> {
        AnalysisContext {
            assistance: Assistance::new(mode),
            evidence: EvidenceCache::new(mode != AssistanceMode::Legacy),
        }
    }
}

impl<'p> AnalysisContext<'p> {
    /// Advice from this provider, or none — for tests that script one.
    pub fn with_provider(
        mode: AssistanceMode,
        provider: Option<&'p dyn Advisor>,
    ) -> AnalysisContext<'p> {
        AnalysisContext {
            assistance: Assistance::with_provider(mode, provider),
            evidence: EvidenceCache::new(mode != AssistanceMode::Legacy),
        }
    }

    pub fn account(&self) -> AssistanceAccount {
        AssistanceAccount {
            mode: self.assistance.mode(),
            advice: self.assistance.account(),
            evidence: self.evidence.account(),
        }
    }
}

/// Every question one request asked, of the parser and of the dictionaries. Diagnostic only: it
/// reaches no reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssistanceAccount {
    pub mode: AssistanceMode,
    pub advice: AdviceAccount,
    pub evidence: EvidenceAccount,
}

/// One passage through the whole path, and whatever was asked to be kept.
#[derive(Debug, Clone)]
pub struct Run {
    /// What the reader would be shown for these options.
    pub analysis: Analysis,
    /// The account of grouping, when grouping ran.
    pub grouping: Option<crate::lexical::Assembly>,
    /// The account of the contraction repairs: empty when they did not run.
    pub repairs: crate::lexical::repair::Account,
    /// Every intermediate analysis, when [`AnalysisOptions::trace`] asked for them.
    pub stages: Option<Stages>,
    /// Every question the request asked, up to the end of this run.
    pub assistance: AssistanceAccount,
}

/// The intermediate analyses of one [`run`], in the order they were produced.
#[derive(Debug, Clone)]
pub struct Stages {
    /// The engine's own output, where it has one ([`crate::analyzer::Traced::raw`]).
    pub raw: Option<String>,
    /// The analyzer's answer: for Ichiran, conversion including the tie-break and line marking.
    pub converted: Analysis,
    /// After [`Pipeline::display`] — the joins and the reading correction.
    pub displayed: Analysis,
    /// After the contraction repairs; equal to `displayed` where none applied.
    pub repaired: Analysis,
}

/// A [`run`] that did not finish, with whatever stages it got through when tracing.
///
/// The stages matter most exactly here: a passage refused by the span contract is refused *after*
/// the display pipeline, and "which stage broke it" cannot be answered from the error alone.
#[derive(Debug, Clone)]
pub struct RunFailure {
    pub error: AnalyzeError,
    pub stages: Option<Box<Stages>>,
}

/// **The one implementation** of the analysis path. Every other entry point delegates here.
///
/// The order is load-bearing: the analyzer; the display pipeline; the span contract, which fails
/// closed; then grouping, which reasons from a baseline that satisfies that contract and validates
/// its own result before returning it (`lexical::assembled`).
pub fn run(
    text: &str,
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
    options: AnalysisOptions,
) -> Result<Run, RunFailure> {
    run_in(
        text,
        analyzer,
        dictionaries,
        options,
        &AnalysisContext::new(options.assistance),
    )
}

/// [`run`], as one request inside `context` — whose mode, not `options.assistance`, decides how
/// the request asks.
pub fn run_in(
    text: &str,
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
    options: AnalysisOptions,
    context: &AnalysisContext<'_>,
) -> Result<Run, RunFailure> {
    // A word the text box wrapped is one word again before anything reads it.
    let joined = crate::analyzer::lines::rejoin_split_words_in(text, &context.assistance);
    let text = joined.as_ref();
    context.assistance.prefetch(text);
    pass(text, analyzer, dictionaries, options, context)
}

/// One analyzer's attempt at the request. [`analyse_resilient`] makes two of these inside one
/// request, which is why the eager pass is not in here.
fn pass(
    text: &str,
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
    options: AnalysisOptions,
    context: &AnalysisContext<'_>,
) -> Result<Run, RunFailure> {
    let fail = |error: AnalyzeError, stages: Option<Stages>| RunFailure {
        error,
        stages: stages.map(Box::new),
    };
    let evidence = Evidence::through(dictionaries, &context.evidence);
    // **One call whatever is kept.** The engine's own output is dropped unless tracing asked for
    // it, so a trace can never be describing a computation the reader was not served.
    let traced = analyzer
        .analyze_in(text, &context.assistance)
        .map_err(|error| fail(error, None))?;
    let raw = if options.trace { traced.raw } else { None };
    let mut analysis = traced.analysis;
    let converted = options.trace.then(|| analysis.clone());
    Pipeline::with_evidence(evidence).display(&mut analysis);
    // Which tokens and parts a reader may select, decided from the source. Before the stages are
    // kept, so the displayed stage is exactly what an ungrouped run returns.
    crate::analyzer::mark_selectable(&mut analysis);
    let displayed = converted.as_ref().map(|_| analysis.clone());
    let partial = |repaired: &Analysis| {
        converted.clone().map(|converted| Stages {
            raw: raw.clone(),
            converted,
            displayed: displayed.clone().unwrap_or_default(),
            repaired: repaired.clone(),
        })
    };
    if let Err(error) = check_spans(&analysis) {
        return Err(fail(error, partial(&analysis)));
    }
    // **After the span contract, before grouping.** The repairs reason from output that satisfies
    // the contract, and grouping reasons from theirs.
    let (analysis, repairs) = if options.repairs && analyzer.engine() == "ichiran" {
        repair(analysis, evidence, &context.assistance)
    } else {
        (analysis, crate::lexical::repair::Account::default())
    };
    let stages = partial(&analysis);
    // **Grouping belongs here, not above `from_analysis`.** `analyse` is a strict prefix of
    // `study`, and the two disagreeing about their tokens is a bug this project has already had
    // once. Putting it in the one place makes the analyze command and the study command see the
    // same words by construction rather than by two call sites staying in step.
    let (mut analysis, grouping) = if options.grouping {
        let grouped = analysed_in(analysis, evidence);
        (grouped.analysis, Some(grouped.grouping))
    } else {
        (analysis, None)
    };
    // Again, because grouping makes new tokens whose parts are the old ones.
    crate::analyzer::mark_selectable(&mut analysis);
    Ok(Run {
        analysis,
        grouping,
        repairs,
        stages,
        assistance: context.account(),
    })
}

/// The contraction repairs over a validated analysis, validated again.
///
/// A repaired analysis the span contract refuses is not served: the engine's own output, which
/// passed, is kept, and every repair is turned into the abstention that says why.
fn repair(
    analysis: Analysis,
    evidence: Evidence<'_>,
    assistance: &Assistance<'_>,
) -> (Analysis, crate::lexical::repair::Account) {
    use crate::lexical::repair::{self, Abstain, Outcome};
    // The advisor was resolved here, before any trigger was found, and the legacy mode keeps that
    // so its first-use cost is measured where it was paid. The shared modes resolve it on the
    // first sentence that needs it.
    if assistance.mode() == AssistanceMode::Legacy {
        let _ = assistance.provider();
    }
    let (mut repaired, mut account) = repair::repaired_in(&analysis, evidence, assistance);
    if account.notes().is_empty() {
        return (analysis, account);
    }
    crate::analyzer::mark_selectable(&mut repaired);
    match check_spans(&repaired) {
        Ok(()) => (repaired, account),
        Err(error) => {
            for investigation in &mut account.investigations {
                if matches!(investigation.outcome, Outcome::Repaired(_)) {
                    investigation.outcome =
                        Outcome::Abstained(Abstain::SpanContract(error.to_string()));
                }
            }
            (analysis, account)
        }
    }
}

/// A passage, and **what grouping did to it**.
///
/// `analyse` returns the tokens because that is all its callers want. This returns the tokens and
/// the account: which units were formed, which regions were left alone and why, and whether
/// anything here failed. A measurement that wants to compare the reader's path with the baseline
/// needs the second half, and an evaluation that reconstructed it by running the pieces again
/// would be measuring a path the reader is not served by.
#[derive(Debug, Clone)]
pub struct Analysed {
    pub analysis: Analysis,
    pub grouping: crate::lexical::Assembly,
}

/// Group a validated analysis, keeping the account.
///
/// **Not `pub` by accident.** `analyse` discards the account; this is the same work with it kept,
/// so production and measurement run one code path and differ only in what they look at.
pub fn analysed(analysis: Analysis, dictionaries: &[Dictionary]) -> Analysed {
    analysed_in(analysis, Evidence::direct(dictionaries))
}

fn analysed_in(analysis: Analysis, dictionaries: Evidence<'_>) -> Analysed {
    let grouping = crate::lexical::regrouped_in(&analysis, dictionaries);
    // **A discarded assembly is a defect, and defects are reported.** It leaves the reader with
    // the baseline, which is the right thing to show them, and exactly the wrong thing to say
    // nothing about: without this the only trace would be a passage that quietly stopped
    // grouping.
    if !grouping.is_sound() {
        crate::log::write(
            crate::log::Level::Warn,
            "lexical",
            &format!(
                "grouping: {} — faults {:?}, unapplied {:?}",
                grouping.summary(),
                grouping.faults,
                grouping.unapplied
            ),
        );
    }
    Analysed {
        analysis: grouping.analysis.clone(),
        grouping,
    }
}

/// The whole path a passage takes, with the account kept.
///
/// The measurement harnesses call this so that "the production path" is not a phrase but a
/// function call.
pub fn analyse_with_grouping(
    text: &str,
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
) -> Result<Analysed, AnalyzeError> {
    let run = run(text, analyzer, dictionaries, AnalysisOptions::PRODUCTION)
        .map_err(|failure| failure.error)?;
    Ok(Analysed {
        analysis: run.analysis,
        grouping: run
            .grouping
            .expect("production options always group, so a run under them carries the account"),
    })
}

/// The same passage **without** grouping: the analyzer and the full display pipeline.
///
/// What grouping is a change *to*, and therefore the only honest thing to compare it against.
/// `Pipeline::segmentation` — the joins alone — is a third thing: it is what the historical gates
/// score, and it is not what the reader received either before this layer or after it.
pub fn analyse_without_grouping(
    text: &str,
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
) -> Result<Analysis, AnalyzeError> {
    run(
        text,
        analyzer,
        dictionaries,
        AnalysisOptions::WITHOUT_GROUPING,
    )
    .map(|run| run.analysis)
    .map_err(|failure| failure.error)
}

/// What a passage runs on: the policy in force, the engine it prefers, and the one it may retry on.
///
/// Built by the caller from **one snapshot** of its analyzer route, so the policy, the handles and
/// the reason a stand-in is standing in cannot disagree with each other mid-call.
pub struct Plan<'a> {
    pub requested: AnalyzerPolicy,
    pub primary: &'a dyn Analyzer,
    /// Tried **once**, on an operational failure of the primary. `None` when the primary already
    /// is the built-in analyzer — a failed Lindera is not retried on itself.
    pub fallback: Option<&'a dyn Analyzer>,
    /// Set when the primary is already standing in for the preferred engine — Ichiran still
    /// starting, or unavailable to this session. Recorded on every passage it reads.
    pub standing_in: Option<Fallback>,
}

/// Why a passage has no analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unanswered {
    /// The primary failed and no retry was made: empty input, or no fallback to try.
    Failed { engine: String, error: AnalyzeError },
    /// The primary failed and the request was no longer wanted when the retry would have started.
    Cancelled { engine: String, error: AnalyzeError },
    /// The primary failed, the retry was made, and it failed too.
    BothFailed {
        primary: String,
        primary_error: AnalyzeError,
        fallback: String,
        fallback_error: AnalyzeError,
    },
}

impl std::fmt::Display for Unanswered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unanswered::Failed { error, .. } => write!(f, "{error}"),
            Unanswered::Cancelled { .. } => write!(f, "the request was cancelled"),
            Unanswered::BothFailed {
                primary,
                primary_error,
                fallback,
                fallback_error,
            } => write!(
                f,
                "{primary} could not analyze this passage ({primary_error}), and {fallback} could not either ({fallback_error})"
            ),
        }
    }
}

impl std::error::Error for Unanswered {}

/// The reason a failure is worth one retry on the built-in analyzer, or `None` when it is not.
///
/// **Operational and malformed-output failures only.** An empty passage is not a failure of the
/// engine, and linguistic disagreement never reaches here at all: missing definitions and doubtful
/// readings are properties of a *successful* analysis.
fn retry_reason(error: &AnalyzeError) -> Option<FallbackReason> {
    match error {
        AnalyzeError::EmptyInput => None,
        AnalyzeError::EngineUnavailable(_) => Some(FallbackReason::Unavailable),
        AnalyzeError::TimedOut(_) => Some(FallbackReason::TimedOut),
        AnalyzeError::BadOutput(_) => Some(FallbackReason::MalformedOutput),
        AnalyzeError::InvalidSpans(_) => Some(FallbackReason::InvalidSpans),
    }
}

/// [`run`], with **one visible retry** on the built-in analyzer when the primary fails.
///
/// The retry wraps the whole path — analyzer, display pipeline, span contract, grouping — because
/// a failure introduced by display processing is a failure of the passage as surely as one from
/// the sidecar. It runs on the same text and the same dictionary snapshot, through the same
/// contract, and at most once.
///
/// - `still_wanted` is asked **before** the retry starts; a superseded or cancelled request never
///   launches one. (The caller's own check after this returns still forbids stale publication.)
/// - Empty input never retries.
/// - Nothing here changes any saved preference or the next passage's route: each call decides for
///   itself, so a passage after a failure tries the primary again.
/// - The result carries its [`Execution`]: the policy, the engine that produced it, and the
///   fallback — passage-scoped for a retry, or the plan's session-scoped stand-in.
pub fn analyse_resilient(
    text: &str,
    plan: &Plan<'_>,
    dictionaries: &[Dictionary],
    options: AnalysisOptions,
    still_wanted: &dyn Fn() -> bool,
) -> Result<Run, Unanswered> {
    analyse_resilient_in(
        text,
        plan,
        dictionaries,
        options,
        still_wanted,
        &AnalysisContext::new(options.assistance),
    )
}

/// [`analyse_resilient`], as one request inside `context`.
///
/// **The retry is part of the request.** It shares the context, so it reuses the passage the
/// embedded parser already read — where the fallback *is* that parser — and every lookup the first
/// attempt made. What it never reuses is a failure: the request keeps only answers, so an advisory
/// failure before the retry is not the retry's, and the fallback's own analysis runs.
pub fn analyse_resilient_in(
    text: &str,
    plan: &Plan<'_>,
    dictionaries: &[Dictionary],
    options: AnalysisOptions,
    still_wanted: &dyn Fn() -> bool,
    context: &AnalysisContext<'_>,
) -> Result<Run, Unanswered> {
    let execution = |engine: &str, fallback: Option<Fallback>, repairs| Execution {
        requested: plan.requested,
        engine: engine.to_string(),
        fallback,
        repairs,
    };
    let primary = plan.primary.engine().to_string();
    // Before the first attempt, so the retry reads the same passage ([`run_in`]).
    let joined = crate::analyzer::lines::rejoin_split_words_in(text, &context.assistance);
    let text = joined.as_ref();
    context.assistance.prefetch(text);
    let failure = match pass(text, plan.primary, dictionaries, options, context) {
        Ok(mut done) => {
            let repairs = done.repairs.notes();
            done.analysis.execution = Some(execution(&primary, plan.standing_in.clone(), repairs));
            return Ok(done);
        }
        Err(failure) => failure.error,
    };
    let (Some(reason), Some(fallback)) = (retry_reason(&failure), plan.fallback) else {
        return Err(Unanswered::Failed {
            engine: primary,
            error: failure,
        });
    };
    if !still_wanted() {
        return Err(Unanswered::Cancelled {
            engine: primary,
            error: failure,
        });
    }
    let secondary = fallback.engine().to_string();
    match pass(text, fallback, dictionaries, options, context) {
        Ok(mut done) => {
            let repairs = done.repairs.notes();
            done.analysis.execution = Some(execution(
                &secondary,
                Some(Fallback {
                    reason,
                    scope: FallbackScope::Passage,
                    detail: format!("{primary}: {failure}"),
                }),
                repairs,
            ));
            Ok(done)
        }
        Err(second) => Err(Unanswered::BothFailed {
            primary,
            primary_error: failure,
            fallback: secondary,
            fallback_error: second.error,
        }),
    }
}

/// [`study`], through [`analyse_resilient`]: the reader's whole request, with the one retry.
pub fn study_resilient(
    text: &str,
    plan: &Plan<'_>,
    dictionaries: &[Dictionary],
    kanji: &KanjiDic,
    store: Option<&Store>,
    still_wanted: &dyn Fn() -> bool,
) -> Result<StudyView, Unanswered> {
    let done = analyse_resilient(
        text,
        plan,
        dictionaries,
        AnalysisOptions::PRODUCTION,
        still_wanted,
    )?;
    Ok(from_analysis(
        done.analysis,
        Resources {
            analyzer: plan.primary,
            dictionaries,
            kanji,
            store,
        },
    ))
}

/// The span contract, as an error a caller can show.
fn check_spans(analysis: &Analysis) -> Result<(), AnalyzeError> {
    let faults = crate::analyzer::validate_analysis(analysis);
    if faults.is_empty() {
        return Ok(());
    }
    let detail = faults
        .iter()
        .take(3)
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    let more = faults.len().saturating_sub(3);
    Err(AnalyzeError::InvalidSpans(if more == 0 {
        format!("invalid source spans: {detail}")
    } else {
        format!("invalid source spans: {detail}; and {more} more")
    }))
}

/// The whole of what the reader asks for, over one passage.
///
/// The order is load-bearing and each step says why in place.
pub fn study(text: &str, resources: Resources<'_>) -> Result<StudyView, AnalyzeError> {
    // Before the grammar layer, so a name is one word to it. Left until after, the 川 inside
    // 善福寺川 would be offered an explanation of its own. The correction pass runs inside
    // `Pipeline::display` after the joins, because rejoining changes which surfaces there are to
    // check and a reading for 善福寺川 is a different question from one for 川.
    //
    // Everything below this line is a pure function of the finished token list. That is where the
    // pipeline ends and this function's own work begins.
    let analysis = analyse(text, resources.analyzer, resources.dictionaries)?;
    Ok(from_analysis(analysis, resources))
}

/// Everything the study UI needs, from a token list that is already finished.
///
/// **The half of [`study`] below its own line**, extracted verbatim: a pure function of the
/// tokens, with no analyzer and no pipeline. `study` is that line plus this call, so the two
/// cannot drift.
///
/// It exists because the hybrid assembles a token sequence by splicing regions from two analyses
/// that have *already* been through `Pipeline::display`. Handing that back to `study` would run
/// composition, name joining and reading correction over every token a second time. Everything
/// here is instead **recomputed against the assembled sequence**: breakdowns, patterns, sentence
/// ranges, word keys, statuses and ruby. Nothing is carried over from either original analysis —
/// which matters most for the sentence-scoped work, since a token's neighbours in the assembled
/// passage may not be the ones it had in the analysis it came from.
pub fn from_analysis(analysis: Analysis, resources: Resources<'_>) -> StudyView {
    let breakdowns = analysis
        .tokens
        .iter()
        .map(grammar::breakdown)
        .collect::<Vec<_>>();
    // The reader's view: whole-token rules, and the grammar inside a fused or compound word.
    let patterns = grammar::displayed(&analysis);
    let sentences = model::sentence_ranges(&analysis.tokens);

    // **One locked read for the whole passage, not one per token.** `statuses` takes the store's
    // lock once for the batch, so every word in a response was marked-or-not at the same instant.
    let keys: Vec<WordKey> = analysis.tokens.iter().map(WordKey::for_token).collect();
    let statuses = match resources.store {
        Some(store) => store.statuses(&keys),
        // Without a store every word is simply unmarked; the reader still works.
        None => vec![WordStatus::default(); keys.len()],
    };

    // One table for the whole passage: furigana set half from a table being replaced underneath it
    // would be a strange thing to ship. The caller resolved it once, which is what makes that so.
    let ruby = analysis
        .tokens
        .iter()
        .map(|token| crate::dict::furigana(&token.surface, &token.reading, resources.kanji))
        .collect();

    StudyView {
        analysis,
        sentences,
        breakdowns,
        patterns,
        statuses,
        ruby,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Token;

    struct Fixed(Analysis);

    impl Analyzer for Fixed {
        fn analyze(&self, _: &str) -> Result<Analysis, AnalyzeError> {
            Ok(self.0.clone())
        }

        fn engine(&self) -> &str {
            "fixed"
        }
    }

    fn token(surface: &str, start: Option<usize>, end: Option<usize>) -> Token {
        Token {
            surface: surface.into(),
            dict_form: surface.into(),
            start,
            end,
            ..Default::default()
        }
    }

    fn rejected(analysis: Analysis, expected: &str) {
        let error = analyse(&analysis.text.clone(), &Fixed(analysis), &[])
            .expect_err("an invalid finished analysis must not reach a reader")
            .to_string();
        assert!(error.contains(expected), "{error}");
    }

    #[test]
    fn production_accepts_a_complete_analysis_and_whitespace_gaps() {
        let analysis = Analysis {
            execution: None,
            text: "水 を".into(),
            tokens: vec![token("水", Some(0), Some(3)), token("を", Some(4), Some(7))],
        };
        assert_eq!(
            analyse("水 を", &Fixed(analysis), &[])
                .unwrap()
                .tokens
                .len(),
            2
        );
    }

    #[test]
    fn production_rejects_uncovered_content() {
        rejected(
            Analysis {
                execution: None,
                text: "水を飲む".into(),
                tokens: vec![
                    token("水", Some(0), Some(3)),
                    token("飲む", Some(6), Some(12)),
                ],
            },
            "covered by no token",
        );
    }

    #[test]
    fn production_rejects_ungrounded_and_overlapping_tokens() {
        rejected(
            Analysis {
                execution: None,
                text: "水を".into(),
                tokens: vec![token("水", Some(0), Some(3)), token("を", None, None)],
            },
            "has no source span",
        );
        rejected(
            Analysis {
                execution: None,
                text: "水水".into(),
                tokens: vec![
                    token("水水", Some(0), Some(6)),
                    token("水", Some(3), Some(6)),
                ],
            },
            "before the previous token ends",
        );
    }

    #[test]
    fn production_rejects_offsets_inside_utf8_characters() {
        rejected(
            Analysis {
                execution: None,
                text: "水".into(),
                tokens: vec![token("水", Some(1), Some(3))],
            },
            "outside UTF-8 boundaries",
        );
    }
}
