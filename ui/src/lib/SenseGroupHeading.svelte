<script lang="ts">
  // One heading over a section or consecutive subgroup of dictionary senses.
  //
  // WordBody and the reference page's particle/dictionary fusion both draw sense lists.
  // Keeping this row here makes "say a repeated classification once" one visual and
  // behavioural rule in both places and at both nesting levels: the same chips, source credit,
  // open-note keying and spacing. A copied row already drifted once; this component is the
  // boundary that keeps another sense-list caller from doing it again.
  import type { TagExplanation } from "../model";
  import SourceBadge from "./SourceBadge.svelte";
  import TagChips from "./TagChips.svelte";
  import TermNote from "./TermNote.svelte";
  import { kindColour, leadingKind } from "./word-kinds.js";

  let {
    tags,
    source = null,
    explained,
    opened,
    ontoggle,
    keyFor,
  }: {
    tags: string[];
    source?: string | null;
    explained: Record<string, TagExplanation>;
    opened: string | null;
    ontoggle: (key: string) => void;
    keyFor: (code: string) => string;
  } = $props();

  /**
   * What part of speech these senses are, and the colour that says so.
   *
   * The grouping is already done — `groupSenseSections` collects consecutive senses sharing a
   * leading classification, and this row is drawn once per group. So the kind is named exactly
   * where it changes, and the rule beside it turns eleven undifferentiated meanings into runs a
   * reader can tell apart before reading any of them. 何 is the case: pronoun, noun, adverb,
   * interjection and prefix, in one column, in one colour.
   *
   * `null` for a group whose tags name no kind we know — about a third of the dictionary — and
   * that draws no rule at all rather than a neutral one, because an absent classification should
   * look absent.
   */
  const kind = $derived(leadingKind({ tags }));
</script>

<div class="heading" class:classified={kind !== null} style={kind ? `--kind: ${kindColour(kind)}` : undefined}>
  <TagChips {tags} {explained} {opened} {ontoggle} {keyFor} />
  {#if source}<SourceBadge title={source} />{/if}
</div>

<!-- Under the row rather than in it: `.heading` wraps like a line of chips, and a note
     rendered inside would sit beside them as one more chip-shaped thing. -->
{#each tags as code}
  {#if opened === keyFor(code)}
    <TermNote id={code} term={explained[code]} />
  {/if}
{/each}

<style>
  /* Exactly the row a sense gives its own chips. These are those same chips lifted above
     several meanings, so changing their packing here would make the umbrella look like a
     different kind of classification. */
  .heading {
    display: flex;
    flex-wrap: wrap;
    gap: var(--gap-xs) var(--gap-sm);
    margin-bottom: 0.3rem;
  }
  /* **The same inset as the senses it introduces, classified or not.** It carried `--gap-sm` while
     a sense carried `--entry-inset`, and an *unclassified* heading carried nothing at all — so a
     run of meanings could start six pixels left of the meanings, or thirteen, depending on whether
     its tags happened to name a kind the app knows. On the element rather than on `.classified`,
     because where a heading sits is not a question about whether it has a colour. */
  .heading {
    padding-left: var(--entry-inset);
  }
  /* A hairline in the kind's hue, and nothing else — the chips keep their own treatment, because
     they say *which* classification and this says only that one has started. The rule is an inset
     shadow, so it costs no layout and the padding above is unaffected by its presence. */
  .heading.classified {
    box-shadow: inset var(--entry-rule) 0 var(--kind);
  }
</style>
