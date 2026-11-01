<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import IchiranSetup from "./IchiranSetup.svelte";
  import type { CatalogEntry } from "../model";
  import { errorMessage } from "./save-notice.js";
  import { duration, humanBytes, setup } from "./setup-store.svelte";

  let { onready, onexplore, onadopt, onlindera }: {
    onready: () => void;
    onexplore: () => void;
    /** Probe Ichiran and select Auto; throws when Ichiran does not answer. */
    onadopt: () => Promise<void>;
    /** Select the built-in analyzer, which is what "Use Lindera for now" promises. */
    onlindera: () => void;
  } = $props();
  let page = $state<"welcome" | "system" | "download" | "analyzer" | "ready">("welcome");
  let ichiran = $state<CatalogEntry | null>(null);
  let catalogError = $state("");
  let analyzerError = $state("");
  let choosing = $state(false);
  let setupRunning = $state(false);

  async function loadCatalog() {
    catalogError = "";
    try {
      ichiran = (await invoke<CatalogEntry[]>("catalog")).find((entry) => entry.id === "ichiran") ?? null;
      if (!ichiran) throw new Error("Ichiran setup details are unavailable.");
    } catch (thrown) { catalogError = errorMessage(thrown); }
  }
  let bundled = $state<boolean | null>(null);
  let bundleError = $state("");
  async function readBundle() {
    bundleError = "";
    try { bundled = await invoke<boolean>("setup_has_bundle"); }
    catch (thrown) { bundleError = errorMessage(thrown); }
  }
  onMount(() => {
    void loadCatalog();
    void readBundle();
  });

  async function chooseAnalyzer(useIchiran: boolean) {
    if (choosing || setupRunning) return;
    choosing = true;
    analyzerError = "";
    try {
      // A completed installer is not a working analyzer. Adopt probes a real analysis first.
      if (useIchiran) await onadopt();
      // **"Use Lindera for now" selects Lindera.** It only dismissed the step once, which left
      // Auto in charge — so on a machine where Ichiran already answered, a reader who chose
      // Lindera went on reading with Ichiran.
      else onlindera();
      await invoke("setup_finish_analyzer");
      await setup.refresh();
    } catch (thrown) { analyzerError = errorMessage(thrown); }
    finally { choosing = false; }
  }
  let storage = $state("");
  let offlinePackage = $state("");
  let technical = $state(false);

  const required = $derived(
    setup.snapshot?.components.filter((component) => component.required) ?? [],
  );
  const current = $derived(
    required.find((component) => component.id === setup.snapshot?.current_component),
  );
  const progressPercent = $derived(
    setup.progress?.total
      ? Math.min(100, Math.round((setup.progress.done / setup.progress.total) * 100))
      : null,
  );
  const completeCount = $derived(required.filter((component) => component.installed).length);
  const interrupted = $derived(
    setup.snapshot?.phase === "paused_retrying" ||
      setup.snapshot?.phase === "needs_attention",
  );

  $effect(() => {
    const snapshot = setup.snapshot;
    if (!snapshot) return;
    storage ||= snapshot.storage.target;
    if (snapshot.required_ready) page = snapshot.analyzer_setup_pending ? "analyzer" : "ready";
    else if (
      snapshot.phase === "downloading" ||
      snapshot.phase === "installing" ||
      snapshot.phase === "verifying" ||
      snapshot.phase === "paused_retrying" ||
      snapshot.phase === "needs_attention"
    ) {
      page = "download";
    } else if (snapshot.welcome_seen) {
      page = "system";
    }
  });

  async function showSystemCheck() {
    page = "system";
    await setup.checkSystem();
  }

  async function begin() {
    await setup.checkSystem();
    if (setup.preflight?.blocking_issue) return;
    page = "download";
    await setup.start();
  }

  async function chooseStorage() {
    if (!storage.trim()) return;
    await setup.chooseStorage(storage.trim());
  }

  async function browseStorage() {
    const selected = await open({ directory: true, multiple: false, title: "Choose where Sakatsu stores downloaded resources" });
    if (typeof selected === "string") storage = selected;
  }

  async function browseOfflinePackage() {
    const selected = await open({
      multiple: false,
      title: "Choose a Sakatsu essentials package",
      filters: [{ name: "Sakatsu essentials", extensions: ["zip"] }],
    });
    if (typeof selected === "string") offlinePackage = selected;
  }

  async function textOnly() {
    await setup.useTextOnly();
    onready();
  }

  async function installOffline() {
    if (!offlinePackage.trim()) return;
    page = "download";
    await setup.installOffline(offlinePackage.trim());
  }
