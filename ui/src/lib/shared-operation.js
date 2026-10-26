/**
 * One operation at a time, shared by everyone who asks for it.
 *
 * **Why this is not just a busy flag.** The dictionary-index rebuild is reachable from two places —
 * the Reference pane's prompt and the Settings panel — and it reports by a `gloss-index-built`
 * event that carries no request id. Each caller registered its own listener, so two overlapping
 * requests meant two listeners for one event: whichever event arrived first settled both, and a
 * refusal meant for the second request was delivered to the first as its answer.
 *
 * A busy flag would not fix that. The second caller still needs *an answer*, and the right answer
 * is the one the operation already running will produce. So the promise is shared: the first call
 * starts the work, every call until it settles gets the same promise, and the slot is cleared
 * afterwards so the next request starts a fresh one.
 *
 * The backend agrees with this shape. The rebuild claims the dictionaries, so a genuinely
 * concurrent one is refused anyway — joining is what both callers wanted.
 *
 * @template T
 */
export function sharedOperation() {
  /** @type {Promise<T> | null} */
  let inFlight = null;
  return {
    /**
     * Run `start`, or join the run already going.
     *
     * @param {() => Promise<T>} start
     * @returns {Promise<T>}
     */
    run(start) {
      if (!inFlight) {
        // Cleared in a `finally` rather than on success: a failed operation has finished too, and
        // leaving the slot occupied would make every later attempt return the same old rejection.
        inFlight = start().finally(() => {
          inFlight = null;
        });
      }
      return inFlight;
    },
    /** Whether an operation is running right now. */
    get busy() {
      return inFlight !== null;
    },
  };
}
