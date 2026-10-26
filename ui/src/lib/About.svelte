<script lang="ts">
  // **Licence and attribution text is never written here.** Software and references come from
  // `credits` (pinned to the manifests by `credits_match_manifests.rs`), data from `catalog`, and
  // installed dictionaries from `dictionaries`; a copy here would be a second licence to drift.
  //
  // **Each source loads, fails and comes back empty on its own.** A failed `catalog` was once
  // swallowed into an empty list, which removed the data attributions without a word.
  //
  // **Creator links leave through the shell**, by name (`open_creator_link`), never in the webview.
  //
  // **The shape is an identity card, then the credits folded away** (2026-10-18, after koharu's
  // About dialog): name, one line of purpose, version and contributor, the links. The credits are every
  // bit as complete as before — several of the licences require them — they simply start shut.

  import { onMount } from "svelte";
  let { onupdates = () => {} }: { onupdates?: () => void } = $props();
  import { invoke } from "@tauri-apps/api/core";
  import Seal from "./Seal.svelte";
  import { errorMessage } from "./save-notice.js";
  import type { CatalogEntry, CreditEntry, DictionaryMeta } from "../model";
  // Official marks, bundled unmodified. Provenance and terms: `../assets/brands/README.md`.
  import githubOnLight from "../assets/brands/github-invertocat-black.svg";
  import githubOnDark from "../assets/brands/github-invertocat-white.svg";
  import kofiCup from "../assets/brands/kofi-cup.png";

  /** Still coming, here (possibly empty), or failed. Never conflated. */
  type Load<T> =
    { state: "loading" } | { state: "ready"; value: T } | { state: "failed"; error: string };

  type Destination = "github" | "kofi";

  /** The addresses `commands::creator_url` maps the same names to; `about.spec.ts` pins both. */
  const CREATOR_LINKS: readonly { destination: Destination; label: string; href: string }[] = [
    { destination: "github", label: "GitHub", href: "https://github.com/S-0E51" },
    { destination: "kofi", label: "Support on Ko-fi", href: "https://ko-fi.com/sevennightmares" },
  ];

  let version = $state("");
  let software = $state<Load<CreditEntry[]>>({ state: "loading" });
  let data = $state<Load<CatalogEntry[]>>({ state: "loading" });
  let installed = $state<Load<DictionaryMeta[]>>({ state: "loading" });

  /// Asked rather than assumed, so a build without the notices file offers no button that fails.
  let hasNotices = $state(false);
  let noticesError = $state("");
  let linkError = $state("");

  /// The latest request per source, so a slow older answer never overwrites a newer one.
  const latest = { software: 0, data: 0, installed: 0 };

  function track<T>(
    key: keyof typeof latest,
    request: () => Promise<T>,
    apply: (load: Load<T>) => void,
  ) {
    const mine = ++latest[key];
    apply({ state: "loading" });
    request().then(
      (value) => {
        if (mine === latest[key]) apply({ state: "ready", value });
      },
      (thrown) => {
        if (mine === latest[key]) apply({ state: "failed", error: errorMessage(thrown) });
      },
    );
  }

  function loadSoftware() {
    track(
      "software",
      () => invoke<[string, CreditEntry[]]>("credits"),
      (load) => {
        if (load.state !== "ready") {
          software = load;
          return;
        }
        // Together, and only for the request that still owns the page.
        version = load.value[0];
        software = { state: "ready", value: load.value[1] };
      },
    );
  }

  function loadData() {
    track(
      "data",
      () => invoke<CatalogEntry[]>("catalog"),
      (load) => (data = load),
    );
  }

  function loadInstalled() {
    track(
      "installed",
      () => invoke<DictionaryMeta[]>("dictionaries"),
      (load) => (installed = load),
    );
  }

  /// The version arrives with the credits, so it says what they say: coming, or not to be had.
  const versionText = $derived(
    version ||
      (software.state === "failed"
        ? "Unavailable"
        : software.state === "loading"
          ? "Loading…"
          : ""),
  );

  /// The credits under their headings, in `credits::Group`'s order (insertion, not alphabetical).
  const sections = $derived.by(() => {
    const out = new Map<string, CreditEntry[]>();
    if (software.state !== "ready") return [];
    for (const credit of software.value) {
      const list = out.get(credit.group);
      if (list) list.push(credit);
      else out.set(credit.group, [credit]);
    }
    return [...out];
  });

  /** Said on the summary, so a failure shows while the fold is shut. */
  function summarise(load: Load<unknown[]>, empty: string): string {
    if (load.state === "loading") return "Loading";
    if (load.state === "failed") return "Could not load";
    const count = load.value.length;
    if (count === 0) return empty;
    return count === 1 ? "1 entry" : `${count} entries`;
  }

  onMount(() => {
    loadSoftware();
    loadData();
    loadInstalled();
    invoke<boolean>("notices_available")
      .then((there) => (hasNotices = there))
      .catch(() => {});
  });

  function openNotices() {
    noticesError = "";
    invoke("open_notices").catch((thrown) => (noticesError = String(thrown)));
  }

  /// Click, Enter, modified and middle clicks all land here; none may navigate the webview.
  function openCreatorLink(event: MouseEvent, destination: Destination) {
    event.preventDefault();
    linkError = "";
    invoke("open_creator_link", { destination }).catch(
      (thrown) => (linkError = `The browser could not be opened. ${errorMessage(thrown)}`),
    );
  }
