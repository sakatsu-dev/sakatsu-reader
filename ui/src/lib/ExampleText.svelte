<script lang="ts">
  let {
    japanese,
    english = "",
    ruled = false,
    muted = false,
  }: {
    japanese: string;
    english?: string;
    /** Vocabulary examples need their own reading guide; grammar cards already supply one. */
    ruled?: boolean;
    /** Alternative particle readings keep the subdued treatment of their surrounding card. */
    muted?: boolean;
  } = $props();
</script>

<!-- One shared Japanese/English hierarchy. The surrounding feature decides whether this pair
     needs its own guide, so grammar cards do not acquire a distracting rule inside a rule. -->
<p class="example" class:ruled class:muted>
  <span class="japanese" lang="ja">{japanese}</span>
  {#if english}<span class="english">{english}</span>{/if}
</p>

<style>
  .example {
    margin: 0.15rem 0;
    color: var(--text);
    font-size: calc(0.9rem * var(--panel-scale, 1));
    line-height: 1.45;
  }
  .example.ruled {
    margin: 0;
    /* The rule takes layout here, so the text lands at `--entry-inset` without adding it. */
    padding: 0.1rem 0 0.1rem var(--entry-gap);
    border-left: var(--entry-rule) solid var(--border);
  }
  .example.muted {
    color: var(--text-dim);
  }
  .japanese,
  .english {
    display: block;
  }
  /* **A serif, so a quoted sentence stops reading like another definition.** An example and a
     meaning were the same face at nearly the same size, one under the other, and the only thing
     separating them was the rule down the left. A different voice does the work the rule was
     being asked to do alone. See `--font-jp-serif`. */
  .japanese {
    font-family: var(--font-jp-serif);
  }
  .english {
    color: var(--text-dim);
    font-size: 0.85rem;
  }
</style>
