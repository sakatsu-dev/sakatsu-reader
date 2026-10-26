//! One evaluation runner: what was run, against what, and what happened to every case.
//!
//! # The problem this exists for
//!
//! A gate that reports a rate has already thrown away the two things a reader of that rate needs:
//! **what it was run against**, and **whether it ran everything it was supposed to**. Both have
//! gone wrong here. A harness scored a smaller set of dictionaries than the app opens and reported
//! a rate for it; gates reported green while measuring nothing at all; and a run that skipped a
//! prerequisite looked exactly like a run that passed it.
//!
//! So a run is not a number. It is:
//!
//! - a **profile** whose demands are checked, not merely written on the run;
//! - an **identity**: fixture hashes, dictionaries actually opened, engine, and a digest of the
//!   **executable that produced the result** — because a commit hash cannot identify a tree with
//!   two hundred uncommitted paths, and hashing a source tree at test time cannot prove the binary
//!   matches what it hashed;
//! - an **expected case manifest**, reconciled against the results, so a case that crashed,
//!   timed out or was never reached fails the run instead of vanishing from the denominator;
//! - a **persisted artifact**, because a run whose per-case outcomes exist only in scrollback
//!   cannot be diffed against the next one, which is the whole reason to record them.
//!
//! # What an earlier version of this got wrong
//!
//! It held outcomes in memory and printed a summary, and two verdicts were reachable that should
//! not have been:
//!
//! - **every selected case could be `Skipped` and the run still passed** — a gate that skipped its
//!   whole corpus reporting success, which is the exact failure this exists to catch;
//! - **`DesktopAcceptance` returned `Passed`** while printing that observation was outstanding, so
//!   a profile no software can produce reported that software had produced it.
//!
//! Both are now failures, with distinct reasons, and both are pinned in `evaluation_runner.rs`.
//!
//! # Attribution may be unknown
//!
//! [`Outcome::Failed`] carries the stage responsible where it can be established and
//! [`Stage::Unknown`] where it cannot. A guessed stage is worse than an honest gap: it is
//! evidence-shaped and wrong, and this project has spent real time on a diagnosis that was
//! confidently attributed to the wrong pass.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

/// Which prerequisites a run declares, and therefore what its result means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Nothing installed. Fixtures are committed or built by the test itself; this profile gates
    /// an ordinary `cargo test` run.
    Hermetic,
    /// The dictionaries, models and corpora a reader would have. Its prerequisites are required:
    /// a missing one fails the profile.
    InstalledResources,
    /// A person launched the app and watched the affected flow. **Cannot be produced by this
    /// runner** — it always reports outstanding, so a report never claims a check nobody ran.
    DesktopAcceptance,
}

/// What a profile insists a run declare, so the label cannot be worn without the substance.
///
/// **This is what makes a profile executable rather than decorative.** Naming a run
/// `InstalledResources` while declaring no resources described nothing; naming one `Hermetic`
/// while requiring an installed dictionary described the opposite of what it claimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Demand {
    /// At least one **required** resource must be declared.
    RequiredResources,
    /// No required resource may be declared — that is what hermetic means.
    NoRequiredResources,
    /// At least one observation row, and the verdict is always outstanding.
    HumanObservation,
}

impl Profile {
    pub fn demands(self) -> Demand {
        match self {
            Profile::Hermetic => Demand::NoRequiredResources,
            Profile::InstalledResources => Demand::RequiredResources,
            Profile::DesktopAcceptance => Demand::HumanObservation,
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Profile::Hermetic => "hermetic",
            Profile::InstalledResources => "installed-resources",
            Profile::DesktopAcceptance => "desktop-acceptance",
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Profile::Hermetic => "hermetic",
            Profile::InstalledResources => "installed resources",
            Profile::DesktopAcceptance => "desktop acceptance",
        })
    }
}

/// What a requirement is for, which decides what its absence costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Absence fails the profile.
    Required,
    /// Absence leaves a comparison incomplete and the profile can still pass — Ichiran is the
    /// standing example.
    Optional,
    /// A checklist row a person has to look at. Software cannot satisfy one.
    Observation,
}

