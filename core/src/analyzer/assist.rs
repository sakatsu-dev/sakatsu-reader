//! The embedded parser's advice to one request, asked once and accounted for.
//!
//! # Four questions, one provider
//!
//! On the Ichiran path the embedded parser is not the analyzer, it is the advisor, and it is
//! asked four kinds of question:
//!
//! | [`Stage`] | Asked by | About |
//! |---|---|---|
//! | `Passage` | the contextual tie-break; the built-in analyzer itself | the whole passage |
//! | `Sentence` | the contraction repairs | one sentence |
//! | `Word` | the bare-word tie-break | one tied surface, alone |
//! | `JoinedLines` | line-break classification | two lines joined with nothing between |
//!
//! Each of those once reached the parser on its own, with no memory of what another had asked
//! and nothing counting any of it. [`Assistance`] is the one place they now go: owned by **one
//! request**, it holds the provider, a memo of the answers it gave, and the account.
//!
//! # What is remembered, and what never is
//!
//! **Only an answer.** A question that could not be answered — no provider, or a provider that
//! failed — is counted and forgotten, so the next caller that needs it asks again. That is the
//! rule the callers already had, kept at the scope they had it: the contextual tie-break still
//! settles a failure once per conversion, and the repairs still settle one per sentence within
//! one pass, because those are *their* memories and live in their own code. What this adds is
//! that a successful answer is not paid for twice: the passage an eager pass read is the passage
//! the tie-break and the fallback read.
//!
//! The key is the stage **and** the exact question **and** the provider's identity. A passage is
//! never served for a sentence that happens to be spelled the same, and an answer from one
//! parser is never served as another's.
//!
//! # No borrow and no lock across a question
//!
//! Answering a passage asks joined-line questions of its own, through this same value. So the memo
//! is consulted and released, the provider is called with nothing held, and the answer is put
//! back afterwards — a nested question can neither panic on a borrow nor wait on a lock its own
//! caller holds. The parser's tokenizer lock is scoped inside `LinderaAnalyzer::pieces` for the
//! same reason.
//!
//! # Nothing here reaches Ichiran
//!
//! The advisor is consulted *by* the Ichiran conversion. Were it ever to start one, the conversion
//! would consult the advisor, which would start another. [`Assistance::refuse_if_advising`] is the
//! guard: the conversion asks it first, and while a question is being answered it is refused.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::analyzer::{AnalyzeError, Analyzer};
use crate::model::Analysis;

/// How a request uses its advisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AssistanceMode {
    /// Every question is put to the provider, as it was before this module: nothing is
    /// remembered, the provider is resolved where each caller resolved it, and every question is
    /// still counted — which is what makes the old volume measurable.
    Legacy,
    /// One request, one memo: an answer is reused by every later caller with the same question.
    /// The provider is resolved on the first question.
    SharedLazy,
    /// [`AssistanceMode::SharedLazy`], with the passage read once at the start of the request, so
    /// whichever caller needs it first finds it ready.
    SharedEager,
}

/// What kind of question is being asked. Part of the memo key, so the kinds never mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    Passage,
    Sentence,
    Word,
    JoinedLines,
}

impl Stage {
    pub const ALL: [Stage; 4] = [
        Stage::Passage,
        Stage::Sentence,
        Stage::Word,
        Stage::JoinedLines,
    ];

    fn slot(self) -> usize {
        self as usize
    }
}

/// Which parser answered. Two providers with one identity must give one answer to one question;
/// the embedded dictionary is compiled in, so every Lindera instance in a build shares one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProviderId(pub &'static str);

/// Something that can read Japanese on the request's behalf.
///
/// **Not [`Analyzer`].** An analyzer answers a reader; an advisor answers the analysis, and is
/// handed the request's [`Assistance`] so the questions *it* needs to ask — the joined lines inside
/// a passage — go through the same memo instead of around it.
pub trait Advisor {
    fn identity(&self) -> ProviderId;
    /// Read `text` afresh. This is the uncached computation: the memo is [`Assistance::ask`]'s.
    fn read(&self, text: &str, assistance: &Assistance<'_>) -> Result<Analysis, AnalyzeError>;
}

/// Any analyzer as an advisor, for the callers that were handed one rather than the provider.
///
/// Its questions are its own, so its identity is one no other provider shares.
pub struct AnalyzerAdvisor<'a>(pub &'a dyn Analyzer);

