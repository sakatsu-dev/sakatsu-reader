//! An [`Analyzer`] that remembers what it has already been asked.
//!
//! Ichiran runs as a sidecar — `wsl -e ichiran-cli -f <text>`, a fresh process every call,
//! each one loading a large Lisp image before it can answer. That cost lands in three places,
//! and only the first is one anybody expects:
//!
//! - the Analyze button, once per passage;
//! - **every Japanese reference search**, so a launch that restores six tabs pays six of
//!   them, on a page that should feel instant;
//! - **every `readiness()` call**, which analyses `"水"` as a health check — the same string,
//!   every time.
//!
//! The proper fix is a resident process, and `ichiran-cli` cannot be one: it takes an
//! argument, prints, and exits, with no server, daemon or stdin loop to send a second query
//! to. So this does the next thing, which is to stop asking twice.
//!
//! Analysis is a pure function of its input — the same text gives the same tokens — so the
//! answer keeps. The [`Analyzer`] trait anticipated exactly this: `engine()`'s own
//! documentation says it exists "to invalidate caches when the engine changes".
//!
//! **What this cannot do** is make the first analysis of new text any faster. Nothing
//! available can; that is the daemon this is standing in for.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Condvar, Mutex};

use serde::{Deserialize, Serialize};

use super::{AnalyzeError, Analyzer};
use crate::model::Analysis;

/// Where a word may be written down between sessions, when that is allowed at all.
///
/// `None` means it is not, and setting it to `None` **deletes** whatever was there. That is not
/// tidiness. This file is a record of what somebody has been reading, and a switch that stopped
/// adding to such a record while leaving it on disk would be a curtain rather than a switch — off
/// has to mean gone, or the setting does not say what it appears to say.
///
/// (The store's search history made the same promise in the same words, and was the precedent
/// cited here until the reference page's tabs took over the job and it was removed.)
static PERSIST: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Say where words may be kept, or that they may not be.
pub fn set_persistence(at: Option<PathBuf>) {
    let Ok(mut slot) = PERSIST.lock() else {
        return;
    };
    if at.is_none() {
        if let Some(old) = slot.as_ref() {
            let _ = std::fs::remove_file(old);
        }
    }
    *slot = at;
}

fn persisting() -> Option<PathBuf> {
    PERSIST.lock().ok().and_then(|slot| slot.clone())
}

/// The shape of what is written down.
///
/// **Bumped whenever [`crate::model::Token`] grows a field a rule reads.** The engine name
/// catches one kind of staleness — answers from a different analyzer — and misses the other
/// entirely: every field on `Token` is `#[serde(default)]`, so a file written before
/// `confidence` and `romaji` existed loads back with both `None` and no complaint. The
/// analyzer never runs again for those words, so the name rules stay switched off for exactly
/// the words a reader has already looked up, and it looks intermittent because it depends on
/// what happens to be in the file.
/// Bumped to 2 when `Token` grew `line_break`, which `model::ends_sentence` reads.
/// Bumped to 3 when `Token` grew `lexical_role`, which contextual ranking reads.
/// Bumped to 4 when Ichiran's tie-break began reading the tied word in its sentence
/// (`ichiran::to_analysis`): a remembered analysis holds the old choice, and serving it would
/// keep the old answer for exactly the passages already read.
const SCHEMA: u32 = 4;

/// The file, which is a list of words and nothing else.
#[derive(Serialize, Deserialize)]
struct Kept {
    /// What shape these were written in — see [`SCHEMA`]. Absent in files written before it
    /// existed, which is what `#[serde(default)]` is for: 0 is not 1, so they are dropped.
    #[serde(default)]
    schema: u32,
    /// Which analyzer produced these.
    ///
    /// `Analyzer::engine`'s own documentation says it is "used in diagnostics and to
    /// invalidate caches when the engine changes", and this is that use. Ichiran and the
    /// built-in analyzer segment differently, so reading one's answers back under the other
    /// would be worse than a cold start: it would be a wrong answer, quickly.
    engine: String,
    words: Vec<(String, Analysis)>,
}

