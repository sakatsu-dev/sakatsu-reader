<script lang="ts">
  import { tick } from "svelte";
  import { OCR_OPTIONS, type OcrChoice } from "../model";

  /// The four engines, from `model.ts` — the settings page and the app menu offer the same list.
  const OPTIONS = OCR_OPTIONS;

  let {
    value = $bindable(),
    compact = false,
    disabled = false,
    label = "OCR model",
  }: {
    value: OcrChoice;
    /** Match the large action-row controls rather than a settings field. */
    compact?: boolean;
    disabled?: boolean;
    label?: string;
  } = $props();

  let open = $state(false);
  let active = $state(0);
  let host = $state<HTMLDivElement | null>(null);
  let trigger = $state<HTMLButtonElement | null>(null);
  let menuStyle = $state("visibility: hidden");
  let optionButtons: HTMLButtonElement[] = [];

  const selected = $derived(OPTIONS.find((option) => option.value === value) ?? OPTIONS[0]);

  // A capture can begin from the global shortcut while this has focus. Do not leave an open
  // panel attached to a trigger that became disabled underneath it.
  $effect(() => {
    if (disabled) open = false;
  });

  function selectedIndex() {
    const index = OPTIONS.findIndex((option) => option.value === value);
    return index < 0 ? 0 : index;
  }

  async function openList(at = selectedIndex()) {
    if (disabled) return;
    active = at;
    const anchor = trigger?.getBoundingClientRect();
    menuStyle = anchor
      ? `visibility: hidden; width: ${anchor.width}px`
      : "visibility: hidden";
    open = true;
    await tick();
    positionMenu();
    optionButtons[active]?.focus();
  }

  /// The desktop WebView can make an absolutely positioned component child use `main` as its
  /// containing block when the component root is also a flex item. Measure the actual trigger and
  /// pin the popup to that viewport rectangle instead, so no ancestor can stretch or relocate it.
  function positionMenu() {
    const anchor = trigger?.getBoundingClientRect();
    if (!anchor) return;

    const edge = 4;
    // Use the trigger's left and width as one pair. Measuring the menu's intrinsic width and
    // right-aligning it made fractional font metrics round the panel one pixel past the left edge.
    const width = anchor.width;
    const wantedLeft = anchor.left;
    const maxLeft = Math.max(edge, window.innerWidth - edge - width);
    const left = Math.min(Math.max(edge, wantedLeft), maxLeft);
    menuStyle = `top: ${anchor.bottom - 1}px; left: ${left}px; width: ${width}px; visibility: visible`;
  }

  async function closeList(returnFocus = false) {
    open = false;
    if (!returnFocus) return;
    await tick();
    trigger?.focus();
  }

  function choose(next: OcrChoice) {
    value = next;
    void closeList(true);
  }

  function focusOption(index: number) {
    active = (index + OPTIONS.length) % OPTIONS.length;
    optionButtons[active]?.focus();
  }

  function onTriggerKey(event: KeyboardEvent) {
    let at: number | undefined;
    if (event.key === "ArrowDown") at = selectedIndex();
    if (event.key === "ArrowUp") at = selectedIndex();
    if (event.key === "Home") at = 0;
    if (event.key === "End") at = OPTIONS.length - 1;
    if (at === undefined) return;
    event.preventDefault();
    void openList(at);
  }

  function onOptionKey(event: KeyboardEvent, index: number, option: OcrChoice) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      focusOption(index + 1);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      focusOption(index - 1);
      return;
    }
    if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      focusOption(event.key === "Home" ? 0 : OPTIONS.length - 1);
      return;
    }
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      choose(option);
      return;
    }

    // A listbox promises typeahead. Start after the current row so repeated letters still
    // cycle when two future options share an initial rather than always choosing the first.
    if (event.key.length === 1 && !event.ctrlKey && !event.altKey && !event.metaKey) {
      const needle = event.key.toLocaleLowerCase();
      for (let step = 1; step <= OPTIONS.length; step += 1) {
        const candidate = (index + step) % OPTIONS.length;
        if (OPTIONS[candidate].label.toLocaleLowerCase().startsWith(needle)) {
          event.preventDefault();
          focusOption(candidate);
          return;
        }
      }
    }
  }

  /// Close only after focus leaves the trigger and the list together. Closing on the trigger's
  /// blur would remove an option before the click that moved focus to it could land.
  function onFocusOut(event: FocusEvent) {
    const next = event.relatedTarget;
    if (next instanceof Node && host?.contains(next)) return;
    open = false;
  }

  function onWindowPointerDown(event: PointerEvent) {
    if (!open) return;
    const target = event.target;
    if (target instanceof Node && host?.contains(target)) return;
    open = false;
  }

  function onWindowKey(event: KeyboardEvent) {
    if (!open || event.key !== "Escape") return;
    event.preventDefault();
    void closeList(true);
  }

  function onWindowResize() {
    if (open) positionMenu();
  }

  function onWindowWheel() {
    // A fixed popup would otherwise stay put while the settings scroller moves its trigger.
    if (open) open = false;
  }
</script>

<svelte:window
  onkeydown={onWindowKey}
  onpointerdown={onWindowPointerDown}
  onresize={onWindowResize}
  onwheel={onWindowWheel}
