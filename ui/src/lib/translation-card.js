/**
 * Pure presentation decisions for the Analysis translation card.
 *
 * Rust owns sentence boundaries and translation. This file only projects that canonical ordered
 * batch into the card, including the one sentence a focused comparison may transform in place.
 */

/** @typedef {import("../model").SentenceTranslation} SentenceTranslation */
/** @typedef {import("../model").VariantTranslations} VariantTranslations */

/** Whether translating the complete passage can add context beyond one sentence. */
/** @param {ReadonlyArray<SentenceTranslation>} sentences */
export function contextCheckEligible(sentences) {
  return sentences.length > 1;
}

/**
 * Return one presentation record for every canonical sentence, in source order.
 *
 * A comparison is accepted only for the range containing the selected token and only when its
 * exact Japanese baseline agrees with that range. A stale or mismatched result therefore cannot
 * replace an unrelated sentence even if a caller's request-generation guard regresses.
 *
 * @param {ReadonlyArray<SentenceTranslation>} sentences
 * @param {{
 *   comparing?: boolean,
 *   activeToken?: number | null,
 *   comparison?: VariantTranslations | null,
 *   comparisonBusy?: boolean,
 *   comparisonError?: string,
 * }} state
 */
export function alignedSentenceFlow(
  sentences,
  {
    comparing = false,
    activeToken = null,
    comparison = null,
    comparisonBusy = false,
    comparisonError = "",
  } = {},
) {
  return sentences.map((sentence) => {
    const active =
      comparing &&
      activeToken !== null &&
      activeToken >= sentence.first_token &&
      activeToken < sentence.last_token_end;
    const matchingComparison =
      active && comparison?.japanese.full === sentence.japanese ? comparison : null;
    return {
      sentence,
      active,
      comparison: matchingComparison,
      comparisonBusy: active && comparisonBusy,
      comparisonError: active ? comparisonError : "",
    };
  });
}

/** What follows the struck baseline, if anything. */
/**
 * @param {VariantTranslations | null | undefined} result
 * @returns {{ kind: "all-struck" } | { kind: "result", text: string } | { kind: "baseline-only" }}
 */
export function comparisonFollowup(result) {
  if (!result?.without) return { kind: "all-struck" };
  if (result.result_says_more) return { kind: "result", text: result.without };
  return { kind: "baseline-only" };
}
