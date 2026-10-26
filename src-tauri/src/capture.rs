//! Screen capture and the region-select overlay.
//!
//! The flow, and why it is this order (`project notes` §3):
//!
//! 1. The screen is captured **first**, before the overlay is shown. A transparent
//!    overlay shows the live desktop, so capturing afterwards would race the
//!    compositor and risk photographing the overlay itself. Freezing first also means
//!    the crop is exactly what was on screen when the hotkey was pressed.
//! 2. The overlay opens over that monitor: transparent, undecorated, always on top.
//! 3. The user drags a rectangle; the overlay reports it in **CSS pixels**.
//! 4. Those are scaled by the monitor's factor into the physical pixels a screenshot
//!    is measured in, and the frozen image is cropped.
//!
//! Step 4 is the trap: on a 150% display a 100-pixel drag covers 150 image pixels, and
//! skipping the conversion silently crops the wrong region on every non-100% monitor.

use sakatsu_core::ocr::OcrImage;

/// A frozen screen, and what is needed to interpret coordinates against it.
pub struct Screenshot {
    pub image: OcrImage,
    /// The monitor's top-left in desktop coordinates, physical pixels.
    pub origin: (i32, i32),
    /// Physical pixels per CSS pixel on this monitor.
    pub scale_factor: f64,
}

impl Screenshot {
    /// Logical size, which is what a window covering this monitor must be built with.
    pub fn logical_size(&self) -> (f64, f64) {
        (
            f64::from(self.image.width()) / self.scale_factor,
            f64::from(self.image.height()) / self.scale_factor,
        )
    }

    /// Logical position, for placing that window.
    pub fn logical_position(&self) -> (f64, f64) {
        (
            f64::from(self.origin.0) / self.scale_factor,
            f64::from(self.origin.1) / self.scale_factor,
        )
    }
}

/// Capture the monitor containing the given desktop point, falling back to the primary
/// monitor when the point is off-screen (or no cursor position is available).
pub fn capture_monitor_at(point: Option<(i32, i32)>) -> Result<Screenshot, String> {
    let monitor = match point {
        Some((x, y)) => xcap::Monitor::from_point(x, y).or_else(|_| primary_monitor()),
        None => primary_monitor(),
    }
    .map_err(|error| format!("no monitor to capture: {error}"))?;

    let scale_factor = monitor
        .scale_factor()
        .map_err(|error| format!("could not read the display scale: {error}"))?;
    let origin = (
        monitor.x().map_err(display_error)?,
        monitor.y().map_err(display_error)?,
    );

    let shot = monitor
        .capture_image()
        .map_err(|error| format!("screen capture failed: {error}"))?;
    let (width, height) = (shot.width(), shot.height());
    let image = OcrImage::new(width, height, shot.into_raw()).map_err(|error| error.to_string())?;

    Ok(Screenshot {
        image,
        origin,
        // `scale_factor` is f32 in xcap; a zero would make every later division
        // nonsense, so it is treated as "unknown, assume 1:1".
        scale_factor: if scale_factor > 0.0 {
            f64::from(scale_factor)
        } else {
            1.0
        },
    })
}

