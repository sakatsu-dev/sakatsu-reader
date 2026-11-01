//! **Which analyzer a passage runs on, as one snapshot.**
//!
//! A passage's analysis depends on five facts at once: the policy in force (the saved choice,
//! resolved with the `SAKATSU_ANALYZER` override), the engine it prefers, the engine it may fall
//! back to, whether that preferred engine is ready — still being probed at startup, or unavailable
//! and why — and which selection all of this belongs to. Held as three separately changing pieces
//! (a saved preference, a live analyzer and a global "probing" flag) they can disagree mid-call,
//! and did: the startup probe compared engine *names* to decide whether the reader had chosen
//! something while it ran, which cannot see a choice of the same engine, or a choice and a
//! change back.
//!
//! So a [`Route`] carries all five together, [`AnalyzerState`] replaces it whole, and every
//! publication carries a **generation**. A caller reads one snapshot for the whole passage; the
//! startup probe and the re-probe publish only by compare-and-set on the generation they started
//! from, so anything that happened in between — a user's selection above all — wins.

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::model::{AnalyzerPolicy, Fallback, FallbackReason, FallbackScope};
use sakatsu_core::store::AnalyzerChoice;
use sakatsu_core::study::Plan;

/// Whether the policy's preferred engine is what a passage will run on.
#[derive(Debug, Clone)]
pub enum Readiness {
    /// The preferred engine is live — or the policy is the built-in analyzer, which always is.
    Ready,
    /// Preferred engine is selected but starts on first use, without a startup check.
    Deferred,
    /// Ichiran is being checked at startup; the built-in analyzer stands in meanwhile.
    Probing,
    /// Ichiran did not answer the last probe. It is probed again, at most once per
    /// [`REPROBE_AFTER`], when the reader signals a read is coming.
    Unavailable { reason: String, since: Instant },
}

/// How long an unavailable Ichiran is left alone before a warm-up intent probes it again.
pub const REPROBE_AFTER: Duration = Duration::from_secs(60);

/// Everything one passage's analysis depends on, together.
#[derive(Clone)]
pub struct Route {
    pub policy: AnalyzerPolicy,
    pub primary: Arc<dyn Analyzer>,
    /// The built-in analyzer, when the primary is not already it. `None` means a failure of the
    /// primary is final: a failed Lindera is not retried on itself.
    pub fallback: Option<Arc<dyn Analyzer>>,
    pub readiness: Readiness,
    /// Set by [`AnalyzerState`] on publication; whatever a caller puts here is overwritten.
    pub generation: u64,
}

impl Route {
    /// The plan core runs a passage on — built from this one snapshot, so its parts agree.
    pub fn plan(&self) -> Plan<'_> {
        Plan {
            requested: self.policy,
            primary: self.primary.as_ref(),
            fallback: self.fallback.as_deref(),
            standing_in: self.standing_in(),
        }
    }

    /// Why the primary is standing in for the preferred engine, when it is.
    ///
    /// **Recorded whenever it is true, whatever the presentation.** Under Automatic, a session
    /// with no Ichiran shows only a quiet label — but the passage still says, in its execution
    /// record, that it was the built-in analyzer standing in and why.
    pub fn standing_in(&self) -> Option<Fallback> {
        if self.policy == AnalyzerPolicy::BuiltIn {
            return None;
        }
        match &self.readiness {
            Readiness::Ready | Readiness::Deferred => None,
            Readiness::Probing => Some(Fallback {
                reason: FallbackReason::Starting,
                scope: FallbackScope::Session,
                detail: "Ichiran is still being checked; the built-in analyzer read this passage"
                    .into(),
            }),
            Readiness::Unavailable { reason, .. } => Some(Fallback {
                reason: FallbackReason::Unavailable,
                scope: FallbackScope::Session,
                detail: reason.clone(),
            }),
        }
    }

    /// Whether an unavailable Ichiran is due to be probed again.
    pub fn reprobe_due(&self, now: Instant) -> bool {
        self.policy != AnalyzerPolicy::BuiltIn
            && match &self.readiness {
                // An install attempt may cancel startup. The next reading intent retries it;
                // the probe owner prevents duplicate checks while one is still running.
                Readiness::Probing => true,
                Readiness::Unavailable { since, .. } => {
                    now.saturating_duration_since(*since) >= REPROBE_AFTER
                }
                Readiness::Ready | Readiness::Deferred => false,
            }
    }
}

/// The policy actually in force: the saved choice, overridden by `SAKATSU_ANALYZER` when set.
///
/// `IchiMoe` survives only so an old settings file still parses; `Settings::normalise` turns it
/// into `Auto` before this is called, and it is treated as Automatic here too rather than trusted.
pub fn effective_policy(saved: AnalyzerChoice) -> AnalyzerPolicy {
    let preference = std::env::var("SAKATSU_ANALYZER").unwrap_or_default();
    if preference.eq_ignore_ascii_case("lindera") {
        return AnalyzerPolicy::BuiltIn;
    }
    if preference.eq_ignore_ascii_case("ichiran") {
        return AnalyzerPolicy::Ichiran;
    }
    match saved {
        AnalyzerChoice::BuiltIn => AnalyzerPolicy::BuiltIn,
        AnalyzerChoice::Ichiran => AnalyzerPolicy::Ichiran,
        AnalyzerChoice::Auto | AnalyzerChoice::IchiMoe => AnalyzerPolicy::Automatic,
    }
}

