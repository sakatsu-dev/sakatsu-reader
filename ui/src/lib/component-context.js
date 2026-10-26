/**
 * Which part of a word a path names, and the token sequence a lookup for it is asked against.
 *
 * # One resolver for everything a selection does
 *
 * The heading, the status control, audio and the contextual lookup must all be about the same
 * token. A path is resolved **once**, by [`resolvePath`], and every one of them uses what it
 * accepted. A step naming a part that is missing or not selectable ends the path there: a stale
 * path, or one naming a grammatical form the text does not spell, lands on the deepest word it
 * still names rather than on something the reader cannot see. [`expandForContext`] resolves with
 * the same function and returns the path it accepted, so the lookup cannot answer a different
 * token from the one the panel describes.
 *
 * # Why expansion, not "swap the part in for the whole"
 *
 * That was the first version and it quietly deleted the rest of the word: selecting `かっこ` in
 * `かっこいい曲` sent `かっこ | 曲`. Contextual ranking is about the words around a word, so a
 * context that drops one is not the context. The word is expanded in place instead, and
 * everything around it is untouched.
 *
 * # Pieces of the source, not the grammatical parts
 *
 * A word's `components` are its grammatical decomposition, and the source does not always spell
 * it: a contracted 食べてる decomposes as 食べて + いる, and いる is not in the text. So a word
 * expands into its **selectable** parts, and every stretch of its own text that no selectable part
 * covers is kept as a plain filler cut from the source. 食べてる becomes `食べて | る`.
 *
 * # The sentence structure travels too
 *
 * Contextual lookup reads sentence bounds, and those come from each token's `line_break` and from
 * sentence-final punctuation in a surface (core's `model::ends_sentence`). Equal text is not an
 * equal context if a boundary moved. So the **last** piece carries the word's own `line_break` and
 * every other piece carries none; and a word whose expansion would put sentence-final punctuation
 * on a non-final piece is not expanded at all — the path stops at the word. Pieces are copies: the
 * analysis itself is never changed.
 */

/**
 * @typedef {{ surface?: string; dict_form?: string; reading?: string; start?: number | null;
 *   end?: number | null; selectable?: boolean; line_break?: string;
 *   components?: TokenLike[] }} TokenLike
 */

/** Sentence-final punctuation, exactly as core's `model::ends_sentence` lists it. */
const SENTENCE_FINAL = /[。！？!?]/;

/**
 * Whether a token ends a sentence, by core's rule.
 *
 * @param {TokenLike} token
 */
export function endsSentence(token) {
  return token.line_break === "break" || SENTENCE_FINAL.test(token.surface ?? "");
}

/**
 * Follow `path` from `word` through **selectable** parts only.
 *
 * Returns the steps it accepted and the token they reach. A step naming a part that does not
 * exist or is not selectable ends the walk; the steps before it stand.
 *
 * @template {TokenLike} T
 * @param {T | null | undefined} word
 * @param {number[]} path
 * @returns {{ path: number[]; token: T | null }}
 */
export function resolvePath(word, path) {
  /** @type {number[]} */
  const accepted = [];
  let at = word ?? null;
  for (const step of path) {
    const next = /** @type {T | undefined} */ (at?.components?.[step]);
    if (!next || !next.selectable) break;
    accepted.push(step);
    at = next;
  }
  return { path: accepted, token: at };
}

/**
 * Expand `tokens[index]` along `path`, keeping every character and every sentence boundary.
 *
 * `text` is the passage the spans index into, **as UTF-8 byte offsets** — what the analyzer
 * reports, not what a JavaScript string indexes. It is needed only to cut fillers.
 *
 * Returns the sequence to ask against, where the chosen token sits in it, and **the path actually
 * accepted** — which is shorter than the one asked for when a step was withheld, missing, or could
 * not be expanded without moving a sentence boundary. The caller must act on that path.
 *
 * @template {TokenLike} T
 * @param {T[]} tokens
 * @param {number} index
 * @param {number[]} path
 * @param {string} text
 * @returns {{ tokens: T[]; index: number; path: number[] }}
 */
export function expandForContext(tokens, index, path, text) {
  if (index < 0 || index >= tokens.length) return { tokens, index, path: [] };
  const bytes = new TextEncoder().encode(text ?? "");
  const decoder = new TextDecoder();
  const { path: resolved } = resolvePath(tokens[index], path);

  let list = tokens;
  let at = index;
  /** @type {number[]} */
  const accepted = [];
  for (const step of resolved) {
    const word = list[at];
    const chosen = word?.components?.[step];
    if (!chosen) break;
    const pieces = piecesOf(word, bytes, decoder);
    if (!pieces) break;
    const position = pieces.findIndex((piece) => piece.original === chosen);
    if (position < 0) break;
    list = [
      ...list.slice(0, at),
      .../** @type {T[]} */ (pieces.map((piece) => piece.token)),
      ...list.slice(at + 1),
    ];
    at += position;
    accepted.push(step);
  }
  return { tokens: list, index: at, path: accepted };
}

/**
 * A word's selectable parts with its uncovered text between and around them, in source order,
 * each a copy carrying the boundary it should. `null` when the word cannot be expanded honestly:
 * no usable span to cut fillers from, or sentence-final punctuation on a piece that is not last.
 *
 * @param {TokenLike} word
 * @param {Uint8Array} bytes
 * @param {TextDecoder} decoder
 * @returns {{ token: TokenLike; original: TokenLike | null }[] | null}
 */
function piecesOf(word, bytes, decoder) {
  const { start, end } = word;
  if (typeof start !== "number" || typeof end !== "number" || end > bytes.length) return null;
  /** @type {{ token: TokenLike; original: TokenLike | null }[]} */
  const pieces = [];
  let cursor = start;
  const fill = (/** @type {number} */ to) => {
    if (to <= cursor) return;
    const surface = decoder.decode(bytes.subarray(cursor, to));
    pieces.push({
      token: { surface, dict_form: surface, reading: "", start: cursor, end: to, components: [], selectable: false },
      original: null,
    });
  };
  for (const part of word.components ?? []) {
    if (!part.selectable || typeof part.start !== "number" || typeof part.end !== "number") continue;
    fill(part.start);
    pieces.push({ token: { ...part }, original: part });
    cursor = part.end;
  }
  fill(end);
  if (pieces.length === 0) return null;
  // The word's own boundary goes on its last piece; no other piece may end a sentence.
  pieces.forEach((piece, at) => {
    piece.token.line_break = at === pieces.length - 1 ? (word.line_break ?? "none") : "none";
  });
  if (pieces.slice(0, -1).some((piece) => endsSentence(piece.token))) return null;
  return pieces;
}
