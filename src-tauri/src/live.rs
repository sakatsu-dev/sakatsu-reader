//! State the app can replace while it is running.
//!
//! Four things used to be decided once, at startup, and held as plain values: the analyzer, the
//! imported dictionaries, the kanji table and the WordNet categories. Changing any of them meant
//! reopening the app, and the interface said so — which is a poor thing to say, because from the
//! reader's side "change this setting, then restart" and "this setting does not work" look
//! identical until you do as you are told.
//!
//! Nothing about those four actually required it. They are read, never written; every command
//! takes a reference and hands it back. That is exactly the shape a lock is cheap for, and
//! `FrequencyState` already proved it here — the frequency list was the first of these to be made
//! swappable, for the same reason and with the same result.
//!
//! # Why the value comes out rather than the work going in
//!
//! [`Live::now`] clones the `Arc` out from under a read lock and returns it. The obvious
//! alternative — hand back a guard, and let the caller work while holding the lock — is wrong
//! here, and expensively so: one `Analyzer::analyze` can be a `wsl -e ichiran-cli` spawn loading a
//! large Lisp image, seconds at a time. A swap arriving during one would block until it finished,
//! on the thread the interface is waiting on.
//!
//! Cloning an `Arc` is a relaxed atomic increment. Holding a lock across a subprocess is a stall.
//!
//! It also settles what happens to work already in flight, which a lock alone does not: an
//! analysis that started before the swap finishes against the analyzer it started with. It cannot
//! observe the engine changing halfway through, because it is holding the old one alive itself.
//! The next call gets the new one. There is no third state.

use std::sync::{Arc, RwLock};

/// Something the app reads constantly and replaces rarely.
///
/// `T: ?Sized` so this works for `dyn Analyzer` as well as for the concrete tables.
pub struct Live<T: ?Sized>(RwLock<Arc<T>>);

impl<T: ?Sized> Live<T> {
    /// Hold a value, ready to be replaced.
    pub fn new(value: Arc<T>) -> Self {
        Live(RwLock::new(value))
    }

    /// The value as it is right now.
    ///
    /// **Poisoning is recovered from rather than reported, and that is deliberate.** A poisoned
    /// lock means some other thread panicked while holding it; the guarantee `RwLock` withdraws
    /// is that the data may have been left half-written. Here it cannot have been. The only
    /// mutation in this module is [`Live::set`], which assigns a whole `Arc` — there is no
    /// partially-updated state for a panic to leave behind, so the warning does not apply and
    /// obeying it would turn one unrelated panic into an app that can no longer analyse anything.
    pub fn now(&self) -> Arc<T> {
        Arc::clone(&self.0.read().unwrap_or_else(|held| held.into_inner()))
    }

    /// Replace it. Whatever is mid-flight finishes against the old value.
    pub fn set(&self, value: Arc<T>) {
        *self.0.write().unwrap_or_else(|held| held.into_inner()) = value;
    }
}

impl<T: ?Sized> From<Arc<T>> for Live<T> {
    fn from(value: Arc<T>) -> Self {
        Live::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::Live;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    #[test]
    fn what_is_read_after_a_swap_is_the_new_value() {
        let live = Live::new(Arc::new(1_u32));
        assert_eq!(*live.now(), 1);
        live.set(Arc::new(2));
        assert_eq!(*live.now(), 2);
    }

    /// The reason `now` returns a value instead of a guard.
    ///
    /// A reader that takes its time must not hold a swap up, and a swap must not cut a reader
    /// off partway. Both hold here because the reader owns an `Arc` to the old value and keeps
    /// it alive for exactly as long as it is using it.
    #[test]
    fn a_slow_reader_neither_blocks_a_swap_nor_is_cut_off_by_one() {
        let live = Arc::new(Live::new(Arc::new("ichiran")));
        let held = live.now();

        let gate = Arc::new(Barrier::new(2));
        let swapper = {
            let live = Arc::clone(&live);
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                // If `now` returned a guard, this would sit here until `held` was dropped.
                live.set(Arc::new("lindera"));
                gate.wait();
            })
        };
        gate.wait();
        swapper.join().expect("the swap finished on its own");