/>

<!-- A native select's closed field accepted the theme, but Windows drew its open menu. This
     listbox keeps both halves in the webview, and therefore in the same surface vocabulary. -->
<!-- `data-keeps-focus`: this closes on `focusout` and focuses an option as it opens, so the
     app-wide "release focus after a click" rule would shut it in the frame it appeared. See
     `onWindowClick` in `App.svelte`. -->
<div
  class="ocr-model-host"
  class:open
  bind:this={host}
  onfocusout={onFocusOut}
  data-keeps-focus
>
  <button
    type="button"
    class="ocr-model-trigger"
    class:compact
    class:open
    bind:this={trigger}
    {disabled}
    aria-label={label}
    aria-haspopup="listbox"
    aria-expanded={open}
    onclick={() => (open ? void closeList() : void openList())}
    onkeydown={onTriggerKey}
    {...open ? {} : { "data-tip": label }}
  >
    <span>{selected.label}</span>
    <span class="ocr-model-chevron" aria-hidden="true"></span>
  </button>

  {#if open}
    <div
      class="ocr-model-menu"
      role="listbox"
      aria-label={label}
      style={menuStyle}
    >
      {#each OPTIONS as option, index}
        <button
          type="button"
          class="ocr-model-option"
          class:selected={option.value === value}
          role="option"
          aria-selected={option.value === value}
          tabindex={index === active ? 0 : -1}
          bind:this={optionButtons[index]}
          onclick={() => choose(option.value)}
          onkeydown={(event) => onOptionKey(event, index, option.value)}
        >
          <span class="ocr-model-mark" aria-hidden="true"></span>
          {option.label}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .ocr-model-host {
    position: relative;
    display: inline-flex;
    flex: 0 0 auto;
  }
  /* Lift the whole disclosure while it is open. Without this, a later positioned sibling can
     paint over the list even though the list itself carries a z-index. */
  .ocr-model-host.open {
    z-index: 5;
  }

  .ocr-model-trigger {
    position: relative;
    z-index: 2;
    /* Wide enough for manga-ocr plus the chevron without either touching an edge. The menu
       copies this measured width, so this one floor protects both halves of the control. */
    min-width: 7.75rem;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--gap-sm);
    padding: 0.15rem 0.6rem;
    background: var(--surface-raised);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 0.8rem;
    transition:
      background var(--anim) var(--ease),
      border-color var(--anim) var(--ease);
  }
  /* A selector is a field even though its trigger is a button: it sinks into the page instead of
     lifting like an action. Open uses the panel surface so both halves read as one object. */
  .ocr-model-trigger:hover:not(:disabled),
  .ocr-model-trigger.open {
    background: var(--surface);
  }
  .ocr-model-trigger.open {
    border-color: var(--border-strong);
    /* The panel continues this surface. Painting the shared bottom edge made the two halves
       look stacked even when their coordinates touched exactly. */
    border-bottom-color: transparent;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  }
  .ocr-model-trigger:disabled {
    opacity: 0.55;
  }
  /* Same height and corner as Analyze / Clear. */
  .ocr-model-trigger.compact {
    border-radius: var(--radius);
    padding: 0.34rem 0.65rem;
    font-size: 0.85rem;
  }
  .ocr-model-trigger.compact.open {
    border-radius: var(--radius) var(--radius) 0 0;
  }

  .ocr-model-chevron {
    flex: 0 0 auto;
    width: 0.38rem;
    height: 0.38rem;
    margin: -0.18rem 0.1rem 0 0;
    border-right: 1px solid currentColor;
    border-bottom: 1px solid currentColor;
    rotate: 45deg;
    color: var(--text-dim);
    transition: rotate var(--anim) var(--ease);
  }
  .ocr-model-trigger.open .ocr-model-chevron {
    margin-top: 0.18rem;
    rotate: 225deg;
  }

  .ocr-model-menu {
    position: fixed;
    z-index: 100;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    /* No top inset: the first row begins where the trigger ends. Bottom breathing room remains
       inside the rounded end of the disclosure. */
    padding: 0 0 var(--gap-xs);
    background: var(--surface);
    border: 1px solid var(--border-strong);
    border-top: 0;
    border-radius: 0 0 var(--radius) var(--radius);
    box-shadow: var(--shadow-popup);
  }

  .ocr-model-option {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    padding: 0.32rem 0.65rem;
    background: none;
    border: 0;
    border-radius: 0;
    color: var(--text-dim);
    text-align: left;
    white-space: nowrap;
    transition:
      background var(--anim) var(--ease),
      color var(--anim) var(--ease);
  }
  .ocr-model-option:hover,
  .ocr-model-option:focus-visible,
  .ocr-model-option.selected {
    background: var(--accent-wash);
    color: var(--text);
  }
  .ocr-model-option:focus-visible {
    /* The row fill is the listbox's focus indicator; an outer ring would be clipped at the sides. */
    outline: 0;
  }
  .ocr-model-mark {
    flex: 0 0 auto;
    width: 0.35rem;
    height: 0.35rem;
    border-radius: 50%;
    background: transparent;
  }
  .ocr-model-option.selected .ocr-model-mark {
    background: var(--accent);
  }
</style>
