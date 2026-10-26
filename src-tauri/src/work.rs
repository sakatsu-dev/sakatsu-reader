//! Scheduling and ownership for native work that is expensive enough to queue.
//!
//! Each engine has its own lane. A request becomes current before it joins a lane; a newer request
//! from the same window and operation makes an older queued request ineligible. Running native
//! inference is allowed to finish because most engines cannot be interrupted safely, but callers
//! can check [`Permit::is_current`] between stages and must retain their frontend stale-result
//! check before painting the answer.

use sakatsu_core::request::{RequestIdentity, WorkPriority};
use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lane {
    Ocr,
    Translation,
    Vlm,
    Ichiran,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Operation {
    owner: String,
    name: String,
}

impl From<&RequestIdentity> for Operation {
    fn from(request: &RequestIdentity) -> Self {
        Self {
            owner: request.owner.clone(),
            name: request.operation.clone(),
        }
    }
}

#[derive(Debug)]
struct Waiting {
    ticket: u64,
    request: Option<RequestIdentity>,
    priority: WorkPriority,
}

#[derive(Default, Debug)]
struct LaneState {
    active: usize,
    waiting: Vec<Waiting>,
}

#[derive(Default, Debug)]
struct State {
    next_ticket: u64,
    lanes: HashMap<Lane, LaneState>,
    latest: HashMap<Operation, u64>,
    cancelled: HashSet<(Operation, u64)>,
}

#[derive(Default, Debug)]
pub struct Scheduler {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("request cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// Why a lane did not admit a request.
///
/// **Two causes, kept apart, because the interface treats them differently.** "request cancelled"
/// means a newer request replaced this one, and the interface drops it without a word. A request
/// refused because a model is being unloaded or an update prepared was replaced by nothing: it
/// has to be shown, or the reader's action simply vanishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Cancelled,
    Busy,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Cancelled => Cancelled.fmt(formatter),
            Refused::Busy => crate::lifecycle::Busy.fmt(formatter),
        }
    }
}

impl std::error::Error for Refused {}

impl From<Cancelled> for Refused {
    fn from(_: Cancelled) -> Self {
        Refused::Cancelled
    }
}

/// The lifecycle gate a lane's work holds while it runs.
fn gate(lane: Lane) -> crate::lifecycle::Gate {
    match lane {
        Lane::Ocr | Lane::Vlm => crate::lifecycle::Gate::Recognition,
        Lane::Translation => crate::lifecycle::Gate::Translation,
        Lane::Ichiran => crate::lifecycle::Gate::Analysis,
    }
}

impl Scheduler {
    /// Start owned work, optionally through an engine lane.
    ///
    /// Passing no lane retains cancellation identity without serializing cheap in-process work.
    pub fn start(
        &self,
        lane: Option<Lane>,
        request: Option<RequestIdentity>,
    ) -> Result<Work<'_>, Refused> {
        match lane {
            Some(lane) => self.enter(lane, request).map(Work::Scheduled),
            None => self.own(request).map(Work::Owned),
        }
    }

    /// Register work that needs cancellation ownership but no engine lane.
    ///
    /// Cheap dictionary and in-process grammar work must not wait behind inference, but it still
    /// needs to refuse an answer after its owning selection has changed.
    pub fn own(&self, request: Option<RequestIdentity>) -> Result<Ownership<'_>, Refused> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        register(&mut state, request.as_ref());
        let entry = crate::lifecycle::gates()
            .enter(crate::lifecycle::Gate::Analysis)
            .map_err(|_| Refused::Busy)?;
        is_current(&state, request.as_ref())
            .then_some(Ownership {
                scheduler: self,
                request,
                _entry: entry,
            })
            .ok_or(Refused::Cancelled)
    }

    /// Register and wait for one lane. Registration happens before queueing, so a newly submitted
    /// request can invalidate obsolete queued work even while another engine invocation is active.
    pub fn enter(
        &self,
        lane: Lane,
        request: Option<RequestIdentity>,
    ) -> Result<Permit<'_>, Refused> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        register(&mut state, request.as_ref());

        let ticket = state.next_ticket;
        state.next_ticket = state.next_ticket.wrapping_add(1);
        let priority = request
            .as_ref()
            .map(|request| request.priority)
            .unwrap_or(WorkPriority::Foreground);
        state.lanes.entry(lane).or_default().waiting.push(Waiting {
            ticket,
            request: request.clone(),
            priority,
        });
        self.changed.notify_all();

        loop {
            if !is_current(&state, request.as_ref()) {
                remove_waiter(&mut state, lane, ticket);
                self.changed.notify_all();
                return Err(Refused::Cancelled);
            }

            let available = state.lanes.entry(lane).or_default().active < capacity(lane);
            let next = next_eligible(&state, lane);
            if available && next == Some(ticket) {
                // Admitted: enter the engine's gate for as long as the permit lives. A closed
                // gate means an unload or an update holds the engine; the request is refused
                // rather than run against it, and its place in the lane goes to the next one.
                let Ok(entry) = crate::lifecycle::gates().enter(gate(lane)) else {
                    remove_waiter(&mut state, lane, ticket);
                    self.changed.notify_all();
                    return Err(Refused::Busy);
                };
                let lane_state = state.lanes.entry(lane).or_default();
                lane_state
                    .waiting
                    .retain(|waiting| waiting.ticket != ticket);
                lane_state.active += 1;
                return Ok(Permit {
                    scheduler: self,
                    lane,
                    request,
                    _entry: entry,
                });
            }

            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// Cancel this subscriber. Other owners waiting on the same cached computation remain current.
    pub fn cancel(&self, request: &RequestIdentity) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state
            .cancelled
            .insert((Operation::from(request), request.generation));
        self.changed.notify_all();
    }

    /// Queue depth for diagnostics and deterministic scheduler tests.
    #[doc(hidden)]
    pub fn pending(&self, lane: Lane) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .lanes
            .get(&lane)
            .map_or(0, |lane| lane.waiting.len())
    }

    /// Whether an identified request still owns its operation.
    #[doc(hidden)]
    pub fn is_current_request(&self, request: &RequestIdentity) -> bool {
        self.current(Some(request))
    }

    /// Run `publish` only if `request` still owns its operation, **under the scheduler's lock**.
    ///
    /// Not `is_current_request` followed by a write: between the two a newer request could
    /// register, and the older result would then be published over a selection that has already
    /// moved on. Registration takes this same lock, so holding it across the check and the write
    /// makes them one step. `publish` must not call back into the scheduler.
    ///
    /// A result with no identity is published: nothing can supersede what was never identified.
    pub fn publish_if_current(
        &self,
        request: Option<&RequestIdentity>,
        publish: impl FnOnce(),
    ) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !is_current(&state, request) {
            return false;
        }
        publish();
        true
    }

    fn current(&self, request: Option<&RequestIdentity>) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        is_current(&state, request)
    }
}