</script>

<main class="onboarding" aria-live="polite">
  <div class="steps" aria-label="Setup progress">
    {#each ["Welcome", "System check", "Essentials", "Analyzer", "Ready"] as label, index}
      <span class:current={index === ["welcome", "system", "download", "analyzer", "ready"].indexOf(page)}>
        {label}
      </span>
    {/each}
  </div>

  {#if page === "welcome"}
    <section class="stage welcome">
      <p class="eyebrow">WELCOME</p>
      <h1>Read Japanese without leaving your game or book.</h1>
      <p class="lead">
        Paste Japanese or capture it from the screen. Sakatsu explains words and grammar,
        and keeps recognition on this device.
      </p>
      <div class="plain-facts">
        <div><span>What is installed</span><strong>Dictionaries + two OCR engines</strong></div>
        <div>
          <span>{bundled ? "Included resources" : "Approximate download"}</span>
          <strong>{humanBytes(setup.snapshot?.storage.download_bytes ?? null)}</strong>
        </div>
        <div><span>Privacy</span><strong>Text and images stay local</strong></div>
      </div>
      <p class="fine">
        Ichiran is the recommended main analyzer, with Lindera assistance and fallback.
        After essentials, guided Ichiran setup asks separately before installing WSL, Docker
        and its database{ichiran ? ` (about ${ichiran.size} additional download)` : ""}.
        You can use built-in Lindera while you defer that step.
      </p>
      <p class="fine">
        {bundled ? "The essentials are included. Setup installs them on this device without downloading." : "Nothing downloads until you continue."}
        You can close Sakatsu during setup and resume later.
        <button class="link" onclick={() => invoke("open_notices")}>Licences and details</button>
      </p>
      {#if bundleError}<p class="note warn" role="alert">{bundleError}</p><button class="control" onclick={readBundle}>Check included resources again</button>{/if}
      <button class="control primary" onclick={showSystemCheck} disabled={bundled === null}>Set up Sakatsu Reader</button>
    </section>
  {:else if page === "system"}
    <section class="stage">
      <p class="eyebrow">SYSTEM CHECK</p>
      <h1>Let’s make sure setup will finish cleanly.</h1>
      {#if setup.checking && !setup.preflight}
        <p class="lead">Checking storage, Windows, and your connection…</p>
      {:else if setup.preflight}
        <ul class="checks">
          <li class:good={setup.preflight.supported_windows && setup.preflight.supported_architecture}>
            <span aria-hidden="true">{setup.preflight.supported_windows ? "✓" : "!"}</span>
            <div><strong>Windows 10/11 x64</strong><small>Supported platform</small></div>
          </li>
          <li class:good={setup.preflight.writable && setup.preflight.enough_space}>
            <span aria-hidden="true">{setup.preflight.writable && setup.preflight.enough_space ? "✓" : "!"}</span>
            <div>
              <strong>Storage</strong>
              <small>
                {humanBytes(setup.preflight.available_bytes)} available ·
                {humanBytes(setup.preflight.peak_bytes)} safe peak required
              </small>
            </div>
          </li>
          <li class:good={bundled || setup.preflight.network_available}>
            <span aria-hidden="true">{bundled || setup.preflight.network_available ? "✓" : "!"}</span>
            <div><strong>{bundled ? "Local installation" : "Connection"}</strong><small>{bundled ? "Included resources do not need an internet connection" : "Downloads can resume after interruptions"}</small></div>
          </li>
          <li class:good={setup.preflight.webview2}>
            <span aria-hidden="true">✓</span>
            <div><strong>Windows app runtime</strong><small>WebView2 is ready</small></div>
          </li>
        </ul>

        {#if setup.preflight.blocking_issue}
          <div class="issue" role="alert">
            <strong>Setup needs attention</strong>
            <span>{setup.preflight.blocking_issue}</span>
          </div>
        {/if}

        <details bind:open={technical}>
          <summary>Technical details</summary>
          <label class="storage-field">
            <span>Downloaded resources location</span>
            <div>
              <input bind:value={storage} />
              <button class="control" onclick={browseStorage}>Browse…</button>
              <button class="control" onclick={chooseStorage}>Use location</button>
            </div>
          </label>
          <label class="storage-field">
            <span>Essentials package already downloaded</span>
            <div>
              <input bind:value={offlinePackage} placeholder="C:\Downloads\sakatsu-essentials.zip" />
              <button class="control" onclick={browseOfflinePackage}>Browse…</button>
              <button class="control" onclick={installOffline}>Install package</button>
            </div>
          </label>
          <p>Settings and learning history remain in the standard roaming profile.</p>
        </details>

        <div class="actions">
          <button
            class="control primary"
            onclick={begin}
            disabled={Boolean(setup.preflight.blocking_issue) || setup.checking}
          >{bundled ? "Install included resources" : "Download and install"}</button>
          <button class="control" onclick={() => (page = "welcome")}>Back</button>
        </div>
      {:else}
        <div class="issue" role="alert">{setup.error || "The system check could not run."}</div>
        <button class="control" onclick={() => setup.checkSystem()}>Try again</button>
      {/if}
    </section>
  {:else if page === "download"}
    <section class="stage">
      <p class="eyebrow">{bundled ? "INSTALL AND VERIFY" : "DOWNLOAD AND VERIFY"}</p>
      <h1>{interrupted ? "Your progress is safe." : "Setting up screen capture…"}</h1>
      <p class="lead">
        {#if interrupted}
          Setup can continue from the saved download instead of starting over.
        {:else}
          Keep this window open, or close it and resume later. Sakatsu checks every model before using it.
        {/if}
      </p>

      <div class="install-card">
        <div class="progress-heading">
          <strong>{current?.label ?? "Required reading tools"}</strong>
          <span>{completeCount} of {required.length}</span>
        </div>
        <div class="track" aria-label="Download progress">
          <span class:indeterminate={progressPercent === null} style:width={progressPercent === null ? undefined : `${progressPercent}%`}></span>
        </div>
        <div class="metrics">
          {#if setup.progress}
            <span>{humanBytes(setup.progress.done)} / {humanBytes(setup.progress.total)}</span>
            <span>{setup.progress.speed ? `${humanBytes(setup.progress.speed)}/s` : "Connecting…"}</span>
            <span>{duration(setup.progress.eta)}</span>
          {:else if setup.snapshot?.resumable}
            <span>Saved partial download found</span>
          {:else}
            <span>Preparing download…</span>
          {/if}
        </div>
        <ul class="component-progress">
          {#each required as component}
            <li class:done={component.installed} class:active={component.id === setup.snapshot?.current_component}>
              <span aria-hidden="true">{component.installed ? "✓" : component.id === setup.snapshot?.current_component ? "●" : "○"}</span>
              <span>{component.label}</span>
              <small>{component.installed ? "verified" : component.transfer_state.replaceAll("_", " ")}</small>
            </li>
          {/each}
        </ul>
      </div>

      {#if setup.snapshot?.current_issue || setup.error}
        <div class="issue" role="alert">
          <strong>{setup.snapshot?.phase === "needs_attention" ? "Setup needs attention" : "Setup paused"}</strong>
          <span>{setup.snapshot?.current_issue || setup.error}</span>
        </div>
      {/if}

      <div class="actions">
        {#if setup.snapshot?.phase === "downloading" || setup.snapshot?.phase === "installing" || setup.snapshot?.phase === "verifying"}
          <button class="control" onclick={() => setup.pause()}>Pause</button>
        {:else}
          <button class="control primary" onclick={() => setup.start()}>Retry now</button>
          {#if bundled || setup.snapshot?.phase === "needs_attention"}<button class="control" onclick={() => setup.start(true)}>Download repair</button>{/if}
          <button class="control" onclick={textOnly}>Use pasted text while setup finishes</button>
        {/if}
      </div>
      {#if interrupted}
        <details>
          <summary>Install from a downloaded essentials package</summary>
          <label class="storage-field">
            <span>Package path</span>
            <div>
              <input bind:value={offlinePackage} placeholder="C:\Downloads\sakatsu-essentials.zip" />
              <button class="control" onclick={browseOfflinePackage}>Browse…</button>
              <button class="control" onclick={installOffline}>Install package</button>
            </div>
          </label>
        </details>
      {/if}
      <p class="fine">Next: set up Ichiran, the recommended main analyzer. Translation, pronunciation and frequency lists remain optional.</p>
    </section>
  {:else if page === "analyzer"}
    <section class="stage">
      <p class="eyebrow">MAIN ANALYZER</p>
      <h1>Read with Ichiran, assisted by Lindera.</h1>
      <p class="lead">Ichiran supplies the main word analysis. Built-in Lindera assists in the background
        and provides a visible fallback if Ichiran cannot answer.</p>
      <p class="copy">Guided setup opens PowerShell and checks existing installations first. It may ask to install
        WSL and Ubuntu, restart Windows, and finish Docker Desktop’s welcome screen and WSL integration.
        Return here afterwards to continue.</p>
      {#if ichiran}
        <p class="copy">Additional download: about {ichiran.size} for Ichiran. WSL and Docker need additional downloads
          and disk space. The essentials storage check does not cover them. Nothing installs until you choose setup.</p>
        <IchiranSetup size={ichiran.size} recommended onadopt={() => chooseAnalyzer(true)} onrunning={(value) => (setupRunning = value)} disabled={choosing} />
      {:else if catalogError}
        <p class="issue" role="alert">{catalogError}</p>
        <button class="control" onclick={loadCatalog}>Retry setup details</button>
      {:else}
        <p>Loading Ichiran setup details…</p>
      {/if}
      <div class="actions">
        <button class="control" onclick={() => chooseAnalyzer(true)} disabled={choosing || setupRunning}>
          {choosing ? "Checking analyzer…" : "Use installed Ichiran"}
        </button>
        <button class="control" onclick={() => chooseAnalyzer(false)} disabled={choosing || setupRunning}>Use Lindera for now</button>
      </div>
      {#if analyzerError}<p class="issue" role="alert">{analyzerError}</p>{/if}
      <p class="fine">Use Lindera for now selects the built-in analyzer. To switch to Ichiran later, choose
        Auto in Settings → Reading → How the text is read, where guided setup is offered.
        Your dictionaries and learning history work with either analyzer.</p>
    </section>
  {:else}
    <section class="stage ready">
      <p class="eyebrow">READY</p>
      <h1>Sakatsu Reader is ready.</h1>
      <p class="lead">Both ordinary game text and difficult manga text can now be read automatically.</p>
      <ol class="first-actions">
        <li><strong>Paste Japanese</strong><span>Paste text into Analysis, then choose Analyze.</span></li>
        <li><strong>Capture from the screen</strong><span>Press your configured capture shortcut over a game or manga.</span></li>
      </ol>
      <button class="control primary" onclick={onready}>Start reading</button>
      <button class="link" onclick={onexplore}>Explore optional features</button>
    </section>
  {/if}
</main>

<style>
  .onboarding {
    width: min(42rem, calc(100% - 2 * var(--gap-lg)));
    margin: clamp(1rem, 5vh, 3.5rem) auto;
    color: var(--text);
  }
  .steps {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 2px;
    margin-bottom: var(--gap-lg);
  }
  .steps span {
    border-top: 2px solid var(--border);
    padding-top: var(--gap-xs);
    color: var(--text-faint);
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .steps span.current { border-color: var(--accent); color: var(--accent); }
  .stage {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: clamp(1rem, 4vw, 2rem);
    display: grid;
    gap: var(--gap-lg);
  }
  .eyebrow { margin: 0; color: var(--accent); font: 0.7rem var(--font-mono); letter-spacing: 0.12em; }
  h1 { margin: calc(-1 * var(--gap-sm)) 0 0; font-size: clamp(1.35rem, 4vw, 2rem); line-height: 1.15; }
  .lead { margin: calc(-1 * var(--gap-sm)) 0 0; color: var(--text-dim); line-height: 1.55; }
  .plain-facts { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--gap-sm); }
  .plain-facts div { padding: var(--gap); border: 1px solid var(--border); border-radius: var(--radius); display: grid; gap: var(--gap-xs); }
  .plain-facts span, small { color: var(--text-faint); font-size: 0.74rem; }
  .plain-facts strong { font-size: 0.86rem; }
  .fine { margin: 0; color: var(--text-faint); font-size: 0.78rem; line-height: 1.5; }
  /* Body text in a stage. Margin-free like every other paragraph here: the stage's grid gap is the
     spacing, and a browser margin on top of it doubled the space between paragraphs. */
  .copy { margin: 0; line-height: 1.55; }
  .link { border: 0; background: none; color: var(--accent); padding: 0; text-decoration: underline; text-underline-offset: 3px; }
  .primary { background: var(--accent-strong); color: var(--on-accent-strong); border-color: var(--accent-strong); font-weight: 600; padding: 0.62rem 1rem; }
  .checks, .component-progress { list-style: none; padding: 0; margin: 0; display: grid; gap: var(--gap-sm); }
  .checks li { display: grid; grid-template-columns: 1.5rem 1fr; gap: var(--gap-sm); align-items: center; color: var(--red); }
  .checks li.good { color: var(--good); }
  .checks div { display: grid; gap: 0.1rem; color: var(--text); }
  .issue { border: 1px solid var(--red-edge); background: var(--red-wash); border-radius: var(--radius); padding: var(--gap); display: grid; gap: var(--gap-xs); color: var(--text); }
  .issue span { color: var(--text-dim); }
  details { color: var(--text-dim); font-size: 0.82rem; }
  summary { cursor: pointer; }
  .storage-field { display: grid; gap: var(--gap-xs); margin-top: var(--gap); }
  .storage-field div { display: flex; gap: var(--gap-sm); }
  input { min-width: 0; flex: 1; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface-raised); color: var(--text); padding: 0.42rem 0.55rem; }
  .actions { display: flex; gap: var(--gap-sm); flex-wrap: wrap; }
  .install-card { border: 1px solid var(--border); border-radius: var(--radius); padding: var(--gap-lg); display: grid; gap: var(--gap); }
  .progress-heading, .metrics { display: flex; justify-content: space-between; gap: var(--gap); }
  .progress-heading span, .metrics { color: var(--text-faint); font: 0.74rem var(--font-mono); }
  .track { height: 6px; border-radius: 999px; background: var(--surface-raised); overflow: hidden; }
  .track > span { display: block; height: 100%; background: var(--accent); transition: width var(--anim) var(--ease); }
  .track > span.indeterminate { width: 35%; animation: sweep 1.4s ease-in-out infinite; }
  @keyframes sweep { from { margin-left: -35%; } to { margin-left: 100%; } }
  .component-progress li { display: grid; grid-template-columns: 1rem 1fr auto; gap: var(--gap-sm); color: var(--text-faint); }
  .component-progress li.done { color: var(--good); }
  .component-progress li.active { color: var(--accent); }
  .component-progress small { text-transform: capitalize; }
  .first-actions { margin: 0; padding-left: 1.4rem; display: grid; gap: var(--gap); }
  .first-actions li { padding-left: var(--gap-xs); }
  .first-actions span { display: block; color: var(--text-dim); margin-top: 0.15rem; }
  @media (max-width: 36rem) {
    .onboarding { width: calc(100% - 2 * var(--gap)); margin: var(--gap) auto; }
    .stage { padding: var(--gap-lg); }
    .plain-facts { grid-template-columns: 1fr; }
    .steps span { font-size: 0.58rem; }
    .metrics { flex-wrap: wrap; }
  }
  @media (prefers-reduced-motion: reduce) {
    .track > span.indeterminate { animation: none; width: 100%; }
  }
</style>
