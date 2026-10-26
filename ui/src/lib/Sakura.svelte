<!--
  桜 — a sprig down the rail, behind the buttons.

  Decoration only: `aria-hidden`, `pointer-events: none`, no state, no motion. Strength and
  colour come from `--decor-sakura` and `--decor-sakura-tint`, which `App.svelte` writes
  from settings; an opacity of 0 switches it off.

  ## What makes it read as sakura

  One petal, rotated five times. Two proportions decide whether it is a cherry blossom or
  a smudge, and both were got wrong before they were measured:

  - **The petal is broad**, so neighbours cross and the outline closes into a rosette.
    Narrow petals with gaps between them make a star.
  - **The valley between petals is deeper than the notch within one.** Reverse those and
    the flower reads as ten small lobes rather than five petals. The notch here cuts to
    89% of the petal's length — shallow on purpose.

  Line art rather than solid, which is the quieter of the two treatments and the one a
  watermark wants. `vector-effect="non-scaling-stroke"` so the stroke stays the same
  weight whatever the blossom is scaled to; without it a small blossom gets a hairline and
  a large one gets a slab.

  More loose petals than blossoms. The flowers are the event; the petals are what is left
  of it, drifting down a 46px rail.
-->
<script lang="ts">
  let {
    /**
     * How far apart the blossoms and petals sit down the rail.
     *
     * A prop, not a CSS custom property: this multiplies the y of every placement and the
     * viewBox's own height, and no custom property reaches inside an SVG's coordinates.
     * `--decor-scale` is how big each blossom is drawn and stays a token, because that one
     * is a CSS transform on the element.
     *
     * **The viewBox height is fixed and the pattern tiles**, for the reason the cloud band
     * does the same: growing the box with the placements changes how far the sprig reaches,
     * and at a low spread it would stop partway down a rail it used to fill. Repeating every
     * `1400 × spread` keeps the run the same length however tightly the blossoms sit in it.
     *
     * At `width: 100%` against a 46-wide viewBox the scale is 1 — user units are device
     * pixels — so spreading y widens the gaps directly.
     */
    spread = 1,
  }: { spread?: number } = $props();

  /// Nine blossoms and twenty loose petals, as data rather than as literal elements.
  ///
  /// More loose petals than blossoms, deliberately: the flowers are the event, the petals
  /// are what is left of it. `x` and the sizes are untouched by spread — a rail 46 across
  /// has nowhere sideways to go, and scaling is the other setting's job.
  /// The viewBox height, fixed. See `spread`.
  const RAIL = 1400;

  const BLOSSOMS = [
    { x: 25, y: 104, size: 0.150, turn: -12 },
    { x: 29, y: 212, size: 0.108, turn: 20 },
    { x: 20, y: 318, size: 0.126, turn: -34 },
    { x: 30, y: 430, size: 0.085, turn: 14 },
    { x: 22, y: 592, size: 0.132, turn: 28 },
    { x: 31, y: 726, size: 0.098, turn: -20 },
    { x: 19, y: 884, size: 0.118, turn: 40 },
    { x: 28, y: 1046, size: 0.090, turn: -8 },
    { x: 23, y: 1216, size: 0.126, turn: 16 },
  ];

  const PETALS = [
    { x: 34, y: 150, turn: 42, size: 0.055, fade: 0.55 },
    { x: 15, y: 176, turn: -28, size: 0.045, fade: 0.45 },
    { x: 36, y: 258, turn: 18, size: 0.062, fade: 0.6 },
    { x: 13, y: 286, turn: -52, size: 0.05, fade: 0.5 },
    { x: 33, y: 360, turn: 66, size: 0.048, fade: 0.5 },
    { x: 16, y: 388, turn: -16, size: 0.058, fade: 0.55 },
    { x: 28, y: 470, turn: 30, size: 0.052, fade: 0.5 },
    { x: 14, y: 502, turn: -40, size: 0.042, fade: 0.4 },
    { x: 32, y: 536, turn: 8, size: 0.056, fade: 0.5 },
    { x: 15, y: 648, turn: -34, size: 0.05, fade: 0.5 },
    { x: 35, y: 676, turn: 52, size: 0.044, fade: 0.45 },
    { x: 18, y: 790, turn: 22, size: 0.058, fade: 0.55 },
    { x: 33, y: 824, turn: -12, size: 0.046, fade: 0.45 },
    { x: 14, y: 942, turn: 60, size: 0.052, fade: 0.5 },
    { x: 34, y: 976, turn: -46, size: 0.042, fade: 0.4 },
    { x: 20, y: 1108, turn: 14, size: 0.056, fade: 0.5 },
    { x: 35, y: 1142, turn: -30, size: 0.046, fade: 0.45 },
    { x: 16, y: 1278, turn: 38, size: 0.05, fade: 0.45 },
    { x: 31, y: 1316, turn: -18, size: 0.042, fade: 0.4 },
    { x: 21, y: 1364, turn: 6, size: 0.036, fade: 0.35 },
  ];

  /// Spread and repeated down the rail, so the sprig runs its whole length at any spacing.
  /// One tile past the end for the same reason as the clouds: a tile fills only from its own
  /// origin, and what overshoots is clipped by the viewBox.
  function tile<T extends { y: number }>(items: T[], spacing: number): T[] {
    const period = RAIL * spacing;
    const copies = Math.ceil(RAIL / period) + 1;
    return Array.from({ length: copies }, (_, step) =>
      items.map((item) => ({ ...item, y: item.y * spacing + step * period })),
    ).flat();
  }

  const blossoms = $derived(tile(BLOSSOMS, spread));
  const petals = $derived(tile(PETALS, spread));
