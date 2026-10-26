//! The app's own title bar, and the one part of it Windows insists on owning.
//!
//! # What this is instead of
//!
//! This module used to tint the *native* caption through three DWM attributes, on the
//! reasoning that drawing our own bar would cost the drag loop, double-click to maximise,
//! the system menu, and **snap layouts** — the flyout Windows 11 shows when the maximise
//! button is hovered — and that the last of those "cannot be reproduced from a webview".
//!
//! The conclusion was wrong, and the reason is worth writing down because it is not obvious:
//! the flyout is not a webview feature and never could be. Windows shows it when a window
//! answers `WM_NCHITTEST` with `HTMAXBUTTON`. That is one integer from native code, and it is
//! the whole trick. Every application that keeps a custom title bar honest does this —
//! Chromium does it for Electron (which is how VS Code has it), and Windows Terminal does it
//! by hand in `NonClientIslandWindow.cpp`.
//!
//! # Why a child window rather than subclassing the main one
//!
//! Because the webview would answer first. Terminal hit exactly this and left the note that
//! saved us the discovery:
//!
//! > to get snap layout support, we need to actually return `HTMAXBUTTON` where the maximize
//! > button is. **If the drag bar doesn't cover the caption buttons, then the core input site
//! > (which takes up the entirety of the XAML island) will steal the `WM_NCHITTEST` before we
//! > get a chance to handle it.**
//!
//! Read "the WebView2 child HWND" for "the XAML island" and that is our window. WebView2 fills
//! the client area with an HWND of its own, so a hit test over the maximise button never
//! reaches the frame's window procedure. So a small child window is parked over that button
//! instead, and *its* procedure answers. It paints nothing; the button the reader sees is
//! ordinary HTML underneath.
//!
//! # What that costs, and what it buys
//!
//! The cost is that the maximise button stops receiving mouse input the ordinary way — the
//! region is non-client now, and the webview is not told about it. So hover and press are
//! forwarded to the page as events ([`MAX_ENTER`] and friends) and the CSS reacts to those
//! instead of to `:hover`. Terminal calls its equivalent "BODGY" and it is, but it is the
//! supported path and the alternative is a button that silently does nothing.
//!
//! What it buys is everything else for free. `decorations: false` in Tauri does not make a
//! bare popup: `tao` handles `WM_NCCALCSIZE` so the window keeps `WS_THICKFRAME`, which means
//! resize borders, Aero Snap, Win+arrow, shake, Alt+Tab, the taskbar and (with `shadow: true`)
//! Windows 11's rounded corners and drop shadow all still work without a line of code here.
//! Dragging and double-click-to-maximise come from `data-tauri-drag-region` in the page.
//!
//! Two things are still lost and are not worth what they would cost to recover: the minimise
//! and close buttons have no UIA names (they are HTML buttons with `aria-label`s, which is a
//! different accessibility story, not an absent one), and Alt+Space is at the mercy of what
//! the webview does with the keystroke. The system menu is reachable by right-clicking the
//! bar, which is where most people look for it.

use std::sync::Mutex;

use tauri::{Emitter, Manager, Runtime};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH, NULL_BRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMenu, PostMessageW,
    RegisterClassExW, SetWindowPos, TrackPopupMenu, CS_HREDRAW, CS_VREDRAW, HTMAXBUTTON, HWND_TOP,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_DPICHANGED,
    WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE,
    WM_SIZE, WM_SYSCOMMAND, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};

/// How tall the bar is, in logical pixels.
///
/// Windows' own caption is 32px at 100%, and matching it is the point: a bar that is taller or
/// shorter than every other window's reads as a web page pretending, which is the failure mode
/// this whole approach is trying to avoid.
pub const TITLEBAR_HEIGHT: f64 = 32.0;

/// How wide one caption button is, in logical pixels. Windows' own is 46.
///
/// Wide for this app's other controls, and deliberately not reconciled with them: these three
/// are the OS's vocabulary, not ours, and a reader's muscle memory for the close button is
/// measured from the window's corner.
pub const CAPTION_BUTTON_WIDTH: f64 = 46.0;

