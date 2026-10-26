<script lang="ts">
  import type { Snippet } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  // The window controls: keep-on-top, the settings menu, then minimise, maximise, close.
  //
  // **The pin is drawn as one of the set rather than as one of the app's toggles.** It is a
  // window control — "keep this above other applications" is about the window, not the page —
  // and a rounded 2rem toggle sitting against three square full-height buttons read as a
  // control that had wandered into the wrong group. It keeps the `.on` treatment the rest of
  // the app uses for a toggle that is switched on, so it still says which state it is in.
  //
  // It lives here rather than beside the other toggles in `App.svelte` because these four now
  // share their shape, and `traps.md` is explicit about what happens when two places draw the
  // same kind of thing: scoped styles do not travel, so the copy silently loses them.
  //
  // **The maximise button does not receive mouse events, and that is on purpose.** A child
  // window sits over it answering `WM_NCHITTEST` with `HTMAXBUTTON`, which is the only way
  // Windows 11 will show the snap-layout flyout for a custom title bar (`titlebar.rs` has the
  // whole argument). The price is that the pointer belongs to that window while it is there:
  // no `:hover`, no `click`. So the overlay reports what the pointer did and the three
  // `titlebar://` events below stand in for the pseudo-classes.
  //
  // Everything here still works without the overlay — off Windows, or if creating it failed.
  // The `onclick` handlers are live, and a keyboard activation goes through them too, since a
  // focused button is not a hovered one and Windows sends nothing for Enter.
  //
  // **The settings button must stay to the left of maximise, and that is a Win32 constraint
  // rather than taste.** `titlebar.rs` measures the hit-test overlay leftward from the window's
  // right edge with `CAPTION_BUTTONS_RIGHT_OF_MAX = 1` — only what is to the *right* of maximise
  // enters the arithmetic. Anything added on that side shifts the overlay onto the wrong button and
  // the Windows 11 snap flyout silently appears over close, with nothing failing to say so.
  //
  // Between the pin and minimise satisfies that, and so did the leftmost slot it sat in first.
  // Anywhere among the three OS buttons does not.

  /// How wide one button is, in CSS pixels — **from Rust**, via `App.svelte`.
  ///
  /// `titlebar.rs` positions the hit-test overlay from `CAPTION_BUTTON_WIDTH`, so a copy kept
  /// here would drift out of step with it and the flyout would end up over the close button.
  /// A prop rather than a second `titlebar_metrics` call, so one fetch answers for the bar's
  /// height and its buttons alike.
  let {
    width = 46,
    pin = false,
    pinned = false,
    onpin,
    menu,
  }: {
    width?: number;
    /// Whether to offer the pin at all — it is hidden on the welcome screen, where there is
    /// nothing to keep on top of anything yet. The other three are never hidden: a window you
    /// cannot close is not a window.
    pin?: boolean;
    pinned?: boolean;
    onpin?: () => void;
    /// The app's own menu — the gear and everything it drops — rendered into the title-bar row.
    ///
    /// **A snippet rather than props, and that is what keeps this file about window chrome.** The
    /// menu needs seven two-way bindings (the theme, three sizes, three reader switches) plus the
    /// three actions it offers; threading those through here would make the title bar know about
    /// settings it has no other business with. `AppMenu.svelte` owns all of it, the caller owns
    /// the state, and this only supplies a place in the row.
    ///
    /// Absent on the welcome screen for the pin's reason: there is nothing to configure until
    /// there is a dictionary to configure it against.
    menu?: Snippet;
  } = $props();

  let maximized = $state(false);
  let hovering = $state(false);
  let pressing = $state(false);

  async function readState() {
    try {
      maximized = await invoke<boolean>("window_maximized");
    } catch {
      /* the window is going away; its glyph is the least of it */
    }
  }
  readState();

  // Snapping, Win+arrow and a double-click on the drag region all maximise without anything
  // here being clicked, so the state has to be read back rather than remembered.
  getCurrentWindow().onResized(() => readState());

  async function toggle() {
    pressing = false;
    try {
      maximized = await invoke<boolean>("toggle_maximize");
    } catch {
      /* nothing to show for it */
    }
  }

  // The four names `titlebar.rs` emits. The component mounts once and lives as long as the
  // window, so these are never unsubscribed — the same as every other `listen` in App.svelte.
  listen("titlebar://max-enter", () => (hovering = true));
  listen("titlebar://max-leave", () => {
    hovering = false;
    pressing = false;
  });
  listen("titlebar://max-down", () => (pressing = true));
  listen("titlebar://max-click", () => toggle());
