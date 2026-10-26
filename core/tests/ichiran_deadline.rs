//! **One budget per passage, across every blocking step.**
//!
//! A passage used to be able to wait sixty seconds on the persistent session and then sixty more
//! on the one-shot path it fell back to, and the session lock, the session start and the write of
//! the passage were not bounded at all. Now one deadline is fixed when the passage starts, and
//! every step gets only what remains:
//!
//! - a sidecar that never answers costs the budget **once**, and no one-shot is started after it;
//! - a sidecar that never reads its stdin cannot hold the write past the budget;
//! - a passage waiting behind another for the session lock gives up at its own budget;
//! - a spent budget starts **nothing**.
//!
//! Hermetic: the sidecar is `fake-ichiran`, built from `src/bin/fake-ichiran.rs`, told to misbehave.
//! The persistent session is process-wide state, so these run one at a time.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sakatsu_core::analyzer::ichiran::server;
use sakatsu_core::analyzer::{AnalyzeError, Analyzer, IchiranAnalyzer};

/// The persistent session is a process-wide singleton; two of these at once would share it.
static SERIAL: Mutex<()> = Mutex::new(());

const BUDGET: Duration = Duration::from_millis(1500);
/// Scheduling slack on a loaded machine. Far less than a second budget, which is what a stacked
/// wait would add.
const SLACK: Duration = Duration::from_millis(1200);

fn fake(mode: &str) -> IchiranAnalyzer {
    IchiranAnalyzer::with_command(
        env!("CARGO_BIN_EXE_fake-ichiran"),
        [format!("--mode={mode}"), "-f".to_string()],
    )
    .with_budget(BUDGET)
}

/// A fresh marker file, and the environment variable the fake reads it from.
fn marker(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sakatsu-fake-ichiran-{name}-{}.log",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    std::env::set_var("FAKE_ICHIRAN_MARK", &path);
    path
}

fn starts(path: &PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Each test starts and ends with no session and persistence off.
struct Fresh<'a>(#[allow(dead_code)] std::sync::MutexGuard<'a, ()>);

fn fresh(persistent: bool) -> Fresh<'static> {
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    server::shutdown();
    server::set_persistent(persistent);
    Fresh(guard)
}

impl Drop for Fresh<'_> {
    fn drop(&mut self) {
        server::shutdown();
        server::set_persistent(false);
        std::env::remove_var("FAKE_ICHIRAN_MARK");
    }
}

fn assert_timed_out(result: Result<sakatsu_core::model::Analysis, AnalyzeError>) {
    match result {
        Err(AnalyzeError::TimedOut(_)) => {}
        other => panic!("expected a timeout, got {other:?}"),
    }
}

#[test]
fn a_silent_one_shot_costs_the_budget_once() {
    let _fresh = fresh(false);
    let started = Instant::now();
    assert_timed_out(fake("silent").analyze("水を飲む"));
    let took = started.elapsed();
    assert!(took >= BUDGET, "gave up early: {took:?}");
    assert!(took < BUDGET + SLACK, "waited past the budget: {took:?}");
}

/// **The stacking this exists to prevent.** A session that never answers used to be followed by a
/// one-shot with a full budget of its own.
#[test]
fn a_silent_session_costs_the_budget_once_and_starts_no_one_shot() {
    let _fresh = fresh(true);
    let mark = marker("silent-session");
    let started = Instant::now();
    assert_timed_out(fake("silent").analyze("水を飲む"));
    let took = started.elapsed();
    assert!(took < BUDGET + SLACK, "a second wait was stacked: {took:?}");
    assert_eq!(server::last_route(), server::Route::TimedOut);
    assert_eq!(
        starts(&mark),
        ["silent session"],
        "no one-shot after a spent budget"
    );
    assert!(
        server::persistent(),
        "a timeout does not switch persistence off"
    );
}

/// A child that never reads stdin blocks the write once the pipe is full. The write is bounded.
#[test]
fn a_session_that_never_reads_cannot_hold_the_write() {
    let _fresh = fresh(true);
    // Several megabytes: far more than any pipe buffers, so the write itself must block.
    let passage = "水".repeat(2_000_000);
    let started = Instant::now();
    assert_timed_out(fake("deaf").analyze(&passage));
    let took = started.elapsed();
    assert!(took < BUDGET + SLACK, "the write was not bounded: {took:?}");
}

/// A passage waiting behind another for the session lock gives up at its **own** budget, not at
/// the end of the other passage's.
#[test]
fn waiting_for_the_session_lock_is_inside_the_budget() {
    let _fresh = fresh(true);
    let holder = std::thread::spawn(|| {
        // Holds the session for its whole, longer budget.
        let _ = fake("silent")
            .with_budget(Duration::from_secs(4))
            .analyze("水を飲む");
    });
    std::thread::sleep(Duration::from_millis(400));
    let waiter = fake("answer").with_budget(Duration::from_millis(600));
    let started = Instant::now();
    assert_timed_out(waiter.analyze("水を飲む"));
    let took = started.elapsed();
    assert!(
        took < Duration::from_millis(600) + SLACK,
        "waited on the lock past its own budget: {took:?}"
    );
    holder.join().expect("the holder finishes");
}