</script>

<svg
  class="sakura"
  viewBox="0 0 46 {RAIL}"
  preserveAspectRatio="xMidYMin meet"
  aria-hidden="true"
  focusable="false"
>
  <defs>
    <path
      id="petal"
      d="M0 0 C-36.1 -22.0 -38.8 -52.0 -36.5 -80.0 C-34.2 -99.0 -12.9 -105.0 -7.6 -94.0
         L0 -89.0
         L7.6 -94.0 C12.9 -105.0 34.2 -99.0 36.5 -80.0 C38.8 -52.0 36.1 -22.0 0 0 Z"
    />
    <g id="blossom">
      <use href="#petal" />
      <use href="#petal" transform="rotate(72)" />
      <use href="#petal" transform="rotate(144)" />
      <use href="#petal" transform="rotate(216)" />
      <use href="#petal" transform="rotate(288)" />
    </g>
  </defs>

  <g fill="none" stroke="var(--decor-sakura-tint)" stroke-width="1.4" vector-effect="non-scaling-stroke">
    {#each blossoms as flower}
      <use
        href="#blossom"
        transform="translate({flower.x} {flower.y}) scale({flower.size}) rotate({flower.turn})"
      />
    {/each}
  </g>
  <g fill="var(--decor-sakura-tint)">
    {#each petals as petal}
      <use
        href="#petal"
        transform="translate({petal.x} {petal.y}) rotate({petal.turn}) scale({petal.size})"
        opacity={petal.fade}
      />
    {/each}
  </g>
</svg>

<style>
  .sakura {
    position: absolute;
    top: 0;
    /* `height: auto`, not `100%`. At `100%` with `slice` the scale is `max(sx, sy)`, and in
     * a rail 46px wide by a whole window tall `sy` wins by a mile — so the blossoms grew
     * with the window. Matching the width to the viewBox's own width makes the scale 1: the
     * flowers are a constant size and move only with `--rail`, which is in `rem`.
     *
     * The SVG then overflows a shorter rail downward, where `.app`'s `overflow: hidden`
     * clips it. Ours would have to go on `.rail`, and that would also cut off the
     * `.tip-side` tooltips, which open outside it. */
    width: calc(100% * var(--decor-scale, 1));
    height: auto;
    /* Grown about the rail's centre rather than its left edge, so the sprig stays in the
       column. Past about 1.4 a blossom is wider than 46px and clips at the edges — which
       is inherent to scaling anything up inside a rail this narrow, and is why the
       setting stops where it does. */
    left: 50%;
    translate: -50% 0;
    /* Inside the rail's own stacking context, under the buttons at `1`. Negative would put
       it behind the rail's background and it would simply not be there. */
    z-index: 0;
    pointer-events: none;
    user-select: none;
    opacity: var(--decor-sakura);
    /* Softness, as a whole filter value so that off is `none` and not `blur(0px)`. Positioned
     * with a numeric `z-index` already, so the stacking context a filter creates is one this
     * element had — the note above about sitting under the buttons still holds.
     *
     * A blur smears past the element's own box, and nothing clips this one (see the note on
     * `width` — `overflow: hidden` would have to go on `.rail` and would take the tooltips
     * with it). That is why this slider stops at 3px where the clouds' goes to 8. */
    filter: var(--decor-sakura-blur);
  }
</style>
