<script lang="ts">
  import type {
    Breakdown,
    ExpressionOffer,
    GrammarPattern,
    Match,
    RankedSense,
    Reading,
    Step,
    Term,
    Token,
  } from "../model";
  import { FORM_TITLES, orderGrammarBlocks } from "./grammar-order.js";
  import TermButton from "./Term.svelte";
  import TermNote from "./TermNote.svelte";
  import Prose from "./Prose.svelte";
  import ExampleText from "./ExampleText.svelte";
  import { onMount } from "svelte";

  let {
    token,
    breakdown,
    patterns,
    reading,
    explainPattern,
    expression,
    expressionFailed = false,
    lookTerm,
    allTerms,
    alternativesExpanded = false,
    ontoggleAlternatives = () => {},
  }: {
    token: Token | null;
    breakdown: Breakdown | null;
    /** Sentence-level patterns covering the selected token. */
    patterns: Match[];
    /** Every sense of the selected particle. Null when the selection is not a particle. */
    reading: Reading | null;
    /**
     * The entry for an expression the grammar layer recognised around this selection.
     *
     * Null is the ordinary answer and means there is no offer here. It is shown **beside** the
     * word's own answer and never replaces it — the reader keeps every existing way of reading
     * the component.
     */
    expression: ExpressionOffer | null;
    /**
     * Whether the offer could not be fetched, as distinct from there being none.
     *
     * Separate from `expression` being null on purpose: a panel that showed nothing for both
     * would hide a dictionary that could not be read behind the far commoner "no offer".
     */
    expressionFailed?: boolean;
    explainPattern: (id: string) => Promise<GrammarPattern | null>;
    lookTerm: (id: string) => Promise<Term | null>;
    allTerms: () => Promise<Term[]>;
    /** Session-level disclosure memory supplied by the Analysis page. */
    alternativesExpanded?: boolean;
    ontoggleAlternatives?: (open: boolean) => void;
  } = $props();

  // The glossary, fetched once so the prose can be scanned for terms. Until it arrives
  // nothing is marked, which is the right way round: the sentence reads normally and
  // gains its offers a moment later rather than blocking on them.
  let glossary = $state<Term[]>([]);
  // `onMount` rather than a bare call: reading a prop at the top level captures the value
  // it had then, and this is a once-on-mount fetch rather than something that should
  // follow the prop.
  onMount(async () => {
    glossary = await allTerms();
  });

  // Which form is open, by its own id. The breakdown keys each step by `form`, and those
  // ids *are* the glossary's ids — so this looks a term up exactly rather than matching
  // the words it printed, and cannot pick the wrong one.
  let openForm = $state<string | null>(null);
  const toggleForm = (form: string) => (openForm = openForm === form ? null : form);

  // A note belongs to the word it was opened on. Left alone it would spring open again on
  // the next word that happens to carry the same form.
  $effect(() => {
    void token;
    openForm = null;
  });

  const empty = $derived(
    !breakdown || (breakdown.steps.length === 0 && breakdown.auxiliaries.length === 0),
  );

  /// The one pedagogical order across what used to be four unrelated blocks.
  ///
  /// This does not rank grammar by a school year or JLPT badge. When Rust finds a unique
  /// leader, every positive-score particle reading answers the selected sentence first; the
  /// leader stays visually strongest and the rest remain qualified as secondary support. The
  /// expression's own dependencies follow. In a compound that means the preceding component's
  /// form, the auxiliary it creates, and only then the auxiliary component's inflection —
  /// 伝わって → 〜てくる → きます, rather than putting "Non-past formal" before explaining
  /// what 来る is doing there. Anything unresolved remains at the end; nothing is filtered.
  const ordered = $derived(orderGrammarBlocks(breakdown, patterns, reading));
</script>

