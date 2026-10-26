// @ts-check
/**
 * Pure composition for Reference's heterogeneous answer.
 *
 * Search commands remain independent so speculative vocabulary warmup can stay bounded. This
 * helper is the one place their foreground answers become a navigable sequence. It neither
 * selects nor fetches anything, which makes losslessness and ordering cheap to prove.
 */

/** @typedef {"all" | "words" | "grammar" | "kanji" | "terms"} ReferenceFacet */
/** @typedef {import("../model").WordHit} WordHit */
/** @typedef {import("../model").GrammarPattern} GrammarPattern */
/** @typedef {import("../model").ParticleView} ParticleView */
/** @typedef {import("../model").Term} Term */
/** @typedef {import("../model").KanjiView} KanjiView */
/** @typedef {{ key: string, facet: ReferenceFacet, type: "word" | "grammar" | "particle" | "kanji" | "term", value: any, direct: boolean }} ReferenceResult */

/** @type {readonly ReferenceFacet[]} */
export const FACETS = ["all", "words", "grammar", "kanji", "terms"];

const japanese = /[\u3040-\u30ff\u3400-\u9fff\uf900-\ufaff]/u;

/** @param {unknown} value */
export function normaliseReferenceText(value) {
  return String(value ?? "")
    .trim()
    .toLocaleLowerCase()
    .replace(/[〜~～・\s]/gu, "");
}

/** @param {{ query?: string, words?: WordHit[], patterns?: GrammarPattern[], particles?: ParticleView[], terms?: Term[], kanji?: KanjiView | null }} input */
export function composeReferenceResults({
  query = "",
  words = [],
  patterns = [],
  particles = [],
  terms = [],
  kanji = null,
} = {}) {
  /** @type {ReferenceResult[]} */
  const results = [];
  const needle = normaliseReferenceText(query);
  const directKanji = kanji && [...String(query).trim()].length === 1 ? kanji : null;
  if (directKanji) {
    results.push({ key: `kanji:${directKanji.character}`, facet: "kanji", type: "kanji", value: directKanji, direct: true });
  }

  /** @type {ReferenceResult[]} */
  const exactParticles = [];
  /** @type {ReferenceResult[]} */
  const otherParticles = [];
  particles.forEach((value, index) => {
    /** @type {ReferenceResult} */
    const result = {
      key: `particle:${value.surface}:${index}`,
      facet: "grammar",
      type: "particle",
      value,
      direct: normaliseReferenceText(value.surface) === needle,
    };
    (result.direct ? exactParticles : otherParticles).push(result);
  });
  results.push(...exactParticles, ...otherParticles);

  /** @type {ReferenceResult[]} */
  const grammar = patterns.map((value, index) => ({
    key: `grammar:${value.id}:${index}`,
    facet: "grammar",
    type: "grammar",
    value,
    direct:
      normaliseReferenceText(value.pattern) === needle ||
      normaliseReferenceText(value.title) === needle,
  }));
  const leading = grammar.slice(0, 4);
  const remaining = grammar.slice(4);
  results.push(...leading);

  words.forEach((value, index) => {
    results.push({ key: `word:${index}`, facet: "words", type: "word", value, direct: false });
  });
  results.push(...remaining);

  terms.forEach((value, index) => {
    const aliases = [value.id, value.label, ...(value.aliases ?? [])];
    results.push({
      key: `term:${value.id}:${index}`,
      facet: "terms",
      type: "term",
      value,
      direct: aliases.some((alias) => normaliseReferenceText(alias) === needle),
    });
  });
  return results;
}

/** @param {ReferenceResult[]} results @param {ReferenceFacet} facet */
export function facetResults(results, facet) {
  return facet === "all" ? results : results.filter((result) => result.facet === facet);
}

/** @param {ReferenceResult[]} results */
export function facetCounts(results) {
  return {
    all: results.length,
    words: results.filter((result) => result.facet === "words").length,
    grammar: results.filter((result) => result.facet === "grammar").length,
    kanji: results.filter((result) => result.facet === "kanji").length,
    terms: results.filter((result) => result.facet === "terms").length,
  };
}

/** A sole answer is safe to open. Otherwise only one uniquely direct non-word answer is. */
/** @param {ReferenceResult[]} results */
export function automaticSelection(results) {
  if (results.length === 1) return results[0].key;
  const direct = results.filter((result) => result.direct && result.type !== "word");
  return direct.length === 1 ? direct[0].key : null;
}

/**
 * The kind vocabulary, re-exported.
 *
 * It moved to `word-kinds.js` when the sense colours arrived: `WordBody` is shared with Analysis
 * and draws them, and it cannot import Reference's own composition module to find out what a tag
 * means. Re-exported rather than relocated in every caller, because this module is where
 * Reference's own code and its tests already look for them.
 */
export { WORD_KIND_LABELS, senseKinds, wordKinds } from "./word-kinds.js";
import { senseKinds, wordKinds } from "./word-kinds.js";

/**
 * The senses worth showing for a word, given the kinds being filtered by.
 *
 * **The half that was missing.** All-sense membership decided *whether* a result appeared and
 * nothing downstream knew *which* sense earned it, so filtering to Adverb listed 道中 correctly
 * and then described it with its noun glosses — the filter finding the right word and saying the
 * wrong thing about it.
 *
 * Falls back to every sense when none match, which is the `Other` case: such a word is in the
 * list precisely because no sense of it is classified, so every `senseKinds` here is empty and
 * returning nothing would leave the card with no meaning at all.
 *
 * @param {WordHit} word
 * @param {readonly string[]} selected Kinds currently filtered by; empty means no filter.
 */
export function sensesForKinds(word, selected = []) {
  if (selected.length === 0) return word.inline;
  const matching = word.inline.filter((sense) => senseKinds(sense).some((kind) => selected.includes(kind)));
  return matching.length > 0 ? matching : word.inline;
}

/** @param {WordHit} word */
export function canonicalWordKind(word) {
  return wordKinds(word)[0];
}

/** @param {string} query */
export function referenceQueryHasContext(query) {
  const runs = String(query).trim().split(/\s+/u).filter(Boolean);
  return runs.length > 1 && runs.some((run) => japanese.test(run));
}