/// A resource a profile needs, and whether the run can be believed without it.
#[derive(Debug, Clone)]
pub struct Requirement {
    pub name: String,
    pub need: Need,
    pub present: bool,
    /// Why it is absent, when it is. Recorded rather than inferred at reporting time.
    pub note: Option<String>,
}

impl Requirement {
    pub fn required(name: impl Into<String>, present: bool) -> Requirement {
        Requirement {
            name: name.into(),
            need: Need::Required,
            present,
            note: None,
        }
    }

    pub fn optional(name: impl Into<String>, present: bool) -> Requirement {
        Requirement {
            name: name.into(),
            need: Need::Optional,
            present,
            note: None,
        }
    }

    /// A checklist row for a person.
    ///
    /// **There is no `present: true` form on purpose.** This runner has no way to know that
    /// somebody looked at a window, and a parameter it could pass would be a parameter somebody
    /// would eventually pass `true` to.
    pub fn observation(name: impl Into<String>) -> Requirement {
        Requirement {
            name: name.into(),
            need: Need::Observation,
            present: false,
            note: None,
        }
    }

    pub fn because(mut self, note: impl Into<String>) -> Requirement {
        self.note = Some(note.into());
        self
    }

    fn label(&self) -> &'static str {
        match self.need {
            Need::Required => "required",
            Need::Optional => "optional",
            Need::Observation => "observation",
        }
    }
}

/// The stage a difference is attributable to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Analyzer,
    Compounds,
    Names,
    Readings,
    Lookup,
    /// Attribution could not be established. Recorded as such, never guessed.
    Unknown,
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Stage::Analyzer => "analyzer",
            Stage::Compounds => "compounds",
            Stage::Names => "names",
            Stage::Readings => "readings",
            Stage::Lookup => "lookup",
            Stage::Unknown => "unknown",
        })
    }
}

/// What happened to one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed {
        stage: Stage,
        detail: String,
    },
    /// Deliberately not evaluated, with a reason. Never a stand-in for "could not".
    Skipped {
        reason: String,
    },
    /// Started and could not be completed — a crash, a timeout, a missing prerequisite for this
    /// case alone. **Counts against the run.**
    Incomplete {
        reason: String,
    },
    /// Both sides produced output and the two could not be put into correspondence, so no
    /// comparison exists to agree or disagree.
    ///
    /// **Distinct from all three above, and the distinction is the point.** It is not `Failed`:
    /// nothing disagreed, because nothing could be compared, and filing it as a disagreement
    /// would invent a finding. It is not `Skipped`: the case was not declined, it was attempted.
    /// It is not `Incomplete`: nothing crashed and nothing was missing — the engines ran and
    /// answered, and their answers do not line up.
    ///
    /// Without it, a comparison harness has to pick one of the three and each lies differently:
    /// as a failure it inflates the disagreement count, as a skip it shrinks the denominator
    /// silently, as incomplete it blames the machine for a property of the output.
    ///
    /// **Reported, never fatal on its own — but a run where everything is unalignable fails**,
    /// on the same reasoning as the all-skipped guard. A comparison that compared nothing has not
    /// passed; it has not run.
    Unalignable {
        reason: String,
    },
}

impl Outcome {
    pub fn failed(stage: Stage, detail: impl Into<String>) -> Outcome {
        Outcome::Failed {
            stage,
            detail: detail.into(),
        }
    }

    pub fn skipped(reason: impl Into<String>) -> Outcome {
        Outcome::Skipped {
            reason: reason.into(),
        }
    }

    pub fn incomplete(reason: impl Into<String>) -> Outcome {
        Outcome::Incomplete {
            reason: reason.into(),
        }
    }

    pub fn unalignable(reason: impl Into<String>) -> Outcome {
        Outcome::Unalignable {
            reason: reason.into(),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Passed => "passed",
            Outcome::Failed { .. } => "failed",
            Outcome::Skipped { .. } => "skipped",
            Outcome::Incomplete { .. } => "incomplete",
            Outcome::Unalignable { .. } => "unalignable",
        }
    }

