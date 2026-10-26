//! Gates around engine work, for the few actions that must not overlap it: unloading a model, and
//! handing over to an update's installer.
//!
//! **A check followed by an action is a race.** "Is the translator busy? No — unload it" leaves a
//! gap in which a translation starts against a model being dropped. So an exclusive action
//! *closes* its gate first: from that moment no new work may enter. It then waits — briefly, and
//! no longer — for the work already inside to finish, and holds the gate closed for the whole of
//! the action. If the work does not finish in time, the gate reopens and the action reports
//! [`Busy`]; the app goes on exactly as before.
//!
//! Work enters through the scheduler: [`crate::work::Scheduler::enter`] takes an [`Entry`] for its
//! lane's gate when it admits a request, and the entry lives as long as the request's permit. So
//! every engine call — a capture, a translation, the OCR warm-up — holds its gate for its whole
//! run without each call site having to remember to.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// One engine's gate, or the store's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gate {
    /// Recognition, including the PaddleOCR-VL server.
    Recognition,
    Translation,
    /// Ichiran.
    Analysis,
    /// Writes to settings and word status.
    Store,
    Publication,
}

/// Every gate, for an action that must have the whole app quiet.
pub const ALL: [Gate; 5] = [
    Gate::Recognition,
    Gate::Translation,
    Gate::Analysis,
    Gate::Store,
    Gate::Publication,
];

/// Refused: an exclusive action is in progress, or the work it waited for did not finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Busy;

impl std::fmt::Display for Busy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "Sakatsu is busy unloading a model or preparing an update. Try again in a moment.",
        )
    }
}

impl std::error::Error for Busy {}

#[derive(Default, Debug)]
struct GateState {
    active: usize,
    closing: bool,
}

/// The gates and the work inside each.
#[derive(Default, Debug)]
pub struct Gates {
    state: Mutex<HashMap<Gate, GateState>>,
    changed: Condvar,
}

/// The process's gates. Global, like the scheduler that enters them.
pub fn gates() -> &'static Gates {
    static GATES: OnceLock<Gates> = OnceLock::new();
    GATES.get_or_init(Gates::default)
}

/// Work inside a gate. Leaving is dropping it.
#[derive(Debug)]
pub struct Entry<'a> {
    gates: &'a Gates,
    gate: Gate,
}

/// Gates held closed for an exclusive action. Dropping it reopens them.
#[derive(Debug)]
pub struct Exclusive<'a> {
    gates: &'a Gates,
    held: Vec<Gate>,
}

impl Gates {
    /// Enter `gate` for a piece of work, unless an exclusive action has closed it.
    pub fn enter(&self, gate: Gate) -> Result<Entry<'_>, Busy> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = state.entry(gate).or_default();
        if entry.closing {
            return Err(Busy);
        }
        entry.active += 1;
        Ok(Entry { gates: self, gate })
    }

    /// Close `held`, wait up to `wait` for the work inside to finish, and keep them closed.
    ///
    /// Refused at once if another exclusive action holds any of them. If the work inside does not
    /// finish in time, the gates reopen — nothing has been done yet — and this is [`Busy`].
    pub fn exclusive(&self, held: &[Gate], wait: Duration) -> Result<Exclusive<'_>, Busy> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if held
            .iter()
            .any(|gate| state.get(gate).is_some_and(|gate| gate.closing))
        {
            return Err(Busy);
        }
        for gate in held {
            state.entry(*gate).or_default().closing = true;
        }
        let deadline = Instant::now() + wait;
        loop {
            if held
                .iter()
                .all(|gate| state.get(gate).map_or(0, |gate| gate.active) == 0)
            {
                return Ok(Exclusive {
                    gates: self,
                    held: held.to_vec(),
                });
            }
            let now = Instant::now();
            if now >= deadline {
                for gate in held {
                    state.entry(*gate).or_default().closing = false;
                }
                self.changed.notify_all();
                return Err(Busy);
            }
            state = self
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }

    /// How much work is inside `gate` now. For tests and diagnostics.
    #[doc(hidden)]
    pub fn active(&self, gate: Gate) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&gate)
            .map_or(0, |gate| gate.active)
    }
}

