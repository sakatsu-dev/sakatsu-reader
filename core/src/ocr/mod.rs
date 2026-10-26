//! The [`Ocr`] trait and the image type that crosses it.
//!
//! Same shape as [`Analyzer`](crate::analyzer::Analyzer), and for the same reason:
//! recognition is done through one interface so the engine behind it is a startup
//! decision. manga-ocr and PP-OCRv6 run through ONNX, Auto routes conservatively between
//! them, and an optional VLM sits behind the same trait — the capture pipeline knows none
//! of those details.
//!
//! Images are plain RGBA bytes rather than an `image::RgbaImage`, so this crate stays
//! free of an imaging dependency: screen capture, the clipboard and image files all
//! produce RGBA already, and the engine that needs to resize can pull in whatever it
//! likes. Cropping lives here because it is arithmetic worth testing, not platform
//! work.

pub mod auto;
pub mod manga;
pub mod paddle;
pub mod preprocess;
pub mod score;
pub mod vlm;

pub use auto::{AutoOcr, AutoReason};
pub use manga::MangaOcr;
pub use paddle::PaddleOcr;
pub use preprocess::{Preprocess, PreprocessOptions};
pub use vlm::VlmOcr;

/// A rectangle in image pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Build a rectangle from two drag corners, in either order.
    ///
    /// A region select is two points, and nothing stops the user dragging up and to
    /// the left — so the corners are sorted rather than assumed.
    pub fn from_corners(x1: i64, y1: i64, x2: i64, y2: i64) -> Rect {
        let left = x1.min(x2).max(0);
        let top = y1.min(y2).max(0);
        let right = x1.max(x2).max(0);
        let bottom = y1.max(y2).max(0);
        Rect {
            x: left as u32,
            y: top as u32,
            width: (right - left) as u32,
            height: (bottom - top) as u32,
        }
    }

    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Scale by a display's factor, for converting the overlay's CSS pixels into the
    /// physical pixels a screenshot is measured in.
    pub fn scaled(self, factor: f64) -> Rect {
        let scale = |value: u32| ((value as f64) * factor).round() as u32;
        Rect {
            x: scale(self.x),
            y: scale(self.y),
            width: scale(self.width),
            height: scale(self.height),
        }
    }
}

/// A captured image: 8-bit RGBA, row-major, `width * height * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl OcrImage {
    /// Wrap raw RGBA bytes, checking that they describe the stated size.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<OcrImage, OcrError> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| OcrError::MalformedImage(format!("{width}x{height} overflows")))?;
        if rgba.len() != expected {
            return Err(OcrError::MalformedImage(format!(
                "{width}x{height} needs {expected} bytes, got {}",
                rgba.len()
            )));
        }
        Ok(OcrImage {
            width,
            height,
            rgba,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Crop to `rect`, clamped to the image.
    ///
    /// Clamped rather than rejected: a drag that runs off the edge of the screen is
    /// an ordinary thing to do, and the useful response is the part that overlaps.
    /// `None` only when nothing overlaps at all.
    pub fn crop(&self, rect: Rect) -> Option<OcrImage> {
        let left = rect.x.min(self.width);
        let top = rect.y.min(self.height);
        let right = rect.x.saturating_add(rect.width).min(self.width);
        let bottom = rect.y.saturating_add(rect.height).min(self.height);

        let (width, height) = (right.saturating_sub(left), bottom.saturating_sub(top));
        if width == 0 || height == 0 {
            return None;
        }

        let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for row in top..bottom {
            let start = ((row as usize) * (self.width as usize) + (left as usize)) * 4;
            let end = start + (width as usize) * 4;
            rgba.extend_from_slice(&self.rgba[start..end]);
        }

        Some(OcrImage {
            width,
            height,
            rgba,
        })
    }
}

/// Anything that turns an image into text.
///
/// `Send + Sync` so the desktop shell can hold one behind shared state, exactly as it
/// holds the analyzer.
pub trait Ocr: Send + Sync {
    fn recognize(&self, image: &OcrImage) -> Result<String, OcrError>;

    /// A short identifier for the engine, e.g. `"manga-ocr"`.
    fn engine(&self) -> &str;

    /// Recognize, and say which engine produced the text.
    ///
    /// The same as [`Ocr::engine`] for an engine that is one model. Auto is two, and which one
    /// answered is decided per image, so it overrides this. A wrapper must forward it, or Auto
    /// behind the wrapper would report only "auto".
    fn recognize_attributed(&self, image: &OcrImage) -> Result<(String, String), OcrError> {
        let text = self.recognize(image)?;
        Ok((text, self.engine().to_string()))
    }
}

/// So a shared engine can be wrapped by another one.
///
/// The shell holds its engine as `Arc<dyn Ocr>` — loading is expensive and the lock has
/// to be released before inference runs. Without this, [`preprocess::Preprocess`] could
/// not be put in front of it, because its inner type has to implement the trait too.
impl<T: Ocr + ?Sized> Ocr for std::sync::Arc<T> {
    fn recognize(&self, image: &OcrImage) -> Result<String, OcrError> {
        (**self).recognize(image)
    }

    fn engine(&self) -> &str {
        (**self).engine()
    }

    fn recognize_attributed(&self, image: &OcrImage) -> Result<(String, String), OcrError> {
        (**self).recognize_attributed(image)
    }
}

/// Why an image could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrError {
    /// Nothing to recognize — an empty selection.
    EmptyImage,
    /// The bytes do not describe the image they claim to.
    MalformedImage(String),
    /// The engine or its model is not installed.
    EngineUnavailable(String),
    /// The engine ran and failed.
    Failed(String),
}

