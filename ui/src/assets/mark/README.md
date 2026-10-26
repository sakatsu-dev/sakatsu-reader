# 屠 — the seal mark

`ui/src/lib/Mark.svelte` carries 屠 in 小篆, drawn monoline — one constant stroke weight,
round caps, round joins. `mono/plain.svg` and `mono/seal.svg` are the same mark standalone,
bare and as an impression. It is path data rather than a character set in a font, and this
is why.

## No font could do it

| font | licence | 屠 | つ |
|---|---|---|---|
| [LxgwSeal](https://github.com/lxgw/LxgwSeal) | OFL 1.1, embedding allowed | absent — 133 CJK, all elementary | absent |
| [JFZSK Seal Script](https://github.com/jeffi369/JFZSKSealScript) | OFL 1.1, embedding allowed | absent — 1,374 CJK | absent, no kana at all |
| 白舟古印体 / 白舟篆書 教育漢字版 | free to use | education kanji only, and no tensho in the free set | — |
| 白舟篆書 (full) | ¥39,600 | yes | yes |

Each was checked for real outlines rather than a `cmap` entry, because a glyph a face does
not cover comes back empty rather than failing.

The commercial face is the only one that covers 屠, and its terms rule it out anyway:
embedding the font in software needs 別途契約, a separate contract, and the single-glyph
outlines it sells instead may not be used for a logo — which is the one thing this is.

屠 is 漢検1級 and outside the jōyō list, which is why specialty faces skip it.

## Why there is no つ

Seal script was standardised under Qin around 220 BCE. Hiragana appears about 900 CE. The
scholarly corpus behind the plate below holds 13,213 character forms and is kanji only,
necessarily — there is no authentic つ to set beside 屠, and any つ in a modern "tensho
font" was invented by that font's designer.

A seal carries kanji. That is what a seal is. The app itself is called **Sakatsu Reader**
now, in the title bar and everywhere it names itself; 屠 survives as the mark, which is
where a kanji works without having to be read.

## Why monoline, and what that fixed

Every earlier mark was a **filled** outline traced off a woodblock plate, and each one was
texture rather than a character at rail size. The conclusion drawn from that — recorded here
for two commits — was that no form of 屠 resolves at 1.25rem, so the mark should be judged
on silhouette and legibility written off.

That conclusion was about filled forms, and it does not survive the change to a monoline.
This mark **is legible at 22px in the rail and at 32px as an icon**, which none of its
predecessors were. Thin strokes of even weight keep their counters open through
downsampling; a filled seal-script form closes its own counters as soon as the ink spreads
past a pixel or two. The variable was never the plate or the script — it was the fill.

Two things do carry over. Judge any replacement in the rail at the size the rail uses, not
on a specimen sheet. And a claim about small sizes is worth nothing unless it was measured
the way a browser draws — rendered large and downsampled — because a rasteriser without
antialiasing makes everything at 20px look like a blot, which is how the wrong conclusion
got made in the first place.

## No symmetrising, and why the script is gone

An earlier drawing built 者 from a fan of splayed strokes that was clearly meant to mirror
and did not quite — pairs out by 40, 37 and 23 units against the axis. That mark was fixed
by averaging each pair with its own reflection, so neither half was declared the good one,
and `scripts/symmetrise-mark.py` did it.

This drawing does not want any of that. Its 者 is a cross and a diagonal over the 日, which
is not a symmetric shape at all, and the script's pairs were **stroke indices belonging to
the old trace** — run against this one it would average unrelated strokes into each other
and quietly return something wrong. It has been deleted rather than left in `scripts/` for
someone to reach for. The technique is recorded here; the trap is not kept.

Worth remembering if a symmetric drawing ever comes back: simplification has to run *after*
averaging, and the halves survive it because `approxPolyDP` picks mirrored points from
mirrored input. That was checked rather than assumed at the time.

## Ragged crossings, and the one number that showed it

The third drawing was traced because the 者's right side looked wrong in the icon — ends
that read as cut off, and a shape that did not match. Both symptoms had one cause, and it
was not the drawing.

A round cap is a disc. Where a stroke crosses another one the trace emits the pieces as
separate paths, and each piece gets its own cap; the pieces stop a few pixels short of the
crossing and at *slightly different points*, so the discs are centred apart. Two discs
centred apart have outlines that cross, and the crossings are cusps — a notch on the inside
of the junction, a lump on the outside. Landed on one point the discs are concentric and
their union is a disc, and nothing shows at all.

So `reach_out` does two things now. A genuinely free tip is pushed back out, as before. A
tip that arrives at a junction is moved **onto** the junction, which it was not before —
it was left wherever thinning had dropped it.

`scripts/check-junctions.py` measures it. It reads the finished SVG, finds pairs of free
ends close enough for their discs to interfere, and reports how far apart they sit. It says
nothing about ends further apart than that, because which ends are meant to be one junction
is topology the SVG does not carry, and it cannot tell a real gap from an intended one.

| | ragged pairs | worst gap |
|---|---|---|
| the mark this replaced | 10 of 10 | 20.9/1000 |
| this mark | 5 of 18 | 11.9/1000 |

The five that remain are all the same crossing, where thinning reported two clot centres
11.9 apart instead of one. Merging clots that close was tried and **rejected**: it takes the
junctions to zero, and costs 2.2 points of fidelity to the drawing (94.2% of its ink to
91.9%) because it drags stroke ends to centres the drawing does not have. A cusp a fifth of
a stroke wide is the cheaper of the two, and at 22px it is a quarter of a pixel.

Two other changes were tried against the same measurement and are not here, which is the
point of writing the measurement down: reading the tangent clear of the clot before pairing
strokes (93.54% against 93.58% — slightly worse), and widening the stroke to fill the
crossings (every step worse, because the extra ink outruns what it fills).

## Source and licence

**The shipped mark is not derived from the CODH plates.** It was traced from a monoline
drawing of 屠 generated with Google Gemini. This matters, and it cuts the right way: the
share-alike problem the CODH trace carried is gone.

The **letterform** is public domain regardless of who draws it — 小篆 was standardised
around 220 BCE and nobody owns the shape of a character. What changes between one mark and
the next is only whether anything encumbers the *particular rendering*, and here nothing
does. Under Google's terms the output is the user's to use commercially, and the underlying
shape was never anyone's to license.

The `traced/` and `uniform/` directories **are** CODH-derived and keep their attribution:

> 『篆書字体データセット』（国文学研究資料館が複数の機関から収集／情報・システム研究機構
> データサイエンス共同利用基盤施設 人文学オープンデータ共同利用センター・一般財団法人
> 人文情報学研究所加工）[doi:10.20676/00000390](https://doi.org/10.20676/00000390)

**CC BY-SA 4.0**, the same licence the app carries for Jitendex, JMnedict, KANJIDIC2 and
Japanese WordNet, attributed the same way in `project notes` §8. Those files are an archive for
comparison and nothing renders them; the attribution stands whether or not they ship,
because that is how this project treats every source it draws on.

One caveat worth keeping honest: the earlier note here said that if the wordmark ever needs
to be exclusively ours, the answer is an original drawing. That is still true. A generated
image is not exclusive either — nothing stops the same prompt producing a near-identical
mark for someone else, and generated output is thin ground for a trademark claim. What it
is free of is *share-alike*, which is a different and lesser problem than it replaces.

## The two sets

Kept side by side so the mark can be judged against alternatives rather than on its own.
**Nothing here ships** — the shipped mark is `mono/`, above. These are the woodblock traces
that came before it, and they are an archive.

| | `traced/` | `uniform/` |
|---|---|---|
| `carved-plain` | 2,510 B | 1,237 B |
| `carved-fine` | 4,615 B | 2,507 B |
| `shuowen` | 5,899 B | 4,112 B |
| `*-seal` | the impression treatment | the impression treatment |

The `*-seal` files record the 白文 impression the mark used to be. They are kept because
the comparison is the point of this directory, not because anything renders them.

`traced/` follows the plate exactly and keeps everything the woodblock and the paper put
there. `uniform/` keeps the same silhouette but straightens it: every edge within 20° of an
axis is made exactly horizontal or vertical, and each corner is then moved to where its two
cleaned edges now intersect rather than left where the wobble had it. Nothing moves that was
not already within tolerance, so the letterform survives and the paper grain does not.

That is a defensible thing to do to **this** script and not to most others. 摹印篆 is built
from horizontals and verticals cut to fill a square, so squaring it up is restoring the
intent; doing the same to a running hand would be vandalism.

The `uniform/` epsilons are coarser than their `traced/` counterparts (0.006 and 0.0025
against 0.0025 and 0.0012) because regularising needs segments long enough to have a
direction — snapping a chain of three-pixel segments to the axis makes a staircase, not a
straight line.

### The approach that did not work

The first attempt was the more obvious one: skeletonise the plate, measure the stroke weight
from a distance transform, and redraw the centreline at one constant width. It is the right
idea for a script of even strokes, and on a 235-pixel woodblock print it fails — recovering
the topology from a scan that small means strokes which merely touch get fused into a blob,
and every crossing sprouts stubs. Chaining strokes through junctions by direction fixed the
fraying but not the fusing.

Regularising the contour needs no topology at all: the trace already has the silhouette
right, and only the wobble is in question. That is why `uniform/` is a contour treatment.

## Making it again

### The shipped mark

`scripts/trace-monoline.py` takes the drawing to stroked paths — centrelines, measured
weight, curves fitted. The skeleton approach that failed on the woodblock plates is the
right one *here*, and for the reason it failed there: it has to recover topology from the
image, which needs strokes of even weight and clean edges. A 235-pixel print has neither. A
900-pixel monoline drawing has both. 15 strokes, 120 cubics, 55/1000 wide, about 4.9 kB.

**Fit curves, do not simplify to chords.** The first version ran `approxPolyDP` over the
skeleton and emitted 50 straight segments and no curves at all, which turned every sweep in
the drawing into a chain of flat facets — a chamfer where 尸 turns, an octagon for 日. It
was blamed on the downsampler; it was the geometry. Nothing resamples the mark in the app
at all, because a browser draws the SVG.

Two things make the fitting work:

- **Smooth before fitting.** A skeleton is one pixel wide on a pixel grid, so it stair-steps
  even where the drawing was smooth. A short moving average takes that off without moving
  the shape.
- **Cut at real corners first.** 屠 has genuine corners — the elbow of 尸, the box of 日 —
  and a curve run through one rounds it off. Corners are found by the turn across a window
  of several points, never between adjacent ones: at one-pixel spacing every step is 0° or
  45°, so adjacent angles describe the grid rather than the drawing. Each stroke is then cut
  into spans and fitted between them. Smooth within a span, sharp at the join.

Catmull-Rom converted to Bézier, so the curve passes **through** the traced points rather
than being pulled near them.

### Two things the trace loses, and gets back

Both were found by overlaying the trace on the drawing and looking at what disagreed, which
is worth doing before believing any of this is faithful. Red for what the drawing has and
the trace does not, blue for the reverse; the systematic errors stand out immediately from
the sub-pixel fringe that is just alignment.

**Free ends come up short.** Thinning stops before a rounded cap and the smoothing pass then
drags the last point inward, so every free tip is missing a crescent of ink. Ends that reach
a junction are pinned there and must not be grown. Extending by 0.15 of the stroke width
halves the disagreement; 0.3 and above make it worse again, so this is swept rather than
reasoned about — thinning alone only eats 0.2 of a width *in total*, which is not enough to
explain what the overlay showed.

**The whisker cutoff ate real strokes.** Runs with a loose end shorter than the cutoff are
dropped as thinning artefacts. At `stroke * 0.8` that is 53px, and the two nubs on top of 日
are 45px — so they were deleted, the 日 came out with its corners missing, and it looked
like a drawing error rather than a filter. Every genuine whisker on this drawing measures
**2px**. The cutoff is `0.25` now, which sits in a very wide gap with room on both sides.

What remains is the diagonal, about a stroke-width short and slightly bent where it crosses
two other strokes at once. That is junction reconstruction, and it is left alone.

Measure stroke weight over the **skeleton**, not over every ink pixel. The median distance
across the whole mask is roughly half the true width — the ridge is what carries it — and
guessing from the mask suggested this drawing was half the weight of the one before it when
in fact the two match almost exactly.

`cv2.imread` goes through the ANSI file API on Windows and cannot open a path with Japanese
in it, which this project's paths routinely have. Read the bytes and `cv2.imdecode` them.

### The archive

The plates: <https://codh.rois.ac.jp/tensho/unicode/U+5C60/> — 45 images across 12 sources.
`TE00029` is 印篆貫珠, the largest at 235×332, and is where `traced/carved-*` comes from;
`TE00039` is 説文解字 and is where `traced/shuowen` does.

```python
# pip install opencv-python numpy
import cv2, numpy as np

image = cv2.imread("TE00029.jpg", cv2.IMREAD_GRAYSCALE)
image = cv2.resize(image, None, fx=4, fy=4, interpolation=cv2.INTER_CUBIC)
image = cv2.GaussianBlur(image, (5, 5), 0)
_, binary = cv2.threshold(image, 0, 255, cv2.THRESH_BINARY_INV + cv2.THRESH_OTSU)

kernel = cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (5, 5))
binary = cv2.morphologyEx(binary, cv2.MORPH_CLOSE, kernel)   # pinholes in the ink
binary = cv2.morphologyEx(binary, cv2.MORPH_OPEN, kernel)    # grain in the paper

contours, _ = cv2.findContours(binary, cv2.RETR_CCOMP, cv2.CHAIN_APPROX_SIMPLE)
# then approxPolyDP at 0.0025 of each perimeter, normalise into a 1000-unit box
# preserving the aspect, and emit one even-odd path so the counters stay holes.
```

Upscale **before** thresholding — the plates are small and the threshold needs something
smoother than a 235-pixel woodblock print to bite on. Otsu rather than a fixed level,
because every plate is a different exposure of ink on aged paper.

At 0.0025 the result is 2,510 bytes over 6 subpaths. Lower is smoother and larger; the
difference is invisible at rail size, and the slight angularity suits a carved letter.

`TE00039` is the 説文解字 plate and traces to 8 subpaths. Both plates take the same relative
cut — anything under a thousandth of the main mass is dirt — so the counts are comparable
between the two sets.

The impression files were reconstructed rather than drawn: when the mark was 白文,
`Mark.svelte` built the plate in CSS from a box 1.35× the glyph, 0.16 of padding and a 0.14
corner, which puts the glyph at 1.03/1.35 of the plate and the radius at 0.14/1.35 of it.
The SVGs here write those numbers out with `--red-strong` (`#cc4f63`) as a literal, so the
files can just be opened. The component no longer works that way — these are an archive of
what it did, not a description of what it does.

## The app icons

`src-tauri/icons/` is generated from the **seal** variant, so the taskbar gets the mark
stamped rather than bare: a solid plate is a stronger shape at icon sizes than an outline
on nothing, and the rail — where the mark sits on the app's own surface with room around
it — keeps the plain one.

```
python scripts/render-mark.py src-tauri/icons/source.png 1024 seal
cd src-tauri && cargo tauri icon icons/source.png
```

The script reads `mono/<variant>.svg` and **refuses to run if any of its paths are missing
from `Mark.svelte`**. It used to parse the component directly, which stopped being possible
once the plate moved behind an `{#if}` and the transform became a ternary. Reading the SVG
and cross-checking the component covers the only failure that matters — an icon that is no
longer the logo — without teaching a Python script to read Svelte.

The seal takes no margin because it brings its own plate; the plain variant takes 12% or it
reads as a crop. Legible to 32px, silhouette at 16.