    fn detail(&self) -> Option<&str> {
        match self {
            Outcome::Passed => None,
            Outcome::Failed { detail, .. } => Some(detail),
            Outcome::Skipped { reason }
            | Outcome::Incomplete { reason }
            | Outcome::Unalignable { reason } => Some(reason),
        }
    }

    fn stage(&self) -> Option<Stage> {
        match self {
            Outcome::Failed { stage, .. } => Some(*stage),
            _ => None,
        }
    }
}

/// The version of the result format itself, so a report read later can be interpreted.
pub const SCHEMA: u32 = 1;

/// A run in progress: its identity, its expected cases, and what has happened so far.
pub struct Run {
    profile: Profile,
    requirements: Vec<Requirement>,
    fixtures: Vec<(String, String)>,
    dictionaries: Vec<String>,
    engine: String,
    /// Where the artifact is written. `None` means `target/evaluation`.
    ///
    /// The seam the unwritable-output test needs. A run cannot be asked to prove it fails when it
    /// cannot save its report unless something can point it somewhere unsaveable, and making the
    /// real directory unwritable would break every other test in the process.
    artifact_dir: Option<PathBuf>,
    expected: Vec<String>,
    results: BTreeMap<String, Outcome>,
    /// Free-form per-case numbers, for a diff between runs to be about something.
    tallies: BTreeMap<String, BTreeMap<String, i64>>,
    /// Which engine (or which pair of them) produced this case.
    ///
    /// **Run-level `engine` is not enough for a comparison.** A run that puts two analyzers
    /// against each other has no single engine, and writing one of the two at the top of the
    /// artifact attributes every case to whichever was named — so a later reader cannot tell a
    /// Lindera result from an Ichiran one, or either from the comparison between them.
    case_engines: BTreeMap<String, String>,
    /// The spans a case produced, when the point of the case is where the boundaries fell.
    ///
    /// `tallies` cannot carry these: it is `i64`, so a boundary disagreement reduces to a count
    /// of how many boundaries differed, and the artifact no longer says *which*. That is exactly
    /// the detail a review queue is built from.
    spans: BTreeMap<String, Vec<Span>>,
}

/// One span a case produced, as the engine that produced it saw it.
///
/// Offsets are byte offsets into the case's own text, which is what both analyzers report and
/// what makes two engines' output comparable without a normalisation step that could itself be
/// the thing under test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Which engine or pipeline produced it, e.g. `lindera-raw`, `ichiran-display`.
    pub engine: String,
    pub start: usize,
    pub end: usize,
    pub surface: String,
    /// Empty where the engine offered none, which is a different thing from an empty reading.
    pub reading: String,
    /// Whether a dictionary entry was found. **Availability, never correctness.**
    pub looked_up: bool,
}

/// What a finished run amounts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Every expected case was accounted for, none failed or was left incomplete, and every
    /// required resource was present.
    Passed,
    /// Passed, but an optional comparison could not be made. Named, not implied.
    Incomplete {
        reasons: Vec<String>,
    },
    Failed {
        reasons: Vec<String>,
    },
    /// A person has not looked yet, and no amount of software can change that.
    ///
    /// **Distinct from `Passed` and from `Failed`.** Nothing is wrong; nothing is confirmed
    /// either. Collapsing it into either one is how a checklist nobody ran comes to be reported
    /// as a checklist that passed.
    Outstanding {
        unobserved: Vec<String>,
    },
}

impl Run {
    /// Begin a run. The manifest is declared **before** anything is evaluated, which is what makes
    /// "a case never reached" detectable at all.
    pub fn new(profile: Profile, expected: impl IntoIterator<Item = String>) -> Run {
        Run {
            profile,
            requirements: Vec::new(),
            fixtures: Vec::new(),
            dictionaries: Vec::new(),
            engine: "unspecified".into(),
            artifact_dir: None,
            expected: expected.into_iter().collect(),
            results: BTreeMap::new(),
            tallies: BTreeMap::new(),
            case_engines: BTreeMap::new(),
            spans: BTreeMap::new(),
        }
    }

    pub fn needs(mut self, requirement: Requirement) -> Run {
        self.requirements.push(requirement);
        self
    }