        assert_eq!(
            *held, "ichiran",
            "work in flight kept the value it started with"
        );
        assert_eq!(
            *live.now(),
            "lindera",
            "and the next caller sees the new one"
        );
    }

    /// A panic somewhere else must not take the analyzer down with it.
    ///
    /// `RwLock` poisons on a panic anywhere in the process that touched it, and the standard
    /// answer — propagate the error — would mean one unrelated crash left the app unable to read
    /// its own analyzer. Nothing here can be left half-written, so there is nothing to protect.
    #[test]
    fn a_poisoned_lock_still_answers() {
        let live = Arc::new(Live::new(Arc::new(7_u32)));
        let poisoner = {
            let live = Arc::clone(&live);
            std::thread::spawn(move || {
                let _guard = live.0.write().expect("uncontended");
                panic!("something else went wrong");
            })
        };
        assert!(poisoner.join().is_err(), "the thread panicked as arranged");
        assert!(live.0.is_poisoned(), "which poisoned the lock");

        assert_eq!(*live.now(), 7, "and the value is still readable");
        live.set(Arc::new(8));
        assert_eq!(*live.now(), 8, "and still replaceable");
    }

    /// Many readers at once, which is why this is an `RwLock` and not a `Mutex`.
    ///
    /// **The guard has to be held across the rendezvous**, which is the whole difficulty. An
    /// earlier version called [`Live::now`] and then waited on a `Barrier` — but `now` clones the
    /// `Arc` out and drops the guard before returning, so every reader waited holding nothing.
    /// Swapping `RwLock` for `Mutex` left it green, so the one property it exists to protect was
    /// unguarded.
    ///
    /// It counts rather than using a `Barrier` so that exclusive reads *fail* instead of hanging
    /// forever: a deadlocked test tells you nothing until it is killed.
    #[test]
    fn readers_do_not_wait_for_each_other() {
        use std::time::{Duration, Instant};

        const READERS: usize = 8;
        let live = Arc::new(Live::new(Arc::new(0_u32)));
        let holding = Arc::new(AtomicUsize::new(0));

        let readers: Vec<_> = (0..READERS)
            .map(|_| {
                let live = Arc::clone(&live);
                let holding = Arc::clone(&holding);
                std::thread::spawn(move || {
                    let guard = live.0.read().unwrap_or_else(|p| p.into_inner());
                    holding.fetch_add(1, Ordering::SeqCst);
                    // Wait for the rest, still holding the read guard. Under a `Mutex` only one
                    // thread ever gets here, so the count stalls and the deadline decides it.
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while holding.load(Ordering::SeqCst) < READERS && Instant::now() < deadline {
                        std::thread::yield_now();
                    }
                    let together = holding.load(Ordering::SeqCst);
                    drop(guard);
                    together
                })
            })
            .collect();

        for reader in readers {
            assert_eq!(
                reader.join().expect("no reader panicked"),
                READERS,
                "every reader must hold its read guard at the same time; if reads were                  exclusive this is where it shows"
            );
        }
    }

    /// `dyn Trait`, which is the case `AnalyzerState` needs and the reason for `?Sized`.
    #[test]
    fn it_holds_a_trait_object() {
        trait Engine: Send + Sync {
            fn name(&self) -> &str;
        }
        struct A;
        struct B;
        impl Engine for A {
            fn name(&self) -> &str {
                "a"
            }
        }
        impl Engine for B {
            fn name(&self) -> &str {
                "b"
            }
        }

        let live: Live<dyn Engine> = Live::new(Arc::new(A));
        assert_eq!(live.now().name(), "a");
        live.set(Arc::new(B));
        assert_eq!(live.now().name(), "b");
    }
}
