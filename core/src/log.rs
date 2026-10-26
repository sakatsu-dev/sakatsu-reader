//! An optional log file, off by default.
//!
//! # Why this exists
//!
//! Everything the app had to say went to stderr, and `main.rs` sets
//! `windows_subsystem = "windows"` outside debug — so a packaged 屠つ has no console and
//! discards all of it. A bug report from an installed copy had nothing to attach.
//!
//! # Why it lives in core
//!
//! The timings worth having — recognition, analysis, lookup, translation — all happen
//! here, and this crate has no route to anything in the shell. It does not decide *where*
//! the file goes: the shell hands it a path, the same way it hands over `model_dir()`.
//! This crate stays free of OS knowledge (see the crate docs).
//!
//! # Two switches
//!
//! [`enable`] takes both. The first turns the file on and records **shapes, timings and
//! decisions** — how long a read took, which engine ran, why the clipboard was skipped.
//! That is safe to attach to a bug report as it stands.
//!
//! The second adds the **text**: what was recognized, what was analyzed, what was copied.
//! It is a separate opt-in because this app reads whatever is on screen and on the
//! clipboard, so a log holding that content is a transcript of what someone has been
//! reading. It is worth having while chasing a misread and worth being deliberate about.
//!
//! # Rules this holds itself to
//!
//! - **Off means no file.** Not an empty one — nothing is created until [`enable`].
//! - **Arguments are not evaluated when off.** The macros check first, so a message that
//!   costs something to build costs nothing while logging is disabled.
//! - **Every write is best-effort.** A logger that can fail the operation it is watching
//!   is worse than no logger, so errors here are swallowed on purpose.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Roll over at this size. Two files exist at most, so the ceiling is twice this.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Checked before anything else on every call, so the cost while off is one atomic load.
static ON: AtomicBool = AtomicBool::new(false);
static CONTENT: AtomicBool = AtomicBool::new(false);

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

struct Sink {
    path: PathBuf,
    file: File,
    written: u64,
}

/// How loud a line is. There is no filtering — everything written is kept — so this is a
/// label for reading the file rather than a threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            // Padded so the target column lines up and the file can be scanned by eye.
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        }
    }
}

/// Start logging to `path`, creating its directory if needed.
///
/// `content` decides whether [`log_text!`] writes anything. Calling this again replaces
/// whatever was open, which is what a settings change does.
pub fn enable(path: &Path, content: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let written = file.metadata().map(|meta| meta.len()).unwrap_or(0);

    {
        let mut sink = SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *sink = Some(Sink {
            path: path.to_path_buf(),
            file,
            written,
        });
    }

    CONTENT.store(content, Ordering::Relaxed);
    // Last, so nothing can be written before there is somewhere to put it.
    ON.store(true, Ordering::Relaxed);
    Ok(())
}

/// Stop logging and close the file.
pub fn disable() {
    // First, so no thread is mid-write when the sink goes.
    ON.store(false, Ordering::Relaxed);
    CONTENT.store(false, Ordering::Relaxed);
    let mut sink = SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *sink = None;
}

/// Whether a file is open. The macros check this before building their message.
pub fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Whether recognized and analyzed text may be written.
pub fn content_allowed() -> bool {
    ON.load(Ordering::Relaxed) && CONTENT.load(Ordering::Relaxed)
}

/// Where the file is, if one is open.
pub fn path() -> Option<PathBuf> {
    SINK.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .map(|sink| sink.path.clone())
}

/// Write one line. Called by the macros; use those.
///
/// Silent on failure by design — a full disk or a file someone deleted underneath us must
/// not turn into an error in the middle of a capture.
pub fn write(level: Level, target: &str, message: &str) {
    if !is_on() {
        return;
    }

    let line = format!(
        "{}  {}  {:<10}  {}\n",
        timestamp(),
        level.label(),
        target,
        message
    );

    let mut guard = SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(sink) = guard.as_mut() else {
        return;
    };

    if sink.file.write_all(line.as_bytes()).is_err() {
        return;
    }
    sink.written += line.len() as u64;

    if sink.written >= MAX_BYTES {
        rotate(sink);
    }
}

/// Move the current file aside and start a fresh one.
///
/// One generation kept. A log that grows without bound is its own bug, and keeping a
/// history of them would need a cleanup policy that nothing here would ever run.
fn rotate(sink: &mut Sink) {
    let previous = sink.path.with_extension("log.1");
    let _ = std::fs::rename(&sink.path, &previous);

    match OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&sink.path)
    {
        Ok(file) => {
            sink.file = file;
            sink.written = 0;
        }
        // The rename already happened, so the old file is safe; there is simply nowhere to
        // write until the next `enable`. Better than tearing down the app over a log.
        Err(_) => {
            sink.written = 0;
        }
    }
}

