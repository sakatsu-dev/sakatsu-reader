//! Turning a detection probability map into text-line boxes.
//!
//! Everything here is arithmetic on points, deliberately independent of `ort` and of the
//! `image` crate's types, because this is the half of PP-OCR that goes subtly wrong. A
//! box a few pixels too tight clips the tops off characters and the recogniser reads
//! something else; too loose and it swallows the neighbouring line. Neither fails
//! visibly — you just get worse text — so it is tested directly rather than through a
//! model.
//!
//! DBNet has two post-processing modes. The *polygon* mode follows the contour exactly
//! and needs general polygon offsetting, which is why ports of it reach for Clipper, a
//! C++ library. The *quad* mode used here fits a rotated rectangle and expands it, which
//! is closed-form: for a rectangle, offsetting outward by `d` on every side is just
//! growing it by `2d` in each dimension.

/// A point in image space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }
}

/// A rotated rectangle: centre, size, and rotation in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotatedRect {
    pub cx: f32,
    pub cy: f32,
    pub width: f32,
    pub height: f32,
    pub angle: f32,
}

impl RotatedRect {
    /// The four corners, in the order the rotation produces them.
    pub fn corners(&self) -> [Point; 4] {
        let (sin, cos) = self.angle.sin_cos();
        let (hw, hh) = (self.width / 2.0, self.height / 2.0);
        [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].map(|(dx, dy)| {
            Point::new(self.cx + dx * cos - dy * sin, self.cy + dx * sin + dy * cos)
        })
    }

    /// The upright box that contains this one, clamped to an image of `bounds`.
    ///
    /// Recognition works on an upright crop, and text in a screenshot is very rarely
    /// rotated more than a degree or two, so the containing box is what gets read.
    pub fn bounding_box(&self, bounds: (u32, u32)) -> Option<(u32, u32, u32, u32)> {
        let corners = self.corners();
        let left = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let right = corners
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let top = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let bottom = corners
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);

        let x = left.max(0.0).round() as u32;
        let y = top.max(0.0).round() as u32;
        let right = (right.round().max(0.0) as u32).min(bounds.0);
        let bottom = (bottom.round().max(0.0) as u32).min(bounds.1);

        if right <= x || bottom <= y {
            return None;
        }
        Some((x, y, right - x, bottom - y))
    }
}

/// The convex hull of a point set, counter-clockwise (Andrew's monotone chain).
///
/// A contour from a probability map is a noisy outline of hundreds of points; the hull
/// is what the rotating calipers below need, and reduces the work to its few corners.
pub fn convex_hull(points: &[Point]) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });
    sorted.dedup_by(|a, b| a.x == b.x && a.y == b.y);
    if sorted.len() < 3 {
        return sorted;
    }

    let cross =
        |o: Point, a: Point, b: Point| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);

    // Two chains, lower then upper. The upper pass must not pop into the lower one, so
    // it stops at the floor the first pass left — without that it eats its own points
    // and a square comes back as a triangle.
    let mut hull: Vec<Point> = Vec::with_capacity(sorted.len() * 2);
    for &point in sorted.iter() {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0 {
            hull.pop();
        }
        hull.push(point);
    }

    let floor = hull.len() + 1;
    for &point in sorted.iter().rev().skip(1) {
        while hull.len() >= floor && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0
        {
            hull.pop();
        }
        hull.push(point);
    }

    hull.pop(); // the start point, which both chains end on
    hull
}

