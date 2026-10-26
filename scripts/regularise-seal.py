"""Redraw a seal glyph with straight edges and square corners.

The other route at this — skeletonise, measure the stroke, redraw at one width — has to
recover the topology from the scan, and on a 235-pixel woodblock print it guesses wrong:
strokes that merely touch get fused, and crossings sprout stubs.

This keeps the silhouette the trace already got right and only regularises it. Every edge
that is nearly horizontal or nearly vertical is made exactly so, and each corner is then
put back at the intersection of its two cleaned edges rather than left where the wobble
had it. Nothing moves that was not already within tolerance of the axis, so the letterform
survives while the paper grain does not.

That suits 摹印篆, which is built from horizontals and verticals cut to fill a square.

Produces the `uniform/` set in `ui/src/assets/mark/`; that README covers where the plates
come from, how they are licensed, and why the epsilons here are coarser than the traced
set's. Needs `opencv-python` and `numpy`, neither of which the app itself depends on —
this is run by hand when the mark is being redrawn, not by the build.

    python scripts/regularise-seal.py TE00029.jpg carved-plain.svg 0.006  20
    python scripts/regularise-seal.py TE00029.jpg carved-fine.svg  0.0025 20
    python scripts/regularise-seal.py TE00039.jpg shuowen.svg      0.0025 20
"""

import pathlib
import sys

import cv2
import numpy as np

src = pathlib.Path(sys.argv[1])
out = pathlib.Path(sys.argv[2])
epsilon = float(sys.argv[3]) if len(sys.argv) > 3 else 0.006
snap_deg = float(sys.argv[4]) if len(sys.argv) > 4 else 20.0
UPSCALE = 4
BOX = 1000.0

# Read the bytes ourselves: cv2.imread goes through the ANSI file API on Windows and
# cannot open a path with Japanese in it, which this project's own path has. Left as
# imread it fails on the invocation printed above and blames the file for an encoding
# problem.
image = cv2.imdecode(np.frombuffer(src.read_bytes(), np.uint8), cv2.IMREAD_GRAYSCALE)
if image is None:
    raise SystemExit(f"could not decode {src}")

image = cv2.resize(image, None, fx=UPSCALE, fy=UPSCALE, interpolation=cv2.INTER_CUBIC)
image = cv2.GaussianBlur(image, (5, 5), 0)
_, binary = cv2.threshold(image, 0, 255, cv2.THRESH_BINARY_INV + cv2.THRESH_OTSU)
kernel = cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (5, 5))
binary = cv2.morphologyEx(binary, cv2.MORPH_CLOSE, kernel)
binary = cv2.morphologyEx(binary, cv2.MORPH_OPEN, kernel)

contours, _ = cv2.findContours(binary, cv2.RETR_CCOMP, cv2.CHAIN_APPROX_SIMPLE)
if not contours:
    raise SystemExit("no contours")
# The same relative cut trace.py makes — anything under a thousandth of the main mass is
# dirt on the plate. Relative rather than absolute, and identical to the trace, so the two
# sets differ only in the regularising and can actually be compared.
biggest = max(cv2.contourArea(c) for c in contours)
contours = [c for c in contours if cv2.contourArea(c) >= biggest * 0.001]

HORIZONTAL, VERTICAL, FREE = "h", "v", None


def regularise(points):
    """Straighten near-axis edges, then rebuild each corner where its edges now meet."""
    n = len(points)
    # Edge i runs from vertex i to vertex i+1. Lock it to an axis if it is close enough,
    # and record the single coordinate that then defines the whole edge.
    axis, value = [], []
    for i in range(n):
        (x0, y0), (x1, y1) = points[i], points[(i + 1) % n]
        angle = np.degrees(np.arctan2(y1 - y0, x1 - x0)) % 180
        if min(angle, 180 - angle) <= snap_deg:
            axis.append(HORIZONTAL)
            value.append((y0 + y1) / 2)
        elif abs(angle - 90) <= snap_deg:
            axis.append(VERTICAL)
            value.append((x0 + x1) / 2)
        else:
            axis.append(FREE)
            value.append(None)

    # A corner is where two edges meet, so once both are straight the corner belongs at
    # their intersection — not at the traced vertex, which is a pixel or two off both.
    fixed = []
    for i in range(n):
        before, after = (i - 1) % n, i
        x, y = points[i]
        a, b = axis[before], axis[after]
        if a == VERTICAL and b == HORIZONTAL:
            x, y = value[before], value[after]
        elif a == HORIZONTAL and b == VERTICAL:
            x, y = value[after], value[before]
        elif a == HORIZONTAL and b == HORIZONTAL:
            y = (value[before] + value[after]) / 2
        elif a == VERTICAL and b == VERTICAL:
            x = (value[before] + value[after]) / 2
        # One locked, one free: the free edge may end wherever it likes, but the locked
        # edge's own coordinate is not negotiable.
        elif a == HORIZONTAL:
            y = value[before]
        elif a == VERTICAL:
            x = value[before]
        elif b == HORIZONTAL:
            y = value[after]
        elif b == VERTICAL:
            x = value[after]
        fixed.append((float(x), float(y)))

    # Straightening collapses some edges to nothing; drop the duplicate vertices so the
    # path does not carry zero-length segments.
    kept = [p for i, p in enumerate(fixed) if np.hypot(*np.subtract(p, fixed[i - 1])) > 0.5]
    return np.array(kept if len(kept) >= 3 else fixed)


shapes = [regularise(cv2.approxPolyDP(c, epsilon * cv2.arcLength(c, True), True).reshape(-1, 2))
          for c in contours]

everything = np.vstack(shapes)
x0, y0 = everything.min(axis=0)
x1, y1 = everything.max(axis=0)
scale = BOX / max(x1 - x0, y1 - y0)
offset_x = (BOX - (x1 - x0) * scale) / 2
offset_y = (BOX - (y1 - y0) * scale) / 2

paths = []
for s in shapes:
    p = s.copy()
    p[:, 0] = (p[:, 0] - x0) * scale + offset_x
    p[:, 1] = (p[:, 1] - y0) * scale + offset_y
    paths.append(
        f"M{p[0][0]:.1f} {p[0][1]:.1f}"
        + "".join(f"L{x:.1f} {y:.1f}" for x, y in p[1:])
        + "Z"
    )

svg = (
    f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {BOX:.0f} {BOX:.0f}">\n'
    f'  <path fill="currentColor" fill-rule="evenodd" d="{"".join(paths)}" />\n</svg>\n'
)
out.write_text(svg, encoding="utf-8")
print(f"{out.name}: {len(paths)} contours, {sum(len(p) for p in paths)} bytes of path data")