impl Advisor for AnalyzerAdvisor<'_> {
    fn identity(&self) -> ProviderId {
        ProviderId("an analyzer handed in by the caller")
    }

    fn read(&self, text: &str, _: &Assistance<'_>) -> Result<Analysis, AnalyzeError> {
        self.0.analyze(text)
    }
}

/// The answer to one question.
#[derive(Debug, Clone)]
pub enum Advice {
    Answered(Rc<Analysis>),
    /// No provider could be loaded.
    Unavailable,
    /// The provider was asked and failed.
    Failed(AnalyzeError),
}

impl Advice {
    pub fn answered(&self) -> Option<&Analysis> {
        match self {
            Advice::Answered(analysis) => Some(analysis),
            Advice::Unavailable | Advice::Failed(_) => None,
        }
    }
}

/// One stage's questions over one request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageAccount {
    /// Questions asked.
    pub requested: usize,
    /// Answered from the memo, without the provider.
    pub remembered: usize,
    /// Put to the provider.
    pub executed: usize,
    /// No provider could be loaded.
    pub unavailable: usize,
    /// The provider failed.
    pub failed: usize,
    /// Answered, and the caller found nothing it could use: no token at the span, a word the
    /// parser split, advice that did not align.
    pub unused: usize,
    /// Time inside the provider. **Inclusive**: a passage's time contains the joined-line
    /// questions it asked while being read.
    pub elapsed: Duration,
}

/// Every question one request asked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AdviceAccount {
    /// Indexed by [`Stage`], in [`Stage::ALL`] order.
    pub stages: [StageAccount; 4],
    /// Ichiran conversions refused because they were asked for from inside a question.
    pub refused: usize,
    /// Whether this request was the one that loaded the shared provider.
    pub loaded_provider: bool,
    /// Time spent resolving the provider, loading included.
    pub resolving: Duration,
    /// The eager pass's time, when there was one.
    pub prefetch: Option<Duration>,
}

impl AdviceAccount {
    pub fn stage(&self, stage: Stage) -> &StageAccount {
        &self.stages[stage.slot()]
    }

    /// Questions that reached the provider, across stages.
    pub fn executed(&self) -> usize {
        self.stages.iter().map(|stage| stage.executed).sum()
    }
}

/// Where the provider comes from.
enum Source<'p> {
    /// The shared embedded parser, loaded on first use.
    Shared,
    /// One handed in: a test's, or the advisor a caller was given.
    Given(Option<&'p dyn Advisor>),
}

type Memo = HashMap<(Stage, ProviderId), HashMap<String, Rc<Analysis>>>;

/// The advisor, its answers and its account, for **one** request.
///
/// Single-threaded on purpose: a request runs on one thread, and nothing here outlives it.
pub struct Assistance<'p> {
    mode: AssistanceMode,
    source: Source<'p>,
    provider: OnceCell<Option<&'p dyn Advisor>>,
    memo: RefCell<Memo>,
    advising: Cell<usize>,
    account: RefCell<AdviceAccount>,
}

impl Assistance<'static> {
    /// Advice from the shared embedded parser.
    pub fn new(mode: AssistanceMode) -> Assistance<'static> {
        Assistance::from_source(mode, Source::Shared)
    }
}

