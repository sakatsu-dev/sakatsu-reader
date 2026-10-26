//! Keeping the Japanese you read, as a corpus rather than as a log.
//!
//! # Why this is not `log_text!`
//!
//! The diagnostic log already had a switch for putting recognized text in it, and for chasing a
//! misread that is the right shape: interleaved with timings and engine names, in the order things
//! happened. It is the wrong shape for everything else. A corpus wants one passage per line, no
//! duplicates, no surrounding prose — the format `SAKATSU_ANALYZER_SENTENCES` already reads, so a
//! collected file can be pointed straight at `analyzer_corpus.rs` with nothing in between.
//!
//! That difference is the whole reason this exists beside the log rather than inside it.
//!
//! # What is collected, and what is not
//!
//! **Recognized captures only.** Not clipboard lines, not analysis of text typed into the
//! reference box — those are reading the user did not choose to capture, and a corpus of them
//! would be a transcript rather than a collection. The switch is described to the user as
//! collecting what the camera reads, and it should keep meaning that.
//!
//! Passages with no Japanese in them are dropped. OCR on a mostly-blank panel produces punctuation
//! and stray latin, and a corpus of that costs whoever reads it more than it gives.
//!
//! # Newlines become spaces, deliberately
//!
//! A multi-line capture is **one passage**, and the file format is one passage per line, so the
//! two have to be reconciled. Flattening to a space is the same choice `scripts/fetch-jesc.ps1`
//! makes for subtitle line breaks and for the same reason: both analyzers treat the space as a
//! boundary, so it suppresses a compound across the gap rather than inventing one, where splitting
//! into separate lines would claim two passages that were never two.
//!
//! # Privacy is the same shape as the log's
//!
//! Off unless asked for, and the file is the learner's own — `store::Settings` says of the log's
//! text switch that a file holding what someone has been reading is "worth being asked about",
//! and nothing about writing it somewhere more useful changes that.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::model::contains_japanese;

/// Where collected passages go, and what is already in it. `None` when collecting is off.
static SINK: Mutex<Option<Sink>> = Mutex::new(None);

struct Sink {
    path: PathBuf,
    file: File,
    /// Every passage the file already holds, by [`key`], so re-reading the same panel adds nothing.
    ///
    /// Loaded from disk on [`enable`] rather than kept only for the session: a corpus is built
    /// across many sittings, and a duplicate check that forgets on restart would let the same
    /// dozen lines accumulate every time the app opens.
    seen: HashSet<String>,
}

/// Start collecting into `path`, reading whatever it already holds.
///
/// Calling this again replaces the target, which is what a settings change does.
pub fn enable(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut seen = HashSet::new();
    if let Ok(existing) = File::open(path) {
        for line in BufReader::new(existing).lines().map_while(Result::ok) {
            let line = line.trim();
            if !line.is_empty() {
                seen.insert(key(line));
            }
        }
    }

    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut sink = SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *sink = Some(Sink {
        path: path.to_path_buf(),
        file,
        seen,
    });
    Ok(())
}

/// Stop collecting. Safe to call when it was never started.
pub fn disable() {
    let mut sink = SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *sink = None;
}

