<script lang="ts">
  // A narrow rail down the left: the wordmark at the top, the three reading views under it, and
  // About on the floor.
  //
  // The views are a set rather than unrelated buttons — exactly one is current at any
  // moment, and the one you are not in is the way out of the one you are. That is what
  // lets the reference and help pages carry no back control of their own.
  //
  // **Every button here is now one of that set, and it used to be four and a gear.** The gear
  // opened a drawer, which is a different kind of act from switching page, so it had a different
  // treatment and its own `open` state — and Help sat beside it, at the bottom, because it was
  // reached about as often. The settings are a page now, opened from the title bar's menu the way
  // Windows Terminal and VS Code do it, and Help went into that menu with them. What is left in the
  // slot the two vacated is About, which inherits Help's `?` — the same glyph Terminal puts on its
  // own About — and the same reason for being at the bottom: consulted rarely, and about the app
  // rather than about Japanese.

  import type { View } from "../model";
  import Mark from "./Mark.svelte";
  import Sakura from "./Sakura.svelte";

  let {
    view,
    onview,
    switchable = true,
    sakuraSpread = 1,
  }: {
    view: View;
    onview: (view: View) => void;
    /**
     * Whether the two views can be switched between at all.
     *
     * False during first-run setup, which takes precedence over both of them. Left live,
     * the buttons still moved the accent and `aria-current` onto a page that was not on
     * screen — and silently turned the arrow keys off with nothing to explain why.
     */
    switchable?: boolean;
    /** Passed straight through to the sprig — see `Sakura.svelte` for why it is a prop. */
    sakuraSpread?: number;
  } = $props();
</script>

<nav class="rail">
  <!-- The sprig is clipped by this rather than by `.rail`, because `overflow: hidden`
       there would also cut off the `.tip-side` tooltips, which open outside the rail. -->
  <div class="decor" aria-hidden="true"><Sakura spread={sakuraSpread} /></div>
  <!-- A seal, not two letters. The rail is `--rail` across and the previous stacked 屠つ
       spent all of it on a name the <h1> already gives a screen reader; a seal says the
       same thing in one glyph and looks like what it is.

       No plate and no border: both spent most of the rail on the container rather than
       on the character. The default size is the mark's own. -->
  <Mark />
  <hr />

  <!-- Inline SVG rather than a font or an image: two windows share this bundle, and a missing
       font glyph would leave a blank button with no way to guess it. -->
  <button
    class="view tip-side"
    class:current={view === "read"}
    onclick={() => onview("read")}
    disabled={!switchable}
    data-tip="Read — the passage and its analysis"
    aria-label="Read"
    aria-current={view === "read" ? "page" : undefined}
  >
    <!-- A sheet of text with one word picked out of it, which is what this view does: it
         breaks a passage into words and puts the analysis of whichever one you choose
         beside it. The filled bar is that word.

         It was a magnifying glass, which says "search" — and searching is the button
         directly below this one. The two had each other's metaphors: the page that looks
         things up wore the book, and the page you read wore the magnifier. A sheet against
         that book is the pair that reads right, one you read and one you consult.

         Bare strokes were tried first and came out looking like a menu at 18px; the sheet
         gives it a silhouette nothing else here has. -->
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
      <rect
        x="4.5" y="3.5" width="15" height="17" rx="2.4"
        fill="none" stroke="currentColor" stroke-width="1.9"
      />
      <path
        d="M8 8h8M8 16h8"
        fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"
      />
      <rect x="8" y="11" width="5.5" height="2" rx="1" fill="currentColor" />
    </svg>
  </button>

  <button
    class="view tip-side"
    class:current={view === "reference"}
    onclick={() => onview("reference")}
    disabled={!switchable}
    data-tip="Reference — look a word or grammar point up"
    aria-label="Reference"
    aria-current={view === "reference" ? "page" : undefined}
  >
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
      <!-- An open book: two leaves meeting at a spine. -->
      <path
        d="M12 6.6C10.4 5.5 8.4 5 6 5H3.4v12.2H6c2.4 0 4.4.5 6 1.6 1.6-1.1 3.6-1.6 6-1.6h2.6V5H18c-2.4 0-4.4.5-6 1.6Z"
        fill="none"
        stroke="currentColor"
        stroke-width="1.7"
        stroke-linejoin="round"
      />
      <path d="M12 6.6v12.2" fill="none" stroke="currentColor" stroke-width="1.7" />
    </svg>
  </button>

  <button
    class="view tip-side"
    class:current={view === "glossary"}
    onclick={() => onview("glossary")}
    disabled={!switchable}
    data-tip="Glossary — every grammar term the reader can explain"
    aria-label="Glossary"
    aria-current={view === "glossary" ? "page" : undefined}
  >
    <!-- A word with the grammar mark under it — the dashed rule `Reader.svelte` paints
         beneath a word the engine can explain. The same instinct as the ア on the furigana
         button: draw the thing the feature does rather than borrow a convention.

         Three dashes, not the reader's fine 4px dotting. Rasterised at 18px the fine
         version closed into a solid bar and the icon became three stacked rules — the
         hamburger this rail has already produced once. At this period the gaps survive.

         The rules sit at 8 and 16 rather than 9 and 15.5: that centres the ink in the
         viewBox exactly, and takes its height from 8.7 to 10.2 — this was the shortest
         icon in a rail whose others run 15.5 to 18.9, and read light beside them. -->
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
      <path
        d="M5 8h14"
        fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"
      />
      <!-- `butt`, not `round`: round caps add half the stroke width at each end, which at
           this size closes the gaps and undoes the whole point. -->
      <path
        d="M5 16h14"
        fill="none" stroke="currentColor" stroke-width="2.2"
        stroke-linecap="butt" stroke-dasharray="3 2.2"
      />
    </svg>
  </button>

  <!-- On the floor, where Help was and for Help's reason: consulted occasionally, and about the app
       rather than about Japanese. It keeps the glyph too — see the note at the top. -->
  <button
    class="view about tip-side"
    class:current={view === "about"}
    onclick={() => onview("about")}
    disabled={!switchable}
    data-tip="About — what this is built on, and who to thank"
    aria-label="About"
    aria-current={view === "about" ? "page" : undefined}
  >
    <!-- Outlined at the same weight as the three page icons above. It opens a page, and should say
         so — there is nothing left in this rail that does anything else. -->
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
      <circle
        cx="12" cy="12" r="8.5"
        fill="none" stroke="currentColor" stroke-width="1.8"
      />
      <path
        d="M9.6 9.4a2.5 2.5 0 1 1 3.1 2.6c-.5.2-.7.6-.7 1.1v.6"
        fill="none" stroke="currentColor" stroke-width="1.8"
        stroke-linecap="round" stroke-linejoin="round"
      />
      <!-- The dot, as its own round cap. A 1px rect reads as a smudge at this size. -->
      <path
        d="M12 16.6v.01"
        fill="none" stroke="currentColor" stroke-width="2"
        stroke-linecap="round"
      />
    </svg>
  </button>
