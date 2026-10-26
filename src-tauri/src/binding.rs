//! What fires a capture, what shows the frame, and how either is changed while running.
//!
//! Two mechanisms, because the platform offers two. A key chord goes through the
//! global-shortcut plugin, which handles registration, conflicts and teardown. A mouse
//! button has no such plugin — there is no mouse support anywhere in it — so on Windows
//! it needs a low-level hook of its own.
//!
//! Both mechanisms feed one place, [`dispatch`], so the show/hide behaviour is written
//! once and works the same whether it is bound to a key or a mouse button.
//!
//! # About the mouse hook
//!
//! `SetWindowsHookEx(WH_MOUSE_LL)` sees every mouse event on the machine, which is a real
//! capability and worth constraining rather than apologising for. Two limits:
//!
//! 1. **It observes and never swallows.** The hook always calls `CallNextHookEx`, so a
//!    bound button goes on doing whatever it normally does. This is what makes binding
//!    *any* button safe, left and right included: the cost of a strange choice is a noisy
//!    binding, not a machine that cannot be clicked.
//! 2. **It is installed only while a mouse binding is configured**, and torn down the
//!    moment none is. The default bindings are key chords, so the common case installs no
//!    hook at all.
//!
//! Worth saying plainly: some antivirus heuristics flag low-level hooks. That is the cost
//! of the feature, not a sign it is doing something else.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sakatsu_core::store::{Binding, MouseButton};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::Shortcut;

/// Emitted when the capture binding fires. The UI owns what a capture *means*, so this
/// only says that one was asked for — exactly as the hotkey always has.
pub const CAPTURE_REQUESTED: &str = "capture-requested";

/// How long a press must last to count as a hold rather than a tap, in milliseconds.
///
/// An atomic rather than a field, because both places that read it — the shortcut handler
/// and the mouse hook callback — are bare functions with nowhere to hang state. Set from
/// [`apply`]; 250ms until the settings say otherwise.
static HOLD_MS: AtomicU64 = AtomicU64::new(250);

fn hold() -> Duration {
    Duration::from_millis(HOLD_MS.load(Ordering::Relaxed))
}

/// How close together two taps must be to lock the frame, in milliseconds. Zero is off.
///
/// An atomic for the same reason [`HOLD_MS`] is one: the release edge arrives on a keyboard
/// poller's thread or the mouse hook's, and neither may reach the store — this has to be
/// answerable without taking a lock.
static LOCK_TAP_MS: AtomicU64 = AtomicU64::new(400);

/// The window a second tap has to land in, or `None` when the gesture is switched off.
fn lock_tap() -> Option<Duration> {
    match LOCK_TAP_MS.load(Ordering::Relaxed) {
        0 => None,
        ms => Some(Duration::from_millis(ms)),
    }
}

/// When the last tap of the show/hide binding was.
///
/// A `Mutex<Option<Instant>>` rather than an atomic because an `Instant` is not one. It is
/// only ever touched on the release edge, which is already doing more work than this.
static LAST_TAP: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether the overlay feature answers at all.
///
/// An atomic for the same reason `HOLD_MS` is one: [`dispatch`] runs on a keyboard
/// poller's thread and on the mouse hook's, neither of which can reach the store — and
/// neither of which may take a lock, because Windows quietly unhooks a callback that is
/// slow to return. Pushed in from `save_settings` and from startup, like the rest.
static OVERLAY_ON: AtomicBool = AtomicBool::new(true);

/// Turn the overlay feature on or off wholesale.
pub fn set_overlay_enabled(on: bool) {
    OVERLAY_ON.store(on, Ordering::Relaxed);
}

/// Whether there is a frame on screen to read through, and therefore whether the capture
/// chord is worth holding.
///
/// A global shortcut is exclusive: while this app owns a chord, no other application can
/// see it. `dispatch` has always refused a capture press with no visible frame — but it
/// refused it *after* taking the key off whoever else wanted it, for the whole time the
/// frame was hidden, which is most of the time.
static CAPTURE_LIVE: AtomicBool = AtomicBool::new(false);

