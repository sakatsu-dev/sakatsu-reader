<script lang="ts">
  // Every concept keeps a balanced preview in the browse collection. Only the selected preview
  // mounts teaching detail, so Cards and Rows are presentation choices rather than different
  // data/effect paths.
  import { onMount, tick } from "svelte";
  import { flip } from "svelte/animate";
  import { TERM_KINDS, type GlossarySort, type GrammarPattern, type Term } from "../model";
  import { glossary } from "./pages.svelte";
  import HighlightedText from "./HighlightedText.svelte";
  import TermDetail from "./TermDetail.svelte";
  import { glossaryAliasMatch, glossaryTermMatches } from "./glossary-flow.js";
  import { promoteSelected, revealHead } from "./promote.js";
  import { CARD_CLICK_IGNORES, travelled } from "./card-click.js";

  let {
    allTerms,
    patternsForTerm,
    focusBeforeOpen,
    sort = $bindable("grouped"),
  }: {
    allTerms: () => Promise<Term[]>;
    patternsForTerm: (id: string) => Promise<GrammarPattern[]>;
    /** Two clicks to open a term — focus, then open — rather than one. */
    focusBeforeOpen: boolean;
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
  /**
   * **Promoted within its heading, never out of it.**
   *
   * Reference lifts an opened result to the very top of one flat collection. Here the headings
   * are the page: you browse the glossary *by* category, and a term that left `PARTS OF SPEECH`
   * to sit above `CONCEPTS` would lose the one thing on screen saying what kind of thing it is.
   * So the lift happens inside the section — which is also the only arrangement the markup can
   * express, since each section renders its own `<ol>`.
   *
   * A–Z sorting is a single section with no label, so the same rule puts the opened term at the
   * top of the whole list there, with no special case.
   */
  const promote = (terms: Term[]) => promoteSelected(terms, glossary.selectedId, (term) => term.id);
  const sections = $derived.by(() => {
    if (sort === "alpha") {
      const ordered = [...matches].sort((a, b) => a.label.localeCompare(b.label));
      return ordered.length ? [{ label: "", terms: promote(ordered) }] : [];
    }
    return TERM_KINDS.map(({ kind, label }) => ({
      label,
      terms: promote(matches.filter((term) => term.kind === kind)),
    })).filter((section) => section.terms.length > 0);
  });
  const ordered = $derived(sections.flatMap((section) => section.terms));
  const nextSort = $derived<GlossarySort>(sort === "grouped" ? "alpha" : "grouped");

  function aliasMatch(term: Term) {
    return glossaryAliasMatch(term, needle);
  }

  /** The animation duration the app is set to — `Reference.svelte` reads it the same way, and why. */
  function motionMs() {
    const declared = getComputedStyle(document.documentElement).getPropertyValue("--anim");
    return Number.parseFloat(declared) || 0;
  }

  /**
   * Open or close a term, then put it where it can be read.
   *
   * **`tick()`, not `queueMicrotask`.** Svelte flushes its own effects in a microtask, so one
   * queued here races it — and both the promotion above and the `firstElementChild` test below
   * exist only after that flush.
   *
   * **A promoted term is scrolled to, not scrolled *at*.** `block: "nearest"` is wrong once a
   * term can be taller than the collection: with none of it showing, the nearest edge is its
   * *bottom*, so opening a long entry lands past the term just opened. See `traps.md`. The
   * anchor is the whole `term-section` rather than the term, because a term at the top of its
   * heading should arrive with that heading above it — it is half of what the term means here.
   */
  async function select(id: string, toggle = true) {
    const closing = toggle && glossary.selectedId === id;
    glossary.selectedId = closing ? null : id;
    rovingId = id;
    await tick();
    const item = collection?.querySelector<HTMLElement>(`[data-item="${CSS.escape(id)}"]`);
    if (!item || !collection) return;
    const section = item.parentElement?.parentElement;
    if (item.parentElement?.firstElementChild === item && section instanceof HTMLElement) {
      revealHead(collection, section);
    } else {
      item.scrollIntoView({ block: "nearest" });
    }
  }

  /**
   * The card as a pointer target, in both models.
   *
   * **The card is the only thing a pointer can click.** Until 2026-10-17 a pull tab hung under each
   * card and was the only pointer route in under the `hover` model. It took a row under every card
   * and crowded the page, so it went, and its click came here: one click opens or closes under
   * `hover`, exactly as one click on the tab did. Under `focus` nothing changed. Every click also
   * focuses the card's hidden control, as clicking the tab used to, so the arrow keys carry on from
   * where the pointer left off.
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
    if (!focusBeforeOpen || rovingId === id) {
      void select(id);
      return;
    }
    rovingId = id;
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
  <div class="controls">
    <div class="bar">
      <input bind:value={glossary.find} placeholder="Filter — te form, transitive, keigo" aria-label="Filter the glossary" spellcheck="false" />
      <button type="button" class="sort" onclick={() => (sort = nextSort)} aria-label={nextSort === "alpha" ? "Sort A–Z" : "Group by kind"}>
        {nextSort === "alpha" ? "A–Z" : "Grouped"}
      </button>
    </div>
    <span class="count">{matches.length}{needle ? ` of ${terms.length}` : ""}</span>
  </div>

  {#if glossary.error}
    <p class="state error">{glossary.error}</p>
  {:else if glossary.loading}
    <p class="state">Reading the glossary…</p>
  {:else if ordered.length === 0}
    <p class="state">No term matching <strong>{glossary.find.trim()}</strong>.</p>
  {:else}
    <nav
      class="collection"
      data-cards={focusBeforeOpen ? "focus" : "hover"}
      aria-label="Glossary terms"
      bind:this={collection}
      onscroll={() => { if (collection) glossary.collectionScroll = collection.scrollTop; }}
    >
      {#each sections as section}
        <section class="term-section">
          {#if section.label}<h3>{section.label}</h3>{/if}
          <ol class="terms">
            {#each section.terms as term (term.id)}
              {@const index = ordered.indexOf(term)}
              {@const alias = aliasMatch(term)}
              {@const expanded = term.id === glossary.selectedId}
              {@const detailId = `glossary-detail-${term.id}`}
              <!-- Keyed above, so a promotion **moves** this node rather than rebuilding it:
                   the loaded detail and the focus survive the lift. `flip` is what makes the
                   move legible rather than a teleport. -->
              <li class="term" class:expanded data-item={term.id} data-focused={term.id === rovingId ? "" : undefined} animate:flip={{ duration: motionMs() }}>
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
                    <TermDetail embedded {term} {terms} {patternsForTerm} onselect={(id) => select(id, false)} />
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
  /* **Its own measure, centred.** The reading page fills the window by default (2026-10-23), and a
     list of names with one-line summaries does not want to: at 1920px each card ran 1,814px wide for
     a sentence of sixty characters. 64rem is the same measure Settings takes, so the two text pages
     line up when you move between them. Narrower windows never reach it. */
  .glossary { container: glossary / inline-size; min-height: 0; width: 100%; max-width: 64rem; margin-inline: auto; }
  .controls { display: flex; align-items: center; gap: var(--gap); }
  .bar { display: flex; flex: 1 1 18rem; min-width: 0; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface); overflow: hidden; }
  .bar:has(input:focus), .bar:hover { background: var(--surface-sunken); }
  input { flex: 1; min-width: 0; border: 0; outline: 0; padding: 0.45rem 0.7rem; background: none; color: var(--text); font: inherit; }
  input:hover:not(:disabled), input:focus:not(:disabled) { background: none; }
  input::placeholder { color: var(--text-faint); }
  .sort { width: 4.4rem; border: 0; border-left: 1px solid var(--border); border-radius: 0; background: none; color: var(--text-dim); font: inherit; font-size: 0.78rem; }
  .sort:hover { color: var(--text); }
  .count { color: var(--text-faint); font-size: 0.75rem; font-variant-numeric: tabular-nums; }
  .state { color: var(--text-faint); font-size: 0.85rem; }
  .error { color: var(--red); }
  .collection { flex: 1; min-height: 0; padding: 0 var(--gap-sm) var(--gap-sm); overflow: auto; scrollbar-gutter: stable; }
  .term-section h3 { margin: var(--gap) var(--gap-sm) var(--gap-xs); color: var(--accent); font-size: 0.68rem; letter-spacing: 0.08em; text-transform: uppercase; }
  /* **One column, at every width.** The glossary was a Cards/Rows choice like Reference; it is a
     list of names read top to bottom under category headings, and the grid only ever bought
     density at the cost of a summary's measure. What is left is what the narrow case always drew.
     The capped multi-column track went with it, and so did the reason it existed — a one-concept
     group can no longer stretch into a disguised row, because every group is rows. */
  .terms { margin: 0; padding: 0; list-style: none; display: grid; grid-template-columns: minmax(0, 1fr); grid-auto-rows: max-content; align-items: start; gap: var(--gap-sm); }
  /* The list item is placement only; `.card` is the card. Its other child is the card's hidden
     control, which is out of the flow and reserves no space. */
  .term { display: flex; flex-direction: column; min-width: 0; align-self: start; }
  .card { min-width: 0; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); overflow: hidden; }
  .term.expanded { grid-column: 1 / -1; }
  .term.expanded .card { border-color: var(--accent-edge); }
  /* A `<div>` now, so the button resets and the pointer cursor that came with being one are
     gone — as is the hover fill, which promised a click the card no longer takes. */
  .preview { min-width: 0; padding: 0.62rem var(--gap); color: var(--text); }
  .preview.current { background: var(--accent-wash); box-shadow: inset 2px 0 var(--accent); }
  /* The lift that makes the first of two clicks mean something — quieter than `.current`, because
     a focused term is one the reader is standing on rather than one they have opened, and both
     can be on screen at once. Reference draws this the same way; see the note there. */
  .collection[data-cards="focus"] .term[data-focused]:not(.expanded) .preview { background: var(--surface-hover); }
  /* A shut card answers a click in both models — picking or opening it — so it shows the pointer
     in both. See the matching rule in Reference. */
  .term:not(.expanded) .card { cursor: pointer; }
  .preview strong, .preview span, .preview small { display: block; overflow-wrap: anywhere; }
  .preview strong { font-size: 0.9rem; }
  .preview span { margin-top: 0.12rem; color: var(--text-dim); font-size: 0.78rem; line-height: 1.35; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; }
  .preview small { margin-top: 0.2rem; color: var(--text-faint); font-size: 0.7rem; }
  .inline-detail { min-width: 0; padding: var(--gap-lg); border-top: 1px solid var(--border); background: var(--surface-sunken); }

  @container glossary (max-width: 41.999rem) {
    .inline-detail { padding: var(--gap); }
  }
</style>
