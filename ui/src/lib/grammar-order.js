/** @typedef {import("../model").Auxiliary} Auxiliary */
/** @typedef {import("../model").Breakdown} Breakdown */
/** @typedef {import("../model").Match} Match */
/** @typedef {import("../model").RankedSense} RankedSense */
/** @typedef {import("../model").Reading} Reading */
/** @typedef {import("../model").Step} Step */

/**
 * @typedef {{ kind: "auxiliary", auxiliary: Auxiliary } | { kind: "match", match: Match }} PatternItem
 */

/**
 * @typedef {{ sense: RankedSense, role: "favoured" | "supported" }} PrioritizedReading
 */

/**
 * @typedef {
 *   | { kind: "prioritized-readings", readings: PrioritizedReading[] }
 *   | { kind: "steps", steps: Step[], start: number }
 *   | { kind: "patterns", items: PatternItem[] }
 *   | { kind: "other-readings", senses: RankedSense[], ambiguous: boolean }
 * } GrammarBlock
 */

/**
 * The learner-facing names of morphological steps in the detailed grammar explanation.
 * @type {Partial<Record<import("../model").FormId, string>>}
 */
export const FORM_TITLES = {
  "causative-passive": "Causative-passive",
  causative: "Causative",
  passive: "Passive",
  potential: "Potential",
  desiderative: "Desiderative (〜たい)",
  volitional: "Volitional",
  imperative: "Imperative",
  provisional: "Provisional (〜ば)",
  conditional: "Conditional (〜たら)",
  alternative: "Alternative (〜たり)",
  conjunctive: "Te-form",
  continuative: "Continuative stem",
  adverbial: "Adverbial",
  past: "Past",
  "non-past": "Non-past",
  "adjective-stem": "Adjective stem",
};

/**
 * Put the selected expression's explanations in the order a learner can use them.
 *
 * This is a dependency order, not a school syllabus or a difficulty score. When one particle
 * reading is uniquely strongest, every reading with positive contextual evidence answers the
 * immediate question first: the winner is favoured and the rest are visibly secondary. The
 * word is then built component by component: the preceding component's forms, the auxiliary it
 * creates with the next component, and only then that auxiliary component's own inflection.
 * Sentence patterns follow the completed word, and unsupported or unresolved readings stay
 * visible at the end.
 *
 * JLPT level is deliberately absent from the inputs. It is useful reference metadata, but the
 * official test publishes no grammar-item curriculum and a badge cannot prove that one item is
 * a prerequisite of another.
 *
 * @param {Breakdown | null} breakdown
 * @param {readonly Match[]} matches
 * @param {Reading | null} reading
 * @returns {GrammarBlock[]}
 */
export function orderGrammarBlocks(breakdown, matches, reading) {
  /** @type {GrammarBlock[]} */
  const blocks = [];

  const favouredIndex =
    reading &&
    reading.favoured !== null &&
    reading.favoured >= 0 &&
    reading.favoured < reading.senses.length
      ? reading.favoured
      : null;
  const favoured = favouredIndex === null ? null : (reading?.senses[favouredIndex] ?? null);

  /** @type {PrioritizedReading[]} */
  const prioritizedReadings = [];
  // Positive scores are evidence *for* a sense, but a tie still promotes nothing. Rust owns
  // that distinction through `favoured`; repeating its score comparison here would let the
  // fixture and the interface disagree about whether the sentence really chose a leader.
  if (favoured && favoured.score > 0) {
    prioritizedReadings.push({ sense: favoured, role: "favoured" });
    for (const sense of reading?.senses ?? []) {
      if (sense !== favoured && sense.score > 0) {
        prioritizedReadings.push({ sense, role: "supported" });
      }
    }
    blocks.push({ kind: "prioritized-readings", readings: prioritizedReadings });
  }

  const steps = breakdown?.steps ?? [];
  const auxiliaries = breakdown?.auxiliaries ?? [];
  let nextStepNumber = 1;

  /** @param {Step[]} run */
  const addSteps = (run) => {
    if (run.length === 0) return;
    const previous = blocks[blocks.length - 1];
    if (previous?.kind === "steps") {
      previous.steps.push(...run);
    } else {
      blocks.push({ kind: "steps", steps: [...run], start: nextStepNumber });
    }
    nextStepNumber += run.length;
  };

  /** @param {PatternItem[]} items */
  const addPatterns = (items) => {
    if (items.length === 0) return;
    const previous = blocks[blocks.length - 1];
    if (previous?.kind === "patterns") {
      previous.items.push(...items);
    } else {
      blocks.push({ kind: "patterns", items: [...items] });
    }
  };

  // A non-compound step has no component. Keeping these first preserves the established
  // deepest-first ladder, while the numbered blocks below continue from it if malformed or
  // future data happens to contain both shapes.
  addSteps(steps.filter((step) => step.component === null));

  const components = [
    ...new Set([
      ...steps.flatMap((step) => (step.component === null ? [] : [step.component])),
      ...auxiliaries.map((auxiliary) => auxiliary.component),
    ]),
  ].sort((left, right) => left - right);

  for (const component of components) {
    // The auxiliary exists before its own component is inflected. 食べて + いる makes
    // 〜ている, and only then does いる become いた; putting Past first reverses the word.
    addPatterns(
      auxiliaries
        .filter((auxiliary) => auxiliary.component === component)
        .map((auxiliary) => ({ kind: "auxiliary", auxiliary })),
    );
    addSteps(steps.filter((step) => step.component === component));
  }

  addPatterns(matches.map((match) => ({ kind: "match", match })));

  const promoted = new Set(prioritizedReadings.map(({ sense }) => sense));
  const otherSenses = reading?.senses.filter((sense) => !promoted.has(sense)) ?? [];
  if (otherSenses.length > 0) {
    blocks.push({
      kind: "other-readings",
      senses: otherSenses,
      ambiguous: prioritizedReadings.length === 0,
    });
  }

  return blocks;
}
