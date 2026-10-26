"""Trace a monoline drawing back to stroked vector paths.

This is the approach that failed on the woodblock plate, applied to the case it was built
for. There it had to guess topology from 235 pixels of blotted, uneven ink and got it
wrong. Here the strokes are one constant width with clean edges at 900 pixels across, so
the skeleton is the drawing's own centreline rather than an inference about it.

    python trace_mono.py <image.png> <out.svg>
"""

import json
import pathlib
import sys
from collections import deque

import cv2
import numpy as np

src, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
BOX = 1000.0
NEIGHBOURS = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]


def thin(mask):
    """Zhang-Suen: erode to a one-pixel centreline without breaking the shape."""
    img = (mask > 0).astype(np.uint8)
    while True:
        before = img.sum()
        for step in (0, 1):
            p = [
                np.roll(np.roll(img, dy, 0), dx, 1)
                for dy, dx in ((-1, 0), (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1))
            ]
            b = sum(p)
            ring = p + [p[0]]
            a = sum(((ring[i] == 0) & (ring[i + 1] == 1)).astype(np.uint8) for i in range(8))
            c1, c2 = ((p[0] * p[2] * p[4], p[2] * p[4] * p[6]) if step == 0
                      else (p[0] * p[2] * p[6], p[0] * p[4] * p[6]))
            img = img & ~((img == 1) & (b >= 2) & (b <= 6) & (a == 1) & (c1 == 0) & (c2 == 0))
        if img.sum() == before:
            return (img * 255).astype(np.uint8)


# Read the bytes ourselves: cv2.imread goes through the ANSI file API on Windows and
# cannot open a path with Japanese in it, which this project's paths routinely have.
image = cv2.imdecode(np.frombuffer(src.read_bytes(), np.uint8), cv2.IMREAD_UNCHANGED)
if image is None:
    raise SystemExit(f"could not decode {src}")
if image.shape[2] == 4:  # composite onto white so transparency does not read as ink
    alpha = image[:, :, 3:4] / 255.0
    image = (image[:, :, :3] * alpha + 255 * (1 - alpha)).astype(np.uint8)