/// One passage per line, whitespace flattened — see the module note.
fn flatten(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What two captures have to share to count as the same passage: everything but the spacing.
///
/// **Stricter than what gets written, and deliberately.** OCR does not reproduce spacing reliably
/// — the same panel captured twice with a slightly different crop can come back with a space where
/// it had none — so comparing the flattened forms would file those as two passages and the corpus
/// would fill with near-duplicates of whatever is hardest to crop.
///
/// The cost is that 食べた 水を飲む and 食べた水を飲む are one key, and only the first is kept.
/// Those are different passages in principle; in a corpus of captures they are overwhelmingly the
/// same panel read twice, and that is the way to be wrong here.
fn key(passage: &str) -> String {
    passage.split_whitespace().collect()
}

/// Append a recognized passage. Returns whether it was new.
///
/// Everything about this is silent: a corpus that interrupts reading to report itself would be
/// worse than one that misses a line. A write that fails is dropped, and the next passage tries
/// again — the alternative is an error dialog over the text somebody is in the middle of.
pub fn collect(text: &str) -> bool {
    let passage = flatten(text);
    if passage.is_empty() || !contains_japanese(&passage) {
        return false;
    }

    let mut guard = SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(sink) = guard.as_mut() else {
        return false;
    };
    if !sink.seen.insert(key(&passage)) {
        return false;
    }
    if writeln!(sink.file, "{passage}").is_err() {
        // Keep it in `seen` regardless: retrying a passage that failed to write once is far more
        // likely to fail again than to succeed, and the corpus is not worth a stutter in the app.
        return false;
    }
    let _ = sink.file.flush();
    true
}

/// How many passages the file at `path` holds, whether or not collecting is on.
///
/// **[`status`] cannot answer this and that is why both exist.** It reports the live sink, so it
/// returns `None` the moment collecting is switched off — and a settings panel has to say what the
/// file holds *especially* then, because "you have collected 340 passages" is the reason to turn
/// it back on. Counted from disk rather than remembered, so it is right after a hand edit too.
pub fn held_in(path: &Path) -> usize {
    let Ok(file) = File::open(path) else {
        return 0;
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .count()
}

/// Where collecting is writing, and how much it holds. `None` when it is off.
pub fn status() -> Option<(PathBuf, usize)> {
    let guard = SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .as_ref()
        .map(|sink| (sink.path.clone(), sink.seen.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sink is global, so these run one at a time — the same guard `log.rs` uses, for the
    /// same reason.
    ///
    /// **A private file each was not enough, and that is the trap.** Every test already took its
    /// own scratch path, which looks like isolation; the shared thing is `SINK`, not the file. Two
    /// tests in parallel meant one calling `disable()` while the other was still collecting, so
    /// `collect` returned false and the failure read as "Japanese beside latin is not collected"
    /// — a claim about the language rule, pointing nowhere near the actual cause. It passed for
    /// as long as the scheduler happened to keep them apart.
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static SERIAL: Mutex<()> = Mutex::new(());
        SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Each test needs its own file, and has to clean up after itself.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sakatsu-corpus-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir.join("captured.txt")
    }

    #[test]
    fn a_passage_is_collected_once_however_many_times_it_is_read() {
        let _serial = lock();
        let path = scratch("dedupe");
        enable(&path).expect("enable");

        assert!(collect("気持ちが伝わってきます"), "the first read is new");
        assert!(
            !collect("気持ちが伝わってきます"),
            "reading the same panel again must not add a second line"
        );
        // Whitespace differences are the same passage: OCR does not reproduce spacing reliably,
        // and two lines differing only in a space would defeat the whole point of collecting.
        assert!(
            !collect("気持ちが  伝わってきます"),
            "flattening happens before the duplicate check, not after"
        );
        disable();

        let body = std::fs::read_to_string(&path).expect("written");
        assert_eq!(body.lines().count(), 1, "one line, got {body:?}");
    }

    /// **Duplicates are remembered across restarts, which is why the file is read on enable.**
    /// A corpus is built over many sittings; a check that only knew this session would re-add the
    /// same lines every launch and quietly make the file useless.
    #[test]
    fn what_the_file_already_holds_is_not_collected_again() {
        let _serial = lock();
        let path = scratch("reopen");
        enable(&path).expect("enable");
        assert!(collect("水を飲む"));
        disable();

        enable(&path).expect("re-enable");
        assert!(
            !collect("水を飲む"),
            "the passage is already in the file from the last session"
        );
        assert!(collect("本を読む"), "but a new one still lands");
        disable();

        let body = std::fs::read_to_string(&path).expect("written");
        assert_eq!(body.lines().count(), 2, "got {body:?}");
    }

    /// A multi-line capture is one passage, not several.
    #[test]
    fn a_multi_line_capture_becomes_one_line() {
        let _serial = lock();
        let path = scratch("multiline");
        enable(&path).expect("enable");
        assert!(collect("食べた\n水を飲む"));
        disable();

        let body = std::fs::read_to_string(&path).expect("written");
        assert_eq!(body.trim(), "食べた 水を飲む");
        assert_eq!(body.lines().count(), 1);
    }

    /// OCR on a blank panel is punctuation and stray latin, and it is not corpus.
    #[test]
    fn a_passage_with_no_japanese_is_not_collected() {
        let _serial = lock();
        let path = scratch("empty");
        enable(&path).expect("enable");
        assert!(!collect("..."));
        assert!(!collect("   "));
        assert!(!collect("Chapter 12"));
        assert!(collect("第12章です"), "but Japanese beside latin is");
        disable();

        let body = std::fs::read_to_string(&path).expect("written");
        assert_eq!(body.lines().count(), 1, "got {body:?}");
    }

    /// Collecting while it is switched off does nothing and says so.
    #[test]
    fn nothing_is_collected_while_it_is_off() {
        let _serial = lock();
        disable();
        assert!(!collect("水を飲む"));
        assert!(status().is_none());
    }
}