impl<'p> Assistance<'p> {
    /// Advice from this provider, or from none.
    pub fn with_provider(
        mode: AssistanceMode,
        provider: Option<&'p dyn Advisor>,
    ) -> Assistance<'p> {
        Assistance::from_source(mode, Source::Given(provider))
    }

    fn from_source(mode: AssistanceMode, source: Source<'p>) -> Assistance<'p> {
        Assistance {
            mode,
            source,
            provider: OnceCell::new(),
            memo: RefCell::new(HashMap::new()),
            advising: Cell::new(0),
            account: RefCell::new(AdviceAccount::default()),
        }
    }

    pub fn mode(&self) -> AssistanceMode {
        self.mode
    }

    fn remembers(&self) -> bool {
        self.mode != AssistanceMode::Legacy
    }

    fn note(&self, change: impl FnOnce(&mut AdviceAccount)) {
        change(&mut self.account.borrow_mut());
    }

    /// The provider, resolved on first use.
    ///
    /// **A provider that failed to load is not retried within the request**, and across requests
    /// only as the shared parser's own `OnceLock` allows — the one remembered failure there was
    /// before this module, kept as it was.
    pub fn provider(&self) -> Option<&'p dyn Advisor> {
        *self.provider.get_or_init(|| {
            let started = Instant::now();
            let (provider, loaded) = match self.source {
                Source::Shared => {
                    let loaded = !crate::analyzer::lindera::shared_loaded();
                    let shared =
                        crate::analyzer::lindera::shared().map(|parser| parser as &dyn Advisor);
                    (shared, loaded && shared.is_some())
                }
                Source::Given(provider) => (provider, false),
            };
            self.note(|account| {
                account.resolving += started.elapsed();
                account.loaded_provider |= loaded;
            });
            provider
        })
    }

    /// Ask the provider a question.
    pub fn ask(&self, stage: Stage, query: &str) -> Advice {
        self.note(|account| account.stages[stage.slot()].requested += 1);
        let Some(provider) = self.provider() else {
            return self.unavailable(stage);
        };
        if let Some(answer) = self.recall(stage, provider.identity(), query) {
            return answer;
        }
        self.execute(stage, query, provider)
    }

    /// Ask a particular advisor — the built-in analyzer answering its own passage, or the joined
    /// lines inside one. Remembered under its identity, so a provider with the same identity
    /// finds the answer and one without never does.
    pub fn ask_of(&self, stage: Stage, query: &str, advisor: &dyn Advisor) -> Advice {
        self.note(|account| account.stages[stage.slot()].requested += 1);
        if let Some(answer) = self.recall(stage, advisor.identity(), query) {
            return answer;
        }
        self.execute(stage, query, advisor)
    }

    fn unavailable(&self, stage: Stage) -> Advice {
        self.note(|account| account.stages[stage.slot()].unavailable += 1);
        Advice::Unavailable
    }

    /// The remembered answer, with the borrow released before it is returned.
    fn recall(&self, stage: Stage, identity: ProviderId, query: &str) -> Option<Advice> {
        if !self.remembers() {
            return None;
        }
        let found = self
            .memo
            .borrow()
            .get(&(stage, identity))
            .and_then(|answers| answers.get(query))
            .cloned()?;
        self.note(|account| account.stages[stage.slot()].remembered += 1);
        Some(Advice::Answered(found))
    }

    /// Put the question to `advisor` with **nothing held**, then remember a success.
    fn execute(&self, stage: Stage, query: &str, advisor: &dyn Advisor) -> Advice {
        self.advising.set(self.advising.get() + 1);
        let started = Instant::now();
        let outcome = advisor.read(query, self);
        let elapsed = started.elapsed();
        self.advising.set(self.advising.get() - 1);
        self.note(|account| {
            let counted = &mut account.stages[stage.slot()];
            counted.executed += 1;
            counted.elapsed += elapsed;
            if outcome.is_err() {
                counted.failed += 1;
            }
        });
        match outcome {
            Ok(analysis) => {
                let analysis = Rc::new(analysis);
                if self.remembers() {
                    self.memo
                        .borrow_mut()
                        .entry((stage, advisor.identity()))
                        .or_default()
                        .insert(query.to_string(), analysis.clone());
                }
                Advice::Answered(analysis)
            }
            Err(error) => Advice::Failed(error),
        }
    }

    /// The caller had an answer and could not use it.
    pub fn unused(&self, stage: Stage) {
        self.note(|account| account.stages[stage.slot()].unused += 1);
    }

    /// Read the passage now, in [`AssistanceMode::SharedEager`]; nothing otherwise.
    ///
    /// **A failure here suppresses nothing.** It is counted and not remembered, so the first
    /// caller that actually needs the passage asks again, and a fallback still runs its own
    /// analysis.
    pub fn prefetch(&self, text: &str) {
        if self.mode != AssistanceMode::SharedEager {
            return;
        }
        let text = crate::analyzer::lines::normalise_breaks(text);
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let started = Instant::now();
        let _ = self.ask(Stage::Passage, text);
        let elapsed = started.elapsed();
        self.note(|account| account.prefetch = Some(elapsed));
    }

    /// Whether a question is being answered right now.
    pub fn advising(&self) -> bool {
        self.advising.get() > 0
    }

    /// Refuse an Ichiran conversion asked for from inside a question. See the module header.
    pub fn refuse_if_advising(&self) -> Result<(), AnalyzeError> {
        if !self.advising() {
            return Ok(());
        }
        self.note(|account| account.refused += 1);
        Err(AnalyzeError::EngineUnavailable(
            "Ichiran was asked for from inside the embedded parser's advice; refused so that \
             assistance cannot recurse"
                .into(),
        ))
    }

    pub fn account(&self) -> AdviceAccount {
        *self.account.borrow()
    }
}
