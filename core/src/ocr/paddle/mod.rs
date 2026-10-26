//! PP-OCRv6 (small): detect the text lines in a crop, then read each one.
//!
//! # Why this is a pipeline and manga-ocr is not
//!
//! manga-ocr is one model — a crop goes in, a string comes out. PP-OCR is a detector and a
//! recogniser, and the recogniser only reads **one line at a time**. So this finds the lines
//! inside the crop, reads them separately, and joins them. That is why there is real code here
//! rather than two tensor calls.
//!
//! The two engines are complementary rather than ranked. manga-ocr is trained on manga and
//! handles vertical text, furigana and hand lettering; PP-OCR is trained on everyday printed
//! text and does better on the typeset text of a game or a menu. Both remain direct settings;
//! Auto uses this pipeline's direction and rejected-line evidence for a conservative route
//! between them.
//!
//! # PP-OCRv6 replaced PP-OCRv5 on 2026-10-09
//!
//! On the 134 reviewed development captures v6 small read with 0.115 mean character error
//! against v5's 0.248 — better on 90 captures, worse on 15 — in a third of the time
//! (`local documentation`). v5 is no longer fetched or loaded.
//!
//! # Run as PaddleOCR runs it
//!
//! PaddlePaddle publishes v6 in Paddle format only; this reads an ONNX conversion of the
//! official weights, checked against PaddlePaddle's own inference (probabilities within 0.0006).
//! Every choice below was read from PaddleOCR 3.7's source rather than assumed, and the pipeline
//! as a whole was compared with PaddleOCR's own on the capture corpus:
//!
//! - **BGR input.** Both models' `inference.yml` decodes images as BGR. PP-OCRv5 was fed RGB here
//!   for as long as it shipped.
//! - **The dictionary's blank line.** The ONNX export's dictionary writes the CTC blank as an
//!   empty first line, which the official dictionary does not; it is stripped, exactly once.
//! - **Box scoring.** A detected region whose mean probability is under [`BOX_THRESHOLD`] is
//!   dropped, as PaddleOCR's `box_thresh` drops it.
//! - **The crop.** Each line's rotated box is warped flat, as `get_rotate_crop_image` does — the
//!   upright box with a margin that v5 read is only the fallback for a quad too thin to warp.
//! - **Recognition width.** Each line is padded to at least [`REC_MIN_WIDTH`] pixels.
//! - **Thresholds.** PaddleOCR's own pipeline defaults for v6 (`OCR.yaml`: 0.3, 0.6, 1.5), which
//!   differ from the model file's (0.2, 0.45, 1.4); the pipeline is what PaddleOCR itself runs.
//!
//! # Shapes, measured rather than assumed
//!
//! ```text
//! det  x → [N, 3, H, W]        fetch_name_0 → [N, 1, H, W]   probability map
//! rec  x → [N, 3, 48, W]       fetch_name_0 → [N, T, 18710]  class logits
//! ```
//!
//! The recogniser's height is fixed at 48 and its width is free, so a line is scaled to that
//! height. The 18710 classes against a dictionary of 18708 characters is PaddleOCR's
//! `blank + dict + space`; see [`decode::Charset`], which refuses a pair of files that do not
//! agree rather than let them decode confidently into the wrong characters.

mod decode;
mod geometry;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ort::session::Session;
use ort::value::TensorRef;

use super::{Ocr, OcrError, OcrImage};
use decode::Charset;
pub(crate) use geometry::ReadingDirection;
use geometry::{
    is_vertical, min_area_rect, paddle_corners, reading_order, recognition_box, unclip,
    warped_crop, Point,
};

/// The engine name results are labelled with.
pub const ENGINE: &str = "pp-ocrv6";

/// The character dictionary beside `det.onnx` and `rec.onnx`.
pub const DICTIONARY: &str = "ppocrv6_dict.txt";

/// The recogniser's fixed input height.
const REC_HEIGHT: u32 = 48;

/// Each line is padded to at least this width, as PaddleOCR's `RecResizeImg` pads it.
const REC_MIN_WIDTH: u32 = 320;

/// The widest a line is read at, PaddleOCR's `max_imgW`.
const REC_MAX_WIDTH: u32 = 3200;

