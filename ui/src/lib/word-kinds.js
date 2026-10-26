// @ts-check
/**
 * What kind of word a sense is — the one answer three surfaces have to agree on.
 *
 * **Here rather than in either caller.** `reference-results.js` owns Reference's composition and
 * `WordBody.svelte` is shared with Analysis, so neither can hold this without the other importing
 * across a boundary it has no business crossing. `traps.md` records the rule: a list two
 * components must agree on cannot live in either of them.
 *
 * Three things read it now, and they must not drift: the filter row's chips, the kind badge on a
 * card, and the colour a sense group is drawn in. A tag that means "verb" to one and nothing to
 * another is a page where filtering to Verb hides a sense drawn as one.
 */

/** @typedef {import("../model").WordHit} WordHit */

/**
 * @type {Array<[string, Set<string>]>}
 *
 * **Singular.** These name what one word *is* — the badge on a card reads NOUN · ADVERB — and a
 * filter button is read the same way, as the kind being asked for rather than a tally of them.
 * Plurals also made the counts row read as a partition of the results, which it is not: a word
 * is counted under every kind it belongs to.
 */
const WORD_KINDS = [
  ["Expression", new Set(["exp", "expression", "idiom", "proverb", "yoji"])],
  ["Noun", new Set(["noun", "n", "numeric", "num", "counter", "ctr", "pronoun", "pn"])],
  ["Adjective", new Set(["adjective", "adjectival", "na-adj", "no-adj", "ku-adj", "shiku", "taru", "aux-adj", "adj-i", "adj-na", "adj-no", "adj-t", "adj-f", "adj-ku", "adj-shiku", "adj-pn"])],
  ["Adverb", new Set(["adverb", "to-adverb", "adv", "adv-to"])],
  ["Verb", new Set(["5-dan", "1-dan", "2-dan", "suru", "su-verb", "zuru", "aux-verb", "transitive", "intransitive", "v1", "v5", "v5u", "v5k", "v5g", "v5s", "v5t", "v5n", "v5b", "v5m", "v5r", "vs", "vt", "vi", "vk", "vz", "vn", "vr", "aux-v"])],
  // **The words that do a job rather than name a thing**, and the reason this kind exists at all:
  // without it が had no classification the app recognised, so a particle — a first-class result on
  // the reference page, with a detail of its own — was the one word type drawn in no colour. 何 lost
  // its interjection and prefix senses the same way, in the middle of a list whose other senses
  // were coloured.
  //
  // Six tags under one name, because they share a hue and not a label: the chip still says
  // `interjection` or `prefix`, so what the colour groups the words still separate. Splitting them
  // into their own kinds would have meant nine hues on a card that already carries four for word
  // status — and a filter row nobody asked for a prefix button on.
  ["Function", new Set(["particle", "prt", "conjunction", "conj", "interjection", "int", "prefix", "pref", "suffix", "suf", "auxiliary", "aux"])],
];

/**
 * Every kind label, in the order the filter row shows them.
 *
 * Exported so the row and the matching cannot drift: they were two literals naming the same six
 * things, and a rename had to be made twice or the filter would offer a category nothing could
 * ever be in. `Other` is last because it is `wordKinds`' fallback rather than a tag set.
 */
export const WORD_KIND_LABELS = [...WORD_KINDS.map(([label]) => label), "Other"];

/**
 * The kinds one sense belongs to, in `WORD_KINDS` order. Empty when it carries no recognised tag.
 *
 * **Part of speech in Japanese is a property of a sense, not of a word.** 道中 is not "a noun that
 * is also an adverb"; it has a noun sense meaning *journey* and an adverb sense meaning *on the
 * way*. Everything below is built on this, so the filter can say which sense earned a result and
 * not merely that one did.
 *
 * @param {{ tags?: string[] }} sense
 */
export function senseKinds(sense) {
  const tags = new Set(sense.tags ?? []);
  return WORD_KINDS.filter(([, accepted]) => [...tags].some((tag) => accepted.has(tag))).map(([label]) => label);
}

/**
 * Every kind any of a word's senses belongs to, in `WORD_KINDS` order.
 *
 * The union of [`senseKinds`] rather than its own pass over a flattened tag set, so the two can
 * never disagree about what a tag means. `Other` is the fallback for a word no sense of which is
 * classified — roughly a third of the dictionary.
 *
 * @param {WordHit} word
 */
export function wordKinds(word) {
  const found = new Set(word.inline.flatMap((sense) => senseKinds(sense)));
  const kinds = WORD_KINDS.map(([label]) => label).filter((label) => found.has(label));
  return kinds.length ? kinds : ["Other"];
}

/**
 * The colour a kind is drawn in, as a CSS custom property.
 *
 * **A function rather than five class names**, because the callers are a sense-group heading and a
 * filter chip and neither should have to carry a rule per kind. The tokens come from `theme.js`,
 * which derives them in OKLCH per mode, so one stop and five hues read as one family.
 *
 * `Other` is `wordKinds`' fallback for a word no sense of which is classified — roughly a third of
 * the dictionary — and it has no hue on purpose: it is the absence of a classification, and giving
 * it a colour would make "we do not know" look like a sixth answer.
 *
 * @param {string} label a member of `WORD_KIND_LABELS`
 * @returns {string} a `var(...)` expression, always safe to interpolate into a style attribute
 */
export function kindColour(label) {
  const known = WORD_KINDS.some(([name]) => name === label);
  return known ? `var(--kind-${label.toLowerCase()})` : "var(--text-faint)";
}

/**
 * The kind a group of senses is drawn as, or `null` for one carrying no recognised tag.
 *
 * A sense can belong to several kinds — the first is taken, in `WORD_KINDS` order, because a
 * group wears one rule and one label. That order is the filter row's order, so the kind shown is
 * the one a reader scanning the row would look for first.
 *
 * @param {{ tags?: string[] }} sense
 */
export function leadingKind(sense) {
  return senseKinds(sense)[0] ?? null;
}