/// Whether an analysis may be written to disk.
///
/// **One token, and that is the whole rule.** A single-token analysis is a word; anything
/// longer is a passage somebody was reading, and this app does not write those down.
///
/// Enforced here, at the write, rather than when an entry is added — so the file's contents
/// are guaranteed by the code that creates the file, not by every caller remembering to be
/// careful. The in-memory cache still holds passages; they simply never leave it.
///
/// It is not perfect anonymity and is not offered as such: a list of words is still a record
/// of what somebody has been reading, and JMnedict is a dictionary of *names*. It is the
/// difference between a vocabulary list and a transcript, which is worth having, gated on the
/// setting that already governs records of this kind.
fn may_be_written_down(analysis: &Analysis) -> bool {
    analysis.tokens.len() == 1
}

fn load(engine: &str) -> VecDeque<(String, Analysis)> {
    let Some(path) = persisting() else {
        return VecDeque::with_capacity(KEPT);
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return VecDeque::with_capacity(KEPT);
    };
    match serde_json::from_str::<Kept>(&text) {
        // A different analyzer's answers are not this analyzer's answers, and neither are
        // answers written in an older shape — see [`SCHEMA`].
        Ok(kept) if kept.engine == engine && kept.schema == SCHEMA => kept
            .words
            .into_iter()
            .filter(|(_, a)| may_be_written_down(a))
            .take(KEPT)
            .collect(),
        _ => VecDeque::with_capacity(KEPT),
    }
}

fn save(seen: &VecDeque<(String, Analysis)>, engine: &str) {
    let Some(path) = persisting() else {
        return;
    };
    let kept = Kept {
        schema: SCHEMA,
        engine: engine.to_string(),
        words: seen
            .iter()
            .filter(|(_, analysis)| may_be_written_down(analysis))
            .cloned()
            .collect(),
    };
    let Ok(json) = serde_json::to_vec(&kept) else {
        return;
    };
    // Temporary then rename, so an interrupted write cannot truncate what was there — the
    // same care `store::write_atomically` takes, for the same reason.
    let temporary = path.with_extension("tmp");
    if std::fs::write(&temporary, &json).is_ok() {
        let _ = std::fs::rename(&temporary, &path);
    }
}

/// How many analyses to keep.
///
/// Small on purpose. This is for the *shape* of the traffic — a handful of tabs, a passage
/// being reread, a health check firing over and over — not for holding a session's history.
/// A passage's analysis is a few kilobytes, and a learner working through one text touches a
/// handful of them.
const KEPT: usize = 24;

/// An analyzer that answers a repeated question from memory.
///
/// Wraps any [`Analyzer`], so it works the same over Ichiran, Lindera or a test stub — and
/// the one that most needs it is the one whose cost is a process spawn.
pub struct Remembering<A: Analyzer> {
    inner: A,
    /// Oldest first, so eviction is a `pop_front`.
    ///
    /// A `VecDeque` scanned start to end, rather than a map plus a recency list. At `KEPT`
    /// entries the scan is nothing against what it is standing in for — a process spawn and
    /// a Lisp image — and an LRU crate would be a dependency bought for a linear scan of two
    /// dozen strings.
    state: Mutex<Remembered>,
    ready: Condvar,
}

struct Remembered {
    seen: VecDeque<(String, Analysis)>,
    /// Questions currently being answered. A cache miss is not permission for every
    /// simultaneous caller to start the same expensive sidecar process.
    in_flight: HashSet<String>,
}

impl<A: Analyzer> Remembering<A> {
    pub fn new(inner: A) -> Self {
        // Whatever was kept last time, if keeping is allowed and it came from this engine.
        let seen = load(inner.engine());
        Remembering {
            inner,
            state: Mutex::new(Remembered {
                seen,
                in_flight: HashSet::new(),
            }),
            ready: Condvar::new(),
        }
    }
}

