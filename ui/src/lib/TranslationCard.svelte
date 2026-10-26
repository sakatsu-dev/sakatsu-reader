<script lang="ts">
  import type { SentenceTranslation, VariantTranslations } from "../model";
  import Carries from "./Carries.svelte";
  import { alignedSentenceFlow } from "./translation-card.js";

  let {
    sentences = [],
    sentencesBusy = false,
    sentencesError = "",
    comparison = null,
    comparisonBusy = false,
    comparisonError = "",
    comparing = false,
    activeToken = null,
    contextView = false,
    contextAvailable = false,
    contextTranslation = "",
    contextBusy = false,
    contextError = "",
    maxHeight = 240,
    onclearcomparison,
    onretrycomparison,
    onretrysentences,
    onretrycontext,
    oncontextview,
  }: {
    sentences?: SentenceTranslation[];
    sentencesBusy?: boolean;
    sentencesError?: string;
    comparison?: VariantTranslations | null;
    comparisonBusy?: boolean;
    comparisonError?: string;
    comparing?: boolean;
    activeToken?: number | null;
    contextView?: boolean;
    contextAvailable?: boolean;
    contextTranslation?: string;
    contextBusy?: boolean;
    contextError?: string;
    /**
     * The tallest the translation may grow before it scrolls inside the card, so a long passage
     * cannot push the panels below it off screen. Below that it is shown whole, always.
     */
    maxHeight?: number;
    onclearcomparison: () => void;
    onretrycomparison: () => void;
    onretrysentences: () => void;
    onretrycontext: () => void;
    oncontextview: (context: boolean) => void;
  } = $props();

  const flow = $derived(
    alignedSentenceFlow(sentences, {
      comparing,
      activeToken,
      comparison,
      comparisonBusy,
      comparisonError,
    }),
  );
  // **The whole translation, always.** It was clamped to three lines behind "Show full
  // translation", which cut sentences off mid-line on the screen whose job is to show them.
  let body: HTMLDivElement | undefined = $state();

  // The transformed sentence can sit below several surrounding sentences. Reveal it only inside
  // this local scrollport; the passage and Selected word card must not jump.
  $effect(() => {
    void comparison;
    void comparisonBusy;
    void comparisonError;
    if (!comparing || contextView) return;
    const frame = requestAnimationFrame(() =>
      body?.querySelector<HTMLElement>("[data-active-sentence=true]")?.scrollIntoView({
        block: "nearest",
      }),
    );
    return () => cancelAnimationFrame(frame);
  });

</script>

