<script lang="ts">
  // One expanded character. Supplemental sources are lazy and optional: KANJIDIC facts remain
  // useful on their own, while a diagram/component failure removes only that small affordance.
  import { invoke } from "@tauri-apps/api/core";
  import HighlightedText from "./HighlightedText.svelte";
  import { splitReading } from "./kana.js";
  import type { CompoundsResult, KanjiDiagram, KanjiView, RelatedKanjiResult } from "../model";

  let {
    character,
    onlookup,
    editorial = false,
  }: {
    character: KanjiView;
    /** Left-aligned study details in Reference; other surfaces keep their presentation. */
    editorial?: boolean;
    /** The Reference page's ordinary current-tab search. */
    onlookup: (text: string) => void;
  } = $props();

  let diagram = $state<KanjiDiagram | null>(null);
  let diagramBusy = $state(false);
  let playing = $state(false);
  let playVersion = $state(0);
  let openComponent = $state<string | null>(null);
  let related = $state<RelatedKanjiResult | null>(null);
  let relatedBusy = $state(false);
  let compounds = $state<CompoundsResult | null>(null);
  let diagramGeneration = 0;
  let relatedGeneration = 0;
  let compoundGeneration = 0;

  /** How many compounds a character shows before it says how many more there are. */
  const COMPOUND_LIMIT = 6;

  $effect(() => {
    const literal = character.character;
    const request = ++diagramGeneration;
    diagram = null;
    diagramBusy = true;
    playing = false;
    openComponent = null;
    related = null;

    // The words written with this character, asked for as it opens — the same moment the diagram
    // is. A generation guard for the same reason: a slow answer for a character the reader has
    // already moved off must not land on the one they are looking at.
    const compoundRequest = ++compoundGeneration;
    compounds = null;
    invoke<CompoundsResult>("kanji_compounds", { character: literal, limit: COMPOUND_LIMIT })
      .then((answer) => {
        if (compoundRequest === compoundGeneration && literal === character.character) {
          compounds = answer;
        }
      })
      .catch(() => {});
    invoke<KanjiDiagram | null>("kanji_diagram", { character: literal })
      .then((answer) => {
        if (request === diagramGeneration && literal === character.character) diagram = answer;
      })
      .catch(() => {})
      .finally(() => {
        if (request === diagramGeneration) diagramBusy = false;
      });
  });

  function play() {
    playVersion += 1;
    playing = true;
  }

  async function chooseComponent(component: string) {
    if (openComponent === component) {
      openComponent = null;
      related = null;
      return;
    }
    openComponent = component;
    related = null;
    relatedBusy = true;
    const request = ++relatedGeneration;
    try {
      const answer = await invoke<RelatedKanjiResult>("kanji_related", {
        component,
        excluding: character.character,
        limit: 24,
      });
      if (request === relatedGeneration && openComponent === component) related = answer;
    } catch {
      // The base character detail remains complete without a related row.
    } finally {
      if (request === relatedGeneration) relatedBusy = false;
    }
  }
</script>

