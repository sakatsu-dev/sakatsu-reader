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
  } = $props();

  const kind = $derived(TERM_KINDS.find((entry) => entry.kind === term.kind)?.label ?? term.kind);
  const named = $derived(new Map(terms.map((entry) => [entry.id, entry.label])));
  const aliases = $derived(
    term.aliases.filter(
      (alias) => alias.toLocaleLowerCase() !== term.id.toLocaleLowerCase()
        && alias.toLocaleLowerCase() !== term.label.toLocaleLowerCase(),
    ),
  );

  function choose(id: string) {
    if (onselect) onselect(id);
    else onopen?.(id);
  }
</script>

<article class="term-detail" aria-labelledby={embedded ? undefined : `term-${term.id}`} aria-label={embedded ? `${term.label} detail` : undefined}>
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

  <p class="summary">{term.summary}</p>
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

  {#each [
    ["Builds on", term.requires],
    ["Contrasts with", term.contrasts],
    ["Related", term.related],
  ] as relation}
    {@const label = relation[0] as string}
    {@const ids = relation[1] as string[]}
    {#if ids.length > 0}
      <section class="relations">
        <h4>{label}</h4>
        <div>
          {#each ids as id}
            <button type="button" onclick={() => choose(id)}>{named.get(id) ?? id}</button>
          {/each}
        </div>
      </section>
    {/if}
  {/each}

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
      {/if}
    {:catch}
      <!-- A relationship supplement must not replace the term's own explanation with an error. -->
    {/await}
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
</style>