impl<A: Analyzer> Remembering<A> {
    /// The cached answer, or `ask`'s, kept if it succeeded. The one body behind both entry points.
    fn remembered(
        &self,
        text: &str,
        ask: impl FnOnce() -> Result<Analysis, AnalyzeError>,
    ) -> Result<Analysis, AnalyzeError> {
        // Trimmed, because that is what the analyzers themselves work on: `analyze` trims
        // before doing anything, so "水 " and "水" are one question and must be one entry.
        let key = text.trim();
        if key.is_empty() {
            // Straight through. The empty case is an error every implementation raises for
            // itself, and answering it from here would move that decision.
            return ask();
        }

        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if let Some((_, hit)) = state.seen.iter().find(|(had, _)| had == key) {
                return Ok(hit.clone());
            }
            if state.in_flight.insert(key.to_string()) {
                break;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        drop(state);

        let outcome = ask();

        // Only on success, and this is the important half of the rule. An
        // `EngineUnavailable` usually means WSL was not up yet — cache that and the app stays
        // broken after the cause is fixed, with no way for the learner to retry into a
        // working state.
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.in_flight.remove(key);
        if let Ok(analysis) = &outcome {
            if !state.seen.iter().any(|(had, _)| had == key) {
                if state.seen.len() >= KEPT {
                    state.seen.pop_front();
                }
                state.seen.push_back((key.to_string(), analysis.clone()));
                // Only a word changes the file. A passage is held in memory and goes no
                // further, so re-reading the same page costs nothing this session and
                // leaves nothing behind after it.
                if may_be_written_down(analysis) {
                    save(&state.seen, self.inner.engine());
                }
            }
        }
        self.ready.notify_all();
        outcome
    }
}

impl<A: Analyzer> Analyzer for Remembering<A> {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.remembered(text, || self.inner.analyze(text))
    }

    /// A hit is the converted analysis, kept, and asks no advice at all. A miss converts inside the
    /// request, so its advice is the request's.
    ///
    /// `raw` stays `None` either way, as [`Analyzer::analyze_traced`]'s default gives it here: an
    /// answer that may have come from the cache does not claim an engine said it just now.
    fn analyze_in(
        &self,
        text: &str,
        assistance: &crate::analyzer::assist::Assistance<'_>,
    ) -> Result<crate::analyzer::Traced, AnalyzeError> {
        self.remembered(text, || {
            self.inner
                .analyze_in(text, assistance)
                .map(|traced| traced.analysis)
        })
        .map(|analysis| crate::analyzer::Traced {
            raw: None,
            analysis,
        })
    }

    fn engine(&self) -> &str {
        self.inner.engine()
    }