/// How many caption buttons sit to the **right** of maximise.
///
/// One: close. The overlay's position is measured leftward from the window's right edge, so
/// only what is to the right of the maximise button enters the arithmetic — the "keep on top"
/// pin sits to its left and costs this nothing.
pub const CAPTION_BUTTONS_RIGHT_OF_MAX: i32 = 1;

/// The pointer entered the maximise button. Sent because the page cannot see it happen.
pub const MAX_ENTER: &str = "titlebar://max-enter";
/// The pointer left it, or the press was abandoned.
pub const MAX_LEAVE: &str = "titlebar://max-leave";
/// The button was pressed and is still held.
pub const MAX_DOWN: &str = "titlebar://max-down";
/// A press and release landed on the button: maximise or restore.
pub const MAX_CLICK: &str = "titlebar://max-click";

/// The window class the overlay is created from. Registered once, on first install.
const OVERLAY_CLASS: PCWSTR = w!("SakatsuSnapOverlay");
/// Ours, and arbitrary — it only has to be unique among subclasses of the same window.
const SUBCLASS_ID: usize = 0x5341_4b41; // "SAKA"

/// The live overlay, of which there is exactly one: the main window's.
///
/// A plain `Option` rather than a map keyed on the window, because the capture frame must
/// never get one — it is a window you look *through*, and a caption button on it would be a
/// control surface on the one thing that cannot be one.
static OVERLAY: Mutex<Option<Overlay>> = Mutex::new(None);

struct Overlay {
    /// The child window that answers the hit test. Stored as an integer because `HWND` is a
    /// raw pointer and therefore not `Send`; it is rebuilt at the point of use.
    hwnd: isize,
    /// The window it belongs to, for the same reason.
    parent: isize,
    hovering: bool,
    pressing: bool,
    /// How the page is told what the pointer did.
    ///
    /// An event rather than a call back into the window: this runs inside a window procedure,
    /// and asking the window to maximise from there re-enters the message loop underneath a
    /// message that has not finished being handled.
    notify: Box<dyn Fn(&'static str) + Send>,
}

/// Give the main window's maximise button its hit test.
///
/// Safe to call when there is no main window, and safe to call twice — the second call
/// replaces the first, which is what a window being rebuilt would need.
pub fn install<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(window) = app.get_webview_window(crate::commands::MAIN_LABEL) else {
        return;
    };
    let Ok(handle) = window.hwnd() else {
        return;
    };
    let parent = handle.0 as isize;

    let notify = {
        let window = window.clone();
        Box::new(move |event: &'static str| {
            let _ = window.emit(event, ());
        })
    };

    // Inside `setup`, so this already *is* the main thread — but `run_on_main_thread` states
    // the requirement rather than relying on the caller to have honoured it. Creating a child
    // window from another thread would attach its message queue to that thread, and the
    // overlay would stop answering the moment the thread parked.
    let _ = window.run_on_main_thread(move || unsafe {
        create(parent, notify);
    });
}

/// Take it down. Called when the window goes, so a rebuilt window does not inherit a handle
/// to a window that no longer exists.
unsafe fn uninstall(parent: isize) {
    let taken = OVERLAY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    let _ = RemoveWindowSubclass(HWND(parent as *mut _), Some(subclass_proc), SUBCLASS_ID);
    if let Some(overlay) = taken {
        let _ = DestroyWindow(HWND(overlay.hwnd as *mut _));
    }
}

