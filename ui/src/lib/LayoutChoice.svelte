<script lang="ts">
  // Reference's binary presentation choice. One action glyph keeps it compact and labels the
  // layout it will produce, rather than spending two buttons on a state with only one possible
  // next action.
  //
  // **Reference only.** The glossary used to make the same choice and no longer does: it is a
  // list of names read top to bottom, and one column is the whole of it.
  import type { BrowseLayout } from "../model";

  let {
    layout = $bindable("cards"),
    flush = false,
  }: {
    layout?: BrowseLayout;
    /** Join a bordered input bar whose edge and fill are already supplied by the parent. */
    flush?: boolean;
  } = $props();

  const next = $derived<BrowseLayout>(layout === "cards" ? "rows" : "cards");
  const says = $derived(
    next === "cards" ? "Show as cards — a block each" : "Show as rows — a row each",
  );
</script>

<button
  type="button"
  class="browse-layout-choice icon tip-right"
  class:control={!flush}
  class:flush
  onclick={(event) => {
    event.preventDefault();
    layout = next;
  }}
  aria-label={says}
  data-tip={says}
>
  <svg class="face rows" class:off={next !== "rows"} viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
    <path d="M4 7h16M4 12h16M4 17h16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
  </svg>
  <svg class="face grid" class:off={next !== "cards"} viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
    <rect x="4" y="4" width="7" height="7" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.9" />
    <rect x="13" y="4" width="7" height="7" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.9" />
    <rect x="4" y="13" width="7" height="7" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.9" />
    <rect x="13" y="13" width="7" height="7" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.9" />
  </svg>
</button>

<style>
  .icon { display: grid; place-items: center; width: 1.6rem; padding: 0.16rem; }
  .flush { width: var(--bar-button, 2.2rem); border: 0; border-left: 1px solid var(--border); border-radius: 0; background: none; color: var(--text-dim); transition: color var(--anim) var(--ease), border-color var(--anim) var(--ease); }
  .flush:hover { color: var(--text); }
  .face { grid-area: 1 / 1; transition: opacity var(--anim) var(--ease), rotate var(--anim) var(--ease); }
  .face.off { opacity: 0; rotate: -90deg; }

  /* The stored preference survives a narrow window, but a choice that cannot be expressed there
     must not offer a no-op control. Rows are forced by each collection's container rule. */
  @container reference (max-width: 41.999rem) { .icon { display: none; } }
</style>
