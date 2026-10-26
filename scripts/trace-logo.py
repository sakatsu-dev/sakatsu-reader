"""Vectorise the 屠つ drawing into SVG outline paths, as two pieces of artwork.

Kept because the artwork in `Mark.svelte` and `Wordmark.svelte` is generated, not hand-written.
Re-run it against a new drawing rather than editing the path data by hand:

    SAKATSU_LOGO_SRC=drawing.jpg SAKATSU_LOGO_OUT=out python scripts/trace-logo.py

Needs numpy, Pillow and opencv-python. It writes `artwork.json` plus a preview SVG of each
piece; the components take their `d` from that JSON.


Outlines rather than centrelines. The drawing is monoline -- one constant 55px stroke with round
caps -- so a centreline trace plus `stroke-width` would be the compact representation, but
skeletonising a glyph with this many crossings invents junctions the drawing does not have.
Tracing the boundary keeps every edge and curve where it was drawn, which is what was asked for.

Two outputs from one trace, because the app has two shapes to fill:
  wordmark  the whole name, 1:2.24 as drawn
  mark      屠 alone, 1:1.19, for the rail and every square icon

Pipeline: ink mask -> 2x upscale for sub-pixel boundaries -> contours with holes -> smooth along
arc length to kill the pixel staircase -> fit cubic Beziers (Schneider) to a tight tolerance.
"""

import json
import os

import cv2
import numpy as np
from PIL import Image

SRC = os.environ.get("SAKATSU_LOGO_SRC", "logo-source.jpg")
OUT = os.environ.get("SAKATSU_LOGO_OUT", ".")
UP = 2  # upscale factor, for sub-pixel contour placement
MAX_ERROR = 0.6  # in source pixels; the whole glyph is 1470px tall
GAP_Y = 1083.0  # the empty band between 屠 and つ, measured


def ink_mask():
    a = np.asarray(Image.open(SRC).convert("RGB")).astype(np.float32)
    lum = 0.2126 * a[:, :, 0] + 0.7152 * a[:, :, 1] + 0.0722 * a[:, :, 2]
    redness = a[:, :, 0] - (a[:, :, 1] + a[:, :, 2]) / 2

    # Coverage rather than a hard yes/no: the drawing is anti-aliased and it is a JPEG, so the
    # edge is a ramp. Reading it as a ramp puts the boundary where the artist's edge is rather
    # than where the compressor's ringing is.
    cover = np.clip((235.0 - lum) / 120.0, 0, 1)
    cover[redness < 18] = 0
    cover = cv2.GaussianBlur(cover, (0, 0), 0.9)

    big = cv2.resize(cover, None, fx=UP, fy=UP, interpolation=cv2.INTER_CUBIC)
    # 0.45, not the obvious 0.5: swept against the unsmoothed mask over sigma and cut, and the
    # half-way point erodes the edge by about half a pixel against a white ground.
    return (big > 0.45).astype(np.uint8)


def resample(points, spacing):
    closed = np.vstack([points, points[:1]])
    seg = np.linalg.norm(np.diff(closed, axis=0), axis=1)
    arc = np.concatenate([[0], np.cumsum(seg)])
    if arc[-1] < spacing * 4:
        return None
    want = np.arange(0, arc[-1], spacing)
    return np.column_stack(
        [np.interp(want, arc, closed[:, 0]), np.interp(want, arc, closed[:, 1])]
    )


def smooth_closed(points, sigma):
    """Gaussian along the contour, wrapping -- a closed curve has no ends to special-case."""
    n = len(points)
    radius = max(1, int(sigma * 3))
    k = np.exp(-0.5 * (np.arange(-radius, radius + 1) / sigma) ** 2)
    k /= k.sum()
    padded = np.vstack([points[-radius:], points, points[:radius]])
    return np.column_stack([np.convolve(padded[:, d], k, mode="valid") for d in range(2)])[:n]


def bezier(p0, p1, p2, p3, t):
    mt = 1 - t
    return (
        (mt**3)[:, None] * p0
        + (3 * mt**2 * t)[:, None] * p1
        + (3 * mt * t**2)[:, None] * p2
        + (t**3)[:, None] * p3
    )


def chord_params(points):
    d = np.linalg.norm(np.diff(points, axis=0), axis=1)
    u = np.concatenate([[0], np.cumsum(d)])
    return u / u[-1] if u[-1] > 0 else np.linspace(0, 1, len(points))


def unit(v):
    n = np.linalg.norm(v)
    return v / n if n > 1e-12 else np.array([0.0, 0.0])


def fit_one(points, u, t_left, t_right):
    """Least-squares cubic with both tangents fixed -- Schneider's generateBezier."""
    p0, p3 = points[0], points[-1]
    a0 = 3 * (1 - u) ** 2 * u
    a1 = 3 * (1 - u) * u**2
    A0 = a0[:, None] * t_left
    A1 = a1[:, None] * t_right
    tmp = points - (
        ((1 - u) ** 3)[:, None] * p0
        + a0[:, None] * p0
        + a1[:, None] * p3
        + (u**3)[:, None] * p3
    )
    c00 = float((A0 * A0).sum())
    c01 = float((A0 * A1).sum())
    c11 = float((A1 * A1).sum())
    x0 = float((A0 * tmp).sum())
    x1 = float((A1 * tmp).sum())
    det = c00 * c11 - c01 * c01
    span = float(np.linalg.norm(p3 - p0))
    if abs(det) < 1e-12:
        return np.array([p0, p0 + t_left * span / 3, p3 + t_right * span / 3, p3])
    alpha0 = (x0 * c11 - x1 * c01) / det
    alpha1 = (c00 * x1 - c01 * x0) / det
    if alpha0 < 1e-6 or alpha1 < 1e-6:
        alpha0 = alpha1 = span / 3
    return np.array([p0, p0 + t_left * alpha0, p3 + t_right * alpha1, p3])