/// The smallest-area rectangle enclosing a point set, by rotating calipers.
///
/// The minimum-area rectangle always has a side flush with an edge of the convex hull,
/// so trying each hull edge as the rectangle's axis is exhaustive rather than a search.
/// `imageproc` gives contours but no rotated-rect fit, and OpenCV is not a dependency
/// this project will take, so it is here.
pub fn min_area_rect(points: &[Point]) -> Option<RotatedRect> {
    let hull = convex_hull(points);
    if hull.len() < 2 {
        return None;
    }

    let mut best: Option<(f32, RotatedRect)> = None;

    for i in 0..hull.len() {
        let a = hull[i];
        let b = hull[(i + 1) % hull.len()];
        let edge = (b.x - a.x, b.y - a.y);
        let length = (edge.0 * edge.0 + edge.1 * edge.1).sqrt();
        if length < f32::EPSILON {
            continue;
        }
        // The edge's direction, and its perpendicular.
        let (ux, uy) = (edge.0 / length, edge.1 / length);
        let (vx, vy) = (-uy, ux);

        // Extent of the hull along both axes.
        let (mut min_u, mut max_u) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_v, mut max_v) = (f32::INFINITY, f32::NEG_INFINITY);
        for point in &hull {
            let u = point.x * ux + point.y * uy;
            let v = point.x * vx + point.y * vy;
            min_u = min_u.min(u);
            max_u = max_u.max(u);
            min_v = min_v.min(v);
            max_v = max_v.max(v);
        }

        let (width, height) = (max_u - min_u, max_v - min_v);
        let area = width * height;
        if best.as_ref().is_some_and(|(seen, _)| area >= *seen) {
            continue;
        }

        // Back from axis space into image space.
        let (mid_u, mid_v) = ((min_u + max_u) / 2.0, (min_v + max_v) / 2.0);
        best = Some((
            area,
            RotatedRect {
                cx: mid_u * ux + mid_v * vx,
                cy: mid_u * uy + mid_v * vy,
                width,
                height,
                angle: uy.atan2(ux),
            },
        ));
    }

    best.map(|(_, rect)| rect)
}

/// Expand a box outward, the way DBNet's post-processing does.
///
/// The detector is trained to predict a *shrunk* version of each text region, so the
/// prediction has to be grown back. PaddleOCR offsets the polygon outward by
/// `area × ratio / perimeter`; for a rectangle that is exactly adding `2d` to each
/// dimension, so no polygon-clipping library is involved.
pub fn unclip(rect: RotatedRect, ratio: f32) -> RotatedRect {
    let area = rect.width * rect.height;
    let perimeter = 2.0 * (rect.width + rect.height);
    if perimeter <= f32::EPSILON {
        return rect;
    }
    let distance = area * ratio / perimeter;
    RotatedRect {
        width: rect.width + 2.0 * distance,
        height: rect.height + 2.0 * distance,
        ..rect
    }
}

/// Whether a box holds a vertical line of text.
///
/// The trap in reading Japanese with a recogniser trained on horizontal lines: a
/// vertical line is a tall narrow box, and fed in as-is it is read as noise. Rotating it
/// makes it a normal line again.
///
/// The threshold is deliberately not 1.0. A single character is roughly square and could
/// fall either side of it by a pixel, and rotating a horizontal line by mistake is as
/// destructive as failing to rotate a vertical one, so a box has to be clearly taller
/// than it is wide before it counts.
pub fn is_vertical(width: u32, height: u32) -> bool {
    width > 0 && height as f32 / width as f32 >= 1.5
}

/// The direction in which the detected lines form a passage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingDirection {
    Horizontal,
    Vertical,
}

/// Pick the passage direction from the boxes that make an unambiguous claim.
///
/// Area rather than box count: a real line should outweigh a handful of square punctuation
/// detections, and a long horizontal heading should count for more than one narrow annotation.
/// Square boxes abstain because a single Japanese character says nothing about its direction.
fn direction(boxes: &[(u32, u32, u32, u32)]) -> ReadingDirection {
    let mut horizontal = 0u64;
    let mut vertical = 0u64;

    for &(_, _, width, height) in boxes {
        let area = u64::from(width) * u64::from(height);
        if is_vertical(width, height) {
            vertical += area;
        } else if height > 0 && width as f32 / height as f32 >= 1.5 {
            horizontal += area;
        }
    }

    if vertical > horizontal {
        ReadingDirection::Vertical
    } else {
        // Ambiguous is deliberately horizontal. Game UI is the all-round engine's ordinary
        // case, and changing established output requires positive evidence rather than a tie.
        ReadingDirection::Horizontal
    }
}

/// Whether two fragments occupy the same vertical column.
///
/// Centres are doubled so the half-pixel centre of an odd-width box stays exact in integer
/// arithmetic. A separation no greater than half the wider box is enough to absorb the small
/// left/right jitter DBNet gives fragments from one column, without merging adjacent columns.
fn same_column(a: (u32, u32, u32, u32), b: (u32, u32, u32, u32)) -> bool {
    let centre_a = u64::from(a.0) * 2 + u64::from(a.2);
    let centre_b = u64::from(b.0) * 2 + u64::from(b.2);
    centre_a.abs_diff(centre_b) <= u64::from(a.2.max(b.2))
}

