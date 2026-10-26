// Pure glossary search primitives. Keeping matching and segmentation outside Svelte makes the
// same normalisation rule testable and prevents a highlighted label from disagreeing with the
// filter that made it visible.

/** @param {string} value */
export function normaliseGlossaryText(value) {
  return value.normalize("NFKC").toLocaleLowerCase();
}

/**
 * Whether a term is **named** by the query. Its summary and detail are deliberately not read.
 *
 * The glossary is a list of names, and full-text matching made it behave like one of prose.
 * "Noun" returned thirteen of seventy-three terms, because nearly every definition of a
 * grammatical concept mentions a noun somewhere — Predicate, Copula, Nominaliser and Adnominal
 * all came back ahead of the term actually called Noun, each of them a card the reader has to
 * rule out by hand. A filter over a glossary is for reaching the entry you can already name.
 *
 * `id` and `aliases` stay: they *are* names. 連用形 and te-form are what the Conjunctive form is
 * called elsewhere, and a reader who knows only that name has no other way in. `aliasMatch`
 * below exists to disclose which of them answered.
 *
 * @param {{ label: string, id: string, aliases: string[] }} term @param {string} query
 */
export function glossaryTermMatches(term, query) {
  const needle = normaliseGlossaryText(query.trim());
  if (!needle) return true;
  return [term.label, term.id, ...term.aliases]
    .some((value) => normaliseGlossaryText(value).includes(needle));
}

/** @param {{ label: string, aliases: string[] }} term @param {string} query */
export function glossaryAliasMatch(term, query) {
  const needle = normaliseGlossaryText(query.trim());
  if (!needle || normaliseGlossaryText(term.label).includes(needle)) return null;
  return term.aliases.find((alias) => normaliseGlossaryText(alias).includes(needle)) ?? null;
}

/**
 * Return safe text segments instead of markup. Offsets are mapped back from NFKC text one source
 * code point at a time, so a compatibility character may expand without splitting its source.
 * @param {string} text
 * @param {string} query
 * @returns {{ text: string, hit: boolean }[]}
 */
export function glossaryHighlightSegments(text, query) {
  const needle = normaliseGlossaryText(query.trim());
  if (!needle) return [{ text, hit: false }];

  let normal = "";
  /** @type {{ start: number, end: number }[]} */
  const sourceAt = [];
  let sourceOffset = 0;
  for (const sourceCharacter of text) {
    const start = sourceOffset;
    sourceOffset += sourceCharacter.length;
    const folded = normaliseGlossaryText(sourceCharacter);
    normal += folded;
    for (let index = 0; index < folded.length; index += 1) {
      sourceAt.push({ start, end: sourceOffset });
    }
  }

  /** @type {{ start: number, end: number }[]} */
  const hits = [];
  let from = 0;
  while (from <= normal.length - needle.length) {
    const at = normal.indexOf(needle, from);
    if (at < 0) break;
    const first = sourceAt[at];
    const last = sourceAt[at + needle.length - 1];
    if (first && last) hits.push({ start: first.start, end: last.end });
    from = at + needle.length;
  }
  if (!hits.length) return [{ text, hit: false }];

  const merged = hits.reduce((ranges, hit) => {
    const last = ranges[ranges.length - 1];
    if (last && hit.start <= last.end) last.end = Math.max(last.end, hit.end);
    else ranges.push({ ...hit });
    return ranges;
  }, /** @type {{ start: number, end: number }[]} */ ([]));

  /** @type {{ text: string, hit: boolean }[]} */
  const pieces = [];
  let offset = 0;
  for (const hit of merged) {
    if (hit.start > offset) pieces.push({ text: text.slice(offset, hit.start), hit: false });
    pieces.push({ text: text.slice(hit.start, hit.end), hit: true });
    offset = hit.end;
  }
  if (offset < text.length) pieces.push({ text: text.slice(offset), hit: false });
  return pieces;
}
