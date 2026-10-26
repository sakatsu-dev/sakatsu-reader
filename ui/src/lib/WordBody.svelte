<script lang="ts">
  // One word's definitions, examples and other sources — everything a `WordView` says,
  // and nothing about studying it.
  //
  // Extracted from Vocabulary so the reference page can show a search result without a
  // second implementation of the sense list drifting away from this one. Vocabulary keeps
  // the parts that are about *the learner* — status, Anki, audio — and renders this
  // beneath them; the reference page renders it alone, once per result.
  import type { Retrieved, TagExplanation, VocabularyRanking, WordView } from "../model";
  import SenseGroupHeading from "./SenseGroupHeading.svelte";
  import SenseRow from "./SenseRow.svelte";
  import ExampleText from "./ExampleText.svelte";
  import { groupSenseSections } from "./sense-groups.js";
  import { vocabularyPresentation } from "./vocabulary-context.js";

  let {
    word,
    loading = false,
    hasDictionaries = true,
    brief = false,
    balanced = false,
    ranking = null,
    contextLabel = "context",
    showSource = false,
    meaningsExpanded = false,
    examplesExpanded = false,
    ontoggleMeanings = () => {},
    ontoggleExamples = () => {},
    explainTags,
  }: {
    word: WordView | null;
    loading?: boolean;
    /**
     * Lead with the first sense and put the rest behind a click.
     *
     * For the reference page, where a search returns two dozen words at once and printing
     * every sense of each buries the one that was searched for. **Off by default**: the
     * vocabulary panel shows the single word the reader selected, which has nothing to
     * bury and no reason to make them click for what they already asked for.
     */
    brief?: boolean;
    /** Analysis depth: one meaning and one example lead; every remainder stays lossless. */
    balanced?: boolean;
    /** Analysis-only evidence over the already reordered primary senses. */
    ranking?: VocabularyRanking | null;
    /** The bounded evidence available to the ranking: a sentence in Analysis, a phrase here. */
    contextLabel?: "context" | "phrase";
    /**
     * Name the dictionary each definition came from, as a chip in its tag row.
     *
     * **Off by default, and that is not a licence decision.** CC BY-SA is satisfied by the
     * footer credit (`project notes` §8); this chip answers "where did *this* one come from",
     * which most reading does not ask. The caller passes the setting for its own page —
     * they are separate because a search repeats the same name on eighteen results where
     * the analysis page shows one word.
     *
     * It goes *in the tag row* rather than floating: it was tried in the flow, where it
     * cost a line, and in the corner, where it hung disconnected from what it described.
     * A source is another chip, and chips already have a row.
     */
    showSource?: boolean;
    meaningsExpanded?: boolean;
    examplesExpanded?: boolean;
    ontoggleMeanings?: (open: boolean) => void;
    ontoggleExamples?: (open: boolean) => void;
    /** Drives the advice in the empty state, not the empty state itself. */
    hasDictionaries?: boolean;
    /** Resolves every tag on a word at once, answering only for what it can explain. */
    explainTags: (codes: string[]) => Promise<Record<string, TagExplanation>>;
  } = $props();

  // The credit for the inline senses, when the page asks for credits at all.
  //
  // `null` rather than a falsy string: `SenseRow` distinguishes "no source" from "a source
  // named empty", and only the first row is given it — the credit belongs to the group, and
  // repeating it down a word with four senses is the noise the chip exists to avoid.
  const inlineCredit = $derived(showSource ? (word?.inline_source ?? null) : null);

  // Which badge is open, keyed by sense index and code so two senses that both carry `n`
  // do not open together. One at a time: several notes stacked under one word is the
  // clutter this feature exists to avoid.
  let opened = $state<string | null>(null);
  const toggle = (key: string) => (opened = opened === key ? null : key);

  // The key is `senseIndex:code`, and the next word has a sense at that index too — so
  // without this a note opened on one word reappears on the next.
  $effect(() => {
    void word;
    opened = null;
  });

  /// What separates one reading from the next.
  ///
  /// `・` between kana, which is the page's own convention for alternatives and the only
  /// mark that reads as a boundary rather than as punctuation inside a word.
  ///
  /// **Not when the readings are romanised.** JMnedict answers some words in Latin letters,
  /// and `Seia・Sei` reads as one person with two names rather than two names — which is
  /// exactly what `・` means between Latin words. Those keep the semicolon.
  const readingSeparator = (readings: string[]) =>
    readings.some((reading) => /[A-Za-z]/.test(reading)) ? "; " : "・";

  /** The component an answer describes, kept null for whole or historical unknown answers. */
  const componentOf = (retrieved: Retrieved): string | null =>
    retrieved.kind === "component" ? retrieved.query : null;

  /// One sense on its way to the page, and which dictionary owes for it.
  ///
  /// `credit` is null for the analyzer's own senses, which are the list's default voice and
  /// need no chip saying so.
  type Listed = {
    sense: WordView["inline"][number];
    credit: string | null;
    component: string | null;
  };
  type SenseSection = ReturnType<typeof groupSenseSections<Listed>>[number];
  type SenseGroup = SenseSection["groups"][number];

  /// What every other dictionary *adds*, flattened into the one list.
  ///
  /// These used to be a disclosure of their own at the foot of the word — "also in
  /// Jitendex.org" — under the examples, three folds down from the senses it was adding to.
  /// They are additional senses of the same word, so they belong with the additional senses.
  ///
  /// **Credited under `showSource` like every other sense**, which it did not used to be.
  ///
  /// The argument for the exception was that these sit in a list whose other members are the
  /// analyzer's, so an uncredited one is not unlabelled but *mis*labelled — and that is still
  /// true of the *claim*. What made it the wrong call is where the obligation is actually met:
  /// `project notes` §8 asks that data travel with its licence and be attributed, and it is, in the
  /// footer on every page and in About beside the licence itself. A per-sense chip is a second,
  /// louder answer to a question the page has already answered, and it was the one thing in the
  /// meanings list the reader could not turn off.
  ///
  /// So the setting now governs all of it. **If the footer credit ever goes, this exception has
  /// to come back** — that is the condition, not the chip.
  const fromElsewhere = $derived(
    (word?.also_in ?? []).flatMap((source) =>
      source.senses.map((sense): Listed => ({
        sense,
        credit: showSource ? source.title : null,
        component: componentOf(source.retrieved),
      })),
    ),
  );

  // Split rather than truncated: the tail is still rendered, just shut. Indices carry
  // through so a badge's key stays `senseIndex:code` against the word's own numbering and
  // the ordered list keeps counting where the first part left off.
  const ownSenses = $derived(
    (word?.inline ?? []).map((sense): Listed => ({
      sense,
      credit: null,
      component: word ? componentOf(word.inline_retrieved) : null,
    })),
  );
  const leadsWithOne = $derived(brief || balanced);
  const contextual = $derived(vocabularyPresentation(ownSenses.length, balanced ? ranking : null));
  const shownCount = $derived(balanced ? contextual.leadCount : leadsWithOne ? 1 : ownSenses.length);
  const shown = $derived(ownSenses.slice(0, shownCount));
  const hidden = $derived([...ownSenses.slice(shownCount), ...fromElsewhere]);

  /// Whole-source classifications, then stable grammatical subgroups.
  ///
  /// 有る demonstrates why both levels matter. `godan irregular` spans all six meanings,
  /// `intransitive` spans only 1–5, and `auxiliary verb` belongs to 6. One whole-word intersection
  /// loses the subgroup; one flat run keeps extending on `godan` and repeats `intransitive` five
  /// times. Sections preserve the global fact and groups preserve the narrower one. Reference
  /// collects separated classifications; Analysis keeps the contextual ranking's exact order,
  /// so an unsupported sense can never jump ahead of a supported one merely to join a badge.
  ///
  /// Shown and hidden senses are grouped separately. A tag behind the "more senses" fold must not
  /// be hoisted above a control while its meanings remain hidden; opening the fold reveals its own
  /// stable groups instead.
  ///
  /// Other dictionaries retain their own group boundaries even if their codes happen to match:
  /// a shared-looking badge must never make one source appear to own another source's senses.
  const shownSections = $derived(groupSenseSections(shown, 0, !balanced));
  const hiddenSections = $derived(groupSenseSections(hidden, shown.length, !balanced));

  // What every tag on this word means, resolved in one call when the word changes.
  //
  // Up front rather than on click, because it decides whether a chip is a control at all.
  // Most of a dictionary's tags name a subject — `computing`, `golf`, `Buddhism` — and say
  // so in the word itself; offering those and then answering "no explanation yet" is worse
  // than leaving them as the plain labels they already are.
  let explained = $state<Record<string, TagExplanation>>({});

  $effect(() => {
    // The name's codes go in the same call: they are drawn as the same chips, and a chip
    // that is dead beside identical live ones reads as a bug rather than as "nothing to
    // say about this one".
    const codes = [
      ...new Set([
        ...(word?.inline ?? []).flatMap((sense) => sense.tags),
        ...(word?.name?.tags ?? []),
        // The disclosed senses' codes too, now that they are drawn as chips. A chip that is
        // dead beside identical live ones reads as a bug rather than as "nothing to say
        // about this one" — the same reason the name's codes are in this call.
        ...(word?.also_in ?? []).flatMap((source) =>
          source.senses.flatMap((sense) => sense.tags),
        ),
      ]),
    ];
    if (codes.length === 0) {
      explained = {};
      return;
    }
    let current = true;
    explainTags(codes).then((found) => {
      // A slow answer for a word that is no longer selected must not land on the one
      // that is.
      if (current) explained = found;
    });
    return () => {
      current = false;
    };
  });