/// Detection runs at a multiple of 32, because the backbone downsamples five times.
const DET_STRIDE: u32 = 32;

/// The longest side the detector sees. Bigger finds smaller text and costs time
/// quadratically; 960 is PaddleOCR's own default and a sensible place to stay.
const DET_LIMIT: u32 = 960;

/// Above this, a pixel of the probability map is text.
const DET_THRESHOLD: f32 = 0.3;

/// A region whose mean probability is below this is dropped (PaddleOCR's `box_thresh`).
const BOX_THRESHOLD: f32 = 0.6;

/// How far to grow each detected box. PaddleOCR's default.
const UNCLIP_RATIO: f32 = 1.5;

/// A line the recogniser was this unsure of is dropped rather than shown.
const MIN_CONFIDENCE: f32 = 0.5;

type TextBox = (u32, u32, u32, u32);

/// A detected line: its upright box, which orders and reports it, and the rotated quadrilateral
/// the crop is taken from.
type Detected = (TextBox, [Point; 4]);

pub struct PaddleOcr {
    detector: Mutex<Session>,
    recognizer: Mutex<Session>,
    charset: Charset,
}

/// What PP-OCR learned while reading one crop.
///
/// Auto OCR uses evidence the pipeline already produced rather than trying to identify
/// "manga" from the outer rectangle. `accepted_lines < detected_lines` means at least one
/// line fell below the recogniser's own confidence floor and is the only confidence signal strong
/// enough to trigger a fallback without inventing another threshold.
pub(crate) struct PaddleRead {
    pub text: String,
    pub direction: ReadingDirection,
    pub detected_lines: usize,
    pub accepted_lines: usize,
}

impl PaddleOcr {
    /// Whether all three files are in `dir`, without loading anything.
    ///
    /// All three, because a pipeline missing its dictionary is not a working engine and
    /// finding that out at the first capture would be worse than saying so up front. A
    /// directory still holding PP-OCRv5's files is not present: its dictionary has another name.
    pub fn is_present(dir: &Path) -> bool {
        ["det.onnx", "rec.onnx", DICTIONARY]
            .iter()
            .all(|name| dir.join(name).is_file())
    }

    /// Load the models from a directory holding `det.onnx`, `rec.onnx` and `ppocrv6_dict.txt`.
    pub fn open(dir: &Path) -> Result<PaddleOcr, OcrError> {
        let detector = session(&dir.join("det.onnx"))?;
        let recognizer = session(&dir.join("rec.onnx"))?;

        let keys = dir.join(DICTIONARY);
        let text = std::fs::read_to_string(&keys).map_err(|error| {
            OcrError::EngineUnavailable(format!(
                "{} is missing or unreadable ({error}) — run scripts/fetch-paddle-ocr.ps1",
                keys.display()
            ))
        })?;
        // Exactly one empty line, and it must be there: a dictionary without it is a different
        // file, and stripping a real first character would shift every index.
        let text = text
            .strip_prefix("\r\n")
            .or_else(|| text.strip_prefix('\n'))
            .ok_or_else(|| {
                OcrError::EngineUnavailable(format!(
                    "{} does not begin with the blank line the PP-OCRv6 export writes",
                    keys.display()
                ))
            })?;
        let charset = Charset::from_dictionary(text);

        // The dictionary and the model come from separate files and nothing else would
        // notice them disagreeing, so they are checked against each other once, here,
        // rather than producing plausible wrong text on every read.
        let classes = recognizer
            .outputs()
            .first()
            .and_then(|output| match output.dtype() {
                ort::value::ValueType::Tensor { shape, .. } => shape.last().copied(),
                _ => None,
            })
            .filter(|&classes| classes > 0)
            .ok_or_else(|| {
                OcrError::EngineUnavailable(
                    "the recognition model does not declare its class count".into(),
                )
            })?;
        charset.check(classes as usize)?;

        Ok(PaddleOcr {
            detector: Mutex::new(detector),
            recognizer: Mutex::new(recognizer),
            charset,
        })
    }

