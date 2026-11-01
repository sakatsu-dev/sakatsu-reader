<script lang="ts">
  // One authoritative concept detail for both Glossary and Reference. Relationships remain
  // short navigable lists: they support a learner's next question without turning the page into
  // a graph whose geometry competes with the explanation.
  import { TERM_KINDS, type GrammarPattern, type Term } from "../model";

  let {
    term,
    terms = [],
    patternsForTerm,
    onselect,
    onopen,
    embedded = false,
    editorial = false,
    showSummary = true,
    similarTerms = false,
  }: {
    term: Term;
    terms?: Term[];
    patternsForTerm?: (id: string) => Promise<GrammarPattern[]>;
    /** Select another term in the current list/detail workspace. */
    onselect?: (id: string) => void;
    /** Route to Glossary when this detail belongs to Reference. */
    onopen?: (id: string) => void;
    /** The surrounding preview already owns the identity heading. */
    embedded?: boolean;
    /** Quiet headings for the Reference and Glossary reading surfaces. */
    editorial?: boolean;
    /** A stable preview may already display the same summary. */
    showSummary?: boolean;
    /** Glossary navigation replaces pattern matches with curated term connections. */
    similarTerms?: boolean;
  } = $props();

  const kind = $derived(TERM_KINDS.find((entry) => entry.kind === term.kind)?.label ?? term.kind);
  const named = $derived(new Map(terms.map((entry) => [entry.id, entry.label])));
  const aliases = $derived(
    term.aliases.filter(
      (alias) => alias.toLocaleLowerCase() !== term.id.toLocaleLowerCase()
        && alias.toLocaleLowerCase() !== term.label.toLocaleLowerCase(),
    ),
  );

  const relations = $derived.by(() => {
    const seen = new Set<string>();
    return [
      { label: "Builds on", ids: term.requires },
      { label: "Contrasts with", ids: term.contrasts },
      { label: "Related", ids: term.related },
    ].map(({ label, ids }) => ({
      label,
      // One destination per glossary entry; retain the first curated relationship label.
      ids: similarTerms ? ids.filter((id) => {
        if (id === term.id || !named.has(id) || seen.has(id)) return false;
        seen.add(id);
        return true;
      }) : ids,
    })).filter(({ ids }) => ids.length > 0);
  });

  let patternsOpen = $state(false);
  let retry = $state(0);
  $effect(() => { void term.id; patternsOpen = false; retry = 0; });

  function choose(id: string) {
    if (onselect) onselect(id);
    else onopen?.(id);
  }
</script>

