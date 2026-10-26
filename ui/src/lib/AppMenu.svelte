<script lang="ts">
  // The app's own menu: how things look, then what the app itself can do.
  //
  // It was two buttons for a moment — a gear for behaviour and an Aa for appearance — and that is
  // a distinction nobody asked for, since both are "the app's menu". One gear, one menu.
  //
  // Settings is a page, so it replaces the reader — and every appearance change was therefore
  // judged blind, then confirmed by closing the page, looking, and going back in. This is that
  // round trip removed: the panel opens over whatever is on screen, the content stays visible
  // behind it, and a dragged slider moves the real thing rather than a preview of it.
  //
  // Kindle's "Aa" is the same idea and states the reason plainly — you gauge a text setting by
  // watching the text, not by remembering what it looked like a screen ago.
  //
  // **The seven section shortcuts are gone with the old gear menu.** Seven of them under seven
  // appearance rows and three actions is an eighteen-row menu, in a menu whose whole point is
  // being quicker than the settings page. `All settings…` still opens at the remembered section,
  // so `settings_section` keeps its only reader.
  import PalettePicker from "./PalettePicker.svelte";
  import {
    ANALYZER_OPTIONS,
    OCR_OPTIONS,
    PASSAGE_ALIGN_OPTIONS,
    type AnalyzerChoice,
    type NamedTheme,
    type OcrChoice,
    type PassageAlign,
    type ThemeSettings,
    type View,
  } from "../model";

  let {
    view,
    theme,
    themePresets,
    fontSize = $bindable(),
    readerScale = $bindable(),
    passageAlign = $bindable(),
    panelScale = $bindable(),
    ocr = $bindable(),
    analyzer = $bindable(),
    keepLineBreaks = $bindable(),
    decorCloudOn = $bindable(),
    decorSakuraOn = $bindable(),
    copyCaptureToClipboard = $bindable(),
    onpalette,
    settingsOn,
    onsettings,
    onhelp,
    onrestart,
  }: {
    /// Which page is behind the panel. Decides which rows are worth offering — see `ROWS`.
    view: View;
    theme: ThemeSettings;
    /// Picking a palette is `App.svelte`'s to do — it owns the decoration a palette can carry as
    /// well as the colours, and this used to apply only half of that.
    onpalette: (name: string) => void;
    themePresets: NamedTheme[];
    fontSize: number;
    readerScale: number;
    passageAlign: PassageAlign;
    panelScale: number;
    ocr: OcrChoice;
    analyzer: AnalyzerChoice;
    keepLineBreaks: boolean;
    /** The two watermarks, on or off. Their strengths and colours stay on the settings page. */
    decorCloudOn: boolean;
    decorSakuraOn: boolean;
    copyCaptureToClipboard: boolean;
    /// Whether the settings page is the one on screen, so the gear can say so.
    settingsOn: boolean;
    /// The way through to everything this menu deliberately does not carry. Opens where you left
    /// off rather than at a named section — the only reader of `settings_section`.
    onsettings: () => void;
    onhelp: () => void;
    onrestart: () => void;
  } = $props();

  /**
   * **What each page can actually show you**, which is the whole rule this panel follows.
   *
   * A control for something you cannot see is the problem the panel exists to remove, so it would
   * be absurd to reintroduce it here. The three groups are measured, not guessed:
   *
   * - `palette` and `interface` reach everything. The theme tokens are written to the root, and
   *   `fontSize` *is* the root's `font-size`, so every `rem` in the app follows it.
   * - `panel` is `--panel-scale`, read by `WordBody`, `Character`, `Grammar`, `ExampleText`,
   *   `SelectedWordHeader`, `PronounceButton` and `Reference` — the definition surfaces, which the
   *   reader page shows beside the passage and Reference and Glossary show on their own.
   * - `reader` reaches `Reader.svelte` and nothing else, and so does `align`: where each line of
   *   the passage starts. It sits beside the passage's size because it is judged the same way, by
   *   watching the text while it changes, and it is appearance rather than pipeline — it changes
   *   how the passage is drawn, never how it was read.
   * - `ocr`, `analyzer` and `breaks` are the reader's *pipeline* rather than its appearance: what
   *   reads a capture, what breaks the result into words, and whether the lines it arrived on
   *   survive. They are ruled off from the sizes above because they answer a different question —
   *   how the passage was made, not how it is drawn — and they are the reader's alone, since no
   *   other page ever sees a capture.
   * - `clipboard` joins that group without being a pipeline stage: it is about a passage that
   *   has already been read — where it is copied to — so it is the reader's alone for the same
   *   reason. The guess mark and the parts row were here too, and moved to General settings
   *   (2026-10-24): each is set once, not adjusted while reading.
   *
   * - `decor` is the clouds and the sakura, on or off. Both are on every page — the clouds along
   *   the foot of `main`, the sakura down the rail — so every page offers them. Only the switches:
   *   strength, colour and spacing are tuning, and they stay on the settings page.
   *
   * Furigana reaches the reader too and is deliberately absent: the analysis page carries its own
   * toggle beside the text it acts on, so a second one here would be the same switch in two places.
   *
   * A table rather than a pile of conditionals, and `Record<View, …>` rather than a partial map, so
   * a new page has to say what it can show instead of silently inheriting the reader's rows.
   */
  const ROWS = {
    read: ["palette", "interface", "reader", "align", "panel", "decor", "ocr", "analyzer", "breaks", "clipboard"],
    reference: ["palette", "interface", "panel", "decor"],
    glossary: ["palette", "interface", "panel", "decor"],
    settings: ["palette", "interface", "decor"],
    about: ["palette", "interface", "decor"],
    help: ["palette", "interface", "decor"],
  } as const satisfies Record<View, readonly string[]>;

  const shown = $derived<readonly string[]>(ROWS[view]);
  /// The pipeline group — the three rows below the rule. Named so the rule and its rows cannot
  /// disagree about whether the group exists.
  const PIPELINE = ["ocr", "analyzer", "breaks", "clipboard"];
  const pipeline = $derived(PIPELINE.some((row) => shown.includes(row)));
  const has = (row: string) => shown.includes(row);



  let open = $state(false);
  /// The Interface size under the pointer while the slider is dragged, applied on release.
  let fontSizeDraft = $state<number | null>(null);
  let host = $state<HTMLElement | null>(null);
  let button = $state<HTMLButtonElement | null>(null);

  /// Dismissed by looking away, the same way the gear's menu is. `data-keeps-focus` on the host is
  /// the convention `OcrSelect` set: focus inside here is not focus lost.
  function onFocusOut(event: FocusEvent) {
    const next = event.relatedTarget;
    if (next instanceof Node && host?.contains(next)) return;
    open = false;
  }

  /// Escape closes it, from wherever focus is inside. On the window for the reason
  /// `Caption.svelte` gives: a `keydown` on a plain wrapper wants an ARIA role it has no honest
  /// one to give, and "Escape dismisses what is open" is a window-level convention. The guard is
  /// the first line, so it costs nothing while shut — and this runs before the settings page's own
  /// Escape, which is why that one checks `defaultPrevented`.
  /// Close, then act. Every row does this, so none of them can leave the menu standing over a
  /// page that has just been replaced.
  function choose(act: () => void) {
    open = false;
    act();
  }

  function onKey(event: KeyboardEvent) {
    if (!open || event.key !== "Escape") return;
    event.preventDefault();
    open = false;
    // Back to the button, or focus is left on an element that no longer exists.
    button?.focus();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="host" bind:this={host} onfocusout={onFocusOut} data-keeps-focus>
  <!-- The tooltip goes while the menu is open, because `.tip-right` opens downward from here,
       which is exactly where the menu's first row lands. Spread rather than `data-tip={…}`: an
       empty string still draws the bubble, so the attribute has to be absent, not blank. -->
  <button
    class="cap gear tip-right"
    class:on={settingsOn}
    class:open
    bind:this={button}
    onclick={() => (open = !open)}
    aria-expanded={open}
    aria-label="Settings"
    {...open ? {} : { "data-tip": "Settings" }}
  >
    <!-- The gear, not an "Aa": this menu holds Help and Restart as well as appearance, and nobody
         looks for Restart under an Aa. Inline rather than a font glyph for the reason it always
         was — two windows share this bundle and a missing face would leave a blank button.

         The body's subpath starts a unit left of where it looks like it should. It was drawn a
         unit right of its own hub — the hole centred on 12, the teeth on 13 — so the icon sat
         off-centre in a button that was centred perfectly. -->
    <svg class="glyph" viewBox="0 0 24 24" aria-hidden="true">
      <path
        fill="currentColor"
        d="M12 15.5A3.5 3.5 0 1 1 12 8.5a3.5 3.5 0 0 1 0 7Zm6.4-2.6.1-.9-.1-.9 1.9-1.5-1.9-3.2-2.3.9a7 7 0 0 0-1.6-.9L14.1 4H9.9l-.4 2.4a7 7 0 0 0-1.6.9l-2.3-.9-1.9 3.2 1.9 1.5-.1.9.1.9-1.9 1.5 1.9 3.2 2.3-.9c.5.4 1 .7 1.6.9l.4 2.4h4.2l.4-2.4c.6-.2 1.1-.5 1.6-.9l2.3.9 1.9-3.2Z"
      />
    </svg>
  </button>

  {#if open}
    <!-- A disclosure, not a `role="menu"` widget — `Caption.svelte`'s comment gives the reasoning
         and this is the same shape: controls after a button with `aria-expanded`, which promises
         Tab and delivers Tab, with Escape on top.

         `{#if}` rather than hidden-with-CSS, so the controls are out of the tab order when shut. -->
    <div class="menu">
      {#if has("palette")}
        <label class="row">
          <span>Palette</span>
          <PalettePicker {theme} {themePresets} onpick={onpalette} />
        </label>
      {/if}

      {#if has("interface")}
        <label class="row">
          <span>Interface <em>{(fontSizeDraft ?? fontSize).toFixed(0)}px</em></span>
          <!-- Applied on release, as on the settings page: the slider is sized in `rem`, so
               applying each step mid-drag moved it out from under the pointer. -->
          <input
            type="range"
            min="12"
            max="24"
            step="1"
            value={fontSize}
            oninput={(event) => (fontSizeDraft = event.currentTarget.valueAsNumber)}
            onchange={(event) => {
              fontSize = event.currentTarget.valueAsNumber;
              fontSizeDraft = null;
            }}
          />
        </label>
      {/if}

      {#if has("reader")}
        <label class="row">
          <span>Analyzed text <em>{Math.round(readerScale * 100)}%</em></span>
          <input type="range" min="0.6" max="2.5" step="0.01" bind:value={readerScale} />
        </label>
      {/if}

      {#if has("align")}
        <label class="row">
          <span>Align text</span>
          <select
            value={passageAlign}
            onchange={(event) => (passageAlign = event.currentTarget.value as PassageAlign)}
          >
            {#each PASSAGE_ALIGN_OPTIONS as option}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>
        </label>
      {/if}

      {#if has("panel")}
        <label class="row">
          <span>Definitions <em>{Math.round(panelScale * 100)}%</em></span>
          <input type="range" min="0.6" max="2" step="0.01" bind:value={panelScale} />
        </label>
      {/if}

      <!-- **One row, two switches**, so turning the watermarks off is a press from any page rather
           than a trip to Appearance. A `div`, not a `label`: a label may hold one control. Each
           switch is named by its own label, "Clouds" and "Sakura". -->
      {#if has("decor")}
        <div class="row">
          <span>Decoration</span>
          <div class="pair">
            <label>Clouds <input type="checkbox" bind:checked={decorCloudOn} /></label>
            <label>Sakura <input type="checkbox" bind:checked={decorSakuraOn} /></label>
          </div>
        </div>
      {/if}

      <!-- How a passage is made, under how it looks. The rule only exists when there is something
           on both sides of it: on Reference and Glossary the menu stops at the sizes, and a rule
           there would be ruling off nothing. -->
      {#if pipeline}
        <hr />
      {/if}

      {#if has("ocr")}
        <label class="row">
          <span>Recognition</span>
          <select value={ocr} onchange={(event) => (ocr = event.currentTarget.value as OcrChoice)}>
            {#each OCR_OPTIONS as option}<option value={option.value}>{option.label}</option>{/each}
          </select>
        </label>
      {/if}

      {#if has("analyzer")}
        <label class="row">
          <span>Analyzer</span>
          <select
            value={analyzer}
            onchange={(event) => (analyzer = event.currentTarget.value as AnalyzerChoice)}
          >
            {#each ANALYZER_OPTIONS as option}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>
        </label>
      {/if}

      {#if has("breaks")}
        <label class="row">
          <span>Keep line breaks</span>
          <input type="checkbox" bind:checked={keepLineBreaks} />
        </label>
      {/if}

      {#if has("clipboard")}
        <label class="row">
          <span>Copy captures to clipboard</span>
          <input type="checkbox" bind:checked={copyCaptureToClipboard} />
        </label>
      {/if}

      <hr />
      <!-- What the gear menu was for, under what it is used for. Each closes the menu first, or it
           outlives the page it was opened over. Help is ruled off from the destinations above and
           Restart from Help: leaving the app belongs to neither group. -->
      <button class="item" onclick={() => choose(onsettings)}>All settings…</button>
      <hr />
      <button class="item" onclick={() => choose(onhelp)}>Help</button>
      <hr />
      <button class="item" onclick={() => choose(onrestart)}>Restart</button>
    </div>
  {/if}
</div>

<style>
  /* The wrapper the menu is positioned against, and one focus region so `focusout` can tell
     leaving from moving about inside. **`position: relative` and no `z-index`**: a positioned
     element with one is a stacking context, and the menu inside it could then never get past
     anything outside it however high it went. `.titlebar` in `App.svelte` is the context that has
     been lifted; this only supplies the coordinates. */
  .host {
    position: relative;
    align-self: stretch;
    display: flex;
  }

  /* **An open menu is one surface with its trigger.** The gear takes the menu's fill and grows the
     menu's side borders, so the two read as a single shape rather than a button with a card under
     it. `z-index: 2` puts it over the menu's own top edge, which `.menu::before` then erases. */
  .cap.gear.open {
    position: relative;
    z-index: 2;
    color: var(--text);
    background: var(--surface);
    box-shadow:
      inset 1px 0 var(--border-strong),
      inset -1px 0 var(--border-strong);
  }
  .cap.gear.on {
    color: var(--accent);
    background: var(--accent-wash);
  }
  /* Keep the accent on a current Settings page, but not the wash that would split the trigger and
     the menu into different fills. */
  .cap.gear.on.open {
    color: var(--accent);
    background: var(--surface);
  }

  /* Hangs off the bar's bottom edge, its **right** corner under the button's, so it opens leftward
     across the drag region rather than clipping at the app's edge.

     **Wider than the gear menu's `8rem` floor, and that is the one thing this borrows differently.**
     These are labelled controls, not a list of destinations: a slider needs room beside its name,
     not under it. Everything else — the fill, the squared top corners, the join below — is the
     menu this button has always dropped. `max-content` with that floor, so a longer label can
     still grow it — the width that varied was the palette control's, and that is pinned below
     rather than papered over here. */
  .menu {
    position: absolute;
    top: 100%;
    right: 0;
    z-index: 1;
    width: max-content;
    min-width: 17rem;
    display: flex;
    flex-direction: column;
    padding: var(--gap-xs) 0;
    background: var(--surface);
    border: 1px solid var(--border-strong);
    border-radius: 0 0 var(--radius) var(--radius);
    box-shadow: var(--shadow-popup);
    /* **The menu's type, set once here.** A destination and a setting are the same rank of thing
       and must not be two sizes; the title bar around this is smaller than the app's body text, so
       the size is stated rather than inherited from the bar. Everything below takes it, including
       the `<select>`, which needs `font: inherit` to follow — `theme.css` gives it that. */
    font-size: 0.8rem;
  }
  /* Erase the menu's top edge only where it meets the open gear. The rest of that edge stays, so
     the menu is still outlined either side of the button it hangs from. `--caption-w` because that
     is the button's width, and it comes from Rust — see `theme.css`. */
  /* **`right: 0`, and the offset that was there is worth the sentence.**
     `right` on an absolutely positioned box is measured from the containing block's *padding* box —
     inside `.menu`'s own 1px border, not outside it. With `right: 1px` the strip landed one pixel
     left of the gear's interior: it left the rightmost border pixel un-erased, which showed as a
     tick poking out of the menu's right edge at the join, and it erased the corner pixel on the
     left, which broke the vertical line where the gear's border meets the menu's. Both are one
     pixel and both are visible, because a straight line with a notch in it is exactly the kind of
     thing an eye finds. `0` here puts the strip on the gear's interior exactly. */
  .menu::before {
    content: "";
    position: absolute;
    top: -1px;
    right: 0;
    width: calc(var(--caption-w) - 2px);
    height: 1px;
    background: var(--surface);
  }

  /* A destination or an action: the gear menu's row, unchanged. */
  .item {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    padding: 0.32rem var(--gap);
    background: none;
    border: 0;
    text-align: left;
    color: var(--text-dim);
    cursor: pointer;
    /* Inherited from `.menu`, so a row and a destination cannot drift apart. */
    font: inherit;
    /* The bar sets `user-select: none` and `cursor: default` for the drag region; a menu row is an
       ordinary control and wants neither. */
    transition:
      background var(--anim) var(--ease),
      color var(--anim) var(--ease);
  }
  .item:hover,
  .item:focus-visible {
    background: var(--accent-wash);
    color: var(--text);
  }

  /* A setting rather than a destination — a label and its control, on the same padding the rows
     above use so the two kinds line up down one edge. */
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--gap);
    align-items: center;
    padding: 0.2rem var(--gap);
    color: var(--text-dim);
  }
  /* The value beside its name, the way the settings page writes it — same words, same shape, so
     the two surfaces cannot look like different features. */
  /* **A slider gets the whole row, because precision here is pixels.**
     Every dial declares the same step — 1% — but a slider can only resolve as finely as it is
     wide, and beside its label each had 129px. `Analyzed text` spans 0.6–2.5, which is 190 steps
     in those pixels: 1.47 per pixel, so dragging it moved 1% or 2% depending on where the pointer
     landed, while `Definitions` at 140 steps mostly moved 1%. Same step, different resolution —
     and the settings page never showed it because its sliders are 390px, where the worst dial is
     0.49 steps per pixel.

     On its own line the slider takes the menu's full width, which puts every dial back under one
     step per pixel. Keyed on `:has()` so a row earns this by holding a slider rather than by being
     told to. */
  .row:has(input[type="range"]) {
    grid-template-columns: minmax(0, 1fr);
    gap: 0.1rem;
  }
  .row input[type="range"] {
    width: 100%;
  }

  /* **A stated width, for the reason the menu has one.** `width: 100%` reads as the obvious answer
     and does nothing here: the control sits in an `auto` grid track, so a percentage resolves
     against a column that is itself sized by this element — circular, and the select goes on
     measuring its own chosen value. A length breaks the loop, so picking a palette no longer
     changes the width of the thing you picked it from. Long names clip; `theme.css` caps the
     control at `100%` and a `<select>` shows only its chosen value anyway. */
  .row :global(select) {
    width: 8rem;
  }
  /* The two decoration switches side by side, each word beside its own switch. Wraps rather than
     pushing the menu wider at a large Interface size. */
  .pair {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--gap-xs) var(--gap);
  }
  .pair label {
    display: inline-flex;
    align-items: center;
    gap: var(--gap-xs);
  }
  .row em {
    color: var(--text-faint);
    font-style: normal;
    font-variant-numeric: tabular-nums;
  }
  .menu hr {
    width: 100%;
    margin: var(--gap-xs) 0;
    border: 0;
    border-top: 1px solid var(--border);
  }
</style>
