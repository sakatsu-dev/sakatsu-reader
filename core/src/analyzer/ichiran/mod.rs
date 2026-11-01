//! The Ichiran adapter: [`Analyzer`] backed by the `ichiran-cli` sidecar.
//!
//! Ichiran gives the richest analysis available (`project notes` §2) at the cost of
//! needing WSL/Docker and PostgreSQL. It sits behind the [`Analyzer`] trait so an
//! embeddable engine can replace it later without the grammar layer noticing —
//! retiring this module is the point of the trait, not an afterthought.
//!
//! The interpretation of Ichiran's output — every subtle rule — is in
//! [`convert`], which is pure and tested against committed fixtures. This file
//! only deals with running the process.

pub mod convert;
pub mod raw;
pub mod server;

use std::ffi::OsString;
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::analyzer::assist::{Advice, Assistance, AssistanceMode, Stage};
use crate::analyzer::{AnalyzeError, Analyzer};
use crate::model::Analysis;

/// Overrides the command used to reach `ichiran-cli`, as a whitespace-separated
/// command line (e.g. `wsl -e ichiran-cli`). `-f`, `--` and the text are appended — see
/// [`args_for`] for why the marker is not optional.
pub const COMMAND_ENV: &str = "SAKATSU_ICHIRAN_CMD";

/// Routes analyses through one long-lived sidecar instead of a process per passage — about 3×,
/// measured. See [`server`], which explains what that costs as well as what it buys.
///
/// The development lever for the measurement harnesses, where the difference is an hour against
/// twenty minutes. In the app the same switch is a setting, and both end at
/// [`server::set_persistent`].
pub const SERVER_ENV: &str = "SAKATSU_ICHIRAN_SERVER";

/// **One budget per passage, across every route.** A cold WSL/database start can take seconds; it
/// must not be able to hold app startup or an analysis worker forever. The guided wrapper gets a
/// full minute to recover its services — once, whichever of the session and the one-shot paths
/// spends it, rather than a minute on each.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Whether to keep the sidecar's console window off the screen.
///
/// Default on, and this is the one setting that does not default to preserving previous
/// behaviour. `wsl.exe` is a console program, so a windowed application spawning it gets
/// a console — once per analysis, which is once per capture and once per Enter. Since
/// `.output()` pipes both stdout and stderr, that window is *empty*: it is not a log
/// anyone can read, it is a black rectangle over the text being studied.
///
/// Turning it off is for when WSL needs to say something. It has nowhere else to prompt,
/// so first-run setup or a distribution that wants installing is invisible while this is
/// on. The failure still surfaces — [`AnalyzeError::EngineUnavailable`] carries the
/// reason — but the prompt itself does not.
static QUIET: AtomicBool = AtomicBool::new(true);

/// Set from the settings, read at spawn time.
///
/// A static because the analyzer is built once at startup and held as a trait object, so
/// there is no instance for a settings change to reach — the same reason the hold
/// threshold lives in a static in the binding layer.
pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