unsafe fn create(parent: isize, notify: Box<dyn Fn(&'static str) + Send>) {
    register_class();

    let overlay = CreateWindowExW(
        Default::default(),
        OVERLAY_CLASS,
        PCWSTR::null(),
        // A child, and visible — an invisible window is not hit-tested, which would defeat
        // the only thing this exists for. It has no background brush, so "visible" costs no
        // pixels: what shows through is the HTML button underneath.
        WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
        0,
        0,
        0,
        0,
        Some(HWND(parent as *mut _)),
        None,
        None,
        None,
    );
    let Ok(overlay) = overlay else {
        // Nothing to recover: the page's button still works through ordinary HTML, and the
        // only thing lost is the flyout. Worth a line in the log rather than a panic.
        sakatsu_core::log_warn!("titlebar", "the snap-layout overlay could not be created");
        return;
    };

    *OVERLAY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Overlay {
        hwnd: overlay.0 as isize,
        parent,
        hovering: false,
        pressing: false,
        notify,
    });

    // The parent is subclassed for one job: telling the overlay where to be. Everything the
    // frame itself does — resizing, snapping, the shadow — is left to `tao` and Windows.
    let _ = SetWindowSubclass(HWND(parent as *mut _), Some(subclass_proc), SUBCLASS_ID, 0);
    reposition(parent);
}

unsafe fn register_class() {
    // `RegisterClassExW` fails harmlessly when the class is already registered, which is what
    // a second install does. The return value is deliberately dropped for that reason.
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(overlay_proc),
        hInstance: GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into(),
        // **`NULL_BRUSH`, and that is what keeps this invisible.** With a real brush the
        // overlay would erase its rectangle and punch a hole in the page exactly where the
        // maximise button is drawn. If a hole ever appears anyway on some composition path,
        // the next thing to try is `WS_EX_LAYERED` with `SetLayeredWindowAttributes(…, 0,
        // LWA_ALPHA)` — which stays hit-testable, where `WS_EX_TRANSPARENT` would not.
        hbrBackground: HBRUSH(GetStockObject(NULL_BRUSH).0),
        lpszClassName: OVERLAY_CLASS,
        ..Default::default()
    };
    RegisterClassExW(&class);
}

/// Put the overlay back over the maximise button.
unsafe fn reposition(parent: isize) {
    let overlay = {
        let held = OVERLAY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match held.as_ref() {
            Some(overlay) if overlay.parent == parent => overlay.hwnd,
            _ => return,
        }
    };

    let parent = HWND(parent as *mut _);
    let mut client = Default::default();
    if GetClientRect(parent, &mut client).is_err() {
        return;
    }

    // **The client rect, not the screen.** A maximised window is deliberately a little larger
    // than the monitor — the resize handles are pushed off-screen — so button positions
    // computed from screen coordinates drift the moment the window is maximised. Measuring
    // inside the parent is immune to that, and to which monitor it is on.
    let (x, y, width, height) = button_rect(client.right - client.left, GetDpiForWindow(parent));

    let _ = SetWindowPos(
        HWND(overlay as *mut _),
        Some(HWND_TOP),
        x,
        y,
        width,
        height,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
}

/// Where the maximise button sits inside a window this wide, in physical pixels.
///
/// Pure, so the arithmetic can be tested at every scale — which matters because it is the
/// part that fails quietly. An overlay a few pixels out does not error; it puts the flyout on
/// the close button, or on nothing.
fn button_rect(client_width: i32, dpi: u32) -> (i32, i32, i32, i32) {
    let scale = |logical: f64| ((logical * f64::from(dpi)) / 96.0).round() as i32;
    let width = scale(CAPTION_BUTTON_WIDTH).max(1);
    let height = scale(TITLEBAR_HEIGHT).max(1);
    // Leftward from the right edge, one button at a time: close is rightmost, maximise next.
    let x = client_width - width * (CAPTION_BUTTONS_RIGHT_OF_MAX + 1);
    (x.max(0), 0, width, height)
}

/// The parent's subclass, whose whole job is telling the overlay where to be.
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if let WM_SIZE | WM_DPICHANGED = message {
        // Both, and not just `WM_SIZE`: dragging the window to a monitor at a different scale
        // changes the button's size in physical pixels without changing the window's logical
        // size, so a handler for one is a handler for half the cases.
        reposition(hwnd.0 as isize);
    }

    let result = DefSubclassProc(hwnd, message, wparam, lparam);

    // **After the chain, and on `WM_NCDESTROY` rather than `WM_DESTROY`.** Removing a subclass
    // unhooks the procedure messages are still arriving through, so it has to happen once
    // nothing more can arrive — `WM_NCDESTROY` is the last message a window is sent — and
    // after `DefSubclassProc` has been given this one, not before it.
    if message == WM_NCDESTROY {
        uninstall(hwnd.0 as isize);
    }
    result
}

