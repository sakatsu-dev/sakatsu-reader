<script lang="ts">
  // One dictionary sense: its badges, then what it means.
  //
  // Extracted from WordBody for the reason WordBody was itself extracted from Vocabulary —
  // so a second place showing a sense does not grow a second implementation that drifts.
  // The reference page's fused particle list needed exactly this row and reimplemented it
  // with plain `<span>`s, which cost the badges both their styling (Svelte scopes styles
  // per component, so the classes matched nothing there) and their behaviour: every chip
  // was dead where the same chip elsewhere opens an explanation.
  //
  // Resolution and open-state stay with the caller. A word's tags are explained in one
  // call because the chips are drawn together, and only one note is open at a time because
  // several stacked under one entry is the clutter the feature exists to avoid — neither
  // of those can be decided from inside a single row.
  import type { MergedSense, TagExplanation } from "../model";
  import SourceBadge from "./SourceBadge.svelte";
  import TagChips from "./TagChips.svelte";
  import TermNote from "./TermNote.svelte";
  import { kindColour, leadingKind } from "./word-kinds.js";

  let {
    sense,
    explained,
    opened,
    ontoggle,
    keyFor,
    n = null,
    source = null,
    separator = "; ",
    omit = [],
    qualifier = null,
  }: {
    sense: MergedSense;
    /** What each tag means, for the tags this row carries. Missing is normal. */
    explained: Record<string, TagExplanation>;
    /** Which note is open, across every row the caller draws. */
    opened: string | null;
    ontoggle: (key: string) => void;
    /** Names this row's note so two rows carrying `n` do not open together. */
    keyFor: (code: string) => string;
    /**
     * Which sense this is, counting from 1 and continuing across the "more senses" fold.
     *
     * Drawn here rather than left to the list marker. A marker sits on the *first* line box
     * of its item, and the first line is the badges — so the number ended up labelling what
     * the sense *is* rather than what it means. The `<ol>` keeps its own numbering for
     * anything reading the page aloud; this is the visible one, and it is `aria-hidden` so
     * the two are never announced together.
     *
     * **Null where the caller's list already draws its own.** The reference page's particle
     * readings are an `<ol>` with its markers left on, and a row that numbered itself there
     * would be numbered twice.
     */
    n?: number | null;
    /**
     * What goes between the things this row lists.
     *
     * A semicolon for a definition, because that is what separates two English senses of one
     * word. A name block passes `・`: its list is not meanings but *readings*, kana against
     * kana, and semicolons between sixteen of them read as one grey block. `・` is the mark
     * the page already uses to separate alternative spellings on the `also 阡・仟` line, so
     * this borrows an idiom rather than inventing one.
     */
    separator?: string;
    /**
     * A dictionary to name at the end of the chip row, or null for none.
     *
     * In the row rather than floating above or beside it: a source is another chip, and
     * chips already have somewhere to live. It keeps `SourceBadge`'s quieter styling — no
     * mono face — because it is a name, not one of the dictionary's own codes.
     *
     * Belongs to a *group* of senses, not to one, so callers pass it on the first row only;
     * repeating it down a word with four senses is the noise the chip is meant to avoid.
     */
    source?: string | null;
    /**
     * Tags the caller has already printed above this row, so it does not print them again.
     *
     * 有る carries its irregular godan class on all six senses, `intransitive` on one through
     * five and `auxiliary verb` on six. Its caller hoists the verb class above the section and
     * transitivity above the five-sense subgroup, then passes both here. What remains is what
     * actually distinguishes one sense from its neighbours — `auxiliary verb` on the sixth,
     * and usually nothing on the first five.
     *
     * The notes below still cover every tag the sense has, hoisted or not: a chip drawn
     * elsewhere is still explained where it was drawn, and the caller owns that row's notes.
     */
    omit?: string[];
    /** Analysis-only confidence beside the authoritative sense; absent on Reference. */
    qualifier?: { label: string; title?: string } | null;
  } = $props();

  /** This sense's part of speech, or `null` where its tags name none we recognise. */
  const kind = $derived(leadingKind(sense));

  /// What this row still has to say for itself.
  const mine = $derived(sense.tags.filter((code) => !omit.includes(code)));

</script>

<!-- A wrapper rather than the <li> itself: any `display` other than `list-item` stops it
     being one, and the number beside it disappears with the marker. That held when this was
     `flex` and still holds now it is `block`. -->
