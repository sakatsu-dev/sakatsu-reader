//! Watch the clipboard for Japanese, and hand it to the reader.
//!
//! Reading a visual novel usually means a text hooker putting each line on the clipboard.
//! With this on, each line arrives and is analyzed; without it, that is a button press per
//! line.
//!
//! # Why this does not poll by reading
//!
//! The thing being watched is *another program writing to the clipboard*, several times a
//! minute. Opening the clipboard takes a global lock, and a program that cannot take it
//! fails — so a watcher that opens it three times a second is a watcher that makes text
//! hookers drop lines. That is the failure this would cause and never see, because it
//! would happen in the other application.
//!
//! `GetClipboardSequenceNumber` is a counter Windows bumps on every change. Reading it
//! takes no lock and cannot fail. So the loop asks the counter, and only opens the
//! clipboard on the rare tick where something actually changed.
//! The reference page's explicit copy action is the one app-owned write; [`write_text`]
//! marks it so the loop below does not mistake a lookup aid for a new passage.
//!
//! # One thread, always alive
//!
//! Started once at setup and parked on an `AtomicBool` when the setting is off, rather
//! than spawned and joined per toggle. There is no work to save — a sleeping thread costs
//! a stack — and a lifecycle is a thing to get wrong.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{Emitter, Runtime};

use crate::capture;

/// The event carrying a copied line to the reader.
pub const CLIPBOARD_TEXT: &str = "clipboard-text";

/// How often to ask whether the clipboard changed.
///
/// The question is nearly free, so this is set by how soon a line should appear rather
/// than by what it costs. Faster than a page turn, slower than a keystroke.
const POLL: Duration = Duration::from_millis(200);

/// Longer than this is not a line someone is reading.
///
/// A copied chapter, a stack trace, a spreadsheet column: sending any of those through the
/// analyzer would lock the interface for seconds and produce something nobody asked to
/// study. The cap is generous — several long sentences fit.
const MAX_CHARS: usize = 2_000;

static WATCHING: AtomicBool = AtomicBool::new(false);

/// The last clipboard generation this has already considered.
///
/// Also the arming mechanism: switching the setting on stores the *current* value, so
/// whatever was copied before then is never read. Turning on a watcher should not reach
/// back and act on something copied ten minutes ago for an unrelated reason.
static SEEN: AtomicU32 = AtomicU32::new(0);

/// Text this app deliberately put on the clipboard for another program.
///
/// The reference page has a copy button, while this module can also watch copied Japanese
/// and send it back to the reader. Without remembering our own write, those two features
/// form a loop: copying a headword immediately replaces the passage with that headword.
/// Matching the text rather than blindly ignoring the next generation preserves an external
/// write that happens in the small interval before the watcher wakes up.
static SELF_WRITE: Mutex<Option<String>> = Mutex::new(None);

/// Turn watching on or off.
pub fn set_watching(on: bool) {
    if on && !WATCHING.swap(true, Ordering::SeqCst) {
        // A copy made while watching was off is already covered by arming `SEEN` at the
        // current generation, and must not make an unrelated future copy look like ours.
        remember_self_write(None);
        SEEN.store(generation(), Ordering::SeqCst);
        sakatsu_core::log_info!(
            "clipboard",
            "watching — armed at generation {}, so nothing already copied is read",
            SEEN.load(Ordering::SeqCst)
        );
    } else if !on && WATCHING.swap(false, Ordering::SeqCst) {
        sakatsu_core::log_info!("clipboard", "no longer watching");
    }
}

/// Put a reference headword on the clipboard without feeding it back into the reader.
pub fn write_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("there is no text to copy".into());
    }

    // Arm before taking the clipboard lock. The watcher sleeps between checks, but setting
    // this afterwards would still leave a race where it wakes between `set_text` and the
    // marker and treats the app's own word as a new passage.
    if WATCHING.load(Ordering::SeqCst) {
        remember_self_write(Some(text.to_string()));
    }

    let result = (|| {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|error| format!("no clipboard: {error}"))?;
        clipboard
            .set_text(text)
            .map_err(|error| format!("could not copy text: {error}"))
    })();

    if result.is_err() {
        remember_self_write(None);
    }
    result
}

fn remember_self_write(text: Option<String>) {
    *SELF_WRITE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = text;
}

/// Whether this is the write made by [`write_text`].
///
/// Every changed clipboard consumes the marker. If another application won the race and
/// wrote different text, its line proceeds normally and the stale marker cannot suppress a
/// later copy that happens to contain the old headword.
fn consume_self_write(text: &str) -> bool {
    let mut expected = SELF_WRITE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let ours = expected.as_deref() == Some(text);
    *expected = None;
    ours
}

