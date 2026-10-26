//! Image preparation that happens *before* an engine's own normalization.
//!
//! # Why a wrapper and not edits inside the engines
//!
//! Both engines normalize deliberately and neither should be touched. `manga.rs` resizes
//! to 224×224 with `Triangle` precisely because that is PIL's BILINEAR, which is what
//! manga-ocr's own `preprocessor_config.json` specifies — it is faithful, not arbitrary.
//! PP-OCRv6 applies ImageNet mean and standard deviation for the same kind of reason. Work
//! done *inside* either would be work done against its training.
//!
//! So these transforms operate on [`OcrImage`], before any of that, through a wrapper that
//! delegates. Anything added to the [`Ocr`] trait later gets them for free.
//!
//! # None of this is known to help
//!
//! `local documentation` argues each of these from the literature, and then says the
//! quiet part: without a test set every claim about accuracy here is unfalsifiable. That
//! is still true. They default to off, they are individually switchable so they can be
//! told apart, and `core/tests/ocr_corpus.rs` exists to score them. Until that harness has
//! been run on real material, "this should help" is a hypothesis wearing a function name.

use image::{imageops::FilterType, RgbaImage};

use super::{Ocr, OcrError, OcrImage, Rect};

/// The longest side to enlarge a small image towards.
///
/// PP-OCRv6's detector scales anything larger down to 960 (`paddle::DET_LIMIT`) and
/// otherwise leaves it alone, so an image brought up to here costs the detector no more
/// than one that arrived at this size — while the recogniser gets line crops it can scale
/// *down* to its fixed 48px height instead of up.
const UPSCALE_TARGET: u32 = 960;

/// Never enlarge by more than this.
///
/// Interpolation invents no detail. Past a point it produces a large blurry image rather
/// than a readable one, and hands the detector a great deal more to chew on for it.
const UPSCALE_MAX: f32 = 4.0;

/// How far a pixel may sit from the border colour and still count as background.
///
/// Generous, because screen text is anti-aliased and a JPEG-ish background is never one
/// flat value. Being too eager here costs a glyph edge; being too shy costs nothing but
/// the trim itself.
const TRIM_TOLERANCE: i32 = 24;

/// Kept around whatever the trim finds.
const TRIM_MARGIN: u32 = 4;

/// A trim that saves less than this fraction of an edge is not worth the risk of it
/// having clipped something.
const TRIM_MIN_GAIN: f32 = 0.02;

/// CLAHE tile count across the longest side.
const CLAHE_TILES: u32 = 8;

/// CLAHE clip limit, as a multiple of the average bin height.
///
/// The standard starting value. Higher approaches plain histogram equalization and its
/// habit of amplifying noise in flat areas into something that looks like texture.
const CLAHE_CLIP: f32 = 3.0;

/// Which preparation steps to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreprocessOptions {
    /// Trim uniform margin around the content.
    pub tighten: bool,
    /// Local contrast enhancement (CLAHE).
    pub contrast: bool,
    /// Enlarge a small image before recognition.
    pub upscale: bool,
}

impl PreprocessOptions {
    pub fn any(self) -> bool {
        self.tighten || self.contrast || self.upscale
    }
}

/// An engine with preparation in front of it.
pub struct Preprocess<E> {
    inner: E,
    options: PreprocessOptions,
}

impl<E: Ocr> Preprocess<E> {
    pub fn new(inner: E, options: PreprocessOptions) -> Preprocess<E> {
        Preprocess { inner, options }
    }
}

impl<E: Ocr> Ocr for Preprocess<E> {
    fn recognize(&self, image: &OcrImage) -> Result<String, OcrError> {
        if !self.options.any() {
            return self.inner.recognize(image);
        }
        self.inner.recognize(&prepare(image, self.options)?)
    }

    fn engine(&self) -> &str {
        self.inner.engine()
    }

    fn recognize_attributed(&self, image: &OcrImage) -> Result<(String, String), OcrError> {
        if !self.options.any() {
            return self.inner.recognize_attributed(image);
        }
        self.inner
            .recognize_attributed(&prepare(image, self.options)?)
    }
}