/// A rectangle on the **virtual desktop**, in physical pixels.
///
/// **Signed, and that is the whole reason this type exists.** `ocr::Rect` is documented as "a
/// rectangle in image pixels" and its `x`/`y` are `u32`, which is right for a crop inside an
/// image — an offset into a buffer cannot be negative. It was also being used for a place on the
/// *desktop*, and on any arrangement with a monitor left of or above the primary, the virtual
/// desktop has negative coordinates.
///
/// What that cost: a frame on a monitor at x = -1080 produced `at (-313,22) → region (0,52)`. The
/// `u32` could not hold -313, a `.max(0)` clamped it to zero without complaint, and the capture
/// read the *primary* monitor's top-left corner instead — so the overlay sat over Japanese and the
/// app recognised whatever happened to be in the other screen's top-left. The recognition was
/// perfect; it was pointed at the wrong pixels.
///
/// The two roles are different things and now have different types. Nothing converts between them
/// implicitly: [`capture_desktop_rect`] turns desktop coordinates into monitor-relative ones, which
/// is the one place that subtraction belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Capture one rectangle of the desktop, in **physical** pixels.
///
/// This is what the persistent frame reads through. Unlike [`capture_monitor_at`], which
/// freezes a whole monitor before anything is shown over it, this grabs only the region
/// asked for, at the moment it is asked for — the frame is already on screen by then, and
/// stays there.
///
/// That the frame does not photograph itself is a property of the *frame*, not of this
/// function: its interior is fully transparent, so the desktop composites the window
/// underneath through it, and the rect passed here is inset past the frame's own border
/// and toolbar. See `commands::read_frame`.
///
/// `xcap` wants monitor-relative coordinates, so the monitor's origin is subtracted here.
/// Getting that wrong is invisible on a single 100% display and wrong on every other
/// arrangement, which is the trap this module exists to keep in one place.
pub fn capture_desktop_rect(rect: DesktopRect) -> Result<OcrImage, String> {
    let monitor = xcap::Monitor::from_point(rect.x, rect.y)
        .or_else(|_| primary_monitor())
        .map_err(|error| format!("no monitor holds that region: {error}"))?;

    let origin = (
        monitor.x().map_err(display_error)?,
        monitor.y().map_err(display_error)?,
    );
    let bounds = (
        monitor.width().map_err(display_error)?,
        monitor.height().map_err(display_error)?,
    );

    // Monitor-relative, and clipped to the monitor: `capture_region` fails outright on a
    // region that runs past the edge, and a frame dragged half off-screen should read the
    // half that is visible rather than report an error.
    //
    // **Off the top or left edge, the width has to lose what the origin gained.** Clamping the
    // origin to zero and keeping the full size does not crop the region, it *slides* it — a frame
    // 40px off the left edge would have read 40px of desktop that was never under it. Both edges
    // are handled, since a monitor above the primary makes the same thing happen vertically.
    let left_of_monitor = i64::from(rect.x) - i64::from(origin.0);
    let top_of_monitor = i64::from(rect.y) - i64::from(origin.1);
    let cut_left = (-left_of_monitor).max(0).min(i64::from(rect.width)) as u32;
    let cut_top = (-top_of_monitor).max(0).min(i64::from(rect.height)) as u32;

    let left = left_of_monitor.max(0) as u32;
    let top = top_of_monitor.max(0) as u32;
    let width = (rect.width - cut_left).min(bounds.0.saturating_sub(left));
    let height = (rect.height - cut_top).min(bounds.1.saturating_sub(top));

    if width == 0 || height == 0 {
        return Err("that region is off-screen".into());
    }

    let shot = monitor
        .capture_region(left, top, width, height)
        .map_err(|error| format!("screen capture failed: {error}"))?;
    let (width, height) = (shot.width(), shot.height());
    OcrImage::new(width, height, shot.into_raw()).map_err(|error| error.to_string())
}

fn primary_monitor() -> xcap::XCapResult<xcap::Monitor> {
    let monitors = xcap::Monitor::all()?;
    let primary = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .cloned();
    match primary {
        Some(monitor) => Ok(monitor),
        None => Err(xcap::XCapError::new("no monitors found")),
    }
}

fn display_error(error: xcap::XCapError) -> String {
    format!("could not read the display geometry: {error}")
}

/// Decode an image file's bytes, whatever format it is in.
///
/// The format is sniffed from the content rather than trusted from the file name: a
/// screenshot saved as `.jpg` that is really a PNG is common enough, and the user
/// should not have to care.
pub fn decode_image(bytes: &[u8]) -> Result<OcrImage, String> {
    if bytes.is_empty() {
        return Err("that file is empty".into());
    }
    let decoded = image::load_from_memory(bytes)
        .map_err(|error| format!("that does not look like an image: {error}"))?;
    let rgba = decoded.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    OcrImage::new(width, height, rgba.into_raw()).map_err(|error| error.to_string())
}

/// What was on the clipboard.
///
/// Text is returned as text rather than being rendered and recognized: copying
/// Japanese from a web page should not go through OCR to come back slightly wrong.
pub enum Clipboard {
    Text(String),
    Image(OcrImage),
}

/// Read the clipboard, preferring text.
pub fn read_clipboard() -> Result<Clipboard, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("no clipboard: {error}"))?;

    // Text first: it is exact, and an image of text is a poor substitute for it.
    if let Ok(text) = clipboard.get_text() {
        if !text.trim().is_empty() {
            return Ok(Clipboard::Text(text));
        }
    }

    match clipboard.get_image() {
        Ok(image) => {
            let (width, height) = (image.width as u32, image.height as u32);
            let rgba = image.bytes.into_owned();
            OcrImage::new(width, height, rgba)
                .map(Clipboard::Image)
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(format!("the clipboard holds no text or image: {error}")),
    }
}