    /// The text boxes and each line's text, in reading order, for the parity harness.
    ///
    /// Boxes are in the source image's pixels as `(x, y, width, height)`; a line the recogniser
    /// dropped for low confidence is kept with its text and flagged, so a comparison against
    /// another implementation can see where the two disagree rather than only that they do.
    #[doc(hidden)]
    pub fn read_lines(&self, image: &OcrImage) -> Result<Vec<ReadLine>, OcrError> {
        let source = rgb(image)?;
        let (detected, _) = self.detect(&source)?;
        let mut lines = Vec::with_capacity(detected.len());
        for found in detected {
            let crop = line_crop(&source, &found);
            let (text, confidence) = self.read_line(&crop)?;
            lines.push(ReadLine {
                bounds: found.0,
                text,
                confidence,
            });
        }
        Ok(lines)
    }
}

/// The crop one line is read from: its rotated box warped flat, as PaddleOCR's
/// `get_rotate_crop_image` takes it, turned if it is at least half again as tall as it is wide.
///
/// The upright box with a small margin is the fallback, for a quad too thin to warp.
fn line_crop(source: &image::RgbImage, (found, quad): &Detected) -> image::RgbImage {
    if let Some(crop) = warped_crop(source, quad) {
        return if is_vertical(crop.width(), crop.height()) {
            image::imageops::rotate270(&crop)
        } else {
            crop
        };
    }
    let (x, y, width, height) = *found;
    let (crop_x, crop_y, crop_width, crop_height) =
        recognition_box((x, y, width, height), source.dimensions());
    let crop =
        image::imageops::crop_imm(source, crop_x, crop_y, crop_width, crop_height).to_image();
    // A vertical line read as a horizontal one is noise, so it is turned first.
    if is_vertical(width, height) {
        image::imageops::rotate270(&crop)
    } else {
        crop
    }
}

impl PaddleOcr {
    /// The evidence Auto routes on, for the harness that tunes it: the text, whether the passage
    /// reads vertically, and how many lines were detected and accepted.
    #[doc(hidden)]
    pub fn routing_evidence(
        &self,
        image: &OcrImage,
    ) -> Result<(String, bool, usize, usize), OcrError> {
        let read = self.recognize_detailed(image)?;
        Ok((
            read.text,
            read.direction == ReadingDirection::Vertical,
            read.detected_lines,
            read.accepted_lines,
        ))
    }
}

/// One detected line, for [`PaddleOcr::read_lines`].
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct ReadLine {
    pub bounds: (u32, u32, u32, u32),
    pub text: String,
    pub confidence: f32,
}

fn rgb(image: &OcrImage) -> Result<image::RgbImage, OcrError> {
    let source = image::RgbaImage::from_raw(image.width(), image.height(), image.rgba().to_vec())
        .ok_or_else(|| OcrError::MalformedImage("could not read the pixels".into()))?;
    Ok(image::DynamicImage::ImageRgba8(source).to_rgb8())
}

/// Channel `index` of a pixel in the BGR order the models were trained on.
fn bgr(pixel: &image::Rgb<u8>, index: usize) -> f32 {
    f32::from(pixel[2 - index])
}

fn session(path: &PathBuf) -> Result<Session, OcrError> {
    if !path.is_file() {
        return Err(OcrError::EngineUnavailable(format!(
            "{} is missing — run scripts/fetch-paddle-ocr.ps1",
            path.display()
        )));
    }
    crate::onnx::session_builder()
        .and_then(|mut builder| builder.commit_from_file(path))
        .map_err(|error| {
            OcrError::EngineUnavailable(format!("could not load {}: {error}", path.display()))
        })
}

fn failed(error: ort::Error) -> OcrError {
    OcrError::Failed(error.to_string())
}

/// The size the detector sees: the image scaled to fit `DET_LIMIT`, rounded to a
/// multiple of the backbone's stride.
fn detection_size(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height) as f32;
    let scale = if longest > DET_LIMIT as f32 {
        DET_LIMIT as f32 / longest
    } else {
        1.0
    };
    let round =
        |value: f32| ((value * scale / DET_STRIDE as f32).round().max(1.0) as u32) * DET_STRIDE;
    (round(width as f32), round(height as f32))
}

