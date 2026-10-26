<!--
  霞 — a band of stylised cloud at the foot of the reading area.

  Decoration, and nothing else: `aria-hidden`, `pointer-events: none`, no state, no
  motion. Strength and colour come from `--decor-cloud` and `--decor-cloud-tint`, which
  `App.svelte` writes from settings; an opacity of 0 is how it is switched off.

  ## The grammar, because the next cloud has to obey it too

  A cloud is bands of equal height stacked so they *touch*. A band holds either

    bars    stadiums one band tall — `<rect rx="7">` at `height="14"`
    a neck  a narrow path whose four corners flare outward on a quadratic, so it grows
            out of the bar above instead of butting into it

  Three things are load-bearing, and each was got wrong once before it was written down:

  1. **Bands touch.** Pitch them further apart than a bar is tall and they cannot merge;
     the cloud becomes a stack of separate lozenges.
  2. **A neck's flare must land on the FLAT of a bar above and below.** A bar is a
     stadium, so only the span between its round ends is a straight edge to join onto —
     for a bar `x..x+w` that is `x+7 .. x+w-7`. A flare reaching past that lands on a
     curve, or on nothing, and the join shows. Two of these six broke that rule.
  3. **The slots either side of a neck are the motif.** They are what reads as cloud. Fill
     every band edge to edge and there is nothing left to recognise.
  4. **A neck overlaps the bars by 3 units rather than meeting them.** Two shapes that abut
     exactly each anti-alias to about half coverage on the shared row, and `src over dst`
     leaves 0.75 — a lighter line through a shape that is one flat colour. It only shows
     when the edge lands on a fractional device pixel, which display scaling and the size
     setting arrange often enough. The overlap is invisible because it is the flare's own
     width and rule 2 already puts that inside solid ink.

  Quadratic fillets rather than arcs: `Q` with the control on the outer corner can only
  bow inward, where an `A` depends on sweep flags that are easy to get backwards and hard
  to see wrong at 6% opacity.

  Six compositions rather than one shape repeated — the variety is in the drawing, not in
  scaling one cloud up and down.
-->
<script lang="ts">
  let {
    /**
     * How far apart the clouds sit, as a multiplier over their placements.
     *
     * A prop and not a CSS custom property, because this multiplies coordinates *inside* the
     * SVG — the x of every placement and the viewBox's own width — and no custom property
     * reaches in there. `--decor-scale`, which is how big each cloud is drawn, is a CSS
     * transform on the element and stays a token.
     *
     * **The viewBox width is fixed and the pattern tiles.** Widening the viewBox with the
     * placements was the obvious move and it is wrong: `slice` scales by `max(sx, sy)`, and
     * `sy` only wins because the viewBox is wide against a pinned height. Halve the width
     * and `sx` takes over — measured, a 1400px window at spread 0.5 draws the clouds at 0.56
     * where they belong at 0.53, and a 4K window at 1.54. A spacing setting would have been
     * quietly changing their size.
     *
     * Tiling instead keeps the scale exactly where it is today and cannot go bald at either
     * end: the placements repeat every `5000 × spread`, so a low spread packs more copies
     * into the same band and a high one spreads fewer across it.
     */
    spread = 1,
  }: { spread?: number } = $props();

  /// The fourteen placements, as data rather than as literal elements.
  ///
  /// Levels are chosen by *bottom* edge, and that is the point. An earlier version varied
  /// the tops and let the shapes' differing heights (42 to 98) bring every bottom to the
  /// same line — six different clouds marching past in a row. No two neighbours are within
  /// 30 of each other, and no three form an even run.
  /// The viewBox width, fixed. See `spread` for why this is not allowed to move.
  const BAND = 5000;

  const PLACEMENTS = [
    { id: "c-reach", x: -70, y: 214 },
    { id: "c-wisp", x: 373, y: 136 },
    { id: "c-tower", x: 670, y: 148 },
    { id: "c-drift", x: 1064, y: 62 },
    { id: "c-bank", x: 1388, y: 198 },
    { id: "c-step", x: 1790, y: 126 },
    { id: "c-wisp", x: 2198, y: 246 },
    { id: "c-bank", x: 2494, y: 84 },
    { id: "c-reach", x: 2895, y: 162 },
    { id: "c-tower", x: 3339, y: 20 },
    { id: "c-drift", x: 3733, y: 192 },
    { id: "c-step", x: 4057, y: 102 },
    { id: "c-wisp", x: 4466, y: 254 },
    { id: "c-bank", x: 4762, y: 140 },
  ];

  /// The placements actually drawn: the pattern above, spread and then repeated until it
  /// covers the band.
  ///
  /// One tile past the end, because a tile only fills from its own origin — without the
  /// spare, the right edge of the band empties as `spread` grows. Anything that lands
  /// outside the viewBox is clipped by it and costs nothing.
  const tiled = $derived.by(() => {
    const period = BAND * spread;
    const copies = Math.ceil(BAND / period) + 1;
    return Array.from({ length: copies }, (_, tile) =>
      PLACEMENTS.map((cloud) => ({
        id: cloud.id,
        x: cloud.x * spread + tile * period,
        y: cloud.y,
      })),
    ).flat();
  });
