<script lang="ts">
  // Which dictionary a definition came from, as a chip.
  //
  // It was a sentence — "from Jitendex.org [2026-07-09]" — above every entry. On the reader
  // that is rare and says something: the analyzer had nothing for this word, so a source was
  // promoted, and naming it is how the learner knows where the answer came from (project notes
  // §6). On the reference page it fires on *every* result, because search goes straight to
  // the dictionaries and never asks the analyzer, so `primary` is always empty and everything
  // is promoted. A label repeated on all eighteen rows names nothing and costs a line each.
  //
  // So it is re-typed, not removed. **This is attribution, and attribution is a licence
  // condition** — Jitendex and JMnedict are CC BY-SA (project notes §8). The app-level credit in
  // the footer is what satisfies that, and this is the per-entry answer to "where did this
  // one come from"; neither is weakened by making it a chip instead of a sentence.
  import { sourceName } from "./sources";

  let {
    /** The dictionary's own title, as it declares itself. */
    title,
    /**
     * Whether this sits in a caller's top-right corner.
     *
     * **Only the tooltip's direction** — a chip against the right edge needs its bubble
     * anchored right or it overflows the panel. Lifting it out of the flow is no longer
     * done here: `WordBody` stacks the credits and positions the stack, because two of
     * them at `top: 0; right: 0` would land on top of each other.
     */
    corner = false,
  }: {
    title: string;
    corner?: boolean;
  } = $props();
</script>

<!-- The full title, dated and with its proper casing, stays in the tooltip. The visible label
     follows the lowercase convention of the classification badges beside it, so
     `Jitendex.org` does not look like a different kind of chip merely because it is a name. -->
<span class="source" data-tip={title} class:tip-right={corner}>
  {sourceName(title).toLowerCase()}
</span>

<style>
  /* The same chip a tag wears in `SenseRow`. It used to differ by dropping the mono face,
     on the argument that this is a name and a tag was a code; the tags are written out as
     names now and have dropped it too, so the two match outright. The rule the mono face
     states is unchanged and still has one holder: `SenseRow`'s `.pill`, the code nothing in
     the app can explain. Deliberately quiet — it answers a question rather than raising
     one. */
  .source {
    display: inline-block;
    font-size: 0.68rem;
    padding: 0.05rem 0.35rem;
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    color: var(--text-faint);
    /* A long title cannot push the entry's own layout around. */
    max-width: 12rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    vertical-align: middle;
  }

</style>
