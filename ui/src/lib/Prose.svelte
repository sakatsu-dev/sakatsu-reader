<script lang="ts">
  // A sentence with the grammar terms in it offered quietly.
  //
  // Nothing is marked unless the glossary has it, nothing is explained unless it is
  // clicked, and a term is offered once per passage however often it is said. See
  // `prose.ts` for what counts as a term worth marking — the short codes a badge uses are
  // deliberately not, because "id" and "int" are not English words and "past" and "noun"
  // are too common to underline every time.
  import type { Term } from "../model";
  import { mark } from "./prose";
  import TermButton from "./Term.svelte";
  import TermNote from "./TermNote.svelte";

  let {
    text,
    terms,
    look,
  }: {
    text: string;
    /** The glossary. Empty until it has loaded, which simply means nothing is marked yet. */
    terms: Term[];
    look: (id: string) => Promise<Term | null>;
  } = $props();

  const parts = $derived(mark(text, terms));
  let open = $state<string | null>(null);

  // Closed whenever the sentence changes. Svelte reuses this instance for the next
  // selection's explanation, and an open note would otherwise stay put under a paragraph
  // that never offered the term — an answer to a question nobody asked here.
  $effect(() => {
    void text;
    open = null;
  });
</script>

<p>
  {#each parts as part}
    {#if part.id}
      <TermButton open={open === part.id} onclick={() => (open = open === part.id ? null : part.id!)}>
        {part.text}
      </TermButton>
    {:else}{part.text}{/if}
  {/each}
</p>

<!-- After the paragraph, not inside it: an inline button cannot contain a block, and the
     explanation wants the full width rather than the space left on one line. -->
{#if open}
  <TermNote id={open} {look} />
{/if}

<style>
  p {
    margin: 0;
  }
</style>