/// A number that changes whenever the clipboard does.
///
/// Zero when there is nothing to ask — on a platform without the call, the loop falls back
/// to comparing the text itself, which is what `last` in [`watch`] is for either way.
fn generation() -> u32 {
    #[cfg(windows)]
    {
        // Safety: no arguments, no handles, no clipboard lock. It reads a counter.
        unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Start the watcher. Called once, from setup.
pub fn watch<R: Runtime>(app: tauri::AppHandle<R>) {
    std::thread::spawn(move || {
        // What was last handed to the reader. The counter says *something* changed, which
        // is not the same as the text being new: several applications rewrite the
        // clipboard with identical content, and on platforms with no counter this is the
        // only check there is.
        let mut last = String::new();

        loop {
            std::thread::sleep(POLL);
            if !WATCHING.load(Ordering::SeqCst) {
                continue;
            }

            let now = generation();
            // `now == 0` means no counter is available, so every tick has to look.
            if now != 0 && now == SEEN.swap(now, Ordering::SeqCst) {
                continue;
            }

            // Text only. An image landing on the clipboard while reading should not
            // quietly start an OCR run that nobody asked for and that takes seconds.
            let Ok(capture::Clipboard::Text(text)) = capture::read_clipboard() else {
                sakatsu_core::log_info!("clipboard", "changed, but holds no text — ignored");
                continue;
            };

            if consume_self_write(&text) {
                sakatsu_core::log_info!("clipboard", "skipped: written by the reference page");
                continue;
            }

            // Every skip says which one it was. "I copied a line and nothing happened" has
            // four different answers and they need telling apart; a watcher that decides
            // silently is one nobody can argue with.
            let trimmed = text.trim();
            if !worth_reading(trimmed) {
                sakatsu_core::log_info!(
                    "clipboard",
                    "skipped {} chars: {}",
                    trimmed.chars().count(),
                    if trimmed.is_empty() {
                        "empty"
                    } else if trimmed.chars().count() > MAX_CHARS {
                        "over the length cap"
                    } else {
                        "no Japanese in it"
                    }
                );
                continue;
            }
            if trimmed == last {
                sakatsu_core::log_info!("clipboard", "skipped: same text as last time");
                continue;
            }

            last = trimmed.to_string();
            sakatsu_core::log_info!("clipboard", "read {} chars", last.chars().count());
            sakatsu_core::log_text!("clipboard", "read: {last:?}");
            let _ = app.emit(CLIPBOARD_TEXT, &last);
        }
    });
}

/// Whether a copied string is a line someone is reading.
///
/// Separate from the loop so it can be tested — the loop itself needs a real clipboard, a
/// real event loop and someone pressing Ctrl+C, and this is where all the judgement is.
fn worth_reading(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= MAX_CHARS
        && sakatsu_core::model::contains_japanese(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_is_read_and_other_copying_is_left_alone() {
        assert!(worth_reading("気持ちが伝わってきます"));
        assert!(worth_reading("「そうか」と彼は言った。"));
        // Copying a line out of a bilingual page still has Japanese in it.
        assert!(worth_reading("ramen (ラーメン)"));

        assert!(!worth_reading(""), "an empty clipboard is not a line");
        assert!(
            !worth_reading("git commit -m fix"),
            "not everything copied is Japanese"
        );
        assert!(
            !worth_reading("。、！？"),
            "Japanese punctuation with no word in it has nothing to look up"
        );
    }

    #[test]
    fn a_copied_chapter_is_not_a_line() {
        // Copying a whole page, a stack trace or a spreadsheet column: sending any of
        // those through the analyzer locks the interface for seconds to produce something
        // nobody asked to study.
        let long: String = "あ".repeat(MAX_CHARS + 1);
        assert!(!worth_reading(&long));

        // The boundary itself is fine — the cap is for runaway pastes, not long sentences.
        let at_limit: String = "あ".repeat(MAX_CHARS);
        assert!(worth_reading(&at_limit));
    }

    #[test]
    fn the_cap_counts_characters_and_not_bytes() {
        // Japanese is three bytes per character in UTF-8, so a byte-length cap would cut
        // the limit to a third for exactly the text this feature exists to read.
        let text: String = "日".repeat(1_000);
        assert!(
            text.len() > MAX_CHARS,
            "the byte length alone would exceed the cap"
        );
        assert!(worth_reading(&text));
    }

    #[test]
    fn a_self_write_is_ignored_once_without_swallowing_an_external_copy() {
        remember_self_write(Some("水".into()));
        assert!(consume_self_write("水"));
        assert!(!consume_self_write("水"), "the marker is single-use");

        remember_self_write(Some("水".into()));
        assert!(!consume_self_write("火"));
        assert!(
            !consume_self_write("水"),
            "different clipboard text consumes the stale marker"
        );
    }
}
