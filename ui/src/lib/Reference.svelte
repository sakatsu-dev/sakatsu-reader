<script lang="ts">
  // Stable rows keep search order intact. Only the selected entry mounts heavy detail.
  import { onDestroy, tick } from "svelte";
  import type {
    Found,
    GrammarPattern,
    KanjiView,
    ParticleView,
    TagExplanation,
    Term,
    WordHit,
    WordView,
  } from "../model";
  import { delayedWarmup } from "./intent-warmup.js";
  import { search, Tab } from "./pages.svelte";
  import { tabShortcut } from "./tab-keys.js";
  import {
    FACETS,
    automaticSelection,
    composeReferenceResults,
    facetCounts,
    facetResults,
    sensesForKinds,
    wordKinds,
    WORD_KIND_LABELS,
  } from "./reference-results.js";
  import { errorMessage } from "./save-notice.js";
  import { revealEntryHeading } from "./entry-scroll.js";
  import { CARD_CLICK_IGNORES, travelled } from "./card-click.js";
  import { openGlossary } from "./glossary-navigation";
  import { surfaceWorthShowing } from "./kana.js";
  import Character from "./Character.svelte";
  import WordCharacters from "./WordCharacters.svelte";
  import ExampleText from "./ExampleText.svelte";
  import TabStrip from "./TabStrip.svelte";
  import TermDetail from "./TermDetail.svelte";
  import WordBody from "./WordBody.svelte";

  import CopyButton from "./CopyButton.svelte";
  import PronounceButton from "./PronounceButton.svelte";
  import { pronunciation } from "./pronunciation.svelte";

  let {
    searchWords,
    prefetchReference,
    searchGrammar,
    lookupKanji,
    buildGlossIndex,
    showSource,
    showFrequencySource,
    explainTags,
    searchTerms,
    searchParticles,
    allTerms,
    patternsForTerm,
    pronounce,
    audioBusy,
    audioNotice,
  }: {
    searchWords: (query: string) => Promise<Found>;
    prefetchReference: (query: string) => Promise<unknown>;
    searchGrammar: (query: string) => Promise<GrammarPattern[]>;
    lookupKanji: (character: string) => Promise<KanjiView | null>;
    buildGlossIndex: () => Promise<string>;
    showSource: boolean;
    showFrequencySource: boolean;
    explainTags: (codes: string[]) => Promise<Record<string, TagExplanation>>;
    searchTerms: (query: string) => Promise<Term[]>;
    searchParticles: (query: string) => Promise<ParticleView[]>;
    allTerms: () => Promise<Term[]>;
    patternsForTerm: (id: string) => Promise<GrammarPattern[]>;
    pronounce: (expression: string, reading: string, key: string) => void;
    audioBusy: string | null;
    audioNotice: { key: string; message: string } | null;
  } = $props();

  const FACET_LABELS = { all: "All", words: "Words", grammar: "Grammar patterns", kanji: "Kanji", terms: "Glossary terms" } as const;
  const POS_LABELS = WORD_KIND_LABELS;
  let termIndex = $state<Term[]>([]);
  let loadingTermIndex = $state(false);
  const tab = $derived(search.current);
  const results = $derived(composeReferenceResults({
    query: tab.asked,
    words: tab.words,
    patterns: tab.patterns,
    particles: tab.particles,
    terms: tab.terms,
    kanji: tab.kanji,
  }));
  const counts = $derived(facetCounts(results));
  const faceted = $derived(facetResults(results, tab.facet));
  /**
   * Whether the parts-of-speech row is on screen, used by the row itself *and* by the rule below.
   *
   * One derivation rather than the same three clauses written twice: a rule that narrows results
   * on a condition the control does not share is a filter the learner cannot see or undo.
   */
  const showsWordKinds = $derived(
    (tab.facet === "all" || tab.facet === "words") && tab.words.length > 0,
  );
  const narrowingWords = $derived(showsWordKinds && tab.wordKinds.length > 0);
  const visible = $derived(
    faceted.filter((result: any) =>
      result.type === "word"
        // **Asking for a part of speech is asking for words.** Non-word answers used to be exempt
        // — `result.type !== "word"` passed every one of them — so filtering to Expression on the
        // All facet returned nine grammar patterns and two words, and the patterns dominated a
        // list nobody had asked them into.
        ? tab.wordKinds.length === 0
          || wordKinds(result.value).some((kind) => tab.wordKinds.includes(kind))
        // Only where the control exists. On the Grammar or Kanji facet the row is not drawn, so a
        // selection left over from the All facet would empty the page with nothing on screen to
        // clear it.
        : !narrowingWords,
    ),
  );
  const arranged = $derived(visible);
  const selected = $derived(visible.find((result: any) => result.key === tab.selectedKey) ?? null);
  const termNames = $derived(new Map(termIndex.map((term) => [term.id, term.label])));
  const pronunciationKey = (word: WordView) => `reference:${word.headword}\u0000${word.reading ?? ""}`;
  const wordCounts = $derived(
    POS_LABELS.map((label) => ({
      label,
      count: tab.words.filter((word) => wordKinds(word).includes(label)).length,
    })),
  );

  let box = $state<HTMLInputElement | null>(null);
  let collection = $state<HTMLElement | null>(null);
  let rovingKey = $state<string | null>(null);
  /** Where the current press landed, so a click can tell itself apart from a drag. Not `$state`:
      nothing renders from it, and it is written on every mousedown over the collection. */
  let pressedAt: { x: number; y: number } | null = null;

  // **Open, because the card's own chevron was the disclosure.** Expanding 道 used to answer
  // with three more things to open — 20 more senses, 1 more example, 16 name readings — so
  // reading one entry took four clicks. `WordBody`'s Analysis branch already draws its examples
  // as `<details open>` and says why: open is a default, not a decision. Reference never took it.
  let meaningsExpanded = $state(true);
  let examplesExpanded = $state(true);
  let indexing = $state(false);
  let indexMessage = $state("");
  let focused: Tab | null = null;
  let composing = false;

  const warmup = delayedWarmup<string>(300, (query) => {
    void prefetchReference(query).catch(() => {});
  });
  onDestroy(() => warmup.cancel());

  function prepare(value: string) {
    const query = value.trim();
    if (query) warmup.schedule(query);
    else warmup.cancel();
  }

  $effect(() => {
    const front = search.current;
    if (front.pending && !front.searching) void run(front);
  });

  $effect(() => {
    const front = search.current;
    const blank = !front.query && !front.asked && !front.pending && !front.searching;
    if (blank && focused !== front) {
      focused = front;
      box?.focus();
    }
    queueMicrotask(() => {
      if (collection) collection.scrollTop = front.collectionScroll;
    });
  });

  $effect(() => {
    // Keyboard positions follow the same stable list that is drawn.
    const shown = arranged;
    if (shown.some((result: any) => result.key === rovingKey)) return;
    rovingKey = shown.find((result: any) => result.key === tab.selectedKey)?.key
      ?? shown[0]?.key
      ?? null;
  });

  // Each card starts open and remembers being collapsed only while it is the open one. The folds
  // survive as *collapse* controls, which is what a 21-sense entry needs — what went is having
  // to open something inside a thing you just opened.
  $effect(() => {
    void tab.selectedKey;
    meaningsExpanded = true;
    examplesExpanded = true;
  });

  /**
   * Ask which of the words on screen have a recording.
   *
   * **One request for the page, not one per card.** `want` batches everything asked in a tick
   * and skips anything already answered, so this is safe to re-run on every change to the
   * visible set. It is also the one piece of detail work that had to stop being lazy: the speak
   * control now lives on the front of a card, and `pronunciation.has` is false until asked, so
   * without this no card would ever show one. Tags, diagrams and related kanji stay behind the
   * chevron, which is where the cost actually is.
   */
  $effect(() => {
    for (const result of visible) {
      if (result.type !== "word") continue;
      const word = result.value as WordHit;
      pronunciation.want(word.headword, word.reading ?? "");
    }
  });

  /// The glossary index, fetched the first time a result needs one to name a term.
  ///
  /// Only for a selected term or a grammar pattern that links to terms — the table is small but
  /// nothing else on the page reads it, so an unopened result should not pay for it.
  $effect(() => {
    const needsTerms = selected?.type === "term"
      || (selected?.type === "grammar" && selected.value.term_ids.length > 0);
    if (!needsTerms || termIndex.length > 0 || loadingTermIndex) return;
    loadingTermIndex = true;
    allTerms()
      .then((found) => (termIndex = found))
      .catch(() => {})
      .finally(() => (loadingTermIndex = false));
  });

  /// Ask a question, from anywhere on the page that can produce one.
  ///
  /// The same question again is not a new search: a kanji's related characters and a word's
  /// forms all route through here, and re-running the one already on screen would throw the
  /// selection and scroll away to arrive at the same answer.
  function ask(text: string) {
    warmup.cancel();
    const wanted = text.trim();
    if (!wanted) return;
    if (tab.searching && tab.requested === wanted) return;
    if (!tab.error && tab.asked && tab.asked.toLocaleLowerCase() === wanted.toLocaleLowerCase()) {
      // Returning to the successful query cancels a replacement without losing its view.
      tab.generation += 1;
      tab.searching = false;
      tab.requested = "";
      return;
    }
    tab.query = wanted;
    run(tab);
  }

  /// Empty the box and the answer, and put the cursor back where the next question goes.
  function clear() {
    warmup.cancel();
    tab.query = "";
    run(tab);
    box?.focus();
  }

  /// Search, into the tab that asked.
  ///
  /// Takes the tab rather than reading `search.current` throughout: a search is several
  /// awaits long, and switching tabs while one is in flight would otherwise land the reply
  /// on whichever tab happened to be in front when it came back.
  async function run(target: Tab = search.current, retryQuery?: string) {
    // A restored tab's question is in `pending`, not in the box — the box is empty after a
    // restart. Every other search reads what was typed.
    const restoring = target.pending !== "";
    const text = (retryQuery ?? (restoring ? target.pending : target.query)).trim();

    // Cleared before the first `await`, or the effect watching it would start this again on
    // the next tick — and before the empty-query branch below, which returns without reaching
    // `reset()`'s copy of this when there was nothing to ask in the first place.
    // `restoring` is remembered rather than re-read because clearing `pending` here would
    // otherwise make it false by the time the failure branch below asks — and that branch is
    // the only thing keeping a restored tab's question when its search fails.
    target.pending = "";

    if (!text) {
      target.reset();
      return;
    }

    const mine = (target.generation += 1);
    target.searching = true;
    target.requested = text;
    target.failedQuery = "";
    target.error = "";
    try {
      // Together: the grammar packs are in memory and answer instantly, so making them
      // wait behind a dictionary search would only make the page feel slower than it is.
      // The kanji table is asked only for a query that *is* one Han character, which is the
      // only query a character result answers.
      const oneCharacter = /^\p{Script=Han}$/u.test(text);
      const [found, grammar, termHits, particle, character] = await Promise.all([
        searchWords(text),
        searchGrammar(text),
        searchTerms(text),
        searchParticles(text),
        oneCharacter ? lookupKanji(text) : Promise.resolve(null),
      ]);
      if (mine !== target.generation) return;
      target.words = found.words;
      target.patterns = grammar;
      target.terms = termHits;
      target.particles = particle;
      target.kanji = character;
      target.japanese = found.japanese;
      target.rankedBy = found.ranked_by;
      target.needsIndex = found.needs_gloss_index;
      target.capped = found.capped;
      target.asked = text;
      // A new answer is a new question: the facet, the parts-of-speech narrowing and the
      // scroll all belonged to the previous one.
      target.facet = "all";
      target.wordKinds = [];
      target.filtersOpen = false;
      target.character = null;
      target.collectionScroll = 0;
      const composed = composeReferenceResults({
        query: text,
        words: found.words,
        patterns: grammar,
        particles: particle,
        terms: termHits,
        kanji: character,
      });
      target.selectedKey = automaticSelection(composed);
    } catch (thrown) {
      if (mine !== target.generation) return;
      target.error = errorMessage(thrown);
      target.failedQuery = text;
      // A restored search that failed hands its question back to the box.
      //
      // Not politeness — survival. `pending` has been cleared and `asked` is only assigned on
      // success, so the tab is now holding nothing at all: `label` would be empty and
      // `stored()` drops unlabelled tabs. A launch where the dictionaries have not been
      // installed yet would quietly delete every saved tab, and the second launch would find
      // none to restore. In the box the question is both kept and retryable.
      if (restoring) target.query = text;
    } finally {
      if (mine === target.generation) target.searching = false;
    }
  }

  // Reveal the heading, never the bottom of an entry taller than the viewport.
  async function selectResult(key: string) {
    const closing = tab.selectedKey === key;
    tab.selectedKey = closing ? null : key;
    tab.character = null;
    rovingKey = key;
    await tick();
    const item = collection?.querySelector<HTMLElement>(`[data-item="${CSS.escape(key)}"]`);
    if (!item || !collection) return;
    revealEntryHeading(collection, item.querySelector<HTMLElement>(".preview") ?? item);
  }

  /**
   * One click toggles an entry. Hover highlights its preview without opening or loading it.
   *
   * **The card is content — selectable, copyable, and only conditionally a click target.** That
   * sentence is why the whole-card button was removed, and the two guards below are what let a
   * click come back without taking it away: a press that lands on a control belongs to that
   * control, and a press that *travelled* was someone dragging across a definition to select it,
   * not asking for the entry. Without them, copying a headword opens the card underneath it.
   *
   * The travel test replaced a `getSelection()` check that looked equivalent and was not — see
   * `card-click.js`. A double-click selects a word without moving the pointer, so that check
   * swallowed the second click of every double-click and left the card unresponsive until the
   * selection cleared.
   *
   * **The whole card while it is shut; never the answer once it is open.** A collapsed card is one
   * target and a reader should not have to learn which part of it answers — that much was tried
   * the other way and scrapped. But an open card's detail is the thing they asked to read, and a
   * stray press in the middle of it taking the answer away is the same mistake as making the card
   * a button was. So `.inline-detail` is excluded, and the preview is what closes an open card.
   *
   * Every click also focuses the card's hidden control (`.disclose`), as clicking the tab used to.
   * That is what keeps the arrow keys working from where the pointer left off — and it is
   * deliberately a programmatic focus after a mouse press, which does not match `:focus-visible`,
   * so no ring appears for a reader who is not using the keyboard.
   */
  function cardClick(event: MouseEvent, key: string) {
    if ((event.target as HTMLElement | null)?.closest(CARD_CLICK_IGNORES)) return;
    const from = pressedAt;
    pressedAt = null;
    if (travelled(from, { x: event.clientX, y: event.clientY })) return;
    // `preventScroll`, as in Glossary's: the clicked card is already on screen.
    collection
      ?.querySelector<HTMLElement>(`[data-result="${CSS.escape(key)}"]`)
      ?.focus({ preventScroll: true });
    void selectResult(key);
  }

  // Arrow positions and rendered rows use the same list.
  function moveIndex(event: KeyboardEvent, at: number) {
    let next = at;
    if (event.key === "ArrowDown") next = Math.min(arranged.length - 1, at + 1);
    else if (event.key === "ArrowUp") next = Math.max(0, at - 1);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = arranged.length - 1;
    else return;
    event.preventDefault();
    const result = arranged[next];
    if (result) {
      rovingKey = result.key;
      collection?.querySelector<HTMLElement>(`[data-result="${CSS.escape(result.key)}"]`)?.focus();
    }
  }

  function collectionKey(event: KeyboardEvent) {
    if (event.key !== "Escape" || !tab.selectedKey) return;
    event.preventDefault();
    const key = tab.selectedKey;
    tab.selectedKey = null;
    rovingKey = key;
    // `queueMicrotask` is still enough here, and that was checked rather than assumed: the each
    // block is keyed, so a collapsing entry is retained and `querySelector`
    // hands back the same element either side of the flush. Focusing it is what scrolls it back
    // into view. Nothing here reads a position, which is the only thing `tick()` buys.
    queueMicrotask(() => collection?.querySelector<HTMLElement>(`[data-result="${CSS.escape(key)}"]`)?.focus());
  }

  function toggleKind(label: string) {
    tab.wordKinds = tab.wordKinds.includes(label)
      ? tab.wordKinds.filter((kind) => kind !== label)
      : [...tab.wordKinds, label];
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape" && tab.selectedKey) {
      collectionKey(event);
      return;
    }
    const command = tabShortcut(event);
    if (!command) return;
    event.preventDefault();
    if (command.kind === "new") search.open();
    else if (command.kind === "close") search.close(search.active);
    else if (command.kind === "reopen") search.reopen();
    else if (command.kind === "next") search.step(1);
    else if (command.kind === "previous") search.step(-1);
    else if (command.kind === "jump") search.jump(command.index ?? 0);
  }

  async function buildIndex() {
    const target = tab;
    indexing = true;
    indexMessage = "";
    try {
      indexMessage = await buildGlossIndex();
      target.needsIndex = false;
      if (target.asked) await run(target, target.asked);
    } catch (thrown) {
      indexMessage = String(thrown);
    } finally {
      indexing = false;
    }
  }

  function resultIdentity(result: any) {
    if (result.type === "word") return result.value.headword;
    if (result.type === "grammar") return result.value.pattern;
    if (result.type === "particle") return result.value.surface;
    if (result.type === "kanji") return result.value.character;
    return result.value.label;
  }

  function resultKind(result: any) {
    // **Every kind, not the leading one.** A Japanese word is often several parts of speech
    // depending on the nuance — 道中 is a noun and an adverb — and naming only the first was
    // wrong twice over: it badged NOUNS while the learner filtered by Adverbs, and it hid the
    // reason the category counts overlap and cannot sum to the number of results.
    if (result.type === "word") {
      const word = result.value as WordHit;
      const personal = (tags: string[]) => tags.some((tag) => ["person", "surname", "given"].includes(tag));
      // Structured name evidence only: gender tags also occur on ordinary dictionary senses.
      if (word.inline_retrieved.kind !== "component" && word.inline.length > 0 && word.inline.every((sense) => personal(sense.tags))) return "Personal name";
      if (word.name?.leads && word.name.retrieved.kind !== "component") return personal(word.name.tags) ? "Personal name" : "Name";
      return `Word · ${wordKinds(word).join(" · ")}`;
    }
    // Keep the result type and level readable while scanning a mixed answer.
    if (result.type === "grammar") {
      return result.value.jlpt ? `Grammar pattern · ${result.value.jlpt}` : "Grammar pattern";
    }
    if (result.type === "particle") return "Particle";
    if (result.type === "kanji") return "Kanji";
    return "Glossary term";
  }

  /**
   * The lines under a result's name, and for a word the *numbered* senses they are.
   *
   * A number rather than plain text because two dim lines said nothing about what they were.
   * The expanded detail hangs the same faint ordinal beside each gloss, so a preview now reads
   * as the truncation of one list and the numbers carry over when the card opens.
   *
   * **The sense's own place in the word, not its place in this preview.** Previews follow the
   * parts-of-speech filter, so 道中 under Adverb shows its *second* meaning and has to say 2.
   * Only words are numbered: a grammar pattern's explanation or a kanji's meanings are one
   * summary line, and numbering them would claim a list that is not there.
   */
  function resultScents(result: any): { n: number | null; text: string }[] {
    if (result.type === "word") {
      // **The senses that earned the result, not the first two it happens to have.** Filtering to
      // Adverb used to list 道中 correctly and then show its noun glosses. Still at most two
      // lines — the research makes a cheap preview a property of the design; this changes which
      // two, never how many, and a filtered card showing one line is the filter being honest.
      const word = result.value;
      const senses = sensesForKinds(word, tab.wordKinds).slice(0, 2);
      if (senses.length > 0) {
        return senses.map((sense: any) => ({
          n: word.inline.indexOf(sense) + 1,
          text: sense.glosses.join("; "),
        }));
      }
      return [{ n: null, text: word.name?.glosses?.join("; ") ?? "No definition" }];
    }
    if (result.type === "grammar") return [{ n: null, text: result.value.explanation }];
    if (result.type === "particle") return [{ n: null, text: result.value.note || result.value.senses[0]?.gloss || "Particle readings" }];
    if (result.type === "kanji") return [{ n: null, text: result.value.meanings.join("; ") || `${result.value.strokes} strokes` }];
    return [{ n: null, text: result.value.summary }];
  }
