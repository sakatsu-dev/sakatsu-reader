/**
 * What the capture hint says, in each of the states it can be in.
 *
 * The hint is the small line at the trailing end of the Analysis actions row. It used to have
 * two states behind one boolean — "Reading…" while a capture was in flight, the binding
 * otherwise — and that was enough while every engine loaded in under a second.
 *
 * PaddleOCR-VL ended that. Its first read starts the app-managed llama.cpp server and loads about
 * 1.8 GB of model data; every second of that wait used to show only the word "Reading…".
 * A reader watching that has no way to tell a model waking up from a capture going wrong, and
 * no way to tell which engine is doing either.
 *
 * # Why this is its own module
 *
 * `App.svelte` uses runes, so `node --test` cannot import it. The rules are small but there are
 * four of them plus an engine that has no name, and each one is a sentence someone reads.
 */

/**
 * @typedef {object} Hint
 * @property {"loading" | "reading" | "idle"} phase Which of the three shapes this is.
 * @property {boolean} spinner Whether the busy glyph belongs beside it.
 * @property {string} text The whole line, for the two busy phases and for idle without motion.
 * @property {string[]} faces The two lines to alternate. Empty unless the hint is cycling.
 */

/**
 * Decide what the hint shows.
 *
 * @param {object} state
 * @param {boolean} state.capturing Whether a read is in flight.
 * @param {boolean} state.ready Whether the engine is loaded. Only consulted while capturing.
 * @param {string} state.engine The engine's display name, already through `engineLabel`.
 * @param {string} state.binding How the capture binding reads, e.g. "Middle click".
 * @param {boolean} state.cycling Whether the interface is allowed to animate.
 * @param {string | null} [state.provenance] What the capture behind the passage on screen adds to
 *   the engine's name — which model Auto picked, verified GPU layers — from `runtime-status.js`.
 *   `null` when it adds nothing, or when the passage did not come from a capture.
 * @returns {Hint}
 */
export function captureHint({ capturing, ready, engine, binding, cycling, provenance = null }) {
  const named = Boolean(engine) && engine !== "no model installed";

  if (capturing && !ready) {
    // **No engine name means no engine name in the sentence.** "Loading no model installed…"
    // is what a template produces when nobody checks, and it is worse than saying less.
    return {
      phase: "loading",
      spinner: true,
      text: named ? `Loading ${engine}…` : "Loading…",
      faces: [],
    };
  }
  if (capturing) {
    return { phase: "reading", spinner: true, text: "Reading…", faces: [] };
  }

  const hint = `${binding} reads`;
  if (!named) {
    // Nothing to alternate with, so nothing alternates. A reader with no model installed has a
    // setup notice telling them so; the hint does not need to blink it at them as well.
    return { phase: "idle", spinner: false, text: hint, faces: [] };
  }

  // The engine's face carries the provenance, so it alternates with the binding like the name.
  const face = provenance ? `${engine}, ${provenance}` : engine;
  return {
    phase: "idle",
    spinner: false,
    // The still form joins what the cycle would have alternated, so turning animation off
    // loses the motion and not the information.
    text: cycling ? hint : `${hint} · ${face}`,
    faces: cycling ? [hint, face] : [],
  };
}
