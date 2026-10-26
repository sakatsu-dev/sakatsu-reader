<script lang="ts">
  // The explanation a `Term` opens.
  //
  // A block in the flow rather than a card floating over one. Everything these appear
  // inside — the vocabulary column, the grammar column, the reference results — scrolls
  // and clips its overflow, so a positioned card would be cut off against a column edge,
  // and worst exactly where there is least room to spare.
  import type { Term } from "../model";
  import { openGlossary } from "./glossary-navigation";

  let {
    id,
    look,
    term,
  }: {
    id: string;
    /** Asked only when `term` is absent. */
    look?: (id: string) => Promise<Term | null>;
    /** An explanation already in hand — the caller resolved it to know whether to offer
        the control at all, so asking again would be a second trip for the same answer. */
    term?: Pick<Term, "id" | "label" | "summary" | "detail"> | { label: string; detail: string } | null;
  } = $props();

  const prose = (value: Pick<Term, "summary" | "detail"> | { detail: string }) =>
    "summary" in value
      ? value.detail ? `${value.summary} ${value.detail}` : value.summary
      : value.detail;
</script>

<div class="note" role="note">
  {#if term}
    <strong>{term.label}</strong>{prose(term)}
    {#if "id" in term}
      <button type="button" class="open-glossary" onclick={() => openGlossary(term.id)}>
        Open in Glossary
      </button>
    {/if}
  {:else if look}
    {#await look(id)}
      <span class="waiting">…</span>
    {:then found}
      {#if found}
        <strong>{found.label}</strong>{prose(found)}
        <button type="button" class="open-glossary" onclick={() => openGlossary(found.id)}>
          Open in Glossary
        </button>
      {:else}
        <span class="waiting">No explanation for “{id}” yet.</span>
      {/if}
    {/await}
  {/if}
</div>

<style>
  .note {
    margin: var(--gap-xs) 0 var(--gap-sm);
    padding: var(--gap-sm) var(--gap);
    border-left: 2px solid var(--accent-edge);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    background: var(--accent-wash);
    color: var(--text-dim);
    /* Set as prose whatever it was opened from: a badge is monospace at chip size, and
       the explanation should not inherit either. */
    font-family: var(--font);
    font-size: 0.85rem;
    font-weight: 400;
    line-height: 1.5;
    text-align: left;
    text-transform: none;
    letter-spacing: normal;
  }
  .note strong {
    color: var(--text);
    font-weight: 600;
  }
  .note strong::after {
    content: " — ";
    color: var(--text-faint);
    font-weight: 400;
  }
  .waiting {
    color: var(--text-faint);
  }
  .open-glossary {
    display: block;
    margin-top: var(--gap-xs);
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-faint);
    font: inherit;
    font-size: 0.78rem;
    text-decoration: underline;
    cursor: pointer;
  }
  .open-glossary:hover { color: var(--text); }
</style>