</script>

<svelte:window onkeydown={onKey} />

<section class="page reference">
  <h2>Reference</h2>
  <div class="search-tools" role="region" aria-label="Reference search controls">
  <form class="search-field" onsubmit={(event) => { event.preventDefault(); if (!composing) ask(tab.query); }}>
    <label for="reference-query">Search Reference</label>
    <div class="bar" class:multiple={search.tabs.length > 1}>
      <input id="reference-query" bind:this={box} bind:value={tab.query} oninput={(event) => prepare(event.currentTarget.value)} oncompositionstart={() => (composing = true)} oncompositionend={() => (composing = false)} onkeydown={(event) => { if (event.key === "Enter" && (event.isComposing || composing || event.keyCode === 229)) event.preventDefault(); }} lang="ja" spellcheck="false" placeholder="Japanese words or English meanings" />
      {#if tab.query || tab.asked || tab.searching || tab.error}<button type="button" class="clear" aria-label="Clear search" onclick={clear}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg></button>{/if}
      <button type="submit" aria-label="Search" title="Search" disabled={!tab.query.trim()}><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.5" cy="10.5" r="6.5" /><path d="m15.5 15.5 5 5" /></svg></button>
    </div>
    <TabStrip />
  </form>

  <div class="search-status" aria-live="polite">
    {#if tab.searching}<p>Searching for “{tab.requested}”…</p>{/if}
    {#if tab.asked}<p class="result-query">Results for “{tab.asked}”</p>{/if}
    {#if tab.query.trim() && tab.query.trim() !== tab.asked && !tab.searching}<p class="draft-hint">Press Enter to search</p>{/if}
  </div>
  {#if tab.error}
    <div class="error" role="alert"><p>Could not search for “{tab.failedQuery}”: {tab.error}</p><button type="button" class="control" onclick={() => run(tab, tab.failedQuery)}>Retry search</button></div>
  {/if}
    {#if tab.asked && !tab.needsIndex && results.length > 0}
    <div class="filters">
    <div class="facets" role="radiogroup" aria-label="Result types">
      {#each FACETS as facet}
        <button type="button" role="radio" class:current={tab.facet === facet} aria-checked={tab.facet === facet} onclick={() => (tab.facet = facet)}>
          {FACET_LABELS[facet]} <span>{counts[facet]}</span>
        </button>
      {/each}
    </div>
    {#if showsWordKinds}
      <div class="word-filter-bar">
        <details bind:open={tab.filtersOpen}>
          <summary>Word filters{#if tab.wordKinds.length} ({tab.wordKinds.length}){/if}</summary>
          <div class="pos" role="group" aria-label="Parts of speech">
            {#each wordCounts as option}
              {@const on = tab.wordKinds.includes(option.label)}
              <button type="button" class:current={on} aria-pressed={on} disabled={option.count === 0} onclick={() => toggleKind(option.label)}>{#if on}<svg class="tick" viewBox="0 0 24 24" aria-hidden="true"><path d="m5 12 4 4L19 6" /></svg>{/if}{option.label} <span>{option.count}</span></button>
            {/each}
          </div>
        </details>
        {#if tab.wordKinds.length > 0}<p class="active-filters">Words only: {tab.wordKinds.join(", ")} <button type="button" onclick={() => (tab.wordKinds = [])}>Clear word filters</button></p>{/if}
      </div>
    {/if}
    <!-- Inside the frame, because it describes the result set exactly as the counts above it do.
         As a bare paragraph between the filters and the results it was one more loose band. -->
    {#if showFrequencySource && tab.rankedBy && (tab.facet === "all" || tab.facet === "words")}
      <p class="ranking">Sorted most common first by {tab.rankedBy}.</p>
    {/if}
    <!-- **Not gated on `showFrequencySource`.** That setting is an opt-in for provenance prose;
         this is the page admitting it held something back, which is not optional. No number,
         because the count available cheaply is of dictionary entries and several spellings of one
         word are one card — see `Found.capped`. -->
    {#if tab.capped && (tab.facet === "all" || tab.facet === "words")}
      <p class="capped">More words matched than are shown.</p>
    {/if}
    </div>
    {/if}
  </div>
  {#if !tab.asked}
    {#if !tab.searching && !tab.error}<div class="welcome">Also finds grammar patterns, particles and glossary terms. Conjugated forms work too.</div>{/if}
  {:else if tab.needsIndex}
    <div class="notice">
      <p>English meaning search needs a one-time local dictionary index.</p>
      <button type="button" class="control" onclick={buildIndex} disabled={indexing}>{indexing ? "Building…" : "Build index"}</button>
      {#if indexMessage}<small>{indexMessage}</small>{/if}
    </div>
  {:else if results.length === 0}
    <div class="welcome">Nothing matching <strong>{tab.asked}</strong>.</div>
  {:else}


    {#if visible.length === 0}
      <div class="welcome"><p>No results remain under these filters.</p><button type="button" class="control" onclick={() => { tab.facet = "all"; tab.wordKinds = []; }}>Clear filters</button></div>
    {:else}
      <ol
        class="result-collection"
        data-cards="single"
        aria-label="Reference results"
        bind:this={collection}
        onscroll={() => { if (collection) tab.collectionScroll = collection.scrollTop; }}
      >
        {#each arranged as result, index (result.key)}
          {@const expanded = result.key === tab.selectedKey}
          {@const detailId = `reference-detail-${index}`}
          {@const scents = resultScents(result)}
          <li class="result" class:expanded data-item={result.key} data-focused={result.key === rovingKey ? "" : undefined}>
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <!-- Deliberately not a button and deliberately without a key handler: the hidden
                 control below carries `aria-expanded`, `aria-controls`, the roving tabindex and the
                 arrow keys, so every route this click offers is already reachable without a
                 pointer. Making the card focusable would put a second, redundant stop in front of
                 that control on every result, and making it a button would take back the
                 selection this card exists to let the reader copy. -->
            <div
              class="card"
              onmousedowncapture={(event) => { pressedAt = { x: event.clientX, y: event.clientY }; }}
              onclick={(event) => cardClick(event, result.key)}
            >
            <div class="preview" class:current={expanded}>
              <!-- A column, so the rule under the word can span the card's whole width. It used
                   to sit inside a left-hand `.summary` beside a right-hand gutter, which made
                   its right inset the badge's width and its left inset the card padding — a line
                   that looked hung rather than drawn. -->
              <div class="head">
                {#if result.type === "word"}
                  {@const front = result.value as WordHit}
                  {@const frontKey = pronunciationKey(front)}
                  <!-- The word wearing its two actions, rather than keeping them behind the
                       chevron. Copying a headword and hearing it are things a learner wants
                       *while scanning*, and a control that has to be opened first is a control
                       that gets skipped. -->
                  <div class="word-line">
                    <CopyButton word={front.headword} />
                    <!-- **The word and how it sounds are one thing, and the row has to say so.**
                         Everything here used to sit at a single `--gap-xs`, so one spacing served
                         three different relationships — an action against its object, two halves
                         of one identity, and another action — and the row read as four loose
                         items. The pair is a real element now, tight inside and standing apart
                         from the controls, which is the grouping stated rather than implied. -->
                    <span class="identity-pair">
                      <span class="identity" lang="ja">{front.headword}</span>
                      {#if front.reading && front.reading !== front.headword}<span class="reading" lang="ja">{front.reading}</span>{/if}
                    </span>
                    {#if pronunciation.has(front.headword, front.reading ?? "")}
                      <PronounceButton word={front.headword} onplay={() => pronounce(front.headword, front.reading ?? "", frontKey)} busy={audioBusy === frontKey} message={audioNotice?.key === frontKey ? audioNotice.message : ""} />
                    {/if}
                    <!-- **What was typed, when it is not what was answered.** 走った resolves to 走る
                         and the card used to say nothing about the difference — which on a page for
                         learners is the lesson, not plumbing. The same chip and the same rule the
                         selected-word header uses; `.flag.base` is documented in `theme.css` as the
                         flag for "the surface a headword was met as", which is exactly this.
                         `.word-line` wraps, so this takes a second line rather than overflowing. -->
                    {#if surfaceWorthShowing(front.headword, front.surface)}
                      <span class="flag base">as written <span lang="ja">{front.surface}</span></span>
                    {/if}
                  </div>
                {:else}
                  <span class="identity" lang={result.type === "term" ? undefined : "ja"}>{resultIdentity(result)}</span>
                {/if}
                <span class="classification">{resultKind(result)}</span>
              </div>
              {#if !expanded && scents.length > 0}
                <div class="foot">
                  <div class="definitions">
                    {#each scents as scent}<span class="scent" class:numbered={scent.n !== null}>{#if scent.n !== null}<span class="n">{scent.n}.</span>{" "}{/if}{scent.text}</span>{/each}
                  </div>
                </div>
              {/if}
            </div>

            {#if expanded}
              <section class="inline-detail" id={detailId} aria-label={`${resultIdentity(result)} detail`}>
                {#if result.type === "word"}
                  {@const word = result.value as WordHit}
                  {@const key = pronunciationKey(word)}
                  <div class="word-column">
                    <!-- **A rare or old form says so.** 散歩 lists 散步 — 步 is the older form of 歩, one
                         stroke apart — and printed plainly beside the headword it read as the word
                         repeated. The dictionary scores such forms below zero, which `Form::rare`
                         carries; it was computed and never shown. -->
                    {#if word.forms.length > 0}<p class="forms"><span>Also</span> {#each word.forms as form, at}{#if at > 0}{" · "}{/if}<span class="form" class:rare={form.rare}>{form.text}{#if form.rare}<small>rare or old form</small>{/if}</span>{/each}</p>{/if}
                    <WordBody {word} balanced ranking={word.ranking} contextLabel="phrase" {showSource} {meaningsExpanded} {examplesExpanded} ontoggleMeanings={(open) => (meaningsExpanded = open)} ontoggleExamples={(open) => (examplesExpanded = open)} {explainTags} />
                  </div>
                  {#if word.chars.length > 0}
                    <WordCharacters characters={word.chars} bind:selected={tab.character} onlookup={ask} scroller={collection} />
                  {/if}
                {:else if result.type === "grammar"}
                  {@const pattern = result.value as GrammarPattern}
                  <article class="pattern-detail">
                    <p class="lead">{pattern.explanation}</p>
                    {#if pattern.formation}<p class="formation">{pattern.formation}</p>{/if}
                    {#if pattern.detail}<p>{pattern.detail}</p>{/if}
                    {#if pattern.term_ids.length > 0}<div class="term-links">{#each pattern.term_ids as id}<button type="button" onclick={() => openGlossary(id)}>{termNames.get(id) ?? id}</button>{/each}</div>{/if}
                    {#if pattern.examples.length > 0}<h4>Examples</h4>{#each pattern.examples as example}<ExampleText japanese={example.japanese} english={example.english} ruled />{/each}{/if}
                    {#if pattern.reference}<a href={pattern.reference.url} target="_blank" rel="noreferrer">{pattern.reference.label}</a>{/if}
                  </article>
                {:else if result.type === "particle"}
                  {@const particle = result.value as ParticleView}
                  <article class="particle-detail">
                    {#if particle.note}<p class="lead">{particle.note}</p>{/if}
                    <ol>
                      {#each particle.senses as sense}
                        <li><h4>{sense.label}</h4><p>{sense.gloss}</p>{#if sense.examples.length > 0}<ExampleText japanese={sense.examples[0].japanese} english={sense.examples[0].english} ruled />{/if}{#if sense.examples.length > 1}<details open><summary>{sense.examples.length - 1} more examples</summary>{#each sense.examples.slice(1) as example}<ExampleText japanese={example.japanese} english={example.english} ruled />{/each}</details>{/if}</li>
                      {/each}
                    </ol>
                  </article>
                {:else if result.type === "kanji"}
                  <Character editorial character={result.value as KanjiView} onlookup={ask} />
                {:else}
                  <TermDetail embedded editorial term={result.value as Term} terms={termIndex} patternsForTerm={patternsForTerm} onopen={openGlossary} />
                {/if}
              </section>
            {/if}
            </div>
            <!-- The card's keyboard and screen-reader control, drawn as nothing (`theme.css`,
                 `.disclose`): the visible tab it used to be was removed on 2026-10-17. It carries
                 the roving tabindex, the arrow-key handler, and `data-result`, which is what
                 Escape and `moveIndex` focus by; its keyboard focus is drawn on the card. -->
            <button
              type="button"
              class="disclose"
              data-result={result.key}
              aria-expanded={expanded}
              aria-controls={detailId}
              aria-label={`${expanded ? "Hide" : "Show"} details for ${resultIdentity(result)}`}
              tabindex={result.key === rovingKey ? 0 : -1}
              onclick={() => selectResult(result.key)}
              onkeydown={(event) => moveIndex(event, index)}
            ></button>
          </li>
        {/each}
      </ol>
    {/if}
  {/if}
</section>

<style>
  /* Only a short window needs page scrolling; ordinary reading scrolls the results. */
  .reference { container: reference / inline-size; min-height: 0; width: 100%; overflow: auto; --list-divider-color: color-mix(in srgb, var(--text-dim) 50%, var(--bg)); }
  .search-tools { display: flex; flex-direction: column; gap: var(--gap-sm); flex: 0 0 auto; }
  .search-field { flex: 0 0 auto; --bar-fill: var(--surface); --bar-edge: var(--border); --bar-button: 2.5rem; }
  /* A rectangular outline cuts through the tabs; the joined border carries focus instead. */
  .search-field:has(.bar:focus-within) { --bar-edge: var(--accent); }
  .search-field label { display: block; margin-bottom: var(--gap-xs); color: var(--text-dim); font-size: 0.85rem; }
  .bar { display: flex; min-width: 0; border: 1px solid var(--bar-edge); border-radius: var(--radius-sm) var(--radius-sm) var(--radius-sm) 0; background: var(--bar-fill); }
  .bar.multiple { border-bottom-right-radius: 0; }
  .bar input { flex: 1; min-width: 0; width: 0; padding: 0.48rem 0.7rem; border: 0; outline: 0; background: none; color: var(--text); font: inherit; }
  .bar input:hover:not(:disabled), .bar input:focus:not(:disabled) { background: none; }
  .bar input::placeholder { color: var(--text-faint); }
  .bar button { display: grid; place-items: center; flex: 0 0 var(--bar-button); width: var(--bar-button); padding: 0; border: 0; border-left: 1px solid var(--border); border-radius: 0; background: none; color: var(--accent); font: inherit; }
  .bar button:disabled { color: var(--text-faint); }
  .bar button svg { display: block; width: 1em; height: 1em; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; }
  .pos .tick { width: 0.8em; height: 0.8em; margin-right: 0.2em; fill: none; stroke: currentColor; stroke-width: 2; }
  .bar button.clear { color: var(--text-dim); }
  .search-status { display: flex; flex-wrap: wrap; gap: var(--gap-xs) var(--gap); color: var(--text-dim); font-size: 0.82rem; }
  .search-status:empty { display: none; }
  .search-status p { margin: 0; overflow-wrap: anywhere; }
  .draft-hint { color: var(--text-faint); }
  .error { color: var(--red); font-size: 0.85rem; overflow-wrap: anywhere; }
  .error p { margin: 0 0 var(--gap-xs); }
  .welcome { flex: 1; min-height: 0; overflow: auto; padding: var(--gap); color: var(--text-dim); text-align: center; text-wrap: pretty; }
  .notice { padding: var(--gap); border-left: 2px solid var(--accent); color: var(--text-dim); }
  .notice p { margin: 0 0 var(--gap-sm); }
  .notice small { display: block; margin-top: var(--gap-sm); }
  .filters { flex: 0 0 auto; border-bottom: 1px solid var(--border); }
  .facets, .pos { display: flex; flex-wrap: wrap; gap: var(--gap-xs); align-items: center; }
  .facets button, .pos button { padding: 0.3rem 0.45rem; border: 1px solid transparent; border-radius: 0; background: none; color: var(--text-dim); font: inherit; font-size: 0.82rem; cursor: pointer; }
  .facets button:hover, .pos button:hover:not(:disabled) { background: var(--surface-hover); color: var(--text); }
  .facets button { border-bottom: 2px solid transparent; }
  .facets button.current { color: var(--accent); border-bottom-color: var(--accent); }
  .pos button.current { color: var(--accent); border-color: var(--accent-edge); }
  .facets span, .pos span { color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .pos button:disabled { opacity: 0.45; cursor: default; }
  .word-filter-bar { padding: var(--gap-xs) 0; font-size: 0.82rem; color: var(--text-dim); }
  .word-filter-bar summary { cursor: pointer; width: fit-content; }
  .active-filters { margin: var(--gap-xs) 0 0; overflow-wrap: anywhere; }
  .active-filters button { border: 0; padding: 0; margin-left: var(--gap-sm); background: none; color: var(--accent); font: inherit; text-decoration: underline; }
  .ranking, .capped { margin: var(--gap-xs) 0; color: var(--text-faint); font-size: 0.8rem; }
  /* Keep a usable list below natural-height controls. If both cannot fit, the page scrolls too. */
  .result-collection { flex: 1; min-height: min(10rem, 30vh); margin: 0; padding: 0 var(--gap-sm) var(--gap-sm) 0; list-style: none; display: grid; grid-template-columns: minmax(0, 1fr); grid-auto-rows: max-content; align-content: start; overflow: auto; scrollbar-gutter: stable; }
  .result { display: flex; flex-direction: column; min-width: 0; align-self: start; }
  .card { min-width: 0; background: none; border-bottom: 1px solid var(--list-divider-color); }
  .result.expanded > .card { background: var(--surface-raised); box-shadow: inset 2px 0 var(--accent); }
  .preview { display: flex; flex-direction: column; min-width: 0; padding: var(--gap) var(--gap); color: var(--text); transition: background-color var(--anim) var(--ease), box-shadow var(--anim) var(--ease); }
  .preview.current { background: var(--accent-wash); box-shadow: inset 2px 0 var(--accent); }
  @media (hover: hover) and (pointer: fine) {
    .result:not(.expanded) .card:hover .preview { background: var(--surface-hover); box-shadow: inset 2px 0 var(--accent-edge); }
  }
  .result:not(.expanded) .card { cursor: pointer; }
  .head { display: flex; flex-wrap: wrap; align-items: center; gap: var(--gap-xs) var(--gap); min-width: 0; }
  .head > :first-child { flex: 1 1 14rem; min-width: 0; }
  .word-line { display: flex; align-items: center; gap: var(--gap-xs) var(--gap); flex-wrap: wrap; min-width: 0; }
  .identity-pair { display: flex; align-items: center; flex-wrap: wrap; gap: var(--gap-xs); min-width: 0; }
  .identity { font-size: calc(1.3rem * var(--panel-scale, 1)); overflow-wrap: anywhere; }
  .reading { color: var(--text-dim); font-size: calc(0.9rem * var(--panel-scale, 1)); }
  .classification { color: var(--text-faint); font-size: calc(0.8rem * var(--panel-scale, 1)); overflow-wrap: anywhere; }
  .foot { min-width: 0; margin-top: var(--gap-xs); }
  .definitions { min-width: 0; }
  .scent { margin-top: 0.15rem; color: var(--text-dim); font-size: calc(0.85rem * var(--panel-scale, 1)); line-height: 1.5; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; }
  .scent.numbered { padding-left: 1.15rem; text-indent: -1.15rem; }
  .scent .n { margin-right: 0.2rem; color: var(--text-faint); font-size: 0.8em; font-variant-numeric: tabular-nums; }
  .inline-detail { min-width: 0; padding: var(--gap) var(--gap) var(--gap-lg); display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--gap-lg); align-items: start; }
  .word-column, .pattern-detail, .particle-detail { min-width: 0; max-width: 65ch; }
  .word-column > :global(.section:first-child) { margin-top: 0; }
  .inline-detail :global(.section) { text-transform: none; letter-spacing: normal; color: var(--text-dim); font-size: calc(0.85rem * var(--panel-scale, 1)); }
  @container reference (min-width: 75rem) {
    .inline-detail:has(> :global(.character-inspector)) { grid-template-columns: minmax(0, 65ch) minmax(22rem, 42rem); grid-template-rows: min-content 1fr; }
    .inline-detail:has(> :global(.character-inspector)) > .word-column { grid-column: 1; grid-row: 1; }
    .inline-detail:has(> :global(.character-inspector)) > :global(.character-choices) { grid-column: 1; grid-row: 2; }
    .inline-detail > :global(.character-inspector) { grid-column: 2; grid-row: 1 / span 2; border-top: 0; border-left: 1px solid var(--border); padding-top: 0; padding-left: var(--gap-lg); }
  }
  .forms { margin: 0 0 var(--gap); color: var(--text-dim); font-size: calc(0.82rem * var(--panel-scale, 1)); }
  .forms > span:first-child { color: var(--text-faint); margin-right: var(--gap-xs); }
  .forms .form small { margin-left: 0.3em; color: var(--text-faint); font-size: 0.85em; }
  .lead { margin-top: 0; color: var(--text); font-size: calc(1rem * var(--panel-scale, 1)); line-height: 1.55; }
  .formation { color: var(--text-dim); font-size: calc(0.85rem * var(--panel-scale, 1)); }
  .pattern-detail > p:not(.lead):not(.formation), .particle-detail li p { color: var(--text-dim); line-height: 1.55; }
  .pattern-detail h4, .particle-detail h4 { margin: var(--gap) 0 var(--gap-xs); color: var(--text); font-size: calc(0.9rem * var(--panel-scale, 1)); }
  .pattern-detail a { display: inline-block; margin-top: var(--gap); font-size: calc(0.85rem * var(--panel-scale, 1)); }
  .term-links { display: flex; flex-wrap: wrap; gap: var(--gap); margin-top: var(--gap); }
  .term-links button { padding: 0; border: 0; background: none; color: var(--text-dim); font: inherit; text-decoration: underline; }
  .particle-detail ol { margin: 0; padding-left: 1.4rem; }
  .particle-detail li { margin-bottom: var(--gap-lg); }
  .particle-detail details { margin-top: var(--gap-sm); }
  .particle-detail summary { color: var(--text-dim); font-size: calc(0.85rem * var(--panel-scale, 1)); cursor: pointer; }
</style>