/// Take the capture chord, or give it back, as the frame appears and disappears.
///
/// Only the capture chord, and only when it is a key. Two reasons this is surgical rather
/// than a re-[`apply`]:
///
/// - `apply` calls [`clear`], which resets `OVERLAY_PRESS` and `CAPTURE_DOWN`. The hold
///   path hides the frame *while the overlay key is still down*, so re-applying there
///   would erase the press mid-gesture and the release would arrive with nothing to match.
/// - The switch chord must never be released: it is the way back from off, and giving it up
///   would make switching off a one-way trip.
///
/// The overlay chord is a third case, and this comment used to lump it in with switch and
/// claim it "must never be released". That is true while the overlay is *on* — the key is
/// what shows the frame, so letting go of it while the frame is hidden would make the frame
/// unreachable. It is false once the overlay is switched off, and `set_overlay_live` below
/// is where that is handled.
///
/// A mouse binding is left hooked either way: the hook observes every button and matches
/// afterwards, so it takes nothing from anyone and there is nothing to give back.
pub fn set_capture_live<R: Runtime>(app: &AppHandle<R>, live: bool) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    if !shortcuts_available(app) {
        return;
    }
    if CAPTURE_LIVE.swap(live, Ordering::SeqCst) == live {
        return; // already in that state; the frame emits its visibility more than once
    }

    let Some(store) = app.try_state::<crate::commands::StoreState>() else {
        return;
    };
    let Binding::Key { chord } = store.settings().capture_bind else {
        return;
    };

    if live {
        match register_chord(app, &chord) {
            Ok(parsed) => {
                trace(|| format!("frame shown → capture {chord} taken"));
                if let Ok(mut slot) = CHORDS.lock() {
                    slot.0 = Some(parsed);
                }
            }
            // Said out loud. Between letting go of the chord and asking for it back,
            // another application may have taken it — and then capture is dead with
            // nothing on screen explaining why.
            Err(error) => {
                sakatsu_core::log_warn!(
                    "binding",
                    "the frame is up but {chord} could not be taken back: {error}"
                );
                trace(|| format!("frame shown → capture {chord} REFUSED: {error}"));
            }
        }
    } else {
        let taken = CHORDS.lock().ok().and_then(|mut slot| slot.0.take());
        if let Some(parsed) = taken {
            let _ = app.global_shortcut().unregister(parsed);
            trace(|| format!("frame hidden → capture {chord} released"));
        }
        // A press that was in flight belongs to a chord this app no longer holds.
        CAPTURE_DOWN.store(false, Ordering::SeqCst);
    }
}

/// Whether the global-shortcut plugin is there to talk to.
///
/// It always is in the running app — `lib.rs` installs it at startup. It is not under the
/// mock runtime the IPC tests drive commands through, and `GlobalShortcutExt::global_shortcut`
/// *panics* rather than returning an error when the state was never managed. A command that
/// takes or releases a chord is therefore a command those tests cannot call, unless this is
/// asked first.
fn shortcuts_available<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<tauri_plugin_global_shortcut::GlobalShortcut<R>>()
        .is_some()
}

/// Take or release the overlay chord as the overlay is switched on and off.
///
/// **This is a bug fix wearing a feature's clothes.** A chord registered with the global
/// shortcut plugin is routed to this application and reaches no other — that is what a global
/// hotkey is. It is a fair price while the app can act on the key, and it was being charged
/// when it could not: `apply` took the overlay chord unconditionally, and switching the
/// overlay off flipped `OVERLAY_ON` and hid the frame without touching the registration. So
/// `dispatch` ignored every press while the OS went on delivering them here, and a bare
/// `` ` `` stopped typing in every application on the machine — including this one's own
/// search box — while doing nothing at all.
///
/// Surgical rather than a re-[`apply`], for the reason [`set_capture_live`] is: `apply` calls
/// [`clear`], which resets `OVERLAY_PRESS`, and the switch can be thrown by a binding while
/// another press is being timed.
///
/// A mouse binding is left hooked either way — the hook observes every button and matches
/// afterwards, so it takes nothing from anyone and has nothing to give back.
pub fn set_overlay_live<R: Runtime>(app: &AppHandle<R>, on: bool) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    if !shortcuts_available(app) {
        return;
    }
    let Some(store) = app.try_state::<crate::commands::StoreState>() else {
        return;
    };
    let Binding::Key { chord } = store.settings().overlay_bind else {
        return;
    };

    if on {
        // Only if it is not already held. `switch_overlay` is not the only caller — startup
        // goes through `apply`, which takes it directly.
        let held = CHORDS.lock().map(|slot| slot.1.is_some()).unwrap_or(false);
        if held {
            return;
        }
        match register_chord(app, &chord) {
            Ok(parsed) => {
                trace(|| format!("overlay on → {chord} taken"));
                if let Ok(mut slot) = CHORDS.lock() {
                    slot.1 = Some(parsed);
                }
            }
            // Said out loud: between letting go and asking back, another application may have
            // taken the chord, and then the frame cannot be shown with nothing explaining why.
            Err(error) => {
                sakatsu_core::log_warn!(
                    "binding",
                    "the overlay is on but {chord} could not be taken back: {error}"
                );
                trace(|| format!("overlay on → {chord} REFUSED: {error}"));
            }
        }
    } else {
        let taken = CHORDS.lock().ok().and_then(|mut slot| slot.1.take());
        if let Some(parsed) = taken {
            let _ = app.global_shortcut().unregister(parsed);
            trace(|| format!("overlay off → {chord} released"));
            sakatsu_core::log_info!("binding", "overlay off — {chord} given back");
        }
        // A press in flight belongs to a chord this app no longer holds.
        if let Ok(mut press) = OVERLAY_PRESS.lock() {
            *press = None;
        }
        HOLD_FIRED.store(false, Ordering::SeqCst);
    }
}

