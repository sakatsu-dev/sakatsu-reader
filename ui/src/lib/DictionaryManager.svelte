<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { open } from "@tauri-apps/plugin-dialog";
  import type {
    DictionaryManagerSnapshot,
    DictionaryPreferences,
    ImportPreview,
  } from "../bindings";
  import { errorMessage } from "./save-notice.js";

  let snapshot = $state<DictionaryManagerSnapshot | null>(null);
  let error = $state("");
  let loading = $state(true);
  let choosing = $state(false);
  let preview = $state<ImportPreview | null>(null);
  let target = $state("");
  let dropArea = $state<HTMLDivElement>();
  let over = $state(false);
  let request = 0;
  let previewRequest = 0;
  let disposed = false;
  const operation = $derived(snapshot?.operation);
  const busy = $derived(
    Boolean(operation && !["complete", "failed", "cancelled"].includes(operation.phase)),
  );
  const words = $derived(snapshot?.dictionaries.filter((row) => row.kind === "words") ?? []);

  async function refresh() {
    const generation = ++request;
    try {
      const result = await invoke<DictionaryManagerSnapshot>("dictionary_manager");
      if (disposed || generation !== request) return;
      snapshot = result;
      error = "";
    } catch (thrown) {
      if (!disposed && generation === request) error = errorMessage(thrown);
    } finally {
      if (!disposed && generation === request) loading = false;
    }
  }
  async function act(command: string, args?: Record<string, unknown>) {
    error = "";
    try {
      await invoke(command, args);
      await refresh();
    } catch (thrown) {
      error = errorMessage(thrown);
    }
  }
  async function preferences(change: Partial<DictionaryPreferences>) {
    if (!snapshot) return;
    await act("dictionary_preferences", { preferences: { ...snapshot.preferences, ...change } });
  }
  async function inspect(file: string) {
    if (disposed || busy || choosing) return;
    choosing = true;
    error = "";
    const generation = ++previewRequest;
    try {
      const found = await invoke<ImportPreview>("dictionary_preview", { file });
      if (disposed || generation !== previewRequest) return;
      preview = found;
      target = found.kind === "kanjidic" ? "kanjidic2" : "";
    } catch (thrown) {
      if (!disposed) error = errorMessage(thrown);
    } finally {
      choosing = false;
    }
  }
  async function choose() {
    error = "";
    try {
      const file = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Dictionary archives", extensions: ["zip", "gz"] }],
      });
      if (typeof file === "string") await inspect(file);
    } catch (thrown) {
      error = errorMessage(thrown);
    }
  }
  async function install() {
    if (!preview) return;
    const id = preview.id;
    error = "";
    try {
      await invoke("dictionary_import", { previewId: id, target: target || null });
      preview = null;
      await refresh();
    } catch (thrown) {
      error = errorMessage(thrown);
    }
  }
  async function dismiss() {
    const id = preview?.id;
    preview = null;
    if (id) await act("dictionary_cancel", { operationId: id });
  }
  onMount(() => {
    const cleanup: (() => void)[] = [];
    const retain = (stop: () => void) => {
      if (disposed) stop();
      else cleanup.push(stop);
    };
    void refresh();
    void listen("dictionary-manager-changed", () => refresh())
      .then(retain)
      .catch((thrown) => (error = errorMessage(thrown)));
    void getCurrentWebviewWindow()
      .onDragDropEvent(({ payload }) => {
        if (payload.type === "leave") {
          over = false;
          return;
        }
        const rect = dropArea?.getBoundingClientRect();
        const x = payload.position.x / window.devicePixelRatio;
        const y = payload.position.y / window.devicePixelRatio;
        const inside = Boolean(
          rect && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom,
        );
        over = inside && !busy;
        if (payload.type === "drop" && inside) {
          over = false;
          if (payload.paths.length !== 1) error = "Choose one dictionary archive at a time.";
          else void inspect(payload.paths[0]);
        }
      })
      .then(retain)
      .catch(() => {
        /* The file picker remains available without native drop support. */
      });
    return () => {
      disposed = true;
      ++request;
      ++previewRequest;
      for (const stop of cleanup) stop();
    };
  });
</script>

