<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { setup } from "./setup-store.svelte";
  import RecoveryPanel from "./RecoveryPanel.svelte";
  import type { RecoveryReport } from "../model";

  // Compatibility props remain while the old setup list is retired from App. Keeping the
  // boundary stable avoids turning a setup redesign into an unrelated settings migration.
  let {
    onready,
    hidden: _hidden = false,
    onhide: _onhide,
    onadopt: _onadopt,
  }: {
    onready: () => void;
    hidden?: boolean;
    onhide: () => void;
    onadopt: () => void;
  } = $props();

  const incomplete = $derived(
    setup.loaded && setup.snapshot && !setup.snapshot.required_ready,
  );
  const active = $derived(
    setup.snapshot?.phase === "downloading" ||
      setup.snapshot?.phase === "installing" ||
      setup.snapshot?.phase === "verifying",
  );

  async function resume() {
    await setup.start();
    onready();
  }

  // **Asked for on every look, never cached.** A recovery screen answering from a snapshot would go
  // on describing a transaction somebody else resolved, and the identity check on the actions
  // exists precisely because this panel is expected to be out of date sometimes.
  let recovery = $state<RecoveryReport | null>(null);
  const loadRecovery = async () => {
    try {
      recovery = await invoke<RecoveryReport>("recovery_report");
    } catch {
      // A report that cannot be fetched is not worth an error of its own: the setup panel has
      // other things to say, and the session log carries the reason.
      recovery = null;
    }
  };
  $effect(() => {
    void loadRecovery();
  });
</script>

<RecoveryPanel
  report={recovery}
  keepCurrent={(container, transaction, published) =>
    invoke("recovery_keep_current", { container, transaction, published })}
  restorePrevious={(container, transaction, published) =>
    invoke("recovery_restore_previous", { container, transaction, published })}
  refresh={loadRecovery}
/>

{#if incomplete}
  <aside class="notice" role="status">
    <span class="dot" aria-hidden="true"></span>
    <span>
      {#if active}
        Screen capture setup is continuing in the background.
      {:else}
        Screen capture setup is incomplete.
      {/if}
    </span>
    {#if active}
      <button class="control" onclick={() => setup.pause()}>Pause</button>
    {:else}
      <button class="control" onclick={resume}>Resume setup</button>
    {/if}
  </aside>
{/if}

<style>
  .notice {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    min-width: 0;
    padding: var(--gap-sm) var(--gap);
    border: 1px solid var(--accent-edge);
    border-radius: var(--radius);
    background: var(--accent-wash);
    color: var(--text-dim);
    font-size: 0.82rem;
  }
  .notice > span:nth-child(2) { flex: 1; min-width: 0; }
  .dot { width: 0.42rem; height: 0.42rem; border-radius: 50%; background: var(--accent); flex: 0 0 auto; }
  button { padding: 0.2rem 0.55rem; color: var(--accent); flex: 0 0 auto; }
  @media (max-width: 30rem) {
    .notice { align-items: flex-start; flex-wrap: wrap; }
    .notice button { margin-left: calc(0.42rem + var(--gap-sm)); }
  }
</style>
