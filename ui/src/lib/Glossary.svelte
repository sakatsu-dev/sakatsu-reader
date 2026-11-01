<script lang="ts">
  // Category and alphabetical order stay fixed while one term expands in place.
  import { onMount, tick } from "svelte";
  import { TERM_KINDS, type GlossarySort, type Term } from "../model";
  import { glossary } from "./pages.svelte";
  import HighlightedText from "./HighlightedText.svelte";
  import TermDetail from "./TermDetail.svelte";
  import { glossaryAliasMatch, glossaryTermMatches } from "./glossary-flow.js";
  import { revealEntryHeading } from "./entry-scroll.js";
  import { CARD_CLICK_IGNORES, travelled } from "./card-click.js";

  let {
    allTerms,
    sort = $bindable("grouped"),
  }: {
    allTerms: () => Promise<Term[]>;
    sort?: GlossarySort;
  } = $props();

  let collection = $state<HTMLElement | null>(null);
  let rovingId = $state<string | null>(null);
  /** Where the current press landed, so a click can tell itself apart from a drag. See
      `card-click.js`; not `$state`, because nothing renders from it. */
  let pressedAt: { x: number; y: number } | null = null;

  const terms = $derived(glossary.terms);
  const needle = $derived(glossary.find.trim());
  const matches = $derived(
    needle === ""
      ? terms
      : terms.filter((term) => glossaryTermMatches(term, needle)),
  );
  const sections = $derived.by(() => {
    if (sort === "alpha") {
      const ordered = [...matches].sort((a, b) => a.label.localeCompare(b.label));
      return ordered.length ? [{ label: "", terms: ordered }] : [];
    }
    return TERM_KINDS.map(({ kind, label }) => ({
      label,
      terms: matches.filter((term) => term.kind === kind),
    })).filter((section) => section.terms.length > 0);
  });
  const ordered = $derived(sections.flatMap((section) => section.terms));

  function aliasMatch(term: Term) {
    return glossaryAliasMatch(term, needle);
  }

  // Keep the selected heading visible without changing category or alphabetical order.
  async function select(id: string, toggle = true) {
    const closing = toggle && glossary.selectedId === id;
    glossary.selectedId = closing ? null : id;
    rovingId = id;
    await tick();
    const item = collection?.querySelector<HTMLElement>(`[data-item="${CSS.escape(id)}"]`);
    if (!item || !collection) return;
    revealEntryHeading(collection, item.querySelector<HTMLElement>(".preview") ?? item);
  }

  async function followTerm(id: string) {
    if (!terms.some((term) => term.id === id)) return;
    // A relationship can leave the current name filter, but keeps the chosen browse order.
    if (!matches.some((term) => term.id === id)) glossary.find = "";
    await select(id, false);
    collection?.querySelector<HTMLElement>(`[data-term="${CSS.escape(id)}"]`)?.focus({ preventScroll: true });
  }

  /**
   * One click toggles an entry. Hover highlights its preview without opening or loading it.
   *
   * Clicking anywhere on a term used to open it, and that was removed because it made the whole
   * card a button and left no quiet place to select a summary from. The two guards are what let
   * the click come back without taking the selection away again: a press that lands on a control
   * belongs to that control, and a press that *travelled* was someone dragging across a summary
   * to select it. The whole `.card` answers while the term is shut, so a reader does not have to
   * learn which part of it to aim at — but never `.inline-detail`, because taking away the answer
   * somebody asked to read is not something a stray press should be able to do. See
   * `card-click.js` for why travel is the question rather than whether anything is selected.
   *
   * `select(id, false)` is deliberately not used here — that is the "open this other term" call
   * from within a detail, which must never toggle. This one toggles, because a second click on a
   * card the reader has already opened means close it.
   */
  function cardClick(event: MouseEvent, id: string) {
    if ((event.target as HTMLElement | null)?.closest(CARD_CLICK_IGNORES)) return;
    const from = pressedAt;
    pressedAt = null;
    if (travelled(from, { x: event.clientX, y: event.clientY })) return;
    // `preventScroll`: the card was just clicked, so it is on screen, and a focus that scrolls
    // here only ever scrolls something it should not — see `.app` in `App.svelte`.
    collection
      ?.querySelector<HTMLElement>(`[data-term="${CSS.escape(id)}"]`)
      ?.focus({ preventScroll: true });
    void select(id);
  }

  function move(event: KeyboardEvent, index: number) {
    let next = index;
    if (event.key === "ArrowDown") next = Math.min(ordered.length - 1, index + 1);
    else if (event.key === "ArrowUp") next = Math.max(0, index - 1);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = ordered.length - 1;
    else return;
    event.preventDefault();
    const target = ordered[next];
    if (!target) return;
    rovingId = target.id;
    collection?.querySelector<HTMLElement>(`[data-term="${CSS.escape(target.id)}"]`)?.focus();
  }

  $effect(() => {
    const visible = ordered;
    if (glossary.selectedId && !visible.some((term) => term.id === glossary.selectedId)) {
      glossary.selectedId = null;
    }
    if (!visible.some((term) => term.id === rovingId)) {
      rovingId = visible.find((term) => term.id === glossary.selectedId)?.id ?? visible[0]?.id ?? null;
    }
  });

  function collectionKey(event: KeyboardEvent) {
    if (event.key !== "Escape" || !glossary.selectedId) return;
    event.preventDefault();
    const id = glossary.selectedId;
    glossary.selectedId = null;
    rovingId = id;
    queueMicrotask(() => collection?.querySelector<HTMLElement>(`[data-term="${CSS.escape(id)}"]`)?.focus());
  }

  onMount(() => {
    if (!glossary.loaded) {
      glossary.loaded = true;
      glossary.loading = true;
      allTerms()
        .then((fetched) => (glossary.terms = fetched))
        .catch((thrown) => (glossary.error = String(thrown)))
        .finally(() => (glossary.loading = false));
    }
    queueMicrotask(() => {
      if (collection) collection.scrollTop = glossary.collectionScroll;
    });
  });