fn register(state: &mut State, request: Option<&RequestIdentity>) {
    let Some(identity) = request else { return };
    let operation = Operation::from(identity);
    let current = state.latest.entry(operation.clone()).or_default();
    *current = (*current).max(identity.generation);
    let current = *current;
    state
        .cancelled
        .retain(|(candidate, generation)| candidate != &operation || *generation >= current);
}

fn priority(priority: WorkPriority) -> u8 {
    match priority {
        WorkPriority::Foreground => 0,
        WorkPriority::Speculative => 1,
    }
}

fn capacity(lane: Lane) -> usize {
    match lane {
        // FuguMT already has sixteen exact-input miss gates so unrelated sentences can decode in
        // parallel. Preserve that deliberate behavior while bounding the number admitted.
        Lane::Translation => 16,
        Lane::Ocr | Lane::Vlm | Lane::Ichiran => 1,
    }
}

fn next_eligible(state: &State, lane: Lane) -> Option<u64> {
    state
        .lanes
        .get(&lane)?
        .waiting
        .iter()
        .filter(|waiting| is_current(state, waiting.request.as_ref()))
        .min_by_key(|waiting| (priority(waiting.priority), waiting.ticket))
        .map(|waiting| waiting.ticket)
}

fn remove_waiter(state: &mut State, lane: Lane, ticket: u64) {
    if let Some(lane) = state.lanes.get_mut(&lane) {
        lane.waiting.retain(|waiting| waiting.ticket != ticket);
    }
}

fn is_current(state: &State, request: Option<&RequestIdentity>) -> bool {
    let Some(request) = request else {
        return true;
    };
    let operation = Operation::from(request);
    state.latest.get(&operation) == Some(&request.generation)
        && !state.cancelled.contains(&(operation, request.generation))
}

pub struct Permit<'a> {
    scheduler: &'a Scheduler,
    lane: Lane,
    request: Option<RequestIdentity>,
    /// The engine's lifecycle gate, held for as long as the work runs.
    _entry: crate::lifecycle::Entry<'static>,
}

pub struct Ownership<'a> {
    scheduler: &'a Scheduler,
    request: Option<RequestIdentity>,
    _entry: crate::lifecycle::Entry<'static>,
}

pub enum Work<'a> {
    Owned(Ownership<'a>),
    Scheduled(Permit<'a>),
}

impl Work<'_> {
    pub fn checkpoint(&self) -> Result<(), Cancelled> {
        match self {
            Self::Owned(ownership) => ownership.checkpoint(),
            Self::Scheduled(permit) => permit.checkpoint(),
        }
    }
}

impl Ownership<'_> {
    pub fn checkpoint(&self) -> Result<(), Cancelled> {
        self.scheduler
            .current(self.request.as_ref())
            .then_some(())
            .ok_or(Cancelled)
    }
}

impl Permit<'_> {
    pub fn is_current(&self) -> bool {
        self.scheduler.current(self.request.as_ref())
    }

    pub fn checkpoint(&self) -> Result<(), Cancelled> {
        self.is_current().then_some(()).ok_or(Cancelled)
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut state = self
            .scheduler
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let active = &mut state.lanes.entry(self.lane).or_default().active;
        *active = active.saturating_sub(1);
        self.scheduler.changed.notify_all();
    }
}

pub fn scheduler() -> &'static Scheduler {
    static SCHEDULER: OnceLock<Scheduler> = OnceLock::new();
    SCHEDULER.get_or_init(Scheduler::default)
}