    /// A fixture and the digest it was verified against.
    pub fn fixture(mut self, name: impl Into<String>, sha256: impl Into<String>) -> Run {
        self.fixtures.push((name.into(), sha256.into()));
        self
    }

    /// A dictionary the run actually opened. Not the ones it hoped for.
    pub fn dictionary(mut self, described: impl Into<String>) -> Run {
        self.dictionaries.push(described.into());
        self
    }

    pub fn engine(mut self, engine: impl Into<String>) -> Run {
        self.engine = engine.into();
        self
    }

    /// Write the artifact somewhere other than `target/evaluation`.
    pub fn artifact_dir(mut self, dir: impl Into<PathBuf>) -> Run {
        self.artifact_dir = Some(dir.into());
        self
    }

    /// Record what happened to one case. Recording the same case twice is a bug in the harness and
    /// is refused: two answers for one case means the denominator is already wrong.
    pub fn record(&mut self, id: impl Into<String>, outcome: Outcome) {
        let id = id.into();
        assert!(
            self.expected.contains(&id),
            "case {id:?} was recorded but is not in the expected manifest; a run that scores \
             cases it never declared cannot say whether it scored them all"
        );
        assert!(
            self.results.insert(id.clone(), outcome).is_none(),
            "case {id:?} was recorded twice"
        );
    }

    /// Numbers belonging to one case, written into the artifact beside its outcome.
    ///
    /// This is what makes a run diff say *which* sentences moved rather than that the total did.
    pub fn tally(
        &mut self,
        id: impl Into<String>,
        counts: impl IntoIterator<Item = (String, i64)>,
    ) {
        self.tallies.entry(id.into()).or_default().extend(counts);
    }

    /// Which engine produced this case, overriding the run-level engine for it.
    ///
    /// For a comparison, name both sides — `"lindera-display vs ichiran-display"` — because the
    /// case belongs to the comparison and not to either engine alone.
    pub fn attribute(&mut self, id: impl Into<String>, engine: impl Into<String>) {
        self.case_engines.insert(id.into(), engine.into());
    }

    /// The spans this case produced, from one or more engines.
    ///
    /// Appends rather than replaces, so each engine's spans can be recorded as it runs without
    /// the caller having to accumulate them.
    pub fn spans(&mut self, id: impl Into<String>, spans: impl IntoIterator<Item = Span>) {
        self.spans.entry(id.into()).or_default().extend(spans);
    }

    /// Reconcile and report, but judge only **reachability** — not whether the cases passed.
    ///
    /// For a gate whose pass/fail decision is an aggregate floor rather than a per-case verdict.
    /// The floor answers "did accuracy hold"; this answers the question the floor cannot: was
    /// every declared case actually reached, was every required resource present, and was the
    /// selection non-empty. A rate over a corpus that quietly shrank is the failure this catches,
    /// and it is the one an aggregate floor is blind to by construction.
    ///
    /// A case recorded as *failed* is therefore reported and not treated as a run failure here.
    /// A case that produced **no result** still is: that is a crash or an early return, and it
    /// changes the denominator the floor is computed over.
    pub fn accounted_for(self) -> Verdict {
        self.conclude(false)
    }

    /// Reconcile, report, and say plainly what the run amounts to.
    pub fn finish(self) -> Verdict {
        self.conclude(true)
    }