/// Change what counts as a hold.
pub fn set_hold(ms: u64) {
    // A zero threshold would make every press a hold and the frame impossible to toggle
    // off; a huge one would make it impossible to hold. Both are recoverable, but neither
    // is worth storing.
    HOLD_MS.store(ms.clamp(50, 5_000), Ordering::Relaxed);
}

/// Change what counts as a double tap. Zero switches the gesture off.
///
/// Not clamped here, unlike [`set_hold`]: the caller passes `Settings::lock_tap`, which has
/// already decided what a usable window is and — crucially — that zero means off rather than
/// "very fast". Clamping again here would either undo that or state the range twice.
pub fn set_lock_tap(ms: u64) {
    LOCK_TAP_MS.store(ms, Ordering::Relaxed);
    // A change of mind mid-gesture must not leave half a double tap waiting: switching the
    // feature off and straight back on would otherwise complete a tap recorded before it.
    *LAST_TAP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

/// Trace what the bindings are doing, when `SAKATSU_BIND_DEBUG` is set.
///
/// Behind a flag because it fires on every key repeat, and off by default because none of
/// it means anything until something is wrong. It exists because this is the second time
/// this feature has been "fixed" without anyone watching what the events actually do —
/// the thread each edge arrives on turned out to be the whole story.
/// The env var stays the verbosity control rather than becoming a third setting: the
/// setting decides whether a file exists, this decides how loud it is. Per-key-repeat
/// detail in every log would drown everything else in it.
pub fn trace(message: impl FnOnce() -> String) {
    if std::env::var_os("SAKATSU_BIND_DEBUG").is_some() {
        let message = message();
        let thread = std::thread::current().id();
        eprintln!("[bind {thread:?}] {message}");
        sakatsu_core::log_info!("bind", "{thread:?} {message}");
    }
}

/// What became of one binding when [`apply`] ran.
///
/// `Withheld` is the distinction that was missing. `apply` deliberately does not take the
/// capture chord unless the frame is up, or the overlay chord while the overlay is switched
/// off — and both of those printed as `REFUSED`, which is the word for a chord another
/// application owns. So the log said the default capture binding was unavailable on every
/// launch, when nothing had tried to take it. A log read for diagnosis has to separate "could
/// not" from "chose not".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Took {
    Registered,
    Refused,
    /// Not attempted, because this binding is only held while it can do something.
    Withheld,
    /// A mouse binding, which cannot fail: the hook observes every button and matches
    /// afterwards, so it takes nothing from anyone.
    Hooked,
}

/// A binding and what became of it, for the log.
fn describe(binding: &Binding, took: Took) -> String {
    match binding {
        Binding::Key { chord } => match took {
            Took::Registered => format!("{chord} (registered)"),
            Took::Refused => format!("{chord} (REFUSED)"),
            Took::Withheld => format!("{chord} (not held yet)"),
            Took::Hooked => format!("{chord} (hooked)"),
        },
        Binding::Mouse { button } => format!("mouse {button:?} (hooked)"),
    }
}

/// What a binding does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Read what is under the frame.
    Capture,
    /// Bring the frame to the cursor on a tap; hide it on a hold.
    Overlay,
    /// Switch the whole overlay feature off, and back on.
    ///
    /// The one action that still answers while the feature is off, because it is the way
    /// back. Everything else is turned away at the top of [`dispatch`].
    Switch,
}

/// The chords currently registered, so a firing shortcut can be traced back to what it
/// was bound to. The plugin hands the handler a `&Shortcut` and nothing else.
static CHORDS: Mutex<(Option<Shortcut>, Option<Shortcut>, Option<Shortcut>)> =
    Mutex::new((None, None, None));

/// When the overlay binding went down.
///
/// Only the time: what the frame was doing at the time is deliberately not consulted, so
/// a tap always shows and a hold always hides whatever state it started from.
///
/// Being `Some` also means a press is in flight, which is what makes the auto-repeat
/// guard below possible.
static OVERLAY_PRESS: Mutex<Option<Instant>> = Mutex::new(None);

/// A press still recorded after this long never saw its release, so the next press is a
/// new one rather than a repeat. Without this, one lost release would make the binding
/// ignore every press until the app restarted.
const ABANDONED: Duration = Duration::from_secs(10);

/// Whether the capture binding is currently down, for the same reason.
static CAPTURE_DOWN: AtomicBool = AtomicBool::new(false);

/// Whether the current press already turned into a hold and hid the frame, so the release
/// knows there is nothing left to do.
static HOLD_FIRED: AtomicBool = AtomicBool::new(false);

/// Which press the pending hold timer belongs to.
///
/// The timer sleeps for the threshold and then acts, so it has to be able to tell that the
/// press it was started for is the one still down. Without it, releasing and pressing again
/// quickly would let the first timer hide a frame the second press had just summoned.
static PRESS_GENERATION: AtomicU64 = AtomicU64::new(0);

fn chord_for(shortcut: &Shortcut) -> Option<Action> {
    let chords = CHORDS.lock().ok()?;
    if chords.0.as_ref() == Some(shortcut) {
        Some(Action::Capture)
    } else if chords.1.as_ref() == Some(shortcut) {
        Some(Action::Overlay)
    } else if chords.2.as_ref() == Some(shortcut) {
        Some(Action::Switch)
    } else {
        None
    }
}

