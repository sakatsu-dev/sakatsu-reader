//! Bounded, side-effect-free answers prepared from the learner's immediate intent.
//!
//! This is deliberately a result cache, not a hidden second analyzer or dictionary. A foreground
//! request and a prefetch use the same key and the same computation, so whichever arrives second
//! joins or reuses the first. Nothing here selects a word, writes learning state, starts audio, or
//! survives the process.

use std::collections::{HashSet, VecDeque};
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Instant;

use sakatsu_core::dict::ContextualWordView;
use sakatsu_core::model::Token;
use serde::Serialize;

use crate::commands::Found;

const REFERENCE_LIMIT: usize = 48;
const CONTEXT_LIMIT: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Foreground,
    Prefetch,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WarmupDiagnostics {
    pub reference_entries: usize,
    pub analysis_entries: usize,
    pub reference_hits: u64,
    pub reference_misses: u64,
    pub analysis_hits: u64,
    pub analysis_misses: u64,
    pub reference_prefetches: u64,
    pub analysis_prefetches: u64,
    /// Sum of the original computation time for foreground cache hits. It is an estimate of
    /// avoided work rather than wall-clock time saved: concurrent activity can change latency.
    pub estimated_saved_ms: f64,
    pub invalidations: u64,
}

#[derive(Clone)]
struct Stored<V> {
    value: V,
    compute_micros: u64,
}

struct Lru<K, V> {
    limit: usize,
    entries: VecDeque<(K, Stored<V>)>,
}

