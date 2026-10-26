<script lang="ts">
  // Identity and actions for the one selected-word surface. Meanings and grammar deliberately
  // do not live here: the detailed sections immediately below are the authoritative answer.
  import { WORD_STATUSES, type Token, type WordStatus, type WordView } from "../model";
  import { readingWorthShowing, surfaceWorthShowing } from "./kana.js";
  import PronounceButton from "./PronounceButton.svelte";
  import { pronunciation } from "./pronunciation.svelte";

  let {
    token, word, status, onstatus, onanki, onplay, playing, audioMessage,
    ankiBusy, ankiMessage, ankiFailed, ankiReady,
  }: {
    token: Token | null;
    word: WordView | null;
    status: WordStatus;
    onstatus: (status: WordStatus) => void;
    onanki: () => void;
    onplay: () => void;
    playing: boolean;
    audioMessage: string;
    ankiBusy: boolean;
    ankiMessage: string;
    ankiFailed: boolean;
    ankiReady: boolean;
  } = $props();

  const expression = $derived(word?.headword ?? token?.dict_form ?? token?.surface ?? "");
  const showsReading = $derived(readingWorthShowing(expression, word?.reading));
  const showsSurface = $derived(surfaceWorthShowing(expression, token?.surface));
  const spoken = $derived({ expression, reading: word?.reading ?? token?.lemma_reading ?? "" });
  const speakable = $derived(pronunciation.has(spoken.expression, spoken.reading));

  $effect(() => pronunciation.want(spoken.expression, spoken.reading));
</script>

<header class="selected-word-header" data-testid="selected-word-header">
  {#if !token}
    <p class="hint">Select a word in the passage.</p>
  {:else}
    <div class="identity">
      <div class="word">
        <h2 class="headword" lang="ja">{expression}</h2>
        {#if showsReading}<span class="reading" lang="ja">{word?.reading}</span>{/if}
        {#if speakable}
          <PronounceButton word={expression} onplay={onplay} busy={playing} message={audioMessage} />
        {/if}
        {#if showsSurface}<span class="flag base">as written <span lang="ja">{token.surface}</span></span>{/if}
      </div>

      <div class="word-actions">
        <div class="status" role="group" aria-label="How well you know this word">
          {#each WORD_STATUSES as option}
            <button
              class="control mark {option}"
              class:active={status === option}
              aria-pressed={status === option}
              onclick={() => onstatus(option)}
            >{option}</button>
          {/each}
        </div>
        {#if ankiReady && word && (word.inline.length > 0 || word.name)}
          <button class="control mark anki" onclick={onanki} disabled={ankiBusy} data-tip="Add to Anki">
            {ankiBusy ? "adding…" : "+ Anki"}
          </button>
        {/if}
      </div>
    </div>

    <!-- **Only an Anki result lands here now.** "Reset details" used to share this row, which
         meant the header grew a line the moment a disclosure was opened and lost it again on
         reset — the identity strip changing height as a side effect of something happening
         below it. It lives at the end of the pane headings instead, where there was already
         room for it. -->
    {#if ankiMessage}
      <div class="messages">
        <span class:failed={ankiFailed}>{ankiMessage}</span>
      </div>
    {/if}
  {/if}
</header>

<style>
  /* **Sunken, so the identity row is not the same surface as everything under it.**
     The card is `--surface` and this had no background of its own, so the headword, its status
     buttons and the whole of the study detail were one flat plane with a hairline across it.

     `--surface-sunken` rather than `--bg`: the ramp keeps the same order in both themes, but in
     light mode `--bg` is 0.016 under `--surface` — a step `theme.css` itself describes as one
     that measurably exists and visibly does not. Sunken is 0.050 under it there and 0.064 in
     dark, which reads either way. `local documentation` carries the widened role.

     The rule stays. At the top of a rounded card the colour change alone leaves a soft edge,
     and the border is what makes it a seam. */
  .selected-word-header {
    flex: 0 0 auto;
    padding: var(--gap-sm) var(--gap);
    background: var(--surface-sunken);
    border-bottom: 1px solid var(--border);
    display: grid;
    gap: var(--gap-sm);
  }
  .identity, .word-actions, .status, .messages {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
  }
  /* Both sides describe the same selected word. Centre the action cluster against the complete
     identity row so the compact status buttons do not hang from the headword's text baseline. */
  .identity { justify-content: space-between; align-items: center; gap: var(--gap-sm) var(--gap); }
  /* The three pronunciation cues are one horizontal identity, despite their different type and
     control sizes. A shared centre line keeps the smaller kana from hanging off the headword's
     baseline and remains correct when panel scaling changes. */
  .word {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--gap-xs) var(--gap-sm);
  }
  .headword {
    margin: 0;
    color: var(--text);
    font-size: calc(1.45rem * var(--panel-scale, 1));
    font-weight: inherit;
    line-height: 1.15;
  }
  .reading { color: var(--text-dim); font-size: calc(0.9rem * var(--panel-scale, 1)); }
  .word-actions { align-items: center; }
  .word-actions, .status { gap: var(--gap-xs); }
  .mark { font-size: 0.68rem; text-transform: capitalize; padding: 0.14rem 0.48rem; color: var(--text-faint); }
  .mark:hover:not(:disabled) { color: var(--text); border-color: var(--border-strong); }
  .mark.active { color: var(--text); }
  .mark.new.active { background: var(--blue-wash); border-color: var(--blue-edge); }
  .mark.seen.active { background: var(--yellow-wash); border-color: var(--yellow-edge); }
  .mark.known.active { background: var(--good-wash); border-color: var(--good-edge); }
  .mark.ignored.active { background: var(--red-wash); border-color: var(--red-edge); color: var(--text-dim); }
  .mark.anki { text-transform: none; }
  .mark.anki:hover:not(:disabled) { color: var(--accent); border-color: var(--accent-edge); }
  .hint, .messages { color: var(--text-faint); font-size: 0.78rem; }
  .hint { margin: 0; }
  .messages { justify-content: space-between; gap: var(--gap-sm); }
  .messages .failed { color: var(--red); }
</style>
