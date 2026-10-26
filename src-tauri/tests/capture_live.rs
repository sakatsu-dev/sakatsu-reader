//! Screen capture against the real display.
//!
//! Ignored by default: it needs a desktop session, which CI does not have. Run:
//!
//! ```text
//! cargo test -p sakatsu-desktop --test capture_live -- --ignored --nocapture
//! ```
//!
//! What this checks that a unit test cannot: that capture works on this machine at
//! all, and what the display's scale factor actually is — the number the whole
//! CSS-pixels-to-image-pixels conversion depends on.

use sakatsu_core::ocr::Rect;
use sakatsu_desktop_lib::capture;

#[test]
#[ignore = "needs a desktop session"]
fn the_screen_captures_and_crops() {
    let screenshot = capture::capture_monitor_at(None).expect("capture the primary monitor");

    let (width, height) = (screenshot.image.width(), screenshot.image.height());
    println!("captured {width}x{height} physical pixels");
    println!("origin      = {:?}", screenshot.origin);
    println!("scale factor= {}", screenshot.scale_factor);
    println!("logical size= {:?}", screenshot.logical_size());

    assert!(width > 0 && height > 0, "an empty screenshot is a failure");
    assert_eq!(
        screenshot.image.rgba().len(),
        (width as usize) * (height as usize) * 4,
        "RGBA bytes must match the stated size"
    );
    assert!(
        screenshot.scale_factor > 0.0,
        "a zero scale factor would make every conversion nonsense"
    );

    // A selection in the overlay's CSS pixels, converted the way `finish_capture`
    // converts it, must land inside the image.
    let selection = Rect {
        x: 10,
        y: 10,
        width: 100,
        height: 50,
    };
    let region = selection.scaled(screenshot.scale_factor);
    println!("selection {selection:?} → image region {region:?}");

    let cropped = screenshot
        .image
        .crop(region)
        .expect("a region near the top-left must overlap the screen");
    assert_eq!(cropped.width(), region.width);
    assert_eq!(cropped.height(), region.height);

    // And the whole screen crops to itself, which catches an off-by-one in the row
    // arithmetic that a small crop would not.
    let whole = screenshot
        .image
        .crop(Rect {
            x: 0,
            y: 0,
            width,
            height,
        })
        .expect("the whole screen overlaps itself");
    assert_eq!(whole.rgba(), screenshot.image.rgba());
}
