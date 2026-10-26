//! The production handoff must drain every writer and reopen its gates on every refusal.
use sakatsu_desktop_lib::{
    lifecycle::{gates, Gate, ALL},
    update::guarded_handoff,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Barrier, Mutex, MutexGuard,
};

/// The gates are process-wide, so the tests in this binary take turns.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn installer_handoff_waits_for_writers_refuses_new_work_and_recovers_from_failure() {
    let _serial = serial();
    for gate in ALL {
        let writer = gates().enter(gate).unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let barrier = Arc::new(Barrier::new(2));
        let pending = entered.clone();
        let ready = barrier.clone();
        let worker = std::thread::spawn(move || {
            ready.wait();
            guarded_handoff(
                || Ok(()),
                || {
                    pending.store(true, Ordering::SeqCst);
                    for held in ALL {
                        assert!(
                            gates().enter(held).is_err(),
                            "{held:?} admitted work during install"
                        );
                    }
                    Err("installer fixture refused".into())
                },
            )
        });
        barrier.wait();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(!entered.load(Ordering::SeqCst), "{gate:?} was not drained");
        drop(writer);
        assert_eq!(
            worker.join().unwrap().unwrap_err(),
            "installer fixture refused"
        );
        assert!(entered.load(Ordering::SeqCst));
        for held in ALL {
            assert!(gates().enter(held).is_ok(), "failure left {held:?} closed");
        }
    }
    {
        let _exclusive = gates().exclusive(&ALL, std::time::Duration::ZERO).unwrap();
        assert!(sakatsu_desktop_lib::vlm_setup::start_vlm_runtime()
            .unwrap_err()
            .contains("busy"));
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        assert!(
            sakatsu_desktop_lib::commands::start_ichiran_setup(app.handle().clone())
                .unwrap_err()
                .contains("busy")
        );
    }
    let called = AtomicBool::new(false);
    assert_eq!(
        guarded_handoff(
            || Err("invalid signature".into()),
            || {
                called.store(true, Ordering::SeqCst);
                Ok(())
            }
        )
        .unwrap_err(),
        "invalid signature"
    );
    assert!(!called.load(Ordering::SeqCst));
    assert!(gates().enter(Gate::Store).is_ok());
}

/// **No install in the middle of setup.** A setup download holds its claim for the whole
/// transfer, far longer than the handoff waits; the handoff gives up without verifying or
/// installing, says why, and leaves the app usable.
#[test]
fn installer_handoff_is_refused_while_setup_holds_a_claim() {
    let _serial = serial();
    let (held, holding) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let setup = std::thread::spawn(move || {
        sakatsu_desktop_lib::owning([sakatsu_core::setup::claim::Claim::Everything], |_| {
            held.send(()).unwrap();
            released.recv().unwrap();
        })
        .unwrap()
    });
    holding.recv().unwrap();
    let ran = AtomicBool::new(false);
    let refused = guarded_handoff(
        || {
            ran.store(true, Ordering::SeqCst);
            Ok(())
        },
        || {
            ran.store(true, Ordering::SeqCst);
            Ok(())
        },
    )
    .unwrap_err();
    assert!(!ran.load(Ordering::SeqCst), "the handoff ran during setup");
    assert!(refused.contains("not installed"), "{refused}");
    release.send(()).unwrap();
    setup.join().unwrap();
    for gate in ALL {
        assert!(gates().enter(gate).is_ok(), "refusal left {gate:?} closed");
    }
}

#[test]
fn suspension_blocks_new_probes_and_rejects_a_cancelled_late_publication() {
    use sakatsu_desktop_lib::analyzer_probe::{suspend, Probe};
    let _serial = serial();
    let probe = Probe::start().unwrap();
    assert!(probe.publish(|| true).unwrap());
    assert!(
        Probe::start().is_none(),
        "startup and re-probe must share one owner"
    );
    let suspended = suspend().unwrap();
    assert!(probe.cancellation().load(Ordering::Acquire));
    assert!(
        Probe::start().is_none(),
        "an install must prevent a new probe"
    );
    assert!(
        suspend().is_err(),
        "a second suspension cannot reopen the first"
    );
    let published = AtomicBool::new(false);
    assert!(probe
        .publish(|| published.store(true, Ordering::SeqCst))
        .is_none());
    assert!(
        !published.load(Ordering::SeqCst),
        "cancelled output reached the route"
    );
    drop(suspended);
    assert!(
        Probe::start().is_none(),
        "the cancelled child must finish before retrying"
    );
    drop(probe);
    let retry = Probe::start().unwrap();
    assert!(!retry.cancellation().load(Ordering::Acquire));
    assert_eq!(retry.publish(|| 42), Some(42));
}