</script>

<svg
  class="kasumi"
  viewBox="0 0 {BAND} 300"
  preserveAspectRatio="xMidYMax slice"
  aria-hidden="true"
  focusable="false"
>
  <defs>
    <g id="c-bank">
      <rect x="84" y="0" width="112" height="14" rx="7" />
      <path d="M114 11 L166 11 L166 14 Q161 14 161 19 L161 23 Q161 28 166 28 L166 31 L114 31 L114 28 Q119 28 119 23 L119 19 Q119 14 114 14 Z" />
      <rect x="42" y="28" width="196" height="14" rx="7" />
      <path d="M51 39 L89 39 L89 42 Q84 42 84 47 L84 51 Q84 56 89 56 L89 59 L51 59 L51 56 Q56 56 56 51 L56 47 Q56 42 51 42 Z" />
      <rect x="14" y="56" width="84" height="14" rx="7" />
      <rect x="119" y="56" width="91" height="14" rx="7" />
    </g>
    <g id="c-step">
      <rect x="21" y="0" width="98" height="14" rx="7" />
      <rect x="133" y="0" width="42" height="14" rx="7" />
      <path d="M51 11 L89 11 L89 14 Q84 14 84 19 L84 23 Q84 28 89 28 L89 31 L51 31 L51 28 Q56 28 56 23 L56 19 Q56 14 51 14 Z" />
      <rect x="0" y="28" width="210" height="14" rx="7" />
      <path d="M142 39 L180 39 L180 42 Q175 42 175 47 L175 51 Q175 56 180 56 L180 59 L142 59 L142 56 Q147 56 147 51 L147 47 Q147 42 142 42 Z" />
      <rect x="98" y="56" width="147" height="14" rx="7" />
    </g>
    <g id="c-drift">
      <rect x="42" y="0" width="77" height="14" rx="7" />
      <path d="M58 11 L89 11 L89 14 Q84 14 84 19 L84 23 Q84 28 89 28 L89 31 L58 31 L58 28 Q63 28 63 23 L63 19 Q63 14 58 14 Z" />
      <rect x="14" y="28" width="147" height="14" rx="7" />
      <path d="M93 39 L124 39 L124 42 Q119 42 119 47 L119 51 Q119 56 124 56 L124 59 L93 59 L93 56 Q98 56 98 51 L98 47 Q98 42 93 42 Z" />
      <rect x="35" y="56" width="105" height="14" rx="7" />
    </g>
    <g id="c-tower">
      <rect x="70" y="0" width="112" height="14" rx="7" />
      <path d="M100 11 L138 11 L138 14 Q133 14 133 19 L133 23 Q133 28 138 28 L138 31 L100 31 L100 28 Q105 28 105 23 L105 19 Q105 14 100 14 Z" />
      <rect x="28" y="28" width="189" height="14" rx="7" />
      <path d="M65 39 L103 39 L103 42 Q98 42 98 47 L98 51 Q98 56 103 56 L103 59 L65 59 L65 56 Q70 56 70 51 L70 47 Q70 42 65 42 Z" />
      <rect x="49" y="56" width="182" height="14" rx="7" />
      <path d="M163 67 L194 67 L194 70 Q189 70 189 75 L189 79 Q189 84 194 84 L194 87 L163 87 L163 84 Q168 84 168 79 L168 75 Q168 70 163 70 Z" />
      <rect x="21" y="84" width="112" height="14" rx="7" />
      <rect x="154" y="84" width="56" height="14" rx="7" />
    </g>
    <g id="c-wisp">
      <rect x="28" y="0" width="77" height="14" rx="7" />
      <path d="M44 11 L82 11 L82 14 Q77 14 77 19 L77 23 Q77 28 82 28 L82 31 L44 31 L44 28 Q49 28 49 23 L49 19 Q49 14 44 14 Z" />
      <rect x="7" y="28" width="126" height="14" rx="7" />
    </g>
    <g id="c-reach">
      <rect x="119" y="0" width="133" height="14" rx="7" />
      <path d="M163 11 L201 11 L201 14 Q196 14 196 19 L196 23 Q196 28 201 28 L201 31 L163 31 L163 28 Q168 28 168 23 L168 19 Q168 14 163 14 Z" />
      <rect x="35" y="28" width="245" height="14" rx="7" />
      <path d="M79 39 L124 39 L124 42 Q119 42 119 47 L119 51 Q119 56 124 56 L124 59 L79 59 L79 56 Q84 56 84 51 L84 47 Q84 42 79 42 Z" />
      <rect x="7" y="56" width="154" height="14" rx="7" />
      <rect x="182" y="56" width="84" height="14" rx="7" />
    </g>
  </defs>

  <!-- Fourteen placements of six shapes across the full width, so the visible slice is
       never bald. Only `x` takes the spread: the levels are what stop the band reading as a
       row, and stretching those would flatten it. -->
    {#each tiled as cloud}
      <use href="#{cloud.id}" transform="translate({cloud.x} {cloud.y})" />
    {/each}
</svg>

<style>
  .kasumi {
    position: absolute;
    left: 0;
    right: 0;
    /* Above the footer's rule, not behind it. `<footer>` is a child of `main`, so anchoring
       to `bottom: 0` put the band underneath it — the measured height comes from
       `bind:clientHeight` in App, because the footer wraps on a narrow window and a guess
       would be wrong exactly when it showed. */
    bottom: calc(var(--footer-h, 2rem) + var(--decor-cloud-pad, 1rem));
    /* The height is the fixed dimension, and therefore the one that sets the scale.
     *
     * `slice` scales by `max(sx, sy)`. With the viewBox 5000 wide and the height pinned,
     * `sy` (150/300 = 0.5) beats `sx` (width/5000) for any window under 2500px — so the
     * width only decides how much of the band is visible. A wider window gets *more* clouds
     * at the same size, where `width: 100%` gave it bigger ones.
     *
     * The viewBox is 300 tall for a band of 150: half of it is air, which is what lets the
     * clouds sit at genuinely different levels instead of on one line.
     *
     * `rem`, so it still follows the interface size — and `--decor-scale`, which is the
     * knob for saying "bigger" without going back to following the window. */
    width: 100%;
    height: calc(10rem * var(--decor-scale, 1));
    /* Behind the page's own content but above the ground `.app::before` paints.
     *
     * `main` is already `position: relative; z-index: 1`, so it is a stacking context, and
     * a negative-z child of one paints above that context's background and below its
     * in-flow content. `z-index: 0` would put the clouds *over* the passage: inside a
     * stacking context, a positioned element paints after non-positioned in-flow content. */
    z-index: -1;
    pointer-events: none;
    user-select: none;
    fill: var(--decor-cloud-tint);
    opacity: var(--decor-cloud);
    /* Softness, as a whole filter value so that off is `none` and not `blur(0px)`.
     *
     * **This does not move where the band paints, and the paragraph above stays true.** A
     * filter creates a stacking context, which is the usual way a decoration ends up on top
     * of the text it was meant to sit behind — but this element is positioned with a numeric
     * `z-index`, so it was a stacking context already. Nothing changes. */
    filter: var(--decor-cloud-blur);
  }
</style>