impl std::fmt::Display for OcrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcrError::EmptyImage => write!(f, "nothing selected"),
            OcrError::MalformedImage(message) => write!(f, "unreadable image: {message}"),
            OcrError::EngineUnavailable(message) => write!(f, "OCR unavailable: {message}"),
            OcrError::Failed(message) => write!(f, "OCR failed: {message}"),
        }
    }
}

impl std::error::Error for OcrError {}

/// Grounds the trait before an engine exists, and stands in wherever one is missing.
///
/// It reports [`OcrError::EngineUnavailable`] rather than returning empty text, so the
/// UI says why nothing happened instead of looking broken.
pub struct StubOcr;

impl Ocr for StubOcr {
    fn recognize(&self, image: &OcrImage) -> Result<String, OcrError> {
        if image.is_empty() {
            return Err(OcrError::EmptyImage);
        }
        Err(OcrError::EngineUnavailable(
            "no recognition model is installed — see scripts/fetch-ocr-model.ps1".into(),
        ))
    }

    fn engine(&self) -> &str {
        "stub"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3x2 image whose pixels are identifiable by their red channel.
    fn grid() -> OcrImage {
        let mut rgba = Vec::new();
        for row in 0..2u8 {
            for column in 0..3u8 {
                rgba.extend_from_slice(&[row * 10 + column, 0, 0, 255]);
            }
        }
        OcrImage::new(3, 2, rgba).expect("valid")
    }

    fn reds(image: &OcrImage) -> Vec<u8> {
        image.rgba().chunks(4).map(|pixel| pixel[0]).collect()
    }

    #[test]
    fn a_rectangle_is_built_from_corners_dragged_in_any_direction() {
        let expected = Rect {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        };
        // Down-right, and the same drag done up-left.
        assert_eq!(Rect::from_corners(10, 20, 40, 60), expected);
        assert_eq!(Rect::from_corners(40, 60, 10, 20), expected);
        // Negative coordinates happen when a drag leaves the window.
        assert_eq!(
            Rect::from_corners(-5, -5, 10, 10),
            Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10
            }
        );
        assert!(Rect::from_corners(5, 5, 5, 5).is_empty());
    }

    #[test]
    fn a_rectangle_scales_to_physical_pixels() {
        // The trap this exists for: the overlay measures in CSS pixels, a screenshot
        // in physical ones, and on a 150% display they are not the same.
        let logical = Rect {
            x: 10,
            y: 20,
            width: 100,
            height: 50,
        };
        assert_eq!(
            logical.scaled(1.5),
            Rect {
                x: 15,
                y: 30,
                width: 150,
                height: 75
            }
        );
        assert_eq!(logical.scaled(1.0), logical);
    }

    #[test]
    fn an_image_must_match_the_size_it_claims() {
        assert!(OcrImage::new(2, 2, vec![0; 16]).is_ok());
        let error = OcrImage::new(2, 2, vec![0; 15]).expect_err("too short");
        assert!(matches!(error, OcrError::MalformedImage(_)));
        assert!(error.to_string().contains("needs 16 bytes"));
    }

    #[test]
    fn cropping_takes_exactly_the_requested_pixels() {
        let cropped = grid()
            .crop(Rect {
                x: 1,
                y: 0,
                width: 2,
                height: 2,
            })
            .expect("overlaps");
        assert_eq!((cropped.width(), cropped.height()), (2, 2));
        // Columns 1 and 2 of both rows: 1,2 then 11,12.
        assert_eq!(reds(&cropped), [1, 2, 11, 12]);
    }

    #[test]
    fn a_crop_running_off_the_edge_is_clamped_rather_than_refused() {
        // Dragging past the edge of the screen is ordinary; the useful answer is the
        // part that overlaps.
        let cropped = grid()
            .crop(Rect {
                x: 2,
                y: 1,
                width: 100,
                height: 100,
            })
            .expect("overlaps");
        assert_eq!((cropped.width(), cropped.height()), (1, 1));
        assert_eq!(reds(&cropped), [12]);
    }

    #[test]
    fn a_crop_that_overlaps_nothing_is_none() {
        let image = grid();
        assert!(image
            .crop(Rect {
                x: 3,
                y: 0,
                width: 5,
                height: 5
            })
            .is_none());
        assert!(image
            .crop(Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 5
            })
            .is_none());
    }

    #[test]
    fn the_stub_explains_itself_rather_than_returning_empty_text() {
        // Empty text would look like a failed recognition; this says why.
        let error = StubOcr.recognize(&grid()).expect_err("no engine");
        assert!(matches!(error, OcrError::EngineUnavailable(_)));
        assert!(error.to_string().contains("model"));

        let empty = OcrImage::new(0, 0, Vec::new()).expect("valid");
        assert_eq!(StubOcr.recognize(&empty), Err(OcrError::EmptyImage));
        assert_eq!(StubOcr.engine(), "stub");
    }
}
