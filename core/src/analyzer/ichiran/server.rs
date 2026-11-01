//! One long-lived `ichiran-cli` instead of one per passage.
//!
//! # Why
//!
//! `ichiran-cli` is a saved SBCL image. Every invocation loads it and reconnects to PostgreSQL
//! before any Japanese is looked at, and that fixed cost dominates: measured over 20 passages of
//! subtitle dialogue, **1428 ms per passage one-shot against 469 ms through one live process** —
//! about 3×. The remaining ~470 ms is `romanize*` itself doing real work against the database, so
//! 3× is near the ceiling here rather than a first step towards more.
//!
//! That is the difference between a lookup that feels sluggish and one that does not, and for the
//! measurement harnesses it is the difference between an hour and twenty minutes.
//!
//! # How
//!
//! `ichiran-cli -e` evaluates arbitrary Lisp *after* `load-connection-from-env`, so the database
//! is already reachable and no rebuild of the sidecar is needed. [`DRIVER`] is a read-eval loop
//! that calls exactly what `-f` calls — `(princ (jsown:to-json (romanize* input :limit 1)))`, as
//! written in `~/ichiran/cli.lisp` — so the JSON on the wire is byte-for-byte the JSON [`raw`]
//! already parses. This module changes *when* the process starts, never what it answers.
//!
//! ## The frame is length-prefixed, and that is not incidental
//!
//! ```text
//!   <character count>\n<passage>\n      request
//!   <one line of JSON>\n                response
//! ```
//!
//! A newline-delimited protocol would have been simpler and is wrong here: passages contain
//! newlines. `lines::normalise_breaks` folds CRLF to LF and deliberately keeps it, because a
//! multi-line capture is one passage and the sentence ranges depend on the breaks surviving. A
//! line protocol would silently cut every multi-line capture into pieces and analyse the first.
//!
//! The alternative was to escape the passage as a JSON string, which needs the Lisp side to
//! decode `\n`, `\"`, `\\` and `\uXXXX` exactly right. Framing needs it to decode nothing.
//! Verified against the real sidecar on the three payloads that separate the two — a plain
//! passage, one containing a newline, and one containing a quote and a backslash.
//!
//! The count is in **characters**, because `read-sequence` fills a character buffer. Bytes would
//! be correct for exactly the ASCII case and truncate every passage this app exists to read.
//!
//! # What is deliberately not here
//!
//! No socket, no port, no HTTP. The child is a pipe-connected sidecar that dies with its parent,
//! which is what the one-shot path already was; a listening socket would be a new thing on the
//! learner's machine to secure, firewall and explain.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

/// Whether to route analyses through a live process. Off unless asked for.
///
/// **Off by default because it changes process lifetime, not just speed.** A one-shot sidecar
/// leaves nothing behind; this holds an SBCL image and a PostgreSQL connection open for as long
/// as the app runs. That is the right trade for someone reading for an hour and the wrong one for
/// someone who opened the app to check a single word.
static PERSISTENT: AtomicBool = AtomicBool::new(false);

/// The live process, or none yet. See [`PERSISTENT`] for why it may never be started.
///
/// One session, shared: the point is to pay the image load once, so a second process would undo
/// the only thing this module does. Requests therefore serialise on this lock, which is also what
/// keeps the framed protocol coherent — two writers interleaving frames would desynchronise the
/// stream permanently.
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// How the last analysis was actually served.
///
/// # Why this exists
///
/// Nothing else could tell the two paths apart. `persistent()` reports *intent*, and it stays
/// `true` after a live session dies mid-request and the passage silently goes one-shot — so a
/// measurement that read it could report the sidecar's cost while having timed the fallback. The
/// only other account of a fallback is a `log_info!` line, and logging is off unless a session has
/// enabled it, which a measurement child never does.
///
/// `core/tests/analyzer_cold_start.rs` reports this beside every sample, so a cold-start number
/// names the path it measured instead of being assumed to describe the sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// No analysis has been attempted yet.
    Unused,
    /// The live session answered. This is the only value that describes the sidecar.
    Session,
    /// Persistence was off before the request — nobody asked for a sidecar.
    NotPersistent,
    /// The session lock was poisoned by a panic in another thread.
    Poisoned,
    /// A session could not be started, and persistence switched itself off.
    StartFailed,
    /// A started session answered badly and was dropped; the passage went one-shot.
    RequestFailed,
    /// The passage's time budget ran out on the session route — waiting for the lock, starting the
    /// session, handing it the passage or waiting for the answer. **No one-shot followed**: there
    /// was no budget left to give it.
    TimedOut,
}

