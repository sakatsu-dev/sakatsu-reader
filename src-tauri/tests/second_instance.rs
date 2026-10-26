//! A rejected second launch must leave a running installation alone.
//!
//! # What this is checking
//!
//! Recovery mutates the install directories: it restores backups and clears the scratch roots it
//! finds. Run before the single-instance plugin has decided who owns those directories, a second
//! launch would sweep an installation **still in progress in the first process** — deleting its
//! payload, its prepared components and the backup holding the only copy of what it moved aside.
//!
//! The fix was to move both sweeps into `.setup()`, which only the accepted instance reaches. This
//! is the check that the fix holds, and it is deliberately **not** part of desktop acceptance: it
//! is about processes and files, not about what a window looks like, so it belongs here where it
//! can run every time.
//!
//! # How it is driven
//!
//! Two child processes against one scratch profile. The first plays the owner — it takes the
//! single-instance lock and holds it while the second runs. The second plays the rejected launch:
//! it goes as far as a doomed instance goes, and the parent then asserts that everything the owner
//! had in flight is byte-for-byte where it was.
//!
//! The lock is the plugin's own, taken through the same crate the app registers, so this is not a
//! model of the ownership rule — it is the rule. The sweep is
//! `sakatsu_desktop_lib::recover_owned_resources`, the function `run()` calls, from the place
//! `run()` calls it — so this is not a model of the placement either.
//!
//! # Why there are two tests
//!
//! One asserts nothing was touched; the other puts the sweep back where the defect had it and
//! requires the installation to be damaged. A test that only ever observes nothing happening will
//! go on passing when nothing can happen for the wrong reason.

use std::path::{Path, PathBuf};

mod common;
use common::interrupt_a_real_publication;

const OWNER: &str = "SAKATSU_SECOND_INSTANCE_OWNER";
const REJECTED: &str = "SAKATSU_SECOND_INSTANCE_REJECTED";

/// The scratch an installation has in flight beside its backup.
const SCRATCH: [(&str, &str); 2] = [
    ("payload/kanji/kanjidic2.xml.gz", "downloaded bytes"),
    ("prepared/kanji/kanjidic.tsv", "built table"),
];

/// What is in the backup: the bytes that exist nowhere else while the swap is half done.
const ONLY_COPY: &str = "the only copy of the old one";

/// One file the sweep must leave alone, and what has to be in it.
struct Untouched {
    label: String,
    path: PathBuf,
    contents: String,
}

fn scratch(name: &str) -> common::Scratch {
    common::Scratch::new("second-instance", name)
}

/// Plant an installation caught between the publisher's two renames, and say what must survive.
///
/// **The first version of this fixture could not be harmed.** It wrote `{"members":[]}` as the
/// journal, which recovery reads as a root whose backup it cannot place — so the sweep reported it
/// and retained everything, and the wrong placement did no damage either. A test whose fixture is
/// one the sweep never touches asserts nothing about where the sweep runs. This one is produced by
/// the publisher, so the journal says where `kanji` belongs and a sweep that reaches it will
/// restore the backup and then clear the root.
fn plant_in_flight(root: &Path, destination: &Path) -> Vec<Untouched> {
    interrupt_a_real_publication(root, "kanji", destination, &[("kanjidic.tsv", ONLY_COPY)]);

    let mut untouched = Vec::new();
    for (relative, contents) in SCRATCH {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).expect("parent");
        std::fs::write(&path, contents.as_bytes()).expect("in-flight file");
        untouched.push(Untouched {
            label: relative.to_string(),
            path,
            contents: contents.to_string(),
        });
    }
    untouched.push(Untouched {
        label: "backup/kanji/kanjidic.tsv".to_string(),
        path: root.join("backup").join("kanji").join("kanjidic.tsv"),
        contents: ONLY_COPY.to_string(),
    });
    let journal = root.join("publication.json");
    untouched.push(Untouched {
        label: "publication.json".to_string(),
        contents: std::fs::read_to_string(&journal).expect("the publisher wrote its journal"),
        path: journal,
    });
    untouched
}

fn child(test: &str, marker: &str, profile: &Path) -> std::process::Command {
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let mut command = std::process::Command::new(&exe);
    let roaming = profile.join("Roaming");
    let local = profile.join("Local");
    command
        .args([test, "--exact", "--nocapture"])
        .env(marker, profile)
        .env("APPDATA", &roaming)
        .env("LOCALAPPDATA", &local)
        .env("SAKATSU_DEVICE_DATA_DIR", local.join("com.sakatsu.app"))
        .env_remove("SAKATSU_KANJI_DIR")
        .env_remove("SAKATSU_DICT_DIR");
    command
}

#[test]
fn a_rejected_second_launch_leaves_an_installation_in_flight_untouched() {
    if let Some(profile) = std::env::var_os(OWNER) {
        return hold_the_lock(Path::new(&profile));
    }
    if let Some(profile) = std::env::var_os(REJECTED) {
        return attempt_to_start(Path::new(&profile));
    }

    let damaged = scenario(
        "a_rejected_second_launch_leaves_an_installation_in_flight_untouched",
        "untouched",
        Placement::AfterOwnership,
    );
    assert!(
        damaged.is_empty(),
        "a launch that does not own this installation changed it: {}",
        damaged.join("; ")
    );
}