impl Ocr for PaddleOcr {
    fn recognize(&self, image: &OcrImage) -> Result<String, OcrError> {
        Ok(self.recognize_detailed(image)?.text)
    }

    fn engine(&self) -> &str {
        ENGINE
    }
}

impl PaddleOcr {
    pub(crate) fn recognize_detailed(&self, image: &OcrImage) -> Result<PaddleRead, OcrError> {
        if image.is_empty() {
            return Err(OcrError::EmptyImage);
        }

        let _timing = crate::log::Timing::start(
            "paddle",
            format!("recognising {}x{}", image.width(), image.height()),
        );

        let source = rgb(image)?;

        let (detected, direction) = self.detect(&source)?;
        let detected_lines = detected.len();
        if detected.is_empty() {
            return Ok(PaddleRead {
                text: String::new(),
                direction,
                detected_lines: 0,
                accepted_lines: 0,
            });
        }

        let mut lines = Vec::with_capacity(detected.len());
        for found in &detected {
            let crop = line_crop(&source, found);
            let (text, confidence) = self.read_line(&crop)?;
            if !text.trim().is_empty() && confidence >= MIN_CONFIDENCE {
                lines.push(text);
            }
        }

        let accepted_lines = lines.len();
        Ok(PaddleRead {
            text: lines.join("\n"),
            direction,
            detected_lines,
            accepted_lines,
        })
    }

    /// Find the text lines, as upright boxes in the source image's own pixels.
    fn detect(
        &self,
        source: &image::RgbImage,
    ) -> Result<(Vec<Detected>, ReadingDirection), OcrError> {
        let (source_width, source_height) = (source.width(), source.height());
        let (width, height) = detection_size(source_width, source_height);
        let resized =
            image::imageops::resize(source, width, height, image::imageops::FilterType::Triangle);

        // NCHW, ImageNet normalization — PaddleOCR's detection preprocessing.
        const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
        const STD: [f32; 3] = [0.229, 0.224, 0.225];
        let pixels = (width * height) as usize;
        let mut values = Vec::with_capacity(pixels * 3);
        for index in 0..3 {
            values.extend(
                resized
                    .pixels()
                    .map(|pixel| (bgr(pixel, index) / 255.0 - MEAN[index]) / STD[index]),
            );
        }

        let map = {
            let mut session = self
                .detector
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let input = TensorRef::<f32>::from_array_view((
                vec![1i64, 3, height as i64, width as i64],
                values.as_slice(),
            ))
            .map_err(failed)?;
            let outputs = session.run(ort::inputs!["x" => input]).map_err(failed)?;
            let (_, data) = outputs[0].try_extract_tensor::<f32>().map_err(failed)?;
            data.to_vec()
        };

        // Back to the source image's scale, since the crop happens there.
        let scale_x = source_width as f32 / width as f32;
        let scale_y = source_height as f32 / height as f32;

        let mut mask = image::GrayImage::new(width, height);
        for (index, &probability) in map.iter().take(pixels).enumerate() {
            if probability > DET_THRESHOLD {
                mask.put_pixel(
                    index as u32 % width,
                    index as u32 / width,
                    image::Luma([255]),
                );
            }
        }

        let mut boxes = Vec::new();
        let mut quads: Vec<Detected> = Vec::new();
        for contour in imageproc::contours::find_contours::<u32>(&mask) {
            // Holes inside a region are not text regions of their own.
            if contour.border_type == imageproc::contours::BorderType::Hole {
                continue;
            }
            // PaddleOCR's `box_thresh`: the region's mean probability, taken over its minimum
            // rectangle in the map's own pixels before any growing (`box_score_fast`).
            let in_map: Vec<Point> = contour
                .points
                .iter()
                .map(|point| Point::new(point.x as f32, point.y as f32))
                .collect();
            let Some(scored) = min_area_rect(&in_map) else {
                continue;
            };
            if box_score(&map, width, height, &scored.corners()) < BOX_THRESHOLD {
                continue;
            }
            let points: Vec<Point> = contour
                .points
                .iter()
                .map(|point| Point::new(point.x as f32 * scale_x, point.y as f32 * scale_y))
                .collect();

            let Some(rect) = min_area_rect(&points) else {
                continue;
            };
            // Detections are of a shrunk region and have to be grown back.
            let rect = unclip(rect, UNCLIP_RATIO);
            let Some(found) = rect.bounding_box((source_width, source_height)) else {
                continue;
            };
            // A box a couple of pixels across is a speck, not a line.
            if found.2 < 4 || found.3 < 4 {
                continue;
            }
            boxes.push(found);
            quads.push((found, paddle_corners(&rect, (source_width, source_height))));
        }

        let direction = reading_order(&mut boxes);
        // Each box takes its own quad back, in the reading order just decided.
        let detected: Vec<Detected> = boxes
            .iter()
            .filter_map(|found| {
                let at = quads.iter().position(|(other, _)| other == found)?;
                Some(quads.swap_remove(at))
            })
            .collect();
        // The count always, since "detection found nothing" explains an empty read and is
        // a different bug from "the recogniser produced nothing". The box geometry only
        // behind the env var, because that is a line per capture nobody wants by default.
        crate::log_info!(
            "detect",
            "{}x{} → {} boxes ({direction:?})",
            source_width,
            source_height,
            boxes.len()
        );
        if std::env::var_os("SAKATSU_OCR_DEBUG").is_some() {
            eprintln!(
                "det: {}x{} → {} boxes: {boxes:?}",
                source_width,
                source_height,
                boxes.len()
            );
            crate::log_info!("detect", "boxes: {boxes:?}");
        }
        Ok((detected, direction))
    }

