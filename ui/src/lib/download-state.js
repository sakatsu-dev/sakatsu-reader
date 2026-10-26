/**
 * How a saved partial download is described in Settings.
 *
 * Its own module because `Settings.svelte` uses runes and `node --test` cannot import it. The
 * formatter is passed in for the same reason: `humanBytes` lives in a runes module too.
 */

/**
 * What a paused download has kept, or `null` when it has kept nothing.
 *
 * An unknown total is left out rather than guessed: "40 MB saved", not "40 MB of 0 B".
 *
 * @param {number} done Bytes on disk.
 * @param {number | null | undefined} total The whole download, when known.
 * @param {(bytes: number) => string} format
 */
export function savedPhrase(done, total, format) {
  if (!done || done <= 0) return null;
  return total && total > 0 ? `${format(done)} of ${format(total)} saved` : `${format(done)} saved`;
}

/**
 * Whether a component has saved bytes or a Windows job to resume or discard.
 *
 * @param {{ downloaded_bytes: number, installed: boolean, artifacts?: { transfer_job?: string | null }[] } | null | undefined} component
 */
export function hasSavedPartial(component) {
  // BITS retains its bytes outside the destination until completion. Its saved job is
  // resumable even when there is no partial file whose length the app can report.
  return Boolean(
    component &&
    !component.installed &&
    (component.downloaded_bytes > 0 ||
      component.artifacts?.some((artifact) => artifact.transfer_job?.trim())),
  );
}