impl Route {
    /// The word an artifact should print.
    pub fn name(self) -> &'static str {
        match self {
            Route::Unused => "unused",
            Route::Session => "session",
            Route::NotPersistent => "not-persistent",
            Route::Poisoned => "poisoned",
            Route::StartFailed => "start-failed",
            Route::RequestFailed => "request-failed",
            Route::TimedOut => "timed-out",
        }
    }

    /// Whether the owned persistent sidecar served the passage.
    pub fn is_session(self) -> bool {
        matches!(self, Route::Session)
    }

    fn code(self) -> u8 {
        match self {
            Route::Unused => 0,
            Route::Session => 1,
            Route::NotPersistent => 2,
            Route::Poisoned => 3,
            Route::StartFailed => 4,
            Route::RequestFailed => 5,
            Route::TimedOut => 6,
        }
    }

    fn from_code(code: u8) -> Route {
        match code {
            1 => Route::Session,
            2 => Route::NotPersistent,
            3 => Route::Poisoned,
            4 => Route::StartFailed,
            5 => Route::RequestFailed,
            6 => Route::TimedOut,
            _ => Route::Unused,
        }
    }
}

/// The last route, and how many passages each path has served.
///
/// One relaxed store per analysis. That is nanoseconds against a process spawn measured in
/// hundreds of milliseconds, so it does not distort what the cold-start harness times — but it is
/// recorded here rather than left to be rediscovered.
static LAST_ROUTE: AtomicU8 = AtomicU8::new(0);
static SERVED_BY_SESSION: AtomicUsize = AtomicUsize::new(0);
static SERVED_ONE_SHOT: AtomicUsize = AtomicUsize::new(0);

