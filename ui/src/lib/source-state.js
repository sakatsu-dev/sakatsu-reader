/** @typedef {{ kind: "draft", previous: string | null } | { kind: "compact", canonical: string }} SourceState */

/**
 * @param {string | null} [previous]
 * @returns {SourceState}
 */
export function draftSource(previous = null) {
  return { kind: "draft", previous };
}

/**
 * @param {string} canonical
 * @returns {SourceState}
 */
export function compactSource(canonical) {
  return { kind: "compact", canonical };
}

/**
 * Escape is meaningful only for an edit of a successful source.
 * @param {SourceState} source
 * @returns {{ text: string, source: SourceState } | null}
 */
export function cancelSourceDraft(source) {
  if (source.kind !== "draft" || source.previous === null) return null;
  return { text: source.previous, source: compactSource(source.previous) };
}