ink = 255 - image.min(axis=2)
_, binary = cv2.threshold(ink, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
ys, xs = np.nonzero(binary)
# Clamped at zero on the way in. A negative slice start does not error in numpy, it counts
# back from the end — so a drawing whose ink begins within `pad` of the top or left crops
# to the wrong rows entirely, or to nothing, and the failure shows up much later as a nan
# stroke width. The upper bounds need no clamp; numpy stops at the edge on its own.
pad = 8
binary = binary[
    max(0, ys.min() - pad):ys.max() + pad,
    max(0, xs.min() - pad):xs.max() + pad,
]

distance = cv2.distanceTransform(binary, cv2.DIST_L2, 5)
skel = (thin(binary) > 0).astype(np.uint8)
stroke = float(np.median(distance[skel > 0])) * 2.0
h, w = skel.shape


def neighbours(y, x):
    for dy, dx in NEIGHBOURS:
        ny, nx = y + dy, x + dx
        if 0 <= ny < h and 0 <= nx < w and skel[ny, nx]:
            yield ny, nx


degree = np.zeros_like(skel, dtype=np.int8)
ys, xs = np.nonzero(skel)
for y, x in zip(ys, xs):
    degree[y, x] = sum(1 for _ in neighbours(y, x))

# Thinning leaves a clot at a crossing rather than a single pixel. Group touching non-2
# pixels so everything meeting there is pinned to one agreed point.
node_of, node_centre = {}, []
for y, x in zip(ys, xs):
    if degree[y, x] == 2 or (y, x) in node_of:
        continue
    clot, queue = [], deque([(y, x)])
    node_of[(y, x)] = len(node_centre)
    while queue:
        cy, cx = queue.popleft()
        clot.append((cy, cx))
        for n in neighbours(cy, cx):
            if degree[n] != 2 and n not in node_of:
                node_of[n] = len(node_centre)
                queue.append(n)
    node_centre.append((sum(c[1] for c in clot) / len(clot), sum(c[0] for c in clot) / len(clot)))

visited, runs = set(), []


def walk(start, first):
    line = [start, first]
    visited.update({(start, first), (first, start)})
    current, previous = first, start
    while degree[current] == 2:
        nxt = next((n for n in neighbours(*current)
                    if n != previous and (current, n) not in visited), None)
        if nxt is None:
            break
        visited.update({(current, nxt), (nxt, current)})
        previous, current = current, nxt
        line.append(current)
    return line


for pass_over_junctions in (True, False):
    for y, x in zip(ys, xs):
        if (degree[y, x] == 2) == pass_over_junctions:
            continue
        for n in neighbours(y, x):
            if ((y, x), n) not in visited:
                runs.append(walk((y, x), n))


# A whisker is what thinning grows out of a junction clot, and it is *short* — measured on
# this drawing every one of them is 2px, while the shortest real feature (the two nubs on
# top of 日) is 45. The cutoff goes in that gap with room on both sides. It used to be
# `stroke * 0.8`, which at 66px of stroke is 53 and swallowed both nubs, so the 日 came out
# with its corners missing and the loss looked like a drawing error rather than a filter.
WHISKER = 0.25


def is_noise(line):
    start, end = node_of.get(line[0]), node_of.get(line[-1])
    if start is not None and start == end:
        return True  # in and out of the same clot
    loose = degree[line[0]] == 1 or degree[line[-1]] == 1
    return loose and len(line) < max(5, stroke * WHISKER)


runs = [r for r in runs if not is_noise(r)]

# A stroke does not stop where it happens to cross another one. Rejoin runs that arrive at
# a node and leave it going the same way, so each stroke is simplified as the single line
# it is rather than as the pieces the crossings cut it into.
TANGENT, STRAIGHT = 12, -0.7
edges = [(node_of.get(r[0]), node_of.get(r[-1]), r) for r in runs]
loops = [r for a, b, r in edges if a is None or b is None]
edges = [(a, b, r) for a, b, r in edges if a is not None and b is not None]

incident = {}
for i, (a, b, _) in enumerate(edges):
    incident.setdefault(a, []).append((i, 0))
    incident.setdefault(b, []).append((i, 1))


def away(edge, end):
    # Measured from the node outward, clot pixels and all. Stepping over the clot first was
    # tried, on the theory that its pixels are dragged toward its centre and so describe the
    # blob rather than the stroke. Measured against the drawing it made the trace very
    # slightly worse (93.54% of the ink against 93.58%), so it is not here.
    pixels = edges[edge][2]
    run = pixels[:TANGENT] if end == 0 else pixels[::-1][:TANGENT]
    dy, dx = run[-1][0] - run[0][0], run[-1][1] - run[0][1]
    return np.array([dx, dy]) / (np.hypot(dx, dy) or 1.0)


partner = {}
for node, halves in incident.items():
    options = []
    for i in range(len(halves)):
        for j in range(i + 1, len(halves)):
            dot = float(away(*halves[i]) @ away(*halves[j]))
            if dot < STRAIGHT:
                options.append((dot, halves[i], halves[j]))
    for _, left, right in sorted(options, key=lambda o: o[0]):
        if left not in partner and right not in partner:
            partner[left], partner[right] = right, left

chains, used = [], set()
for i in range(len(edges)):
    if i in used:
        continue
    head, seen = (i, 0), {i}
    while (nxt := partner.get(head)) is not None and nxt[0] not in seen:
        head = (nxt[0], 1 - nxt[1])
        seen.add(nxt[0])
    chain, cursor = [], head
    while cursor is not None and cursor[0] not in used:
        edge, end = cursor
        used.add(edge)
        pixels = edges[edge][2]
        chain.extend(pixels if end == 0 else pixels[::-1])
        cursor = partner.get((edge, 1 - end))
    chains.append(chain)

# ---------------------------------------------------------------------------------------
# Pixels -> spans, split at the corners that are really there.
#
# A skeleton is a one-pixel path on a pixel grid, so it stair-steps even where the drawing
# was smooth. Simplifying it to straight chords keeps the drawing's shape and throws away
# the fact that it curved — 屠's long sweeps came back as chains of flat facets.
#
# Curves are fitted instead. But 屠 has real corners too — the elbow of 尸, the box of 日 —
# and a curve run through those would round them off, so the corners are found first and
# each stroke is cut into spans between them. Smooth within a span, sharp at the join.
# ---------------------------------------------------------------------------------------
CORNER = 55.0  # degrees of turn that count as a corner rather than a curve
# How far a free end is pushed back out, as a fraction of the stroke width. Measured, not
# guessed: see `reach_out`, and the sweep in the mark README.
EXTEND = float(sys.argv[3]) if len(sys.argv) > 3 else 0.15


def smooth(points, window):
    """Moving average along the path. Takes the pixel stair-step off, keeps the shape."""
    if len(points) < 3 or window < 1:
        return points
    padded = np.pad(points, ((window, window), (0, 0)), mode="edge")
    kernel = np.ones(2 * window + 1) / (2 * window + 1)
    return np.column_stack([np.convolve(padded[:, i], kernel, mode="valid") for i in range(2)])


def corners(points, reach):
    """Indices where the path turns hard enough to be a corner and not a bend.

    The turn is measured across `reach` points either side rather than between adjacent
    ones: at one-pixel spacing every step is 0 or 45 degrees, so adjacent-point angles say
    nothing about the shape and everything about the grid.
    """
    if len(points) < 2 * reach + 3:
        return []
    found = []
    for i in range(reach, len(points) - reach):
        back, forward = points[i] - points[i - reach], points[i + reach] - points[i]
        nb, nf = np.hypot(*back), np.hypot(*forward)
        if nb == 0 or nf == 0:
            continue
        cosine = float(np.clip(back @ forward / (nb * nf), -1, 1))
        found.append((np.degrees(np.arccos(cosine)), i))
    # Only the sharpest point of each turn: a corner spread over several samples would
    # otherwise be cut two or three times and reappear as a tiny facet.
    picked, taken = [], np.zeros(len(points), bool)
    for angle, i in sorted(found, reverse=True):
        if angle < CORNER or taken[max(0, i - reach):i + reach + 1].any():
            continue
        picked.append(i)
        taken[max(0, i - reach):i + reach + 1] = True
    return sorted(picked)


def reach_out(points, line, distance):
    """Put back the length a free end loses on the way through this pipeline.

    Thinning stops short of a rounded cap, and the moving average then drags the last point
    inward toward its neighbours. Together they cost noticeably more than either alone, and
    the loss lands entirely on the ends, so a traced stroke comes out shorter than the one
    drawn — visible as a crescent of missing ink over every free tip.

    Only ends that are really ends. A stroke arriving at a junction is pinned there by the
    junction, and growing it would push it through whatever it meets — that end is moved
    onto the junction instead, which is the other half of this function.

    **Landing every arriving stroke on one point is what keeps a crossing clean.** Thinning
    leaves the strokes that meet there stopping a few pixels apart, and each one carries its
    own round cap; caps at different centres have outlines that cross, and that shows as a
    notch on the inside of the crossing and a lump on the outside. It is the whole of what
    "ragged ends" looked like on 者's diagonal — the drawing sweeps one stroke through the
    horizontal, and the trace was drawing three stubs that nearly touched. Landed together
    the caps are concentric discs, and their union is a disc.
    """
    out = points.copy()
    span = max(2, int(round(distance)))
    ends = ((0, min(span, len(out) - 1)), (len(out) - 1, max(len(out) - 1 - span, 0)))
    for tip, inner in ends:
        # Every non-degree-2 pixel is a node, free ends included, so the degree is what
        # separates a tip to grow from a junction to land on.
        if degree[line[tip]] != 1:
            node = node_of.get(line[tip])
            if node is not None:
                out[tip] = np.array(node_centre[node], dtype=float)
            continue
        if tip == inner:
            continue
        direction = out[tip] - out[inner]
        length = float(np.hypot(*direction))
        if length > 1e-6:
            out[tip] = out[tip] + direction / length * distance
    return out


def resample(points, step):
    """Even spacing by arc length. Uniform Catmull-Rom below assumes it."""
    walked = np.r_[0.0, np.cumsum(np.hypot(*np.diff(points, axis=0).T))]
    if walked[-1] < 1e-9:
        return points[:1]
    count = max(2, int(round(walked[-1] / step)) + 1)
    want = np.linspace(0, walked[-1], count)
    return np.column_stack([np.interp(want, walked, points[:, i]) for i in range(2)])


strokes = []
for line in chains + loops:
    points = np.array([[x, y] for y, x in line], dtype=float)
    reach = max(2, int(round(stroke * 0.35)))
    points = smooth(points, max(1, int(round(stroke * 0.18))))
    points = reach_out(points, line, stroke * EXTEND)
    cuts = [0, *corners(points, reach), len(points) - 1]
    spans = [
        resample(points[a:b + 1], stroke * 0.9)
        for a, b in zip(cuts, cuts[1:])
        if b - a >= 2
    ]
    strokes.append([s for s in spans if len(s) >= 2] or [points[[0, -1]]])

everything = np.vstack([s for spans in strokes for s in spans])
x0, y0 = everything.min(axis=0)
x1, y1 = everything.max(axis=0)
scale = (BOX - stroke) / max(x1 - x0, y1 - y0)
ox = (BOX - (x1 - x0) * scale) / 2
oy = (BOX - (y1 - y0) * scale) / 2
strokes = [
    [np.column_stack([(s[:, 0] - x0) * scale + ox, (s[:, 1] - y0) * scale + oy]) for s in spans]
    for spans in strokes
]


def bezier(span):
    """A span of evenly spaced points as cubic segments through every one of them.

    Catmull-Rom, converted to Bézier by the standard tangent/6 rule: the curve passes
    through the points rather than being pulled towards them, so the fitted stroke sits on
    the traced centreline instead of near it. Ends are clamped by repeating the endpoint,
    which makes the first and last segments leave straight along the path.
    """
    if len(span) == 2:
        return f"L{span[1][0]:.1f} {span[1][1]:.1f}"
    padded = np.vstack([span[0], span, span[-1]])
    out = []
    for i in range(1, len(padded) - 2):
        p0, p1, p2, p3 = padded[i - 1], padded[i], padded[i + 1], padded[i + 2]
        c1, c2 = p1 + (p2 - p0) / 6, p2 - (p3 - p1) / 6
        out.append(f"C{c1[0]:.1f} {c1[1]:.1f} {c2[0]:.1f} {c2[1]:.1f} {p2[0]:.1f} {p2[1]:.1f}")
    return "".join(out)


drawn = []
for spans in strokes:
    d = f"M{spans[0][0][0]:.1f} {spans[0][0][1]:.1f}"
    for i, span in enumerate(spans):
        # A span after the first starts where the last ended; the corner between them is
        # the join, and drawing a line to it would double back over the same point.
        d += bezier(span if i == 0 else np.vstack([spans[i - 1][-1], span]))
    drawn.append(d)

out.with_suffix(".json").write_text(json.dumps(drawn), encoding="utf-8")
paths = "".join(f'    <path d="{d}" />\n' for d in drawn)
out.write_text(
    f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {BOX:.0f} {BOX:.0f}">\n'
    f'  <g fill="none" stroke="currentColor" stroke-width="{stroke * scale:.1f}" '
    f'stroke-linecap="round" stroke-linejoin="round">\n{paths}  </g>\n</svg>\n',
    encoding="utf-8",
)
curves = sum(d.count("C") for d in drawn)
print(f"{out.name}: {len(drawn)} strokes, {curves} curves, width {stroke * scale:.0f}/1000")