<section class="translation-card" aria-labelledby="translation-title" data-testid="translation-card">
  <header>
    <div class="identity">
      <h2 id="translation-title">Translation</h2>
      <!-- Named in both directions now that the whole passage is the default. "Back to
           aligned" was the first thing a reader saw, which reads as an undo of something they
           never did. -->
      {#if contextView}
        <span class="mode">Whole passage</span>
      {:else if comparing}
        <span class="mode">Focused comparison</span>
      {/if}
    </div>
    <div class="card-actions">
      {#if contextAvailable}
        <button
          class="control quiet"
          type="button"
          aria-describedby="context-check-help"
          onclick={() => oncontextview(!contextView)}
        >{contextView ? "Read line by line" : "Read as one passage"}</button>
        <span class="sr-only" id="context-check-help">
          Reading as one passage translates every sentence together, which is what lets it
          recover a subject the Japanese left out. Line by line translates each sentence on its
          own and lines the English up against the passage above.
        </span>
      {/if}
      {#if comparing}
        <button class="control quiet" type="button" onclick={onclearcomparison}
          >Clear comparison</button
        >
      {/if}
    </div>
  </header>

  <div
    class="translation-body"
    bind:this={body}
    style:--translation-max={`${maxHeight}px`}
    data-testid="translation-body"
    aria-live="polite"
  >
    {#if contextView}
      {#if contextBusy || (!contextTranslation && !contextError)}
        <p class="notice"><span class="spinner" aria-hidden="true"></span>Translating context…</p>
      {:else if contextError}
        <div class="failure" role="alert">
          <span>{contextError}</span>
          <button class="control quiet" type="button" onclick={onretrycontext}>Retry</button>
        </div>
      {:else}
        <p class="context-output" data-testid="context-translation">{contextTranslation}</p>
      {/if}
    {:else if sentences.length === 0}
      {#if sentencesBusy || !sentencesError}
        <p class="notice"><span class="spinner" aria-hidden="true"></span>Translating…</p>
      {:else}
        <div class="failure" role="alert">
          <span>{sentencesError}</span>
          <button class="control quiet" type="button" onclick={onretrysentences}>Retry</button>
        </div>
      {/if}
    {:else}
      <div class="sentence-flow" data-testid="aligned-translation">
        {#each flow as item (item.sentence.first_token)}
          <div
            class="sentence"
            class:active={item.active}
            data-testid="translation-sentence"
            data-active-sentence={item.active}
          >
            {#if item.comparison}
              <Carries result={item.comparison} />
            {:else}
              <p>{item.sentence.english}</p>
            {/if}
            {#if item.comparisonBusy}
              <p class="notice local">
                <span class="spinner" aria-hidden="true"></span>Updating comparison…
              </p>
            {:else if item.comparisonError}
              <div class="failure local" role="alert">
                <span>{item.comparisonError}</span>
                <button class="control quiet" type="button" onclick={onretrycomparison}>Retry</button>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>

</section>

<style>
  .translation-card {
    flex: 0 0 auto;
    min-width: 0;
    padding: var(--gap-sm) var(--gap);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
  }
  header,
  .identity,
  .card-actions,
  .failure,
  .notice {
    display: flex;
    align-items: center;
  }
  header {
    justify-content: space-between;
    gap: var(--gap);
    margin-bottom: var(--gap-xs);
  }
  .identity,
  .card-actions { gap: var(--gap-sm); min-width: 0; }
  .card-actions {
    justify-content: flex-end;
    flex-wrap: wrap;
  }
  h2 {
    margin: 0;
    color: var(--text-dim);
    font-size: 0.68rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }
  .mode {
    color: var(--text-faint);
    font-size: 0.72rem;
  }
  .quiet {
    flex: 0 0 auto;
    padding: 0.2rem 0.5rem;
    font-size: 0.72rem;
  }
  /* **Both size controls, each doing its own job.** `1rem` follows Interface size, which sets the
     root font size; `--panel-scale` is Definitions size, and multiplying by it is how every other
     panel component opts into that control.

     It used to read `font-size: calc(var(--panel-font) * 1rem)` on `.translation-card`, and there
     is no `--panel-font` anywhere in this project — so the declaration was invalid at
     computed-value time and dropped. The body text still followed Interface size by inheriting a
     plain `1rem`; what it never did was respond to Definitions size, alone among the panels.

     It belongs on the body rather than the card: the translation is the text being read, and the
     header chrome above declares absolute `rem` sizes and is deliberately left out.

     **Never clamped.** The whole translation shows; only past `--translation-max` — a share of the
     analysis pane, from `App.svelte` — does it scroll, so a long passage cannot push the panels
     below off screen. */
  .translation-body {
    min-width: 0;
    font-size: calc(1rem * var(--panel-scale, 1));
    max-height: var(--translation-max);
    overflow-y: auto;
    line-height: 1.55;
  }
  p { margin: 0; }
  .sentence + .sentence { margin-top: 0.25em; }
  .failure {
    justify-content: space-between;
    gap: var(--gap);
    color: var(--red);
  }
  .notice { gap: var(--gap-xs); color: var(--text-faint); }
  .local {
    margin-top: var(--gap-xs);
    font-size: 0.82em;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
</style>