#[cfg(windows)]
#[test]
fn install_cancels_the_real_probe_child_but_still_waits_for_foreground_analysis() {
    use sakatsu_core::analyzer::{AnalyzeError, Analyzer};
    use sakatsu_core::model::{Analysis, AnalyzerPolicy};
    use sakatsu_desktop_lib::{
        analyzer_probe::Probe,
        commands::{AnalyzerState, BuiltInAnalyzerState},
        route::{Readiness, Route},
    };
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use tauri::Manager;
    let _serial = serial();
    let dir = std::env::temp_dir().join(format!(
        "sakatsu-probe-cancel-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            for name in [
                "sidecar.ps1",
                "ready",
                "roaming/com.sakatsu.app/ichiran-command.json",
            ] {
                let _ = std::fs::remove_file(self.0.join(name));
            }
            let _ = std::fs::remove_dir(self.0.join("roaming/com.sakatsu.app"));
            let _ = std::fs::remove_dir(self.0.join("roaming"));
            let _ = std::fs::remove_dir(&self.0);
        }
    }
    let _fixture = Fixture(dir.clone());
    let marker = dir.join("ready");
    let script = dir.join("sidecar.ps1");
    let marker_literal = marker.to_string_lossy().replace('\'', "''");
    std::fs::write(
        &script,
        format!("[IO.File]::WriteAllText('{marker_literal}', 'ready')\nStart-Sleep -Seconds 60\n"),
    )
    .unwrap();
    // Use the same configured launcher and startup entry point as the app, in an isolated profile.
    struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Environment {
        fn drop(&mut self) {
            for (key, old) in &self.0 {
                match old {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
    let keys = ["APPDATA", "SAKATSU_ICHIRAN_CMD", "SAKATSU_ANALYZER_FAULT"];
    let _environment = Environment(
        keys.iter()
            .map(|&key| (key, std::env::var_os(key)))
            .collect(),
    );
    let appdata = dir.join("roaming");
    let configured = appdata.join("com.sakatsu.app/ichiran-command.json");
    std::fs::create_dir_all(configured.parent().unwrap()).unwrap();
    std::fs::write(configured, serde_json::to_vec(&serde_json::json!({"program": "powershell.exe", "prefix_args": ["-NoProfile", "-File", script]})).unwrap()).unwrap();
    std::env::set_var("APPDATA", &appdata);
    std::env::remove_var("SAKATSU_ICHIRAN_CMD");
    std::env::remove_var("SAKATSU_ANALYZER_FAULT");
    struct BuiltIn;
    impl Analyzer for BuiltIn {
        fn analyze(&self, _: &str) -> Result<Analysis, AnalyzeError> {
            Err(AnalyzeError::EmptyInput)
        }
        fn engine(&self) -> &str {
            "lindera"
        }
    }
    let built_in: Arc<dyn Analyzer> = Arc::new(BuiltIn);
    let app = tauri::test::mock_builder()
        .manage(BuiltInAnalyzerState(built_in.clone()))
        .manage(AnalyzerState::new(Route {
            policy: AnalyzerPolicy::Automatic,
            primary: built_in,
            fallback: None,
            readiness: Readiness::Probing,
            generation: 0,
        }))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let handle = app.handle().clone();
    let (finished, done) = mpsc::channel();
    let child = std::thread::spawn(move || {
        sakatsu_desktop_lib::probe_and_publish(&handle);
        let route = handle.state::<AnalyzerState>().route();
        finished
            .send((
                route.generation,
                matches!(route.readiness, Readiness::Probing),
            ))
            .unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        marker.exists(),
        "the actual sidecar fixture did not start: {:?}",
        app.state::<AnalyzerState>().route().readiness
    );
    let foreground = gates().enter(Gate::Analysis).unwrap();
    let installed = Arc::new(AtomicBool::new(false));
    let called = installed.clone();
    let handoff = std::thread::spawn(move || {
        guarded_handoff(
            || Ok(()),
            || {
                called.store(true, Ordering::SeqCst);
                Err("installer fixture refused".into())
            },
        )
    });
    let (generation, still_pending) = done
        .recv_timeout(Duration::from_secs(2))
        .expect("install did not cancel and reap the background child");
    assert_eq!(
        generation, 0,
        "a cancelled startup check published a new route"
    );
    assert!(
        still_pending,
        "cancellation was misreported as an unavailable analyzer"
    );
    assert_eq!(
        gates().active(Gate::Analysis),
        1,
        "foreground analysis lost its gate"
    );
    assert!(
        !installed.load(Ordering::SeqCst),
        "install interrupted foreground work"
    );
    drop(foreground);
    assert_eq!(
        handoff.join().unwrap().unwrap_err(),
        "installer fixture refused"
    );
    child.join().unwrap();
    assert!(installed.load(Ordering::SeqCst));
    std::fs::remove_file(&marker).unwrap();
    sakatsu_desktop_lib::reprobe_if_due(app.handle());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        marker.exists(),
        "failed installation stranded startup instead of permitting a re-probe"
    );
    assert_eq!(
        guarded_handoff(
            || Err("signature fixture refused".into()),
            || panic!("invalid signature installed")
        )
        .unwrap_err(),
        "signature fixture refused"
    );
    assert_eq!(app.state::<AnalyzerState>().route().generation, 0);

    assert!(
        Probe::start().is_some(),
        "a failed install left probes suspended"
    );
    for gate in ALL {
        assert!(gates().enter(gate).is_ok(), "failure left {gate:?} closed");
    }
    // A schema-valid answer is still publishable; this is protocol evidence, not lexical gold.
    std::fs::write(&script, "[Console]::Write('[\"\\u6c34\"]')").unwrap();
    sakatsu_desktop_lib::probe_and_publish(app.handle());
    let ready = app.state::<AnalyzerState>().route();
    assert_eq!(ready.generation, 1);
    assert_eq!(ready.primary.engine(), "ichiran");
    assert!(matches!(ready.readiness, Readiness::Ready));
    std::fs::write(&script, "[Console]::Write('not JSON')").unwrap();
    sakatsu_desktop_lib::probe_and_publish(app.handle());
    let failed = app.state::<AnalyzerState>().route();
    assert_eq!(failed.generation, 2);
    assert!(
        matches!(&failed.readiness, Readiness::Unavailable { reason, .. } if reason.contains("unexpected JSON"))
    );
}