/// Run the enabled steps, in order.
///
/// Tighten first so the enlargement is spent on text rather than on margin, and contrast
/// before enlargement so the tiles cover real captured pixels rather than interpolated
/// ones. That ordering is reasoning, not evidence — the harness can overturn it.
pub fn prepare(image: &OcrImage, options: PreprocessOptions) -> Result<OcrImage, OcrError> {
    let mut current = image.clone();
    if options.tighten {
        current = tighten(&current);
    }
    if options.contrast {
        current = contrast(&current)?;
    }
    if options.upscale {
        current = upscale(&current)?;
    }

    // Both sizes, because the interesting failure is a transform that did nothing — a trim
    // that found no margin, an upscale already at the cap. "Enabled" and "had an effect"
    // are different facts and only one of them is visible from the settings.
    crate::log_info!(
        "preprocess",
        "{}{}{} : {}x{} → {}x{}",
        if options.tighten { "tighten " } else { "" },
        if options.contrast { "contrast " } else { "" },
        if options.upscale { "upscale" } else { "" },
        image.width(),
        image.height(),
        current.width(),
        current.height()
    );
    Ok(current)
}

/// Trim uniform margin from the edges.
///
/// The capture frame is placed by hand and usually carries some. What is *not* attempted
/// is finding the text: this only removes border that matches the border, so a busy
/// background is left entirely alone rather than guessed at.
///
/// Returns the input unchanged whenever the result would be degenerate or barely smaller,
/// because trimming away a glyph is a worse failure than trimming nothing.
pub fn tighten(image: &OcrImage) -> OcrImage {
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        return image.clone();
    }

    let rgba = image.rgba();
    let at = |x: u32, y: u32| -> [u8; 4] {
        let index = ((y as usize * width as usize) + x as usize) * 4;
        [
            rgba[index],
            rgba[index + 1],
            rgba[index + 2],
            rgba[index + 3],
        ]
    };

    // The corner, as the best available guess at "what the background is". A frame placed
    // over a text box has its corners in the box, not in the text.
    let background = at(0, 0);
    let differs = |pixel: [u8; 4]| {
        (0..3).any(|c| (i32::from(pixel[c]) - i32::from(background[c])).abs() > TRIM_TOLERANCE)
            || (i32::from(pixel[3]) - i32::from(background[3])).abs() > TRIM_TOLERANCE
    };

    let (mut left, mut top) = (width, height);
    let (mut right, mut bottom) = (0u32, 0u32);
    let mut found = false;
    for y in 0..height {
        for x in 0..width {
            if differs(at(x, y)) {
                found = true;
                left = left.min(x);
                right = right.max(x);
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }

    // Every pixel matched the corner. That is a blank capture, and cropping it to nothing
    // would turn a clear "nothing was recognized" into a confusing empty-image error.
    if !found {
        return image.clone();
    }

    let left = left.saturating_sub(TRIM_MARGIN);
    let top = top.saturating_sub(TRIM_MARGIN);
    let right = (right + TRIM_MARGIN).min(width - 1);
    let bottom = (bottom + TRIM_MARGIN).min(height - 1);

    let trimmed = Rect {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    };

    // Not worth it: the risk of a bad trim is constant, so it has to buy something.
    let saved_x = 1.0 - trimmed.width as f32 / width as f32;
    let saved_y = 1.0 - trimmed.height as f32 / height as f32;
    if saved_x.max(saved_y) < TRIM_MIN_GAIN {
        return image.clone();
    }

    image.crop(trimmed).unwrap_or_else(|| image.clone())
}

/// Enlarge a small image towards [`UPSCALE_TARGET`].
///
/// Only ever enlarges. Shrinking here would throw away detail the engines are perfectly
/// able to use — PP-OCRv6's detector does its own downscale, and knows what size it wants.
///
/// `Lanczos3` rather than the `Triangle` both engines use internally: bilinear is the
/// weakest of the common filters for enlargement, and enlarging is the entire point.
pub fn upscale(image: &OcrImage) -> Result<OcrImage, OcrError> {
    let (width, height) = (image.width(), image.height());
    let longest = width.max(height);
    if longest == 0 {
        return Ok(image.clone());
    }

    let factor = (UPSCALE_TARGET as f32 / longest as f32).min(UPSCALE_MAX);
    if factor <= 1.0 {
        return Ok(image.clone());
    }

    let target_w = ((width as f32 * factor).round() as u32).max(1);
    let target_h = ((height as f32 * factor).round() as u32).max(1);

    let source = as_rgba(image)?;
    let resized = image::imageops::resize(&source, target_w, target_h, FilterType::Lanczos3);
    OcrImage::new(target_w, target_h, resized.into_raw())
}

/// CLAHE — contrast-limited adaptive histogram equalization.
///
/// Local, not global: a game text box that is bright on one side and dim on the other is
/// exactly the case a single histogram cannot fix. `imageproc` ships `equalize_histogram`
/// and `stretch_contrast`, both global, and no adaptive variant — hence this.
///
/// **Not** thresholding. Anti-aliased light-on-dark text is where a global threshold eats
/// the glyph, and it is a large share of what this app reads.
///
/// Applied to luminance, with the colour channels rescaled by the ratio, so hue survives.
/// PP-OCRv6's detector was trained on colour images; handing it a grey one would be
/// handing it something it has not seen.
///
/// The contrast it adds is *local*. Whether a smooth gradient across the whole image
/// survives depends on how much of the ramp falls inside one tile relative to the clip
/// limit — a steep one gets flattened, a shallow one is left alone because there is
/// nothing in that tile to equalize. Both are the algorithm working.
pub fn contrast(image: &OcrImage) -> Result<OcrImage, OcrError> {
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        return Ok(image.clone());
    }

    let rgba = image.rgba();
    let pixels = (width as usize) * (height as usize);
    let mut luma = vec![0u8; pixels];
    for (index, slot) in luma.iter_mut().enumerate() {
        let (r, g, b) = (
            f32::from(rgba[index * 4]),
            f32::from(rgba[index * 4 + 1]),
            f32::from(rgba[index * 4 + 2]),
        );
        *slot = (0.299 * r + 0.587 * g + 0.114 * b)
            .round()
            .clamp(0.0, 255.0) as u8;
    }

    let equalized = clahe(&luma, width, height);

    let mut out = rgba.to_vec();
    for index in 0..pixels {
        let before = luma[index];
        let after = equalized[index];
        if before == after {
            continue;
        }
        // Scale rather than replace, so a red glyph stays red and only its brightness
        // moves. `before + 1` avoids dividing by a black pixel.
        let ratio = (f32::from(after) + 1.0) / (f32::from(before) + 1.0);
        for channel in 0..3 {
            let value = f32::from(rgba[index * 4 + channel]) * ratio;
            out[index * 4 + channel] = value.round().clamp(0.0, 255.0) as u8;
        }
    }

    OcrImage::new(width, height, out)
}

