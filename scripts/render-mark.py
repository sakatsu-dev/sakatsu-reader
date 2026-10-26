"""Rasterise the app's mark, to feed `cargo tauri icon`.

Reads `ui/src/assets/mark/mono/{plain,seal}.svg`, and **checks that every path in it also
appears in `Mark.svelte`** before drawing anything. The component cannot simply be parsed
the way it used to be — it now switches the plate on a prop, so its rect sits inside an
`{#if}` and its transform is a ternary rather than a literal. Rather than teach this script
to read Svelte, it reads the plain SVG and refuses to run if the two have drifted apart,
which is the only failure that actually matters: an icon that is no longer the logo.

Only the subset of SVG these files use is understood — one optional rounded rect, then a
group of paths of absolute M/L/C with a round cap and join, optionally under a
translate/scale. Cubics are flattened rather than drawn: PIL has no curves, and the round
caps and joins it also lacks are supplied by a disc at every point.

    pip install pillow
    python scripts/render-mark.py src-tauri/icons/source.png 1024 plain
    cd src-tauri && cargo tauri icon icons/source.png
"""

import math
import pathlib
import re
import sys

from PIL import Image, ImageDraw

SS = 8  # supersample factor; round caps alias badly without it
RED = (226, 104, 122, 255)  # --red, for the plain mark
REPO = pathlib.Path(__file__).resolve().parent.parent

out = pathlib.Path(sys.argv[1])
size = int(sys.argv[2]) if len(sys.argv) > 2 else 1024
variant = sys.argv[3] if len(sys.argv) > 3 else "plain"
# Icons are not shown edge to edge. The impression carries its own plate, so it wants no
# margin at all; the bare glyph needs one or it reads as a crop.
margin = float(sys.argv[4]) if len(sys.argv) > 4 else (0.0 if variant == "seal" else 0.12)

art = (REPO / f"ui/src/assets/mark/mono/{variant}.svg").read_text(encoding="utf-8")
component = (REPO / "ui/src/lib/Mark.svelte").read_text(encoding="utf-8")

paths = re.findall(r'<path d="([^"]+)"', art)
if not paths:
    raise SystemExit(f"{variant}.svg: no paths")
missing = [d for d in paths if d not in component]
if missing:
    raise SystemExit(
        f"{variant}.svg and Mark.svelte disagree on {len(missing)} of {len(paths)} paths.\n"
        "The icons would no longer be the logo. Regenerate both from the same source."
    )

view = re.search(r'viewBox="0 0 ([\d.]+)', art)
box = float(view.group(1))

n = round(size * SS)
unit = n * (1 - 2 * margin) / box
pad = n * margin
canvas = Image.new("RGBA", (n, n), (0, 0, 0, 0))
draw = ImageDraw.Draw(canvas)

plate = re.search(r"<rect\b[^>]*>", art)
if plate:
    rx = float(re.search(r'rx="([\d.]+)"', plate.group(0)).group(1))
    fill = re.search(r'fill="(#[0-9a-fA-F]{3,6})"', plate.group(0)).group(1)
    draw.rounded_rectangle(
        [pad, pad, n - pad - 1, n - pad - 1], radius=rx * unit, fill=fill
    )

group = re.search(r"<g\b[^>]*>", art).group(0)
weight = float(re.search(r'stroke-width="([\d.]+)"', group).group(1))
colour = re.search(r'stroke="(#[0-9a-fA-F]{3,6})"', group)
ink = colour.group(1) if colour else RED
move = re.search(r"translate\(([-\d.]+) ([-\d.]+)\) scale\(([-\d.]+)\)", group)
tx, ty, scale = (float(g) for g in move.groups()) if move else (0.0, 0.0, 1.0)

half = weight * scale * unit / 2


def at(x, y):
    return (x * scale + tx) * unit + pad, (y * scale + ty) * unit + pad


TOKEN = re.compile(r"([MLC])((?:\s*-?[\d.]+)+)")
FLATTEN = 16  # samples per cubic, well past what is visible even at 1024px


def walk(d):
    """One path as a point list, flattening cubics. M/L/C absolute is all the mark uses."""
    here = (0.0, 0.0)
    out = []
    for command, raw in TOKEN.findall(d):
        n = [float(v) for v in re.findall(r"-?[\d.]+", raw)]
        if command in "ML":
            here = (n[0], n[1])
            out.append(here)
            continue
        for i in range(0, len(n), 6):
            (x1, y1), (x2, y2), (x3, y3) = (n[i], n[i + 1]), (n[i + 2], n[i + 3]), (n[i + 4], n[i + 5])
            x0, y0 = here
            for step in range(1, FLATTEN + 1):
                t = step / FLATTEN
                u = 1 - t
                out.append((
                    u**3 * x0 + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t**3 * x3,
                    u**3 * y0 + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t**3 * y3,
                ))
            here = (x3, y3)
    return out


for d in paths:
    points = [at(x, y) for x, y in walk(d)]
    for (x0, y0), (x1, y1) in zip(points, points[1:]):
        length = math.hypot(x1 - x0, y1 - y0)
        if length == 0:
            continue
        # Butt-ended quad along the segment; the discs below supply the round cap and the
        # round join, which is exactly what stroke-linecap/linejoin="round" draws.
        nx, ny = -(y1 - y0) / length * half, (x1 - x0) / length * half
        draw.polygon(
            [(x0 + nx, y0 + ny), (x1 + nx, y1 + ny), (x1 - nx, y1 - ny), (x0 - nx, y0 - ny)],
            fill=ink,
        )
    for x, y in points:
        draw.ellipse([x - half, y - half, x + half, y + half], fill=ink)

out.parent.mkdir(parents=True, exist_ok=True)
canvas.resize((size, size), Image.LANCZOS).save(out)
# Plain ASCII: a Windows console on the Japanese code page cannot encode a dash.
print(f"{out}: {size}px, {variant}, {len(paths)} strokes, {margin:.0%} margin")