<div class="sense" class:numbered={n !== null} class:classified={kind !== null} style={kind ? `--kind: ${kindColour(kind)}` : undefined}>
  <!-- The row exists for either — a sense with no tags but a source still needs somewhere
       to put it, and one with tags and no source is the ordinary case. -->
  {#if mine.length > 0 || source}
    <span class="tags">
      <TagChips tags={mine} {explained} {opened} {ontoggle} {keyFor} />
      <!-- Last, after the codes: it answers where this came from, which is worth less than
           what it is. -->
      {#if source}<SourceBadge title={source} />{/if}
    </span>
  {/if}
  {#if n !== null}
    <span class="n" aria-hidden="true">{n}.</span>
  {/if}
  <span class="glosses">{sense.glosses.join(separator)}</span>
  {#if qualifier}
    <span class="qualifier" title={qualifier.title}>{qualifier.label}</span>
  {/if}
</div>

<!-- Under the whole sense, not inside the row of badges: `.tags` is an inline row, so a
     note rendered in there would line up beside the chips. -->
{#each sense.tags as code}
  {#if opened === keyFor(code)}
    <TermNote id={code} term={explained[code]} />
  {/if}
{/each}

<style>
  /* The badges get their own line, above the meaning.
   *
   * They were a baseline flex row beside the glosses, which read as a prefix to the first
   * line and then left the rest of a long definition hanging under an indent that meant
   * nothing. What a sense *is* and what it *means* are two statements, so they get a line
   * each, and every definition starts at the same left edge.
   *
   * A block, not a column flex: as a flex child `.glosses` needed `flex: 1; min-width: 0`
   * to stop a long unbroken gloss pushing the column wider than the panel, and in normal
   * flow it simply wraps. Those two declarations go with the row that needed them. */
  .sense {
    display: grid;
    grid-template-columns: 1fr;
  }
  /* **A hairline in this sense's part of speech.** 何 has eleven meanings running across pronoun,
     noun, adverb, interjection and prefix; in one colour they are eleven lines a reader has to
     read to tell apart. The chips already say *which* classification — either on the row or
     lifted into a group heading above it — so this adds no words, only the shape.
     On the row rather than only on the heading, because a heading is drawn only where a kind
     covers several senses: a lone adverb between two pronouns has no heading to colour, and it is
     exactly the change of kind a reader most needs to see. */
  /* **The same inset an example entry has**, so the two lists under one card start on one line.
     `ExampleText`'s `.ruled` is `border-left: 2px` plus `0.7rem` of padding, and a border takes
     layout where this rule's inset `box-shadow` does not — so matching the padding alone would
     leave the senses two pixels shy of the examples. The 2px is added back deliberately; it is
     the whole difference between the two ways of drawing the same hairline.

     On `.sense` rather than on the badges or the number, because those two move as a pair:
     indenting either alone leaves the sense with two left edges, which was tried first. */
  .sense {
    padding-left: var(--entry-inset);
  }
  .sense.classified {
    box-shadow: inset var(--entry-rule) 0 var(--kind);
  }
  /* The gutter exists only when something is in it: an `auto` column with no content is zero
     wide, but its `column-gap` is not, and an unnumbered row would sit 0.4rem in from the
     rest for no reason anyone could see. */
  .sense.numbered {
    grid-template-columns: auto 1fr;
    column-gap: 0.4rem;
    /* The number is 0.8em against the definition's 1em, so aligning their boxes would leave
       it riding high. Grid baselines are per row, and the badges are alone in theirs, so
       this only ever pairs the number with the line it numbers. */
    align-items: baseline;
  }
  /* The number hangs in the gutter beside the line it numbers. The badges span both columns
     above it, so nothing is indented past it except the definition itself. */
  .n {
    grid-column: 1;
    grid-row: 2;
    /* What the list marker used to look like, kept so nothing changed but the position. */
    color: var(--text-faint);
    font-size: 0.8em;
    line-height: 1.55;
    font-variant-numeric: tabular-nums;
  }
  /* Full width by default, and only moved aside when there is a number to sit beside.
   *
   * **These two rules are a pair with `grid-template-columns` above and must agree.** They
   * did not: `grid-column: 2` was unconditional while the second column only exists under
   * `.numbered`, so an unnumbered row put its meaning in an *implicit* column beside a `1fr`
   * first one — and the text landed hard against the right edge. It went unnoticed because
   * the only unnumbered caller was the reference page's particle readings; naming a word
   * with one made it obvious. */
  .glosses {
    grid-column: 1 / -1;
    grid-row: 2;
    line-height: 1.55;
  }
  .sense.numbered .glosses {
    grid-column: 2;
  }
  .qualifier {
    grid-column: 1 / -1;
    width: fit-content;
    margin-top: 0.18rem;
    border-radius: var(--radius-sm);
    padding: 0.08rem 0.32rem;
    color: var(--accent);
    background: var(--accent-wash);
    font-size: 0.67rem;
    line-height: 1.4;
    white-space: nowrap;
  }
  /* Still a row among themselves, and still `wrap` — a sense can carry five badges, and on
     the vocabulary panel's narrow column they have to be able to break.
     One rule. There were two `.tags` blocks a dozen lines apart, one placing it in the grid
     and one laying it out; same trap as the two `.reading` blocks in `Reference`. */
  .tags {
    /* The whole width, so the badges begin at the entry's left edge — level with the number
       below them. Both are moved together by `.sense`'s own padding; indenting the badges
       alone was tried and left the number behind, which is two left edges rather than one. */
    grid-column: 1 / -1;
    grid-row: 1;
    display: flex;
    flex-wrap: wrap;
    /* The spacing the spellings row uses, which is what these sit under. They were
       `--gap-xs` against its `--gap-sm` — 3.75px against 6.75px — so two rows of chips a
       line apart were packed differently for no reason either of them stated. */
    gap: var(--gap-xs) var(--gap-sm);
    margin-bottom: 0.15rem;
  }

</style>