<div class="dictionary-manager">
  <h4 class="subhead">Dictionaries</h4>
  <button class="control" onclick={() => act("dictionary_open_source", { id: "resource-licences" })}
    >Resource licences</button
  >
  <p class="note">
    Use the included snapshots, import a downloaded dictionary, or update from its publisher.
  </p>
  {#if loading}
    <p role="status">Reading installed dictionaries…</p>
  {:else if snapshot}
    <div class="preferences">
      <label
        ><input
          type="checkbox"
          checked={snapshot.preferences.check_updates}
          onchange={(event) => preferences({ check_updates: event.currentTarget.checked })}
        /> Check for dictionary updates daily</label
      >
      <label
        ><input
          type="checkbox"
          checked={snapshot.preferences.automatic}
          onchange={(event) => preferences({ automatic: event.currentTarget.checked })}
        /> Download and install updates automatically</label
      >
      <p class="note">
        Automatic updates use approved publishers and wait for a safe installation point. Ichiran
        and OCR models are managed separately.
      </p>
    </div>
    <button class="control" disabled={busy} onclick={() => act("dictionary_check")}
      >Check for dictionary updates</button
    >
    {#if snapshot.dictionaries.length === 0}
      <p class="note">No dictionary is installed yet. Complete setup or choose an archive below.</p>
    {:else}
      <ul class="dictionaries">
        {#each snapshot.dictionaries as row (row.id)}
          <li>
            <strong>{row.title}</strong>
            <p class="note">
              {row.revision || "Revision not recorded"}{row.entries !== null
                ? ` · ${row.entries.toLocaleString()} entries`
                : ""}
            </p>
            {#if row.home || row.source}
              <a
                href={row.home ?? row.source ?? ""}
                target="_blank"
                rel="noopener noreferrer"
                onclick={(event) => {
                  event.preventDefault();
                  void act("dictionary_open_source", { id: row.id });
                }}>Publisher and downloads</a
              >
            {/if}
            {#if row.source && !row.consented}
              <p class="endpoint">Update endpoint: {row.source}</p>
              <button
                class="control"
                onclick={() =>
                  act("dictionary_source_consent", {
                    id: row.id,
                    endpoint: row.source,
                    allowed: true,
                  })}>Allow publisher checks</button
              >
            {/if}
            <p class="note" role="status">
              {row.phase === "current"
                ? "Up to date."
                : row.phase === "checking"
                  ? "Checking publisher…"
                  : row.detail}
            </p>
            {#if row.available_revision}
              <button
                class="control"
                disabled={busy}
                onclick={() => act("dictionary_update", { id: row.id })}
                >Update to {row.available_revision}</button
              >
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    <div class="drop-area" class:over bind:this={dropArea}>
      <p>Drop a dictionary ZIP or KANJIDIC2 .xml.gz here.</p>
      <button class="control" disabled={busy || choosing} onclick={choose}
        >{choosing ? "Reading archive…" : "Choose dictionary file"}</button
      >
    </div>
    {#if preview}
      <div class="preview" aria-label="Dictionary import preview">
        <strong>{preview.title}</strong>
        <p>{preview.revision}</p>
        <p class="note">SHA-256 identifies this file; it does not verify its publisher.</p>
        {#if preview.kind === "words"}
          <label
            >Install as
            <select bind:value={target}>
              <option value="">Add a separate dictionary</option>
              {#each words as row (row.id)}<option value={row.id}>Replace {row.title}</option
                >{/each}
            </select>
          </label>
        {:else}<p class="note">
            Replaces KANJIDIC2. Stroke diagrams and component data are kept.
          </p>{/if}
        <div class="actions">
          <button class="control primary" disabled={busy} onclick={install}
            >Install dictionary</button
          ><button class="control" onclick={dismiss}>Cancel import</button>
        </div>
      </div>
    {/if}
    {#if operation}
      <p role="status">
        {operation.detail || operation.phase}{operation.bytes
          ? ` (${Math.round(operation.bytes / 1024)} KB)`
          : ""}
      </p>
      {#if busy}<button
          class="control"
          onclick={() => act("dictionary_cancel", { operationId: operation.id })}
          >Cancel dictionary operation</button
        >{/if}
    {/if}
  {/if}
  {#if error}<p class="note warn" role="alert">{error}</p>
    <button class="control" onclick={refresh}>Read dictionary status again</button>{/if}
</div>

<style>
  .dictionary-manager {
    min-width: 0;
    padding: var(--gap) 0;
  }
  .preferences {
    display: grid;
    gap: var(--gap-sm);
    margin: var(--gap) 0;
  }
  .preferences label {
    display: flex;
    align-items: baseline;
    gap: var(--gap-sm);
  }
  .dictionaries {
    list-style: none;
    padding: 0;
    margin: var(--gap) 0;
  }
  .dictionaries li {
    border-top: 1px solid var(--border);
    padding: var(--gap) 0;
    overflow-wrap: anywhere;
  }
  .dictionaries p {
    margin: var(--gap-xs) 0;
  }
  .endpoint {
    font-size: 0.85em;
    overflow-wrap: anywhere;
  }
  .drop-area,
  .preview {
    margin: var(--gap) 0;
    padding: var(--gap);
    border: 1px dashed var(--border);
    border-radius: var(--radius);
  }
  .drop-area.over {
    border-color: var(--accent);
    background: var(--accent-wash);
  }
  .preview {
    border-style: solid;
    overflow-wrap: anywhere;
  }
  .preview select {
    display: block;
    max-width: 100%;
    margin: var(--gap-sm) 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--gap-sm);
    margin-top: var(--gap);
  }
</style>