/// Put vertical fragments into columns, then read those columns in Japanese order.
fn vertical_order(boxes: &mut [(u32, u32, u32, u32)]) {
    #[derive(Debug)]
    struct Column {
        /// The widest fragment is the most stable estimate of the column's centre.
        anchor: (u32, u32, u32, u32),
        fragments: Vec<(u32, u32, u32, u32)>,
    }

    let mut pending = boxes.to_vec();
    pending.sort_by(|a, b| {
        let centre_a = u64::from(a.0) * 2 + u64::from(a.2);
        let centre_b = u64::from(b.0) * 2 + u64::from(b.2);
        centre_b.cmp(&centre_a).then(a.1.cmp(&b.1))
    });

    let mut columns: Vec<Column> = Vec::new();
    for found in pending {
        if let Some(column) = columns
            .iter_mut()
            .find(|column| same_column(found, column.anchor))
        {
            column.fragments.push(found);
            if found.2 > column.anchor.2 {
                column.anchor = found;
            }
        } else {
            columns.push(Column {
                anchor: found,
                fragments: vec![found],
            });
        }
    }

    columns.sort_by(|a, b| {
        let centre_a = u64::from(a.anchor.0) * 2 + u64::from(a.anchor.2);
        let centre_b = u64::from(b.anchor.0) * 2 + u64::from(b.anchor.2);
        centre_b.cmp(&centre_a)
    });

    let mut output = boxes.iter_mut();
    for column in &mut columns {
        // Top to bottom inside a column. If furigana and body text begin at the same height,
        // the right-hand fragment leads, matching where Japanese ruby sits in vertical type.
        column
            .fragments
            .sort_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
        for &fragment in &column.fragments {
            if let Some(slot) = output.next() {
                *slot = fragment;
            }
        }
    }
}

/// Sort boxes into the order they are read and report the inferred direction.
///
/// Horizontal text remains top-to-bottom then left-to-right. Vertical Japanese is top-to-bottom
/// within a column and right-to-left between columns; applying the horizontal rule to it reverses
/// the sentence one otherwise well-recognized column at a time.
pub fn reading_order(boxes: &mut [(u32, u32, u32, u32)]) -> ReadingDirection {
    let direction = direction(boxes);
    match direction {
        ReadingDirection::Vertical => vertical_order(boxes),
        ReadingDirection::Horizontal => {
            // "Same line" tolerates boxes whose tops differ by a few pixels, or two halves of
            // one line come back reversed. Half the taller box scales with the text.
            boxes.sort_by(|a, b| {
                let tolerance = (a.3.max(b.3) as f32 * 0.5) as u32;
                if a.1.abs_diff(b.1) <= tolerance {
                    a.0.cmp(&b.0)
                } else {
                    a.1.cmp(&b.1)
                }
            });
        }
    }
    direction
}

/// The four corners in PaddleOCR's order — top-left, top-right, bottom-right, bottom-left — each
/// rounded and clamped to an image of `bounds`, as `DBPostProcess.get_mini_boxes` and its
/// rescale leave them.
///
/// Sorted by x; of the two leftmost the higher is top-left, of the two rightmost the higher is
/// top-right. That is PaddleOCR's rule exactly, ties included, because the crop's orientation
/// follows from it.
pub fn paddle_corners(rect: &RotatedRect, bounds: (u32, u32)) -> [Point; 4] {
    let mut points = rect.corners();
    points.sort_by(|a, b| a.x.total_cmp(&b.x));
    let (top_left, bottom_left) = if points[1].y > points[0].y {
        (points[0], points[1])
    } else {
        (points[1], points[0])
    };
    let (top_right, bottom_right) = if points[3].y > points[2].y {
        (points[2], points[3])
    } else {
        (points[3], points[2])
    };
    [top_left, top_right, bottom_right, bottom_left].map(|point| {
        Point::new(
            point.x.round().clamp(0.0, bounds.0 as f32),
            point.y.round().clamp(0.0, bounds.1 as f32),
        )
    })
}

