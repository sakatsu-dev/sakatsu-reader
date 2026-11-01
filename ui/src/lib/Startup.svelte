<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { animate } from "motion/mini";
  import type { RuntimeStatus } from "../model";
  import { startupRows, startupPolling } from "./startup.js";
  import Logo from "./Logo.svelte";

  let {
    settings,
    resources,
    needsSetup,
    settingsFailed = false,
    resourcesFailed = false,
    titlebarHeight = 32,
    animationMs = 250,
    oncontinue,
  }: {
    settings: boolean;
    resources: boolean;
    needsSetup: boolean;
    settingsFailed?: boolean;
    resourcesFailed?: boolean;
    titlebarHeight?: number;
    animationMs?: number;
    oncontinue: () => void;
  } = $props();
  let status = $state<RuntimeStatus | null>(null);
  let error = $state("");
  let reduced = $state(false);
  const motionDisabled = $derived(reduced || animationMs <= 0);
  const transitionSeconds = $derived(Math.max(0, animationMs) / 1000);
  let mounted = $state(false);
  let closing = false;
  let alive = true;
  let surface: HTMLElement;
  let spinner: SVGRectElement;
  let fill: HTMLDivElement;
  let exit: ReturnType<typeof animate> | undefined;
  let poller: ReturnType<typeof startupPolling>;
  const rows = $derived(
    startupRows({
      settings,
      resources,
      needsSetup,
      settingsFailed,
      resourcesFailed,
      engines: status?.engines ?? null,
      error,
    }),
  );
  const completed = $derived(rows.filter((row) => row.state !== "loading").length);
  const needsAttention = $derived(!needsSetup && rows.some((row) => row.state === "attention"));

  async function leave() {
    if (closing) return;
    closing = true;
    poller?.dispose();
    if (!motionDisabled) {
      try {
        exit = animate(surface, { opacity: 0 }, { duration: transitionSeconds });
        await exit.finished;
      } catch {
        /* Continue even if animation is unavailable. */
      }
    }
    if (alive) oncontinue();
  }

  onMount(() => {
    const preference = matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => {
      reduced = preference.matches;
    };
    update();
    preference.addEventListener("change", update);
    poller = startupPolling(
      () => invoke<RuntimeStatus>("runtime_status"),
      (next, failure) => {
        status = next;
        error = failure;
      },
    );
    void poller.refresh();
    mounted = true;
    return () => {
      alive = false;
      exit?.cancel();
      poller.dispose();
      preference.removeEventListener("change", update);
    };
  });

  $effect(() => {
    if (!mounted || motionDisabled) return;
    const spin = animate(
      spinner,
      { strokeDashoffset: ["0", "-100"] },
      { duration: Math.max(0.75, transitionSeconds * 6), repeat: Infinity, ease: "linear" },
    );
    return () => {
      spin.cancel();
    };
  });
  $effect(() => {
    const ratio = completed / rows.length;
    if (!mounted) return;
    if (motionDisabled) {
      fill.style.transform = `scaleX(${ratio})`;
      return;
    }
    const progress = animate(
      fill,
      { transform: `scaleX(${ratio})` },
      { duration: transitionSeconds, ease: "easeOut" },
    );
    return () => {
      progress.stop();
    };
  });
  $effect(() => {
    if (mounted && completed === rows.length && !error && !needsAttention) void leave();
  });
</script>

<section
  class="startup"
  style:top={`${titlebarHeight}px`}
  aria-label="Starting Sakatsu Reader"
  data-keeps-focus
  data-motion={motionDisabled ? "reduced" : "full"}
  bind:this={surface}
