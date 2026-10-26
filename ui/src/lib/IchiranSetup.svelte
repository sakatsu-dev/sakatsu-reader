<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { errorMessage } from "./save-notice.js";

  let {
    size = "",
    onadopt,
    onrunning = () => {},
    disabled = false,
    recommended = false,
  }: {
    /**
     * Stated on the action itself because the database is a consequential download.
     *
     * **From the catalog, and with no default figure.** This defaulted to `"4.7 GB"`, which was
     * a second home for a number the catalog already owns — harmless while `Settings.svelte`
     * passed the real one, and wrong the moment anything rendered this without it.
     */
    size?: string;
    /**
     * Pick Ichiran up, now that it answers.
     *
     * This was `onrestart`, and the button said "Restart to use Ichiran" — the analyzer was
     * chosen once at startup, so a sidecar that came up afterwards could not be reached until
     * the app was reopened. It is chosen again on demand now: the same probe, run a second
     * time, against a machine where the setup has finished.
     */
    onadopt: () => void;
    onrunning?: (running: boolean) => void;
    disabled?: boolean;
    recommended?: boolean;
  } = $props();

  let running = $state(false);
  let result = $state("");
  let ready = $state(false);
  let listening = $state(false);
  /// Why the completion event could not be subscribed to. Setup stays disabled while this is set,
  /// because a finish nobody hears would leave "Setup window open" on screen for ever — but it is
  /// said, and it can be tried again, rather than being a button that is simply grey.
  let listenError = $state("");

  async function start() {
    running = true;
    onrunning(true);
    ready = false;
    result = "Opening setup…";
    try {
      const started = await invoke<{ detail: string }>("start_ichiran_setup");
      // A very fast readiness check can finish before this invoke resolves. Do not
      // overwrite its final event with the earlier "window opened" acknowledgement.
      if (running) result = started.detail;
    } catch (thrown) {
      running = false;
      onrunning(false);
      result = String(thrown);
    }
  }

  let disposed = false;
  let stop: (() => void) | undefined;
  /// Each attempt's number, so a slow earlier attempt cannot install itself over a later one.
  let attempt = 0;

  function subscribe() {
    const mine = ++attempt;
    listenError = "";
    listen<{ ok: boolean; detail: string }>("ichiran-setup-finished", (event) => {
      running = false;
      onrunning(false);
      ready = event.payload.ok;
      result = event.payload.detail;
    }).then(
      (unlisten) => {
        // Disposed, or superseded by a retry: this subscription is nobody's, so release it.
        if (disposed || mine !== attempt) unlisten();
        else {
          stop = unlisten;
          listening = true;
        }
      },
      (thrown) => {
        if (disposed || mine !== attempt) return;
        listenError = `Sakatsu could not listen for setup to finish: ${errorMessage(thrown)}`;
      },
    );
  }

  onMount(() => {
    subscribe();
    return () => {
      disposed = true;
      stop?.();
    };
  });
</script>

<!-- `compact` is the Settings form: a small outlined action in a note-sized row. Onboarding asks for
     the prominent form, which is the shared filled control at ordinary size — and the compact rules
     must not reach it, because scoped they outrank `.control.primary` and would paint accent text
     on an accent fill (`local documentation`, scoping outranks a global class). -->
<div class="ichiran-setup" class:compact={!recommended}>
  {#if ready}
    <button class="control restart" class:primary={recommended} onclick={onadopt} {disabled}>Use Ichiran now</button>
  {:else}
    <!-- The recommended form is the shared filled control, not a local copy of one. -->
    <button class="control install" class:primary={recommended} onclick={start} disabled={running || disabled || !listening}>
      {running ? "Setup window open" : size ? `Set up Ichiran · ${size}` : "Set up Ichiran"}
    </button>
  {/if}
  {#if listenError}
    <span class="status failed" role="alert">{listenError}</span>
    <button class="control" onclick={subscribe}>Try again</button>
  {:else}
    <span class:failed={Boolean(result) && !running && !ready} class:ready={ready} class="status" role="status">
      {result || "Checks WSL and Docker first, then guides each required step."}
    </span>
  {/if}
</div>

<style>
  .ichiran-setup {
    grid-column: 1 / -1;
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: var(--gap-xs);
    margin-top: 0.15rem;
  }
  button {
    white-space: nowrap;
  }
  .compact button {
    padding: 0.18rem 0.55rem;
    font-size: 0.76rem;
  }
  .compact .install,
  .compact .restart {
    color: var(--accent);
  }
  .compact .install:hover:not(:disabled),
  .compact .restart:hover:not(:disabled) {
    border-color: var(--accent-edge);
    background: var(--accent-wash);
  }
  .status {
    flex: 1 1 14rem;
    color: var(--text-faint);
    font-size: 0.78rem;
    line-height: 1.45;
  }
  .compact .status {
    font-size: 0.74rem;
  }
  .status.failed {
    color: var(--red);
  }
  .status.ready {
    color: var(--good, var(--accent));
  }
</style>