/// A spent budget starts nothing — on either route.
#[test]
fn a_spent_budget_starts_nothing() {
    for persistent in [false, true] {
        let _fresh = fresh(persistent);
        let mark = marker("spent");
        let started = Instant::now();
        assert_timed_out(
            fake("answer")
                .with_budget(Duration::ZERO)
                .analyze("水を飲む"),
        );
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(
            starts(&mark).is_empty(),
            "persistent={persistent}: something was started with no budget: {:?}",
            starts(&mark)
        );
    }
}

/// **"Starts nothing" observed without a race.** A marker the child writes can lose to the kill, so
/// this points at a program that does not exist: any attempt to start it is an `EngineUnavailable`,
/// and only an analyzer that attempted nothing can answer `TimedOut`.
#[test]
fn a_spent_budget_does_not_even_attempt_a_start() {
    for persistent in [false, true] {
        let _fresh = fresh(persistent);
        let nowhere =
            IchiranAnalyzer::with_command("sakatsu-no-such-sidecar-3f9a", ["-f".to_string()])
                .with_budget(Duration::ZERO);
        match nowhere.analyze("水を飲む") {
            Err(AnalyzeError::TimedOut(_)) => {}
            other => panic!("persistent={persistent}: a start was attempted: {other:?}"),
        }
    }
}

/// **A line the driver did not write for this frame ends the session.** The fake prints one stray
/// line — a backtrace frame, which is what SBCL wrote after an uncaught condition — before its
/// first answer. Kept, that session would hand every later passage the line queued for the one
/// before it; a paired measurement lost 72 passages that way. Dropped, this passage is answered by
/// the one-shot route and the next one by a **new** session, in step again.
#[test]
fn a_stray_line_ends_the_session_and_the_next_passage_gets_a_clean_one() {
    let _fresh = fresh(true);
    let marked = marker("stray");
    let first = fake("stray")
        .analyze("水を飲む")
        .expect("answered despite the stray line");
    assert_eq!(
        surfaces(&first),
        ["水", "を", "飲む"],
        "the first passage's own answer"
    );
    // A different passage, so an answer queued for the first could not pass for this one's.
    let second = fake("stray")
        .analyze("気持ち")
        .expect("answered by a clean session");
    assert_eq!(
        surfaces(&second),
        ["気持ち"],
        "the second passage's own answer"
    );
    assert_eq!(server::last_route(), server::Route::Session);
    let started = starts(&marked);
    assert_eq!(
        started
            .iter()
            .filter(|line| line.ends_with("session"))
            .count(),
        2,
        "the out-of-step session was replaced, not kept: {started:?}"
    );
    assert!(
        started.iter().any(|line| line.ends_with("one-shot")),
        "the passage that met the stray line took the one-shot route: {started:?}"
    );
}

/// **A line that looks like an answer and is not one ends the session too.** `[broken` begins
/// with `[`, so a check on the first character accepts it, and the session would be handed back
/// with the real answer still queued — for the next passage to read as its own. The line has to
/// be *read* as an answer, while the session is held, before the session is kept.
#[test]
fn an_unreadable_line_that_looks_like_an_answer_ends_the_session() {
    let _fresh = fresh(true);
    let marked = marker("broken");
    let first = fake("broken")
        .analyze("水を飲む")
        .expect("answered by the one-shot route");
    assert_eq!(
        surfaces(&first),
        ["水", "を", "飲む"],
        "the first passage's own answer"
    );
    let second = fake("broken")
        .analyze("気持ち")
        .expect("answered by a clean session");
    assert_eq!(
        surfaces(&second),
        ["気持ち"],
        "the second passage got its own answer, not the one queued behind the broken line"
    );
    assert_eq!(server::last_route(), server::Route::Session);
    let started = starts(&marked);
    assert_eq!(
        started
            .iter()
            .filter(|line| line.ends_with("session"))
            .count(),
        2,
        "the untrustworthy session was replaced, not kept: {started:?}"
    );
}

fn surfaces(analysis: &sakatsu_core::model::Analysis) -> Vec<&str> {
    analysis
        .tokens
        .iter()
        .map(|token| token.surface.as_str())
        .collect()
}

/// The controls: a healthy session answers through the session route, and malformed output is
/// malformed output — not a timeout.
#[test]
fn a_healthy_session_answers_and_malformed_output_is_reported_as_such() {
    {
        let _fresh = fresh(true);
        let analysis = fake("answer").analyze("水を飲む").expect("answers");
        assert!(!analysis.tokens.is_empty());
        assert_eq!(server::last_route(), server::Route::Session);
    }
    {
        let _fresh = fresh(true);
        match fake("malformed").analyze("水を飲む") {
            Err(AnalyzeError::BadOutput(_)) => {}
            other => panic!("expected malformed output, got {other:?}"),
        }
    }
    {
        let _fresh = fresh(false);
        let analysis = fake("answer")
            .analyze("水を飲む")
            .expect("the one-shot answers");
        assert!(!analysis.tokens.is_empty());
    }
}