>
  <div class="intro">
    <div class="emblem">
      <svg class="loading-outline" viewBox="0 0 100 100" aria-hidden="true">
        <rect class="outline-track" x="2" y="2" width="96" height="96" rx="17.28" />
        <rect
          class="loading-trace"
          x="2"
          y="2"
          width="96"
          height="96"
          rx="17.28"
          pathLength="100"
          bind:this={spinner}
        />
      </svg>
      <Logo size="5.5rem" />
    </div>
    <h1>Sakatsu Reader</h1>
    <p class="welcome">Getting your reading space ready.</p>
    <div
      class="track"
      role="progressbar"
      aria-label="Startup checks"
      aria-valuemin="0"
      aria-valuemax={rows.length}
      aria-valuenow={completed}
      aria-valuetext={`${completed} of ${rows.length} startup checks finished`}
    >
      <div class="fill" bind:this={fill}></div>
    </div>
    <p class="count" aria-live="polite" aria-atomic="true">
      {completed} of {rows.length} checks finished
    </p>
    <ul>
      {#each rows as row (row.id)}
        <li data-state={row.state}>
          <span class="indicator" aria-hidden="true"
            >{row.state === "ready"
              ? "✓"
              : row.state === "attention"
                ? "!"
                : row.state === "deferred"
                  ? "·"
                  : "○"}</span
          >
          <div><span class="name">{row.name}</span><span class="detail">{row.detail}</span></div>
        </li>
      {/each}
    </ul>
    {#if needsAttention && !error}<p class="note warn" role="status">
        Some startup checks need attention. You can continue reading.
      </p>{/if}
    {#if error}<p class="note warn" role="status">{error}</p>
      <button
        class="control"
        onclick={() => {
          error = "";
          void poller.refresh();
        }}>Try again</button
      >{/if}
    <button class="control continue" onclick={leave}>Continue to reader</button>
    <p class="hint">Loading continues in the background.</p>
  </div>
</section>

<style>
  .startup {
    position: absolute;
    z-index: 2;
    inset: 0;
    background: var(--bg);
    display: grid;
    place-items: center;
    overflow: auto;
    padding: var(--gap-lg);
  }
  .intro {
    width: min(100%, 25rem);
    margin: auto;
    padding-block: var(--gap-lg);
    text-align: center;
  }
  .emblem {
    position: relative;
    width: 7rem;
    height: 7rem;
    margin: 0 auto var(--gap-lg);
    display: grid;
    place-items: center;
  }
  /* Move the stroke along the logo's contour; rotating a square would leave its outline. */
  .loading-outline {
    position: absolute;
    width: 5.75rem;
    height: 5.75rem;
    fill: none;
    pointer-events: none;
  }
  .outline-track {
    stroke: var(--border);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .loading-trace {
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linecap: round;
    stroke-dasharray: 24 76;
    stroke-dashoffset: 0;
    vector-effect: non-scaling-stroke;
  }
  h1 {
    font-family: var(--font-display);
    font-size: 1.75rem;
    font-weight: 500;
    margin: 0 0 var(--gap-xs);
  }
  .welcome {
    color: var(--text-dim);
    margin: 0 0 var(--gap-lg);
  }
  .track {
    height: 0.25rem;
    overflow: hidden;
    border-radius: 1rem;
    background: var(--border);
  }
  .fill {
    width: 100%;
    height: 100%;
    transform: scaleX(0);
    transform-origin: left;
    background: var(--accent);
  }
  .count,
  .hint {
    color: var(--text-dim);
    font-size: 0.8rem;
  }
  .count {
    text-align: right;
    margin: var(--gap-xs) 0 var(--gap);
  }
  ul {
    list-style: none;
    padding: 0;
    margin: 0 0 var(--gap-lg);
    text-align: left;
  }
  li {
    display: flex;
    align-items: baseline;
    gap: var(--gap-sm);
    padding-block: var(--gap-sm);
    border-bottom: 1px solid var(--border);
  }
  .indicator {
    width: 1.5em;
    flex: none;
    color: var(--accent);
    text-align: center;
  }
  li > div {
    display: flex;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--gap-xs) var(--gap);
    flex: 1;
    min-width: 0;
  }
  .name {
    font-weight: 500;
  }
  .detail {
    color: var(--text-dim);
    font-size: 0.85rem;
  }
  li[data-state="attention"] .indicator {
    color: var(--yellow);
  }
  .continue {
    margin-top: var(--gap-sm);
  }
  .hint {
    margin: var(--gap-sm) 0 0;
  }
</style>