{#snippet particleSense(
  sense: RankedSense,
  priority: "favoured" | "supported" | "plain",
  surface: string,
)}
  <article class="sense" class:favoured={priority === "favoured"}>
    <h4>
      {#if priority !== "plain"}<span lang="ja" class="particle">{surface}</span>{/if}
      {sense.label}
      {#if priority === "supported"}<span class="jlpt">also supported</span>{/if}
    </h4>
    {#if priority === "favoured"}
      <p>{sense.gloss}</p>
    {:else}
      <Prose text={sense.gloss} terms={glossary} look={lookTerm} />
    {/if}
    {#if sense.because.length > 0}
      <ul class="because">
        {#each sense.because as reason}<li>{reason}</li>{/each}
      </ul>
    {/if}
    {#each sense.examples.slice(0, priority === "favoured" ? 2 : 1) as example}
      <ExampleText
        japanese={example.japanese}
        english={example.english}
        muted={priority !== "favoured"}
      />
    {/each}
  </article>
{/snippet}

<!-- The unpromoted readings, rendered the same whether they are folded behind a disclosure or
     standing open under a ranked one. A snippet rather than the same `{#each}` written twice:
     the two call sites differ only in what wraps them, and a second copy is a second thing to
     remember when `particleSense` grows an argument.

     `surface` is a parameter rather than read from `token` here, because a snippet declared at
     component scope cannot see the narrowing that the `{#if !token}` branch below performs. -->
{#snippet plainSenses(senses: RankedSense[], surface: string)}
  {#each senses as sense}
    {@render particleSense(sense, "plain", surface)}
  {/each}
{/snippet}

{#snippet formStep(step: Step)}
  <li>
    <div class="rung">
      {#if FORM_TITLES[step.form]}
        <TermButton open={openForm === step.form} onclick={() => toggleForm(step.form)}>
          <span class="title">{FORM_TITLES[step.form]}</span>
        </TermButton>
      {:else}
        <!-- A form the classifier does not know, so there is no term to look up and
             nothing to click: the analyzer's own wording stands as it is. -->
        <span class="title">{step.label}</span>
      {/if}
      <!-- Negation reverses the sentence, so it is marked in the attention colour rather
           than sitting quietly beside "formal". -->
      {#if step.neg}<span class="flag negative">negative</span>{/if}
      {#if step.fml}<span class="flag">formal</span>{/if}
      {#if step.is_base}<span class="flag base">on {step.lemma}</span>{/if}
    </div>
    {#if openForm === step.form}
      <TermNote id={step.form} look={lookTerm} />
    {/if}
    {#if step.formation}
      <div class="formation" lang="ja">
        {step.formation.from} → <strong>{step.formation.result}</strong>
        <span class="change">{step.formation.change}</span>
      </div>
    {/if}
  </li>
{/snippet}

{#snippet patternCard(pattern: GrammarPattern, source: "auxiliary" | "match")}
  <article class="pattern">
    <h4><span lang="ja">{pattern.pattern}</span>
      {#if pattern.jlpt}<span class="jlpt">{pattern.jlpt}</span>{/if}
    </h4>
    <Prose text={pattern.explanation} terms={glossary} look={lookTerm} />
    {#if source === "auxiliary" && pattern.formation}
      <p class="how">{pattern.formation}</p>
    {/if}
    {#if source === "match" && pattern.examples.length > 0}
      <ExampleText
        japanese={pattern.examples[0].japanese}
        english={pattern.examples[0].english}
      />
    {/if}
    {#if source === "match" && pattern.reference}
      <a href={pattern.reference.url} target="_blank" rel="noreferrer noopener">
        {pattern.reference.label}
      </a>
    {/if}
  </article>
{/snippet}

{#snippet expressionCard(offer: ExpressionOffer)}
  <!-- **Beside the word, not instead of it.** The heading names the expression and the line under
       it says how the selection stands to it, so it is plain that の is still の and this is an
       additional reading of the pair it belongs to. -->
  <article class="expression">
    <h4><span lang="ja">{offer.expression}</span></h4>
    <!-- **From the span, not the spelling.** The core compares the selected token's validated
         range with the offer's and reports which way round they sit; the panel only names it.
         Four cases, because there are four: the selection is the expression, the expression sits
         inside the selection (Ichiran fuses 映画とか into one token), the selection is part of the
         expression, or the two overlap without either containing the other — which happens when
         an ending begins inside a token that also holds text before it. Telling a reader who
         selected 映画とか that the offer "contains" their selection would be exactly backwards,
         and saying it of an overlap would be untrue of both halves.

         `unknown` renders nothing: an ungrounded selection, or one that does not touch the
         offer, supports no sentence about how the two stand. -->
    {#if offer.extent !== "unknown"}
      <p class="relationship">
        {offer.extent === "same"
          ? "Expression."
          : offer.extent === "within_selection"
            ? "Expression within this selection."
            : offer.extent === "overlapping"
              ? "Expression overlapping this selection."
              : "Expression containing this selection."}
      </p>
    {/if}
    <!-- An occurrence keeps the construction it belongs to in view. The offer is for this とか,
         not for the list — it claims only its own span — so the list is named here rather than
         highlighted, and the reader can still reach its explanation among the patterns below. -->
    {#if offer.occurrence}
      <p class="caveat context-note">
        One occurrence in <span lang="ja">{offer.title}</span>.
      </p>
    {/if}
    <ul class="expression-senses">
      {#each offer.senses as sense}
        <li>
          {#if sense.tags.length > 0}<span class="tags">{sense.tags.join(", ")}</span>{/if}
          {sense.glosses.join("; ")}
        </li>
      {/each}
    </ul>
    <!-- No favourite is claimed, and the wording says why rather than hedging. のに carries both
         the concessive and the "in order to" reading; nothing here has weighed the context, so
         every sense is shown, none is promoted, and the reader is told that plainly. -->
    <p class="caveat context-note">Dictionary senses; no contextual preference established.</p>
    <p class="source">{offer.source}</p>
  </article>
{/snippet}

<section class="grammar">
  {#if !token}
    <p class="hint">Select a word in the text above.</p>
  {:else if empty && patterns.length === 0 && !reading && !expression && !expressionFailed}
    <p class="hint">
      <span lang="ja">{token.surface}</span> is a plain dictionary word — no grammar to explain.
    </p>
  {:else}
    <!-- First, because it is about the largest thing the selection belongs to: a reader who
         selected の in 可愛いのに wants to know they are inside のに before they read の's own
         senses. -->
    {#if expression}{@render expressionCard(expression)}{/if}
    {#if expressionFailed}
      <p class="caveat context-note">
        The expression entry could not be read. The word's own senses are unaffected.
      </p>
    {/if}
    {#each ordered as block}
      {#if block.kind === "prioritized-readings"}
        <div class="senses">
          {#each block.readings as reading}
            {@render particleSense(reading.sense, reading.role, token.surface)}
          {/each}
          <p class="caveat context-note">
            Likely here, based on nearby words.
          </p>
        </div>
      {:else if block.kind === "steps"}
        <!-- Separate ordered lists let a construction sit where it is formed. `start` keeps
             the visible and accessible ladder numbering continuous across that explanation. -->
        <ol class="ladder" start={block.start}>
          {#each block.steps as step}{@render formStep(step)}{/each}
        </ol>
      {:else if block.kind === "patterns"}
        <div class="patterns">
          {#each block.items as item}
            {#if item.kind === "auxiliary"}
              {#await explainPattern(item.auxiliary.pattern_id) then pattern}
                {#if pattern}{@render patternCard(pattern, "auxiliary")}{/if}
              {/await}
            {:else}
              {#await explainPattern(item.match.pattern_id) then pattern}
                {#if pattern}{@render patternCard(pattern, "match")}{/if}
              {/await}
            {/if}
          {/each}
        </div>
      {:else if block.ambiguous}
        <!-- **The abstention state, folded.** `ambiguous` means the ranker promoted nothing, and
             `local documentation` measured that at 82.6% of occurrences — so this
             is the ordinary case, not the exception, and open it was six unranked readings pushing
             the rest of the grammar panel off the screen. The caveat *is* the summary: the line
             saying why nothing is ranked is the same line you press to see what was not ranked. -->
        <details
          class="unranked"
          open={alternativesExpanded}
          ontoggle={(event) => ontoggleAlternatives(event.currentTarget.open)}
        >
          <summary class="caveat context-note">Several readings fit this context.</summary>
          <!-- An explicit wrapper, because the grid cannot go on the `<details>` itself: the
               browser puts the open content in a `::details-content` box of its own, so the
               cards are that box's children rather than the details' grid items and every gap
               between them collapses to nothing. See `traps.md`. -->
          <div class="senses">
            {@render plainSenses(block.senses, token.surface)}
          </div>
        </details>
      {:else}
        <details
          class="unranked"
          open={alternativesExpanded}
          ontoggle={(event) => ontoggleAlternatives(event.currentTarget.open)}
        >
          <summary class="alternative-summary">
            {block.senses.length} alternative reading{block.senses.length === 1 ? "" : "s"}
          </summary>
          <div class="senses">
            {@render plainSenses(block.senses, token.surface)}
          </div>
        </details>
      {/if}
    {/each}
  {/if}

</section>

<style>
  /* The expression card. Deliberately close to `.pattern` — it is the same kind of thing, an
     explanation of something larger than the token — and separated from it only by taking the
     lighter `--accent` against the pattern's `--accent-strong`, so a reader can see at a glance
     which answers are about the word and which about the phrase.

     Its first version was written against a `--gap-1`…`--gap-4` scale this project does not
     have. Every one of those declarations resolved to nothing, which is why the card arrived
     with no padding and no separation at all; the scale here is
     `--gap-xs`/`--gap-sm`/`--gap`/`--gap-lg`, and greys are `--text-dim`/`--text-faint`. */
  .expression {
    border-left: 2px solid var(--accent);
    padding: 0.1rem 0 0.1rem 0.7rem;
  }
  .expression h4 {
    margin: 0;
    font-size: calc(1.05rem * var(--panel-scale, 1));
  }
  /* On its own line rather than beside the heading. It is a sentence, not a badge: in a baseline
     row "Expression containing this selection." either squeezes the heading or wraps into a
     ragged second column at the widths the grammar panel actually gets. */
  .expression .relationship {
    margin: 0 0 var(--gap-sm);
    font-size: calc(0.8rem * var(--panel-scale, 1));
    color: var(--text-faint);
  }
  .expression-senses {
    margin: 0;
    padding-left: 1.1rem;
    display: grid;
    gap: var(--gap-sm);
    font-size: calc(0.9rem * var(--panel-scale, 1));
    color: var(--text-dim);
  }
  /* Above the glosses, not in front of them. Inline, a tag reads as the first word of the
     definition — "particle although; despite" — and every sense of a particle carries the
     same one, so the repetition is pure noise where it can be mistaken for content. */
  .expression .tags {
    display: block;
    font-size: calc(0.78rem * var(--panel-scale, 1));
    color: var(--text-faint);
  }
  .expression .source {
    margin: var(--gap-sm) 0 0;
    font-size: calc(0.78rem * var(--panel-scale, 1));
    color: var(--text-faint);
  }

  .grammar {
    padding: var(--gap-sm) 0 var(--gap-lg);
  }

  .ladder {
    margin: 0;
    padding-left: 1.3rem;
  }
  .ladder li {
    margin-bottom: var(--gap-sm);
  }
  .ladder li::marker {
    color: var(--text-faint);
    font-size: 0.8em;
  }
  .rung {
    display: flex;
    align-items: baseline;
    gap: var(--gap-xs);
    flex-wrap: wrap;
  }
  .title {
    font-weight: 600;
  }
  /* `.flag`, `.flag.negative` and `.flag.base` are in `theme.css` now — the vocabulary panel
     wears one too. Deleted here rather than left behind: a scoped copy would outrank the
     shared rule and the two would drift with nothing to report it. See traps.md. */
  .formation {
    font-size: 0.95rem;
    color: var(--text-dim);
    margin-top: 0.15rem;
  }
  .formation strong {
    color: var(--text);
    font-weight: 600;
  }
  .formation .change {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--text-faint);
    margin-left: var(--gap-xs);
  }

  .senses {
    display: grid;
    gap: var(--gap-sm);
    margin-bottom: var(--gap);
  }
  /* The folded abstention list.

     `display: list-item` and **not** `flex`: flex on a `<summary>` deletes the disclosure
     triangle outright, which is the only thing on screen saying the section opens. That one is in
     `traps.md` because it has cost this project three separate times. */
  .unranked {
    margin-bottom: var(--gap);
  }
  /* The inner `.senses` carries its own bottom margin for the un-folded case; inside the fold the
     `<details>` above owns that space, so it is taken back here rather than doubled. */
  .unranked > .senses {
    margin: var(--gap-sm) 0 0;
  }
  .unranked > summary {
    display: list-item;
    list-style-position: inside;
    cursor: pointer;
  }
  .unranked > summary::marker {
    color: var(--text-faint);
  }
  .alternative-summary {
    color: var(--text-faint);
    font-size: 0.82rem;
  }
  .alternative-summary:hover {
    color: var(--text-dim);
  }
  .sense {
    border-left: 2px solid var(--border);
    padding: 0.1rem 0 0.1rem 0.7rem;
  }
  /* The one the sentence points at gets the accent the grammar patterns use; the rest
     stay in the neutral rule, so which is which reads at a glance without the others
     looking dismissed. */
  .sense.favoured {
    border-left-color: var(--accent-strong);
  }
  .sense h4 {
    margin: 0 0 0.2rem;
    font-size: calc(1.02rem * var(--panel-scale, 1));
    display: flex;
    align-items: baseline;
    gap: var(--gap-xs);
    flex-wrap: wrap;
  }
  .sense .particle {
    color: var(--accent);
    font-weight: 700;
  }
  .sense p {
    margin: 0.15rem 0;
    font-size: calc(0.9rem * var(--panel-scale, 1));
    color: var(--text);
  }
  .sense:not(.favoured) p {
    color: var(--text-dim);
  }
  .because {
    margin: 0.25rem 0 0;
    padding-left: 1.1rem;
    font-size: calc(0.82rem * var(--panel-scale, 1));
    color: var(--text-dim);
  }
  .because li::marker {
    color: var(--accent-edge);
  }
  /* Says out loud that this is inference from the surrounding words. A learner who
     believes it is a parse will trust it further than it can carry. */
  .caveat {
    margin: 0;
    font-size: calc(0.8rem * var(--panel-scale, 1));
    color: var(--text-faint);
  }
  /* These qualify the analyzer's evidence without becoming another full-width grammar card. */
  .caveat.context-note {
    justify-self: start;
    max-width: 100%;
    box-sizing: border-box;
    margin: var(--gap-sm) 0 0;
    padding: 0.3rem 0.55rem;
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    color: var(--text-dim);
    line-height: 1.35;
  }

  .patterns {
    margin-top: var(--gap);
    display: grid;
    gap: var(--gap-sm);
  }
  /* A compound explanation can now divide one numbered ladder. Match the space before the
     explanation when the ladder resumes after it; the `start` attribute, not CSS, carries
     the numbering across the gap. */
  .patterns + .ladder {
    margin-top: var(--gap);
  }
  .pattern {
    border-left: 2px solid var(--accent-strong);
    padding: 0.1rem 0 0.1rem 0.7rem;
  }
  .pattern h4 {
    margin: 0 0 0.2rem;
    font-size: calc(1.05rem * var(--panel-scale, 1));
    display: flex;
    align-items: baseline;
    gap: var(--gap-xs);
  }
  .jlpt {
    font-family: var(--font-mono);
    font-size: 0.63rem;
    padding: 0.05rem 0.3rem;
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    color: var(--text-faint);
  }
  .pattern p {
    margin: 0.15rem 0;
    /* Same scale as the definitions: an explanation and a gloss are read the same way. */
    font-size: calc(0.9rem * var(--panel-scale, 1));
    color: var(--text);
  }
  .how {
    color: var(--text-faint) !important;
    font-size: 0.8rem !important;
  }
  .hint {
    color: var(--text-faint);
    font-size: 0.88rem;
  }
  /* Smaller than the prose it hangs under — a source line, not a heading. The colour is in
     `theme.css` now, shared with the other two places that link out. */
  a {
    font-size: 0.8rem;
  }

</style>
