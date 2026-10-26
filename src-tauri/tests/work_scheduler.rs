//! Deterministic scheduling checks. Barriers own the interleavings; elapsed time is only a
//! deadlock safeguard and never decides which result is accepted.

use sakatsu_core::request::{RequestIdentity, WorkPriority};
use sakatsu_desktop_lib::work::{Lane, Scheduler};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier, Mutex};

fn request(
    owner: &str,
    operation: &str,
    generation: u64,
    priority: WorkPriority,
) -> RequestIdentity {
    RequestIdentity {
        owner: owner.into(),
        operation: operation.into(),
        generation,
        priority,
    }
}

fn wait_for_pending(scheduler: &Scheduler, lane: Lane, count: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while scheduler.pending(lane) < count {
        assert!(
            std::time::Instant::now() < deadline,
            "queue never reached {count}"
        );
        std::thread::yield_now();
    }
}

#[test]
fn a_newer_request_removes_obsolete_queued_work_before_engine_invocation() {
    let scheduler = Arc::new(Scheduler::default());
    let held = scheduler.enter(Lane::Ichiran, None).expect("hold lane");
    let (answers, done) = mpsc::channel();

    let first = request("window", "study", 1, WorkPriority::Foreground);
    let first_identity = first.clone();
    let worker = Arc::clone(&scheduler);
    let first_done = answers.clone();
    let a = std::thread::spawn(move || {
        let result = worker.enter(Lane::Ichiran, Some(first));
        first_done.send(("A", result.is_ok())).unwrap();
    });
    wait_for_pending(&scheduler, Lane::Ichiran, 1);

    let second = request("window", "study", 2, WorkPriority::Foreground);
    let worker = Arc::clone(&scheduler);
    let b = std::thread::spawn(move || {
        let result = worker.enter(Lane::Ichiran, Some(second));
        answers.send(("B", result.is_ok())).unwrap();
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while scheduler.is_current_request(&first_identity) {
        assert!(
            std::time::Instant::now() < deadline,
            "new ownership never registered"
        );
        std::thread::yield_now();
    }

    assert_eq!(done.recv().unwrap(), ("A", false));
    drop(held);
    assert_eq!(done.recv().unwrap(), ("B", true));
    a.join().unwrap();
    b.join().unwrap();
}

#[test]
fn foreground_overtakes_speculation_that_has_not_started() {
    let scheduler = Arc::new(Scheduler::default());
    let held = scheduler.enter(Lane::Ocr, None).expect("hold lane");
    let (seen, order) = mpsc::channel();

    let worker = Arc::clone(&scheduler);
    let speculative_seen = seen.clone();
    let speculative = std::thread::spawn(move || {
        let _permit = worker
            .enter(
                Lane::Ocr,
                Some(request("window", "warm", 1, WorkPriority::Speculative)),
            )
            .unwrap();
        speculative_seen.send("speculative").unwrap();
    });
    wait_for_pending(&scheduler, Lane::Ocr, 1);

    let worker = Arc::clone(&scheduler);
    let foreground = std::thread::spawn(move || {
        let _permit = worker
            .enter(
                Lane::Ocr,
                Some(request("window", "capture", 1, WorkPriority::Foreground)),
            )
            .unwrap();
        seen.send("foreground").unwrap();
    });
    wait_for_pending(&scheduler, Lane::Ocr, 2);
    drop(held);

    assert_eq!(order.recv().unwrap(), "foreground");
    assert_eq!(order.recv().unwrap(), "speculative");
    foreground.join().unwrap();
    speculative.join().unwrap();
}

#[test]
fn cancelling_one_subscriber_keeps_shared_work_for_another() {
    let scheduler = Arc::new(Scheduler::default());
    let cache = Arc::new(Mutex::new(None::<String>));
    let invocations = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Barrier::new(2));
    let (started, start_seen) = mpsc::channel();

    let first = request("window-a", "translate", 1, WorkPriority::Foreground);
    let first_id = first.clone();
    let worker = Arc::clone(&scheduler);
    let shared = Arc::clone(&cache);
    let calls = Arc::clone(&invocations);
    let gate = Arc::clone(&release);
    let a = std::thread::spawn(move || {
        let permit = worker.enter(Lane::Ichiran, Some(first)).unwrap();
        calls.fetch_add(1, Ordering::SeqCst);
        started.send(()).unwrap();
        gate.wait();
        *shared.lock().unwrap() = Some("answer".into());
        permit.checkpoint()
    });
    start_seen.recv().unwrap();

    let second = request("window-b", "translate", 1, WorkPriority::Foreground);
    let worker = Arc::clone(&scheduler);
    let shared = Arc::clone(&cache);
    let calls = Arc::clone(&invocations);
    let b = std::thread::spawn(move || {
        let _permit = worker.enter(Lane::Ichiran, Some(second)).unwrap();
        let mut cache = shared.lock().unwrap();
        if cache.is_none() {
            calls.fetch_add(1, Ordering::SeqCst);
            *cache = Some("answer".into());
        }
        cache.clone().unwrap()
    });
    wait_for_pending(&scheduler, Lane::Ichiran, 1);
    scheduler.cancel(&first_id);
    release.wait();

    assert!(
        a.join().unwrap().is_err(),
        "the cancelled subscriber is stale"
    );
    assert_eq!(b.join().unwrap(), "answer");
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
}

#[test]
fn lanes_do_not_block_one_another() {
    let scheduler = Scheduler::default();
    let _ocr = scheduler.enter(Lane::Ocr, None).unwrap();
    let _translation = scheduler.enter(Lane::Translation, None).unwrap();
    let _vlm = scheduler.enter(Lane::Vlm, None).unwrap();
    let _ichiran = scheduler.enter(Lane::Ichiran, None).unwrap();
}

#[test]
fn cheap_owned_work_is_cancelled_without_entering_an_engine_lane() {
    let scheduler = Scheduler::default();
    let first = request("window", "lookup", 1, WorkPriority::Foreground);
    let ownership = scheduler
        .own(Some(first.clone()))
        .expect("first owns lookup");
    let replacement = request("window", "lookup", 2, WorkPriority::Foreground);
    let current = scheduler
        .own(Some(replacement))
        .expect("replacement owns lookup");

    assert!(ownership.checkpoint().is_err());
    assert!(current.checkpoint().is_ok());
    scheduler.cancel(&first);
    assert!(
        current.checkpoint().is_ok(),
        "cancellation is generation scoped"
    );
}

/// A result is published only for the request that still owns its operation.
#[test]
fn a_superseded_request_cannot_publish_its_result() {
    let scheduler = Scheduler::default();
    let older = request("window", "capture", 1, WorkPriority::Foreground);
    let newer = request("window", "capture", 2, WorkPriority::Foreground);
    let _first = scheduler
        .own(Some(older.clone()))
        .expect("older starts current");
    let _second = scheduler
        .own(Some(newer.clone()))
        .expect("newer replaces it");

    let published = Mutex::new(Vec::new());
    assert!(!scheduler.publish_if_current(Some(&older), || published.lock().unwrap().push(1)));
    assert!(scheduler.publish_if_current(Some(&newer), || published.lock().unwrap().push(2)));
    assert_eq!(
        *published.lock().unwrap(),
        vec![2],
        "only the current request's result is kept"
    );
    // Another operation is independent: a capture does not supersede a translation.
    let other = request("window", "translate_sentences", 1, WorkPriority::Foreground);
    let _third = scheduler
        .own(Some(other.clone()))
        .expect("separate operation");
    assert!(scheduler.publish_if_current(Some(&other), || {}));
}

/// **Check and write are one step.** While a result is being published, a newer request of the
/// same operation cannot register; it waits until the publication is over. A check followed by a
/// separate write would let it register in between, and the stale result would land after it.
#[test]
fn a_newer_request_cannot_register_in_the_middle_of_a_publication() {
    let scheduler = Arc::new(Scheduler::default());
    let older = request("window", "capture", 1, WorkPriority::Foreground);
    let _owned = scheduler
        .own(Some(older.clone()))
        .expect("older is current");

    let registered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let inside = Arc::new(Barrier::new(2));
    let contender = {
        let scheduler = Arc::clone(&scheduler);
        let registered = Arc::clone(&registered);
        let inside = Arc::clone(&inside);
        std::thread::spawn(move || {
            inside.wait();
            let newer = request("window", "capture", 2, WorkPriority::Foreground);
            let _ = scheduler.own(Some(newer));
            registered.store(true, Ordering::SeqCst);
        })
    };

    let mut registered_during = None;
    let published = scheduler.publish_if_current(Some(&older), || {
        inside.wait();
        // Give the contender every chance to register if it could.
        std::thread::sleep(std::time::Duration::from_millis(150));
        registered_during = Some(registered.load(Ordering::SeqCst));
    });
    contender.join().expect("contender finishes");

    assert!(published, "the older request was current when it published");
    assert_eq!(
        registered_during,
        Some(false),
        "the newer request registered mid-publication"
    );
    assert!(
        registered.load(Ordering::SeqCst),
        "and it registered afterwards"
    );
    assert!(
        !scheduler.publish_if_current(Some(&older), || {}),
        "after which the older one is stale"
    );
}