def fit_curves(points, t_left, t_right, tol, depth=0):
    if len(points) < 3:
        span = np.linalg.norm(points[-1] - points[0]) / 3
        return [
            np.array(
                [points[0], points[0] + t_left * span, points[-1] + t_right * span, points[-1]]
            )
        ]
    u = chord_params(points)
    curve = fit_one(points, u, t_left, t_right)
    on = bezier(curve[0], curve[1], curve[2], curve[3], u)
    d = np.linalg.norm(on - points, axis=1)
    if float(d.max()) < tol or depth > 18:
        return [curve]
    at = min(max(int(d.argmax()), 1), len(points) - 2)
    centre = unit(points[at - 1] - points[at + 1])
    return fit_curves(points[: at + 1], t_left, centre, tol, depth + 1) + fit_curves(
        points[at:], -centre, t_right, tol, depth + 1
    )


def corners(points, threshold_deg=52):
    """Where the contour genuinely turns. Round caps have none; stroke crossings do."""
    n, look, out = len(points), 5, []
    for i in range(n):
        before = unit(points[i] - points[(i - look) % n])
        after = unit(points[(i + look) % n] - points[i])
        angle = np.degrees(np.arccos(np.clip(float(before @ after), -1, 1)))
        if angle > threshold_deg:
            out.append((i, angle))
    kept = []
    for i, angle in out:
        if kept and (i - kept[-1][0]) < look * 2:
            if angle > kept[-1][1]:
                kept[-1] = (i, angle)
        else:
            kept.append((i, angle))
    return [i for i, _ in kept]


def trace(contour, tol):
    points = resample(contour, spacing=1.6)
    if points is None:
        return []
    points = smooth_closed(points, sigma=1.6)
    n = len(points)
    breaks = corners(points) or []
    if len(breaks) < 2:
        breaks = [0, n // 2]
    segments = []
    for a, b in zip(breaks, breaks[1:] + [breaks[0] + n]):
        run = np.array([points[i % n] for i in range(a, b + 1)])
        if len(run) < 3:
            continue
        segments.extend(fit_curves(run, unit(run[1] - run[0]), unit(run[-2] - run[-1]), tol))
    return segments


def emit(pieces, name, square):
    """Normalise traced contours into a viewBox and render the path data."""
    allpts = np.vstack([np.vstack([c[[0, 3]] for c in segs]) for _, segs in pieces])
    x0, y0 = allpts.min(axis=0)
    x1, y1 = allpts.max(axis=0)
    w, h = x1 - x0, y1 - y0
    scale = 1000.0 / h
    vw = w * scale
    # A square box centres the glyph, so the seal plate can stay square; otherwise the box is
    # the artwork's own shape.
    dx = (1000.0 - vw) / 2 if square else 0.0
    box = 1000.0 if square else round(vw, 1)

    def at(pt):
        return ((pt[0] - x0) * scale + dx, (pt[1] - y0) * scale)

    out = []
    for _, segs in pieces:
        s0 = at(segs[0][0])
        d = [f"M{s0[0]:.1f} {s0[1]:.1f}"]
        for c in segs:
            c1, c2, p3 = at(c[1]), at(c[2]), at(c[3])
            d.append(f"C{c1[0]:.1f} {c1[1]:.1f} {c2[0]:.1f} {c2[1]:.1f} {p3[0]:.1f} {p3[1]:.1f}")
        d.append("Z")
        out.append("".join(d))
    print(f"  {name}: viewBox 0 0 {box:g} 1000, {len(out)} paths, {sum(map(len, out))} chars")
    return {"viewBox": f"0 0 {box:g} 1000", "paths": out}


mask = ink_mask()
found, _ = cv2.findContours(mask, cv2.RETR_CCOMP, cv2.CHAIN_APPROX_NONE)
traced = []
for contour in found:
    pts = contour.reshape(-1, 2).astype(np.float64) / UP
    if cv2.contourArea(contour.astype(np.float32)) / (UP * UP) < 12:
        continue
    segs = trace(pts, tol=MAX_ERROR)
    if segs:
        traced.append((float(pts[:, 1].min()), segs))
print(f"contours kept: {len(traced)}")

result = {
    "wordmark": emit(traced, "wordmark 屠つ", square=False),
    "mark": emit([t for t in traced if t[0] < GAP_Y], "mark 屠", square=True),
}
with open(f"{OUT}/artwork.json", "w") as f:
    json.dump(result, f, indent=1)

for key, art in result.items():
    svg = (
        f'<svg viewBox="{art["viewBox"]}" xmlns="http://www.w3.org/2000/svg">\n'
        f'  <path fill="#990b0a" fill-rule="evenodd" d="{" ".join(art["paths"])}"/>\n</svg>\n'
    )
    with open(f"{OUT}/{key}.svg", "w") as f:
        f.write(svg)
print("wrote artwork.json, wordmark.svg, mark.svg")