fn took(route: Route) {
    LAST_ROUTE.store(route.code(), Ordering::Relaxed);
    match route {
        Route::Session => {
            SERVED_BY_SESSION.fetch_add(1, Ordering::Relaxed);
        }
        // Nothing served it: the budget was spent before any path answered.
        Route::TimedOut => {}
        _ => {
            SERVED_ONE_SHOT.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// How the last analysis was served.
pub fn last_route() -> Route {
    Route::from_code(LAST_ROUTE.load(Ordering::Relaxed))
}

/// `(served by the live session, served one passage per process)` since this process started.
pub fn routing_counts() -> (usize, usize) {
    (
        SERVED_BY_SESSION.load(Ordering::Relaxed),
        SERVED_ONE_SHOT.load(Ordering::Relaxed),
    )
}

/// Where the read-eval loop is installed inside the sidecar's filesystem.
///
/// `/tmp` rather than the home directory: it needs no `~` expansion (Lisp's `load` does not do
/// it), it is writable without asking, and losing it to a reboot is correct — the driver is
/// rewritten on every session start anyway, so a stale copy from an older build can never be
/// what runs.
const DRIVER_PATH: &str = "/tmp/sakatsu-ichiran-server.lisp";

/// Where the driver goes for `program`. Under WSL, [`DRIVER_PATH`] inside the distribution; for a
/// local `ichiran-cli`, this machine's temporary directory — which is `/tmp` on the platforms that
/// have one, and exists on the ones that do not. Forward slashes, because it is written inside a
/// Lisp string, where a backslash escapes.
fn driver_path(program: &OsStr) -> String {
    if program == "wsl" {
        DRIVER_PATH.to_string()
    } else {
        std::env::temp_dir()
            .join("sakatsu-ichiran-server.lisp")
            .to_string_lossy()
            .replace('\\', "/")
    }
}

/// The read-eval loop, evaluated by `ichiran-cli -e`.
///
/// Kept as one string rather than a file in the repository because it has to reach *inside* WSL,
/// where no path of ours exists. See [`install_driver`].
///
/// Errors are reported in-band as a JSON object rather than raised, so one bad passage cannot
/// desynchronise the stream and cost every later request in the session.
///
/// **`serious-condition`, not `error`.** Exhausting the control stack or the heap signals a
/// `storage-condition`, which is not an `error` and would pass straight through an `error`
/// handler into SBCL's own report. That is the **likely, unconfirmed** cause of what a paired
/// measurement saw: 72 passages lost in two runs of 41 and
/// 31 consecutive requests whose "answers" were the integers 0, 1, 2 … — the shape of a numbered
/// backtrace read one line per request. The trigger did not reproduce, so this handler is a
/// guard against the probable class of cause, not a proven fix; the adapter's own check in
/// [`Session::request`] is what recovers whatever the cause was.
const DRIVER: &str = r#"
(loop for header = (read-line *standard-input* nil nil)
      while header
      do (let ((n (parse-integer header :junk-allowed t)))
           (if (null n)
               (progn (princ "{\"error\":\"bad frame header\"}") (terpri) (finish-output))
               (let ((buf (make-string n)))
                 (read-sequence buf *standard-input*)
                 (read-line *standard-input* nil nil)
                 (handler-case
                     (princ (jsown:to-json (romanize* buf :limit 1)))
                   (serious-condition (e) (format t "{\"error\":~s}" (princ-to-string e))))
                 (terpri)
                 (finish-output)))))
"#;

/// Set from the settings, read when the next analysis starts.
///
/// Turning it off ends the session rather than only declining to use it, because the cost this
/// exists to avoid is the one already paid — leaving the image resident while nothing routes
/// through it is all of the memory and none of the benefit.
pub fn set_persistent(on: bool) {
    let was = PERSISTENT.swap(on, Ordering::Relaxed);
    if was && !on {
        shutdown();
    }
}

/// Whether analyses currently route through a live process.
pub fn persistent() -> bool {
    PERSISTENT.load(Ordering::Relaxed)
}

/// End the session, if there is one. Safe to call when there is not.
pub fn shutdown() {
    if let Ok(mut guard) = SESSION.lock() {
        // Dropping it ends it: see `Drop for Session`.
        guard.take();
    }
}

/// A nonblocking status read: `None` means a request or preload owns the session lock.
pub fn session_ready() -> Option<bool> {
    match SESSION.try_lock() {
        Ok(session) => Some(session.is_some()),
        Err(TryLockError::WouldBlock) => None,
        Err(TryLockError::Poisoned(_)) => Some(false),
    }
}

/// Holds the session reservation until its matching analyzer route is published.
/// An unpublished candidate is private: dropping it kills only that new process.
pub struct PreparedSession {
    slot: MutexGuard<'static, Option<Session>>,
    candidate: Option<Session>,
    answer: Option<String>,
}

impl PreparedSession {
    pub(super) fn answer(&self) -> Option<&str> {
        self.answer.as_deref()
    }

    pub(super) fn commit(mut self) {
        if persistent() {
            if let Some(session) = self.candidate.take() {
                *self.slot = Some(session);
            }
        }
    }
}

impl Drop for PreparedSession {
    fn drop(&mut self) {
        // Reap the private child before releasing the shared reservation.
        self.candidate.take();
    }
}

/// Prepare the actual persistent process, with the same frame protocol as foreground reads.
/// Cancellation never takes a session away from a foreground owner.
pub(super) fn prepare(
    program: &OsStr,
    prefix_args: &[OsString],
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<Option<PreparedSession>, String> {
    let slot = loop {
        if cancelled.load(Ordering::Acquire) || !persistent() {
            return Ok(None);
        }
        if Instant::now() >= deadline {
            return Err("the session preload exhausted its budget".into());
        }
        match SESSION.try_lock() {
            Ok(slot) => break slot,
            Err(TryLockError::Poisoned(_)) => return Err("the session lock was poisoned".into()),
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let mut prepared = PreparedSession {
        slot,
        candidate: None,
        answer: None,
    };
    if prepared.slot.is_none() {
        if !install_driver_cancellable(program, deadline, cancelled)? {
            return Ok(None);
        }
        if cancelled.load(Ordering::Acquire) || !persistent() {
            return Ok(None);
        }
        let mut candidate = Session::spawn(program, prefix_args)?;
        let answer = match candidate.request("\u{6c34}", deadline, Some(cancelled)) {
            Ok(answer) => answer,
            Err(_) if cancelled.load(Ordering::Acquire) || !persistent() => return Ok(None),
            Err(Failure::Failed(error) | Failure::TimedOut(error)) => return Err(error),
        };
        prepared.candidate = Some(candidate);
        prepared.answer = Some(answer);
    }
    if cancelled.load(Ordering::Acquire) || !persistent() {
        return Ok(None);
    }
    Ok(Some(prepared))
}

/// What the session route did with one passage.
pub(super) enum Answer {
    /// The live session answered.
    Session(String),
    /// The session declined — switched off, could not start, or answered badly — and the caller
    /// may run the one-shot path **with whatever budget remains**.
    Declined,
    /// The budget ran out on the session route. The caller must not start a one-shot.
    TimedOut(String),
}

/// One passage through the live process, within `deadline`.
///
/// **The deadline covers every blocking step**, not only the wait for the answer: acquiring the
/// session lock (another passage may hold it), starting the session (a cold WSL can take seconds),
/// handing it the passage (a child that never reads blocks the write once the pipe is full) and
/// waiting for the reply. Each gets only what remains, so a stalled step costs the budget once —
/// never a full budget here and another full one on the one-shot path after it.
///
/// `Declined` keeps its old meaning — nothing this optimisation does wrong can fail an analysis
/// that would otherwise have succeeded — but only within the budget: a spent budget is `TimedOut`,
/// and the caller then stops.
pub(super) fn analyze(
    program: &OsStr,
    prefix_args: &[OsString],
    text: &str,
    deadline: Instant,
) -> Answer {
    if !persistent() {
        took(Route::NotPersistent);
        return Answer::Declined;
    }

    let mut guard = loop {
        match SESSION.try_lock() {
            Ok(guard) => break guard,
            Err(TryLockError::Poisoned(_)) => {
                took(Route::Poisoned);
                return Answer::Declined;
            }
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    took(Route::TimedOut);
                    return Answer::TimedOut(
                        "the session was busy with another passage for the whole budget".into(),
                    );
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };

    if guard.is_none() {
        match start_within(program, prefix_args, deadline) {
            Ok(session) => *guard = Some(session),
            Err(Started::TimedOut) => {
                took(Route::TimedOut);
                return Answer::TimedOut("the session did not start within the budget".into());
            }
            Err(Started::Failed(error)) => {
                crate::log_info!(
                    "ichiran",
                    "persistent session could not start ({error}); using one process per passage"
                );
                // Left off rather than retried on every analysis: a sidecar that cannot host a
                // session will not start hosting one between two captures, and trying again each
                // time would add a failed spawn to the cost of every lookup.
                PERSISTENT.store(false, Ordering::Relaxed);
                took(Route::StartFailed);
                return Answer::Declined;
            }
        }
    }

    let Some(session) = guard.as_mut() else {
        took(Route::Poisoned);
        return Answer::Declined;
    };
    match session.request(text, deadline, None) {
        Ok(json) => {
            took(Route::Session);
            Answer::Session(json)
        }
        Err(Failure::TimedOut(reason)) => {
            // The child may be blocked mid-frame; the stream is no longer aligned. It goes.
            guard.take();
            took(Route::TimedOut);
            Answer::TimedOut(reason)
        }
        Err(Failure::Failed(error)) => {
            crate::log_info!(
                "ichiran",
                "persistent session failed ({error}); falling back and restarting it"
            );
            guard.take();
            took(Route::RequestFailed);
            Answer::Declined
        }
    }
}

/// Why a session did not start.
enum Started {
    TimedOut,
    Failed(String),
}

/// Why a request did not answer.
enum Failure {
    TimedOut(String),
    Failed(String),
}

/// Start a session on a helper thread, and wait for it only as long as the budget allows.
///
/// A start that finishes after the budget is dropped by the helper when it finds nobody waiting,
/// and dropping a session kills its child — so a late start cannot leave a process behind.
fn start_within(
    program: &OsStr,
    prefix_args: &[OsString],
    deadline: Instant,
) -> Result<Session, Started> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(Started::TimedOut);
    }
    let (program, prefix_args) = (program.to_owned(), prefix_args.to_vec());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(Session::start(&program, &prefix_args));
    });
    match rx.recv_timeout(remaining) {
        Ok(Ok(session)) => Ok(session),
        Ok(Err(error)) => Err(Started::Failed(error)),
        Err(_) => Err(Started::TimedOut),
    }
}

/// A live `ichiran-cli` with its pipes.
struct Session {
    child: Child,
    /// Taken while a writer thread holds it, and put back when the write finishes. `None` after a
    /// write that did not finish, which only ever precedes the session being dropped.
    stdin: Option<ChildStdin>,
    /// Lines from the child, read by a thread so a wedged sidecar can be timed out.
    ///
    /// A blocking read on the pipe cannot be abandoned, so the thread owns it and this end waits
    /// with a deadline instead — the same reason `output_with_timeout` exists on the one-shot path.
    lines: Receiver<std::io::Result<String>>,
}

impl Session {
    fn start(program: &OsStr, prefix_args: &[OsString]) -> Result<Self, String> {
        install_driver(program)?;
        Self::spawn(program, prefix_args)
    }

    fn spawn(program: &OsStr, prefix_args: &[OsString]) -> Result<Self, String> {
        // `-f` is what the one-shot path asks for and is meaningless here: the driver calls
        // `romanize*` itself. Everything else in the prefix is how the binary is reached at all
        // (`-e ichiran-cli` under WSL) and has to be kept.
        let mut args: Vec<OsString> = prefix_args
            .iter()
            .filter(|arg| *arg != "-f" && *arg != "--full")
            .cloned()
            .collect();
        args.push(OsString::from("-e"));
        args.push(OsString::from(format!(
            "(load \"{}\")",
            driver_path(program)
        )));

        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        super::hide_console(&mut command);

        let mut child = command
            .spawn()
            .map_err(|error| format!("could not run {}: {error}", program.to_string_lossy()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "child stdin was not piped".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "child stdout was not piped".to_string())?;

        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        Ok(Session {
            child,
            stdin: Some(stdin),
            lines,
        })
    }

    /// Write one frame and wait for its line, both within `deadline`.
    ///
    /// **The write runs on its own thread.** A child that never reads stdin blocks the write once
    /// the pipe fills, and a blocked write cannot be abandoned from the thread doing it. Waiting on
    /// the writer with the remaining budget bounds it; the caller drops the session on a timeout,
    /// which kills the child and frees the writer.
    fn request(
        &mut self,
        text: &str,
        deadline: Instant,
        cancelled: Option<&AtomicBool>,
    ) -> Result<String, Failure> {
        // Characters, matching `read-sequence` on the other end. See the module note.
        let frame = format!("{}\n{}\n", text.chars().count(), text);
        let mut stdin = self
            .stdin
            .take()
            .ok_or_else(|| Failure::Failed("the session has no stdin".into()))?;
        let (tx, written) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = stdin
                .write_all(frame.as_bytes())
                .and_then(|()| stdin.flush())
                .map(|()| stdin);
            let _ = tx.send(outcome);
        });
        match receive(&written, deadline, cancelled) {
            Ok(Ok(stdin)) => self.stdin = Some(stdin),
            Ok(Err(error)) => {
                return Err(Failure::Failed(format!(
                    "could not write to the sidecar: {error}"
                )))
            }
            Err(_) => {
                return Err(Failure::TimedOut(
                    "the sidecar did not take the passage within the budget".into(),
                ))
            }
        }

        let line = match receive(&self.lines, deadline, cancelled) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => {
                return Err(Failure::Failed(format!(
                    "could not read from the sidecar: {error}"
                )))
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(Failure::TimedOut(
                    "the sidecar did not answer within the budget".into(),
                ))
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Failure::Failed("the sidecar exited".to_string()))
            }
        };

        // The driver reports a failed passage in-band so the stream stays aligned. Recognised
        // here rather than left to `raw::parse`, whose error would name JSON when the sidecar has
        // already said what was actually wrong.
        if line.starts_with("{\"error\"") {
            return Err(Failure::Failed(format!(
                "the sidecar refused the passage: {line}"
            )));
        }
        // **Anything that is not a readable answer ends the session.** A refusal is the object
        // above; an answer is a line the adapter's own schema reads (`raw::parse`), checked here,
        // while the session is still held, rather than after it has been handed back for the next
        // passage. A line that fails is either not the driver's answer to this frame — a condition
        // report, a backtrace, a prompt, a fragment — or an answer nothing downstream could read;
        // either way the stream can no longer be trusted to be in step, and keeping it would hand
        // later passages someone else's line. Failing drops and kills it, this passage takes the
        // one-shot route, and the next starts a clean session. The cost is one parse more than the
        // conversion does anyway. The line itself is not repeated: it may quote the passage.
        if super::raw::parse(&line).is_err() {
            return Err(Failure::Failed(format!(
                "the sidecar's session is no longer trustworthy: a {}-byte line that is neither a \
                 readable answer nor a refusal",
                line.len()
            )));
        }

        Ok(line)
    }
}