/// Keep the console window hidden, on the platform that has one.
#[cfg(windows)]
fn hide_console(command: &mut Command) {
    if QUIET.load(Ordering::Relaxed) {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NO_WINDOW`. Spelled out rather than pulled from the `windows` crate:
        /// core would take on that dependency for one integer.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

/// Runs `ichiran-cli -f` and normalizes what comes back.
#[derive(Debug, Clone)]
pub struct IchiranAnalyzer {
    program: OsString,
    /// Arguments before the text — the `-f` (full/JSON) flag, plus whatever is
    /// needed to reach the binary at all.
    prefix_args: Vec<OsString>,
    /// How long one passage may take across every route. [`TIMEOUT`] unless a caller says
    /// otherwise — which the deadline tests do, and nothing in the app does.
    budget: Duration,
}

impl IchiranAnalyzer {
    /// The platform default, overridable with [`COMMAND_ENV`].
    ///
    /// On Windows, Ichiran lives inside WSL (it needs PostgreSQL), so the default
    /// goes through `wsl -e`. Elsewhere `ichiran-cli` is expected on `PATH`.
    /// Phase 6 will bundle the sidecar and set this explicitly.
    pub fn new() -> Self {
        // Read here rather than in `server`, so the variable is consulted once at construction and
        // a later settings change is not silently overridden by the environment it started in.
        // Absent means "leave the setting alone", not "off".
        if let Some(value) = std::env::var_os(SERVER_ENV) {
            let value = value.to_string_lossy().trim().to_ascii_lowercase();
            server::set_persistent(matches!(value.as_str(), "1" | "true" | "on" | "yes"));
        }

        if let Some(command) = std::env::var_os(COMMAND_ENV) {
            let command = command.to_string_lossy().into_owned();
            let mut parts = command.split_whitespace().map(OsString::from);
            if let Some(program) = parts.next() {
                let mut prefix_args: Vec<OsString> = parts.collect();
                prefix_args.push(OsString::from("-f"));
                return IchiranAnalyzer {
                    program,
                    prefix_args,
                    budget: TIMEOUT,
                };
            }
        }

        if cfg!(windows) {
            Self::with_command("wsl", ["-e", "ichiran-cli", "-f"])
        } else {
            Self::with_command("ichiran-cli", ["-f"])
        }
    }

    /// Build an analyzer around an explicit command, whose `prefix_args` must include `-f`.
    ///
    /// The passage is appended by [`args_for`], behind a `--`, so no caller has to remember
    /// that a passage beginning with a hyphen would otherwise be read as options.
    pub fn with_command<P, A, S>(program: P, prefix_args: A) -> Self
    where
        P: Into<OsString>,
        A: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        IchiranAnalyzer {
            program: program.into(),
            prefix_args: prefix_args.into_iter().map(Into::into).collect(),
            budget: TIMEOUT,
        }
    }

    /// The same analyzer with a different per-passage budget.
    pub fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }

    /// One process, one passage — the path this analyzer has always taken.
    ///
    /// Kept whole and separate so [`server`] sits *in front of* it rather than replacing it. When
    /// the live session declines — switched off, failed to start, just died — this runs unchanged,
    /// which is what makes the optimisation unable to fail an analysis.
    fn one_shot(&self, text: &str, deadline: Instant) -> Result<String, AnalyzeError> {
        self.one_shot_cancellable(text, deadline, None)?
            .ok_or_else(|| {
                AnalyzeError::EngineUnavailable("the analyzer request was cancelled".into())
            })
    }

    /// Check readiness in a private one-shot process. Cancellation never touches the shared
    /// sidecar, which may be serving a passage. `false` means cancelled, not unavailable.
    pub fn check_available(&self, cancelled: &AtomicBool) -> Result<bool, AnalyzeError> {
        self.check_available_by(Instant::now() + self.budget, cancelled)
    }

    fn check_available_by(
        &self,
        deadline: Instant,
        cancelled: &AtomicBool,
    ) -> Result<bool, AnalyzeError> {
        let Some(stdout) = self.one_shot_cancellable("\u{6c34}", deadline, Some(cancelled))? else {
            return Ok(false);
        };
        let segments = raw::parse(&stdout)
            .map_err(|error| AnalyzeError::BadOutput(format!("unexpected JSON: {error}")))?;
        to_analysis_in(
            "\u{6c34}",
            &segments,
            TiePolicy::SHIPPED,
            &Assistance::new(AssistanceMode::Legacy),
        )?;
        Ok(!cancelled.load(Ordering::Acquire))
    }

    /// Warm the process used by passages, keeping it private until its route is published.
    /// A sidecar without session support retains the normal one-shot readiness path.
    pub fn prepare_for_startup(
        &self,
        cancelled: &AtomicBool,
    ) -> Result<Option<PreloadedIchiran>, AnalyzeError> {
        let deadline = Instant::now() + self.budget;
        if server::persistent() {
            match server::prepare(&self.program, &self.prefix_args, deadline, cancelled) {
                Ok(Some(session)) => {
                    if let Some(answer) = session.answer() {
                        let segments = raw::parse(answer).map_err(|error| {
                            AnalyzeError::BadOutput(format!("unexpected JSON: {error}"))
                        })?;
                        to_analysis_in(
                            "\u{6c34}",
                            &segments,
                            TiePolicy::SHIPPED,
                            &Assistance::new(AssistanceMode::Legacy),
                        )?;
                    }
                    return Ok(Some(PreloadedIchiran {
                        session: Some(session),
                    }));
                }
                Ok(None) => return Ok(None),
                Err(error) => crate::log_warn!("ichiran", "session preload declined: {error}"),
            }
        }
        self.check_available_by(deadline, cancelled)
            .map(|ready| ready.then_some(PreloadedIchiran { session: None }))
    }

    fn one_shot_cancellable(
        &self,
        text: &str,
        deadline: Instant,
        cancelled: Option<&AtomicBool>,
    ) -> Result<Option<String>, AnalyzeError> {
        // **Only what remains.** Starting a process with no budget left would spend time the
        // caller no longer has, and could only end in the same timeout later.
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(AnalyzeError::TimedOut(format!(
                "the {} s budget was spent before {} could be started",
                self.budget.as_secs(),
                self.program.to_string_lossy()
            )));
        }
        let mut command = Command::new(&self.program);
        command.args(args_for(&self.prefix_args, text));
        hide_console(&mut command);

        let output = match output_with_cancellation(&mut command, remaining, cancelled) {
            Err(RunError::Cancelled) => return Ok(None),
            result => result,
        }
        .map_err(|error| match error {
            RunError::Io(error) => AnalyzeError::EngineUnavailable(format!(
                "could not run {}: {error}",
                self.program.to_string_lossy()
            )),
            RunError::Cancelled => {
                AnalyzeError::EngineUnavailable("the availability check was cancelled".into())
            }
            RunError::TimedOut => AnalyzeError::TimedOut(format!(
                "{} did not answer within its {} s budget and was stopped",
                self.program.to_string_lossy(),
                self.budget.as_secs()
            )),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AnalyzeError::EngineUnavailable(format!(
                "{} exited with {}: {}",
                self.program.to_string_lossy(),
                output.status,
                first_line(&stderr)
            )));
        }

        String::from_utf8(output.stdout)
            .map(Some)
            .map_err(|error| AnalyzeError::BadOutput(format!("output was not UTF-8: {error}")))
    }
}

impl IchiranAnalyzer {
    /// **Diagnostic only:** up to `limit` segmentations of `text`, as `ichiran-cli -f -l <limit>`
    /// prints them. The reader never calls this.
    ///
    /// It exists to answer one question about a wrong reading: was the right one **among the
    /// candidates Ichiran returned**, or not observed within this limit? It cannot establish that a
    /// candidate is *absent* from Ichiran's inventory — five returned analyses are five, not all of
    /// them — so callers must not report "not observed" as "absent".
    ///
    /// Always the one-shot route, never the live session: the session's driver asks for one
    /// segmentation, and asking it for more would change what every later passage in the session
    /// was measured against.
    pub fn segmentations(&self, text: &str, limit: usize) -> Result<String, AnalyzeError> {
        let text = crate::analyzer::lines::normalise_breaks(text);
        let text = text.trim();
        if text.is_empty() {
            return Err(AnalyzeError::EmptyInput);
        }
        let mut prefix = self.prefix_args.clone();
        prefix.push(OsString::from("-l"));
        prefix.push(OsString::from(limit.max(1).to_string()));
        let limited = IchiranAnalyzer {
            program: self.program.clone(),
            prefix_args: prefix,
            budget: self.budget,
        };
        limited.one_shot(text, Instant::now() + self.budget)
    }
}

/// Startup owns this preparation until the matching route wins publication.
pub struct PreloadedIchiran {
    session: Option<server::PreparedSession>,
}

impl PreloadedIchiran {
    pub fn commit(self) {
        if let Some(session) = self.session {
            session.commit();
        }
    }
}

impl Default for IchiranAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Everything after the program name, for a given passage.
///
/// **The `--` is the whole reason this is a function.** `ichiran-cli` parses its arguments with
/// getopt, so a passage handed over bare is read as *options* the moment it begins with an ASCII
/// hyphen: recognised text starting `-l` came back as
/// `fatal: option "-l" needs an argument!` and the capture failed with "analyzer unavailable",
/// which points at WSL and the sidecar rather than at the one character responsible.
///
/// That is not a rare shape for OCR output. A dash opening a line of dialogue, a bullet, a
/// horizontal rule, an em dash flattened to ASCII, or — as it happened here — Latin text read off
/// the screen. Note the *katakana* prolonged mark ー (U+30FC) is a different character and was
/// never affected, which is what made this look arbitrary.
///
/// `--` is getopt's end-of-options marker: everything after it is positional however it starts.
/// Verified against the real `ichiran-cli`, not assumed.
fn args_for(prefix: &[OsString], text: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = prefix.to_vec();
    args.push(OsString::from("--"));
    args.push(OsString::from(text));
    args
}

impl Analyzer for IchiranAnalyzer {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        // **One body.** The trace is this analysis with the sidecar's output kept beside it, so a
        // diagnostic can never be describing a conversion the reader was not served.
        self.analyze_traced(text).map(|traced| traced.analysis)
    }

    fn analyze_traced(&self, text: &str) -> Result<crate::analyzer::Traced, AnalyzeError> {
        // Outside a request, the advice is asked as it always was: of the shared parser, each
        // time, with nothing kept.
        self.analyze_in(text, &Assistance::new(AssistanceMode::Legacy))
    }

    fn analyze_in(
        &self,
        text: &str,
        assistance: &Assistance<'_>,
    ) -> Result<crate::analyzer::Traced, AnalyzeError> {
        // Before the sidecar is woken for nothing. See `Assistance::refuse_if_advising`.
        assistance.refuse_if_advising()?;
        // Folded before either analyzer sees it, so CRLF and LF produce one canonical source and
        // identical sentence ranges. This also preserves the fix for the former IPADIC path,
        // where a bare carriage return could become a token. See `lines::normalise_breaks`.
        let text = crate::analyzer::lines::normalise_breaks(text);
        let text = text.trim();
        if text.is_empty() {
            return Err(AnalyzeError::EmptyInput);
        }

        // The sidecar spawn, which is what `analyzer::remembering` exists to avoid paying
        // twice. Characters, not the text: the length is a fact about the work, the passage
        // is the learner's.
        let _timing = crate::log::Timing::start(
            "ichiran",
            format!("analysing {} characters", text.chars().count()),
        );

        // **The live session is tried first and is allowed to decline.** It answers `None` when
        // it is switched off, cannot start, or has just died — never an error — so a passage that
        // would have analysed still analyses. See [`server`].
        // **One deadline for the passage**, fixed here and shared by both routes. The session
        // gets it first; whatever it leaves is all the one-shot path may use, and a session that
        // spent it ends the passage rather than starting a second full wait.
        let deadline = Instant::now() + self.budget;
        let stdout = match server::analyze(&self.program, &self.prefix_args, text, deadline) {
            server::Answer::Session(json) => json,
            server::Answer::Declined => self.one_shot(text, deadline)?,
            server::Answer::TimedOut(reason) => return Err(AnalyzeError::TimedOut(reason)),
        };

        let segments = raw::parse(&stdout)
            .map_err(|error| AnalyzeError::BadOutput(format!("unexpected JSON: {error}")))?;

        Ok(crate::analyzer::Traced {
            analysis: to_analysis_in(text, &segments, TiePolicy::SHIPPED, assistance)?,
            raw: Some(stdout),
        })
    }

    fn engine(&self) -> &str {
        "ichiran"
    }

    /// Wake the session this analyzer will need, and pay nothing else.
    ///
    /// **The cheapest command that exists, on purpose.** What costs the seconds is Windows
    /// bringing the WSL distro session back up, not anything Ichiran does — measured, after a
    /// 100 s idle: `wsl -e true` 4.6 s, then `ichiran-cli --help` 0.40 s, then a real analysis
    /// 1.0 s. The wake-up lands on whichever command is first, so this makes it a command
    /// nobody is waiting for.
    ///
    /// Running a real analysis here would work too and would also warm PostgreSQL, at the cost
    /// of a query nobody asked for. `true` was enough in the measurement, so `true` is what
    /// this runs.
    ///
    /// **Only one at a time.** Opening the overlay and starting a capture both call this, and
    /// on a cold session the first is still running when the second arrives; a second process
    /// would queue behind the first for no gain.
    fn warm(&self) {
        // Nothing to wake where the analyzer is a local binary — there is no VM between us
        // and it, and spawning a process to prove that would be the only cost incurred.
        if self.program != *"wsl" {
            return;
        }
        if WARMING.swap(true, Ordering::AcqRel) {
            return;
        }
        let program = self.program.clone();
        std::thread::spawn(move || {
            let mut command = Command::new(&program);
            command.args([OsString::from("-e"), OsString::from("true")]);
            hide_console(&mut command);
            let started = Instant::now();
            let woken = output_with_timeout(&mut command, WARM_TIMEOUT).is_ok();
            WARMING.store(false, Ordering::Release);
            if woken {
                crate::log_info!(
                    "ichiran",
                    "woke the sidecar's session in {:.2}s",
                    started.elapsed().as_secs_f64()
                );
            }
        });
    }
}

/// One wake-up in flight at a time. See [`Analyzer::warm`] on `IchiranAnalyzer`.
static WARMING: AtomicBool = AtomicBool::new(false);

/// Long enough for a cold session, short enough that a stuck one is eventually retried.
///
/// The wake-up itself was measured at 4.6 s; this is that with room, and well under the
/// analysis timeout — a warm-up that outlived the read it was warming for would be holding
/// the guard above against the next one.
const WARM_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
enum RunError {
    Io(std::io::Error),
    TimedOut,
    Cancelled,
}

/// Capture both pipes without letting either fill and deadlock the child, while retaining a
/// handle that can be killed on timeout. `Command::output` has the first property but offers no
/// timeout; moving it to another thread has the second problem in reverse because the handle is
/// then out of reach.
fn output_with_timeout(command: &mut Command, timeout: Duration) -> Result<Output, RunError> {
    output_with_cancellation(command, timeout, None)
}

fn output_with_cancellation(
    command: &mut Command,
    timeout: Duration,
    cancelled: Option<&AtomicBool>,
) -> Result<Output, RunError> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(RunError::Cancelled);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(RunError::Io)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RunError::Io(std::io::Error::other("child stdout was not piped")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| RunError::Io(std::io::Error::other("child stderr was not piped")))?;
    let stdout = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut stdout = stdout;
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut stderr = stderr;
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });

    let started = Instant::now();
    let status = loop {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            // Stop and reap this invocation before its caller releases the lifecycle gate.
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout.join();
            let _ = stderr.join();
            return Err(RunError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout.join();
                let _ = stderr.join();
                return Err(RunError::TimedOut);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout.join();
                let _ = stderr.join();
                return Err(RunError::Io(error));
            }
        }
    };

    let stdout = stdout
        .join()
        .map_err(|_| RunError::Io(std::io::Error::other("stdout reader panicked")))?
        .map_err(RunError::Io)?;
    let stderr = stderr
        .join()
        .map_err(|_| RunError::Io(std::io::Error::other("stderr reader panicked")))?
        .map_err(RunError::Io)?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}

/// Everything that happens to parsed Ichiran output before a reader sees it.
///
/// Its own function because the fixture tests need the *shipped* pipeline, and the only other
/// way to give them one is to have them repeat these three lines — which they did, and which
/// the note they carried admitted was a no-op on every committed fixture and would therefore
/// rot unnoticed. Two steps use the embedded parser and neither belongs in `convert`, which
/// stays pure so it can be driven from fixtures with a fake second opinion.
pub fn to_analysis(text: &str, segments: &[raw::Segment]) -> Analysis {
    to_analysis_under(text, segments, TiePolicy::SHIPPED)
}

/// Which second opinion breaks an equal-score tie.
///
/// **The contextual rule is provisional**: it is shipped, and it awaits acceptance
/// in the development evaluation. It gained 3 KWDLC ties with no
/// KWDLC regression and 7 heteronym ties against **3 new errors** — 寒気 read さむけ where the
/// dictionary files かんき, and 金 read かね twice where it files きん — which
/// `ichiran_fixtures.rs` keeps as regression controls. The bare-word rule stays callable so the
/// two can be compared over the same raw candidates, and so the decision can be reversed in one
/// line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TiePolicy {
    /// The word in its sentence first, then the bare word, then Ichiran's order.
    Contextual,
    /// The bare word alone, then Ichiran's order — the rule before slice 4.
    BareWord,
}

impl TiePolicy {
    /// What the reader is served.
    pub const SHIPPED: TiePolicy = TiePolicy::Contextual;
}

/// [`to_analysis`] under a chosen tie-break, for comparing the two over the same raw output.
pub fn to_analysis_under(text: &str, segments: &[raw::Segment], policy: TiePolicy) -> Analysis {
    converted(
        text,
        segments,
        policy,
        &Assistance::new(AssistanceMode::Legacy),
    )
}

/// [`to_analysis_under`], with the advice asked through one request's [`Assistance`].
///
/// **Refused from inside a question** — see `Assistance::refuse_if_advising` — which is the only
/// way it fails: a conversion started by the advisor would ask the advisor, which would start
/// another.
pub fn to_analysis_in(
    text: &str,
    segments: &[raw::Segment],
    policy: TiePolicy,
    assistance: &Assistance<'_>,
) -> Result<Analysis, AnalyzeError> {
    assistance.refuse_if_advising()?;
    Ok(converted(text, segments, policy, assistance))
}

fn converted(
    text: &str,
    segments: &[raw::Segment],
    policy: TiePolicy,
    assistance: &Assistance<'_>,
) -> Analysis {
    // Read once per conversion, and only if a tie asks: most passages have none. **The failure
    // is settled here too, for this conversion only**: the request remembers only answers, so a
    // later conversion, or the fallback, asks again.
    let sentence: std::cell::OnceCell<Option<std::rc::Rc<Analysis>>> = std::cell::OnceCell::new();
    let resolve = |tie: &convert::Tie| {
        let in_context = tie
            .span
            .filter(|_| policy == TiePolicy::Contextual)
            .and_then(|span| {
                let read = sentence.get_or_init(|| match assistance.ask(Stage::Passage, text) {
                    Advice::Answered(analysis) => Some(analysis),
                    Advice::Unavailable | Advice::Failed(_) => None,
                });
                let found = read
                    .as_ref()
                    .and_then(|analysis| reading_in_context(analysis, span, tie.surface));
                if read.is_some() && found.is_none() {
                    assistance.unused(Stage::Passage);
                }
                found
            });
        let mut preferences: Vec<String> = in_context.into_iter().collect();
        if let Some(bare) = second_opinion(tie.surface, assistance) {
            if !preferences.contains(&bare) {
                preferences.push(bare);
            }
        }
        preferences
    };
    let mut analysis = convert::to_analysis_resolving(text, segments, &resolve);
    // The break classification is a second pass over the grounded tokens, and it uses the
    // embedded parser rather than this one — see `lines::mark_with_shared_probe` for why.
    crate::analyzer::lines::mark_in(&mut analysis, assistance, None);
    analysis
}

/// The embedded parser's reading of a tied word **in its sentence**: its token at exactly this
/// span, spelled the same way.
///
/// **Asked first, because the question is about this occurrence.** 方 ties at 16 between ほう and
/// かた; alone it reads ほう, and in この方は… it reads かた. The bare question answered every
/// such tie the same way whatever the sentence said — three KWDLC losses were exactly that
/// (see `core/tests/tie_audit.rs`). Where the parser segments
/// the sentence differently and has no token here, the bare question below still answers.
fn reading_in_context(analysis: &Analysis, span: (usize, usize), surface: &str) -> Option<String> {
    analysis
        .tokens
        .iter()
        .find(|token| (token.start, token.end) == (Some(span.0), Some(span.1)))
        .filter(|token| token.surface == surface && !token.reading.is_empty())
        .map(|token| crate::kana::to_hiragana(&token.reading))
}

/// The embedded parser's reading for one surface **alone**, for the ties Ichiran declines to break
/// where the reading in context is unavailable.
///
/// Consulted **only** where Ichiran scored two candidates identically — see
/// [`convert::to_analysis_with`] for why that boundary is the whole design. 晴々 comes back as
/// せいせい and はればれ at 65 apiece; UniDic says はればれ, which is the reading a reader
/// wants in 晴々として…良い気分.
///
/// Answers nothing unless it read the surface as **one token spelled exactly the same way**. A
/// surface it splits has been analysed as a different thing, and a reading assembled from the
/// pieces would be an answer to a question nobody asked.
///
/// **Asked once per tie, as before; remembered per request only where the request remembers.**
/// A failure on one tie is never kept, so the next tie asks again.
fn second_opinion(surface: &str, assistance: &Assistance<'_>) -> Option<String> {
    let advice = assistance.ask(Stage::Word, surface);
    let analysis = advice.answered()?;
    let answer = match analysis.tokens.as_slice() {
        [token] if token.surface == surface && !token.reading.is_empty() => {
            Some(crate::kana::to_hiragana(&token.reading))
        }
        _ => None,
    };
    if answer.is_none() {
        assistance.unused(Stage::Word);
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A passage that starts with a hyphen must not be read as a flag.
    ///
    /// The bug this pins: OCR handed `ichiran-cli` text beginning `-l`, getopt took it for an
    /// option, and the capture came back "analyzer unavailable — wsl exited with exit code 1",
    /// naming WSL for a failure that was one character of the passage. It is checked here rather
    /// than end-to-end because the sidecar needs WSL, PostgreSQL and about 4.7 GB, so no test that
    /// spawns it can run in the ordinary suite.
    #[test]
    fn a_passage_cannot_be_mistaken_for_options() {
        let prefix: Vec<OsString> = ["-e", "ichiran-cli", "-f"]
            .iter()
            .map(OsString::from)
            .collect();
        let args = args_for(&prefix, "-l テスト");

        assert_eq!(
            args,
            ["-e", "ichiran-cli", "-f", "--", "-l テスト"]
                .iter()
                .map(OsString::from)
                .collect::<Vec<_>>()
        );

        // The marker's position is what makes it work, not its presence: after the text it would
        // be an argument, and anywhere earlier it would end the *prefix's* options instead.
        let marker = args.iter().position(|arg| arg == "--").expect("the marker");
        assert_eq!(
            marker,
            args.len() - 2,
            "-- must sit immediately before the passage"
        );
    }

    /// And an ordinary passage gains nothing else.
    #[test]
    fn the_marker_is_the_only_thing_added() {
        let prefix: Vec<OsString> = ["-f"].iter().map(OsString::from).collect();
        assert_eq!(
            args_for(&prefix, "水を飲む"),
            ["-f", "--", "水を飲む"]
                .iter()
                .map(OsString::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn empty_input_is_rejected_before_a_process_is_spawned() {
        // `with_command` points at a program that does not exist, so reaching the
        // spawn would fail differently — proving the check happens first.
        let analyzer = IchiranAnalyzer::with_command("definitely-not-a-program", ["-f"]);
        assert_eq!(analyzer.analyze("   "), Err(AnalyzeError::EmptyInput));
        assert_eq!(analyzer.engine(), "ichiran");
    }

    #[test]
    fn a_missing_sidecar_is_reported_as_unavailable_not_a_panic() {
        let analyzer = IchiranAnalyzer::with_command("definitely-not-a-program", ["-f"]);
        match analyzer.analyze("水を飲む") {
            Err(AnalyzeError::EngineUnavailable(message)) => {
                assert!(
                    message.contains("definitely-not-a-program"),
                    "message should name the program: {message}"
                );
            }
            other => panic!("expected EngineUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn a_cancelled_availability_check_does_not_spawn_or_report_unavailability() {
        let cancelled = AtomicBool::new(true);
        let analyzer = IchiranAnalyzer::with_command("definitely-not-a-program", ["-f"]);
        assert_eq!(analyzer.check_available(&cancelled), Ok(false));
        cancelled.store(false, Ordering::Release);
        assert!(matches!(
            analyzer.check_available(&cancelled),
            Err(AnalyzeError::EngineUnavailable(_))
        ));
    }

    #[test]
    fn a_stuck_process_is_killed_at_the_deadline() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 5"]);
            command
        } else {
            // Exercise the owned child, as the PowerShell branch does. A shell can leave
            // sleep alive with inherited pipes after the shell is killed, testing a
            // descendant-process policy rather than this child's deadline.
            let mut command = Command::new("sleep");
            command.arg("5");
            command
        };
        let started = Instant::now();
        assert!(matches!(
            output_with_timeout(&mut command, Duration::from_millis(50)),
            Err(RunError::TimedOut)
        ));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the timeout did not bound the child"
        );
    }

    /// Hits the real sidecar. Ignored by default so the suite stays hermetic (CI
    /// has no Ichiran); run it after changing this file:
    ///
    /// ```text
    /// cargo test -p sakatsu-core -- --ignored
    /// ```
    #[test]
    #[ignore = "requires a working ichiran-cli"]
    fn the_real_sidecar_analyses_a_sentence() {
        let analysis = IchiranAnalyzer::new()
            .analyze("水を飲む")
            .expect("ichiran-cli should be reachable");
        assert_eq!(analysis.text, "水を飲む");
        let surfaces: Vec<&str> = analysis
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect();
        assert_eq!(surfaces, ["水", "を", "飲む"]);
    }
}
