<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  import { readSpeed } from "./vlm-speed.js";

  type Status = {
    installed: boolean;
    ready: boolean;
    endpoint: string;
    model: string;
    detail: string;
    /** Null until setup has timed a read on this machine. Absence is not slowness. */
    secondsPerRead: number | null;
  };

  let {
    onready,
    onconfigure,
    onchoose,
    selected = false,
    endpoint = "",
    size = "",
  }: {
    onready?: () => void;
    onconfigure?: (endpoint: string, model: string) => void;
    onchoose?: () => void;
    /** Whether PaddleOCR-VL is the chosen engine. Not the same as being pointed here. */
    selected?: boolean;
    /** The endpoint the reader is currently configured to use. */
    endpoint?: string;
    /**
     * What the download costs, from the catalog.
     *
     * **Not written here.** The button used to say "1.8 GB" as a string literal, which is a
     * second place the weights' size lives and the one nobody updates when the model moves.
     * Empty means the catalog has not answered yet, and the button simply omits the figure
     * rather than stating one this build cannot vouch for.
     */
    size?: string;
  } = $props();

  let status = $state<Status | null>(null);
  let working = $state(false);
  let result = $state("");

  // Whether this machine is quick enough to recommend, and how to say so. The rules live in
  // `vlm-speed.js` rather than here because `node --test` cannot import a rune-using module,
  // and the threshold is worth a test.
  const speed = $derived(readSpeed(status?.secondsPerRead));

  // **Two conditions, not one.** Being set to `vlm` is not the same as being set to *this*
  // install: a stored endpoint from an earlier experiment stays exactly where it was put, so
  // the engine can be selected and pointed somewhere that answers nothing. Treating the two as
  // one hid the button that fixes it behind the very state that needed fixing.
  const pointedHere = $derived(status !== null && endpoint.trim() === status.endpoint);

  // **A recommendation and a repair are gated differently.** Offering this as the everyday
  // reader depends on the machine being quick enough to deserve it. Repointing a reader that
  // is already selected but aimed somewhere dead is a repair, and a slow machine needs that
  // just as much — gating both on speed left the only fix hidden on the machines least able
  // to do without it.
  const offer = $derived(speed.fast && !selected);
  const repair = $derived(selected && !pointedHere);

  async function refresh() {
    try {
      status = await invoke<Status>("vlm_setup_status");
    } catch (thrown) {
      result = String(thrown);
    }
  }

  /// **An ordinary download now.**
  ///
  /// This used to open a PowerShell window that installed Ollama — a system-wide runtime with
  /// a tray application and a startup entry, which no progress bar could honestly represent.
  /// What it fetches instead is a llama.cpp build into the app's own folder, so it goes through
  /// `fetch_component` like every other download and reports progress in the same place.
  async function setup() {
    working = true;
    result = "Downloading…";
    try {
      await invoke("fetch_component", { id: "paddle-vl" });
    } catch (thrown) {
      working = false;
      result = String(thrown);
    }
  }

  async function start() {
    working = true;
    result = "Starting PaddleOCR-VL…";
    try {
      await invoke("start_vlm_runtime");
      for (let attempt = 0; attempt < 40; attempt += 1) {
        await new Promise((resolve) => setTimeout(resolve, 500));
        await refresh();
        if (status?.ready) {
          result = "PaddleOCR-VL is ready.";
          onconfigure?.(status.endpoint, status.model);
          return;
        }
      }
      result = "The local service is taking longer than expected. Try Start again.";
    } catch (thrown) {
      result = String(thrown);
    } finally {
      working = false;
    }
  }

  onMount(() => {
    void refresh();
    let disposed = false;
    let stop: (() => void) | undefined;
    // `fetch-done` rather than an event of this component's own: the download goes through
    // the same machinery as every other component now, so it reports completion the same way.
    // Filtered by id, because every component's completion arrives here.
    void listen<{ id: string; ok: boolean; detail: string }>(
      "fetch-done",
      async (event) => {
        if (event.payload.id !== "paddle-vl") return;
        working = false;
        result = event.payload.detail;
        if (event.payload.ok) {
          await refresh();
          if (status) onconfigure?.(status.endpoint, status.model);
          onready?.();
        }
      },
    ).then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  });
</script>

<div class="vlm-setup">
  {#if status?.ready}
    <span class="ready">● Ready</span>
    <!-- The offer is made where the choice lives, not in the setup window, because the window
         cannot change a setting while the app that owns it is running. It appears only once a
         read has been timed and only when that read was quick: recommending a reader that
         takes a minute a capture would be the dishonest half of measuring at all. -->
    {#if offer || repair}
      <!-- Points the reader at this install as well as selecting the engine. Selecting alone
           left whatever endpoint was already stored — an address from an earlier experiment,
           say — so choosing the model that had just been measured as ready gave a capture that
           failed on a server nobody was running. -->
      <button
        class="control"
        onclick={() => {
          if (status) onconfigure?.(status.endpoint, status.model);
          onchoose?.();
        }}
      >
        {repair ? "Point it at this install" : "Use it as my reader"}
      </button>
    {/if}
  {:else if status?.installed}
    <button class="control" onclick={start} disabled={working}>
      {working ? "Starting…" : "Start PaddleOCR-VL"}
    </button>
  {:else}
    <button class="control" onclick={setup} disabled={working}>
      {working ? "Downloading…" : size ? `Set up PaddleOCR-VL · ${size}` : "Set up PaddleOCR-VL"}
    </button>
  {/if}
  <span class:failed={Boolean(result) && !working && !status?.ready} class="status" role="status">
    {result || status?.detail || "Checking local setup…"}
    {#if speed.timed}
      <span class="timed" class:slow={!speed.fast}>
        {#if speed.fast}
          Reads a capture in {speed.perCapture} on this machine.
        {:else}
          Reads a capture in {speed.perCapture} on this machine — accurate, but slow enough
          that Auto stays the better default.
        {/if}
      </span>
    {/if}
  </span>
</div>

<style>
  .vlm-setup {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: var(--gap-xs);
    margin: 0 0 var(--gap-sm);
  }
  button {
    padding: 0.18rem 0.55rem;
    font-size: 0.76rem;
    white-space: nowrap;
    color: var(--accent);
  }
  button:hover:not(:disabled) {
    border-color: var(--accent-edge);
    background: var(--accent-wash);
  }
  .ready {
    color: var(--good, var(--accent));
    font-size: 0.76rem;
  }
  .status {
    flex: 1 1 15rem;
    color: var(--text-faint);
    font-size: 0.74rem;
    line-height: 1.45;
  }
  .status.failed {
    color: var(--red);
  }
  /* Its own element rather than more of the sentence before it: the measurement is about this
     machine, and it reads as a fact about the machine only if it looks separate from the
     service's own state. */
  .timed {
    display: block;
    color: var(--text-faint);
  }
  /* A step brighter, not dimmer. The slow reading is the one carrying a caveat, and a caveat
     set fainter than the sentence it qualifies is a caveat nobody reads. */
  .timed.slow {
    color: var(--text-dim);
  }
</style>