/// The crop PaddleOCR's pipeline reads (`get_rotate_crop_image`): the quadrilateral warped flat to
/// `max(top, bottom) × max(left, right)` pixels, bicubic, the image's edge repeated beyond it.
///
/// `None` for a quad too thin to read.
pub fn warped_crop(source: &image::RgbImage, quad: &[Point; 4]) -> Option<image::RgbImage> {
    use imageproc::geometric_transformations::{warp_into_with, Border, Interpolation, Projection};

    let distance = |a: Point, b: Point| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    let width = distance(quad[0], quad[1]).max(distance(quad[2], quad[3])) as u32;
    let height = distance(quad[0], quad[3]).max(distance(quad[1], quad[2])) as u32;
    if width == 0 || height == 0 {
        return None;
    }
    let (w, h) = (width as f32, height as f32);
    let projection = Projection::from_control_points(
        [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)],
        quad.map(|point| (point.x, point.y)),
    )?;
    let (max_x, max_y) = (
        source.width().saturating_sub(1) as f32,
        source.height().saturating_sub(1) as f32,
    );
    let mut out = image::RgbImage::new(width, height);
    warp_into_with(
        source,
        |x, y| {
            let (sx, sy) = projection * (x, y);
            // BORDER_REPLICATE: a sample past the edge takes the edge's pixel.
            (sx.clamp(0.0, max_x), sy.clamp(0.0, max_y))
        },
        Interpolation::Bicubic,
        // The clamp above keeps the sample point inside; this keeps the bicubic kernel's outer
        // taps there too. Before imageproc 0.27 a sample whose 4×4 kernel crossed the edge was
        // replaced whole by a black default pixel, so a line touching the image's edge was read
        // with a black fringe.
        Border::Replicate,
        &mut out,
    );
    Some(out)
}