/// Everything a binding firing can lead to, whichever mechanism noticed it.
///
/// # A held key is not one press
///
/// Windows auto-repeats `WM_HOTKEY`, and `global-hotkey` turns every repeat into another
/// `Pressed` — plus a fresh thread polling for the release, so the eventual release
/// arrives several times over
/// (`global-hotkey/src/platform_impl/windows/mod.rs`, `global_hotkey_proc`).
///
/// Holding a key therefore delivers a stream of presses, not one. Acting on each of them
/// made hold-to-hide flicker hide→show→hide at the repeat rate, and holding the capture
/// binding fired a burst of captures. Both are guarded here on a press already being in
/// flight, so only the first edge counts. The release side needs no guard: it takes the
/// press, and the duplicates that follow find nothing left to take.
pub fn dispatch<R: Runtime>(app: &AppHandle<R>, action: Action, pressed: bool) {
    // Switched off, neither binding does anything. Before the match rather than in each
    // arm: both actions and both edges are covered by one check, and a press that is
    // swallowed here cannot leave a half-recorded press behind for the release to find.
    // `Switch` is exempt: it is the way back on, and a gate that swallowed it would make
    // switching off a one-way trip for anyone who set the hotkey and left the settings.
    if action != Action::Switch && !OVERLAY_ON.load(Ordering::Relaxed) {
        trace(|| format!("{action:?} ignored — the overlay is switched off"));
        return;
    }

    match (action, pressed) {
        (Action::Capture, true) => {
            if CAPTURE_DOWN.swap(true, Ordering::SeqCst) {
                return; // a repeat of a key still held
            }

            // Nothing on screen to read through: do nothing at all.
            //
            // Not "fire, refuse, and show why". Capture used to reach the reader, get
            // turned down by `read_frame` and put a message under the input box —
            // a blocked feature announcing that it is blocked, in the middle of whatever
            // was being read. The overlay not being there is already the answer, and it
            // is on screen where the question was asked.
            //
            // Stopping here rather than in the UI means the event never crosses the IPC
            // boundary, so there is nothing on the other side that could decide to say
            // something about it.
            if !crate::commands::frame_is_visible(app) {
                trace(|| "capture pressed with no visible frame — ignored".into());
                sakatsu_core::log_info!(
                    "capture",
                    "binding pressed with no visible frame — ignored"
                );
                return;
            }

            let _ = app.emit(CAPTURE_REQUESTED, ());
        }
        (Action::Capture, false) => CAPTURE_DOWN.store(false, Ordering::SeqCst),

        // On the press, and nothing on the release: this is a toggle, and acting on both
        // edges would flip it twice per keystroke and look like it did nothing.
        (Action::Switch, true) => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match crate::commands::switch_overlay(&app) {
                    Ok(on) => trace(|| {
                        format!("switch pressed → overlay {}", if on { "on" } else { "off" })
                    }),
                    Err(error) => {
                        sakatsu_core::log_warn!("binding", "could not switch the overlay: {error}");
                        trace(|| format!("switch failed: {error}"));
                    }
                }
            });
        }
        (Action::Switch, false) => {}

        // A tap shows the frame, a hold hides it, and both stay that way. How long the
        // press lasted is the whole input; what the frame was doing beforehand is not
        // consulted at all.
        //
        // The earlier version decided on the *press* using the frame's current state,
        // which meant holding while it was hidden showed it — the opposite of what a
        // "hold to hide" is for. Since the frame is usually hidden when you reach for the
        // key, holding and tapping did the same thing.
        (Action::Overlay, true) => {
            // The press starts a clock and nothing else. Touching a window here would be
            // the deadlock `begin_capture` already taught us about — this edge arrives on
            // the main thread, inside a window procedure.
            let generation = {
                let Ok(mut press) = OVERLAY_PRESS.lock() else {
                    return;
                };
                match *press {
                    // A press already in flight is Windows auto-repeating a held key.
                    Some(at) if at.elapsed() < ABANDONED => {
                        trace(|| "overlay press ignored: already down".into());
                        return;
                    }
                    // Unless it is impossibly old, in which case its release was lost and
                    // holding on to it would ignore every press from here on.
                    _ => *press = Some(Instant::now()),
                }
                HOLD_FIRED.store(false, Ordering::SeqCst);
                PRESS_GENERATION.fetch_add(1, Ordering::SeqCst) + 1
            };
            trace(|| format!("overlay press {generation} recorded"));

            // The hold fires *at* the threshold, while the key is still down, rather than
            // waiting for the release. That is the whole feel of it: hold, and the frame
            // gets out of the way there and then. Deciding on release instead means
            // nothing happens until you let go, which reads as the binding being dead.
            let app = app.clone();
            let threshold = hold();
            // A blocking sleep, so this goes on a thread of its own rather than parking a
            // shared async worker for half a second.
            std::thread::spawn(move || {
                std::thread::sleep(threshold);
                // Still the same press, and still held?
                if PRESS_GENERATION.load(Ordering::SeqCst) != generation {
                    return;
                }
                if OVERLAY_PRESS
                    .lock()
                    .map(|press| press.is_none())
                    .unwrap_or(true)
                {
                    return; // released before the threshold — that was a tap
                }
                trace(|| format!("overlay press {generation} became a hold → hide"));
                HOLD_FIRED.store(true, Ordering::SeqCst);
                crate::commands::hide_frame(&app);
            });
        }
        (Action::Overlay, false) => {
            let press = OVERLAY_PRESS.lock().ok().and_then(|mut slot| slot.take());
            // Retire the generation so a timer still sleeping cannot fire after this.
            PRESS_GENERATION.fetch_add(1, Ordering::SeqCst);

            if press.is_none() {
                trace(|| "overlay release with no press recorded".into());
                return;
            }
            if HOLD_FIRED.swap(false, Ordering::SeqCst) {
                trace(|| "overlay release after a hold — already hidden".into());
                return;
            }

            // **Decided here, on the edge, rather than in the task below.** The task is
            // spawned and may be scheduled at whatever moment the runtime gets to it, so
            // timing two taps from inside it measures the scheduler as much as the reader.
            // This is the only place the gap between two taps is the gap between two taps.
            //
            // A hold is deliberately not a tap for this purpose: it never reaches here, so
            // tap-then-hold cannot lock anything, and hold-then-tap starts the count fresh.
            let double_tap = match lock_tap() {
                None => false,
                Some(window) => {
                    let now = Instant::now();
                    // Recovered rather than bailed out of, like every other lock here. A
                    // `return` on poisoning leaves `dispatch` before the spawn below, so the
                    // show/hide chord would stop doing anything at all - silently, for the rest
                    // of the session. The value guarded is an `Option<Instant>`: a panic cannot
                    // leave it meaning anything but "a tap was, or was not, seen".
                    let mut last = LAST_TAP
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let double = last.is_some_and(|at| now.duration_since(at) <= window);
                    // Cleared on a match, so three taps are one double tap and a fresh
                    // first — not two overlapping ones flipping the lock back as fast as
                    // the key repeats.
                    *last = if double { None } else { Some(now) };
                    double
                }
            };

            // Off this thread: the release arrives on a key poller's thread or on the
            // mouse hook's, and Windows quietly unhooks a callback that is slow to return.
            // The store is read inside for that reason too — it takes a lock, and this is
            // not the thread to take one on.
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                // Locked, a tap still shows the frame — it just shows it where it was
                // left. Read here rather than carried in, because the binding layer is a
                // bare handler with no state of its own; `show_frame` reaches for the
                // store the same way for the rect.
                let locked = app
                    .try_state::<crate::commands::StoreState>()
                    .map(|store| store.settings().frame_locked)
                    .unwrap_or(false);
                trace(|| {
                    if locked {
                        "overlay release was a tap, frame locked → show in place".into()
                    } else {
                        "overlay release was a tap → move to cursor".into()
                    }
                });

                let shown = if locked {
                    crate::commands::show_frame(&app, None)
                } else {
                    crate::commands::center_frame_on_cursor(&app)
                };
                if let Err(error) = shown {
                    eprintln!("could not bring the capture frame up: {error}");
                    sakatsu_core::log_warn!(
                        "binding",
                        "could not bring the capture frame up: {error}"
                    );
                    trace(|| format!("showing the frame failed: {error}"));
                }

                // **After the show, not instead of it.** The second tap does everything the
                // first did and then locks, which is what makes the gesture read as "put it
                // here and keep it here": the frame has already travelled to the pointer by
                // the time the lock closes on it. Locking *instead* of showing would leave
                // the frame wherever the first tap dropped it and lock that, which is a
                // different and worse promise.
                if double_tap {
                    match crate::commands::toggle_frame_lock(&app) {
                        Ok(now) => {
                            trace(|| {
                                format!(
                                    "overlay double tap → frame {}",
                                    if now { "locked" } else { "unlocked" }
                                )
                            });
                            sakatsu_core::log_info!(
                                "binding",
                                "double tap on the show/hide binding — frame {}",
                                if now { "locked" } else { "unlocked" }
                            );
                        }
                        Err(error) => {
                            sakatsu_core::log_warn!("binding", "could not lock the frame: {error}");
                            trace(|| format!("locking the frame failed: {error}"));
                        }
                    }
                }
                trace(|| {
                    format!(
                        "overlay tap done, frame visible = {}",
                        crate::commands::frame_is_visible(&app)
                    )
                });
            });
        }
    }
}