</script>

<svelte:window onkeydown={collectionKey} />

<section class="page glossary">
  <h2>Glossary</h2>
  <label class="filter-label" for="glossary-query">Find a glossary term</label>
  <div class="controls">
    <div class="bar">
      <input id="glossary-query" bind:value={glossary.find} placeholder="te form, transitive, keigo" aria-label="Find a glossary term" spellcheck="false" />
      {#if glossary.find}<button type="button" aria-label="Clear glossary filter" onclick={() => { glossary.find = ""; document.getElementById("glossary-query")?.focus(); }}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg></button>{/if}
    </div>
    <div class="sort" role="group" aria-label="Glossary order">
      <button type="button" aria-pressed={sort === "grouped"} onclick={() => (sort = "grouped")}>Grouped</button>
      <button type="button" aria-pressed={sort === "alpha"} onclick={() => (sort = "alpha")}>A–Z</button>
    </div>
    <span class="count">{matches.length}{needle ? ` of ${terms.length}` : ""}</span>
  </div>

  {#if glossary.error}
    <p class="state error">{glossary.error}</p>
  {:else if glossary.loading}
    <p class="state">Reading the glossary…</p>
  {:else if ordered.length === 0}
    <div class="state"><p>No term name or alias matches “{glossary.find.trim()}”.</p><button type="button" class="control" onclick={() => (glossary.find = "")}>Clear filter</button></div>
  {:else}
    <nav
      class="collection"
      data-cards="single"
      aria-label="Glossary terms"
      bind:this={collection}
      onscroll={() => { if (collection) glossary.collectionScroll = collection.scrollTop; }}
    >
      {#each sections as section}
        <section class="term-section">
          {#if section.label}<h3>{section.label} <span class="count">{section.terms.length}</span></h3>{/if}
          <ol class="terms">
            {#each section.terms as term (term.id)}
              {@const index = ordered.indexOf(term)}
              {@const alias = aliasMatch(term)}
              {@const expanded = term.id === glossary.selectedId}
              {@const detailId = `glossary-detail-${term.id}`}
              <li class="term" class:expanded data-item={term.id} data-focused={term.id === rovingId ? "" : undefined}>
                <!-- `.card` carries the border, the fill and the clipping; the list item carries
                     only the grid placement, and holds the card's hidden control beside it. -->
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <!-- Deliberately not a button and deliberately without a key handler: the hidden
                     control below carries `aria-expanded`, `aria-controls`, the roving tabindex
                     and the arrow keys, so this click offers no route that is not already
                     reachable without a pointer. A button here would take back the selection the
                     card exists to leave available. -->
                <div
                  class="card"
                  onmousedowncapture={(event) => { pressedAt = { x: event.clientX, y: event.clientY }; }}
                  onclick={(event) => cardClick(event, term.id)}
                >
                <div class="preview" class:current={expanded}>
                  <strong><HighlightedText text={term.label} {needle} /></strong>
                  <!-- The summary is not highlighted, because it is not searched. A mark here
                       would claim to say why this card is in the list and be wrong: the filter
                       reads names only, so the only honest marks are on the label and the alias
                       under it. See `glossary-flow.js`. -->
                  <span>{term.summary}</span>
                  {#if alias}<small>Matched “<HighlightedText text={alias} {needle} />”</small>{/if}
                </div>
                {#if expanded}
                  <section class="inline-detail" id={detailId} aria-label={`${term.label} detail`}>
                    <TermDetail embedded editorial similarTerms showSummary={false} {term} {terms} onselect={followTerm} />
                  </section>
                {/if}
                </div>
                <!-- The card's keyboard and screen-reader control, drawn as nothing (`theme.css`,
                     `.disclose`): the visible tab it used to be was removed on 2026-10-17. It
                     carries the roving tabindex, the arrow-key handler, and `data-term`, which is
                     what Escape focuses by; its keyboard focus is drawn on the card. -->
                <button
                  type="button"
                  class="disclose"
                  data-term={term.id}
                  aria-expanded={expanded}
                  aria-controls={detailId}
                  aria-label={`${expanded ? "Hide" : "Show"} details for ${term.label}`}
                  tabindex={term.id === rovingId ? 0 : -1}
                  onclick={() => select(term.id)}
                  onkeydown={(event) => move(event, index)}
                ></button>
              </li>
            {/each}
          </ol>
        </section>
      {/each}
    </nav>
  {/if}
</section>

<style>
  .glossary { container: glossary / inline-size; min-height: 0; width: 100%; --list-divider-color: color-mix(in srgb, var(--text-dim) 50%, var(--bg)); }
  .filter-label { color: var(--text-dim); font-size: 0.85rem; }
  .controls { display: flex; flex-wrap: wrap; align-items: center; gap: var(--gap-sm); }
  .bar { display: flex; flex: 1 1 16rem; min-width: 0; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface); }
  .bar:focus-within { outline: 1px solid var(--accent); outline-offset: 1px; }
  input { flex: 1; min-width: 0; width: 0; border: 0; outline: 0; padding: 0.45rem 0.7rem; background: none; color: var(--text); font: inherit; }
  input:hover:not(:disabled), input:focus:not(:disabled) { background: none; }
  input::placeholder { color: var(--text-faint); }
  .bar button { padding: 0.35rem 0.65rem; border: 0; background: none; color: var(--text-dim); font: inherit; }
  .bar button svg { display: block; width: 1em; height: 1em; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; }
  .sort { display: flex; gap: var(--gap-xs); }
  .sort button { padding: 0.3rem 0.4rem; border: 0; border-bottom: 2px solid transparent; border-radius: 0; background: none; color: var(--text-dim); font: inherit; font-size: 0.82rem; }
  .sort button[aria-pressed="true"] { color: var(--accent); border-bottom-color: var(--accent); }
  .count { color: var(--text-faint); font-size: 0.8rem; font-weight: normal; font-variant-numeric: tabular-nums; }
  .state { color: var(--text-dim); font-size: 0.85rem; }
  .error { color: var(--red); }
  .collection { flex: 1; min-height: 0; padding: 0 var(--gap-sm) var(--gap-sm) 0; overflow: auto; scrollbar-gutter: stable; }
  .term-section h3 { margin: var(--gap-lg) var(--gap) var(--gap-xs); color: var(--text); font-size: 0.95rem; font-weight: 600; }
  .term-section h3 .count { margin-left: var(--gap-xs); }
  .terms { margin: 0; padding: 0; list-style: none; display: grid; grid-template-columns: minmax(0, 1fr); grid-auto-rows: max-content; align-items: start; }
  .term { display: flex; flex-direction: column; min-width: 0; align-self: start; border-bottom: 1px solid var(--list-divider-color); }
  .term.expanded { background: var(--surface-raised); box-shadow: inset 2px 0 var(--accent); }
  .card { min-width: 0; background: none; }
  .preview { min-width: 0; padding: var(--gap) var(--gap); color: var(--text); transition: background-color var(--anim) var(--ease), box-shadow var(--anim) var(--ease); }
  .preview.current { background: var(--accent-wash); box-shadow: inset 2px 0 var(--accent); }
  @media (hover: hover) and (pointer: fine) {
    .term:not(.expanded) .card:hover .preview { background: var(--surface-hover); box-shadow: inset 2px 0 var(--accent-edge); }
  }
  .term:not(.expanded) .card { cursor: pointer; }
  .preview strong, .preview span, .preview small { display: block; overflow-wrap: anywhere; }
  .preview strong { font-size: 1rem; font-weight: 600; }
  .term.expanded .preview strong { font-size: 1.15rem; font-weight: 700; }
  .preview span { margin-top: 0.2rem; color: var(--text-dim); font-size: 0.85rem; line-height: 1.5; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; }
  .preview small { margin-top: 0.2rem; color: var(--text-faint); font-size: 0.8rem; }
  .inline-detail { min-width: 0; max-width: 65ch; padding: var(--gap) var(--gap) var(--gap-lg); }
  .inline-detail :global(.term-detail > .detail) { color: var(--text); font-size: 0.95rem; line-height: 1.65; }
</style>