/// **The positive control.** The same scenario with the sweep above the ownership check.
///
/// Without this, the test above is an arrangement that passes because nothing happens in it. This
/// runs the identical child with `recover_owned_resources` called before the builder — the
/// placement production had, and the one that must never come back — and requires the in-flight
/// installation to be damaged. If a future change made that placement harmless, this fails and says
/// the other test has stopped being evidence rather than letting it go quietly green.
#[test]
fn a_sweep_before_ownership_is_what_this_forbids() {
    if let Some(profile) = std::env::var_os(OWNER) {
        return hold_the_lock(Path::new(&profile));
    }
    if let Some(profile) = std::env::var_os(REJECTED) {
        return attempt_to_start(Path::new(&profile));
    }

    let damaged = scenario(
        "a_sweep_before_ownership_is_what_this_forbids",
        "control",
        Placement::BeforeOwnership,
    );
    assert!(
        !damaged.is_empty(),
        "the wrong placement damaged nothing, so the test that forbids it is not evidence of \
         anything"
    );
    // Printed rather than only counted: what the wrong placement destroys is the argument for the
    // right one, and `--nocapture` is where somebody goes looking for it.
    eprintln!("a sweep before ownership destroyed: {}", damaged.join("; "));
}

#[derive(Clone, Copy)]
enum Placement {
    /// Production: inside `.setup()`, which a rejected instance never reaches.
    AfterOwnership,
    /// The defect: above the builder, where a rejected instance runs it.
    BeforeOwnership,
}

/// **The single-instance lock is the machine's, not the profile's.**
///
/// Two scenarios running at once means the second owner is rejected and never takes the lock, which
/// surfaces as "the owner never took the lock" — a true statement about the wrong thing. Serialised
/// here rather than by asking whoever runs the suite for `--test-threads=1`, because a test that
/// needs a flag to be correct will be run without it.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One owner, one rejected launch, one installation in flight. Returns what did not survive.
///
/// Both children call `sakatsu_desktop_lib::recover_owned_resources`; only *where* differs.
fn scenario(test: &str, name: &str, placement: Placement) -> Vec<String> {
    // Poisoning means the other scenario failed; that is its own report, and this one still has to
    // run rather than fail for a reason belonging to a different test.
    let _serialised = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let profile = scratch(name);
    let device = profile.join("Local").join("com.sakatsu.app");
    let interrupted = device.join(".offline-setup-4242-1");
    std::fs::create_dir_all(&interrupted).expect("interrupted root");
    // Inside the profile, because the child resolves `kanji_dir()` from the environment this
    // profile sets — a destination anywhere else would be somebody's installed resources.
    let destination = device.join("kanji");
    let untouched = plant_in_flight(&interrupted, &destination);

    // The owner starts and holds the lock. It says so by writing a file rather than to a pipe:
    // reading one child's stdout while running another meant closing the read end mid-run, and the
    // owner then died of a broken pipe.
    let mut owner = Reaped::spawn("owner", child(test, OWNER, &profile));
    wait_for_file(&profile.join("holding"), "the owner never took the lock");

    // **Spawned, not `output()`-ed, and this is the diagnosis that took a while.** A rejected
    // second instance hands its arguments to the first and waits for the handshake — which a real
    // first instance answers from its event loop. The owner here is a mock with no event loop, so
    // the rejected child blocks until the owner exits, and `output()` blocked the parent with it.
    // The parent then wrote the release file *after* the owner had already timed out, so the owner
    // always ran to its deadline and the test always cost exactly that deadline.
    let mut command = child(test, REJECTED, &profile);
    if let Placement::BeforeOwnership = placement {
        command.env("PLACEMENT", "before");
    }
    let mut rejected = Reaped::spawn("rejected launch", command);

    // It reports that it has reached the point a doomed instance reaches. **That is where the
    // observation has its force**: whatever a sweep placed above the builder was going to do has
    // had its chance by the time this file exists.
    wait_for_file(
        &profile.join("attempted"),
        "the rejected launch never got as far as trying to start",
    );

    let mut damaged = Vec::new();
    for file in &untouched {
        match std::fs::read_to_string(&file.path) {
            Err(_) => damaged.push(format!("{} was removed", file.label)),
            Ok(found) if found != file.contents => {
                damaged.push(format!("{} was rewritten", file.label))
            }
            Ok(_) => {}
        }
    }
    // And the swap itself is not finished by somebody who does not own it: restoring the backup is
    // the sweep's first act, so the destination appearing is damage even though it creates a file
    // rather than deleting one.
    if destination.exists() {
        damaged.push("the backup was restored to its destination".to_string());
    }

    // Release the owner and require it to notice. A deadline still exists in the owner, but it is a
    // failure safeguard now rather than the ordinary path — asserting on the reason is what keeps
    // it that way.
    std::fs::write(profile.join("release-the-lock"), b"done").expect("release");
    let owner_status = owner.wait_or_kill();
    assert!(
        owner_status.success(),
        "the owner did not exit cleanly: {owner_status:?}"
    );
    assert_eq!(
        std::fs::read_to_string(profile.join("owner-exit")).unwrap_or_default(),
        "released",
        "the owner ran to its deadline instead of noticing the release; the deadline is a \
         safeguard, and a test that relies on it is not testing the protocol"
    );

    // The rejected launch unblocks once the lock is gone. Bounded, and killed if it does not.
    let _ = rejected.wait_or_kill();

    damaged
}

