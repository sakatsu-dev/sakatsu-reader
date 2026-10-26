<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { emit, listen } from "@tauri-apps/api/event";
  import { defaultTheme, watchTheme, type Theme } from "./theme-live";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  // The persistent capture frame. This renders in its own transparent, always-on-top
  // window that stays where it is put: position it over a column of text once, then read
  // it as many times as you like.
  //
  // **Nothing may paint the interior — unless the frame leaves the shot.** The frame does
  // not photograph itself because the desktop composites the window underneath straight
  // through its transparent middle, and because `read_frame` insets past the border and the
  // grip before capturing. Anything drawn over the region lands in the OCR, which is why
  // the border was for a long time the only affordance here.
  //
  // The exception, and its price: `hide_during_capture` takes the whole window out of the
  // picture for ~120ms per read. That is the only condition under which the region may be
  // tinted, and `Settings::normalise` turns it on rather than trusting a checkbox to.
  //
  // Four numbers here must agree with `commands.rs`: the border with FRAME_BORDER, the
  // grip's height with FRAME_GRIP — together they are what a capture skips — and the
  // handles' `--thickness` and `--corner` with FRAME_HANDLE and FRAME_CORNER, which are
  // where click-through stops passing clicks on. None of the four fails loudly when it
  // drifts.

  // There is no Read button here, and no close button either. Reading is started by the
  // capture binding or by the main window's Capture button; closing is the header's overlay
  // toggle, which calls `close_frame`, or a hold on the show/hide binding. Every one of
  // those already exists, and a control here would be another way to do the same thing on
  // the one surface that has to stay out of the way of what it is pointed at.

  /// A wash over the region. Zero is off, and off is the only state in which this window
  /// paints nothing over what it is pointed at.
  ///
  /// The frame carries no controls any more — lock and click-through are in the main
  /// window's header, which is also where their events were already handled, so the round
  /// trip through this webview is gone with them.
  let tint = $state(0);

  invoke<{ frame_tint: number }>("settings")
    .then((saved) => {
      tint = saved.frame_tint;
    })
    .catch(() => {});

  // And again whenever it changes, which reading it once could never see.
  //
  // This window is created on the first `show_frame` of the session and only ever hidden
  // after that — `close_frame` hides, it does not destroy — so this component mounts once.
  // The read above was therefore the only read there ever was: moving the slider wrote the
  // store, and the overlay went on wearing its launch-time tint until the app was restarted.
  listen<number>("frame-tint", (event) => (tint = event.payload));

  // And the palette, for exactly the same reason and by exactly the same route. This window is a
  // separate document, so custom properties written onto the main window's root never arrive
  // here; `main.ts` paints it once at startup and this keeps it in step afterwards.
  let theme = $state<Theme>(defaultTheme());
  invoke<{ theme?: Theme }>("settings")
    .then((saved) => {
      if (saved?.theme) theme = saved.theme;
    })
    .catch(() => {});
  $effect(() => watchTheme(document.documentElement, () => theme));


  // Resize. Eight handles, each pinning the edges it does not move.
  //
  // Native edge-resize on an undecorated window is inconsistent across platforms, so the
  // handles are explicit. Pointer capture keeps a fast drag from escaping the handle —
  // the same reason Split.svelte captures.
  type Edge = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

  let dragging: {
    edge: Edge;
    startX: number;
    startY: number;
    rect: { x: number; y: number; width: number; height: number };
  } | null = null;

  /// The floor a resize holds itself to, asked for rather than restated.
  ///
  /// Rust owns these — `FRAME_MIN_WIDTH` and `FRAME_MIN_HEIGHT` — and `place_frame` enforces
  /// them whatever arrives. Fetching them keeps one source: a copy here would be a second
  /// number to keep in step, and the two disagreeing is how a handle stops moving for no
  /// visible reason.
  ///
  /// The fallbacks matter only for the instant before the answer lands, and they are
  /// deliberately small: too low simply means `place_frame` does the clamping, which is what
  /// it is for. Too high would refuse sizes the frame can actually take.
  let limits = $state({ minWidth: 1, minHeight: 1 });
  invoke<{ min_width: number; min_height: number }>("frame_limits")
    .then((got) => (limits = { minWidth: got.min_width, minHeight: got.min_height }))
    .catch(() => {});

  async function grab(event: PointerEvent, edge: Edge) {
    event.preventDefault();
    event.stopPropagation();
    const rect = await invoke<{ x: number; y: number; width: number; height: number }>(
      "frame_rect",
    );
    dragging = { edge, startX: event.screenX, startY: event.screenY, rect };
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
  }

  function resize(event: PointerEvent) {
    if (!dragging) return;
    const { edge, startX, startY, rect } = dragging;
    const dx = event.screenX - startX;
    const dy = event.screenY - startY;

    // **The origin is clamped with the size, not after it.**
    //
    // `place_frame` holds the width and height to their minimum but is handed a rectangle
    // and never learns which edge moved, so it cannot know that pulling a *top* edge past
    // the floor should stop the top rather than carry the whole frame down. It did carry it:
    // the size pinned, the origin kept travelling, and the frame slid across the screen —
    // which looked like resizing and dragging at once from the top, and like a frame that
    // refused to get any narrower from the left.
    //
    // So the leading edges take the smaller of "how far the pointer went" and "how far there
    // is left to give", and the opposite edge stays exactly where it was put.
    let { x, y, width, height } = rect;
    if (edge.includes("e")) width = Math.max(limits.minWidth, width + dx);
    if (edge.includes("s")) height = Math.max(limits.minHeight, height + dy);
    if (edge.includes("w")) {
      const room = Math.min(dx, width - limits.minWidth);
      x += room;
      width -= room;
    }
    if (edge.includes("n")) {
      const room = Math.min(dy, height - limits.minHeight);
      y += room;
      height -= room;
    }

    invoke("place_frame", { rect: { x, y, width, height } });
  }

  function release(event: PointerEvent) {
    if (!dragging) return;
    (event.target as HTMLElement).releasePointerCapture(event.pointerId);
    dragging = null;
    report();
  }

  /// Tell the main window where the frame ended up, so it can be remembered.
  ///
  /// An event rather than a return value, for the same reason the recognized text travels
  /// as one: these are separate webviews, and the window that moved is not the window
  /// that owns the settings.
  ///
  /// **Once the geometry settles, not once per event.** Dragging a handle calls
  /// `place_frame` on every pointer-move and each of those moves the window, so every
  /// mousemove used to start a `frame_rect` round trip of its own. Neither command orders
  /// against the other, so a reply describing an earlier moment could arrive after the
  /// final one and be the size that got saved — which is the last few pixels of the drag,
  /// the whole of what was being adjusted. One report in flight is also no report to race.
  let settling: ReturnType<typeof setTimeout> | undefined;
  function report() {
    clearTimeout(settling);
    settling = setTimeout(() => {
      invoke("frame_rect")
        .then((rect) => emit("frame-moved", rect))
        .catch(() => {});
    }, 150);
  }

  const EDGES: Edge[] = ["n", "s", "e", "w", "ne", "nw", "se", "sw"];

  // Dragging the toolbar is handled by the OS, so no pointer handler here ever sees it.
  // The window's own move event is the only way to learn the frame was dragged.
  //
  // Resizing keeps its own subscription even though the platform no longer resizes this
  // window: `place_frame` is called on every pointer-move of a drag, and `onResized` is what
  // notices the last of them settled. Cheap, and one less way for a saved rect to be stale.
  getCurrentWindow().onMoved(() => report());
  getCurrentWindow().onResized(() => report());