/// The overlay's own procedure. Small on purpose: it answers one question and reports what
/// the pointer did.
unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // **The whole reason this window exists.** Every pixel of it is the maximise button,
        // so there is no rectangle to test against: the window *is* the rectangle.
        WM_NCHITTEST => return LRESULT(HTMAXBUTTON as isize),

        WM_NCMOUSEMOVE => {
            act(|overlay| {
                // Only the crossing is worth an event. This message arrives on every pixel of
                // movement, and a `max-enter` per pixel would be a stream of IPC that says the
                // same thing the first one did.
                let entered = !overlay.hovering;
                overlay.hovering = true;
                entered.then_some(MAX_ENTER)
            });
            // Non-client tracking, or the leave never arrives: `TME_LEAVE` alone reports
            // leaving the *client* area, and this window has none the pointer can be in.
            let mut track = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE | TME_NONCLIENT,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            let _ = TrackMouseEvent(&mut track);
            return LRESULT(0);
        }

        WM_NCMOUSELEAVE => {
            act(|overlay| {
                overlay.hovering = false;
                // A press abandoned by leaving the button is not a click, and the page has to
                // be told to let go of the pressed state it is drawing.
                overlay.pressing = false;
                Some(MAX_LEAVE)
            });
            return LRESULT(0);
        }

        WM_NCLBUTTONDOWN => {
            act(|overlay| {
                overlay.pressing = true;
                Some(MAX_DOWN)
            });
            return LRESULT(0);
        }

        WM_NCLBUTTONUP => {
            act(|overlay| {
                // A release with no press behind it is not a click — it is the tail of a drag
                // that started somewhere else and happened to end here.
                let clicked = overlay.pressing;
                overlay.pressing = false;
                Some(if clicked { MAX_CLICK } else { MAX_LEAVE })
            });
            return LRESULT(0);
        }

        _ => {}
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

/// Change what the pointer is doing and tell the page, under one lock.
///
/// The two are one operation on purpose. Taking the lock to update the state, dropping it and
/// taking it again to send is how two pointer messages arriving close together get reported in
/// the wrong order — and the page is drawing hover and press from exactly this sequence.
///
/// The emit happens with the lock held, which is safe rather than merely convenient:
/// `Emitter::emit` serialises and queues for the webview and returns, and nothing on that path
/// comes back to this mutex. The alternative would be cloning a boxed callback out of the
/// struct on every mouse move.
///
/// Returning `None` from `change` means "nothing to say" — the pointer moved inside a button it
/// was already inside, which happens on every mouse move and must not be an event.
fn act(change: impl FnOnce(&mut Overlay) -> Option<&'static str>) {
    let mut held = OVERLAY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(overlay) = held.as_mut() {
        if let Some(event) = change(overlay) {
            (overlay.notify)(event);
        }
    }
}