fn timestamp() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%.3f%:z")
        .to_string()
}

/// Times a scope and logs how long it took, however it exits.
///
/// A drop guard rather than a timing call at the end of the function, because the paths worth
/// measuring here are full of `?` — an OCR pass that fails halfway is still work that took
/// time, and a version that only logged on success would report the fast cases and stay quiet
/// about the slow ones, which is the wrong way round for finding a bottleneck.
///
/// **The duration is not content.** `log_text!` exists for what the learner is reading and is
/// gated on their say-so; a millisecond count says nothing about the passage, so this is
/// written whenever logging is on at all. Anything identifying belongs in `note`, and callers
/// must keep it to what they would be willing to write down — see `content_allowed`.
pub struct Timing {
    target: &'static str,
    note: String,
    start: std::time::Instant,
}

impl Timing {
    pub fn start(target: &'static str, note: impl Into<String>) -> Option<Timing> {
        // `None` when logging is off, so a caller pays nothing at all — not the `Instant`,
        // not the `String`. The measured paths run on every capture and every search.
        is_on().then(|| Timing {
            target,
            note: note.into(),
            start: std::time::Instant::now(),
        })
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        // Milliseconds with one decimal: these are tens to thousands of milliseconds, and a
        // bare integer loses the difference between a fast path and a free one.
        let ms = self.start.elapsed().as_secs_f64() * 1000.0;
        write(
            Level::Info,
            self.target,
            &format!("{} took {ms:.1}ms", self.note),
        );
    }
}

/// A note about something the app did.
#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => {
        if $crate::log::is_on() {
            $crate::log::write($crate::log::Level::Info, $target, &format!($($arg)*));
        }
    };
}

/// Something went sideways but the app carried on.
#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => {
        if $crate::log::is_on() {
            $crate::log::write($crate::log::Level::Warn, $target, &format!($($arg)*));
        }
    };
}

/// Something failed.
#[macro_export]
macro_rules! log_error {
    ($target:expr, $($arg:tt)*) => {
        if $crate::log::is_on() {
            $crate::log::write($crate::log::Level::Error, $target, &format!($($arg)*));
        }
    };
}

