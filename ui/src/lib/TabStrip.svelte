<script lang="ts">
  // The reference page's tabs, hanging off the bottom edge of the search box.
  //
  // They have been three things. Under the search box as a plain row; inside the window's caption
  // beside the close button, which is what Edge does and which squeezed them into 32px next to
  // controls they have nothing to do with; then as free-floating pills in a `--surface` band that
  // bled to the window's edges, imitating a browser's chrome. That last one is what this replaces.
  // The page does not imitate a browser any more — it flows like the glossary — so the tabs are
  // back under the box, and **joined** to it: the one you are on shares the field's fill and erases
  // the field's bottom edge across its own width, so the two read as one object. See `.tab` below
  // for how, because it is cheaper than the comment it replaced claimed.
  //
  // **A component rather than markup in the page, and it stays one.** Extracting it was what the
  // trip through the title bar required — scoped styles do not travel with copied markup, so tabs
  // written in `Reference.svelte` would have rendered as bare buttons up there with nothing in the
  // build to say so. Back in the page it is still the right shape: the strip is one thing, with one
  // set of rules, wherever it is asked to draw.
  //
  // Reads the `search` store directly, as `Reference.svelte` does. It is a module singleton, so
  // there is one strip and one list of tabs however many places draw them.
  import { search } from "./pages.svelte";
</script>

<!-- The scrolling strip, and beside it the one control that must not scroll away with it.

     **Every button in here is `type="button"`, and that is load-bearing.** This component renders
     *inside* the search `<form>` — it has to, because the join needs the strip to inherit the box's
     exact width and left edge — and a `<button>` in a form with no `type` is a **submit** button.
     Implicit submission does not merely submit: it clicks the form's *default* button, the first
     submit button in tree order, which was the first tab's label. So pressing Enter on a search
     jumped to tab one and left the tab you were typing in showing your term, because `Tab.label`
     falls back to `query`. It read as "the search went to the wrong tab and made a new one".

     Not `role="tablist"` on either. That role promises arrow-key navigation between tabs, which
     this does not implement; `aria-current` is the same thing the sidebar's views say and is true
     as written. -->