    fn conclude(self, case_failures_fail_the_run: bool) -> Verdict {
        /// How many per-case failures reach the log before the rest are counted instead.
        const FAILURES_PRINTED: usize = 20;

        let mut failures = Vec::new();
        let mut incomplete = Vec::new();
        let mut reported_failures = 0usize;

        // An empty selection is a failure, never a pass. A gate that evaluates nothing and prints
        // a green line is the exact failure mode this project has already shipped twice.
        if self.expected.is_empty() {
            failures
                .push("no cases were selected; a run that evaluates nothing cannot pass".into());
        }

        // **Distinct from the above, because they are different bugs.** An empty selection is a
        // filter that matched nothing; an all-skipped run is a corpus that was reached and then
        // declined, one case at a time. Reporting them with one message hides which happened.
        let skipped = self
            .results
            .values()
            .filter(|outcome| matches!(outcome, Outcome::Skipped { .. }))
            .count();
        if !self.expected.is_empty() && skipped == self.expected.len() {
            failures.push(format!(
                "every one of the {skipped} selected cases was skipped; a run that declined its \
                 whole corpus has not passed, it has not run"
            ));
        }

        // **Third distinct shape, third distinct message.** Every case attempted, every case
        // producing output on both sides, and not one pair that could be put in correspondence:
        // a comparison that compared nothing. It reaches the same conclusion as the two above by
        // a different route, and naming which route it took is the difference between a
        // diagnosable report and a mystery.
        let unalignable = self
            .results
            .values()
            .filter(|outcome| matches!(outcome, Outcome::Unalignable { .. }))
            .count();
        if !self.expected.is_empty() && unalignable == self.expected.len() {
            failures.push(format!(
                "all {unalignable} selected cases were unalignable; the engines ran and answered                  but no pair could be put in correspondence, so the run compared nothing"
            ));
        }

        // The profile's demands, checked rather than taken on trust.
        let required = self
            .requirements
            .iter()
            .filter(|requirement| requirement.need == Need::Required)
            .count();
        let observations: Vec<&Requirement> = self
            .requirements
            .iter()
            .filter(|requirement| requirement.need == Need::Observation)
            .collect();
        match self.profile.demands() {
            Demand::RequiredResources if required == 0 => failures.push(format!(
                "the {} profile declared no required resource, so its name describes nothing",
                self.profile
            )),
            Demand::NoRequiredResources if required > 0 => failures.push(format!(
                "the {} profile declared {required} required resource(s); a profile that needs \
                 something installed is not hermetic",
                self.profile
            )),
            Demand::HumanObservation if observations.is_empty() => failures.push(format!(
                "the {} profile declared no observation rows, so there is nothing to be \
                 outstanding about",
                self.profile
            )),
            _ => {}
        }

        for requirement in &self.requirements {
            if requirement.present {
                continue;
            }
            let note = requirement
                .note
                .clone()
                .unwrap_or_else(|| "absent".to_string());
            match requirement.need {
                Need::Required => failures.push(format!(
                    "required resource {:?} is absent ({note}); this is an unsuccessful profile \
                     result, not a skip",
                    requirement.name
                )),
                Need::Optional => incomplete.push(format!(
                    "optional comparison {:?} was not made ({note})",
                    requirement.name
                )),
                // Handled by the outstanding verdict below, not as a failure: nothing is wrong.
                Need::Observation => {}
            }
        }

        // Every declared case must be accounted for. A case with no result is the shape a crash,
        // a timeout and an early return all take, and none of them may be silent.
        for id in &self.expected {
            match self.results.get(id) {
                None => failures.push(format!(
                    "case {id:?} was selected but produced no result — a crash, a timeout or a \
                     path that returned early. It is unaccounted for, which fails the run"
                )),
                Some(Outcome::Failed { stage, detail }) => {
                    let line = format!("case {id:?} failed at {stage}: {detail}");
                    if case_failures_fail_the_run {
                        failures.push(line);
                    } else {
                        // Reported to a bound, and left to the aggregate floor to judge. A gate
                        // whose per-case standard is "agreed with the treebank exactly" has
                        // hundreds of these — partial agreement is the norm at an F1 of 74 — and
                        // printing every one buries the summary under them. The artifact holds
                        // them all, which is what it is for.
                        reported_failures += 1;
                        if reported_failures <= FAILURES_PRINTED {
                            println!("  case failed    {line}");
                        }
                    }
                }
                Some(Outcome::Incomplete { reason }) => {
                    failures.push(format!("case {id:?} could not be completed: {reason}"))
                }
                // Reported to the log and carried in the artifact, and never counted as
                // agreement. It fails the run only when it is the whole run, above.
                Some(Outcome::Unalignable { reason }) => {
                    reported_failures += 1;
                    if reported_failures <= FAILURES_PRINTED {
                        println!("  unalignable    case {id:?}: {reason}");
                    }
                }
                Some(Outcome::Skipped { .. } | Outcome::Passed) => {}
            }
        }

        if reported_failures > FAILURES_PRINTED {
            println!(
                "  case failed    … and {} more, all of them in the artifact",
                reported_failures - FAILURES_PRINTED
            );
        }

        // Written before the verdict is decided, and **after** the case failures are collected,
        // so the artifact records the same failures the verdict is built from.
        let artifact = self.write_artifact(&failures, &incomplete);
        if let Err(reason) = &artifact {
            failures.push(format!(
                "the run could not save its report ({reason}); the measured result is above and \
                 the accuracy gate speaks for itself, but an evaluation that leaves no record has \
                 not produced one of its outputs"
            ));
        }
        self.print(&failures, &incomplete, artifact.as_deref().ok());

        if !failures.is_empty() {
            return Verdict::Failed { reasons: failures };
        }
        // **Checked after the failures and before the passes.** A desktop-acceptance run with a
        // genuine failure in it has failed; one with nothing wrong has still not been looked at.
        if self.profile == Profile::DesktopAcceptance {
            return Verdict::Outstanding {
                unobserved: observations
                    .iter()
                    .filter(|requirement| !requirement.present)
                    .map(|requirement| requirement.name.clone())
                    .collect(),
            };
        }
        if !incomplete.is_empty() {
            return Verdict::Incomplete {
                reasons: incomplete,
            };
        }
        Verdict::Passed
    }

