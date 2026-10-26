<script lang="ts">
  /**
   * Copy one word to the clipboard, and say so for a moment afterwards.
   *
   * Its own component for the same reason `PronounceButton` is: two surfaces now flank a word
   * with it — the front of a Reference result card and the character tools inside an opened
   * one — and the transient "copied" state is the component's own business. Left inside
   * `WordHeader` it could only ever appear where that header appeared.
   */
  import { invoke } from "@tauri-apps/api/core";

  let { word }: { word: string } = $props();

  let state = $state<"idle" | "copied" | "failed">("idle");
  let reset: ReturnType<typeof setTimeout> | undefined;

  async function copy() {
    clearTimeout(reset);
    try {
      await invoke("copy_text", { text: word });
      state = "copied";
    } catch {
      state = "failed";
    }
    reset = setTimeout(() => (state = "idle"), 1_600);
  }

  // A card reused for a different word would otherwise keep the previous one's tick.
  $effect(() => {
    void word;
    state = "idle";
    clearTimeout(reset);
    return () => clearTimeout(reset);
  });
</script>

<!-- Two sheets, drawn rather than typed: it is the conventional copy silhouette, it keeps
     the same fine outline as the app's other action icons, and it cannot disappear because
     one of the two bundled windows lacks a particular symbol font. The check occupies the
     same box after success, so feedback does not move the word. -->
<button
  class="copy"
  class:copied={state === "copied"}
  class:failed={state === "failed"}
  type="button"
  onclick={copy}
  aria-label={state === "copied" ? `Copied ${word}` : `Copy ${word}`}
  data-tip={state === "copied"
    ? "Copied to the clipboard"
    : state === "failed"
      ? "Could not copy to the clipboard"
      : "Copy this word to the clipboard"}
>
  <svg class="copy-state" class:off={state === "copied"} viewBox="3 2 18 20" aria-hidden="true">
    <path d="M8 7H6.6A1.6 1.6 0 0 0 5 8.6v9.8A1.6 1.6 0 0 0 6.6 20h7.8a1.6 1.6 0 0 0 1.6-1.6V17" />
    <rect x="8" y="4" width="11" height="13" rx="1.6" />
  </svg>
  <svg class="copy-state check" class:off={state !== "copied"} viewBox="3 2 18 20" aria-hidden="true">
    <path d="M5.5 12.5l4.1 4.1L18.8 7.4" />
  </svg>
</button>

<style>
  /* **No border.** This is an icon, not a boxed control: it sits directly beside a word, and
     giving it an edge turned it into a second pill competing with the speak button rather than
     a mark on the word. What it matches is the speak button's *height*, so the two ends of the
     row weigh the same — which is a size, not a shape.
     `align-self: center` for the reason `PronounceButton` records: the glyph is far smaller than
     the word between them, and a shared baseline hangs both off its bottom. */
  .copy {
    display: grid;
    place-items: center;
    align-self: center;
    width: 1em;
    height: 1em;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-dim);
    font-size: 1.15rem;
    line-height: 1;
    transition:
      color var(--anim) var(--ease),
      background var(--anim) var(--ease);
  }
  .copy:hover,
  .copy.copied {
    color: var(--accent);
    background: var(--accent-wash);
  }
  .copy.failed {
    color: var(--red);
  }
  .copy-state {
    grid-area: 1 / 1;
    /* Sized against the ▶ in the speak button beside it rather than against the text. The two
       do not share a box — that was tried and made them compete — so the glyphs matching in
       weight is the only thing holding the two ends of the row together. */
    width: 1em;
    height: 1em;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.7;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition:
      opacity var(--anim) var(--ease),
      scale var(--anim) var(--ease);
  }
  .copy-state.off {
    opacity: 0;
    scale: 0.65;
    pointer-events: none;
  }
  .copy-state.check {
    stroke-width: 2.1;
  }
</style>