<div class="strip">
  <div class="tabs">
    <!-- Keyed by the tab itself, not by index. Closing the middle of three renumbers every tab
         after it, and an index key would leave the wrong one lit. -->
    {#each search.tabs as open, index (open)}
      <div class="tab" class:current={index === search.active}>
        <button
          type="button"
          class="name"
          lang="ja"
          onclick={() => (search.active = index)}
          aria-current={index === search.active ? "true" : undefined}
        >
          {open.label || "New search"}
        </button>
        <button
          type="button"
          class="shut"
          onclick={() => search.close(index)}
          aria-label={`Close ${open.label || "this tab"}`}
        >
          <svg viewBox="0 0 24 24" width="11" height="11" aria-hidden="true">
            <path
              d="M7 7L17 17M17 7L7 17"
              fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"
            />
          </svg>
        </button>
      </div>
    {/each}
    <!-- The control the strip gained back. It was deleted once for being a second way to do what
         Search did anyway; searching navigates in place now, so this is the only way to keep an
         answer and ask something else. The box focuses itself when the new tab arrives — see the
         effect in `Reference.svelte`.

         **No `data-tip`, and it cannot have one.** `[data-tip]::after` is absolutely positioned
         below its anchor, and `.tabs` is a scrollport — `overflow-x: auto` with `overflow-y:
         hidden` — which clips it. What reached the screen was a two-pixel band of the tooltip's
         top border under the button, on hover, reading as a stray underline. Nothing about the
         tooltip was wrong; it was drawn inside a box that crops. `aria-label` still names the
         button, and a `+` at the end of a tab strip is as conventional as a control gets. -->
    <button type="button" class="add" onclick={() => search.open()} aria-label="New search">
      <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
        <path
          d="M12 5v14M5 12h14"
          fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"
        />
      </svg>
    </button>
  </div>

  <!-- Outside `.tabs`, which is what lets it keep a tooltip: `.strip` does not scroll, so there
       is no scrollport to crop the panel. It is also the reason it stays reachable — pinned at
       the end rather than scrolling away exactly when a crowded strip makes it worth pressing.

       Only past one tab. Clearing a single empty tab is a control that does nothing, and this is
       the shape the recent-searches row's Clear had: same chip, red only under the pointer. -->
  {#if search.tabs.length > 1}
    <button
      type="button"
      class="clear-all tip-right"
      onclick={() => search.clearAll()}
      data-tip="Close every tab and start again"
    >
      Clear all
    </button>
  {/if}
</div>

<style>
  /* **The negative margin is the whole trick — do not tidy it away.**
   *
   * It rides the row up by exactly the width of the search box's border, so every tab's own top
   * border lands *on* that border rather than below it. What each tab then paints there is what
   * joins it to the box or leaves it hanging under it. See `.tab`.
   *
   * On the wrapper rather than on `.tabs`, so the scrolling part and the button pinned beside it
   * ride up together and their tops agree. */
  .strip {
    display: flex;
    align-items: flex-start;
    gap: var(--gap-sm);
    flex: 0 0 auto;
    margin-top: -1px;
  }

  /* Scrolls rather than wraps: a second row of tabs would push the results down the page every
   * time one was opened, and the thing being read would move. `thin` and a gutter, matching the
   * chip row that used to sit here — the strip needed `scrollbar-width: none` while the tabs
   * shared the width with `flex: 1 1 0`, because a fraction of a pixel of rounding armed a
   * permanent bar under a strip that fitted. They size to their labels now, so there is nothing
   * to round and a bar only appears when one is genuinely needed.
   *
   * **This is a scrollport, so nothing inside it may own a `data-tip`.** `overflow-x: auto` with
   * `overflow-y: hidden` crops absolutely positioned descendants, and a tooltip is one — see the
   * `+` button's comment above for what that looked like. Controls that need a tooltip go in
   * `.strip`, outside this. */
  .tabs {
    display: flex;
    align-items: flex-start;
    gap: var(--gap-xs);
    flex: 1;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: thin;
    /* Gutter, so the scrollbar does not sit on the tabs it is scrolling. */
    padding-bottom: 2px;
  }

  /* **Joined to the search box, and it costs one pixel rather than four pseudo-elements.**
   *
   * The comment that used to be here talked this out of existence. It said the joined look "needs
   * an outward corner where the tab meets the bar, and the only CSS for it is four pseudo-elements
   * per tab (background-coloured circles over squares) whose overlaps swallow clicks on the
   * neighbouring tab" — all true, and about the *flare*, not the join. Conflating the two is what
   * sent this to free-floating pills and then to a band imitating a browser. Without the flare:
   *
   * - `.tabs` above rides up 1px, so a tab's top border sits exactly on the field's bottom border.
   * - An inactive tab repaints that pixel in `--border`, the colour already there. Nothing appears
   *   to change: the tab simply hangs off a line the field had drawn anyway.
   * - The current tab repaints it in `--surface` — **the field's fill, not the page's** — which
   *   erases the field's bottom edge across the tab's width. The two become one shape.
   *
   * Every tab carries the same overlap, so their contents stay on one baseline; only the colour of
   * that one border differs between them.
   *
   * Square-topped and rounded below, because the top is where it meets the field. `--radius-sm`,
   * the same corner the field itself uses, or the tab would read as a different family of object
   * hanging off it. */
  .tab {
    display: flex;
    align-items: center;
    flex: 0 0 auto;
    max-width: 13rem;
    /* A constant edge. It briefly followed a `--bar-edge` that turned accent on focus, along with
       the bar and every other tab — five elements lighting up to report one cursor. What separates
       the open tab is its *fill* and its erased top, which is the distinction that matters: one
       belongs to the bar, the others sit behind it. */
    border: 1px solid var(--border);
    border-radius: 0 0 var(--radius-sm) var(--radius-sm);
    /* The page's own ground, so an inactive tab reads as sitting *behind* the current one rather
       than beside it. `--surface` here would make all of them look open. */
    background: var(--bg);
    transition:
      background var(--anim) var(--ease),
      border-color var(--anim) var(--ease);
  }
  .tab:hover:not(.current) {
    background: var(--surface-hover);
  }
  .tab.current {
    /* **`--bar-fill`, not a fixed `--surface`, and that is what keeps the join invisible while the
       bar changes colour.** The bar lifts a step when the pointer is over it; a tab pinned to
       `--surface` would suddenly differ from the thing it is joined to, drawing a seam in exactly
       the place the join exists to remove. The property is set on `.field` in `Reference.svelte`
       and *inherits* down into this component, which a selector could not do — scoped styles do
       not cross a component boundary, but custom properties do. The fallback covers any other page
       that ever draws this strip. */
    background: var(--bar-fill, var(--surface));
    /* The sides and bottom follow the bar's edge, so this stays part of the outline rather than a
       shape with a stale border hanging off a bar that has lifted. **Shorthand first, then the top**
       — `border-color` sets all four, so the erasing pixel below has to be written after it. */
    border-color: var(--border);
    /* The erasing pixel. It has to be the bar's own fill; `transparent` would let the bar's border
       show through, and `--bg` would cut a page-coloured line across the join. */
    border-top-color: var(--bar-fill, var(--surface));
    /* So the border above beats the bar's, whatever else ends up painting near this. Sibling
       order already wins it — the strip comes after the bar — but the join is the one thing on
       this page that a later positioned element could silently break. */
    position: relative;
    transition:
      background var(--anim) var(--ease),
      border-color var(--anim) var(--ease);
  }

  .tab .name {
    flex: 1;
    min-width: 0;
    padding: 0.25rem 0.2rem 0.25rem 0.55rem;
    border: 0;
    background: none;
    color: var(--text-faint);
    font-size: 0.82rem;
    /* One line, cut where it runs out. A tab is a label for a search, not the search. */
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
    transition: color var(--anim) var(--ease);
  }
  .tab.current .name {
    color: var(--text);
  }
  .tab .name:hover {
    color: var(--text);
  }
  .tab .shut {
    display: grid;
    place-items: center;
    width: 1.1rem;
    height: 1.1rem;
    margin-top: 0.25rem;
    margin-right: 0.25rem;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-faint);
    transition:
      color var(--anim) var(--ease),
      background var(--anim) var(--ease);
  }


  /* Beside the tabs rather than one of them, so it carries none of the join: no border, no fill,
     and centred against the row instead of hanging off the field. */
  .add {
    display: grid;
    place-items: center;
    flex: 0 0 auto;
    width: 1.9rem;
    /* **Stretch, with no height of its own, so the glyph lands on the tabs' own centre line.**
       At a fixed `1.6rem` against `align-items: flex-start` it was a shorter box hung from the top
       of the row, which put the `+` a pixel or two above the middle of the labels beside it —
       close enough to look like a mistake rather than a difference. Filling the row and centring
       inside it makes the two agree by construction, whatever the tabs' padding becomes.
       An explicit `height` would win over `stretch`, so there must not be one. */
    align-self: stretch;
    margin-top: 0.2rem;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: var(--text-faint);
    transition:
      color var(--anim) var(--ease),
      background var(--anim) var(--ease);
  }
  /* **Colour alone, no fill.** A `--surface-hover` box lit up behind the glyph, and beside tabs
     that are themselves outlined boxes it read as a third kind of tab appearing out of nowhere
     rather than as a button answering the pointer. Brightening the glyph says the same thing
     without adding a shape to a row that is already made of them. */
  .add:hover {
    color: var(--text);
  }

  /* **The recent row's Clear, joined to the search box the way the open tab is.**
   *
   * It wore `.control` first, which was the safe answer for a chip and the wrong one for this: a
   * chip has a fill one step up from the page and a radius on all four corners, and neither can
   * flow into the box above it. So it is built like `.tab.current` instead — square top, rounded
   * bottom, the box's own `--surface`, and a top border painted in that same fill so the box's
   * bottom edge is erased across its width. Read `.tab` above for the mechanism; this is the same
   * three declarations.
   *
   * **Opting out of `.control` is deliberate, not drift.** The tabs are not `.control` either, for
   * exactly this reason — a shared chip cannot be a joined shape. `where.md` records the pair.
   *
   * The height matches the tabs by arithmetic rather than by eye: the same `0.82rem` and the same
   * `0.25rem` of vertical padding `.tab .name` carries, inside the same 1px border. Anything else
   * and its bottom edge would sit off the tabs' by a pixel or two.
   *
   * **Colour alone on hover, no fill and no edge.** It used to take `--red-edge` on its border
   * too, which cannot work here: `border-color` sets all four sides, so hovering repainted the
   * one border pixel that erases the box's edge and drew a red line straight through the join.
   * The text going red says the same word — the same word the tab's own ✕ says — and leaves the
   * join alone. Red only under the pointer: sitting red permanently would put a warning colour
   * beside the tabs at all times, which is a lot of alarm for a control you press once. */
  .clear-all {
    flex: 0 0 auto;
    /* **Exactly the block above it**, so the bar's end and this read as one column rather than two
       things that happen to finish at the same place. Its width used to be its text's, which landed
       two pixels shy of the divider by coincidence.
     *
     * The two controls in the bar are `--bar-button` each, and the `+ 1px` is the bar's own right
     * border: they sit *inside* that border and this sits outside it, so covering their outer span
     * means covering it too. Both numbers come from `.field` in `Reference.svelte`.
     *
     * No horizontal padding, because at that width there is none to spare. The label centres in it
     * instead, and `nowrap` keeps it on one line if the interface size is turned up. */
    width: calc(var(--bar-button, 2.2rem) * 2 + 1px);
    padding: 0.25rem 0;
    text-align: center;
    white-space: nowrap;
    font-size: 0.82rem;
    border: 1px solid var(--border);
    /* The erasing pixel, as on `.tab.current`: the bar's own fill painted over the bar's bottom
       edge, so this hangs off the bar rather than sitting under it. `--bar-fill` rather than a
       fixed `--surface` because the bar changes colour under the pointer, and a fixed value would
       draw a seam across the join at exactly that moment. */

    border-radius: 0 0 var(--radius-sm) var(--radius-sm);
    background: var(--bar-fill, var(--surface));
    color: var(--text-faint);
    /* So the erasing border above wins against the bar's own, as on `.tab.current`. */
    position: relative;
    transition:
      color var(--anim) var(--ease),
      background var(--anim) var(--ease),
      border-color var(--anim) var(--ease);
  }
  .clear-all:hover:not(:disabled) {
    color: var(--red);
  }
</style>
