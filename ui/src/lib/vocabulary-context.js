/** @typedef {import("../model").VocabularyRanking} VocabularyRanking */

/**
 * Decide how much of Analysis Vocabulary leads before its counted disclosures.
 *
 * Rust owns ranking and reorders the canonical WordView losslessly. This helper owns only
 * presentation: one evidenced winner, or two neutral source-order senses when the scorer
 * explicitly abstains. Reference never supplies a ranking and therefore never enters either
 * contextual state.
 *
 * @param {number} senseCount
 * @param {VocabularyRanking | null | undefined} ranking
 */
export function vocabularyPresentation(senseCount, ranking) {
  const count = Math.max(0, Math.trunc(senseCount));
  const favourite = ranking?.favoured;
  const chosen = favourite === 0 ? ranking?.senses[0] : undefined;
  const validFavourite =
    favourite === 0 &&
    count > 0 &&
    chosen !== undefined &&
    chosen.score > 0;

  if (validFavourite) {
    return {
      leadCount: Math.min(1, count),
      favourite,
      evidence: chosen?.reasons ?? [],
      ambiguous: false,
    };
  }

  const ambiguous = Boolean(ranking && count > 1);
  return {
    leadCount: Math.min(ambiguous ? 2 : 1, count),
    favourite: null,
    evidence: [],
    ambiguous,
  };
}