</script>

<!-- Not `data-tauri-drag-region`, and none of the children are either: Tauri's drag region
     matches the element the pointer is actually on, so a button inside the strip is safe from
     it as long as it does not carry the attribute itself. -->
<div class="caption" style="--caption-w: {width}px">
  {#if pin}
    <!-- Ours, and the only one of the four that has an on state — hence `.on` and a tooltip,
         where the OS's three need neither. -->
    <button
      class="cap pin tip-right"
      class:on={pinned}
      onclick={() => onpin?.()}
      aria-pressed={pinned}
      aria-label="Keep this window above others"
      data-tip={pinned
        ? "On top — this window stays above other applications"
        : "Keep this window above other applications"}
    >
      <!-- One pin, turned. The off state was a second hand-drawn path and came out reading
           as a lightning bolt: two drawings of "a pin" is two chances to draw something that
           is not one. Rotating the shape that already works cannot produce a different
           object, and a pin lying at an angle is the ordinary way of saying it is not pushed
           in.

           `transform` on a group rather than CSS `rotate`, so it turns about the viewBox's
           own centre whatever the button's box is doing. -->
      <svg class="glyph" viewBox="0 0 24 24" aria-hidden="true">
        <g transform={pinned ? "" : "rotate(45 12 12)"}>
          <path
            d="M12 15.5V21M8.5 4h7l-.7 5.2 2.7 2.6a1 1 0 0 1-.7 1.7H6.2a1 1 0 0 1-.7-1.7l2.7-2.6L7.5 4h1Z"
            fill="none" stroke="currentColor" stroke-width="1.8"
            stroke-linecap="round" stroke-linejoin="round"
          />
        </g>
      </svg>
    </button>
  {/if}

  {@render menu?.()}

  <button
    class="cap tip-right"
    onclick={() => invoke("minimize_window").catch(() => {})}
    aria-label="Minimise"
    data-tip="Minimise"
  >
    <!-- One rule, and the same 10px span as the square beside it, so the three read as a
         set rather than as three drawings that happen to be adjacent. -->
    <svg viewBox="0 0 10 10" aria-hidden="true">
      <path d="M0 5h10" />
    </svg>
  </button>

  <!-- `class:hover` rather than a `:hover` rule — see the note at the top: the pointer is
       over a different window when it is over this button. -->
  <button
    class="cap tip-right"
    class:hover={hovering}
    class:press={pressing}
    onclick={toggle}
    aria-label={maximized ? "Restore" : "Maximise"}
    data-tip={maximized ? "Restore" : "Maximise"}
  >
    {#if maximized}
      <!-- Two squares, offset — the shape Windows uses for restore. Drawn as one path pair
           rather than a single glyph so the front square can sit opaque over the back one. -->
      <svg viewBox="0 0 10 10" aria-hidden="true">
        <path d="M2.5 0.5h7v7" />
        <path d="M0.5 2.5h7v7h-7z" />
      </svg>
    {:else}
      <svg viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0.5 0.5h9v9h-9z" />
      </svg>
    {/if}
  </button>

  <button
    class="cap close tip-right"
    onclick={() => invoke("close_window").catch(() => {})}
    aria-label="Close"
    data-tip="Close"
  >
    <svg viewBox="0 0 10 10" aria-hidden="true">
      <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />
    </svg>
  </button>
</div>

<style>
  .caption {
    display: flex;
    /* No gap. Caption buttons abut — a gap between them would leave strips of bar that look
       clickable and are not, and the close button has to reach the window's corner or the
       Fitts's-law throw everyone makes at it lands on nothing. */
    gap: 0;
    /* Against the strip's own padding: these go all the way to the edges. */
    margin: 0;
    align-self: stretch;
  }

  /* The app's own "this is on" treatment, minus the border and radius the other toggles
     carry — a rounded outline inside a square full-height button would be a button drawn
     twice. The accent alone is enough: it is the only colour in the bar. */
  .cap.pin.on,
  .cap.pin.on:hover {
    background: var(--accent-edge);
  }

  /* The one red affordance in the app that is not an error or a discard. It is red because
     every other window's is, and this is the one control where matching the platform beats
     matching the palette. */
  .cap.close:hover,
  .cap.close:active {
    background: var(--red-strong);
    color: #fff;
  }

</style>