impl<K: PartialEq, V: Clone> Lru<K, V> {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            entries: VecDeque::new(),
        }
    }

    fn get(&mut self, key: &K) -> Option<Stored<V>> {
        let at = self.entries.iter().position(|(had, _)| had == key)?;
        let entry = self.entries.remove(at)?;
        let answer = entry.1.clone();
        self.entries.push_back(entry);
        Some(answer)
    }

    fn insert(&mut self, key: K, value: V, compute_micros: u64) {
        if let Some(at) = self.entries.iter().position(|(had, _)| had == &key) {
            self.entries.remove(at);
        }
        if self.entries.len() >= self.limit {
            self.entries.pop_front();
        }
        self.entries.push_back((
            key,
            Stored {
                value,
                compute_micros,
            },
        ));
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReferenceKey {
    epoch: u64,
    query: String,
}

/// The passage a contextual answer was computed against, as a digest.
///
/// **Built once per request, not once per token.** The key used to be
/// `serde_json::to_string(&(tokens, index))` - the whole token array, senses, components and
/// alternatives included, re-serialised on every single lookup. Selecting one word in a long
/// passage does that five times (the foreground call plus four prefetched neighbours), on every
/// arrow-key press; and [`Lru`] then kept up to `CONTEXT_LIMIT` of those strings and compared
/// them byte-for-byte on a linear scan, which for one passage could retain tens of megabytes.
///
/// SHA-256 rather than a 64-bit hash because a collision here would not lose a cache entry, it
/// would silently answer with **another word's definition**. At 32 bytes the whole table is 3 KB.
///
/// The digest still covers the entire passage, so this is exactly as specific as the string it
/// replaces: the same tokens give the same key, and an edit anywhere gives a different one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PassageDigest([u8; 32]);

impl PassageDigest {
    pub fn of(tokens: &[Token]) -> Result<Self, String> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(tokens)
            .map_err(|error| format!("could not key contextual lookup: {error}"))?;
        Ok(PassageDigest(Sha256::digest(bytes).into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ContextKey {
    epoch: u64,
    passage: PassageDigest,
    index: usize,
}

#[derive(Default)]
struct Metrics {
    reference_hits: u64,
    reference_misses: u64,
    analysis_hits: u64,
    analysis_misses: u64,
    reference_prefetches: u64,
    analysis_prefetches: u64,
    estimated_saved_micros: u64,
    invalidations: u64,
}

struct Cache {
    reference: Lru<ReferenceKey, Found>,
    contextual: Lru<ContextKey, ContextualWordView>,
    reference_in_flight: HashSet<ReferenceKey>,
    contextual_in_flight: HashSet<ContextKey>,
    metrics: Metrics,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            reference: Lru::new(REFERENCE_LIMIT),
            contextual: Lru::new(CONTEXT_LIMIT),
            reference_in_flight: HashSet::new(),
            contextual_in_flight: HashSet::new(),
            metrics: Metrics::default(),
        }
    }
}

/// One process-wide warmup coordinator.
pub struct WarmupState {
    epoch: AtomicU64,
    cache: Mutex<Cache>,
    ready: Condvar,
    prefetch_gate: Mutex<()>,
    reference_intent: AtomicU64,
    analysis_intent: AtomicU64,
}

impl Default for WarmupState {
    fn default() -> Self {
        Self {
            epoch: AtomicU64::new(0),
            cache: Mutex::new(Cache::default()),
            ready: Condvar::new(),
            prefetch_gate: Mutex::new(()),
            reference_intent: AtomicU64::new(0),
            analysis_intent: AtomicU64::new(0),
        }
    }
}

impl WarmupState {
    fn cache(&self) -> MutexGuard<'_, Cache> {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Only speculative work takes this gate. Foreground work never queues behind an unrelated
    /// preload, while two speculative surfaces cannot scatter reads across the drive together.
    pub fn prefetch_guard(&self) -> MutexGuard<'_, ()> {
        self.prefetch_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn begin_reference_intent(&self) -> u64 {
        self.reference_intent.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn reference_intent_is_current(&self, intent: u64) -> bool {
        self.reference_intent.load(Ordering::Acquire) == intent
    }

    pub fn begin_analysis_intent(&self) -> u64 {
        self.analysis_intent.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn analysis_intent_is_current(&self, intent: u64) -> bool {
        self.analysis_intent.load(Ordering::Acquire) == intent
    }

    /// Forget answers derived from replaceable dictionaries, analyzer, kanji or frequency data.
    /// Work already holding the old `Arc` may finish for its caller, but its old epoch cannot be
    /// inserted into the new cache.
    pub fn invalidate(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        let mut cache = self.cache();
        cache.reference.clear();
        cache.contextual.clear();
        cache.metrics.invalidations += 1;
        self.ready.notify_all();
    }

    pub fn diagnostics(&self) -> WarmupDiagnostics {
        let cache = self.cache();
        WarmupDiagnostics {
            reference_entries: cache.reference.len(),
            analysis_entries: cache.contextual.len(),
            reference_hits: cache.metrics.reference_hits,
            reference_misses: cache.metrics.reference_misses,
            analysis_hits: cache.metrics.analysis_hits,
            analysis_misses: cache.metrics.analysis_misses,
            reference_prefetches: cache.metrics.reference_prefetches,
            analysis_prefetches: cache.metrics.analysis_prefetches,
            estimated_saved_ms: cache.metrics.estimated_saved_micros as f64 / 1_000.0,
            invalidations: cache.metrics.invalidations,
        }
    }

    pub fn reference<F>(&self, query: &str, access: Access, compute: F) -> Result<Found, String>
    where
        F: FnOnce() -> Result<Found, String>,
    {
        let key = ReferenceKey {
            epoch: self.epoch.load(Ordering::Acquire),
            query: query.to_string(),
        };
        let mut compute = Some(compute);
        loop {
            let mut cache = self.cache();
            if let Some(stored) = cache.reference.get(&key) {
                if access == Access::Foreground {
                    cache.metrics.reference_hits += 1;
                    cache.metrics.estimated_saved_micros = cache
                        .metrics
                        .estimated_saved_micros
                        .saturating_add(stored.compute_micros);
                }
                return Ok(stored.value);
            }
            if cache.reference_in_flight.insert(key.clone()) {
                if access == Access::Foreground {
                    cache.metrics.reference_misses += 1;
                }
                break;
            }
            drop(
                self.ready
                    .wait(cache)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        }

        let started = Instant::now();
        let outcome = compute.take().expect("the cache miss computes once")();
        let elapsed = micros(started);
        let mut cache = self.cache();
        cache.reference_in_flight.remove(&key);
        if let Ok(answer) = &outcome {
            if key.epoch == self.epoch.load(Ordering::Acquire) {
                cache.reference.insert(key, answer.clone(), elapsed);
                if access == Access::Prefetch {
                    cache.metrics.reference_prefetches += 1;
                }
            }
        }
        self.ready.notify_all();
        outcome
    }

    /// `passage` is [`PassageDigest::of`] the same tokens, taken once by the caller so that a
    /// loop over many indices does not digest the whole passage again for each one.
    pub fn contextual<F>(
        &self,
        passage: &PassageDigest,
        index: usize,
        access: Access,
        compute: F,
    ) -> Result<ContextualWordView, String>
    where
        F: FnOnce() -> Result<ContextualWordView, String>,
    {
        let key = ContextKey {
            epoch: self.epoch.load(Ordering::Acquire),
            passage: passage.clone(),
            index,
        };
        let mut compute = Some(compute);
        loop {
            let mut cache = self.cache();
            if let Some(stored) = cache.contextual.get(&key) {
                if access == Access::Foreground {
                    cache.metrics.analysis_hits += 1;
                    cache.metrics.estimated_saved_micros = cache
                        .metrics
                        .estimated_saved_micros
                        .saturating_add(stored.compute_micros);
                }
                return Ok(stored.value);
            }
            if cache.contextual_in_flight.insert(key.clone()) {
                if access == Access::Foreground {
                    cache.metrics.analysis_misses += 1;
                }
                break;
            }
            drop(
                self.ready
                    .wait(cache)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        }

        let started = Instant::now();
        let outcome = compute.take().expect("the cache miss computes once")();
        let elapsed = micros(started);
        let mut cache = self.cache();
        cache.contextual_in_flight.remove(&key);
        if let Ok(answer) = &outcome {
            if key.epoch == self.epoch.load(Ordering::Acquire) {
                cache.contextual.insert(key, answer.clone(), elapsed);
                if access == Access::Prefetch {
                    cache.metrics.analysis_prefetches += 1;
                }
            }
        }
        self.ready.notify_all();
        outcome
    }
}

fn micros(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sakatsu_core::dict::{Retrieved, VocabularyRanking, WordView};
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Barrier};

    fn found() -> Found {
        Found {
            words: Vec::new(),
            japanese: true,
            ranked_by: None,
            needs_gloss_index: false,
            capped: false,
        }
    }

    fn contextual() -> ContextualWordView {
        ContextualWordView {
            word: WordView {
                headword: "水".into(),
                reading: Some("みず".into()),
                inline: Vec::new(),
                inline_source: None,
                inline_retrieved: Retrieved::Unknown,
                also_in: Vec::new(),
                examples: Vec::new(),
                name: None,
                likely_name: false,
                romaji: None,
            },
            ranking: VocabularyRanking {
                senses: Vec::new(),
                favoured: None,
            },
        }
    }

    #[test]
    fn an_exact_reference_question_computes_once() {
        let state = WarmupState::default();
        let calls = AtomicUsize::new(0);
        for _ in 0..2 {
            state
                .reference("水", Access::Foreground, || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Ok(found())
                })
                .expect("answer");
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let stats = state.diagnostics();
        assert_eq!((stats.reference_misses, stats.reference_hits), (1, 1));
    }

    #[test]
    fn simultaneous_misses_share_one_computation() {
        let state = Arc::new(WarmupState::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker = {
            let state = Arc::clone(&state);
            let calls = Arc::clone(&calls);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            std::thread::spawn(move || {
                state.reference("猫", Access::Prefetch, || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    entered.wait();
                    release.wait();
                    Ok(found())
                })
            })
        };
        entered.wait();
        let waiter = {
            let state = Arc::clone(&state);
            let calls = Arc::clone(&calls);
            std::thread::spawn(move || {
                state.reference("猫", Access::Foreground, || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Ok(found())
                })
            })
        };
        release.wait();
        worker.join().expect("worker").expect("prefetch");
        waiter.join().expect("waiter").expect("foreground");
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(state.diagnostics().reference_hits, 1);
    }

    #[test]
    fn failures_are_retryable_and_never_cached() {
        let state = WarmupState::default();
        assert!(state
            .reference("失敗", Access::Prefetch, || Err("offline".into()))
            .is_err());
        state
            .reference("失敗", Access::Foreground, || Ok(found()))
            .expect("retry");
        assert_eq!(state.diagnostics().reference_entries, 1);
    }

    #[test]
    fn invalidation_removes_answers_from_both_surfaces() {
        let state = WarmupState::default();
        let tokens = vec![Token {
            surface: "水".into(),
            dict_form: "水".into(),
            ..Token::default()
        }];
        state
            .reference("水", Access::Prefetch, || Ok(found()))
            .expect("reference");
        state
            .contextual(
                &PassageDigest::of(&tokens).expect("tokens serialise"),
                0,
                Access::Prefetch,
                || Ok(contextual()),
            )
            .expect("context");
        state.invalidate();
        let stats = state.diagnostics();
        assert_eq!((stats.reference_entries, stats.analysis_entries), (0, 0));
        assert_eq!(stats.invalidations, 1);
    }

    #[test]
    fn a_new_intent_supersedes_queued_speculation() {
        let state = WarmupState::default();
        let first = state.begin_analysis_intent();
        let second = state.begin_analysis_intent();
        assert!(!state.analysis_intent_is_current(first));
        assert!(state.analysis_intent_is_current(second));
    }

    #[test]
    fn caches_are_bounded() {
        let state = WarmupState::default();
        for n in 0..(REFERENCE_LIMIT + 7) {
            state
                .reference(&format!("word-{n}"), Access::Prefetch, || Ok(found()))
                .expect("answer");
        }
        assert_eq!(state.diagnostics().reference_entries, REFERENCE_LIMIT);
    }
}