/// The global-shortcut plugin, with the handler that routes both bindings.
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri_plugin_global_shortcut::ShortcutState;

    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            let action = chord_for(shortcut);
            trace(|| {
                format!(
                    "shortcut {:?} state {:?} → {:?}",
                    shortcut.id(),
                    event.state(),
                    action
                )
            });
            let Some(action) = action else {
                return;
            };
            // Both edges matter now. The capture binding ignores the release itself —
            // firing on both would start two captures per press — but the overlay
            // binding is defined by the gap between them.
            dispatch(app, action, event.state() == ShortcutState::Pressed);
        })
        .build()
}

/// What [`apply`] could not take.
///
/// **Which ones, not just why.** The message alone was enough for a caller that only had to
/// report it, and not enough for one that has to decide what to do about it: startup used to
/// see any failure and put *all three* bindings back to their defaults, throwing away the two
/// that had registered perfectly well. Naming the refused actions is what lets a caller
/// replace exactly those.
#[derive(Debug, Clone)]
pub struct Refusal {
    /// The actions whose chords were refused, in the order they were tried.
    pub actions: Vec<Action>,
    /// The first reason, which is the one worth showing someone.
    pub message: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.message)
    }
}

/// Apply the bindings, replacing whatever was active.
///
/// Returns what was refused rather than logging it: a binding another application already owns
/// cannot be taken, and the caller needs to know so it can keep the old one. Silently ending up
/// with nothing bound is the outcome to avoid.
pub fn apply<R: Runtime>(
    app: &AppHandle<R>,
    capture: &Binding,
    overlay: &Binding,
    switch: Option<&Binding>,
) -> Result<(), Refusal> {
    clear(app);

    // Both are attempted, and whichever registered is kept, before any failure is
    // returned. Bailing out on the first error used to leave `CHORDS` unwritten, so a
    // conflict on the second binding silently disabled the first one too — which
    // routes nothing and looks exactly like the app ignoring the keyboard.
    let mut chords = (None, None, None);
    let mut refused: Vec<Action> = Vec::new();
    let mut first_error = None;
    // Per action, so the log can say what actually happened to each rather than reading it
    // back off `chords` — where "absent" covers refused and withheld alike.
    let mut took = [Took::Withheld; 3];
    let slot = |action: Action| match action {
        Action::Capture => 0,
        Action::Overlay => 1,
        Action::Switch => 2,
    };
    for (action, binding) in [
        (Action::Capture, Some(capture)),
        (Action::Overlay, Some(overlay)),
        (Action::Switch, switch),
    ]
    .into_iter()
    .filter_map(|(action, binding)| binding.map(|b| (action, b)))
    {
        match binding {
            Binding::Key { chord } => {
                // Capture is held only while there is a frame to read through — see
                // `set_capture_live`. At startup, and after any rebind made while the frame
                // is hidden, that means not taking it at all; showing the frame takes it.
                if action == Action::Capture && !CAPTURE_LIVE.load(Ordering::SeqCst) {
                    continue;
                }
                // The same rule for the overlay, and it has to be here as well as in
                // `set_overlay_live`: `apply` runs at startup and on every rebind, so
                // without this, saving a binding while the overlay is off would take the
                // key straight back.
                if action == Action::Overlay && !OVERLAY_ON.load(Ordering::Relaxed) {
                    continue;
                }
                match register_chord(app, chord) {
                    Ok(parsed) => {
                        took[slot(action)] = Took::Registered;
                        match action {
                            Action::Capture => chords.0 = Some(parsed),
                            Action::Overlay => chords.1 = Some(parsed),
                            Action::Switch => chords.2 = Some(parsed),
                        }
                    }
                    // Every refusal, not only the first. The first-only version made a second
                    // failure invisible: the log said one chord was unavailable while two were,
                    // and the second had no line of its own to be found by.
                    Err(error) => {
                        took[slot(action)] = Took::Refused;
                        sakatsu_core::log_warn!("binding", "{action:?}: {error}");
                        trace(|| format!("{action:?} REFUSED: {error}"));
                        refused.push(action);
                        first_error.get_or_insert(error);
                    }
                }
            }
            Binding::Mouse { button } => {
                took[slot(action)] = Took::Hooked;
                mouse::bind(app.clone(), action, *button)
            }
        }
    }

    trace(|| {
        format!(
            "applied: capture {:?} → {:?}, overlay {:?} → {:?}, switch {:?} → {:?}",
            capture,
            chords.0.map(|c| c.id()),
            overlay,
            chords.1.map(|c| c.id()),
            switch,
            chords.2.map(|c| c.id())
        )
    });

    // What registered, not only what failed.
    //
    // Only failures were logged before, which makes "the binding never registered" and
    // "it registered and the press never arrived" the same silence. Those have completely
    // different causes — a chord another app owns, versus a key the OS did not route —
    // and no line in the file separated them.
    sakatsu_core::log_info!(
        "binding",
        "capture {}, overlay {}, switch {}",
        describe(capture, took[0]),
        describe(overlay, took[1]),
        match switch {
            Some(binding) => describe(binding, took[2]),
            None => "unbound".to_string(),
        }
    );
    if let Ok(mut slot) = CHORDS.lock() {
        *slot = chords;
    }
    if refused.is_empty() {
        Ok(())
    } else {
        Err(Refusal {
            actions: refused,
            message: first_error.unwrap_or_else(|| "a binding was refused".to_string()),
        })
    }
}

