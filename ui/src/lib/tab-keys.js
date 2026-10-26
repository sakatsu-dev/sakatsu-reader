/**
 * Which tab command a keystroke means, if any.
 *
 * The Reference page keeps several searches open, and until now they were reachable only by
 * pointer. These are the chords a reader already knows from their browser, mapped onto the same
 * moves.
 *
 * # Why this is its own module
 *
 * `pages.svelte.ts` holds the tabs and uses runes, so `node --test` cannot import it. The state is
 * not where the mistakes are, though: the mistakes are in the modifier arithmetic — `Ctrl+9` being
 * *last* rather than ninth, `Ctrl+Shift+T` not also reading as `Ctrl+T`, AltGr not counting as
 * Ctrl. That part is pure, so it lives here where it can be tested.
 *
 * # Read from `code`, not `key`
 *
 * `event.key` is what the layout produces: on a French AZERTY the top row gives `&`, `é`, `"`
 * rather than `1`, `2`, `3`, and `event.key` for `Ctrl+1` is `&`. `event.code` is the physical
 * key — `Digit1`, `KeyT` — which is what a shortcut means. The one exception is Tab, which is
 * layout-independent and reads more plainly as `key`.
 */

/**
 * @typedef {object} Keystroke
 * @property {string} key
 * @property {string} [code]
 * @property {boolean} [ctrlKey]
 * @property {boolean} [metaKey]
 * @property {boolean} [shiftKey]
 * @property {boolean} [altKey]
 */

/**
 * @typedef {{ kind: "new" | "close" | "reopen" | "next" | "previous" | "jump", index?: number }}
 *   TabCommand
 */

/**
 * @param {Keystroke} event
 * @returns {TabCommand | null} the command, or `null` for every keystroke that is not one
 */
export function tabShortcut(event) {
  // **Alt disqualifies, and that is not fussiness.** On a great many European layouts AltGr
  // arrives as Ctrl+Alt, so `Ctrl+Alt+T` is somebody typing a character rather than asking for a
  // tab. Taking it would make the app eat their keyboard.
  if (event.altKey) return null;
  if (!event.ctrlKey && !event.metaKey) return null;

  if (event.key === "Tab") {
    return event.shiftKey ? { kind: "previous" } : { kind: "next" };
  }

  const digit = /^Digit([1-9])$/.exec(event.code ?? "");
  if (digit && !event.shiftKey) {
    const nth = Number(digit[1]);
    // **Nine is the last tab, not the ninth.** Every browser does this, and it is the more useful
    // of the two: eight is enough to count to by eye, and past that what you want is the end.
    return nth === 9 ? { kind: "jump", index: -1 } : { kind: "jump", index: nth - 1 };
  }

  switch (event.code) {
    case "KeyT":
      // Shift is the difference between a fresh tab and the one you just regretted closing.
      return event.shiftKey ? { kind: "reopen" } : { kind: "new" };
    case "KeyW":
      // `Ctrl+Shift+W` closes the whole window in a browser. There is one window here and it holds
      // the reading session, so it deliberately means nothing.
      return event.shiftKey ? null : { kind: "close" };
    default:
      return null;
  }
}
