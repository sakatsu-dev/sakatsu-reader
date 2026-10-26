<script lang="ts">
  // A resizable split with a draggable divider. `ratio` is the first pane's share.
  //
  // Two directions: a column, which is the text above and the study area below
  // (project notes §3), and a row, which is vocabulary beside grammar. One component rather
  // than two, because everything that makes this worth having — clamping, capturing the
  // pointer so a fast drag does not drop the grab, being operable from the keyboard — is
  // the same either way and only the axis differs.
  import { clampRatio, snapToCentre } from "./split.js";

  let {
    ratio = $bindable(0.45),
    min = 0.2,
    max = 0.8,
    direction = "column",
    firstMin = "0",
    fill = true,
    snapWithin = 0,
    onuserresize = () => {},
    first,
    second,
  }: {
    ratio?: number;
    min?: number;
    max?: number;
    /** Maps straight onto `flex-direction`: panes stacked, or side by side. */
    direction?: "column" | "row";
    /**
     * A floor for the first pane, as a CSS length — anything `min-height` accepts.
     *
     * `min` and `max` are shares, which is what should be *stored*, and useless for saying "this
     * pane must fit one line of Japanese". The two are clamped together: see `split.js` for why
     * leaving it to flexbox alone silently corrupts the saved ratio.
     */
    firstMin?: string;
    /** Fill the available block axis, or stop at the panes' natural content height. */
    fill?: boolean;
    /**
     * How near the centre, **in pixels**, a drag has to come before it lands exactly on it.
     *
     * `0` is off, which is what every split gets unless it asks. Only the Vocabulary/Grammar
     * split does: even panes are the position somebody wants back, and `0.5` is not a number a
     * pointer can hit. The passage split has no such position — it divides a reading area from a
     * study area, and half and half means nothing there — so it stays off.
     *
     * Pixels rather than a share so the zone is the same width to the hand whatever the window
     * is doing; see `split.js`.
     */
    snapWithin?: number;
    /** Called only for pointer or keyboard changes, never for a bound programmatic fit. */
    onuserresize?: (ratio: number) => void;
    first: import("svelte").Snippet;
    second: import("svelte").Snippet;
  } = $props();

  let container: HTMLDivElement | undefined = $state();
  let firstPane: HTMLDivElement | undefined = $state();
  let dragging = $state(false);

  const sideways = $derived(direction === "row");

  /**
   * The first pane's floor in pixels, read back from the DOM rather than parsed here.
   *
   * `firstMin` is a CSS length and is very often a `calc()` over custom properties — the reading
   * pane's floor is one line of Japanese at whatever scale the reader is set to. Resolving that
   * is the browser's job, and asking it means the number stays written where it belongs instead
   * of being duplicated in arithmetic that would drift.
   */
  function floor() {
    if (!firstPane) return 0;
    const style = getComputedStyle(firstPane);
    return Number.parseFloat(sideways ? style.minWidth : style.minHeight);
  }

  function clamp(value: number, span?: number) {
    return clampRatio(value, {
      min,
      max,
      span: span ?? (container ? measure(container) : 0),
      floor: floor(),
    });
  }

  function measure(el: HTMLElement) {
    const box = el.getBoundingClientRect();
    return sideways ? box.width : box.height;
  }

  /**
   * Snap, then clamp — and in that order.
   *
   * `clampRatio` is the final authority on where the divider may sit; it honours the pane's pixel
   * floor over even the share ceiling. A snap applied afterwards could put the divider back
   * outside it. For the one split that snaps today the centre is always inside its bounds, so the
   * order never shows; it is right anyway, and the next caller may not be so lucky.
   */
  function moveTo(event: PointerEvent) {
    if (!container) return;
    const box = container.getBoundingClientRect();
    const span = sideways ? box.width : box.height;
    const asked = sideways ? (event.clientX - box.left) / span : (event.clientY - box.top) / span;
    ratio = clamp(snapToCentre(asked, { span, within: snapWithin }), span);
    onuserresize(ratio);
  }

  function start(event: PointerEvent) {
    dragging = true;
    onuserresize(ratio);
    // Capture the pointer so the drag survives the cursor leaving the divider —
    // otherwise a fast drag drops the grab.
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
  }

  function move(event: PointerEvent) {
    if (dragging) moveTo(event);
  }

  // The divider is a real separator, so it is keyboard-operable too.
  //
  // **No snapping here, deliberately.** The step is 0.02 and the snap zone is wider than that, so
  // snapping would pull every step back to the centre: the divider would reach the middle and be
  // unable to leave it by keyboard, in either direction, for good. It is also the right behaviour
  // on its own terms — arrow keys are already discrete and exact, and somebody pressing one is
  // asking for a step, not for help hitting a target.
  function key(event: KeyboardEvent) {
    const step = event.shiftKey ? 0.1 : 0.02;
    const [less, more] = sideways ? ["ArrowLeft", "ArrowRight"] : ["ArrowUp", "ArrowDown"];
    if (event.key === less) ratio = clamp(ratio - step);
    else if (event.key === more) ratio = clamp(ratio + step);
    else return;
    onuserresize(ratio);
    event.preventDefault();
  }