/// CLAHE over a single 8-bit plane.
///
/// Tile histograms, each clipped and the excess spread back evenly, then every pixel is
/// mapped by bilinearly blending the four nearest tile transforms. The blend is what keeps
/// tile edges from showing up as a grid in the output.
fn clahe(luma: &[u8], width: u32, height: u32) -> Vec<u8> {
    // Tile *size* first, then the count that covers the image — not the other way round.
    //
    // Fixing the count at CLAHE_TILES and rounding the size up overshoots: 8 tiles of
    // `ceil(20/8) = 3` rows span 24 rows of a 20-row image, so the last two tiles begin
    // past the bottom edge. Their `x1 - x0` underflows, and the mapping for a tile that
    // covers nothing is all zeroes — which the edge-clamping lookup below would then hand
    // to real pixels, turning the margin of a short crop black.
    let tile_w = width.div_ceil(CLAHE_TILES).max(1);
    let tile_h = height.div_ceil(CLAHE_TILES).max(1);
    let tiles_x = width.div_ceil(tile_w).max(1);
    let tiles_y = height.div_ceil(tile_h).max(1);

    // One 256-entry mapping per tile.
    let mut maps = vec![[0u8; 256]; (tiles_x * tiles_y) as usize];
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let x0 = tx * tile_w;
            let y0 = ty * tile_h;
            let x1 = (x0 + tile_w).min(width);
            let y1 = (y0 + tile_h).min(height);

            let mut histogram = [0u32; 256];
            for y in y0..y1 {
                for x in x0..x1 {
                    histogram[luma[(y as usize * width as usize) + x as usize] as usize] += 1;
                }
            }

            let count = (x1 - x0) * (y1 - y0);
            if count == 0 {
                continue;
            }

            // Clip, then hand the clipped excess back to every bin equally. Without this
            // step a flat region's single tall bin stretches into full range and the
            // sensor noise underneath it becomes visible structure.
            let limit = ((CLAHE_CLIP * count as f32) / 256.0).max(1.0) as u32;
            let mut excess = 0u32;
            for bin in histogram.iter_mut() {
                if *bin > limit {
                    excess += *bin - limit;
                    *bin = limit;
                }
            }
            let share = excess / 256;
            let leftover = (excess % 256) as usize;
            for (index, bin) in histogram.iter_mut().enumerate() {
                *bin += share + u32::from(index < leftover);
            }

            let map = &mut maps[(ty * tiles_x + tx) as usize];
            let mut cumulative = 0u32;
            for value in 0..256 {
                cumulative += histogram[value];
                map[value] = ((cumulative as f32 / count as f32) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }

    let lookup = |tx: u32, ty: u32, value: u8| -> f32 {
        f32::from(
            maps[(ty.min(tiles_y - 1) * tiles_x + tx.min(tiles_x - 1)) as usize][value as usize],
        )
    };

    let mut out = vec![0u8; luma.len()];
    for y in 0..height {
        // Position relative to tile centres, so the interpolation is between the tiles a
        // pixel actually sits among rather than between tile origins.
        let fy = (y as f32 + 0.5) / tile_h as f32 - 0.5;
        let ty0 = fy.floor().max(0.0) as u32;
        let wy = (fy - fy.floor()).clamp(0.0, 1.0);
        for x in 0..width {
            let fx = (x as f32 + 0.5) / tile_w as f32 - 0.5;
            let tx0 = fx.floor().max(0.0) as u32;
            let wx = (fx - fx.floor()).clamp(0.0, 1.0);

            let value = luma[(y as usize * width as usize) + x as usize];
            let top = lookup(tx0, ty0, value) * (1.0 - wx) + lookup(tx0 + 1, ty0, value) * wx;
            let bottom =
                lookup(tx0, ty0 + 1, value) * (1.0 - wx) + lookup(tx0 + 1, ty0 + 1, value) * wx;

            out[(y as usize * width as usize) + x as usize] =
                (top * (1.0 - wy) + bottom * wy).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn as_rgba(image: &OcrImage) -> Result<RgbaImage, OcrError> {
    RgbaImage::from_raw(image.width(), image.height(), image.rgba().to_vec())
        .ok_or_else(|| OcrError::MalformedImage("could not read the pixels".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A solid image with a differently-coloured block in it.
    fn with_block(
        width: u32,
        height: u32,
        block: Rect,
        background: [u8; 4],
        fill: [u8; 4],
    ) -> OcrImage {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let inside = x >= block.x
                    && x < block.x + block.width
                    && y >= block.y
                    && y < block.y + block.height;
                rgba.extend_from_slice(if inside { &fill } else { &background });
            }
        }
        OcrImage::new(width, height, rgba).expect("well-formed")
    }

    #[test]
    fn tightening_finds_the_content_and_keeps_a_margin() {
        let block = Rect {
            x: 40,
            y: 30,
            width: 20,
            height: 10,
        };
        let image = with_block(200, 100, block, [255, 255, 255, 255], [0, 0, 0, 255]);

        let trimmed = tighten(&image);

        // The block plus TRIM_MARGIN on each side, and not a pixel of it lost.
        assert_eq!(trimmed.width(), block.width + TRIM_MARGIN * 2);
        assert_eq!(trimmed.height(), block.height + TRIM_MARGIN * 2);
    }

    #[test]
    fn tightening_leaves_a_full_bleed_image_alone() {
        // Content reaching every edge: there is no margin to take, and taking some anyway
        // would be cutting into the text.
        let full = Rect {
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        };
        let image = with_block(200, 100, full, [255, 255, 255, 255], [0, 0, 0, 255]);
        assert_eq!(tighten(&image), image);
    }

    #[test]
    fn tightening_leaves_a_blank_image_alone() {
        // Nothing differs from the corner. Cropping to nothing would turn "nothing was
        // recognized" into an empty-image error, which says less.
        let none = Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        let image = with_block(50, 50, none, [12, 12, 12, 255], [12, 12, 12, 255]);
        assert_eq!(tighten(&image), image);
    }

    #[test]
    fn upscaling_enlarges_towards_the_target_and_keeps_the_shape() {
        // 2x away from the target, so the cap does not bind and this measures reaching it.
        let image = with_block(
            480,
            240,
            Rect {
                x: 10,
                y: 10,
                width: 4,
                height: 4,
            },
            [0; 4],
            [255; 4],
        );
        let bigger = upscale(&image).expect("upscales");

        assert_eq!(
            bigger.width(),
            UPSCALE_TARGET,
            "longest side reaches the target"
        );
        assert_eq!(bigger.height(), UPSCALE_TARGET / 2, "aspect ratio is kept");
    }

    #[test]
    fn upscaling_is_capped() {
        // 40px would need 24x to reach the target. Interpolation invents no detail, and a
        // 960px blur costs the detector real time for it.
        let image = with_block(
            40,
            40,
            Rect {
                x: 5,
                y: 5,
                width: 4,
                height: 4,
            },
            [0; 4],
            [255; 4],
        );
        let bigger = upscale(&image).expect("upscales");
        assert_eq!(bigger.width(), (40.0 * UPSCALE_MAX) as u32);
    }

    #[test]
    fn upscaling_never_shrinks() {
        let image = with_block(
            1600,
            900,
            Rect {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            [0; 4],
            [255; 4],
        );
        assert_eq!(upscale(&image).expect("passes through"), image);
    }

    fn spread(image: &OcrImage) -> u8 {
        let values: Vec<u8> = image.rgba().iter().step_by(4).copied().collect();
        let (lo, hi) = values
            .iter()
            .fold((255u8, 0u8), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        hi - lo
    }

    #[test]
    fn contrast_opens_up_a_dim_image() {
        // Detail spread broadly but confined to 60..120 — a dim capture with everything
        // still in it, which is the case CLAHE is for.
        let (width, height) = (128u32, 128u32);
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let value = 60 + ((x * 7 + y * 13) % 61) as u8;
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let image = OcrImage::new(width, height, rgba).expect("well-formed");

        let before = spread(&image);
        let after = spread(&contrast(&image).expect("enhances"));
        assert!(
            after > before + before / 2,
            "a dim image should open up substantially: {before} -> {after}"
        );
    }

    #[test]
    fn contrast_does_not_amplify_a_two_tone_image() {
        // The "contrast-limited" half, and the reason this is not plain histogram
        // equalization. A tile holding two values carries almost no information; the clip
        // limit means its handful of tall bins cannot be stretched across the full range.
        // Equalizing it anyway is how sensor noise in a flat area becomes visible texture
        // that the detector then tries to read.
        let image = with_block(
            128,
            128,
            Rect {
                x: 32,
                y: 32,
                width: 64,
                height: 64,
            },
            [90, 90, 90, 255],
            [60, 60, 60, 255],
        );
        let enhanced = contrast(&image).expect("passes through");
        assert!(
            spread(&enhanced) < spread(&image) * 3,
            "two tones must not be stretched across the range, got {}",
            spread(&enhanced)
        );
    }

    // There is deliberately no test that an even global ramp gets flattened.
    //
    // It does at 64x64 and does not at 128x128, because how much of the ramp falls inside
    // one tile — relative to the clip limit — decides whether there is anything for that
    // tile to equalize. Both outcomes are correct. Picking the image size that makes an
    // assertion pass would be choosing the fixture to fit the claim, which is how a test
    // stops meaning anything.

    #[test]
    fn contrast_leaves_a_flat_image_flat() {
        // One value everywhere. There is no local contrast to recover, and inventing some
        // would be amplifying nothing into something.
        let image = with_block(
            32,
            32,
            Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            [90, 90, 90, 255],
            [0; 4],
        );
        let enhanced = contrast(&image).expect("passes through");

        let values: Vec<u8> = enhanced.rgba().iter().step_by(4).copied().collect();
        let (lo, hi) = values
            .iter()
            .fold((255u8, 0u8), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        assert_eq!(lo, hi, "a flat image has no local contrast to find");
    }

    #[test]
    fn contrast_keeps_hue() {
        // A red block on dark grey. Whatever happens to brightness, red must not come out
        // grey — the detector was trained on colour.
        let image = with_block(
            64,
            64,
            Rect {
                x: 16,
                y: 16,
                width: 32,
                height: 32,
            },
            [30, 30, 30, 255],
            [200, 20, 20, 255],
        );
        let enhanced = contrast(&image).expect("enhances");

        let index = ((32 * 64) + 32) * 4;
        let pixel = &enhanced.rgba()[index..index + 3];
        assert!(
            pixel[0] > pixel[1] * 2 && pixel[0] > pixel[2] * 2,
            "red should stay dominant, got {pixel:?}"
        );
    }

    #[test]
    fn contrast_survives_awkward_sizes() {
        // Found by running the corpus harness, not by reading the code: a tightened crop
        // came out short enough that the tiling overshot its bottom edge and the tile
        // arithmetic underflowed. Sizes that do not divide by the tile count, and sizes
        // smaller than it.
        for (width, height) in [
            (9, 20),
            (20, 9),
            (1, 1),
            (3, 200),
            (200, 3),
            (17, 23),
            (7, 7),
        ] {
            let image = with_block(
                width,
                height,
                Rect {
                    x: 0,
                    y: 0,
                    width: width / 2,
                    height: height / 2,
                },
                [40, 40, 40, 255],
                [200, 200, 200, 255],
            );
            let enhanced = contrast(&image).expect("handles {width}x{height}");
            assert_eq!(enhanced.width(), width);
            assert_eq!(enhanced.height(), height);
        }
    }

    #[test]
    fn nothing_enabled_is_the_identity() {
        let image = with_block(
            80,
            40,
            Rect {
                x: 10,
                y: 10,
                width: 8,
                height: 8,
            },
            [0; 4],
            [255; 4],
        );
        let untouched = prepare(&image, PreprocessOptions::default()).expect("passes through");
        assert_eq!(untouched, image);
    }
}