    /// Read one line of text.
    fn read_line(&self, line: &image::RgbImage) -> Result<(String, f32), OcrError> {
        // Scaled to the fixed height, keeping the aspect so characters are not squashed.
        let scale = REC_HEIGHT as f32 / line.height() as f32;
        // PaddleOCR's `resize_norm_img`: the line at ceil(48 × aspect), placed at the left of a
        // canvas at least `REC_MIN_WIDTH` wide, the rest left at 0 — which after normalisation
        // is mid-grey rather than white.
        let width = ((line.width() as f32 * scale).ceil() as u32).clamp(1, REC_MAX_WIDTH);
        let padded = width.clamp(REC_MIN_WIDTH, REC_MAX_WIDTH);
        let resized = image::imageops::resize(
            line,
            width,
            REC_HEIGHT,
            image::imageops::FilterType::Triangle,
        );

        // Recognition normalizes to −1..1 rather than by ImageNet statistics.
        let pixels = (padded * REC_HEIGHT) as usize;
        let mut values = Vec::with_capacity(pixels * 3);
        for index in 0..3 {
            for row in resized.rows() {
                values.extend(row.map(|pixel| bgr(pixel, index) / 255.0 * 2.0 - 1.0));
                values.extend(std::iter::repeat_n(0.0, (padded - width) as usize));
            }
        }
        let width = padded;

        let (steps, classes, logits) = {
            let mut session = self
                .recognizer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let input = TensorRef::<f32>::from_array_view((
                vec![1i64, 3, REC_HEIGHT as i64, width as i64],
                values.as_slice(),
            ))
            .map_err(failed)?;
            let outputs = session.run(ort::inputs!["x" => input]).map_err(failed)?;
            let (shape, data) = outputs[0].try_extract_tensor::<f32>().map_err(failed)?;
            let steps = *shape.get(1).unwrap_or(&0) as usize;
            let classes = *shape.get(2).unwrap_or(&0) as usize;
            (steps, classes, data.to_vec())
        };

        if steps == 0 || classes == 0 {
            return Ok((String::new(), 0.0));
        }
        Ok(decode::greedy(&logits, classes, &self.charset))
    }
}