/// Give recognition a small margin beyond DBNet's line box, clamped to the source image.
///
/// DBNet predicts a region and `unclip` grows it geometrically, but integer conversion can still
/// land on an antialiased edge. Eight percent of the short side is one or two pixels on ordinary
/// game text: enough to keep kana and punctuation whole, not enough to swallow the next line.
pub fn recognition_box(
    (x, y, width, height): (u32, u32, u32, u32),
    bounds: (u32, u32),
) -> (u32, u32, u32, u32) {
    let padding = ((width.min(height) as f32 * 0.08).ceil() as u32).max(1);
    let left = x.saturating_sub(padding);
    let top = y.saturating_sub(padding);
    let right = x
        .saturating_add(width)
        .saturating_add(padding)
        .min(bounds.0);
    let bottom = y
        .saturating_add(height)
        .saturating_add(padding)
        .min(bounds.1);
    (
        left,
        top,
        right.saturating_sub(left),
        bottom.saturating_sub(top),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PaddleOCR's corner order: top-left, top-right, bottom-right, bottom-left, whatever the
    /// rectangle's own winding — and a flat quad warps to its own size.
    #[test]
    fn paddle_corners_and_the_warped_crop_follow_paddleocr() {
        let rect = RotatedRect {
            cx: 20.0,
            cy: 10.0,
            width: 30.0,
            height: 10.0,
            angle: 0.0,
        };
        let quad = paddle_corners(&rect, (100, 100));
        let flat: Vec<(f32, f32)> = quad.iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(
            flat,
            vec![(5.0, 5.0), (35.0, 5.0), (35.0, 15.0), (5.0, 15.0)]
        );
        // The same rectangle described a quarter-turn round reorders to the same corners.
        let turned = RotatedRect {
            width: 10.0,
            height: 30.0,
            angle: std::f32::consts::FRAC_PI_2,
            ..rect
        };
        let again: Vec<(f32, f32)> = paddle_corners(&turned, (100, 100))
            .iter()
            .map(|p| (p.x, p.y))
            .collect();
        assert_eq!(again, flat);

        let mut source = image::RgbImage::from_pixel(100, 100, image::Rgb([255, 255, 255]));
        source.put_pixel(20, 10, image::Rgb([0, 0, 0]));
        let crop = warped_crop(&source, &quad).expect("a readable quad");
        assert_eq!(crop.dimensions(), (30, 10));
        // The dark pixel lands where it was, relative to the quad's corner.
        assert!(crop.get_pixel(15, 5)[0] < 128);
        assert_eq!(*crop.get_pixel(1, 1), image::Rgb([255, 255, 255]));
    }

    /// A line flush with the image's edge is read with the edge repeated, never a black fringe.
    ///
    /// The image is one mid-grey and the quad slightly skewed, so samples fall between pixels and
    /// the bicubic kernel reaches past the edge. Repeating the edge keeps a uniform image uniform.
    /// White would not show a fringe: the kernel's outer weights are negative, so a black tap
    /// there pushes white above 255, where it clamps back to white.
    #[test]
    fn a_crop_at_the_image_edge_repeats_the_edge() {
        let source = image::RgbImage::from_pixel(41, 14, image::Rgb([128, 128, 128]));
        let quad = [
            Point::new(0.0, 0.0),
            Point::new(40.0, 1.0),
            Point::new(40.0, 13.0),
            Point::new(0.0, 12.0),
        ];
        let crop = warped_crop(&source, &quad).expect("a readable quad");
        let off: Vec<(u32, u32, u8)> = crop
            .enumerate_pixels()
            .filter(|(_, _, pixel)| pixel[0].abs_diff(128) > 1)
            .map(|(x, y, pixel)| (x, y, pixel[0]))
            .collect();
        assert!(off.is_empty(), "the edge was not repeated: {off:?}");
    }

    fn points(raw: &[(f32, f32)]) -> Vec<Point> {
        raw.iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    #[test]
    fn the_hull_of_a_square_with_an_interior_point_is_the_square() {
        let hull = convex_hull(&points(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (5.0, 5.0), // inside, and must not survive
        ]));
        assert_eq!(hull.len(), 4);
        assert!(!hull.contains(&Point::new(5.0, 5.0)));
    }

    #[test]
    fn an_axis_aligned_box_gets_its_own_dimensions_back() {
        let rect = min_area_rect(&points(&[
            (10.0, 20.0),
            (40.0, 20.0),
            (40.0, 30.0),
            (10.0, 30.0),
        ]))
        .expect("a rect");

        // Width and height may arrive swapped depending on which edge won, so compare
        // the pair rather than the fields.
        let (long, short) = if rect.width >= rect.height {
            (rect.width, rect.height)
        } else {
            (rect.height, rect.width)
        };
        assert!((long - 30.0).abs() < 0.01, "long side was {long}");
        assert!((short - 10.0).abs() < 0.01, "short side was {short}");
        assert!((rect.cx - 25.0).abs() < 0.01);
        assert!((rect.cy - 25.0).abs() < 0.01);
    }

    #[test]
    fn a_rotated_box_is_measured_along_its_own_axis() {
        // A 40×10 rectangle turned 45°. A bounding box would call this ~35×35; the
        // minimum-area rect has to find the real shape, which is the whole point.
        let angle: f32 = std::f32::consts::FRAC_PI_4;
        let (sin, cos) = angle.sin_cos();
        let corners: Vec<(f32, f32)> = [(-20.0, -5.0), (20.0, -5.0), (20.0, 5.0), (-20.0, 5.0)]
            .iter()
            .map(|(x, y)| (x * cos - y * sin + 100.0, x * sin + y * cos + 100.0))
            .collect();

        let rect = min_area_rect(&points(&corners)).expect("a rect");
        let (long, short) = if rect.width >= rect.height {
            (rect.width, rect.height)
        } else {
            (rect.height, rect.width)
        };
        assert!((long - 40.0).abs() < 0.1, "long side was {long}");
        assert!((short - 10.0).abs() < 0.1, "short side was {short}");
    }

    #[test]
    fn unclipping_grows_a_box_by_the_documented_distance() {
        let rect = RotatedRect {
            cx: 50.0,
            cy: 50.0,
            width: 40.0,
            height: 10.0,
            angle: 0.0,
        };
        // area 400, perimeter 100, ratio 1.5 → d = 6, so each dimension gains 12.
        let grown = unclip(rect, 1.5);
        assert!((grown.width - 52.0).abs() < 0.01, "{}", grown.width);
        assert!((grown.height - 22.0).abs() < 0.01, "{}", grown.height);
        // Growing outward must not move the box.
        assert_eq!((grown.cx, grown.cy), (rect.cx, rect.cy));
    }

    #[test]
    fn a_bounding_box_is_clamped_to_the_image() {
        let rect = RotatedRect {
            cx: 5.0,
            cy: 5.0,
            width: 40.0,
            height: 40.0,
            angle: 0.0,
        };
        // Centred near the corner, so it hangs off two edges and must be trimmed rather
        // than producing a negative origin.
        let (x, y, w, h) = rect.bounding_box((20, 20)).expect("some overlap");
        assert_eq!((x, y), (0, 0));
        assert_eq!((w, h), (20, 20));

        // Entirely outside is not a box at all.
        let outside = RotatedRect {
            cx: 500.0,
            cy: 500.0,
            width: 10.0,
            height: 10.0,
            angle: 0.0,
        };
        assert!(outside.bounding_box((20, 20)).is_none());
    }

    #[test]
    fn only_a_clearly_tall_box_counts_as_vertical() {
        assert!(is_vertical(20, 200), "a column of text");
        assert!(!is_vertical(200, 20), "a line of text");
        // Roughly square is the ambiguous case, and rotating it by mistake would be as
        // damaging as missing a real column.
        assert!(!is_vertical(40, 44));
        assert!(!is_vertical(0, 100), "no width is not a ratio");
    }

    #[test]
    fn horizontal_boxes_come_back_in_reading_order() {
        // Deliberately shuffled, and with the two boxes on the first line differing in
        // top edge by a few pixels the way real detections do.
        let mut boxes = [
            (10, 100, 50, 20), // second line
            (80, 12, 50, 20),  // first line, right
            (10, 10, 50, 20),  // first line, left
        ];
        assert_eq!(reading_order(&mut boxes), ReadingDirection::Horizontal);
        assert_eq!(boxes[0].0, 10, "leftmost of the first line");
        assert_eq!(boxes[1].0, 80, "then its right half");
        assert_eq!(boxes[2].1, 100, "then the next line down");
    }

    #[test]
    fn vertical_columns_read_right_to_left_and_top_to_bottom() {
        // The left column is split into two detections with a one-pixel horizontal jitter.
        // Column grouping must keep those together rather than reading each as a new column.
        let mut boxes = [
            (10, 80, 19, 55),  // left column, lower fragment
            (82, 12, 20, 120), // right column
            (45, 9, 21, 125),  // middle column
            (11, 10, 20, 60),  // left column, upper fragment
        ];

        assert_eq!(reading_order(&mut boxes), ReadingDirection::Vertical);
        assert_eq!(boxes[0].0, 82, "rightmost column first");
        assert_eq!(boxes[1].0, 45, "then the middle column");
        assert_eq!(boxes[2].1, 10, "top of the left column");
        assert_eq!(boxes[3].1, 80, "then down that column");
    }

    #[test]
    fn ambiguous_square_detections_do_not_turn_a_game_capture_vertical() {
        let mut boxes = [(10, 10, 20, 20), (40, 12, 22, 21)];
        assert_eq!(reading_order(&mut boxes), ReadingDirection::Horizontal);
        assert_eq!(boxes[0].0, 10);
    }

    #[test]
    fn a_horizontal_game_line_outweighs_a_vertical_annotation() {
        let mut boxes = [
            (20, 30, 200, 20), // ordinary dialogue
            (240, 10, 10, 60), // a narrow vertical annotation
        ];

        assert_eq!(reading_order(&mut boxes), ReadingDirection::Horizontal);
    }

    #[test]
    fn vertical_manga_columns_outweigh_a_horizontal_label() {
        let mut boxes = [
            (70, 10, 20, 100),
            (40, 10, 20, 100),
            (10, 10, 20, 100),
            (5, 130, 100, 20), // a horizontal sound effect or label
        ];

        assert_eq!(reading_order(&mut boxes), ReadingDirection::Vertical);
    }

    #[test]
    fn recognition_margin_is_small_and_stays_inside_the_image() {
        // Short side 20 → ceil(1.6) = 2 pixels on every free edge.
        assert_eq!(
            recognition_box((10, 20, 20, 100), (200, 200)),
            (8, 18, 24, 104)
        );
        // At an image corner only the two available sides grow.
        assert_eq!(recognition_box((0, 0, 10, 10), (20, 20)), (0, 0, 11, 11));
    }
}