/// The analyzer route the app runs on, replaced whole.
///
/// Behind a lock because the choice is a *setting*, and a setting that only takes effect next
/// launch is indistinguishable from one that does not work. The route is cloned out rather than
/// analysed under the lock — with Ichiran one analysis can take seconds, and a swap must not have
/// to queue behind it.
pub struct AnalyzerState {
    route: RwLock<Arc<Route>>,
    /// The last analysis this state answered, for the status display.
    last: crate::runtime_status::LastResult,
}

impl AnalyzerState {
    /// Hold `route` as generation 0.
    pub fn new(mut route: Route) -> Self {
        route.generation = 0;
        AnalyzerState {
            route: RwLock::new(Arc::new(route)),
            last: Default::default(),
        }
    }

    /// One engine and nothing to fall back to: what the tests and the setup harness register.
    ///
    /// The policy follows the engine, so a fixed Lindera is the built-in policy (and records no
    /// fallback) while any other engine is asked for by name.
    pub fn fixed(analyzer: Arc<dyn Analyzer>) -> Self {
        let policy = if analyzer.engine() == "lindera" {
            AnalyzerPolicy::BuiltIn
        } else {
            AnalyzerPolicy::Ichiran
        };
        AnalyzerState::new(Route {
            policy,
            primary: analyzer,
            fallback: None,
            readiness: Readiness::Ready,
            generation: 0,
        })
    }

    /// The last analysis this state answered.
    pub fn last(&self) -> &crate::runtime_status::LastResult {
        &self.last
    }