</script>

<!-- The grip is the drag region, and it is the only one: `data-tauri-drag-region` is on that
     strip and on the mark inside it, nowhere else. The interior cannot carry it — that is
     what you look through, and a drag region there would swallow exactly the clicks
     click-through exists to pass on. -->
<div class="frame">
  <!-- The way to move the frame without a keybinding.
       It cannot be the interior: that is what you look *through*, and a drag region there
       would swallow exactly the clicks click-through exists to pass on. So it is a strip,
       and a strip inside the frame is paint — which is why `FRAME_GRIP` is part of what a
       capture skips. Twenty-eight pixels, which **must equal `FRAME_GRIP`**, holding a mark
       and nothing else. -->
  <div class="grip" data-tauri-drag-region>
    <!-- Three rules, shortest in the middle — a drag handle that happens to read as 三.
         Drawn rather than typed: a glyph would take the reader's Japanese font and its
         weight, and this has to sit at 28px beside a purple hairline without competing
         with the text underneath the frame. -->
    <span class="mark" data-tauri-drag-region aria-hidden="true">
      <i data-tauri-drag-region></i>
      <i data-tauri-drag-region></i>
      <i data-tauri-drag-region></i>
    </span>
  </div>

  <!-- Deliberately empty and deliberately unstyled. What shows here is the screen behind,
       and that is exactly what gets read. -->
  <div class="region">
    <!-- The one thing allowed to paint the interior, and only because the frame is guaranteed
         to be out of the shot when it does.
         `Settings::normalise` turns `hide_during_capture` on the moment this rises above zero
         — enforced in the store rather than here, so a hand-edited file cannot arrive with a
         wash and a visible frame. Rendered only when it is wanted, so the header's rule holds
         untouched for everyone who has not asked for this.
         **Inside the region, not over the frame.** Against `.frame` it covered the grip as
         well, which put the accent over the 三 mark and hid the frame's one hand-drag
         affordance behind a setting about something else. Here it covers what it is named
         for, which is also exactly the rectangle `interior_rect` will read.
         `pointer-events: none`: it must not take clicks the region is meant to pass through. -->
    {#if tint > 0}
      <div class="wash" style="--wash: {tint}" aria-hidden="true"></div>
    {/if}
  </div>

  {#each EDGES as edge}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="handle {edge}"
      onpointerdown={(event) => grab(event, edge)}
      onpointermove={resize}
      onpointerup={release}
      onpointercancel={release}
    ></div>
  {/each}
</div>

<style>
  /* The window is transparent; only the border and toolbar are allowed to paint. */
  :global(html),
  :global(body) {
    background: transparent !important;
    margin: 0;
    overflow: hidden;
  }

  /* `fixed` and `inset: 0`: the frame is the whole window, and the window is the frame.
     The column flex lays out the two things that are actually stacked — the grip and the
     region under it — and the eight handles sit over the result, positioned absolutely
     against this. */
  .frame {
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    border: 2px solid var(--accent);
    border-radius: 2px;
    box-sizing: border-box;
  }

  /* **Must equal `FRAME_GRIP`.** A capture skips exactly this much from the top, so a taller
     strip paints into the OCR and a shorter one throws away readable pixels. Neither fails
     loudly, which is why the number is stated in both places and said so in both.
     It is deliberately worth aiming at: this is the only way to move the frame by hand, and
     a hairline is something you miss. */
  .grip {
    flex: 0 0 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: move;
    user-select: none;
    /* The panel's own surface, sitting under the accent border like a strip of paper under
       an ink rule — the same relationship the reader's header has to the page. */
    background: var(--surface);
    border-bottom: 1px solid var(--border-strong);
    transition: background var(--anim) var(--ease);
  }
  .grip:hover {
    background: var(--surface-raised);
  }

  /* Three rules, shortest in the middle. It works as a drag handle first and reads as 三
     second, which is the right way round: nobody should have to recognise a character to
     know they can grab it. */
  .mark {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    width: 26px;
    max-width: 45%;
  }
  .mark i {
    height: 1px;
    background: var(--text-faint);
    border-radius: 1px;
    /* Full, then short, then full — the middle stroke of 三 is the shortest. */
    width: 100%;
    transition:
      background var(--anim) var(--ease),
      opacity var(--anim) var(--ease);
  }
  .mark i:nth-child(2) {
    width: 62%;
  }
  .grip:hover .mark i {
    background: var(--accent);
  }
  @media (prefers-reduced-motion: reduce) {
    .grip,
    .mark i {
      transition: none;
    }
  }

  /* The region itself: no background, no tint, nothing. See the note at the top.
     `relative` only so the wash has something to be absolute against — the region is what
     the wash is a wash *of*, and against the frame it painted the grip too. */
  .region {
    flex: 1;
    min-height: 0;
    position: relative;
  }

  /* The wash over the region. Only ever rendered when it is above zero, and only ever
     correct because the store guarantees the frame hides itself while it is. */
  .wash {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: var(--accent);
    opacity: var(--wash, 0);
  }

  /* **Must match `FRAME_HANDLE`**, which `over_chrome` uses to decide where the pointer is
     on solid chrome while click-through is on.

     **Eight, which is the main window's rim.** `SM_CXSIZEFRAME + SM_CXPADDEDBORDER` is 8px and
     `tao` keeps `WS_THICKFRAME`, so the reader resizes from an 8px border with its title bar
     clear below. The same gesture should want the same aim here.

     This was 14px for a while, widened when `resizable(false)` removed the native border on the
     reasoning that 8px alone is thin to hit. **It fixed the wrong half.** The rim is drawn over
     the grip — on a window with no native border there is nowhere else to put it — so what
     decided whether the strip could be grabbed was how deep the rim reached, not how wide it
     was. At 14px, with 20px corners on top, a 28px strip had about 14px of drag left in it and
     was inset 20px at each end. Widening made it worse.

     The corners are what fixed it: `--corner` is a width now, and every corner is `--thickness`
     tall, so nothing reaches below 8px and the grip keeps the rest right across its width.

     It costs no capture area — `interior_rect` insets the border and the grip and knows nothing
     about handles — and no height either. What it costs is how much of the region's rim resizes
     rather than passing clicks through, and at 8px that is less than it was.

     **The cap is not decoration.** `FRAME_MIN_WIDTH` is 32px, and at that width two 20px corners
     would tile the whole strip: every pixel of it a resize target, and the frame undraggable by
     hand at exactly the size a column of vertical text is read at, which is the size this all
     exists for. So each handle takes the smaller of its ideal and a share of the frame, and
     something in the middle always survives.

     The cap only ever makes a handle *smaller* than the constant it mirrors, so the disagreement
     with Rust runs one way: at the smallest sizes the window still counts the rim as solid while
     the CSS has let go of it. A click there does nothing instead of passing through — and at 32px
     wide there is nothing behind the rim to pass a click to anyway.

     **`FRAME_CORNER` is still mirrored, and now only in width.** A press near the bottom-left
     was once inside the drawn handle and outside everything `over_chrome` knew about: the window
     went transparent to the pointer, the click went to the application behind, and the corner
     never resized. `over_chrome` keeps treating the full 20px square as chrome while the CSS
     draws only `--thickness` of it — more chrome than is drawn, which is the harmless direction,
     and the top of the frame is grip there in any case. Changing the width means changing both. */
  .handle {
    position: absolute;
    /* **Must match `FRAME_HANDLE`, which is the main window's 8px.**

       Also the height of every corner, which is the half that actually mattered. The rim sits
       on top of the grip - there is nowhere else for it to go on a window with no native
       border - so what decides whether the strip can be grabbed is how far down the rim
       reaches, not how wide it is. At 14px with 20px corners the strip had about 14px of drag
       left in it, inset 20px at each end. At 8px with corners no deeper than the rim it keeps
       everything below 8px, right across its width.

       Chrome is unchanged at 28px: this costs no height and grows the drag area. */
    --thickness: min(8px, 25%);
    /* **Width only.** Corners size both axes at once, so they are worth aiming at, and 20px
       along the edge is what makes them easy to find. Their *height* is `--thickness`: a corner
       deeper than the rim is a corner that eats the drag strip, which is exactly what it was
       doing. **Must match `FRAME_CORNER`**, which is what tells click-through this is chrome. */
    --corner: min(20px, 30%);
    /* Where the drag strip ends: **must equal `FRAME_BORDER` + `FRAME_GRIP`.** The side handles
       start here rather than at the rim, so the strip is not bitten into at either end. */
    --below-grip: 30px;
  }
  .handle.n {
    top: 0;
    left: var(--corner);
    right: var(--corner);
    height: var(--thickness);
    cursor: ns-resize;
  }
  .handle.s {
    bottom: 0;
    left: var(--corner);
    right: var(--corner);
    height: var(--thickness);
    cursor: ns-resize;
  }
  .handle.e {
    right: 0;
    /* **Below the grip, not beside it**, which is the half the rim change left undone.

       Measured on the running reader: its window rect is 916px wide and only 902px of that is
       visible - `DWMWA_EXTENDED_FRAME_BOUNDS` reports a 7px invisible margin left, right and
       bottom, and none at the top. Windows hit-tests the resize border from the *window* rect,
       so on the sides that border is spent almost entirely on margin nobody can see, and every
       visible pixel of the title bar drags.

       The frame is transparent with no shadow and has no margin at all, so a side handle at
       strip height comes out of the strip itself - 8px gone from each end of the one thing you
       grab to move it. Starting them below the grip is what makes the visible strip behave like
       the visible title bar: all of it drags.

       Width is still resizable from the sides under the strip, from the two bottom corners, and
       from the top corners above it. **The alternative was giving this window an invisible
       margin of its own**, which is what the reader has - but on a click-through overlay that
       ring would swallow clicks the interior exists to pass on, and it would move every saved
       `frame_rect`. Not worth it for a resize you can already reach three other ways. */
    top: var(--below-grip);
    bottom: var(--thickness);
    width: var(--thickness);
    cursor: ew-resize;
  }
  .handle.w {
    left: 0;
    /* See `.handle.e`: below the grip, so the strip drags end to end. */
    top: var(--below-grip);
    bottom: var(--thickness);
    width: var(--thickness);
    cursor: ew-resize;
  }
  .handle.ne {
    top: 0;
    right: 0;
    width: var(--corner);
    height: var(--thickness);
    cursor: nesw-resize;
  }
  .handle.nw {
    top: 0;
    left: 0;
    width: var(--corner);
    height: var(--thickness);
    cursor: nwse-resize;
  }
  .handle.se {
    bottom: 0;
    right: 0;
    width: var(--corner);
    height: var(--thickness);
    cursor: nwse-resize;
  }
  .handle.sw {
    bottom: 0;
    left: 0;
    width: var(--corner);
    height: var(--thickness);
    cursor: nesw-resize;
  }
</style>