    /// Write the run to a file, so its per-case outcomes outlive the scrollback.
    ///
    /// Under `target/evaluation/`, which is not committed: these are observations of one machine
    /// at one moment, and a repository of them would be a repository of noise. The path is printed
    /// so a run diff is a `diff` away.
    ///
    /// **A write failure fails the run.** This used to be excused — "a gate that held is a gate
    /// that held whether or not a scratch directory was writable" — and that was wrong. Persisting
    /// the result is part of what a run *is*, now that per-case outcomes are the thing a later run
    /// is compared against; a run nobody can compare against has not produced one of its outputs.
    ///
    /// It is also not hypothetical. The disk on this machine reached 100% twice during this work,
    /// failing a link and then an archive write, so the condition that makes a report most worth
    /// having is exactly the condition that was silently discarding it.
    ///
    /// **The accuracy result survives regardless.** `print` runs before the verdict is computed,
    /// and the aggregate gates (`hold_the_line`, the capture floor) assert on their own — so the
    /// measured numbers still reach the log and the accuracy assertion still speaks. What fails is
    /// the *evaluation*, and the reason says which of the two happened.
    ///
    /// The error is a string rather than an absence, because "could not save the report" and
    /// "could not save the report because the disk is full" are different things to be told.
    fn write_artifact(
        &self,
        failures: &[String],
        incomplete: &[String],
    ) -> Result<PathBuf, String> {
        let dir = match &self.artifact_dir {
            Some(dir) => dir.clone(),
            None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/evaluation"),
        };
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
        // Canonicalised so the printed path is one a person can paste, rather than one with a
        // `core/..` still in the middle of it — then stripped of Windows' verbatim `\?\` prefix,
        // which `canonicalize` adds and no shell wants back.
        let dir = std::fs::canonicalize(&dir)
            .ok()
            .map(|full| {
                let text = full.to_string_lossy().to_string();
                PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text).to_string())
            })
            .unwrap_or(dir);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or(0);
        // A counter as well as a timestamp and a pid: two tests in the same process finishing
        // inside the same second would otherwise write the same filename, and one would truncate
        // the other while it was being read. `write_atomically` carries the same counter for the
        // same reason.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = dir.join(format!(
            "{}-{stamp}-{}-{}.json",
            self.profile.slug(),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        let cases: Vec<serde_json::Value> = self
            .expected
            .iter()
            .map(|id| {
                let outcome = self.results.get(id);
                serde_json::json!({
                    "id": id,
                    "outcome": outcome.map(Outcome::label).unwrap_or("unaccounted"),
                    "stage": outcome.and_then(Outcome::stage).map(|stage| stage.to_string()),
                    "detail": outcome.and_then(Outcome::detail),
                    "engine": self.case_engines.get(id),
                    "tallies": self.tallies.get(id),
                    "spans": self.spans.get(id).map(|spans| spans.iter().map(|span| {
                        serde_json::json!({
                            "engine": span.engine,
                            "start": span.start,
                            "end": span.end,
                            "surface": span.surface,
                            "reading": span.reading,
                            "looked_up": span.looked_up,
                        })
                    }).collect::<Vec<_>>()),
                })
            })
            .collect();

        let document = serde_json::json!({
            "schema": SCHEMA,
            "profile": self.profile.to_string(),
            "verdict": if !failures.is_empty() {
                "failed"
            } else if self.profile == Profile::DesktopAcceptance {
                "outstanding"
            } else if !incomplete.is_empty() {
                "incomplete"
            } else {
                "passed"
            },
            "identity": {
                "executable": executable_digest(),
                "source_tree": source_digest(),
                "engine": self.engine,
                "dictionaries": self.dictionaries,
                "fixtures": self.fixtures.iter()
                    .map(|(name, digest)| serde_json::json!({ "name": name, "sha256": digest }))
                    .collect::<Vec<_>>(),
            },
            "requirements": self.requirements.iter().map(|requirement| serde_json::json!({
                "name": requirement.name,
                "need": requirement.label(),
                "present": requirement.present,
                "note": requirement.note,
            })).collect::<Vec<_>>(),
            "counts": {
                "selected": self.expected.len(),
                "reported": self.results.len(),
            },
            "failures": failures,
            "incomplete": incomplete,
            "cases": cases,
        });

        let body = serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("could not serialise the run: {error}"))?;
        std::fs::write(&path, body)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        remember_artifact(&path);
        Ok(path)
    }

    fn print(&self, failures: &[String], incomplete: &[String], artifact: Option<&Path>) {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for outcome in self.results.values() {
            *counts.entry(outcome.label()).or_default() += 1;
        }
        let unaccounted = self.expected.len().saturating_sub(self.results.len());

        println!("\n=== evaluation run (schema {SCHEMA}) ===");
        println!("  profile        {}", self.profile);
        println!("  engine         {}", self.engine);
        println!("  executable     {}", executable_digest());
        println!("  source tree    {}", source_digest());
        println!(
            "  dictionaries   {}",
            if self.dictionaries.is_empty() {
                "none opened".to_string()
            } else {
                self.dictionaries.join(", ")
            }
        );
        for (name, digest) in &self.fixtures {
            println!("  fixture        {name} {digest}");
        }
        for requirement in &self.requirements {
            println!(
                "  requires       {} ({}) — {}",
                requirement.name,
                requirement.label(),
                if requirement.present {
                    "present".to_string()
                } else {
                    requirement
                        .note
                        .clone()
                        .unwrap_or_else(|| "absent".to_string())
                }
            );
        }
        println!(
            "  cases          {} selected · {} passed · {} failed · {} skipped · {} incomplete · \
             {unaccounted} unaccounted for",
            self.expected.len(),
            counts.get("passed").copied().unwrap_or(0),
            counts.get("failed").copied().unwrap_or(0),
            counts.get("skipped").copied().unwrap_or(0),
            counts.get("incomplete").copied().unwrap_or(0),
        );
        for reason in incomplete {
            println!("  INCOMPLETE     {reason}");
        }
        for reason in failures {
            println!("  FAILED         {reason}");
        }
        if self.profile == Profile::DesktopAcceptance {
            println!(
                "  OUTSTANDING    desktop acceptance is observed by a person, not produced here; \
                 this run cannot pass"
            );
        }
        match artifact {
            Some(path) => println!("  artifact       {}", path.display()),
            // The reason is in `failures`, which prints above this.
            None => println!("  artifact       NOT WRITTEN — see the failure above"),
        }
    }
}