    /// The whole route, as one snapshot. Read once per passage.
    pub fn route(&self) -> Arc<Route> {
        self.route
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// The primary engine alone, for callers that only need an analyzer.
    pub fn now(&self) -> Arc<dyn Analyzer> {
        self.route().primary.clone()
    }

    /// Publish `route` unconditionally — the reader's own selection. Returns its generation.
    pub fn publish(&self, mut route: Route) -> u64 {
        let mut slot = self
            .route
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        route.generation = slot.generation + 1;
        let generation = route.generation;
        *slot = Arc::new(route);
        generation
    }

    /// Publish `route` **only if nothing was published since `generation`**.
    ///
    /// What the startup probe and the re-probe use: they read the generation before probing, and
    /// a selection made while they were probing has moved it on, so their result is dropped rather
    /// than laid over the reader's choice. Returns whether it was published.
    pub fn publish_if(&self, generation: u64, mut route: Route) -> bool {
        let mut slot = self
            .route
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.generation != generation {
            return false;
        }
        route.generation = generation + 1;
        *slot = Arc::new(route);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sakatsu_core::analyzer::{AnalyzeError, StubAnalyzer};
    use sakatsu_core::model::Analysis;

    struct Named(&'static str);
    impl Analyzer for Named {
        fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
            StubAnalyzer.analyze(text)
        }
        fn engine(&self) -> &str {
            self.0
        }
    }

    fn route(policy: AnalyzerPolicy, primary: &'static str, readiness: Readiness) -> Route {
        Route {
            policy,
            primary: Arc::new(Named(primary)),
            fallback: (primary != "lindera")
                .then(|| Arc::new(Named("lindera")) as Arc<dyn Analyzer>),
            readiness,
            generation: 99,
        }
    }

    /// **The race the engine-name comparison could not see.** A probe starts from one generation;
    /// the reader selects while it runs; the probe's answer is dropped, whatever engines the two
    /// happen to name.
    #[test]
    fn a_probe_that_finishes_after_a_selection_does_not_overwrite_it() {
        let state = AnalyzerState::new(route(
            AnalyzerPolicy::Automatic,
            "lindera",
            Readiness::Probing,
        ));
        let started = state.route().generation;

        // The reader chooses the built-in analyzer while the probe is still running…
        let chosen = state.publish(route(AnalyzerPolicy::BuiltIn, "lindera", Readiness::Ready));
        // …and the probe then finds Ichiran.
        let published = state.publish_if(
            started,
            route(AnalyzerPolicy::Automatic, "ichiran", Readiness::Ready),
        );

        assert!(!published, "the probe's answer is dropped");
        let now = state.route();
        assert_eq!(now.generation, chosen);
        assert_eq!(now.policy, AnalyzerPolicy::BuiltIn);
        assert_eq!(now.primary.engine(), "lindera");
    }

    /// Even a selection of the *same* engine moves the generation on — which is exactly what an
    /// engine-name comparison could not detect.
    #[test]
    fn a_selection_of_the_same_engine_still_supersedes_a_running_probe() {
        let state = AnalyzerState::new(route(
            AnalyzerPolicy::Automatic,
            "lindera",
            Readiness::Probing,
        ));
        let started = state.route().generation;
        state.publish(route(
            AnalyzerPolicy::Automatic,
            "lindera",
            Readiness::Probing,
        ));
        assert!(!state.publish_if(
            started,
            route(AnalyzerPolicy::Automatic, "ichiran", Readiness::Ready)
        ));
    }

    #[test]
    fn a_probe_with_nothing_in_between_is_published_and_moves_the_generation() {
        let state = AnalyzerState::new(route(
            AnalyzerPolicy::Automatic,
            "lindera",
            Readiness::Probing,
        ));
        let started = state.route().generation;
        assert_eq!(
            started, 0,
            "a new state is generation 0, whatever the route said"
        );
        assert!(state.publish_if(
            started,
            route(AnalyzerPolicy::Automatic, "ichiran", Readiness::Ready)
        ));
        assert_eq!(state.route().generation, 1);
        assert_eq!(state.now().engine(), "ichiran");
        // And a second probe from the stale generation is refused.
        assert!(!state.publish_if(
            started,
            route(AnalyzerPolicy::Automatic, "lindera", Readiness::Probing)
        ));
    }

    #[test]
    fn standing_in_is_recorded_for_starting_and_unavailable_and_never_for_built_in() {
        let probing = route(AnalyzerPolicy::Automatic, "lindera", Readiness::Probing);
        assert_eq!(
            probing.standing_in().map(|fallback| fallback.reason),
            Some(FallbackReason::Starting)
        );
        let unavailable = route(
            AnalyzerPolicy::Ichiran,
            "lindera",
            Readiness::Unavailable {
                reason: "wsl is not installed".into(),
                since: Instant::now(),
            },
        );
        let recorded = unavailable.standing_in().expect("recorded");
        assert_eq!(recorded.reason, FallbackReason::Unavailable);
        assert_eq!(recorded.scope, FallbackScope::Session);
        assert_eq!(recorded.detail, "wsl is not installed");
        // Automatic with no Ichiran is recorded too: quiet presentation is not absent provenance.
        let quiet = route(
            AnalyzerPolicy::Automatic,
            "lindera",
            Readiness::Unavailable {
                reason: "not installed".into(),
                since: Instant::now(),
            },
        );
        assert!(quiet.standing_in().is_some());
        // The built-in analyzer asked for by name stands in for nothing.
        let built_in = route(
            AnalyzerPolicy::BuiltIn,
            "lindera",
            Readiness::Unavailable {
                reason: "irrelevant".into(),
                since: Instant::now(),
            },
        );
        assert!(built_in.standing_in().is_none());
        assert!(
            route(AnalyzerPolicy::Automatic, "ichiran", Readiness::Ready)
                .standing_in()
                .is_none()
        );
    }

    #[test]
    fn deferred_startup_waits_for_reading_without_claiming_a_fallback() {
        let deferred = route(AnalyzerPolicy::Automatic, "ichiran", Readiness::Deferred);
        assert!(deferred.standing_in().is_none());
        assert!(!deferred.reprobe_due(Instant::now() + REPROBE_AFTER));
        assert_eq!(deferred.plan().primary.engine(), "ichiran");
    }

    #[test]
    fn interrupted_startup_can_be_retried_without_changing_the_route() {
        let pending = route(AnalyzerPolicy::Automatic, "lindera", Readiness::Probing);
        assert!(pending.reprobe_due(Instant::now()));
        assert!(matches!(pending.readiness, Readiness::Probing));
        assert!(
            !route(AnalyzerPolicy::BuiltIn, "lindera", Readiness::Probing)
                .reprobe_due(Instant::now())
        );
        assert!(
            !route(AnalyzerPolicy::Automatic, "ichiran", Readiness::Ready)
                .reprobe_due(Instant::now())
        );
    }

    #[test]
    fn an_unavailable_ichiran_is_due_for_a_probe_only_after_the_interval() {
        let now = Instant::now();
        let fresh = route(
            AnalyzerPolicy::Automatic,
            "lindera",
            Readiness::Unavailable {
                reason: "x".into(),
                since: now,
            },
        );
        assert!(!fresh.reprobe_due(now));
        assert!(fresh.reprobe_due(now + REPROBE_AFTER));
        let built_in = route(
            AnalyzerPolicy::BuiltIn,
            "lindera",
            Readiness::Unavailable {
                reason: "x".into(),
                since: now,
            },
        );
        assert!(
            !built_in.reprobe_due(now + REPROBE_AFTER),
            "nothing to probe for"
        );
        assert!(
            !route(AnalyzerPolicy::Automatic, "ichiran", Readiness::Ready)
                .reprobe_due(now + REPROBE_AFTER)
        );
    }

    #[test]
    fn a_fixed_route_has_nothing_to_fall_back_to() {
        let state = AnalyzerState::fixed(Arc::new(Named("lindera")));
        let route = state.route();
        assert_eq!(route.policy, AnalyzerPolicy::BuiltIn);
        assert!(route.fallback.is_none());
        assert!(route.plan().standing_in.is_none());
    }
}