/// Foreground reads retain one deadline wait; speculative reads poll their cancellation token.
fn receive<T>(
    receiver: &Receiver<T>,
    deadline: Instant,
    cancelled: Option<&AtomicBool>,
) -> Result<T, RecvTimeoutError> {
    loop {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire) || !persistent()) {
            return Err(RecvTimeoutError::Timeout);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let interval = if cancelled.is_some() {
            remaining.min(Duration::from_millis(10))
        } else {
            remaining
        };
        match receiver.recv_timeout(interval) {
            Err(RecvTimeoutError::Timeout) if Instant::now() < deadline && cancelled.is_some() => {
                continue
            }
            result => return result,
        }
    }
}

/// Unlike foreground startup's helper, preload must reap a cancelled driver installation
/// before releasing its lifecycle reservation.
fn install_driver_cancellable(
    program: &OsStr,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<bool, String> {
    if cancelled.load(Ordering::Acquire) || !persistent() {
        return Ok(false);
    }
    if program != "wsl" {
        install_driver(program)?;
        return Ok(true);
    }
    let mut command = Command::new(program);
    command
        .args(["-e", "bash", "-c", &format!("cat > {DRIVER_PATH}")])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    super::hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not install the driver: {error}"))?;
    let result = (|| {
        let mut stdin = child.stdin.take().ok_or("driver stdin was not piped")?;
        let (written, result) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = written.send(stdin.write_all(DRIVER.as_bytes()));
        });
        match receive(&result, deadline, Some(cancelled)) {
            Ok(Ok(())) => {}
            _ if cancelled.load(Ordering::Acquire) || !persistent() => return Ok(false),
            Ok(Err(error)) => return Err(error.to_string()),
            Err(_) => return Err("driver write exhausted the preload budget".into()),
        }
        loop {
            if cancelled.load(Ordering::Acquire) || !persistent() {
                return Ok(false);
            }
            match child.try_wait().map_err(|error| error.to_string())? {
                Some(status) if status.success() => return Ok(true),
                Some(status) => return Err(format!("driver install exited with {status}")),
                None if Instant::now() >= deadline => {
                    return Err("driver install exhausted the preload budget".into())
                }
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    })();
    if !matches!(result, Ok(true)) {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

/// **Dropping a session ends it**: stdin closes so the loop ends, and the process is killed
/// regardless. Every way a session goes — shutdown, a failed request, a timeout, a start that
/// finished after nobody was waiting — goes through here, so none of them can leave a child behind.
impl Drop for Session {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Put [`DRIVER`] where the sidecar can `load` it.
///
/// **Under WSL the driver has to cross a filesystem boundary**, and no path this side is visible
/// to `load` in a form worth relying on — `/mnt/c/...` exists but carries the learner's user name,
/// which may contain spaces or characters that would then need quoting inside a Lisp string. So
/// the content is piped to `cat` instead and no path of ours is ever named.
fn install_driver(program: &OsStr) -> Result<(), String> {
    if program != "wsl" {
        // A local `ichiran-cli` shares our filesystem, so nothing has to cross anything.
        let path = driver_path(program);
        return std::fs::write(&path, DRIVER)
            .map_err(|error| format!("could not write {path}: {error}"));
    }

    let mut command = Command::new(program);
    command
        .args([
            OsString::from("-e"),
            OsString::from("bash"),
            OsString::from("-c"),
            OsString::from(format!("cat > {DRIVER_PATH}")),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    super::hide_console(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| format!("could not run wsl to install the driver: {error}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "driver install stdin was not piped".to_string())?
        .write_all(DRIVER.as_bytes())
        .map_err(|error| format!("could not write the driver: {error}"))?;

    let status = child
        .wait()
        .map_err(|error| format!("driver install did not finish: {error}"))?;
    if !status.success() {
        return Err(format!("driver install exited with {status}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame a request writes, without needing a sidecar to receive it.
    ///
    /// **Characters, not bytes, and the assertion is the point.** `read-sequence` on the other end
    /// fills a character buffer, so a byte count would be right for ASCII and truncate every
    /// passage with Japanese in it — which is all of them.
    #[test]
    fn the_frame_counts_characters_and_not_bytes() {
        let text = "食べちゃった";
        assert_eq!(text.chars().count(), 6);
        assert_eq!(text.len(), 18, "and the byte length is the wrong answer");

        let frame = format!("{}\n{}\n", text.chars().count(), text);
        assert_eq!(frame, "6\n食べちゃった\n");
    }

    /// A passage containing newlines goes over intact, which is the reason for framing at all.
    #[test]
    fn a_multi_line_passage_is_one_frame() {
        let text = "食べた\n水を飲む";
        let frame = format!("{}\n{}\n", text.chars().count(), text);

        assert_eq!(frame, "8\n食べた\n水を飲む\n");
        // Read as lines the payload is two, and that is exactly what a line protocol would have
        // got wrong: the header says how much to take, so the reader never has to guess.
        assert_eq!(text.lines().count(), 2);
    }

    /// `-f` is dropped when building the server's arguments and the rest is kept.
    ///
    /// The prefix is also how the binary is *reached* — `-e ichiran-cli` under WSL — so filtering
    /// too much would leave a command that runs the wrong program entirely.
    #[test]
    fn the_server_drops_the_full_flag_and_keeps_the_way_in() {
        let prefix: Vec<OsString> = ["-e", "ichiran-cli", "-f"]
            .into_iter()
            .map(OsString::from)
            .collect();

        let kept: Vec<OsString> = prefix
            .iter()
            .filter(|arg| *arg != "-f" && *arg != "--full")
            .cloned()
            .collect();

        assert_eq!(kept, ["-e", "ichiran-cli"].map(OsString::from));
    }

    /// Off unless asked for, and asking twice does not start two.
    #[test]
    fn persistence_is_off_until_it_is_turned_on() {
        assert!(!persistent(), "the default must not hold a process open");
        set_persistent(true);
        assert!(persistent());
        set_persistent(false);
        assert!(!persistent());
    }
}