thread_local! {
    /// The artifact this thread wrote most recently.
    ///
    /// **Per thread, because `cargo test` runs tests in parallel.** Finding "the newest file in
    /// the directory" is a race the moment two runs finish in the same instant, and it produced
    /// exactly that: two tests read each other's half-written file. A test asks for the artifact
    /// *it* produced.
    static LAST_ARTIFACT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

fn remember_artifact(path: &Path) {
    LAST_ARTIFACT.with(|last| *last.borrow_mut() = Some(path.to_path_buf()));
}

/// The artifact the most recent run on this thread wrote.
pub fn last_artifact() -> Option<PathBuf> {
    LAST_ARTIFACT.with(|last| last.borrow().clone())
}

impl Verdict {
    /// Fail the test unless the run passed, or passed with a named optional comparison missing.
    pub fn expect_success(self) {
        match self {
            Verdict::Passed => {}
            Verdict::Incomplete { reasons } => {
                println!("  run passed with incomplete comparisons: {reasons:?}");
            }
            Verdict::Failed { reasons } => {
                panic!("the evaluation run failed:\n  - {}", reasons.join("\n  - "))
            }
            Verdict::Outstanding { unobserved } => panic!(
                "this profile is observed by a person and cannot be asserted here; still \
                 unobserved:\n  - {}",
                unobserved.join("\n  - ")
            ),
        }
    }

    pub fn failed(&self) -> bool {
        matches!(self, Verdict::Failed { .. })
    }

    pub fn passed(&self) -> bool {
        matches!(self, Verdict::Passed)
    }
}

/// A digest of the running test executable.
///
/// **This is the artefact that produced the result**, so it covers every source file, dependency
/// version and embedded pack by construction. The digest it replaced hashed a source tree at test
/// time, which cannot prove the binary matches what it hashed — a stale build and an edited tree
/// produce the same reading.
///
/// Cached, because the file is tens of megabytes and a run asks more than once. Comparable within
/// a machine and profile, not across them: a debug and a release build of identical source are
/// different executables and should read as different.
pub fn executable_digest() -> String {
    static DIGEST: OnceLock<String> = OnceLock::new();
    DIGEST
        .get_or_init(|| {
            let Ok(path) = std::env::current_exe() else {
                return "unknown (no current_exe)".to_string();
            };
            let Ok(bytes) = std::fs::read(&path) else {
                return "unknown (unreadable)".to_string();
            };
            let digest = sakatsu_core::hashing::hex(Sha256::digest(&bytes));
            format!(
                "{}… ({:.1} MB, {})",
                &digest[..16],
                bytes.len() as f64 / (1024.0 * 1024.0),
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                }
            )
        })
        .clone()
}