/// A spawned child that is **never left running**, including on a failed assertion.
///
/// The reaping used to be a plain call after the assertions, so a failure before it unwound past
/// the cleanup and left a process holding the single-instance lock — and the next run then waited
/// for a lock nobody would release, which reads as a new failure rather than the old one. A guard
/// runs during the unwind, so the failure that gets reported is the one that matters.
struct Reaped {
    what: &'static str,
    child: Option<std::process::Child>,
}

impl Reaped {
    fn spawn(what: &'static str, mut command: std::process::Command) -> Self {
        let child = command
            .spawn()
            .unwrap_or_else(|why| panic!("spawn the {what}: {why}"));
        Self {
            what,
            child: Some(child),
        }
    }

    /// Wait for the child, killing it if it outstays a bounded window.
    fn wait_or_kill(&mut self) -> std::process::ExitStatus {
        let Some(mut child) = self.child.take() else {
            panic!("the {} was already waited for", self.what)
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
            match child.try_wait().expect("waitable") {
                Some(status) => return status,
                None => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
        let _ = child.kill();
        let status = child.wait().expect("waitable");
        panic!(
            "the {} did not exit within its window; killed it ({status:?})",
            self.what
        );
    }
}

impl Drop for Reaped {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Wait for a child to write its signal file.
fn wait_for_file(path: &Path, complaint: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        if path.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("{complaint}");
}

/// The accepted instance: take the single-instance lock and keep it until released.
fn hold_the_lock(profile: &Path) {
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("the first instance is accepted");
    std::fs::write(profile.join("holding"), b"yes").expect("signal that the lock is held");
    // Held until the parent says it is finished, rather than for a fixed span: a sleep long enough
    // to be safe is a sleep the whole suite pays on every run, and one short enough to be quick is
    // a flake waiting for a slow machine.
    // **Bounded at ten seconds, which is the whole cost of this test.** The owner only has to
    // outlive the rejected launch's startup; the release file is the fast path, and the deadline is
    // what stops a missed signal turning into a minute of waiting on every suite run.
    let release = profile.join("release-the-lock");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut why = "deadline";
    while std::time::Instant::now() < deadline {
        if release.exists() {
            why = "released";
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::fs::write(profile.join("owner-exit"), why.as_bytes()).expect("record why");
    drop(app);
}

/// The doomed instance, wired the way production is.
///
/// **This used to prove nothing.** It built an app that never called recovery at all, so moving the
/// sweep back above the builder in `run()` would have left the test green — the arrangement was
/// re-created rather than exercised. It now registers the same plugin and calls
/// `recover_owned_resources` from `.setup()`, which is the function and the place production uses;
/// a rejected instance never reaches setup, so it sweeps nothing.
///
/// `PLACEMENT=before` runs the same function above the builder instead, which is the defect this
/// test exists to forbid. `a_sweep_before_ownership_is_what_this_forbids` uses it to show the
/// observation is sensitive to placement rather than holding regardless.
fn attempt_to_start(profile: &Path) {
    if std::env::var("PLACEMENT").as_deref() == Ok("before") {
        // The defect, reproduced deliberately: recovery reaching the install directories before
        // anything has decided who owns them. A handle is all it needs, and before the real build
        // the only handle available is one from a builder that checks nothing — which is exactly
        // what the defective ordering had.
        let stub = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("a builder with no ownership check accepts anybody");
        sakatsu_desktop_lib::recover_owned_resources(stub.handle());
    }

    // Written before the build, because the build is where this process blocks waiting for a
    // handshake the mock owner cannot answer — and because a sweep placed above the builder has
    // already run by this point, which is what makes the parent's reading of the files meaningful.
    std::fs::write(profile.join("attempted"), b"yes").expect("signal the attempt");

    let outcome = tauri::test::mock_builder()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .setup(|app| {
            // The production placement: the same function `run()` calls, from the same place. A
            // rejected instance never gets here.
            sakatsu_desktop_lib::recover_owned_resources(app.handle());
            Ok(())
        })
        .build(tauri::test::mock_context(tauri::test::noop_assets()));
    // Whether the plugin rejects by erroring or by exiting, what matters is that this process ran
    // no recovery. Both outcomes are acceptable here; sweeping is not.
    drop(outcome);
}
