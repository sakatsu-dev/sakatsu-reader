<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import type { UpdateStatus } from "../bindings";
  import { humanBytes } from "./setup-store.svelte";
  import { errorMessage } from "./save-notice.js";

  let {
    enabled = $bindable(true),
    oninstall,
  }: {
    enabled?: boolean;
    oninstall: () => Promise<UpdateStatus>;
  } = $props();
  let status = $state<UpdateStatus | null>(null);
  let error = $state("");
  let pending = $state(false);
  const busy = $derived(pending || status?.phase === "checking" || status?.phase === "downloading");

  function accept(next: UpdateStatus) {
    if (!status || next.generation >= status.generation) status = next;
  }
  async function run(action: string) {
    pending = true;
    error = "";
    try {
      if (action === "update_install") accept(await oninstall());
      else
        accept(
          await invoke<UpdateStatus>(action, action === "update_check" ? { manual: true } : {}),
        );
      accept(await invoke<UpdateStatus>("update_status"));
    } catch (thrown) {
      error = errorMessage(thrown);
    } finally {
      pending = false;
    }
  }
  onMount(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    // Subscribe first, then read: a completed background check must not get lost between them.
    listen<UpdateStatus>("update-status", ({ payload }) => {
      if (!disposed) accept(payload);
    })
      .then(async (stop) => {
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        const initial = await invoke<UpdateStatus>("update_status");
        if (!disposed) accept(initial);
      })
      .catch((thrown) => {
        if (!disposed) error = errorMessage(thrown);
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });
</script>

<h3>App updates</h3>
<label><input type="checkbox" bind:checked={enabled} /> Check for updates when Sakatsu opens</label>
<p class="note">
  Checks at most once a day. Only the release service is contacted; captured text is never sent. You
  choose when to download and install.
</p>
<div aria-live="polite">
  {#if !status}
    <p>Reading update status…</p>
  {:else if status.phase === "not_configured"}
    <p>Updates are not configured in this build.</p>
  {:else if status.phase === "checking"}
    <p>Checking for updates…</p>
  {:else if status.phase === "current"}
    <p>You have the latest version.</p>
  {:else if status.phase === "available"}
    <p>Version {status.version} is available.</p>
  {:else if status.phase === "downloading"}
    <p>
      Downloaded {humanBytes(status.downloaded)}{status.total === null
        ? ""
        : ` of ${humanBytes(status.total)}`}.
    </p>
  {:else if status.phase === "ready"}
    <p>Version {status.version} is ready. Installing will close and restart Sakatsu.</p>
  {:else if status.phase === "offline"}
    <p>The release service could not be reached.</p>
  {:else if status.phase === "idle"}
    <p>Check for a new version when you are ready.</p>
  {/if}
</div>
{#if status?.notes}<details>
    <summary>Release notes</summary>
    <p class="notes">{status.notes}</p>
  </details>{/if}
{#if error || status?.error}<p role="alert">{error || status?.error}</p>{/if}
<div class="actions">
  <button
    class="control"
    type="button"
    disabled={busy || status?.phase === "not_configured"}
    onclick={() => run("update_check")}>Check for updates</button
  >
  {#if status?.version && ["available", "failed"].includes(status.phase)}
    <button
      class="control primary"
      type="button"
      disabled={busy}
      onclick={() => run("update_download")}>Download update</button
    >
  {/if}
  {#if status?.phase === "ready"}
    <button
      class="control primary"
      type="button"
      disabled={busy}
      onclick={() => run("update_install")}>Restart and install</button
    >
  {/if}
</div>

<style>
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--gap-sm);
  }
  .note {
    color: var(--text-dim);
  }
  .notes {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
