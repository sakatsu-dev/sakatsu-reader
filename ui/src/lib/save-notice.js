/**
 * Where a failed save's message goes: the short notice, or the full error.
 *
 * A session whose store could not be opened shows a standing warning saying so, in the sentence
 * Rust wrote. Every save then fails for that same reason, and before this each failure printed
 * the reason again — the degraded-store checkpoint on 2026-09-08 had three banners on screen
 * saying one thing, one of them nested three prefixes deep.
 *
 * **Silence was the wrong fix and was considered.** Suppressing action errors while the warning
 * is up would make a click look like it worked, which is the exact failure the warning exists to
 * pre-empt. So both are kept: the standing warning explains the session, and a short line
 * confirms that *this* action did not save.
 *
 * # Recognised by code, not by prose
 *
 * This first compared the rejection against the standing warning with `===`, which worked and
 * made behaviour depend on presentation text: rewording a sentence in Rust would have silently
 * changed what this drew. The persistence commands now reject with a `PersistenceError` — a
 * stable `code` the interface branches on, and a `message` it prints and never parses.
 *
 * `store_unavailable` is the one case the standing warning has already explained, so it is the one
 * case answered briefly. `settings_read_only` and `write_failed` are *not*: the first means the
 * store opened and declined this particular file, the second that a write went wrong, and neither
 * is on screen anywhere else. Answering those with "not saved" would hide the only account of
 * them — which is the same mistake as repeating the reason, arriving from the other side.
 *
 * A rejection that is not a `PersistenceError` at all — any other command, or a transport failure
 * — keeps its own words untouched.
 *
 * @param {unknown} thrown what the command rejected with
 * @param {string} short what to say when this failed for the standing reason
 * @returns {{ notice: string | null, error: string | null }} exactly one of the two is set
 */
export function saveFailure(thrown, short) {
  if (isPersistenceError(thrown) && thrown.code === "store_unavailable") {
    return { notice: short, error: null };
  }
  // `message` when there is one, so a typed rejection still reads as a sentence rather than as
  // `[object Object]` — which is what `String(thrown)` gives for the new shape and is the trap
  // this function has to absorb for every caller.
  return { notice: null, error: errorMessage(thrown) };
}

/** A transport or typed command rejection as readable text. @param {unknown} thrown */
export function errorMessage(thrown) {
  return isPersistenceError(thrown) ? thrown.message : String(thrown);
}

/**
 * Whether this rejection is the structured one.
 *
 * Checked by shape rather than by `instanceof`: it arrives as plain JSON across the IPC boundary,
 * so there is no class to be an instance of.
 *
 * @param {unknown} thrown
 * @returns {thrown is import("../model").PersistenceError}
 */
export function isPersistenceError(thrown) {
  return (
    typeof thrown === "object" &&
    thrown !== null &&
    typeof (/** @type {{ code?: unknown }} */ (thrown).code) === "string" &&
    typeof (/** @type {{ message?: unknown }} */ (thrown).message) === "string"
  );
}

/** What a failed word-status save says. Fixed text: the reason is on screen already. */
export const STATUS_NOT_SAVED = "Word status wasn't saved";

/** And a failed settings save. */
export const SETTINGS_NOT_SAVED = "Settings weren't saved";