</script>

{#snippet failure(error: string, retry: () => void)}
  <div class="fold-failure">
    <p>This list could not be loaded. {error}</p>
    <button class="control" type="button" onclick={retry}>Try again</button>
  </div>
{/snippet}

<section class="page about">
  <h2>About</h2>

  <div class="page-body">
    <article class="paper">
      <div class="spread">
        <div class="card">
          <header class="identity">
            <div class="tile"><Seal size="2.2rem" /></div>
            <div class="nameplate">
              <p class="title">Sakatsu Reader</p>
              <p class="standfirst">
                Capture Japanese text, look up words, and study while you read.
              </p>
            </div>
          </header>

          <dl class="facts">
            <dt>Version</dt>
            <dd class="edition">
              <span class="version">{versionText}</span>
              <button class="control" type="button" onclick={onupdates}>Check for updates</button>
            </dd>
            <dt>Contributor</dt>
            <dd class="byline">seven-nightmares</dd>
          </dl>

          <section class="creator" aria-label="From the creator">
            <ul>
              {#each CREATOR_LINKS as link (link.destination)}
                <li>
                  <a
                    href={link.href}
                    target="_blank"
                    rel="noreferrer noopener"
                    onclick={(event) => openCreatorLink(event, link.destination)}
                    onauxclick={(event) => {
                      if (event.button === 1) openCreatorLink(event, link.destination);
                    }}
                  >
                    {#if link.destination === "github"}
                      <!-- GitHub allows the mark only in black or white: one per theme. -->
                      <img class="mark on-dark" src={githubOnDark} alt="" />
                      <img class="mark on-light" src={githubOnLight} alt="" />
                    {:else}
                      <img class="mark cup" src={kofiCup} alt="" />
                    {/if}
                    <span>{link.label}</span>
                  </a>
                </li>
              {/each}
            </ul>
            {#if linkError}<p class="warn" role="alert">{linkError}</p>{/if}
          </section>
        </div>

        <section class="credits" aria-labelledby="about-credits">
          <h3 class="kicker" id="about-credits">Credits and licences</h3>

          <!-- Plain `<summary>` headings keep their native marker, and controls stay in the body
             (`local documentation`: a flex summary loses its triangle). -->
          <details class="fold" data-fold="software">
            <summary>
              <span class="fold-title">Software and references</span>
              <span class="fold-state" class:failed={software.state === "failed"}>
                {summarise(software, "None listed")}
              </span>
            </summary>
            <div class="fold-body">
              {#if software.state === "loading"}
                <p class="fold-note">Loading…</p>
              {:else if software.state === "failed"}
                {@render failure(software.error, loadSoftware)}
              {:else if software.value.length === 0}
                <p class="fold-note">No credits are listed.</p>
              {:else}
                <p class="fold-note">
                  Software used by Sakatsu and references consulted for its grammar notes.
                </p>
                <p class="fold-note">
                  The consulted references are linked, not bundled. Their content is all rights
                  reserved.
                </p>
                {#each sections as [group, entries] (group)}
                  <h4>{group}</h4>
                  <ul class="entries">
                    {#each entries as credit}
                      <li class="entry">
                        <p class="entry-head">
                          <a href={credit.url} target="_blank" rel="noreferrer noopener"
                            >{credit.name}</a
                          >
                          <span class="licence">{credit.licence}</span>
                        </p>
                        <p class="why">{credit.why}</p>
                      </li>
                    {/each}
                  </ul>
                {/each}
              {/if}
            </div>
          </details>

          <details class="fold" data-fold="data">
            <summary>
              <span class="fold-title">Dictionaries, models and data</span>
              <span class="fold-state" class:failed={data.state === "failed"}>
                {summarise(data, "None listed")}
              </span>
            </summary>
            <div class="fold-body">
              {#if data.state === "loading"}
                <p class="fold-note">Loading…</p>
              {:else if data.state === "failed"}
                {@render failure(data.error, loadData)}
              {:else if data.value.length === 0}
                <p class="fold-note">No downloadable data is listed.</p>
              {:else}
                <p class="fold-note">Downloaded resources and their licences.</p>
                <ul class="entries">
                  {#each data.value as component}
                    <li class="entry">
                      <p class="entry-head">
                        <a href={component.home} target="_blank" rel="noreferrer noopener">
                          {component.label}
                        </a>
                        {#if component.ready}<span class="here">installed</span>{/if}
                      </p>
                      <p class="why">{component.about}</p>
                      <p class="licence-line">{component.licence}</p>
                      {#if component.sources.length > 0}
                        <ul class="source-links" aria-label={`${component.label} sources`}>
                          {#each component.sources as source}
                            <li>
                              <a href={source.home} target="_blank" rel="noreferrer noopener">
                                {source.name}
                              </a>
                              <span>{source.licence}</span>
                            </li>
                          {/each}
                        </ul>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          </details>

          <details class="fold" data-fold="installed">
            <summary>
              <span class="fold-title">Installed dictionaries</span>
              <span class="fold-state" class:failed={installed.state === "failed"}>
                {summarise(installed, "None installed")}
              </span>
            </summary>
            <div class="fold-body">
              {#if installed.state === "loading"}
                <p class="fold-note">Loading…</p>
              {:else if installed.state === "failed"}
                {@render failure(installed.error, loadInstalled)}
              {:else if installed.value.length === 0}
                <p class="fold-note">No dictionaries are installed yet.</p>
              {:else}
                <p class="fold-note">Attribution supplied by each installed dictionary.</p>
                <ul class="entries">
                  {#each installed.value as dictionary}
                    <li class="entry">
                      <p class="entry-head">
                        {#if dictionary.url}
                          <a href={dictionary.url} target="_blank" rel="noreferrer noopener">
                            {dictionary.title}
                          </a>
                        {:else}
                          <span class="plain">{dictionary.title}</span>
                        {/if}
                        <span class="licence">{dictionary.revision}</span>
                      </p>
                      {#if dictionary.attribution}
                        <p class="why">{dictionary.attribution}</p>
                      {:else if dictionary.author}
                        <p class="why">{dictionary.author}</p>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          </details>

          {#if hasNotices}
            <!-- Outside the folds: this file is where the MIT and Apache-2.0 notices are actually
               carried, for every package that ships. -->
            <p class="notices">
              Full third-party licence texts:
              <button class="link" type="button" onclick={openNotices}
                >THIRD-PARTY-NOTICES.txt</button
              >
            </p>
            {#if noticesError}<p class="warn" role="alert">{noticesError}</p>{/if}
          {/if}
        </section>
      </div>
    </article>
  </div>
</section>

<style>
  /* The container every responsive rule queries; not `.page-body`, which scrolls (traps.md). */
  .paper {
    container-type: inline-size;
    /* `body` is a fixed 15px and Interface size moves only the root, so inherited text needs
       `1rem` to follow the setting. */
    font-size: 1rem;
    /* **No width cap, and that is what makes it resolution-aware.** It was `64rem`, which at a
       full-screen 1920px window stopped the spread half way across: the card floated in the
       middle of the page with the right half empty. Uncapped, the card sits at the page's own
       right edge however wide the window is, and the room in between goes to the credits, whose
       columns grow with it (below). Prose inside keeps its own measure — `.fold-note` and
       `.notices` are capped at `42rem` — so long lines are not the price. */
    padding: var(--gap-sm) 0 calc(var(--gap-lg) * 2);
  }

  /* **A newspaper's sidebar.** One column when narrow, the card first. From `46rem` the card moves
     to a column of its own on the right and the credits run down the main column beside it, both
     starting at the top. The source order stays card-then-credits, so the name is still read
     first; only the placement changes. The query is on `.paper`, the container. */
  @container (min-width: 46rem) {
    .spread {
      display: grid;
      /* The sidebar grows a little with the page — a fifth of it, between 20rem and 26rem — so
         on a wide screen it is not a sliver against a long credits column. `cqi` is `.paper`'s
         width, the container. */
      grid-template-columns: minmax(0, 1fr) clamp(20rem, 20cqi, 26rem);
      column-gap: calc(var(--gap-lg) * 2);
      align-items: start;
    }
    .spread > .card {
      grid-column: 2;
      grid-row: 1;
    }
    .spread > .credits {
      grid-column: 1;
      grid-row: 1;
      margin-top: 0;
    }
  }

  /* ---- the identity card ---- */

  .card {
    /* As wide as its column beside the credits; stacked above them it stops at a card's width
       rather than spanning the page. */
    max-width: 30rem;
    padding: var(--gap-lg);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-raised);
  }
  /* Top-aligned, not centred: the purpose wraps to two lines in the sidebar, and centring the tile
     against the whole block dropped it below the title. The tile's top meets the title's. */
  .identity {
    display: flex;
    align-items: flex-start;
    gap: var(--gap);
  }
  /* The seal on a soft tile of the accent, as an app icon sits in its own square. */
  .tile {
    display: grid;
    place-items: center;
    flex: none;
    width: 3.4rem;
    height: 3.4rem;
    border-radius: var(--radius-lg);
    background: color-mix(in oklab, var(--accent) 12%, transparent);
  }
  .nameplate {
    min-width: 0;
  }
  .title {
    margin: 0;
    font-family: var(--font-display);
    font-size: 1.35rem;
    font-weight: 600;
    line-height: 1.2;
    color: var(--text);
  }
  .standfirst {
    margin: var(--gap-xs) 0 0;
    line-height: 1.45;
    color: var(--text-dim);
  }

  /* Label left, value right: the facts read as a table without being one. */
  .facts {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: var(--gap-sm) var(--gap-lg);
    margin: var(--gap-lg) 0 0;
    padding-top: var(--gap);
    border-top: 1px solid var(--border);
  }
  .facts dt {
    font-size: 0.85rem;
    color: var(--text-faint);
  }
  .facts dd {
    margin: 0;
    text-align: right;
    font-weight: 600;
    color: var(--text);
    overflow-wrap: anywhere;
  }

  /* ---- the creator's links ---- */

  .creator {
    margin-top: var(--gap-lg);
    padding-top: var(--gap);
    border-top: 1px solid var(--border);
  }
  .creator ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--gap-sm);
  }
  .creator a {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.3rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 0.9rem;
    color: var(--text);
    text-decoration: none;
    white-space: nowrap;
    transition:
      background var(--anim) var(--ease),
      border-color var(--anim) var(--ease);
  }
  .creator a:hover {
    background: var(--surface-hover);
    border-color: var(--border-strong);
  }
  .mark {
    display: block;
    height: 1rem;
    width: auto;
  }
  .cup {
    height: 1.05rem;
  }
  /* The static stylesheet is dark until a theme is painted, so the white mark is the default. */
  .on-light {
    display: none;
  }
  :global(:root[data-theme="light"]) .on-light {
    display: block;
  }
  :global(:root[data-theme="light"]) .on-dark {
    display: none;
  }

  /* ---- credits ---- */

  /* Its own container: beside the card the credits column is narrower than the page, and the
     entries' two columns are decided by the room they actually have. */
  .credits {
    container: credits / inline-size;
    margin-top: calc(var(--gap-lg) * 1.5);
  }
  .kicker {
    margin: 0 0 var(--gap-sm);
    font-size: 0.8rem;
    font-weight: 600;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--text-faint);
  }

  /* No flex on the summary: it would delete the native marker (traps.md). */
  .fold {
    border-top: 1px solid var(--border);
  }
  .fold:last-of-type {
    border-bottom: 1px solid var(--border);
  }
  .fold > summary {
    padding: var(--gap-sm) 0;
    cursor: pointer;
    color: var(--text);
  }
  .fold > summary:hover .fold-title {
    color: var(--accent);
  }
  .fold-title {
    font-weight: 600;
  }
  .fold-state {
    margin-left: var(--gap-sm);
    font-size: 0.85rem;
    color: var(--text-faint);
  }
  .fold-state.failed {
    color: var(--red);
  }
  /* The body is an explicit wrapper: a `<details>` puts its content in a box of its own, so spacing
     set on the fold itself never reaches it (`traps.md`). */
  .fold-body {
    padding: 0 0 var(--gap-lg) 1.1rem;
  }
  .fold-note {
    margin: 0 0 var(--gap);
    max-width: 42rem;
    color: var(--text-dim);
  }
  .fold-body h4 {
    margin: var(--gap-lg) 0 var(--gap-sm);
    padding-bottom: var(--gap-xs);
    border-bottom: 1px solid var(--border);
    font-size: 0.8rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  .fold-failure {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--gap-sm) var(--gap);
  }
  .fold-failure p {
    flex: 1 1 18rem;
    margin: 0;
    color: var(--red);
  }

  /* Columns fill down first, so reading order stays the source's; no entry splits across two. */
  .entries {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  /* One column more for every ~26rem the credits gain, up to four: about the width at which an
     entry's sentence stops being a comfortable line. Stepped rather than `columns: <width>` so the
     count is a stated fact a test can compute from the width, not something the engine rounds. */
  @container credits (min-width: 36rem) {
    .entries {
      columns: 2;
      column-gap: calc(var(--gap-lg) * 2);
      column-rule: 1px solid var(--border);
    }
  }
  @container credits (min-width: 62rem) {
    .entries {
      columns: 3;
    }
  }
  @container credits (min-width: 88rem) {
    .entries {
      columns: 4;
    }
  }
  .entry {
    break-inside: avoid;
    padding: 0 0 var(--gap);
  }
  .entry-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    /* Wraps, so a licence too long to sit beside the name drops beneath it rather than racing it
       for the width. */
    flex-wrap: wrap;
    gap: 0.1rem var(--gap-sm);
    margin: 0;
    font-weight: 600;
    color: var(--text);
  }
  /* **`0 1 auto`, and the shrink is load-bearing.** Nearly every licence is a tag three characters
     wide; the WebView2 runtime's is a 74-character clause, and with `flex-shrink: 0` it crushed the
     name beside it to one word per line. */
  .licence,
  .here {
    flex: 0 1 auto;
    font-size: 0.8rem;
    font-weight: 400;
    color: var(--text-faint);
  }
  .here {
    color: var(--accent);
  }
  .why {
    margin: 0.2rem 0 0;
    color: var(--text-dim);
  }
  /* Under the sentence rather than beside the name: the catalog's licence strings are whole
     clauses where a crate's is three characters. */
  .licence-line {
    margin: var(--gap-xs) 0 0;
    font-size: 0.85rem;
    color: var(--text-faint);
  }
  .source-links {
    list-style: none;
    margin: var(--gap-xs) 0 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--gap-xs) var(--gap);
    font-size: 0.85rem;
    color: var(--text-faint);
  }
  .source-links li {
    display: inline-flex;
    gap: 0.35rem;
  }

  .notices {
    margin: var(--gap-lg) 0 0;
    max-width: 42rem;
    color: var(--text-dim);
  }
  /* A button that reads as a link, because it is one in every way except that it opens a local
     file rather than a URL. */
  .link {
    background: none;
    border: 0;
    padding: 0;
    font: inherit;
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
  }
  .link:hover {
    color: var(--accent-strong);
  }
  .warn {
    margin: var(--gap-sm) 0 0;
    color: var(--red);
  }
</style>