/// Show the window's own system menu — Restore, Move, Size, Minimise, Maximise, Close.
///
/// Reachable by right-clicking the bar, because with no native caption `DefWindowProc` has
/// nowhere to put it. Its absence is a small thing that reads as a large one: it is how a
/// window says it is a real window.
///
/// `TrackPopupMenu` is modal — it runs its own message loop until something is chosen — so
/// this must happen on the thread that owns the window, and the chosen command is posted
/// rather than sent, so the menu is gone before the window acts on it.
pub fn system_menu<R: Runtime>(window: &tauri::WebviewWindow<R>, x: i32, y: i32) {
    let Ok(handle) = window.hwnd() else {
        return;
    };
    let hwnd = handle.0 as isize;
    let _ = window.run_on_main_thread(move || unsafe {
        let hwnd = HWND(hwnd as *mut _);
        let menu = GetSystemMenu(hwnd, false);
        if menu.is_invalid() {
            return;
        }
        // `TPM_RETURNCMD` makes the return value the chosen command rather than a success
        // flag — which is why it is read as an integer here and not as a `bool`. Zero means
        // the menu was dismissed.
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_LEFTALIGN | TPM_RIGHTBUTTON,
            x,
            y,
            None,
            hwnd,
            None,
        );
        if chosen.0 != 0 {
            // Posted, not sent: the menu's own modal loop is still unwinding, and asking the
            // window to close itself from inside it is asking to be destroyed mid-message.
            let _ = PostMessageW(
                Some(hwnd),
                WM_SYSCOMMAND,
                WPARAM(chosen.0 as usize),
                LPARAM(0),
            );
        }
    });
}

/// Where the pointer was, in physical screen pixels, for [`system_menu`].
///
/// Kept separate from the command so the conversion has one home: the page reports CSS pixels
/// and `TrackPopupMenu` wants screen pixels, and getting that wrong puts the menu on the wrong
/// monitor rather than failing.
pub fn to_screen<R: Runtime>(window: &tauri::WebviewWindow<R>, x: f64, y: f64) -> (i32, i32) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let origin = window
        .outer_position()
        .map(|at| (at.x, at.y))
        .unwrap_or((0, 0));
    (
        origin.0 + (x * scale).round() as i32,
        origin.1 + (y * scale).round() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The geometry, at the three scales a Windows machine is actually set to.
    ///
    /// This is the part that fails silently: an overlay a few pixels out does not error, it
    /// puts the flyout over the close button — or over nothing, which looks exactly like the
    /// feature never having been implemented.
    #[test]
    fn the_overlay_covers_the_maximise_button_at_every_scale() {
        // 900px window at 100%: close occupies the last 46px, maximise the 46 before it.
        let (x, y, width, height) = button_rect(900, 96);
        assert_eq!((x, y), (900 - 92, 0));
        assert_eq!((width, height), (46, 32));

        // 150%. Everything scales, including where it starts from.
        let (x, _, width, height) = button_rect(1350, 144);
        assert_eq!((width, height), (69, 48));
        assert_eq!(x, 1350 - 138);

        // 200%.
        let (x, _, width, height) = button_rect(1800, 192);
        assert_eq!((width, height), (92, 64));
        assert_eq!(x, 1800 - 184);
    }

    /// A window narrower than its own caption buttons is not a crash.
    ///
    /// `FRAME_MIN_WIDTH` is 32px for the capture frame, and while the main window's floor is
    /// 480 there is nothing here that guarantees a `WM_SIZE` never arrives mid-animation with
    /// something absurd. A negative `x` would be passed to `SetWindowPos` as a position off
    /// the left edge, which is a hole in the page rather than an error.
    #[test]
    fn a_window_too_narrow_for_the_buttons_clamps_rather_than_going_negative() {
        let (x, _, _, _) = button_rect(20, 96);
        assert_eq!(x, 0);
        let (x, _, _, _) = button_rect(0, 192);
        assert_eq!(x, 0);
    }

    /// The maximise button is second from the right, and the arithmetic has to say so.
    ///
    /// Stated against the constant rather than against 2: adding a button to the right of
    /// maximise — there is no reason to, but Windows itself once had one — must move the
    /// overlay, and a test written with a literal would keep passing while it stopped
    /// covering anything.
    #[test]
    fn only_the_buttons_right_of_maximise_move_the_overlay() {
        let (x, _, width, _) = button_rect(1000, 96);
        assert_eq!(
            x + width * (CAPTION_BUTTONS_RIGHT_OF_MAX + 1),
            1000,
            "the overlay's right edge plus the buttons beyond it is the window's edge"
        );
    }
}