/// Release everything currently bound.
pub fn clear<R: Runtime>(app: &AppHandle<R>) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let _ = app.global_shortcut().unregister_all();
    // Any press in flight belonged to a binding that no longer exists, and leaving it
    // would make the next press look like a repeat and be ignored.
    CAPTURE_DOWN.store(false, Ordering::SeqCst);
    if let Ok(mut press) = OVERLAY_PRESS.lock() {
        *press = None;
    }
    if let Ok(mut slot) = CHORDS.lock() {
        *slot = (None, None, None);
    }
    mouse::clear();
}

fn register_chord<R: Runtime>(app: &AppHandle<R>, chord: &str) -> Result<Shortcut, String> {
    use std::str::FromStr;
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let parsed = Shortcut::from_str(chord)
        .map_err(|error| format!("{chord} is not a shortcut — {error}"))?;
    app.global_shortcut()
        .register(parsed)
        .map_err(|error| format!("{chord} is unavailable — {error}"))?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    /// Anything the recorder can produce has to parse, including a bare key with no
    /// modifier. The recorder emits `KeyboardEvent.code` names — Backquote, KeyQ, F1 —
    /// precisely because those are the names this parser accepts; using `key` instead
    /// would send "`" for a backtick and "Dead" for anything on a compose layout.
    #[test]
    fn the_recorder_chords_parse_including_bare_keys() {
        for chord in [
            "Backquote", // the bare ` this exists for
            "KeyQ",
            "F1",
            "Escape",
            "Space",
            "Digit1",
            "ArrowUp",
            "CmdOrCtrl+Shift+KeyQ",
            "Alt+Backquote",
            "CmdOrCtrl+Shift+Alt+Super+KeyE",
        ] {
            assert!(
                Shortcut::from_str(chord).is_ok(),
                "{chord} should parse as an accelerator"
            );
        }
    }

    /// A chord and the same chord are the same binding; the handler tells the two
    /// actions apart by comparing these, so equality has to mean what it looks like.
    #[test]
    fn chords_compare_by_what_they_are() {
        let a = Shortcut::from_str("CmdOrCtrl+Shift+KeyQ").expect("parses");
        let b = Shortcut::from_str("CmdOrCtrl+Shift+KeyQ").expect("parses");
        let other = Shortcut::from_str("Backquote").expect("parses");
        assert_eq!(a, b);
        assert_ne!(a, other);
    }
}