impl Drop for Entry<'_> {
    fn drop(&mut self) {
        let mut state = self
            .gates
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = state.entry(self.gate).or_default();
        entry.active = entry.active.saturating_sub(1);
        self.gates.changed.notify_all();
    }
}

impl Drop for Exclusive<'_> {
    fn drop(&mut self) {
        let mut state = self
            .gates
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for gate in &self.held {
            state.entry(*gate).or_default().closing = false;
        }
        self.gates.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn a_closed_gate_refuses_new_work_and_an_open_one_admits_it() {
        let gates = Gates::default();
        let exclusive = gates
            .exclusive(&[Gate::Translation], Duration::ZERO)
            .expect("nothing inside");
        assert_eq!(gates.enter(Gate::Translation).unwrap_err(), Busy);
        assert!(
            gates.enter(Gate::Recognition).is_ok(),
            "other gates stay open"
        );
        drop(exclusive);
        assert!(
            gates.enter(Gate::Translation).is_ok(),
            "reopened when the action ends"
        );
    }

    #[test]
    fn work_that_does_not_finish_in_time_reopens_the_gate() {
        let gates = Gates::default();
        let inside = gates.enter(Gate::Translation).unwrap();
        assert_eq!(
            gates
                .exclusive(&[Gate::Translation], Duration::from_millis(50))
                .unwrap_err(),
            Busy
        );
        assert!(
            gates.enter(Gate::Translation).is_ok(),
            "the refusal left the gate open"
        );
        drop(inside);
    }

    #[test]
    fn an_exclusive_action_waits_for_the_work_inside_then_holds() {
        let gates: &'static Gates = Box::leak(Box::new(Gates::default()));
        let inside = gates.enter(Gate::Recognition).unwrap();
        let finished = Arc::new(AtomicBool::new(false));
        let worker = {
            let finished = Arc::clone(&finished);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                finished.store(true, Ordering::SeqCst);
                drop(inside);
            })
        };
        let exclusive = gates
            .exclusive(&[Gate::Recognition], Duration::from_secs(5))
            .expect("the work finishes in time");
        assert!(
            finished.load(Ordering::SeqCst),
            "it waited for the work to finish"
        );
        assert_eq!(gates.active(Gate::Recognition), 0);
        drop(exclusive);
        worker.join().unwrap();
    }

    /// **Never both.** Many workers entering while exclusive actions come and go: whenever an
    /// action holds the gate, nothing is inside it.
    #[test]
    fn work_and_an_exclusive_action_never_overlap() {
        let gates: &'static Gates = Box::leak(Box::new(Gates::default()));
        let inside = Arc::new(AtomicUsize::new(0));
        let violations = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let inside = Arc::clone(&inside);
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        if let Ok(entry) = gates.enter(Gate::Translation) {
                            inside.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_micros(200));
                            inside.fetch_sub(1, Ordering::SeqCst);
                            drop(entry);
                        }
                    }
                })
            })
            .collect();
        let mut held = 0;
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(500) {
            if let Ok(exclusive) = gates.exclusive(&[Gate::Translation], Duration::from_millis(20))
            {
                held += 1;
                for _ in 0..20 {
                    if inside.load(Ordering::SeqCst) != 0 {
                        violations.fetch_add(1, Ordering::SeqCst);
                    }
                    std::thread::yield_now();
                }
                drop(exclusive);
            }
        }
        stop.store(true, Ordering::SeqCst);
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(held > 0, "the action got the gate at least once");
        assert_eq!(
            violations.load(Ordering::SeqCst),
            0,
            "work ran inside a held gate"
        );
    }

    #[test]
    fn two_exclusive_actions_never_hold_the_same_gate() {
        let gates = Gates::default();
        let first = gates.exclusive(&ALL, Duration::ZERO).unwrap();
        assert_eq!(
            gates.exclusive(&[Gate::Store], Duration::ZERO).unwrap_err(),
            Busy
        );
        drop(first);
        assert!(gates.exclusive(&[Gate::Store], Duration::ZERO).is_ok());
    }
}