    /// Passed straight through. **The shell warms whatever the route holds**, and when Ichiran
    /// answers that is Ichiran inside this cache: without this, opening the capture frame and
    /// starting a capture woke nothing, and the first passage after an idle spell paid the whole
    /// WSL wake-up the signal exists to hide. Nothing is cached about it — warming is not a
    /// question with an answer.
    fn warm(&self) {
        self.inner.warm();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Token;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    /// The persistence switch is a global, and every test here can reach it: the moment the
    /// stub could return a single-token analysis, *any* of them could trigger a write into
    /// whichever file a persistence test had just set. So they all take this, not only the
    /// ones that look at a file.
    fn alone() -> std::sync::MutexGuard<'static, ()> {
        static SERIAL: Mutex<()> = Mutex::new(());
        SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Counts what reaches it, so the tests can assert the thing that matters: that the
    /// expensive call did not happen twice.
    struct Counting {
        calls: AtomicUsize,
        warmed: AtomicUsize,
        fail: bool,
    }

    impl Counting {
        fn new() -> Self {
            Counting {
                calls: AtomicUsize::new(0),
                warmed: AtomicUsize::new(0),
                fail: false,
            }
        }
        fn failing() -> Self {
            Counting {
                calls: AtomicUsize::new(0),
                warmed: AtomicUsize::new(0),
                fail: true,
            }
        }
        fn count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Analyzer for Counting {
        fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if text.trim().is_empty() {
                return Err(AnalyzeError::EmptyInput);
            }
            if self.fail {
                return Err(AnalyzeError::EngineUnavailable("no wsl".into()));
            }
            // One token per whitespace-separated piece, so a test can ask for a word or a
            // passage and get the shape it asked for.
            Ok(Analysis {
                execution: None,
                text: text.trim().to_string(),
                tokens: text
                    .split_whitespace()
                    .map(|piece| Token {
                        surface: piece.to_string(),
                        ..Default::default()
                    })
                    .collect(),
            })
        }
        fn engine(&self) -> &str {
            "counting"
        }
        fn warm(&self) {
            self.warmed.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// The shell warms what the route holds, and Ichiran is held inside this cache.
    #[test]
    fn warming_the_cache_warms_what_it_wraps() {
        let _serial = alone();
        let cache = Remembering::new(Counting::new());
        cache.warm();
        cache.warm();
        assert_eq!(cache.inner.warmed.load(Ordering::SeqCst), 2);
        assert_eq!(cache.inner.count(), 0, "warming asks no question");
    }

    /// The whole point: a repeated question costs one sidecar run, not two.
    #[test]
    fn the_same_text_is_analysed_once() {
        let _serial = alone();
        let cache = Remembering::new(Counting::new());
        let first = cache.analyze("水を飲む").expect("first");
        let second = cache.analyze("水を飲む").expect("second");
        assert_eq!(
            first, second,
            "the remembered answer must be the same answer"
        );
        assert_eq!(cache.inner.count(), 1, "the sidecar ran twice");
    }

    /// The in-memory cache stores the raw analyzer result. Both a miss and a cache hit must
    /// pass through the finished-analysis contract before any response reaches the reader.
    #[test]
    fn a_cached_bad_analysis_is_rejected_on_every_read() {
        let _serial = alone();
        set_persistence(None);
        let cache = Remembering::new(Counting::new());
        for _ in 0..2 {
            let error = crate::study::analyse("水", &cache, &[]).expect_err("ungrounded token");
            assert!(error.to_string().contains("no source span"), "{error}");
        }
        assert_eq!(
            cache.inner.count(),
            1,
            "the second rejection should be a cache hit"
        );
    }
    struct Blocking {
        calls: AtomicUsize,
        entered: Barrier,
        release: Barrier,
    }

    impl Analyzer for Blocking {
        fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.wait();
            self.release.wait();
            Ok(Analysis {
                execution: None,
                text: text.into(),
                tokens: vec![Token {
                    surface: text.into(),
                    ..Default::default()
                }],
            })
        }

        fn engine(&self) -> &str {
            "blocking"
        }
    }

    #[test]
    fn simultaneous_misses_for_the_same_text_share_one_analysis() {
        let _serial = alone();
        let cache = Arc::new(Remembering::new(Blocking {
            calls: AtomicUsize::new(0),
            entered: Barrier::new(2),
            release: Barrier::new(2),
        }));

        let first_cache = Arc::clone(&cache);
        let first = std::thread::spawn(move || first_cache.analyze("水"));
        cache.inner.entered.wait();
        let second_cache = Arc::clone(&cache);
        let second = std::thread::spawn(move || second_cache.analyze("水"));
        cache.inner.release.wait();

        assert_eq!(
            first.join().expect("first thread"),
            second.join().expect("second thread")
        );
        assert_eq!(cache.inner.calls.load(Ordering::SeqCst), 1);
    }

    /// The analyzers trim before working, so these are one question.
    #[test]
    fn surrounding_space_is_the_same_question() {
        let _serial = alone();
        let cache = Remembering::new(Counting::new());
        cache.analyze("水").expect("first");
        cache.analyze("  水  ").expect("second");
        assert_eq!(cache.inner.count(), 1);
    }

    #[test]
    fn different_text_is_analysed_again() {
        let _serial = alone();
        let cache = Remembering::new(Counting::new());
        cache.analyze("水").expect("first");
        cache.analyze("火").expect("second");
        assert_eq!(cache.inner.count(), 2);
    }

    /// A failure must stay retryable. WSL not being up yet is the ordinary cause, and
    /// remembering it would leave the app broken after the learner fixed it.
    #[test]
    fn a_failure_is_not_remembered() {
        let _serial = alone();
        let cache = Remembering::new(Counting::failing());
        assert!(cache.analyze("水").is_err());
        assert!(cache.analyze("水").is_err());
        assert_eq!(
            cache.inner.count(),
            2,
            "the retry never reached the analyzer"
        );
    }

    /// The bound holds, and the oldest goes first.
    #[test]
    fn the_oldest_is_dropped_when_full() {
        let _serial = alone();
        let cache = Remembering::new(Counting::new());
        for n in 0..KEPT {
            cache.analyze(&format!("word {n}")).expect("fill");
        }
        assert_eq!(cache.inner.count(), KEPT);

        // Still remembered: the newest.
        cache
            .analyze(&format!("word {}", KEPT - 1))
            .expect("recent");
        assert_eq!(
            cache.inner.count(),
            KEPT,
            "the newest should have been remembered"
        );

        // Pushes the oldest out.
        cache.analyze("one more").expect("evict");
        cache.analyze("word 0").expect("the evicted one");
        assert_eq!(
            cache.inner.count(),
            KEPT + 2,
            "the oldest should have been dropped"
        );
    }

    /// Empty input keeps whatever the wrapped analyzer says about it, rather than this
    /// deciding on its behalf.
    #[test]
    fn an_empty_query_is_passed_through() {
        let _serial = alone();
        let cache = Remembering::new(Counting::new());
        assert_eq!(cache.analyze("   "), Err(AnalyzeError::EmptyInput));
    }

    /// The engine name has to be the real one: it is what a cache keyed on the engine would
    /// use to know the answers came from something else.
    #[test]
    fn the_engine_name_is_the_wrapped_one() {
        let _serial = alone();
        assert_eq!(Remembering::new(Counting::new()).engine(), "counting");
    }

    /// A file of its own per test, cleaned up either way.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let path = std::env::temp_dir()
                .join(format!("sakatsu-words-{name}-{}.json", std::process::id()));
            let _ = std::fs::remove_file(&path);
            set_persistence(Some(path.clone()));
            Scratch(path)
        }
        fn written(&self) -> String {
            std::fs::read_to_string(&self.0).unwrap_or_default()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            set_persistence(None);
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// A word survives a restart — which is the whole point of writing anything down.
    #[test]
    fn a_word_is_still_known_next_session() {
        let _serial = alone();
        let scratch = Scratch::new("word");

        let first = Remembering::new(Counting::new());
        first.analyze("食べる").expect("analyze");
        assert!(scratch.written().contains("食べる"), "nothing was written");

        // A new instance, as a restart would build.
        let next = Remembering::new(Counting::new());
        next.analyze("食べる").expect("analyze");
        assert_eq!(
            next.inner.count(),
            0,
            "the sidecar ran again after a restart"
        );
    }

    /// **The rule this whole design rests on.** A passage is held in memory and never
    /// reaches the file, so what is on disk is a vocabulary list and not a transcript.
    #[test]
    fn a_passage_is_never_written_down() {
        let _serial = alone();
        let scratch = Scratch::new("passage");

        let cache = Remembering::new(Counting::new());
        cache.analyze("水 を 飲む").expect("analyze");

        assert!(
            !scratch.written().contains("飲む"),
            "a multi-token analysis reached the file: {}",
            scratch.written()
        );

        // Still remembered *this* session, which is the other half of the bargain.
        cache.analyze("水 を 飲む").expect("analyze");
        assert_eq!(
            cache.inner.count(),
            1,
            "the passage should still be cached in memory"
        );
    }

    /// Even beside a word, which is the case that would leak: one write carries the whole
    /// cache, and the filter has to run over it rather than over the entry that triggered it.
    #[test]
    fn a_passage_does_not_ride_along_with_a_word() {
        let _serial = alone();
        let scratch = Scratch::new("mixed");

        let cache = Remembering::new(Counting::new());
        cache.analyze("秘密 の 文").expect("passage first");
        cache
            .analyze("猫")
            .expect("then a word, which triggers the write");

        let written = scratch.written();
        assert!(written.contains("猫"), "the word should be kept");
        assert!(
            !written.contains("秘密"),
            "the passage rode along: {written}"
        );
    }

    /// Answers from another analyzer are not this analyzer's answers. Ichiran and the
    /// built-in engine segment differently, so reading one back under the other would be a
    /// wrong answer delivered quickly.
    #[test]
    fn a_file_from_another_engine_is_ignored() {
        let _serial = alone();
        let scratch = Scratch::new("engine");

        std::fs::write(
            &scratch.0,
            format!(
                r#"{{"schema":{SCHEMA},"engine":"something-else","words":[["猫",{{"text":"猫","tokens":[{{"surface":"猫"}}]}}]]}}"#
            ),
        )
        .expect("write");

        let cache = Remembering::new(Counting::new());
        cache.analyze("猫").expect("analyze");
        assert_eq!(
            cache.inner.count(),
            1,
            "another engine's answer was trusted"
        );
    }

    /// **A token that has since grown a field is not this version's token.**
    ///
    /// Every field on `Token` is `#[serde(default)]`, so an older file loads clean and
    /// silently wrong: `confidence` and `romaji` come back `None`, the name rules keyed on
    /// them never fire, and because the analyzer is never asked again they never fire for
    /// those words — the ones already looked up, which is the worst possible set to lose.
    #[test]
    fn a_file_written_in_an_older_shape_is_ignored() {
        let _serial = alone();
        let scratch = Scratch::new("schema");

        // The engine matches; only the shape is old. Written without the key at all, which
        // is how every file from before it existed looks.
        std::fs::write(
            &scratch.0,
            r#"{"engine":"counting","words":[["猫",{"text":"猫","tokens":[{"surface":"猫"}]}]]}"#,
        )
        .expect("write");

        let cache = Remembering::new(Counting::new());
        cache.analyze("猫").expect("analyze");
        assert_eq!(
            cache.inner.count(),
            1,
            "an older shape was read back as current"
        );

        // And what it writes in its place is readable again.
        let next = Remembering::new(Counting::new());
        next.analyze("猫").expect("analyze");
        assert_eq!(
            next.inner.count(),
            0,
            "the rewritten file was not trusted either"
        );
    }

    /// Version 2 files deserialize after the role was added, but silently lose the role
    /// that the pronoun/suffix ranker needs. Re-analyse instead of trusting that answer.
    #[test]
    fn a_version_two_file_is_reanalysed_for_lexical_role() {
        let _serial = alone();
        let scratch = Scratch::new("schema-two");
        std::fs::write(
            &scratch.0,
            r#"{"schema":2,"engine":"counting","words":[["猫",{"text":"猫","tokens":[{"surface":"猫"}]}]]}"#,
        )
        .expect("write old cache");

        let cache = Remembering::new(Counting::new());
        cache.analyze("猫").expect("analyze");
        assert_eq!(
            cache.inner.count(),
            1,
            "a version two token lacks lexical role"
        );
        assert!(scratch.written().contains(&format!("\"schema\":{SCHEMA}")));
    }

    /// Version 3 files hold Ichiran ties broken by the bare word alone. The tie-break now reads
    /// the word in its sentence, and a remembered answer would keep the old choice for exactly
    /// the passages already read — so a version three file is re-analysed too.
    #[test]
    fn a_version_three_file_is_reanalysed_for_the_contextual_tie_break() {
        let _serial = alone();
        let scratch = Scratch::new("schema-three");
        std::fs::write(
            &scratch.0,
            r#"{"schema":3,"engine":"counting","words":[["猫",{"text":"猫","tokens":[{"surface":"猫"}]}]]}"#,
        )
        .expect("write old cache");

        let cache = Remembering::new(Counting::new());
        cache.analyze("猫").expect("analyze");
        assert_eq!(
            cache.inner.count(),
            1,
            "a version three answer may hold a tie broken without its sentence"
        );
        assert!(scratch.written().contains(&format!("\"schema\":{SCHEMA}")));
    }

    /// **The other half of that rule, and the one easier to break.** The gate is the file's schema
    /// version, not whether every token carries a role — and most tokens legitimately carry none.
    /// Lindera sets a role only for UniDic pronouns and suffixes, composition clears it, and the
    /// Ichiran path never sets one at all. Re-analysing whenever a role is absent would therefore
    /// re-analyse ordinary vocabulary on every read, which is the cache doing nothing at a
    /// sidecar's price.
    #[test]
    fn a_current_schema_token_without_a_role_is_still_served_from_the_file() {
        let _serial = alone();
        let scratch = Scratch::new("schema-current-roleless");
        std::fs::write(
            &scratch.0,
            format!(
                r#"{{"schema":{SCHEMA},"engine":"counting","words":[["猫",{{"text":"猫","tokens":[{{"surface":"猫"}}]}}]]}}"#
            ),
        )
        .expect("write current cache");

        let cache = Remembering::new(Counting::new());
        cache.analyze("猫").expect("analyze");
        assert_eq!(
            cache.inner.count(),
            0,
            "a roleless token in a current-schema file was re-analysed; the gate is the schema,              and an absent role is the ordinary case rather than a stale one"
        );
    }
    /// Turning it off erases what it had, which is the promise `PERSIST`'s doc comment makes and
    /// the reason this is a deletion rather than a stop.
    #[test]
    fn switching_it_off_deletes_the_file() {
        let _serial = alone();
        let scratch = Scratch::new("off");

        Remembering::new(Counting::new())
            .analyze("猫")
            .expect("analyze");
        assert!(scratch.0.exists(), "nothing to delete");

        set_persistence(None);
        assert!(!scratch.0.exists(), "the file survived being switched off");
    }

    /// With it off from the start, nothing is written at all.
    #[test]
    fn nothing_is_written_while_it_is_off() {
        let _serial = alone();
        set_persistence(None);
        let path =
            std::env::temp_dir().join(format!("sakatsu-words-never-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);

        Remembering::new(Counting::new())
            .analyze("猫")
            .expect("analyze");
        assert!(!path.exists());
    }
}
