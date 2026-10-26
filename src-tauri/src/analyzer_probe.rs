//! Cancellation belongs to background readiness work, never to the foreground analyzer.
//! Installation suspends registration before draining gates, closing the cancel/start race.
use crate::lifecycle::{gates, Entry, Gate};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Default)]
struct State {
    suspended: bool,
    active: Option<Arc<AtomicBool>>,
}
static STATE: Mutex<State> = Mutex::new(State {
    suspended: false,
    active: None,
});

/// One background check, including its analysis lifecycle entry.
pub struct Probe {
    cancelled: Arc<AtomicBool>,
    _entry: Entry<'static>,
}

impl Probe {
    pub fn start() -> Option<Self> {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if state.suspended || state.active.is_some() {
            return None;
        }
        let entry = gates().enter(Gate::Analysis).ok()?;
        let cancelled = Arc::new(AtomicBool::new(false));
        state.active = Some(cancelled.clone());
        Some(Self {
            cancelled,
            _entry: entry,
        })
    }

    pub fn cancellation(&self) -> &AtomicBool {
        &self.cancelled
    }

    /// Publish under the same lock that cancels the check, so a late answer cannot change route.
    /// The callback must only publish state, without starting work or entering lifecycle gates.
    pub fn publish<T>(&self, publish: impl FnOnce() -> T) -> Option<T> {
        let _state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if self.cancelled.load(Ordering::Acquire) {
            None
        } else {
            Some(publish())
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if state
            .active
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, &self.cancelled))
        {
            state.active = None;
        }
    }
}

/// Stops new probes and asks the current private check to exit. Dropping this permits retries.
pub struct Suspended {
    _private: (),
}
pub fn suspend() -> Result<Suspended, crate::lifecycle::Busy> {
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if state.suspended {
        return Err(crate::lifecycle::Busy);
    }
    state.suspended = true;
    if let Some(active) = &state.active {
        active.store(true, Ordering::Release);
    }
    Ok(Suspended { _private: () })
}
impl Drop for Suspended {
    fn drop(&mut self) {
        STATE.lock().unwrap_or_else(|e| e.into_inner()).suspended = false;
    }
}
