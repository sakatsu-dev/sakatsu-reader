<script lang="ts">
  let {
    word,
    onplay,
    busy = false,
    message = "",
  }: {
    word: string;
    onplay: () => void;
    busy?: boolean;
    message?: string;
  } = $props();
</script>

<!-- Whether this is drawn at all is the caller's decision, and both callers ask
     `pronunciation.has(...)` first. A word found to have no recording loses its button rather
     than keeping one that reports a miss.
     Not decided here, because the endpoint cannot be asked in advance: it answers a miss with a
     placeholder MP3 and a 200, so a word is only known to be silent after it has been played
     once. `pronunciation.svelte.ts` holds that knowledge and explains why it is not eager. -->
<button
  class="control speak"
  type="button"
  onclick={onplay}
  disabled={busy}
  aria-label={`Hear ${word}`}
  data-tip="Hear this word"
>
  {busy ? "♪" : "▶"}
</button>
{#if message}<span class="audio-note" aria-live="polite">{message}</span>{/if}

<style>
  /* **Centred against the headword, not sat on its baseline.**
   *
   * Baseline is right for text beside text, and wrong for this: the glyph inside is 0.75rem
   * against a headword more than twice that, so aligning the two baselines hung a small control
   * off the bottom of a large word. The copy button on the other side does not have the problem
   * — it is `1em` at the headword's own size, so its box already matches the glyph — and the two
   * flanking the same word at visibly different heights was the tell.
   *
   * `center` rather than a nudge, because the headword scales with `--panel-scale` and any
   * fixed offset would only be correct at one setting. */
  .speak {
    align-self: center;
    padding: 0.05rem 0.5rem;
    font-size: 0.75rem;
  }
  .speak:hover:not(:disabled) {
    color: var(--accent);
    border-color: var(--accent-edge);
  }
  .audio-note {
    align-self: baseline;
    color: var(--text-faint);
    font-size: 0.7rem;
  }
</style>
