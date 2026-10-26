<script lang="ts">
  // What an interrupted installation left, and what can be done about it.
  //
  // **A person with an unfinished install does not have two problems**, so the two mechanisms —
  // dictionary generations and directory swaps — are drawn as one list. What they do have is four
  // different situations, and the whole point of this panel is that they read differently:
  //
  //   - something to finish, where both actions are offered;
  //   - a competing newer installation, where only keeping is;
  //   - a record nothing can read, where no action is offered at all and the path is copyable;
  //   - a restoration that was tried and failed, which is not the same as a file already being
  //     there — the distinction the report carries because conflating them told readers something
  //     that may be false.
  //
  // Every action carries the transaction and published generation it was shown for. A screen is a
  // picture of a moment, and the backend refuses one that has passed; when that happens this
  // refreshes rather than reporting a failure, because nothing is wrong — the picture was old.
  import type { DictionaryRecovery, InstallRecovery, RecoveryReport } from "../model";
  import { isPersistenceError } from "./save-notice.js";

  let {
    report,
    keepCurrent,
    restorePrevious,
    refresh,
  }: {
    report: RecoveryReport | null;
    keepCurrent: (container: string, transaction: string, published: string) => Promise<void>;
    restorePrevious: (container: string, transaction: string, published: string) => Promise<void>;
    refresh: () => Promise<void>;
  } = $props();

  /**
   * What just happened, and it outlives the entry it happened to.
   *
   * **This used to be drawn inside the entry, and that made it unreadable in the one case it was
   * written for.** A stale action refreshes, the refresh removes the entry -- the record it
   * described is already gone -- and the message went with it. Observed on 2026-09-10: the reader
   * pressed a button, the row vanished, and nothing said whether anything had happened. A real
   * failure kept its entry and so kept its message, which is why only the stale case was affected,
   * and why the rendered test could not see it: its fixture kept the entry too.
   *
   * So it lives beside the list rather than inside it, and the panel stays on screen for it even
   * when nothing is left outstanding. The next action replaces it; only the reader clears it.
   */
  let outcome = $state<{ container: string; message: string; failed: boolean } | null>(null);
  let working = $state<string | null>(null);

  const empty = $derived(
    !report || (report.dictionaries.length === 0 && report.installs.length === 0),
  );
  /// The panel is drawn while there is anything outstanding **or** anything to say about what was
  /// just done. Hiding it the moment the last entry resolved is what swallowed the explanation.
  const showing = $derived(!empty || outcome !== null);

  /// Run one action, and answer the two ways it can go wrong differently.
  ///
  /// A **stale** rejection is not a failure to report: the panel was describing a moment that has
  /// passed, so it refreshes and says so plainly. Anything else keeps its own message, because it
  /// is something that actually went wrong and the reader can act on the reason.
  async function act(
    entry: DictionaryRecovery,
    run: (container: string, transaction: string, published: string) => Promise<void>,
    succeeded: string,
  ) {
    if (!entry.transaction || !entry.published) return;
    working = entry.name;
    try {
      await run(entry.name, entry.transaction, entry.published);
      outcome = { container: entry.name, message: succeeded, failed: false };
    } catch (thrown) {
      if (isPersistenceError(thrown) && thrown.code === "stale_recovery") {
        outcome = {
          container: entry.name,
          message: "This was already resolved elsewhere. Showing what is true now.",
          failed: false,
        };
      } else {
        outcome = {
          container: entry.name,
          message: isPersistenceError(thrown) ? thrown.message : String(thrown),
          failed: true,
        };
      }
    } finally {
      working = null;
      await refresh();
    }
  }

  /// What each dictionary state means, said without generations or pointers.
  function explain(entry: DictionaryRecovery): string {
    switch (entry.state) {
      case "unresolved":
        return "An installation switched to a newer version and did not finish. The previous version is being kept until you decide.";
      case "conflicting":
        return "An installation did not finish, and something newer has been installed since. Going back would discard that newer version.";
      case "cleanup_only":
        return "An installation did not finish, but nothing was changed. Only its record is left.";
      case "never_switched":
        return "An installation stopped before it changed anything. Only its record is left.";
      case "unreadable":
        return "This installation's record cannot be read, so what it did cannot be established. Nothing will be cleaned up here, and no new installation of this dictionary will start, until it is resolved by hand.";
    }
  }

  function installExplains(entry: InstallRecovery): string {
    switch (entry.state) {
      case "restored":
        // **Four answers, because there are four things that can be true.** Telling somebody to
        // restart when nothing established whether they need to is a guess wearing the clothes of
        // advice, so "unknown" says so instead.
        switch (entry.loading) {
          case "loaded":
            return "Restored, and in use now.";
          case "failed":
            return `Restored on disk, but loading it did not work${entry.detail ? `: ${entry.detail}` : "."}`;
          case "not_needed":
            return "Restored. It is read when it is next used, so nothing else is needed.";
          case "unknown":
            return "Restored on disk. Whether this session picked it up was not established.";
        }
      case "retained":
        return "An older copy was kept beside files that are already in place. Nothing was changed.";
      case "failed":
        return "This could not be put back. Its files are still where they were saved.";
      case "competing":
        return "More than one unfinished installation offers to restore this, so none was chosen.";
      case "unidentified":
        return "Something was left behind without saying where it belongs.";
    }
  }
