<script lang="ts">
  import { tick } from "svelte";
  import type { KanjiView } from "../model";
  import Character from "./Character.svelte";
  import { revealEntryHeading } from "./entry-scroll.js";

  let {
    characters,
    selected = $bindable(null),
    onlookup,
    scroller = null,
  }: {
    characters: KanjiView[];
    selected?: string | null;
    onlookup: (text: string) => void;
    scroller?: HTMLElement | null;
  } = $props();

  let choices = $state<HTMLElement | null>(null);
  let inspectorHeading = $state<HTMLElement | null>(null);
  const active = $derived(characters.find((character) => character.character === selected));
  $effect(() => {
    if (selected && !characters.some((character) => character.character === selected))
      selected = null;
  });

  async function close() {
    const previous = selected;
    selected = null;
    await tick();
    const button = previous
      ? choices?.querySelector<HTMLButtonElement>(`[data-character="${CSS.escape(previous)}"]`)
      : null;
    button?.focus({ preventScroll: true });
    if (scroller && button) revealEntryHeading(scroller, button);
  }

  async function choose(character: string) {
    selected = selected === character ? null : character;
    await tick();
    if (selected && scroller && inspectorHeading) revealEntryHeading(scroller, inspectorHeading);
  }
</script>

<section class="character-choices" aria-label="Word characters" bind:this={choices}>
  <h4>Characters</h4>
  <div class="choices">
    {#each characters as character (character.character)}
      <button
        type="button"
        data-character={character.character}
        aria-label={`Study ${character.character}`}
        aria-expanded={selected === character.character}
        aria-controls="reference-character-detail"
        onclick={() => choose(character.character)}
      >
        <span class="glyph" lang="ja">{character.character}</span>
        {#if character.meanings.length}<span class="meaning">{character.meanings[0]}</span>{/if}
      </button>
    {/each}
  </div>
</section>

{#if active}
  <aside
    class="character-inspector"
    id="reference-character-detail"
    aria-label={`Details for ${active.character}`}
  >
    <header bind:this={inspectorHeading}>
      <h4>Character details</h4>
      <button type="button" class="close" aria-label="Close character details" onclick={close}
        >Close</button
      >
    </header>
    {#key active.character}<Character character={active} {onlookup} editorial />{/key}
  </aside>
{/if}

<style>
  .character-choices,
  .character-inspector {
    min-width: 0;
  }
  h4 {
    margin: 0 0 var(--gap-sm);
    color: var(--text-dim);
    font-size: 0.9rem;
    font-weight: 500;
  }
  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: var(--gap-sm) var(--gap-lg);
  }
  .choices button {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    max-width: 100%;
    min-width: 0;
    padding: var(--gap-xs) 0;
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .choices button[aria-expanded="true"] {
    color: var(--accent);
    border-bottom-color: var(--accent);
  }
  .choices button:hover {
    color: var(--accent);
  }
  .glyph {
    flex: 0 0 auto;
    font-family: var(--font-kanji);
    font-size: calc(1.5rem * var(--panel-scale, 1));
  }
  .meaning {
    min-width: 0;
    max-width: 22ch;
    color: var(--text-dim);
    font-size: 0.85rem;
    overflow-wrap: anywhere;
  }
  .character-inspector {
    border-top: 1px solid var(--border);
    padding-top: var(--gap);
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--gap-sm);
    margin-bottom: var(--gap);
  }
  header h4 {
    margin: 0;
  }
  .close {
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-dim);
    font: inherit;
    font-size: 0.8rem;
    text-decoration: underline;
    text-underline-offset: 0.2em;
    cursor: pointer;
  }
</style>
