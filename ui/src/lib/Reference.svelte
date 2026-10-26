<script lang="ts">
  // Reference is one lossless collection of balanced previews. Exactly one preview may unfold
  // into authoritative detail, so Cards recover their scanability without restoring the old
  // eager page where every result started audio, tag and character work at once.
  import { onDestroy, tick } from "svelte";
  import { flip } from "svelte/animate";
  import type {
    Found,
    GrammarPattern,
    KanjiView,
    ParticleView,
    TagExplanation,
    Term,
    BrowseLayout,
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
  import { kindColour } from "./word-kinds.js";
  import { promoteSelected, revealHead } from "./promote.js";
  import { CARD_CLICK_IGNORES, travelled } from "./card-click.js";
  import { openGlossary } from "./glossary-navigation";
  import { surfaceWorthShowing } from "./kana.js";
  import Character from "./Character.svelte";
  import ExampleText from "./ExampleText.svelte";
  import LayoutChoice from "./LayoutChoice.svelte";
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
    focusBeforeOpen,
    layout = $bindable("cards"),
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
    /** Two clicks to open a card — focus, then open — rather than one. */
    focusBeforeOpen: boolean;
    layout?: BrowseLayout;
  } = $props();

  const FACET_LABELS = { all: "All", words: "Words", grammar: "Grammar", kanji: "Kanji", terms: "Terms" } as const;
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
  /**
   * `visible` as it is drawn: the same results, with the opened one lifted to the front.
   *
   * A detail then always appears in the same place, so the second use of the page reads like the
   * first. **The grid needs this more than the list does**, which is the opposite of how it was
   * first built: an expanded result is `grid-column: 1 / -1`, so in Cards it tears the grid open
   * wherever it happens to sit — opening the third of three put the entry below both its
   * siblings, with the word that had just been clicked stranded mid-screen. Promoted, the detail
   * is where the detail always is and the remaining cards close up beneath it.
   *
   * **Unconditional, and that is load-bearing.** Gated on the layout it was invisible unless you
   * knew which layout you were in, and nothing on the page says: the toggle is one glyph naming
   * the layout pressing it will *produce*, so the only way to tell is to count the columns. Two
   * collections that look identical — Rows, and Cards below `42rem`, which is drawn as a single
   * column — would also have expanded differently for a reason nothing on screen could show.
   *
   * Everything about **order on screen** reads this — the `{#each}`, `moveIndex`, and the roving
   * fallback below. Everything about **membership** keeps reading `visible`, because the two hold
   * the same results and order is noise to a question about which ones are there.
   */
  const arranged = $derived(promoteSelected(visible, tab.selectedKey, (result: any) => result.key));
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
    // `arranged`, so the fallback below lands on the first result *on screen* rather than the
    // first the search composed — in Rows those differ whenever something is open.
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
    if (tab.asked && tab.asked.toLocaleLowerCase() === wanted.toLocaleLowerCase()) return;
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
  async function run(target: Tab = search.current) {
    // A restored tab's question is in `pending`, not in the box — the box is empty after a
    // restart. Every other search reads what was typed.
    const restoring = target.pending !== "";
    const text = (restoring ? target.pending : target.query).trim();

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
      target.error = String(thrown);
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

  /**
   * How long a promoted card should take to travel, in milliseconds.
   *
   * Read from `--anim` rather than from `prefers-reduced-motion` directly, because that property
   * is already how the rest of the app decides whether to move: `theme.css` sets it to `0ms`
   * under the media query as a *default*, and `App.svelte` overwrites it from the animation
   * setting, which is the more specific answer. Both routes to "no motion" arrive here as `0`,
   * which `flip` renders as an instant jump.
   *
   * Called per animation rather than derived, because nothing invalidates a custom property and
   * a `$derived` would hold whatever it happened to read first.
   */
  function motionMs() {
    const declared = getComputedStyle(document.documentElement).getPropertyValue("--anim");
    return Number.parseFloat(declared) || 0;
  }

  /**
   * Open or close a result, then put it where it can be seen.
   *
   * **`tick()`, not `queueMicrotask`.** Svelte flushes its own effects in a microtask, so one
   * queued here is a race for which runs first. In place that never showed: the element had not
   * moved, so scrolling to it early landed on the same pixels. In Rows the card is being lifted
   * to the front, and both the move and the `firstElementChild` test below only exist after the
   * flush.
   *
   * **A card at the front is scrolled to, not scrolled *at*.** `block: "nearest"` was wrong for
   * this the moment a card could be taller than the collection: with nothing of a tall element
   * visible, the nearest edge to bring into view is its *bottom*, so promoting a long entry left
   * the collection 117px past the word that had just been opened. Measured, not reasoned about.
   * The head is what the reader wants, and for the first child that is simply the top.
   */
  async function selectResult(key: string) {
    const closing = tab.selectedKey === key;
    tab.selectedKey = closing ? null : key;
    rovingKey = key;
    await tick();
    const item = collection?.querySelector<HTMLElement>(`[data-item="${CSS.escape(key)}"]`);
    if (!item || !collection) return;
    // This collection is flat, so a promoted card *is* the collection's first child and topping
    // it out means going to the very top. Glossary is grouped and hands the helper its section
    // instead. Anything not at the front is not promoted, and `nearest` is right for it.
    if (item === collection.firstElementChild) revealHead(collection, item);
    else item.scrollIntoView({ block: "nearest" });
  }

  /**
   * The card as a pointer target, in both models.
   *
   * **The card is the only thing a pointer can click.** Until 2026-10-17 a pull tab hung under each
   * card, and under the `hover` model it was the only pointer route in: the card ignored clicks
   * and the tab opened it. The tab took a row under every card and crowded the page, so it went,
   * and its click came here — one click opens or closes under `hover`, exactly as one click on the
   * tab did. Under `focus` nothing changed: the first click picks, the second opens.
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
    if (!focusBeforeOpen || rovingKey === key) {
      void selectResult(key);
      return;
    }
    rovingKey = key;
  }

  /**
   * Arrow keys, over `arranged` rather than `visible`.
   *
   * **Load-bearing.** `at` arrives as the `{#each}` block's positional index, which counts the
   * arrangement on screen. Resolving it against `visible` would step to whatever the search had
   * put at that position instead — silently, and only while something is promoted.
   */
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
    // block is keyed, so a collapsing card is *moved* rather than rebuilt and `querySelector`
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
    indexing = true;
    indexMessage = "";
    try {
      indexMessage = await buildGlossIndex();
      tab.needsIndex = false;
      if (tab.asked) await run(tab);
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
    if (result.type === "word") return wordKinds(result.value).join(" · ");
    // **The level, on the front, where it is worth something.** Every one of the 860 packed
    // patterns carries a JLPT level, and it is the fact that tells a learner which of nine
    // results is for them — yet it was only drawn once a card was opened, right-aligned into a
    // corner of the detail, while the badge in prime position said GRAMMAR PATTERN, which the
    // facet above already said. Kind plus qualifier, exactly as a word reads NOUN · ADVERB.
    // The short kind because the pair has to fit a card: "Grammar pattern · N4" wraps.
    if (result.type === "grammar") {
      return result.value.jlpt ? `Grammar · ${result.value.jlpt}` : "Grammar pattern";
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
  <form class="search-field" onsubmit={(event) => { event.preventDefault(); ask(tab.query); }}>
    <div class="bar">
      <input bind:this={box} bind:value={tab.query} oninput={(event) => prepare(event.currentTarget.value)} lang="ja" spellcheck="false" placeholder="A word, meaning, grammar point, kanji, or term" aria-label="Search Reference" />
      <LayoutChoice bind:layout flush />
      <button type={tab.asked && tab.query.trim() === tab.asked ? "button" : "submit"} class:clear={tab.asked && tab.query.trim() === tab.asked} onclick={tab.asked && tab.query.trim() === tab.asked ? clear : undefined} disabled={tab.searching} aria-label={tab.asked && tab.query.trim() === tab.asked ? "Clear the search" : "Search"}>
        {tab.searching ? "…" : tab.asked && tab.query.trim() === tab.asked ? "×" : "⌕"}
      </button>
    </div>
    <TabStrip />
  </form>

  {#if tab.error}<p class="error">{tab.error}</p>{/if}
  {#if !tab.asked}
    <div class="welcome">Search Japanese or English. Conjugated forms are welcome.</div>
  {:else if tab.needsIndex}
    <div class="notice">
      <p>English meaning search needs a one-time local dictionary index.</p>
      <button type="button" class="control" onclick={buildIndex} disabled={indexing}>{indexing ? "Building…" : "Build index"}</button>
      {#if indexMessage}<small>{indexMessage}</small>{/if}
    </div>
  {:else if results.length === 0}
    <div class="welcome">Nothing matching <strong>{tab.asked}</strong>.</div>
  {:else}
    <!-- **One surface, divided by hairlines** — the search field's own construction, where the
         input and its buttons are cells of one box rather than things with gaps between them.
         These rows used to float on the page background while everything around them (the field,
         the tab strip, every result card) was a bordered surface.

         `--radius-sm` on purpose: `tokens.md` reserves it for small controls and `--radius` for
         panels, and this is a cluster of small controls. Following that scale is also what makes
         it match the field. -->
    <div class="filters">
    <!-- **One of these**, and the shape says so: an underline, no fill, the idiom `.panel-switch`
         already uses for a value that replaces rather than accumulates. `radiogroup` because that
         is what choosing exactly one is; as five toggle buttons a screen reader could not tell
         this row from the multi-select one below it. -->
    <div class="facets" role="radiogroup" aria-label="Result types">
      {#each FACETS as facet}
        <button type="button" role="radio" class:current={tab.facet === facet} aria-checked={tab.facet === facet} onclick={() => (tab.facet = facet)}>
          {FACET_LABELS[facet]} <span>{counts[facet]}</span>
        </button>
      {/each}
    </div>
    {#if showsWordKinds}
      <!-- **Any number of these.** `role="group"` because the row had an `aria-label` on a bare
           div, which assistive technology ignores — it announced nothing at all. `aria-pressed`
           was already right for a multi-select toggle and stays; what changes is that a chosen
           chip now wears a check, so the two rows cannot be told apart only by trying them. -->
      <div class="pos" role="group" aria-label="Parts of speech">
        {#each wordCounts as option}
          {@const on = tab.wordKinds.includes(option.label)}
          <!-- The chip wears the colour its senses are drawn in, so the row you narrow by and
               the meanings you then read are recognisably the same thing. `Other` has no hue —
               `kindColour` falls back — because it is the absence of a classification. -->
          <button type="button" class:current={on} aria-pressed={on} disabled={option.count === 0} style={`--kind: ${kindColour(option.label)}`} onclick={() => toggleKind(option.label)}>
            {#if on}
              <!-- Drawn, not typed: `controls.md` requires it, and this is the same stroke as the
                   copied-state check in `CopyButton`. -->
              <svg class="tick" viewBox="3 2 18 20" aria-hidden="true"><path d="M5.5 12.5l4.1 4.1L18.8 7.4" /></svg>
            {/if}
            {option.label} <span>{option.count}</span>
          </button>
        {/each}
        {#if tab.wordKinds.length > 0}<button type="button" class="reset" onclick={() => (tab.wordKinds = [])}>Show all words</button>{/if}
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

    {#if visible.length === 0}
      <div class="welcome">No results remain under these filters.</div>
    {:else}
      <ol
        class="result-collection"
        class:cards={layout === "cards"}
        data-cards={focusBeforeOpen ? "focus" : "hover"}
        aria-label="Reference results"
        bind:this={collection}
        onscroll={() => { if (collection) tab.collectionScroll = collection.scrollTop; }}
      >
        {#each arranged as result, index (result.key)}
          {@const expanded = result.key === tab.selectedKey}
          {@const detailId = `reference-detail-${index}`}
          {@const scents = resultScents(result)}
          <!-- Keyed above, so a promotion **moves** this node rather than rebuilding it: loaded
               detail, open folds and focus all survive the trip to the front. `flip` is what
               makes the move legible — an unannounced reorder teleports the card the reader
               just clicked, and in the grid it also closes the gap that card left behind. -->
          <li class="result" class:expanded data-item={result.key} data-focused={result.key === rovingKey ? "" : undefined} animate:flip={{ duration: motionMs() }}>
            <!-- `.card` carries the border, the fill and the clipping; the list item carries only
                 the grid placement, and holds the card's hidden control beside it. -->
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
              {#if scents.length > 0}
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
                  <!-- **Two columns, and the word owns the measure.** The word reads down the
                       left at a fixed measure; the character panel sits beside it rather than
                       stacked above its senses, which is what put the kanji's meanings directly
                       over the word's under two identical headings. Below `42rem` the grid
                       collapses and `.beside` falls back into the flow, which is the layout this
                       always had. -->
                  <div class="word-column">
                    <!-- **A rare or old form says so.** 散歩 lists 散步 — 步 is the older form of 歩, one
                         stroke apart — and printed plainly beside the headword it read as the word
                         repeated. The dictionary scores such forms below zero, which `Form::rare`
                         carries; it was computed and never shown. -->
                    {#if word.forms.length > 0}<p class="forms"><span>Also</span> {#each word.forms as form, at}{#if at > 0}{" · "}{/if}<span class="form" class:rare={form.rare}>{form.text}{#if form.rare}<small>rare or old form</small>{/if}</span>{/each}</p>{/if}
                    <WordBody {word} balanced ranking={word.ranking} contextLabel="phrase" {showSource} {meaningsExpanded} {examplesExpanded} ontoggleMeanings={(open) => (meaningsExpanded = open)} ontoggleExamples={(open) => (examplesExpanded = open)} {explainTags} />
                  </div>
                  <!-- **Every character the word is written with, open, in the order it is read
                       in.** They used to be a row of chips that opened one at a time, which made
                       a two-kanji word two clicks and a decision before it said anything — and
                       the decision was uninformed, because a chip is the glyph and nothing else.
                       Opening a word is already the ask; there is nothing further to ask for.

                       `word.chars` rather than the headword's characters: it is the list the
                       lookup actually answered with, so a character the dictionary has nothing
                       for cannot appear as an empty panel, and a headword that repeats a kanji
                       cannot draw it twice. -->
                  {#if word.chars.length > 0}
                    <aside class="beside" aria-label={`${word.headword} characters`}>
                      <!-- **The column says what it is, like the one beside it.** `MEANINGS` and
                           `EXAMPLES` name the sections of the word column; this column had a glyph
                           at the top and nothing saying why it was there. The same `.section`
                           heading, so it reads as a third section of one card rather than a panel
                           that arrived from somewhere else. Right-aligned with the column it
                           heads — see `Character.svelte` for why that column hangs off its right
                           edge. -->
                      <h4 class="section">Characters</h4>
                      {#each word.chars as character (character.character)}
                        <Character {character} onlookup={ask} />
                      {/each}
                    </aside>
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
                  <Character character={result.value as KanjiView} onlookup={ask} />
                {:else}
                  <TermDetail embedded term={result.value as Term} terms={termIndex} patternsForTerm={patternsForTerm} onopen={openGlossary} />
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
  .reference { container: reference / inline-size; min-height: 0; }
  /* **The two numbers everything joined to the bar is built from**, declared here and nowhere
     else. `traps.md` explains why they are custom properties: a scoped selector cannot reach into
     `TabStrip` or `LayoutChoice`, but an inherited property crosses that boundary. `.act` and
     `.field` were renamed to `.bar button` and `.search-field`, and these went with them — so
     three components each fell back to their own guess, and `Clear all` came out 0.2rem narrower
     than the two controls it is supposed to sit exactly beneath. */
  .search-field { flex: 0 0 auto; --bar-button: 2.2rem; --bar-fill: var(--surface); }
  /* On the wrapper, not on `.bar`, because the tab joined to its underside and `Clear all` below
     that both have to change colour with it. A fill pinned on one of them draws a seam across the
     join at exactly the moment the join exists to remove. */
  .search-field:has(.bar:hover), .search-field:has(.bar input:focus) { --bar-fill: var(--surface-sunken); }
  .bar { display: flex; border: 1px solid var(--border); border-radius: var(--radius-sm) var(--radius-sm) 0 0; background: var(--bar-fill); overflow: hidden; }
  .bar input { flex: 1; min-width: 0; padding: 0.48rem 0.7rem; border: 0; outline: 0; background: none; color: var(--text); font: inherit; }
  .bar input:hover:not(:disabled), .bar input:focus:not(:disabled) { background: none; }
  .bar input::placeholder { color: var(--text-faint); }
  .bar button { width: var(--bar-button); padding: 0; border: 0; border-left: 1px solid var(--border); border-radius: 0; background: none; color: var(--accent); font: inherit; font-size: calc(1rem * var(--panel-scale, 1)); }
  .bar button.clear { color: var(--red); }
  .error { color: var(--red); font-size: calc(0.85rem * var(--panel-scale, 1)); }
  .welcome { flex: 1; display: grid; place-items: center; color: var(--text-faint); text-align: center; }
  .notice { padding: var(--gap); border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); color: var(--text-dim); }
  .notice p { margin: 0 0 var(--gap-sm); }
  .notice small { display: block; margin-top: var(--gap-sm); color: var(--text-faint); }
  /* The search field's material, so the filters read as its sibling rather than as loose text. */
  .filters { flex: 0 0 auto; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface); overflow: hidden; }
  .facets, .pos { display: flex; flex-wrap: wrap; gap: var(--gap-xs); align-items: center; padding: var(--gap-sm); }
  /* **A hairline on each row that can follow, never a `> * + *` rule.** `traps.md` records that
     form silently missing `<TabStrip />` because a component is not an element; the same selector
     here would break the first time one of these rows becomes one. `.facets` is always first and
     so never carries a top border — which is also why a frame holding only it has no stray
     divider, and the rows below are independently conditional. */
  .pos, .ranking, .capped { border-top: 1px solid var(--border); }
  .facets button, .pos button { padding: 0.25rem 0.55rem; border: 1px solid transparent; border-radius: var(--radius-sm); background: none; color: var(--text-dim); font: inherit; font-size: calc(0.78rem * var(--panel-scale, 1)); cursor: pointer; }
  .facets button:hover, .pos button:hover:not(:disabled) { background: var(--surface-hover); color: var(--text); }
  /* **An underline, and deliberately no fill.** The row below fills, and the two used to draw
     their on-state identically while one replaced a choice and the other added to it. See
     `App.svelte` `.panel-switch`, which carries "current" the same way. */
  .facets button { border-radius: var(--radius-sm) var(--radius-sm) 0 0; border-bottom: 2px solid transparent; }
  .facets button.current { color: var(--accent); border-bottom-color: var(--accent); }
  /* The wash-and-edge idiom, which is what "on" looks like everywhere else. */
  /* **The kind's own colour, not the accent.** A chosen chip used to be accent-coloured like
     every other current control, which said "this is on" and nothing about *what* is on. Wearing
     the hue its senses carry makes narrowing to Verb and reading a verb sense one recognisable
     thing. `Other` resolves to `--text-faint`, which is the same "on" without a claim. */
  .pos button.current { color: var(--kind); border-color: var(--kind); background: none; }
  .pos .tick { width: 0.62em; height: 0.62em; margin-right: 0.15rem; vertical-align: -0.02em; fill: none; stroke: currentColor; stroke-width: 2.6; stroke-linecap: round; stroke-linejoin: round; }
  .facets span, .pos span { color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .pos button:disabled { opacity: 0.45; cursor: default; }
  .pos .reset { margin-left: auto; color: var(--text-faint); text-decoration: underline; }
  .ranking, .capped { margin: 0; padding: var(--gap-xs) var(--gap-sm); color: var(--text-faint); font-size: calc(0.75rem * var(--panel-scale, 1)); }
  /* **Flush on the left, clear of the scrollbar on the right**, and the asymmetry is the point.
     Measured: the search field and the filter frame both start at 86px and a card used to start
     at 93 — a 7px step nothing else on the page takes, so the left padding went. Taking the
     right one with it left the cards butted against the scrollbar, which `scrollbar-gutter:
     stable` reserves but does not pad. The gap is `--gap-sm`, the same distance the cards keep
     from each other, so the column reads at one rhythm. */
  .result-collection { flex: 1; min-height: 0; margin: 0; padding: var(--gap-sm) var(--gap-sm) var(--gap-sm) 0; list-style: none; display: grid; grid-template-columns: minmax(0, 1fr); grid-auto-rows: max-content; align-content: start; gap: var(--gap-sm); overflow: auto; scrollbar-gutter: stable; }
  .result-collection.cards { grid-template-columns: repeat(auto-fill, minmax(min(18rem, 100%), 1fr)); }
  /* The list item is placement only; `.card` is the card. Its other child is the card's hidden
     control, which is out of the flow and reserves no space. */
  .result { display: flex; flex-direction: column; min-width: 0; align-self: start; }
  .card { min-width: 0; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); overflow: hidden; }
  .result.expanded { grid-column: 1 / -1; }
  .result.expanded .card { border-color: var(--accent-edge); }
  /* Content, not a control. The card stopped being one button so its text can be selected and
     copied, and so the only thing that opens it is the thing that looks like it opens it. There
     is deliberately no hover here: a background that lifts under the pointer promises a click
     that no longer happens. */
  /* **Vertical padding only; the horizontal inset belongs to each child.** A negative margin
     would be the shorter way to reach the card's edge and it fails the containment check on
     principle: `scrollWidth > clientWidth` is exactly what pulling a child past its parent
     means. Letting the badge sit against a row that simply has no right padding costs one
     declaration per child and escapes nothing. */
  /* The badge cancels exactly `--head-top` to reach the card's corner. One number, read twice,
     so the two cannot drift apart. */
  .preview { --head-top: 0.68rem; display: flex; flex-direction: column; min-width: 0; padding: 0 0 0.68rem; color: var(--text); }
  .preview.current { background: var(--accent-wash); box-shadow: inset 2px 0 var(--accent); }
  /* **The lift that makes the first of two clicks mean something.** A focused card is one the
     reader is standing on, not one they have opened, so it stays well short of `.current` — the
     two are on screen together whenever a focused card sits beside the open one, and the open one
     has to keep winning. `--surface-hover` because that is the token for "the pointer is dealing
     with this", which is exactly what a picked card is.
     Scoped to the focus model: under `hover` the pointer already does this marking, and a card
     that stayed lit after the pointer left would be claiming a state that model does not have. */
  .result-collection[data-cards="focus"] .result[data-focused]:not(.expanded) .preview { background: var(--surface-hover); }
  /* Only while the click means one thing. A shut card answers a click in both models — picking
     or opening it — so it shows the pointer in both; on an open card the detail is prose to be
     read and selected, so it keeps the text cursor even though a click on the preview closes it. */
  .result:not(.expanded) .card { cursor: pointer; }
  /* The word and the badge share the top line; everything below spans the full width. No right
     padding, so the badge ends flush against the card. */
  .head { display: flex; align-items: flex-start; gap: var(--gap-xs); min-width: 0; padding: var(--head-top) 0 0 var(--gap); }
  .head > :first-child { flex: 1; min-width: 0; }
  /* `.disclose`, the card's hidden control, is shared with Glossary from `theme.css`. */
  /* **Centred on each other, not on their baselines.** The copy glyph is 1rem and the speak
     control smaller again, against a 0.95rem headword; sharing a baseline hung both off the
     bottom of the word. This is the same reasoning `PronounceButton`'s own comment records. */
  /* **Two gaps, because there are two kinds of neighbour.** The columns are `--gap`, which is what
     separates the identity from the things that act on it; the rows stay `--gap-xs`, so the
     `as written` chip taking a second line costs the card four pixels rather than thirteen.
     One value for both was what made a wrapped card taller than it needed to be. */
  .word-line { display: flex; align-items: center; gap: var(--gap-xs) var(--gap); flex-wrap: wrap; min-width: 0; }
  /* Tight, and `center` rather than `baseline` to match what the row already did — the reading is
     0.9rem against a 1.45rem headword, and this changes their spacing, not their alignment. */
  .identity-pair { display: flex; align-items: center; flex-wrap: wrap; gap: var(--gap-xs); min-width: 0; }
  /* The word leads the row: a shade larger than the copy icon (1.15rem) beside it, so the thing
     being read outweighs the things acting on it. The reading takes the size the word used to
     have — still clearly secondary, but large enough to be read rather than squinted at. */
  /* Size carries the headword, not weight. At 1.25rem against 0.78rem meanings it already leads
     the card; the extra 650 made every result shout. */
  .identity { font-size: calc(1.45rem * var(--panel-scale, 1)); }
  .reading { color: var(--text-dim); font-size: calc(0.9rem * var(--panel-scale, 1)); }
  /* A hairline between what the word *is* and what it means. Both halves were plain text at
     similar weights, so a card read as one paragraph that happened to start with a headword. */
  /* The meanings, on the line under the rule. A chevron had a row of its own here once, which
     cost a band of empty card under every result; no visible control is left on a card now.
     **The rule's insets have to match at both ends.** Margin rather than padding, so it stops
     `--gap` short of each card edge instead of running the full bleed; a line inset differently
     at each end reads as hung rather than drawn. */
  .foot { min-width: 0; margin: 0.4rem var(--gap) 0; padding-top: 0.4rem; border-top: 1px solid var(--border); }
  .definitions { min-width: 0; }
  /* A corner tag, not a floating chip: flush to two edges, so the only rounded corner is the one
     facing into the card. It sits in space the card already had and takes a line back from the
     summary, which is the point — what a result *is* belongs beside it, not under it.
     **Sunken, not raised.** A tag cut into the card's corner rather than laid on top of it: the
     card is already `--surface`, and a lighter patch in its corner read as a chip that had come
     loose. `--surface-sunken` is the same material the opened detail below it is floored with. */
  /* Wraps rather than refusing to: a word in three kinds is rare but real, and `nowrap` here
     would push the badge into the headword instead of taking a second line of its own space.
     `max-width` on `.side` keeps the summary the majority of the card either way. */
  /* Only the top margin is negative — it cancels `.head`'s padding so the badge reaches the
     card's real corner, where the card's own `overflow: hidden` rounds it. Vertical is safe;
     a negative *horizontal* margin is what the containment check exists to catch. Capped so a
     word in three kinds takes a second line rather than squeezing the headword. */
  .classification { flex: 0 0 auto; max-width: 45%; margin-top: calc(var(--head-top) * -1); padding: 0.2rem 0.45rem; border-bottom-left-radius: var(--radius); background: var(--surface-sunken); color: var(--accent); font-size: calc(0.65rem * var(--panel-scale, 1)); letter-spacing: 0.06em; text-transform: uppercase; text-align: right; text-wrap: balance; }
  .scent { margin-top: 0.15rem; color: var(--text-dim); font-size: calc(0.78rem * var(--panel-scale, 1)); line-height: 1.35; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; }
  /* A hanging indent rather than a grid gutter: the clamp above needs `-webkit-box`, which has
     no columns to hang anything in. Padding moves the block right and the negative indent pulls
     the first line back out, so the number sits in the margin and wrapped text clears it. */
  .scent.numbered { padding-left: 1.15rem; text-indent: -1.15rem; }
  /* The detail's ordinal, to the declaration: faint, 0.8em, tabular so 1. and 2. line up.
     Deliberately *not* `aria-hidden` as it is there — the detail's numbers are decoration over
     a real `<ol>`, and these spans have no list semantics to fall back on. */
  .scent .n { margin-right: 0.2rem; color: var(--text-faint); font-size: 0.8em; font-variant-numeric: tabular-nums; }
  /* **A measure, which this never had.** With no cap, a sense reading "1. what" was forty pixels
     of ink on an eight-hundred pixel line and the rest of the card was empty. `65ch` is the
     ordinary reading measure; the second column takes the room that frees. */
  .inline-detail { min-width: 0; padding: var(--gap-lg); border-top: 1px solid var(--border); background: var(--surface-sunken); display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--gap-lg); align-items: start; }
  .word-column { min-width: 0; max-width: 65ch; }
  /* **A stack of characters, not a run of them.** Every kanji in the word is open here, so two or
     three entries follow each other down one column — and each is itself a glyph, a table of
     readings and up to four headed sections. Without a divider the last compound of 貴 and the
     first reading of 方 are two lines of the same colour a few pixels apart, and the reader has to
     find the seam by reading it.

     The rule belongs to the *following* entry rather than to a `> * + *` gap, which `traps.md`
     records: a divider that lives on the container is a divider that survives its neighbour being
     removed. One character draws none, which is the common case and has to cost nothing. */
  .beside { min-width: 0; display: grid; gap: var(--gap); align-content: start; text-align: right; }
  /* **A heading that leads its column starts flush.** `.section`'s top margin exists to separate a
     heading from whatever ran above it; at the top of a column there is nothing above it, so the
     margin is a band of empty card — and, because only one of the two columns had it zeroed,
     `MEANINGS` sat thirteen pixels below `CHARACTERS` when they are the same rank of thing.
     `:first-child` and a direct child, deliberately: the `.section` headings *inside* a character
     panel do follow content and keep their space. `:global` because `WordBody` has no root
     element, so its `Meanings` heading is a child of this column rather than of itself. */
  .word-column > :global(.section:first-child),
  .beside > :global(.section:first-child) {
    margin-top: 0;
  }
  .beside > :global(.character + .character) {
    padding-top: var(--gap);
    border-top: 1px solid var(--border);
  }
  /* Only where a second column is worth having. The same `42rem` the collection already switches
     on, so the card grid and the detail change shape together rather than at two widths.
     **The aside is sized, not left over.** Given the remainder it came out at 168px and the
     character panel's own label column wrapped KUN-YOMI onto two lines — the reading names are
     what that panel is *for*. So the aside states what it needs and the word column takes the
     rest, capped by `.word-column` where the measure already lives. */
  @container reference (min-width: 42rem) {
    .inline-detail:has(.beside) { grid-template-columns: minmax(0, 1fr) minmax(15rem, 20rem); }
  }
  .forms { margin: 0 0 var(--gap); color: var(--text-dim); font-size: calc(0.82rem * var(--panel-scale, 1)); }
  .forms > span:first-child { color: var(--text-faint); margin-right: var(--gap-xs); }
  /* The form itself in the word's colour; the note after a rare one in the faint one, small. */
  .forms .form { color: var(--text-dim); }
  .forms .form small { margin-left: 0.3em; color: var(--text-faint); font-size: 0.85em; }
  .lead { color: var(--text); font-size: calc(1rem * var(--panel-scale, 1)); line-height: 1.55; }
  .formation { color: var(--text-faint); font-size: calc(0.82rem * var(--panel-scale, 1)); }
  .pattern-detail > p:not(.lead):not(.formation), .particle-detail li p { color: var(--text-dim); line-height: 1.55; }
  .pattern-detail h4, .particle-detail h4 { margin: var(--gap) 0 var(--gap-xs); color: var(--accent); font-size: calc(0.8rem * var(--panel-scale, 1)); }
  .pattern-detail a { display: inline-block; margin-top: var(--gap); font-size: calc(0.8rem * var(--panel-scale, 1)); }
  .term-links { display: flex; flex-wrap: wrap; gap: var(--gap-xs); margin-top: var(--gap); }
  .term-links button { padding: 0.15rem 0.4rem; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface-raised); color: var(--text-dim); font: inherit; font-size: calc(0.75rem * var(--panel-scale, 1)); }
  .particle-detail ol { margin: 0; padding-left: 1.4rem; }
  .particle-detail li { margin-bottom: var(--gap-lg); }
  .particle-detail details { margin-top: var(--gap-sm); }
  .particle-detail summary { color: var(--text-faint); font-size: calc(0.78rem * var(--panel-scale, 1)); cursor: pointer; }

  @container reference (max-width: 41.999rem) {
    .result-collection.cards { grid-template-columns: minmax(0, 1fr); }
    .inline-detail { padding: var(--gap); }
    .facets { flex-wrap: nowrap; overflow-x: auto; padding-bottom: 1px; }
    .facets button { white-space: nowrap; }
  }
</style>
