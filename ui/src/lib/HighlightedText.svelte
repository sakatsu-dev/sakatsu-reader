<script lang="ts">
  // Safe text segments, never HTML. Search highlighting should not turn dictionary or glossary
  // content into markup merely because a matching substring came from data.
  import { glossaryHighlightSegments } from "./glossary-flow.js";

  let { text, needle }: { text: string; needle: string } = $props();
  const parts = $derived(glossaryHighlightSegments(text, needle));
</script>

{#each parts as part}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}

<style>
  mark { color: inherit; background: var(--accent-wash); border-radius: 0.12rem; }
</style>
