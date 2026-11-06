<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import type { DictionaryManagerSnapshot } from "../bindings";
  import { errorMessage } from "./save-notice.js";
  let { onmanage }: { onmanage: () => void } = $props();
  let updates = $state<Record<string, string>>({});
  let titles = $state<string[]>([]);
  let dismissing = $state(false);
  let error = $state("");
  async function dismiss() {
    dismissing = true;
    error = "";
    try {
      await invoke("dictionary_dismiss_updates", { updates: { ...updates } });
    } catch (thrown) {
      error = errorMessage(thrown);
    } finally {
      dismissing = false;
    }
  }
  onMount(() => {
    let active = true;
    let request = 0;
    let stop: (() => void) | undefined;
    const refresh = async () => {
      const generation = ++request;
      try {
        const result = await invoke<DictionaryManagerSnapshot>("dictionary_manager");
        if (active && generation === request) {
          const rows = result.preferences.notify_updates
            ? result.dictionaries.filter(
                (row) =>
                  row.available_revision &&
                  result.dismissed_updates[row.id] !== row.available_revision,
              )
            : [];
          updates = Object.fromEntries(rows.map((row) => [row.id, row.available_revision!]));
          titles = rows.map((row) => row.title);
        }
      } catch {
        /* Detailed status and retry remain in Settings. */
      }
    };
    void refresh();
    void listen("dictionary-manager-changed", refresh)
      .then((unlisten) => {
        if (active) stop = unlisten;
        else unlisten();
      })
      .catch(() => {
        /* Browser preview has no desktop event channel. */
      });
    return () => {
      active = false;
      ++request;
      stop?.();
    };
  });
</script>

{#if titles.length}
  <aside class="dictionary-update-notice" role="status">
    <span>Dictionary updates available: {titles.join(", ")}.</span>
    <button class="control" onclick={onmanage}>Review dictionary updates</button>
    <button class="control" disabled={dismissing} onclick={dismiss}>Dismiss</button>
    {#if error}<p role="alert">{error}</p>{/if}
  </aside>
{/if}

<style>
  .dictionary-update-notice {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--gap-sm);
    padding: var(--gap-sm) var(--gap);
    border: 1px solid var(--accent-edge);
    border-radius: var(--radius);
    background: var(--accent-wash);
  }
  span {
    flex: 1;
    min-width: 10rem;
  }
</style>
