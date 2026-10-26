<script lang="ts">
  // Analysis vocabulary is the authoritative contextual answer. Identity and word actions live
  // in the fixed header above it; no second summary repeats these meanings elsewhere.
  import type { TagExplanation, VocabularyRanking, WordView } from "../model";
  import WordBody from "./WordBody.svelte";

  let {
    word,
    ranking,
    loading,
    hasDictionaries,
    explainTags,
    showSource,
    meaningsExpanded,
    examplesExpanded,
    ontoggleMeanings,
    ontoggleExamples,
  }: {
    word: WordView | null;
    ranking: VocabularyRanking | null;
    loading: boolean;
    hasDictionaries: boolean;
    explainTags: (codes: string[]) => Promise<Record<string, TagExplanation>>;
    showSource: boolean;
    meaningsExpanded: boolean;
    examplesExpanded: boolean;
    ontoggleMeanings: (open: boolean) => void;
    ontoggleExamples: (open: boolean) => void;
  } = $props();
</script>

<section class="vocabulary">
  <WordBody
    {word}
    {ranking}
    {loading}
    {hasDictionaries}
    {explainTags}
    {showSource}
    balanced
    {meaningsExpanded}
    {examplesExpanded}
    {ontoggleMeanings}
    {ontoggleExamples}
  />
</section>

<style>
  .vocabulary { padding: var(--gap-sm) 0 var(--gap-lg); }
</style>