</nav>

<style>
  .rail {
    /* So the sprig behind the buttons has something to be absolute against — and above
     * `main`, which is `z-index: 1`.
     *
     * The `z-index` is not optional. Lifting the buttons to `1` below makes each of them a
     * stacking context, which traps their tooltips: a tooltip at `z-index: 4` inside a
     * button at `1` cannot get past `main`, also at `1`, and loses the tie on source order
     * because the rail comes first. One context here, above `main`, and everything inside
     * it sorts among itself.
     *
     * The settings used to be a drawer at `5`, over both. They are a page now, so nothing in the
     * app covers this — except the title bar's dropdown, which is why `.titlebar` is lifted above
     * this in `App.svelte` rather than the menu being lifted inside it. */
    position: relative;
    z-index: 2;
    flex: 0 0 auto;
    width: var(--rail);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 1rem 0 var(--gap-sm);
    background: var(--surface);
    /* No border down the right. The reading area beside it is a different surface with a
       rounded corner at the top, and that curve is what separates them — a hairline
       running into it read as unresolved rather than as an edge. */
  }

  /* Everything the rail is *for* sits above the decoration. The sprig is `z-index: 0`
     rather than negative, because negative would put it behind the rail's own background
     and it would simply not be there. */
  .rail > :global(:not(.decor)) {
    position: relative;
    z-index: 1;
  }

  .decor {
    position: absolute;
    inset: 0;
    overflow: hidden;
    z-index: 0;
    pointer-events: none;
  }

  hr {
    width: 60%;
    margin: var(--gap-sm) 0 0;
    border: 0;
    border-top: 1px solid var(--border);
  }

  /* One shape for every rail button. It used to be shared with `.gear`, which was a different kind
     of control wearing the same box on purpose; every button here is a view now, so the sharing is
     no longer holding anything together and the rule is simply the button. */
  /* **The glyph follows the interface size, and it did not.** These are `width="18"` in the
     markup — 18 device pixels whatever the reader has set — while the button around them is `2rem`
     and the rail `2.9rem`. At the default the icon is 60% of its button; at the smallest interface
     size the button shrinks to 24px and the icon does not, so it fills three quarters of it and the
     rail reads as broken. `1.2rem` is the same 18px at the default size and stays 60% at every
     other. CSS rather than the attributes, because a presentation attribute loses to a rule and
     the `viewBox` is what actually draws the art. */
  .view svg {
    width: 1.2rem;
    height: 1.2rem;
  }
  .view {
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    /* **The browser's own `button` padding, which is 6px and does not scale.**
       Everything here is `rem`, so the button shrinks with the interface size and that padding
       does not. `box-sizing` is `border-box`, so at 12px the 24px button keeps 6px each side and
       1px of border and offers its content 10px — narrower than the 14.4px icon. The icon
       overflows the track it is centred in, and `place-items: center` centres it against a box
       that starts 7px in, so it sits right of centre by a margin that grows as the interface
       shrinks. Below 18px it was visibly off; at 12px it was 4.4px out of 24.
       A fixed square with a centred glyph has no use for padding at all. */
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: none;
    color: var(--text-faint);
    margin-top: var(--gap-sm);
    transition:
      color var(--anim) var(--ease),
      background var(--anim) var(--ease),
      border-color var(--anim) var(--ease);
  }

  .view:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-hover);
  }
  .view:disabled {
    opacity: 0.4;
    cursor: default;
  }
  /* The view you are in, in the accent. The gear took the same treatment when its drawer was open,
     because it meant the same thing; the title bar's settings button still does. */
  .view.current {
    color: var(--accent);
    background: var(--accent-wash);
    border-color: var(--accent-edge);
  }

  /* The one that sits on the floor — `auto` eats the space above it, so About stays at the bottom
     however many views are added above. It carries `margin-top` twice as a result, which is fine:
     `auto` beats the `--gap-sm` it overrides, and the shared value is what every other view wants. */
  .view.about {
    margin-top: auto;
  }
</style>
