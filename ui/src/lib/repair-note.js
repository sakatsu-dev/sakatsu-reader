/**
 * The one line the word panel adds when **the selected word was rejoined after the engine split
 * it** — a contracted verb form core rebuilt from the dictionary (`lexical::repair`).
 *
 * Matched by exact span, so a part of the word, or a neighbour that happens to share a lemma, is
 * never attributed. It says where the word came from; it is not a warning, and the passage still
 * says it was read by Ichiran.
 */

/**
 * @typedef {{ start: number; end: number; family: string; lemma: string }} RepairLike
 * @typedef {{ repairs?: RepairLike[] | null } | null | undefined} ExecutionLike
 * @typedef {{ start: number | null; end: number | null } | null | undefined} TokenLike
 */

const FORMS = /** @type {Record<string, string>} */ ({
  te_oru: "～ておる",
  negative_conditional: "～なければ",
});

/**
 * @param {ExecutionLike} execution
 * @param {TokenLike} token
 * @returns {string | null}
 */
export function repairNoteFor(execution, token) {
  if (!token || token.start == null || token.end == null) return null;
  const note = execution?.repairs?.find(
    (repair) => repair.start === token.start && repair.end === token.end,
  );
  if (!note) return null;
  const form = FORMS[note.family];
  return form
    ? `Contracted ${form} form of ${note.lemma}, rejoined using the dictionary.`
    : `A form of ${note.lemma}, rejoined using the dictionary.`;
}