</script>

{#if loading}
  <p class="hint"><span class="spinner" aria-hidden="true"></span> Looking up…</p>
{/if}

<!-- A name, when a name dictionary has this word.

     Its own block rather than another sense, because it answers a different question —
     *who is this* rather than *what does this mean* — and because it is the answer the
     reader wants when a character is being addressed by name. `leads` decides whether it
     comes before the senses or after: the analyzer sets it when it barely believed its own
     reading, which is what セイア (a name folded onto 井蛙, "frog in a well") looks like.

     Attributed for the same reason a promoted definition is: it is somebody else's data. -->
{#snippet nameBlock(name: NonNullable<WordView["name"]>)}
  <!-- Drawn through `SenseRow`, the same component every definition uses, so a name wears
       the chip a sense wears and starts at the same left edge. It had its own layout and
       read as a different kind of object on the page; a name is a different *answer*, not a
       different kind of thing.

       Unnumbered, deliberately. The word's senses are an ordered list of what it can mean,
       and a name is not item one of that list — numbering it would put two "1."s on screen
       claiming to enumerate the same thing. -->
  {@const separator = readingSeparator(name.glosses)}
  <div class="name">
    {#if componentOf(name.retrieved) !== null}
      <p class="component-scope">For the component <span lang="ja">{componentOf(name.retrieved)}</span></p>
    {/if}
    <SenseRow
      sense={{ tags: name.tags, glosses: name.glosses, sequence: null }}
      {explained}
      {opened}
      ontoggle={toggle}
      keyFor={(code) => `name:${code}`}
      n={null}
      source={showSource ? name.source : null}
      {separator}
    />
  </div>
{/snippet}

<!-- The name, when it is the answer.
     `leads` is set when the analyzer barely believed its own reading — セイア folded onto
     井蛙, "frog in a well" — and in that case the name is what the reader came for. It is not
     folded away: a disclosure over the one useful thing on screen is a disclosure over the
     answer. -->
{#if word?.name?.leads}{@render nameBlock(word.name)}{/if}

{#snippet senseRow(listed: Listed, index: number, omit: string[], source: string | null)}
      {@const isFavourite = balanced && contextual.favourite === index}
      <li>
        <SenseRow
          sense={listed.sense}
          {explained}
          {opened}
          ontoggle={toggle}
          keyFor={(code) => `${index}:${code}`}
          n={index + 1}
          {source}
          {omit}
          qualifier={isFavourite
            ? { label: contextLabel === "phrase" ? "Likely in this phrase" : "Likely here", title: contextual.evidence.join(" ") || undefined }
            : null}
        />
      </li>
{/snippet}

<!-- One stable grammatical subgroup, beneath whatever the whole source section already said. -->
{#snippet senseGroup(
  group: SenseGroup,
  section: SenseSection,
  source: string | null,
  sectionHasHeading: boolean,
)}
  {@const hasHeading = group.sharedTags.length > 0}
  {@const groupKey = `group:${group.start}`}
  {#if hasHeading}
    <SenseGroupHeading
      tags={group.sharedTags}
      {explained}
      {opened}
      ontoggle={toggle}
      keyFor={(code) => `${groupKey}:${code}`}
    />
  {/if}
  <ol class="senses" start={group.start + 1}>
    {#each group.items as listed, within}
      {@const omit = [...section.sharedTags, ...group.sharedTags]}
      {@render senseRow(
        listed,
        group.start + within,
        omit,
        !sectionHasHeading && !hasHeading && group.start === section.start && within === 0
          ? source
          : null,
      )}
    {/each}
  </ol>
{/snippet}

<!-- One source's senses. Its common tags are the outer umbrella; groups below can add a
     narrower one without repeating or contradicting it. -->
{#snippet senseSection(section: SenseSection)}
  {@const source = section.credit ?? (section.start === 0 ? inlineCredit : null)}
  {@const hasHeading = section.sharedTags.length > 0 || (source !== null && section.items.length > 1)}
  {#if section.component !== null}
    <p class="component-scope">For the component <span lang="ja">{section.component}</span></p>
  {/if}
  {#if hasHeading}
    <SenseGroupHeading
      tags={section.sharedTags}
      {source}
      {explained}
      {opened}
      ontoggle={toggle}
      keyFor={(code) => `section:${section.start}:${code}`}
    />
  {/if}
  {#each section.groups as group}
    {@render senseGroup(group, section, source, hasHeading)}
  {/each}
{/snippet}

{#if word && word.inline.length > 0}
  <!-- The same headings a character's detail uses, so a word and a kanji read alike. Without
       them a word ran spellings, meanings, examples and name readings together as one column of
       text whose only landmarks were the classification chips. -->
  {#if balanced}<h4 class="section">Meanings</h4>{/if}
  {#each shownSections as section}{@render senseSection(section)}{/each}
  {#if balanced && contextual.ambiguous}
    <p class="context-note">Several meanings fit this {contextLabel}.</p>
  {/if}
  <!-- One fold for everything else this word can mean.
       The analyzer's remaining senses and whatever other dictionaries add, counted together
       and numbered on from the list above. They were two disclosures — "4 more senses" here
       and "also in Jitendex.org" at the foot of the word, past the examples — which put the
       senses of one word in two places and made the reader open both to see them all. -->
  {#if hidden.length > 0}
    <details
      class="extra"
      open={balanced ? meaningsExpanded : undefined}
      ontoggle={(event) => {
        if (balanced) ontoggleMeanings(event.currentTarget.open);
      }}
    >
      <summary>{hidden.length} more sense{hidden.length === 1 ? "" : "s"}</summary>
      <!-- A disclosure's contents need the same first-step gap as the examples below. Without
           this wrapper the first source/classification badge sat directly against the summary,
           even though the badge's own padding was identical to every other one. The visible
           mismatch belonged to the disclosure flow, not to `SourceBadge`. -->
      <div class="extra-senses">
        {#each hiddenSections as section}{@render senseSection(section)}{/each}
      </div>
    </details>
  {/if}
{:else if !loading && !word?.likely_name && !word?.name}
  <!-- Neither a name nor the likely-name guess: those two *are* answers, and printing "No
       definition found" over one of them contradicts the block directly above it. A name
       arrives with no senses whenever the analyzer has none of its own — every word, under
       the built-in analyzer — so this is the ordinary case there, not a corner. -->
  <p class="hint">
    No definition found.
    {#if !hasDictionaries}
      Run <code>scripts/fetch-dictionaries.ps1</code> to add dictionaries.
    {/if}
  </p>
{/if}

<!-- A katakana word with no trustworthy answer.

     Said as the guess it is: unknown katakana in manga is usually a character's name, but
     it is sometimes an effect word, and the page cannot tell. It sits *after* any senses
     rather than instead of them, because both can be true at once — シロコ reaches 白子
     ("soft roe") by folding its kana, and that gloss is real while also being almost
     certainly not what the panel meant. Deleting it would be as much of a lie as asserting
     it, so the caveat goes beside it.

     The wording holds either way: nothing is *spelled* this way in either case. -->
{#if word?.likely_name}
  <p class="hint">
    No dictionary has this spelling — likely a name{#if word.romaji}, read
      <span class="reading-name">{word.romaji}</span>{/if}.
  </p>
{/if}

<!-- Offered under the senses when the analyzer was sure of the word: アカリ is genuinely
     both 明かり and Akari, and picking for the reader would be guessing.

     **Behind a click, because here it is a footnote and not the answer.** 水 is read sixteen
     ways as a name, which is four lines of kana under a word whose meaning is one — the name
     outweighed the meaning on a page that exists for the meaning. Folded, it costs one line
     and says what it is in that line.

     There is no fold *inside* it any more. Splitting sixteen readings into eight and a "more
     readings" disclosure put the same list in two places, and once the block itself opens
     and shuts the inner one had nothing left to do. -->
<!-- Examples belong to the word, not to whichever source supplied them.

     **Above the name readings, and open.** A sentence using the word is the closest thing on
     this page to seeing it used, which is what a reader is here for; the readings are a
     footnote about who else is called this. The order said the opposite, and the fold made
     the useful one cost a click while the footnote sat above it.

     Still a `<details>`: open is a default, not a decision, and eight sentences is a lot of
     page to owe someone who only wanted the meaning. -->
{#if word && word.examples.length > 0}
  {#if balanced}<h4 class="section">Examples</h4>{/if}
  {#if balanced}
    <!-- The closest usage evidence is immediate; only the remainder asks for another action. -->
    <div class="lead-example">
      {#if componentOf(word.examples[0].retrieved) !== null}
        <p class="component-scope">For the component <span lang="ja">{componentOf(word.examples[0].retrieved)}</span></p>
      {/if}
      <ExampleText
        japanese={word.examples[0].japanese}
        english={word.examples[0].english}
        ruled
      />
    </div>
    {#if word.examples.length > 1}
      <details
        class="extra"
        open={examplesExpanded}
        ontoggle={(event) => ontoggleExamples(event.currentTarget.open)}
      >
        <summary>{word.examples.length - 1} more example{word.examples.length === 2 ? "" : "s"}</summary>
        <ul class="examples">
          <!-- Analysis promises a lossless depth control: the fold changes when the examples
               appear, not whether they exist. Reference keeps its established eight-example
               ceiling below because one search can render dozens of independent entries. -->
          {#each word.examples.slice(1) as example}
            <li>
              {#if componentOf(example.retrieved) !== null}
                <p class="component-scope">For the component <span lang="ja">{componentOf(example.retrieved)}</span></p>
              {/if}
              <ExampleText japanese={example.japanese} english={example.english} ruled />
            </li>
          {/each}
        </ul>
      </details>
    {/if}
  {:else}
    <details class="extra" open>
      <summary>{word.examples.length} example{word.examples.length === 1 ? "" : "s"}</summary>
      <ul class="examples">
        {#each word.examples.slice(0, 8) as example}
          <li>
            {#if componentOf(example.retrieved) !== null}
              <p class="component-scope">For the component <span lang="ja">{componentOf(example.retrieved)}</span></p>
            {/if}
            <ExampleText japanese={example.japanese} english={example.english} ruled />
          </li>
        {/each}
      </ul>
    </details>
  {/if}
{/if}

{#if word?.name && !word.name.leads}
  <details class="extra">
    <summary>
      {word.name.glosses.length} name reading{word.name.glosses.length === 1 ? "" : "s"}
    </summary>
    {@render nameBlock(word.name)}
  </details>
{/if}

<style>
  /* Still an `<ol>`, and still numbered — by anything reading the page aloud. What is gone
     is the *drawn* marker: it sits on an item's first line box, which is the badge row, so it
     numbered what a sense is rather than what it means. `SenseRow` draws the visible number
     against the definition instead, and hides it from the accessibility tree so the list's
     own count is not announced twice.

     `padding-left` goes with the marker it was making room for; the number now lives inside
     the row and brings its own column. */
  .senses {
    margin: 0;
    padding-left: 0;
    list-style: none;
    color: var(--text);
    font-size: calc(1rem * var(--panel-scale, 1));
  }
  .senses li {
    margin-bottom: 0.4rem;
  }
  .hint {
    color: var(--text-faint);
    font-size: 0.88rem;
  }
  .context-note {
    width: fit-content;
    max-width: 100%;
    margin: var(--gap-sm) 0 0;
    border-radius: var(--radius-sm);
    padding: 0.2rem 0.45rem;
    color: var(--text-dim);
    background: var(--surface-raised);
    font-size: 0.76rem;
  }
  .component-scope {
    margin: 0 0 var(--gap-xs);
    color: var(--text-dim);
    font-size: 0.8rem;
  }

  /* Nothing of its own beyond the spacing that separates it from the senses — the row
     inside is a `SenseRow` and is styled there, which is the point of drawing it with one. */
  .name {
    margin-bottom: var(--gap-sm);
    font-size: calc(1rem * var(--panel-scale, 1));
  }

  /* A romanisation is *how it is said*, not what it means. */
  .reading-name {
    color: var(--text);
    font-style: italic;
  }
  .extra {
    margin-top: var(--gap);
    font-size: 0.88rem;
  }
  .extra summary {
    cursor: pointer;
    color: var(--text-faint);
  }
  .extra summary:hover {
    color: var(--text-dim);
  }
  /* The same opening gap `.examples` uses. Kept on a wrapper this component owns rather than
     reaching through Svelte's style boundary into `SenseGroupHeading`. It also covers the case
     where the folded tail begins with a plain sense and therefore has no heading to carry it. */
  .extra-senses {
    margin-top: var(--gap-sm);
  }
  .examples {
    margin: var(--gap-sm) 0 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: var(--gap-sm);
  }
  .examples li {
    margin: 0;
  }
  .lead-example {
    margin-top: var(--gap);
  }
  code {
    font-family: var(--font-mono);
    background: var(--surface-raised);
    padding: 0.05rem 0.3rem;
    border-radius: var(--radius-sm);
    font-size: 0.85em;
  }
</style>
