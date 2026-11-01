<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import type { DictionaryManagerSnapshot } from "../bindings";
  let { onmanage }: { onmanage: () => void } = $props();
  let available = $state("");
  let dismissed = $state("");
  onMount(() => {
    let active = true;
    let request = 0;
    let stop: (() => void) | undefined;
    const refresh = async () => {
      const generation = ++request;
      try {
        const result = await invoke<DictionaryManagerSnapshot>("dictionary_manager");
        if (active && generation === request)
          available = result.dictionaries
            .filter((row) => row.available_revision)
            .map((row) => `${row.id}:${row.available_revision}`)
            .sort()
            .join("|");
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

{#if available && available !== dismissed}
  <aside class="dictionary-update-notice" role="status">
    <span>Dictionary updates are available.</span>
    <button class="control" onclick={onmanage}>Review dictionary updates</button>
    <button class="control" onclick={() => (dismissed = available)}>Dismiss</button>
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