/// The mean of the probability map over a quadrilateral, as PaddleOCR's `box_score_fast` takes it:
/// over the pixels of the quad's bounding rectangle whose centres fall inside the quad.
fn box_score(map: &[f32], width: u32, height: u32, quad: &[Point; 4]) -> f32 {
    let clamp = |value: f32, limit: u32| value.clamp(0.0, (limit.max(1) - 1) as f32);
    let xs = quad.iter().map(|p| p.x);
    let ys = quad.iter().map(|p| p.y);
    let x0 = clamp(xs.clone().fold(f32::INFINITY, f32::min).floor(), width) as u32;
    let x1 = clamp(xs.fold(f32::NEG_INFINITY, f32::max).ceil(), width) as u32;
    let y0 = clamp(ys.clone().fold(f32::INFINITY, f32::min).floor(), height) as u32;
    let y1 = clamp(ys.fold(f32::NEG_INFINITY, f32::max).ceil(), height) as u32;

    // Inside a convex quad: on the same side of all four edges, either winding.
    let inside = |x: f32, y: f32| {
        let mut sign = 0.0f32;
        for i in 0..4 {
            let (a, b) = (quad[i], quad[(i + 1) % 4]);
            let cross = (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x);
            if cross.abs() <= f32::EPSILON {
                continue;
            }
            if sign == 0.0 {
                sign = cross.signum();
            } else if cross.signum() != sign {
                return false;
            }
        }
        true
    };

    let (mut total, mut count) = (0.0f64, 0usize);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if inside(x as f32, y as f32) {
                if let Some(&value) = map.get((y * width + x) as usize) {
                    total += f64::from(value);
                    count += 1;
                }
            }
        }
    }
    if count == 0 {
        0.0
    } else {
        (total / count as f64) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PaddleOCR's own pipeline values for v6 (`OCR.yaml`), not the model file's.
    #[test]
    fn the_thresholds_are_paddleocrs_pipeline_defaults() {
        assert_eq!(
            (DET_THRESHOLD, BOX_THRESHOLD, UNCLIP_RATIO),
            (0.3, 0.6, 1.5)
        );
        assert_eq!((REC_MIN_WIDTH, REC_MAX_WIDTH), (320, 3200));
    }

    /// The models were trained on BGR: channel 0 is blue.
    #[test]
    fn the_models_are_fed_bgr() {
        let pixel = image::Rgb([10u8, 20, 30]);
        assert_eq!(
            [bgr(&pixel, 0), bgr(&pixel, 1), bgr(&pixel, 2)],
            [30.0, 20.0, 10.0]
        );
    }

    #[test]
    fn a_box_scores_the_mean_inside_it_and_nothing_outside() {
        // 4×4 map: a bright 2×2 block in the top-left, zero elsewhere.
        let mut map = vec![0.0f32; 16];
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            map[y * 4 + x] = 1.0;
        }
        let quad = |x0: f32, y0: f32, x1: f32, y1: f32| {
            [
                Point::new(x0, y0),
                Point::new(x1, y0),
                Point::new(x1, y1),
                Point::new(x0, y1),
            ]
        };
        assert_eq!(box_score(&map, 4, 4, &quad(0.0, 0.0, 1.0, 1.0)), 1.0);
        assert_eq!(box_score(&map, 4, 4, &quad(2.0, 2.0, 3.0, 3.0)), 0.0);
        assert_eq!(box_score(&map, 4, 4, &quad(0.0, 0.0, 3.0, 3.0)), 0.25);
    }

    #[test]
    fn the_detector_sees_a_multiple_of_the_stride() {
        // Whatever comes in, both sides must land on the stride or the backbone's
        // downsampling leaves a partial row and the output map is the wrong shape.
        for (width, height) in [(100, 50), (1920, 1080), (33, 17), (960, 960)] {
            let (w, h) = detection_size(width, height);
            assert_eq!(w % DET_STRIDE, 0, "{width}x{height} gave width {w}");
            assert_eq!(h % DET_STRIDE, 0, "{width}x{height} gave height {h}");
            assert!(w > 0 && h > 0);
        }
    }

    #[test]
    fn a_large_image_is_capped_and_a_small_one_is_not_blown_up() {
        // 1920 wide is over the limit and comes down near it.
        let (wide, _) = detection_size(1920, 1080);
        assert!(wide <= DET_LIMIT, "capped to {wide}");

        // A small crop keeps its own resolution rather than being upscaled into blur.
        let (small, _) = detection_size(320, 64);
        assert_eq!(small, 320);
    }
}
