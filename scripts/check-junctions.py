"""Find the ragged junctions in a monoline mark.

A stroke that crosses another one is still one stroke. When the trace fails to see that, it
emits the pieces as separate paths, and every piece gets its own round cap — so where the
drawing has a single sweep passing through a crossing, the SVG has several free ends near
the same spot rather than on it.

That is the whole of the defect, and it is geometric rather than a matter of taste. A round
cap is a disc. Two discs sharing a centre have a union that is a disc, and nothing shows.
Two discs a little apart have outlines that cross, and the crossing points are cusps — a
notch on the inside of the junction and a lump on the outside. So the fault is not several
ends meeting, which is normal and unavoidable; it is several ends meeting *and missing*.

Which pairs of ends are supposed to be one junction is topology this file does not have —
it reads the finished SVG. What it can say is that two caps close enough for their discs to
interfere ought to be concentric. Ends further apart than that are separate features whose
caps barely touch, and it says nothing about them, because it cannot tell a real gap from
an intended one.

Endpoints only. Two paths crossing mid-span is fine and normal — 屠 is full of it — and the
join there is drawn by the overlap, not by a cap.

    python scripts/check-junctions.py ui/src/assets/mark/mono/plain.svg
"""

import itertools
import math
import pathlib
import re
import sys

art = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
width = float(re.search(r'stroke-width="([\d.]+)"', art).group(1))

# Close enough that the two discs overlap over most of their area, so a mismatch in centres
# shows as a cusp rather than as two strokes that happen to pass near each other.
INTERFERE = width * 0.6
# And how far apart they may then sit before it shows. A tenth of the stroke is comfortably
# under what survives being drawn at 22px in the rail.
APART = width * 0.1

ends = []
for i, d in enumerate(re.findall(r'<path d="([^"]+)"', art)):
    n = [float(v) for v in re.findall(r"-?\d+\.?\d*", d)]
    # Absolute M/L/C only, which is all these files use, and every command's last pair is
    # the point it lands on — so the first pair and the last pair are the two free ends.
    ends.append(((n[0], n[1]), i, "start"))
    ends.append(((n[-2], n[-1]), i, "end"))

near = [
    (math.dist(a[0], b[0]), a, b)
    for a, b in itertools.combinations(ends, 2)
    # A path meeting itself is a closed shape, not a fragmented stroke.
    if a[1] != b[1] and math.dist(a[0], b[0]) < INTERFERE
]
ragged = [n for n in near if n[0] > APART]

print(f"{sys.argv[1]}")
print(f"  {len(ends) // 2} paths, stroke {width:g}; caps within {INTERFERE:.1f} should be "
      f"within {APART:.1f} of each other")
for gap, a, b in sorted(near, key=lambda n: -n[0]):
    mark = "RAGGED" if gap > APART else "  ok  "
    print(f"  {mark} {gap:5.1f} apart: paths[{a[1]}] {a[2]} ({a[0][0]:.1f}, {a[0][1]:.1f})"
          f"  /  paths[{b[1]}] {b[2]} ({b[0][0]:.1f}, {b[0][1]:.1f})")
print(f"  {len(ragged)} ragged of {len(near)} interfering pair(s)")
sys.exit(1 if ragged else 0)