{#snippet termConnections()}
  {#each relations as relation}
    <section class="relations">
      {#if similarTerms}<h5>{relation.label}</h5>{:else}<h4>{relation.label}</h4>{/if}
      <div>
        {#each relation.ids as id}
          <button type="button" onclick={() => choose(id)}>{named.get(id) ?? id}</button>
        {/each}
      </div>
    </section>
  {/each}
{/snippet}

{#snippet connectedPatterns()}
  {#if patternsForTerm}
    {#await patternsForTerm(term.id)}
      <p class="loading">Finding connected patterns…</p>
    {:then patterns}
      {#if patterns.length > 0}
        <section class="patterns">
          <h4>Used in these patterns</h4>
          <ul>
            {#each patterns as pattern}
              <li><b lang="ja">{pattern.pattern}</b><span>{pattern.explanation}</span></li>
            {/each}
          </ul>
        </section>
      {:else if editorial}
        <p class="loading">No connected patterns.</p>
      {/if}
    {:catch}
      {#if editorial}<p class="loading" role="alert">Could not load connected patterns.</p><button type="button" class="open" onclick={() => (retry += 1)}>Retry patterns</button>{/if}
    {/await}
  {/if}
{/snippet}

<article class="term-detail" class:editorial aria-labelledby={embedded ? undefined : `term-${term.id}`} aria-label={embedded ? `${term.label} detail` : undefined}>
  {#if !embedded}
    <header>
      <div>
        <span class="eyebrow">{kind}</span>
        <h3 id="term-{term.id}">{term.label}</h3>
      </div>
      {#if onopen}
        <button type="button" class="open" onclick={() => onopen?.(term.id)}>Open in Glossary</button>
      {/if}
    </header>
  {:else if onopen}
    <div class="embedded-actions"><button type="button" class="open" onclick={() => onopen?.(term.id)}>Open in Glossary</button></div>
  {/if}

  {#if showSummary}<p class="summary">{term.summary}</p>{/if}
  {#if term.detail}<p class="detail">{term.detail}</p>{/if}
  {#if aliases.length > 0}
    <p class="aliases"><span>Also called</span> {aliases.join(", ")}</p>
  {/if}

  {#if term.examples.length > 0}
    <section class="examples" aria-labelledby="examples-{term.id}">
      <h4 id="examples-{term.id}">Examples</h4>
      {#each term.examples as example}
        <blockquote>
          <b lang="ja">{example.japanese}</b>
          <span>{example.english}</span>
        </blockquote>
      {/each}
    </section>
  {/if}

  {#if similarTerms && relations.length > 0}
    <section class="similar-terms" aria-labelledby="similar-{term.id}">
      <h4 id="similar-{term.id}">Similar terms</h4>
      {@render termConnections()}
    </section>
  {:else}
    {@render termConnections()}
  {/if}

  {#if patternsForTerm}
    {#if editorial}
      <details class="pattern-disclosure" bind:open={patternsOpen}>
        <summary>Connected grammar patterns</summary>
        {#if patternsOpen}{#key retry}{@render connectedPatterns()}{/key}{/if}
      </details>
    {:else}
      {@render connectedPatterns()}
    {/if}
  {/if}

</article>

<style>
  .term-detail { color: var(--text); min-width: 0; }
  header { display: flex; align-items: start; justify-content: space-between; gap: var(--gap); border-bottom: 1px solid var(--border); padding-bottom: var(--gap-sm); }
  h3 { margin: 0.15rem 0 0; font-size: 1.3rem; }
  .eyebrow, h4 { color: var(--accent); font-size: 0.7rem; font-weight: 600; letter-spacing: 0.08em; text-transform: uppercase; }
  .summary { margin: var(--gap) 0 0; font-size: 1rem; line-height: 1.55; }
  .detail { margin: var(--gap-sm) 0 0; color: var(--text-dim); line-height: 1.6; }
  .aliases { margin: var(--gap-sm) 0 0; color: var(--text-dim); font-size: 0.82rem; }
  .aliases span { color: var(--text-faint); margin-right: var(--gap-xs); }
  .open { flex: 0 0 auto; padding: 0; border: 0; background: none; color: var(--text-faint); font: inherit; font-size: 0.78rem; text-decoration: underline; cursor: pointer; }
  .open:hover { color: var(--text); }
  .embedded-actions { display: flex; justify-content: flex-end; }
  section { margin-top: var(--gap-lg); }
  h4 { margin: 0 0 var(--gap-sm); }
  blockquote { margin: 0 0 var(--gap-sm); padding-left: var(--gap); border-left: 2px solid var(--border-strong); }
  blockquote b, blockquote span { display: block; }
  blockquote span { color: var(--text-dim); font-size: 0.85rem; margin-top: 0.1rem; }
  .relations div { display: flex; flex-wrap: wrap; gap: var(--gap-xs); }
  .relations button { padding: 0.22rem 0.55rem; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface-raised); color: var(--text-dim); font: inherit; font-size: 0.8rem; cursor: pointer; }
  .relations button:hover { color: var(--text); border-color: var(--accent-edge); }
  .patterns ul { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--gap-sm); }
  .patterns li { padding-left: var(--gap); border-left: 2px solid var(--accent-edge); }
  .patterns b, .patterns span { display: block; }
  .patterns span { margin-top: 0.15rem; color: var(--text-dim); font-size: 0.85rem; line-height: 1.5; }
  .loading { color: var(--text-faint); font-size: 0.8rem; }
  .editorial h4 { color: var(--text); font-size: 0.9rem; letter-spacing: normal; text-transform: none; }
  .editorial .summary { margin-top: 0; }
  .editorial .relations button { border: 0; padding-inline: 0; margin-right: var(--gap); background: none; text-decoration: underline; text-underline-offset: 0.2em; }
  .similar-terms .relations { margin-top: var(--gap-sm); }
  .relations h5 { margin: 0 0 var(--gap-xs); color: var(--text-dim); font-size: 0.82rem; font-weight: 400; }
  .pattern-disclosure { margin-top: var(--gap-lg); }
  .pattern-disclosure summary { cursor: pointer; color: var(--text-dim); font-size: 0.9rem; }
</style>
