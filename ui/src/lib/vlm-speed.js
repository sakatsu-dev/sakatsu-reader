/**
 * What a measured read means for the person who just installed PaddleOCR-VL.
 *
 * The model reads more accurately than anything else measured here — 0.0297 mean character
 * error against 0.0975 for Auto over the 141-capture corpus — and that number is the same on
 * every machine. What is not the same is the wait: identical weights measured 0.1 s a crop on
 * an RTX 4070 Ti SUPER and 49 s on a processor, a spread of about 500x. Recommending it
 * without knowing which of those a machine is would hand half of them an app that appears to
 * hang for a minute per capture.
 *
 * So the desktop setup path times a read, writes the seconds into its marker, and this decides
 * what to say about the answer.
 *
 * # Why this is its own module
 *
 * `VlmSetup.svelte` uses runes, so `node --test` cannot import it. The rules here are the part
 * worth pinning: the threshold that decides whether an offer appears at all, and a rounding
 * convention that has to stay readable across three orders of magnitude.
 */

/**
 * Seconds per capture below which PaddleOCR-VL is worth recommending as the everyday reader.
 *
 * Two rather than one. A capture is not a keystroke — it follows a drag, and the eye is still
 * moving — so a second of recognition is not felt the way a second of typing lag is. Under
 * two, the read has landed before attention returns to the text.
 *
 * The Rust setup path records only the duration. This is the one threshold that turns that
 * measurement into a recommendation, so setup and Settings cannot disagree about the machine.
 */
export const FAST_SECONDS = 2;

/**
 * @typedef {object} Speed
 * @property {boolean} timed Whether this machine has been measured at all.
 * @property {boolean} fast Whether it is quick enough to recommend. False when untimed.
 * @property {string} perCapture Human-readable duration, empty when untimed.
 */

/**
 * Describe a measured read.
 *
 * `null` means setup has not timed this machine — markers written before setup learned to do
 * so have no measurement, and neither does one whose timing was interrupted. **That is not the
 * same as slow**, and the two must not collapse: an unmeasured machine gets no claim made
 * about it, where a slow one gets an honest warning. Both withhold the recommendation, which
 * is why it is tempting to treat them alike.
 *
 * @param {number | null | undefined} seconds
 * @returns {Speed}
 */
export function readSpeed(seconds) {
  if (typeof seconds !== "number" || !Number.isFinite(seconds) || seconds <= 0) {
    return { timed: false, fast: false, perCapture: "" };
  }
  return {
    timed: true,
    fast: seconds <= FAST_SECONDS,
    perCapture: format(seconds),
  };
}

/**
 * A floor, then one decimal below ten seconds and none above.
 *
 * The range being formatted spans hundredths of a second to a minute or more, so no single
 * precision is right across it. Below ten the decimal is the whole distinction — 0.1 s and
 * 0.9 s are different experiences and both round to the same integer. Above it the decimal is
 * noise: nobody choosing against a 47-second wait is swayed by .3.
 *
 * **The floor is not cosmetic.** This machine's probe answered in 0.03 s, which one decimal
 * renders as "0.0s" — a measured, working, extremely fast install described to its owner as
 * taking no time at all, which reads as a bug rather than as good news.
 *
 * Formatting lives only here; the marker stores the raw rounded duration rather than a second
 * human-readable copy that could drift from this one.
 *
 * @param {number} seconds
 * @returns {string}
 */
function format(seconds) {
  if (seconds < 0.1) return "under 0.1s";
  return seconds < 10 ? `${seconds.toFixed(1)}s` : `${Math.round(seconds)}s`;
}
