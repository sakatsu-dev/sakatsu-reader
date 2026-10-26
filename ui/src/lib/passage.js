/**
 * Where the reader breaks a passage into lines.
 *
 * Out here rather than inline in `Reader.svelte` because it turns on an offset convention that is
 * wrong in a way nothing on screen would show — see below — and because "what the reader shows is
 * what you pasted" deserves a test rather than a look.
 */

/**
 * Group tokens into the lines the passage itself has.
 *
 * **Breaks only where the text breaks.** The reader used to start a new line after every `。！？`
 * so a long capture would not arrive as one unbroken wall, and that was too strong a rule:
 * `提出締め切り！つぎのステップへ` is one line of input and came back as two, each with the width
 * of the window empty beside it. Text that was never broken should not look broken.
 *
 * The wall it guarded against still is: a capture that has lines *has newlines* — OCR of a manga
 * panel or a game box returns them — and those are honoured. A single long paragraph now wraps
 * the way a paragraph should.
 *
 * ## The offsets are bytes, not characters
 *
 * `Token.start` and `Token.end` come from Rust, where a string index is a **UTF-8 byte offset**.
 * JavaScript indexes UTF-16 code units. For ASCII the two agree, which is exactly why this is
 * dangerous: it would pass any test written in English and mis-split every real passage, because
 * 提 is three bytes and one JS index. So the search happens over encoded bytes, and the newline is
 * looked for as `0x0A` rather than as `"\n"`.
 *
 * ## One paragraph unless asked otherwise
 *
 * `keepBreaks` false — the default the app now ships — returns every token on one flowing line.
 * A capture's breaks are the bubble's, not the sentence's, and reproducing them costs height on
 * a shape that came from the artwork. The strictness above is still exactly right for text
 * somebody typed or pasted, which is why it is a setting and not a deletion.
 *
 * @param {string | null} text the passage as analysed, or null when it is not known
 * @param {{start: number | null, end: number | null}[]} tokens the flat token array
 * @param {boolean} [keepBreaks] honour the passage's own newlines; false flows it as one line
 * @returns {number[][]} token indices, grouped into lines, in order
 */
export function passageLines(text, tokens, keepBreaks = true) {
  const all = tokens.map((_, index) => index);
  if (!keepBreaks) return tokens.length === 0 ? [] : [all];
  // Nothing to read the breaks from. One flowing line is the right fallback: the alternative is
  // guessing at them, which is the behaviour this replaced.
  if (text === null || text === undefined || tokens.length === 0) {
    return tokens.length === 0 ? [] : [all];
  }

  const bytes = new TextEncoder().encode(text);
  /** @type {number[][]} */
  const lines = [];
  /** @type {number[]} */
  let current = [];
  let read = 0;

  tokens.forEach((token, index) => {
    // The break lives in the gap *between* two tokens, which no token covers — so it is looked
    // for in the passage rather than in either of them.
    if (token.start !== null && token.start !== undefined && token.start >= read) {
      const gap = bytes.subarray(read, token.start);
      if (gap.includes(0x0a) && current.length > 0) {
        lines.push(current);
        current = [];
      }
    }
    current.push(index);
    if (token.end !== null && token.end !== undefined) read = token.end;
  });

  if (current.length > 0) lines.push(current);
  return lines;
}

/**
 * Closing marks a line may not begin with, and opening marks it may not end with — the core of
 * *kinsoku shori*, the line-breaking rule every Japanese typesetter applies.
 *
 * Only the punctuation tokens are tested against these. Small kana and ー can also never begin a
 * line, but they never arrive as tokens of their own: they are inside words, and a word is one box.
 */
const CLOSING = /^[、。，．・：；？！…‥」』）］｝〕〉》】〙〗〟’”｠»〜～‐゠–,.!?)\]}%:;]/;
const OPENING = /[「『（［｛〔〈《【〘〖〝‘“｟«([{]$/;

/**
 * Group one line's tokens into the units the reader may not break inside.
 *
 * **Why the browser does not do this itself.** Every word in the reader is a `<button>`, and a
 * button is an atomic inline: the line breaker treats it like an object, which a line may break
 * after, so the ordinary rule that 。 never begins a line never got a chance to apply. 座っていた。
 * at the end of a long passage sent the 。 down to a line of its own. Seen in the README
 * screenshots, 2026-10-19.
 *
 * So the reader groups them itself: a closing mark joins the word before it, an opening mark joins
 * the word after it, and each group is rendered as one unbreakable run. Everything else is a unit
 * of its own, exactly as before.
 *
 * @template T
 * @param {number[]} line token indices of one line, in order
 * @param {T[]} tokens the flat token array the indices point into
 * @param {(token: T) => boolean} isWord whether a token is a word rather than punctuation
 * @returns {number[][]} the line's indices, grouped; concatenated, they are `line` unchanged
 */
export function lineUnits(line, tokens, isWord) {
  /** @type {number[][]} */
  const units = [];
  /** @type {number[]} */
  let opening = [];
  for (const index of line) {
    const token = /** @type {T & { surface: string }} */ (tokens[index]);
    const mark = !isWord(token);
    if (mark && opening.length === 0 && units.length > 0 && CLOSING.test(token.surface)) {
      units[units.length - 1].push(index);
    } else if (mark && OPENING.test(token.surface)) {
      opening.push(index);
    } else {
      units.push([...opening, index]);
      opening = [];
    }
  }
  // An opening mark with nothing after it on this line still has to be shown.
  if (opening.length > 0) units.push(opening);
  return units;
}