</script>

{#if showing}
  <section class="recovery" aria-label="Unfinished installations">
    {#if !empty}
      <h3>Unfinished installations</h3>
    {/if}

    <!-- **Always in the DOM while the panel is**, empty or not: a live region a screen reader is
         to announce has to exist before it has something to say, and one created at the same
         moment as its text is one some readers never announce. Polite, because this reports what
         the reader themselves just did — it is not an interruption. -->
    <div class="said" role="status" aria-live="polite">
      {#if outcome}
        <p class="outcome" class:failed={outcome.failed}>{outcome.message}</p>
        <button class="control" onclick={() => (outcome = null)}>Dismiss</button>
      {/if}
    </div>

    {#each report?.dictionaries ?? [] as entry (entry.path)}
      <article class="entry" data-recovery={entry.name} data-state={entry.state}>
        <h4>{entry.name}</h4>
        <p class="what">{explain(entry)}</p>
        {#if entry.already_tried}
          <p class="tried">A repair has already been tried here and did not work.</p>
        {/if}

        {#if entry.state === "unreadable"}
          <!-- No action, deliberately: nothing can establish what this transaction did, so every
               button would be a guess. The path is here to be copied into a message instead. -->
          <p class="detail" data-diagnostic>{entry.path}{entry.detail ? ` — ${entry.detail}` : ""}</p>
        {:else}
          <div class="actions">
            <button
              class="control"
              disabled={working === entry.name}
              onclick={() => act(entry, keepCurrent, "Kept the current installation.")}
            >Keep current installation</button>
            {#if entry.state === "unresolved"}
              <button
                class="control"
                disabled={working === entry.name}
                onclick={() => act(entry, restorePrevious, "Restored the previous installation.")}
              >Restore previous installation</button>
            {/if}
          </div>
          <!-- **What keeping gives up, said before it is pressed.** The live selection is
               preserved, but clearing the record relinquishes the protection on the older version,
               which a later installation may then remove. Promising that nothing can be lost would
               be the easier sentence and the wrong one. -->
          <p class="consequence">
            Keeping leaves the current version in place. The previous version stops being protected
            and may be removed by a later installation.
          </p>
        {/if}

      </article>
    {/each}

    {#each report?.installs ?? [] as entry (entry.id + (entry.destination ?? ""))}
      <article class="entry" data-recovery={entry.id} data-state={entry.state}>
        <h4>{entry.id}</h4>
        <p class="what">{installExplains(entry)}</p>
        {#if entry.claimants.length > 1}
          <p class="detail" data-diagnostic>{entry.claimants.join("\n")}</p>
        {:else if entry.saved || entry.detail}
          <p class="detail" data-diagnostic>
            {entry.detail ?? ""}{entry.detail && entry.saved ? " — " : ""}{entry.saved ?? ""}
          </p>
        {/if}
      </article>
    {/each}
  </section>
{/if}

<style>
  .recovery {
    display: grid;
    gap: var(--gap);
    margin-bottom: var(--gap-lg);
  }
  h3 {
    margin: 0;
    font-size: 1rem;
  }
  .entry {
    display: grid;
    gap: var(--gap-sm);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    padding: var(--gap);
    background: var(--surface);
  }
  h4 {
    margin: 0;
    font-size: 0.95rem;
  }
  .what {
    margin: 0;
    color: var(--text-dim);
    font-size: 0.88rem;
  }
  .tried {
    margin: 0;
    color: var(--text-dim);
    font-size: 0.82rem;
  }
  /* Selectable, because the whole use of it is being pasted into a message to somebody. */
  .detail {
    margin: 0;
    padding: 0.3rem 0.45rem;
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    color: var(--text-faint);
    font-family: var(--font-mono);
    font-size: 0.78rem;
    white-space: pre-wrap;
    user-select: text;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--gap-sm);
  }
  .consequence {
    margin: 0;
    color: var(--text-faint);
    font-size: 0.8rem;
  }
  /* Empty until there is something to say, and it takes no room until then: an always-present
     live region must not leave an always-present gap. */
  .said:not(:empty) {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--gap);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    padding: var(--gap-sm) var(--gap);
    background: var(--surface);
  }
  .outcome {
    margin: 0;
    font-size: 0.85rem;
    color: var(--text-dim);
  }
  .outcome.failed {
    color: var(--red);
  }
</style>
