<!-- Presentation only. TranslationCard owns the comparison's heading, lifecycle, and actions;
     this component keeps the baseline and ablated result structurally inseparable. -->
<script lang="ts">
  import type { VariantTranslations } from "../model";

  import { comparisonFollowup } from "./translation-card.js";

  let { result }: { result: VariantTranslations } = $props();

  /// Straight from Rust — see `translate::compare` for why neither of these is computed here.
  const pieces = $derived(result.pieces);
  const followup = $derived(comparisonFollowup(result));
</script>

<section class="carries" aria-label="Focused comparison" data-testid="comparison-output">
  <p class="line struck" data-testid="comparison-baseline">
    {#each pieces as piece}<span class:gone={piece.gone}>{piece.text}</span>{" "}{/each}
  </p>

  {#if followup.kind === "all-struck"}
    <p class="line dim">Nothing is left — the whole sentence is struck.</p>
  {:else if followup.kind === "result"}
    <p class="line result"><span class="arrow" aria-hidden="true">→</span>{followup.text}</p>
  {/if}
</section>

<style>
  /* The surrounding card owns chrome and type scale; comparison output only owns its internal
     rhythm. This prevents it from developing a second, competing panel hierarchy. */
  .carries { min-width: 0; }

  .line {
    margin: 0 0 0.15rem;
  }
  .line.dim {
    color: var(--text-faint);
    font-size: 0.86em;
  }
  /* **The same declaration `.token.studied` makes in `Reader.svelte`**, and that is the point: a
     struck word and the struck English are now level with each other on screen, so anything less
     than an identical treatment would read as two different marks rather than one gesture. Marked
     rather than deleted, because a struck word is still readable and reading it is the whole
     point. */
  .struck .gone {
    text-decoration: line-through;
    text-decoration-color: var(--accent);
    text-decoration-thickness: 2px;
    color: var(--text-faint);
  }
  .result {
    font-weight: 600;
  }
  .arrow {
    margin-right: var(--gap-xs);
    color: var(--text-faint);
    font-weight: 400;
  }
</style>