/// Text the user was reading — only when they have asked for it.
///
/// The gate is checked *here*, before the arguments are formatted, so a recognized passage
/// is never built into a string that then gets thrown away. That is the difference between
/// a switch that is off and a switch that merely discards.
#[macro_export]
macro_rules! log_text {
    ($target:expr, $($arg:tt)*) => {
        if $crate::log::content_allowed() {
            $crate::log::write($crate::log::Level::Info, $target, &format!($($arg)*));
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of its own per test, since the sink is global.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir =
                std::env::temp_dir().join(format!("sakatsu-log-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch");
            Scratch(dir)
        }
        fn file(&self) -> PathBuf {
            self.0.join("sakatsu.log")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            disable();
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The sink is global, so these run one at a time.
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static SERIAL: Mutex<()> = Mutex::new(());
        SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A duration is written whenever logging is on, and does not wait on the content gate.
    /// A millisecond count says nothing about what was being read.
    #[test]
    fn a_timing_is_written_without_content_logging() {
        let _serial = lock();
        let scratch = Scratch::new("timing");
        enable(&scratch.file(), false).expect("enable");
        assert!(
            !content_allowed(),
            "this test is about the content gate being shut"
        );

        {
            let _timing = Timing::start("test", "doing the thing");
        }

        let written = std::fs::read_to_string(scratch.file()).expect("read");
        assert!(
            written.contains("doing the thing took"),
            "no timing line: {written:?}"
        );
        assert!(written.contains("ms"), "no unit: {written:?}");
    }

    /// Nothing measured, nothing allocated, nothing written. The timed paths run on every
    /// capture and every search, so an off switch that still did the work would be a cost
    /// paid by everyone who turned it off.
    #[test]
    fn a_timing_costs_nothing_while_off() {
        let _serial = lock();
        let scratch = Scratch::new("timing-off");
        disable();

        {
            let timing = Timing::start("test", "doing the thing");
            assert!(
                timing.is_none(),
                "no clock should be started while logging is off"
            );
        }

        assert!(!scratch.file().exists());
    }

    /// The guard reports however the scope ended, which is the reason it is a guard: a pass
    /// that fails halfway still took time, and measuring only the successes would report the
    /// fast cases and stay quiet about the slow ones.
    #[test]
    fn a_timing_is_written_even_when_the_scope_fails() {
        let _serial = lock();
        let scratch = Scratch::new("timing-err");
        enable(&scratch.file(), false).expect("enable");

        fn work() -> Result<(), &'static str> {
            let _timing = Timing::start("test", "work that fails");
            Err("gave up")?;
            unreachable!()
        }
        assert!(work().is_err());

        let written = std::fs::read_to_string(scratch.file()).expect("read");
        assert!(
            written.contains("work that fails took"),
            "no timing line: {written:?}"
        );
    }

    #[test]
    fn off_means_no_file_at_all() {
        let _serial = lock();
        let scratch = Scratch::new("off");
        disable();

        log_info!("test", "this goes nowhere");

        assert!(!is_on());
        assert!(
            !scratch.file().exists(),
            "disabled logging must not create a file, not even an empty one"
        );
    }

    #[test]
    fn arguments_are_not_evaluated_while_off() {
        let _serial = lock();
        let _scratch = Scratch::new("lazy");
        disable();

        // If the macro formatted first and checked afterwards, this would panic. That is
        // the difference between a switch that is off and one that merely throws the
        // result away — and it is invisible in the output either way.
        fn must_not_run() -> String {
            panic!("the message was built while logging was off");
        }

        log_info!("test", "{}", must_not_run());
        log_text!("test", "{}", must_not_run());
    }

    #[test]
    fn content_is_written_only_when_asked_for() {
        let _serial = lock();
        let scratch = Scratch::new("content");

        enable(&scratch.file(), false).expect("enable");
        log_info!("ocr", "read 11 characters");
        log_text!("ocr", "text: {}", "気持ちが伝わってきます");
        disable();

        let quiet = std::fs::read_to_string(scratch.file()).expect("read");
        assert!(
            quiet.contains("read 11 characters"),
            "shapes are always logged"
        );
        assert!(
            !quiet.contains("気持ち"),
            "the passage must not be in the file when content logging is off:\n{quiet}"
        );

        enable(&scratch.file(), true).expect("re-enable");
        log_text!("ocr", "text: {}", "気持ちが伝わってきます");
        disable();

        let loud = std::fs::read_to_string(scratch.file()).expect("read");
        assert!(
            loud.contains("気持ちが伝わってきます"),
            "asked for, so written"
        );
    }

    #[test]
    fn writing_after_disable_is_a_no_op_and_not_an_error() {
        let _serial = lock();
        let scratch = Scratch::new("closed");

        enable(&scratch.file(), false).expect("enable");
        log_info!("test", "before");
        disable();
        log_info!("test", "after");

        let body = std::fs::read_to_string(scratch.file()).expect("read");
        assert!(body.contains("before"));
        assert!(!body.contains("after"), "closed means closed");
    }

    #[test]
    fn the_file_rotates_and_only_two_are_kept() {
        let _serial = lock();
        let scratch = Scratch::new("rotate");

        enable(&scratch.file(), false).expect("enable");
        // Enough to cross MAX_BYTES twice over, so a second rotation replaces the first
        // archive rather than accumulating a third file.
        let padding = "x".repeat(512);
        for i in 0..(MAX_BYTES / 400 * 2) {
            log_info!("test", "line {i} {padding}");
        }
        disable();

        let previous = scratch.file().with_extension("log.1");
        assert!(scratch.file().exists(), "a live file survives rotation");
        assert!(previous.exists(), "the previous generation is kept");

        let count = std::fs::read_dir(&scratch.0).expect("read dir").count();
        assert_eq!(count, 2, "exactly two files, however long the session runs");

        let live = std::fs::metadata(scratch.file()).expect("meta").len();
        assert!(live < MAX_BYTES, "the live file is bounded, was {live}");
    }

    #[test]
    fn a_line_carries_its_level_and_target() {
        let _serial = lock();
        let scratch = Scratch::new("shape");

        enable(&scratch.file(), false).expect("enable");
        log_warn!("clipboard", "skipped: not Japanese");
        disable();

        let body = std::fs::read_to_string(scratch.file()).expect("read");
        assert!(body.contains("WARN"), "the level is readable: {body}");
        assert!(
            body.contains("clipboard"),
            "the subsystem is readable: {body}"
        );
        assert!(body.contains("skipped: not Japanese"));
        // A timestamp that starts with the year is enough to know one is there without
        // pinning the format.
        assert!(
            body.starts_with("20"),
            "a line begins with its timestamp: {body}"
        );
    }
}