/// The Windows mouse hook. Everywhere else, a mouse binding is accepted and inert.
#[cfg(windows)]
mod mouse {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{mpsc, OnceLock};
    use std::thread::JoinHandle;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
        UnhookWindowsHookEx, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_MOUSE_LL, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP,
        WM_XBUTTONDOWN, WM_XBUTTONUP,
    };

    /// Which button each action is bound to, or `NONE`.
    static CAPTURE: AtomicU32 = AtomicU32::new(NONE);
    static OVERLAY: AtomicU32 = AtomicU32::new(NONE);
    static SWITCH: AtomicU32 = AtomicU32::new(NONE);

    const NONE: u32 = 0;
    const LEFT: u32 = 1;
    const MIDDLE: u32 = 2;
    const RIGHT: u32 = 3;
    const X1: u32 = 4;
    const X2: u32 = 5;

    /// How the hook reaches the app. Boxed because the hook callback is a bare `extern
    /// "system"` function and cannot carry a runtime-generic `AppHandle` any other way.
    type Dispatch = Box<dyn Fn(Action, bool) + Send + Sync>;
    static DISPATCH: OnceLock<Mutex<Option<Dispatch>>> = OnceLock::new();

    /// The hook thread is owned rather than detached. In particular, `thread.is_some()` is
    /// written before another caller can try to start one, closing the start-up gap where the
    /// old raw handle was still zero and every mouse-bound action spawned another hook.
    #[derive(Default)]
    struct HookOwner {
        thread_id: u32,
        thread: Option<JoinHandle<()>>,
    }

    static OWNER: OnceLock<Mutex<HookOwner>> = OnceLock::new();

    fn owner() -> &'static Mutex<HookOwner> {
        OWNER.get_or_init(|| Mutex::new(HookOwner::default()))
    }

    fn code(button: MouseButton) -> u32 {
        match button {
            MouseButton::Left => LEFT,
            MouseButton::Middle => MIDDLE,
            MouseButton::Right => RIGHT,
            MouseButton::X1 => X1,
            MouseButton::X2 => X2,
        }
    }

    pub fn bind<R: Runtime>(app: AppHandle<R>, action: Action, button: MouseButton) {
        match action {
            Action::Capture => CAPTURE.store(code(button), Ordering::SeqCst),
            Action::Overlay => OVERLAY.store(code(button), Ordering::SeqCst),
            Action::Switch => SWITCH.store(code(button), Ordering::SeqCst),
        }

        let slot = DISPATCH.get_or_init(|| Mutex::new(None));
        if let Ok(mut held) = slot.lock() {
            *held = Some(Box::new(move |action, pressed| {
                super::dispatch(&app, action, pressed)
            }));
        }

        let mut owner = owner()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if owner
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
        {
            return; // already hooked; only the bound button changed
        }
        if let Some(finished) = owner.thread.take() {
            let _ = finished.join();
        }
        owner.thread_id = 0;

        // The hook must live on a thread with a message pump. Wait for that pump to exist
        // before returning: `clear` can then always address it by thread id, post `WM_QUIT`,
        // and join it. No detached thread and no polling an unrelated atomic from GetMessage.
        let (started, ready) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || hook_thread(started));
        match ready.recv() {
            Ok(Some(thread_id)) => {
                owner.thread_id = thread_id;
                owner.thread = Some(thread);
            }
            Ok(None) | Err(_) => {
                let _ = thread.join();
                sakatsu_core::log_warn!("binding", "could not install the Windows mouse hook");
            }
        }
    }

    pub fn clear() {
        CAPTURE.store(NONE, Ordering::SeqCst);
        OVERLAY.store(NONE, Ordering::SeqCst);
        SWITCH.store(NONE, Ordering::SeqCst);
        if let Some(slot) = DISPATCH.get() {
            if let Ok(mut dispatch) = slot.lock() {
                *dispatch = None;
            }
        }

        let mut owner = owner()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(thread) = owner.thread.take() else {
            owner.thread_id = 0;
            return;
        };
        let thread_id = std::mem::take(&mut owner.thread_id);
        let posted = unsafe { PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Err(error) = posted {
            // A valid, live owner creates its queue before publishing the id, so this means
            // the thread has already gone away. Never wait forever on the impossible case.
            if !thread.is_finished() {
                sakatsu_core::log_warn!(
                    "binding",
                    "could not stop the Windows mouse-hook thread: {error}"
                );
                owner.thread_id = thread_id;
                owner.thread = Some(thread);
                return;
            }
        }
        let _ = thread.join();
    }

    fn hook_thread(started: mpsc::SyncSender<Option<u32>>) {
        unsafe {
            let thread_id = GetCurrentThreadId();
            // `PostThreadMessageW` fails until the recipient owns a message queue. Peeking is
            // the documented way to create it without consuming anything.
            let mut message = MSG::default();
            let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);

            let Ok(hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), None, 0) else {
                let _ = started.send(None);
                return;
            };
            if started.send(Some(thread_id)).is_err() {
                let _ = UnhookWindowsHookEx(hook);
                return;
            }

            loop {
                let result = GetMessageW(&mut message, None, 0, 0);
                if result.0 <= 0 {
                    break;
                }
            }
            let _ = UnhookWindowsHookEx(hook);
        }
    }

    /// Which button an event is about, or `NONE` for one that cannot be bound.
    unsafe fn button_of(message: u32, lparam: LPARAM) -> u32 {
        match message {
            WM_LBUTTONDOWN | WM_LBUTTONUP => LEFT,
            WM_RBUTTONDOWN | WM_RBUTTONUP => RIGHT,
            WM_MBUTTONDOWN | WM_MBUTTONUP => MIDDLE,
            WM_XBUTTONDOWN | WM_XBUTTONUP => {
                // Which X button is in the high word of mouseData.
                let data = (*(lparam.0 as *const MSLLHOOKSTRUCT)).mouseData;
                match (data >> 16) & 0xFFFF {
                    1 => X1,
                    2 => X2,
                    _ => NONE,
                }
            }
            _ => NONE,
        }
    }

    /// The hook callback.
    ///
    /// Does as little as possible and **always** passes the event on. Windows removes a
    /// hook that is slow to return, and a hook that swallows input is a hazard, so this
    /// only compares a button and dispatches.
    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let message = wparam.0 as u32;
            let pressed = matches!(
                message,
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
            );
            let released = matches!(
                message,
                WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP
            );

            if pressed || released {
                let button = button_of(message, lparam);
                if button != NONE {
                    let action = if CAPTURE.load(Ordering::SeqCst) == button {
                        Some(Action::Capture)
                    } else if OVERLAY.load(Ordering::SeqCst) == button {
                        Some(Action::Overlay)
                    } else if SWITCH.load(Ordering::SeqCst) == button {
                        Some(Action::Switch)
                    } else {
                        None
                    };
                    if let Some(action) = action {
                        if let Some(slot) = DISPATCH.get() {
                            if let Ok(held) = slot.lock() {
                                if let Some(dispatch) = held.as_ref() {
                                    dispatch(action, pressed);
                                }
                            }
                        }
                    }
                }
            }
        }
        // Unconditionally. The bound button keeps doing whatever it normally does.
        CallNextHookEx(None, code, wparam, lparam)
    }
}

#[cfg(not(windows))]
mod mouse {
    use super::*;

    /// Mouse binding is Windows-only for now. Accepted and inert elsewhere rather than
    /// failing, so a settings file written on Windows still loads.
    pub fn bind<R: Runtime>(_app: AppHandle<R>, _action: Action, _button: MouseButton) {}
    pub fn clear() {}
}