<!-- **One reading, drawn in two tones.** KANJIDIC separates the kanji's own sound from the kana
     written beside it with a period, and at this size a baseline dot between two kana is easy to
     miss — which means misreading which sound the character is actually responsible for. The
     okurigana is drawn a step fainter instead, so the split is carried by tone rather than by a
     mark. No mark would have been safe anyway: `〜` means an affix reading, `・` joins alternate
     spellings, and a dash beside kana reads as `ー`. See `splitReading` in `kana.js`. -->
{#snippet written(reading: string, here: boolean)}
  {@const parts = splitReading(reading)}
  <span class="reading" class:here lang="ja"
    >{parts.stem}{#if parts.okurigana}<span class="okurigana">{parts.okurigana}</span>{/if}</span
  >
{/snippet}

<div class="character" class:editorial>
  <div class="visual">
    {#if diagram}
      {#key playVersion}
        <svg
          class="diagram"
          class:playing
          viewBox="0 0 109 109"
          role="img"
          aria-label={`Stroke-order diagram for ${character.character}`}
        >
          {#each diagram.strokes as stroke, index}
            <path class="stroke" style={`--stroke-index: ${index}`} pathLength="1" d={stroke.path} />
          {/each}
          {#each diagram.numbers as number, index}
            <text class="number" style={`--stroke-index: ${index}`} x={number.x} y={number.y}
              >{number.number}</text
            >
          {/each}
        </svg>
      {/key}
      <button class="control play" type="button" onclick={play}>
        {playing ? "Replay stroke order" : "Play stroke order"}
      </button>
    {:else}
      <span class="glyph" class:loading={diagramBusy} lang="ja">{character.character}</span>
    {/if}
  </div>

  <!-- **A label column**, which is what a character's facts are: pairs. Readings, strokes and
         school year used to be three inline rows and a line of prose, so a reader hunting for one
         of them had to read all of it. A `<dl>` because that is the element for this, and it
         costs nothing to be right about it. Kun before On, the order a learner meets them in. -->
    <dl class="facts">
      {#if character.kun.length > 0}
        <dt>Kun-yomi</dt>
        <dd>{#each character.kun as reading}{@render written(reading.text, reading.here)}{/each}</dd>
      {/if}
      {#if character.on.length > 0}
        <dt>On-yomi</dt>
        <dd>{#each character.on as reading}{@render written(reading.text, reading.here)}{/each}</dd>
      {/if}
      {#if character.nanori.length > 0}
        <dt>Nanori</dt>
        <dd>{#each character.nanori as reading}{@render written(reading, false)}{/each}</dd>
      {/if}
      <dt>Strokes</dt>
      <dd>{character.strokes}</dd>
      {#if character.grade !== null}
        <dt>School year</dt>
        <dd>{character.grade <= 6 ? character.grade : "secondary"}</dd>
      {/if}
    </dl>

  <!-- **Everything past the facts runs the full width, starting under the glyph.** The character
       is 7rem tall at most and the entry is far taller, so confining all of it to a column beside
       the glyph left a tall empty block below one and a cramped ribbon beside the other. -->
  <div class="block">
    {#if character.meanings.length > 0}
      {@const shown = character.meanings.slice(0, 6)}
      <h4 class="section">Character meanings</h4>
      <!-- Numbered, like every other list of senses on this page — `SenseRow` in the word detail
           and `.scent .n` in a result preview. One joined line said nothing about how many
           meanings a character has or where one ended. -->
      <ol class="meanings">
        {#each shown as meaning, index}
          <li><span class="n">{index + 1}.</span>{" "}{meaning}</li>
        {/each}
      </ol>
      {#if character.meanings.length > shown.length}
        <p class="bounded">Showing {shown.length} of {character.meanings.length}</p>
      {/if}
    {/if}

    {#if compounds && compounds.words.length > 0}
      <h4 class="section">Common compounds</h4>
      <ul class="compounds">
        {#each compounds.words as word}
          <li>
            <button type="button" onclick={() => onlookup(word.headword)} aria-label={`Look up ${word.headword}`}>
              <!-- The character marked inside the compound, so it is visible *why* this word is
                   on this page. `HighlightedText` segments the string rather than building
                   markup, which is what keeps dictionary content from becoming HTML. -->
              <span class="compound-word" lang="ja"><HighlightedText text={word.headword} needle={character.character} /></span>
              {#if word.reading && word.reading !== word.headword}<span class="compound-reading" lang="ja">{word.reading}</span>{/if}
              <span class="compound-gloss">{word.glosses.join("; ")}</span>
            </button>
          </li>
        {/each}
      </ul>
      {#if compounds.total > compounds.words.length}
        <p class="bounded">Showing {compounds.words.length} of {compounds.total}</p>
      {/if}
    {/if}

    {#if character.components.length > 0}
      <h4 class="section">Components</h4>
      <div class="components">
        <span class="component-list">
          {#each character.components as component}
            <button
              class="component"
              class:open={openComponent === component}
              type="button"
              aria-expanded={openComponent === component}
              onclick={() => chooseComponent(component)}
              lang="ja"
            >{component}</button>
          {/each}
        </span>
        <span class="component-note">Visible parts; not necessarily traditional radicals.</span>
      </div>
    {/if}

    {#if openComponent}
      <div class="related" aria-live="polite">
        {#if relatedBusy}
          <span class="related-note">Finding related kanji…</span>
        {:else if related && related.characters.length > 0}
          <span class="related-note">
            {related.total} character{related.total === 1 ? "" : "s"} share {openComponent}
          </span>
          <div class="related-row">
            {#each related.characters as item}
              <button
                class="related-kanji"
                type="button"
                title={item.meanings.slice(0, 3).join("; ")}
                aria-label={`Look up ${item.character}${item.meanings[0] ? `, ${item.meanings[0]}` : ""}`}
                onclick={() => onlookup(item.character)}
                lang="ja"
              >{item.character}</button>
            {/each}
          </div>
        {:else if related}
          <span class="related-note">No other installed characters use this component.</span>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  /* **The glyph is 7rem tall and the entry is not.** Only the facts sit beside it; everything
     after them spans both columns, so the block continues *under* the character instead of
     leaving a tall empty square below it and a narrow ribbon of text alongside. */
  .character {
    min-width: 0;
    width: 100%;
    display: grid;
    grid-template-columns: minmax(5.5rem, 7rem) minmax(0, 1fr);
    column-gap: var(--gap);
    align-items: start;
    /* **Everything in this panel hangs off its right edge.** The readings were justified there
       first, and left the rest of the entry — headings, meanings, compounds — starting at the far
       left of a narrow column, so a single panel had two margins and the eye had to find a new
       one for every section. One edge to come back to, and it is the edge the readings already
       taught. Set once here rather than per block: the whole aside is one column of facts. */
    text-align: right;
  }
  .visual { grid-column: 1; grid-row: 1; }
  .facts { grid-column: 2; grid-row: 1; }
  .block { grid-column: 1 / -1; min-width: 0; }
  .visual { display: grid; justify-items: center; gap: var(--gap-xs); }
  /* **The taught shapes.** 教科書体 is the face Japanese children learn to write from, and this
     is the one glyph on the page drawn large enough for the difference to be information rather
     than decoration — a gothic UI face flattens exactly the stroke entries and exits a learner is
     looking at. See `--font-kanji`. */
  .glyph {
    font-family: var(--font-kanji);
    font-size: calc(2.6rem * var(--panel-scale, 1));
    line-height: 1;
    min-width: 2.6rem;
    text-align: center;
  }
  .glyph.loading { color: var(--text-dim); }
  .diagram { width: min(100%, 7rem); aspect-ratio: 1; overflow: visible; }
  .stroke {
    fill: none;
    stroke: var(--text);
    stroke-width: 3;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .number { fill: var(--text-faint); font-family: var(--font-mono); font-size: 8px; }
  .diagram.playing .stroke {
    stroke-dasharray: 1;
    stroke-dashoffset: 1;
    animation: draw-stroke 0.42s var(--ease) forwards;
    animation-delay: calc(var(--stroke-index) * 0.32s);
  }
  .diagram.playing .number {
    opacity: 0;
    animation: reveal-number 0.01s linear forwards;
    animation-delay: calc(var(--stroke-index) * 0.32s);
  }
  @keyframes draw-stroke { to { stroke-dashoffset: 0; } }
  @keyframes reveal-number { to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) {
    .diagram.playing .stroke,
    .diagram.playing .number { animation: none; stroke-dashoffset: 0; opacity: 1; }
    .diagram.playing .stroke { stroke-dasharray: none; }
  }
  .play {
    max-width: 100%;
    padding: 0.15rem 0.45rem;
    font-size: 0.7rem;
    line-height: 1.25;
    text-align: center;
    white-space: normal;
  }
  /* **A right-aligned label column.** Two columns rather than a label inline with its value:
     the labels differ in length, and left-aligning them puts every value at a different indent,
     which is what made these read as three unrelated lines instead of one table. */
  .facts {
    margin: 0;
    display: grid;
    /* **Both columns sized to their content, and the pair pushed right.** The value column used
       to be `1fr`, so it took every pixel left in the panel and the readings began wherever the
       labels happened to end — leaving a ragged margin of empty card to their right. Sized to
       content and justified to the end, the block finishes flush with the card's edge and the
       readings start from there, which is where the eye goes back to for the next row. */
    grid-template-columns: auto minmax(0, max-content);
    justify-content: end;
    align-items: baseline;
    column-gap: var(--gap);
    row-gap: 0.2rem;
  }
  .facts dt {
    text-align: right;
    font-family: var(--font-mono);
    font-size: 0.63rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-faint);
  }
  /* The readings wrap inside their own cell, so a long Kun-yomi list never pushes the column. */
  .facts dd {
    margin: 0;
    min-width: 0;
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: baseline;
    gap: var(--gap-xs);
    font-size: calc(0.88rem * var(--panel-scale, 1));
    color: var(--text);
  }
  /* A plain list. Flowing these into equal columns was tried and read worse: a meaning is a word
     or two, so `1fr` tracks spread six of them across the full width with enormous gaps between
     them. The room this entry was wasting is *under* the glyph, and `.block` above is what
     reclaims it. */
  .meanings { margin: 0; padding: 0; list-style: none; }
  /* **No hanging indent.** It existed so a wrapped meaning cleared its own ordinal on the left;
     against a right margin there is nothing on the left to clear, and the negative indent pulled
     the first line out past the edge everything else lines up on. */
  .meanings li { font-size: calc(0.88rem * var(--panel-scale, 1)); color: var(--text); }
  /* The ordinal treatment `SenseRow` and the result previews already use. */
  .meanings .n { color: var(--text-faint); font-size: 0.8em; font-variant-numeric: tabular-nums; }
  /* One per row, as the app this borrows from lists them: a compound carries a word, a reading
     and a gloss, so a full-width row lets the gloss run and the readings line up down the page.
     Two columns was tried and left the same gaps the meanings had. */
  .compounds { margin: 0; padding: 0; list-style: none; display: grid; gap: 0.1rem; }
  .compounds button {
    width: 100%;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--gap-xs);
    min-width: 0;
    /* Wrapped to the right with the rest, and the padding bled off the *right* so the hover
       background still finishes flush with the column instead of a third of a rem short of it. */
    justify-content: flex-end;
    padding: 0.15rem 0.3rem;
    margin-right: -0.3rem;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: inherit;
    font: inherit;
    text-align: right;
    cursor: pointer;
  }
  .compounds button:hover { background: var(--surface-hover); }
  .compound-word { font-size: calc(0.88rem * var(--panel-scale, 1)); color: var(--text); }
  .compound-reading { color: var(--text-dim); font-size: 0.78rem; }
  /* Truncated to one line: a compound row says which word it is, and the word's own page says
     the rest. 行く alone would otherwise run to fourteen glosses. */
  .compound-gloss { flex: 1; min-width: 0; color: var(--text-dim); font-size: 0.78rem; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* What a bounded list is holding back, said rather than implied. */
  .bounded { margin: 0.2rem 0 0; color: var(--text-faint); font-size: 0.7rem; }
  .reading { font-size: calc(0.95rem * var(--panel-scale, 1)); color: var(--text-dim); }
  /* **A step fainter than whatever the reading is**, which is why this is `opacity` and not a
     colour: a reading used by the word on screen is drawn in the accent and bold, and a second
     colour token here would have had to be chosen twice and kept in step with both. Opacity is
     one declaration that stays correct in either state. The weight is reset for the same reason —
     against a bold `.here` reading it is a second signal costing nothing. */
  .okurigana { opacity: 0.6; font-weight: 400; }
  .reading.here {
    color: var(--accent);
    font-weight: 700;
    background: var(--accent-wash);
    border-radius: var(--radius-sm);
    padding: 0.02rem 0.3rem;
    margin: 0 -0.05rem;
  }
  /* A plain block since the heading above replaced the label this grid's first column held. */
  .components { min-width: 0; }
  .component-list { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--gap-xs); }
  .component {
    min-width: 1.65rem;
    padding: 0.05rem 0.3rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    color: var(--text-dim);
    font: inherit;
    cursor: pointer;
  }
  .component:hover,
  .component.open { color: var(--accent); border-color: var(--accent-edge); background: var(--accent-wash); }
  .component-note { display: block; margin-top: 0.2rem; color: var(--text-faint); font-size: 0.7rem; }
  .related { margin-top: var(--gap-sm); padding-top: var(--gap-sm); border-top: 1px solid var(--border); }
  .related-note { display: block; color: var(--text-faint); font-size: 0.72rem; margin-bottom: var(--gap-xs); }
  .related-row { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--gap-xs); }
  .related-kanji {
    padding: 0.08rem 0.32rem;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    font-size: calc(1rem * var(--panel-scale, 1));
    cursor: pointer;
  }
  .related-kanji:hover { color: var(--accent); background: var(--accent-wash); }
  @container (max-width: 30rem) {
    .character { grid-template-columns: 1fr; }
    .visual { justify-items: start; }
    .diagram { width: 6rem; }
    /* **`grid-column: 2` has to go when there is no column 2**, and this was a real defect that
       right-aligning the panel finally made visible.
       With one explicit track, `.facts` naming column 2 creates an *implicit* one — and `-1`
       counts explicit grid lines only, so `.block`'s `1 / -1` stopped at the end of column 1 and
       the meanings, compounds and components were laid out 108px narrower than the panel holding
       them. Left-aligned they simply started at the left like everything else and nobody could
       see it; against a right margin they stopped 108px short of it.
       So the stack is stated rather than implied: both cells in the one column, in order. */
    .facts { grid-column: 1; grid-row: 2; }
  }
  .editorial { text-align: left; grid-template-columns: minmax(4rem, 5.5rem) minmax(0, 1fr); }
  .editorial .facts { grid-template-columns: auto minmax(0, 1fr); justify-content: start; }
  .editorial .facts dt { text-align: left; font-family: inherit; font-size: 0.8rem; letter-spacing: normal; text-transform: none; }
  .editorial .facts dd, .editorial .component-list, .editorial .related-row { justify-content: flex-start; }
  .editorial .compounds button { justify-content: flex-start; text-align: left; margin-right: 0; padding-inline: 0; }
  .editorial .compound-gloss { flex-basis: 100%; white-space: normal; }
  .editorial .section { text-transform: none; letter-spacing: normal; }
  @container (max-width: 30rem) { .editorial { grid-template-columns: 1fr; } }
</style>