</script>

<div class="split" class:sideways class:content-sized={!fill} bind:this={container}>
  <div
    class="pane"
    bind:this={firstPane}
    style="flex: {ratio}; {sideways ? 'min-width' : 'min-height'}: {firstMin}"
  >
    {@render first()}
  </div>

  <!-- A focusable separator is a valid ARIA widget (it reports aria-valuenow and
       responds to arrow keys); Svelte's a11y lint treats every separator as
       non-interactive and so cannot see that.

       `data-keeps-focus` exempts it from the app-wide rule that a control releases focus once
       its click is dealt with (`onWindowClick` in `App.svelte`). Unlike a button it cannot
       misfire — Enter and Space do nothing to a separator — and with Tab swallowed, the click
       that grabs focus is the only way its arrow keys can ever be reached. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="divider"
    class:dragging
    role="separator"
    aria-orientation={sideways ? "vertical" : "horizontal"}
    aria-valuenow={Math.round(ratio * 100)}
    aria-label={sideways ? "Resize the panels" : "Resize the reader"}
    tabindex="0"
    onpointerdown={start}
    onpointermove={move}
    onpointerup={() => (dragging = false)}
    onpointercancel={() => (dragging = false)}
    onkeydown={key}
    data-keeps-focus
  ></div>

  <div class="pane" style="flex: {1 - ratio}">{@render second()}</div>
</div>

<style>
  .split {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }
  /* The passage split owns a fixed workspace and must fill it. The Vocabulary/Grammar split can
     instead contribute its natural cross-size so a short selected-word card does not impersonate
     a window-sized panel; `flex-shrink` still lets long entries stop at the available height and
     hand overflow to their pane bodies. */
  .split.content-sized {
    flex: 0 1 auto;
  }
  .split.sideways {
    flex-direction: row;
    /* Panes are flex-sized from `ratio`, and a flex item will not shrink below its content
       without this — a long definition would otherwise push the divider off its ratio. */
    min-width: 0;
  }
  .pane {
    min-height: 0;
    /* Split owns geometry, its children own scrolling. Letting every nesting level scroll is
       what allowed long Vocabulary content to paint under the footer before the outer pane
       noticed it had overflowed. */
    overflow: hidden;
  }
  .sideways .pane {
    min-width: 0;
  }

  .divider {
    flex: 0 0 auto;
    height: 10px;
    margin: var(--gap-xs) 0;
    border-radius: 5px;
    cursor: row-resize;
    position: relative;
    touch-action: none;
  }
  .sideways .divider {
    height: auto;
    width: 10px;
    /* A real gutter. The panes carry no horizontal padding of their own, so this is the
       only thing between one pane's text and the next — at `--gap-xs` the two columns ran
       into the grip from both sides. */
    margin: 0 var(--gap);
    cursor: col-resize;
  }
  /* The grip, not the whole bar — a full-width slab reads as a border and invites
     nobody to drag it. */
  .divider::after {
    content: "";
    position: absolute;
    inset: 4px 42%;
    border-radius: 2px;
    background: var(--border-strong);
    transition: background var(--anim) var(--ease);
  }
  .sideways .divider::after {
    inset: 42% 4px;
  }
  .divider:hover::after,
  .divider.dragging::after {
    background: var(--accent);
  }
</style>