/// A content digest of the source tree, **beside** the executable digest and not instead of it.
///
/// Human-readable and greppable where the executable digest is not: it says which files were
/// covered and how many. It describes the tree, which may not be what was compiled — that is what
/// [`executable_digest`] is for, and why both are recorded.
///
/// Covers the crates whose behaviour a run measures, the tests that decide its verdict, the
/// grammar packs compiled into it, and `Cargo.lock`. The first version covered only `core/src` and
/// `src-tauri/src`, so a changed gate or a bumped dependency left it identical.
pub fn source_digest() -> String {
    static DIGEST: OnceLock<String> = OnceLock::new();
    DIGEST
        .get_or_init(|| {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let mut files = Vec::new();
            for dir in [
                root.join("src"),
                root.join("tests"),
                root.join("../src-tauri/src"),
            ] {
                collect_sources(&dir, &mut files);
            }
            for manifest in [
                root.join("Cargo.toml"),
                root.join("../Cargo.lock"),
                root.join("../src-tauri/Cargo.toml"),
            ] {
                if manifest.is_file() {
                    files.push(manifest);
                }
            }
            files.sort();
            let mut hasher = Sha256::new();
            for file in &files {
                hasher.update(file.to_string_lossy().as_bytes());
                hasher.update(std::fs::read(file).unwrap_or_default());
            }
            let digest = sakatsu_core::hashing::hex(hasher.finalize());
            format!(
                "{}… ({} files, tree not binary)",
                &digest[..16],
                files.len()
            )
        })
        .clone()
}

/// Rust sources and the JSON data compiled in beside them.
fn collect_sources(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, into);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "rs" || extension == "json")
        {
            into.push(path);
        }
    }
}

/// The SHA-256 of a file, for a fixture identity line.
pub fn digest_of(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(sakatsu_core::hashing::hex(Sha256::digest(bytes)))
}
