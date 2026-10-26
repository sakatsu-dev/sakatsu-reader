import assert from "node:assert/strict";
import test from "node:test";
import { WORD_KIND_LABELS, kindColour, leadingKind } from "./word-kinds.js";
import {
  automaticSelection,
  canonicalWordKind,
  senseKinds,
  sensesForKinds,
  composeReferenceResults,
  facetCounts,
  facetResults,
  wordKinds,
} from "./reference-results.js";

const word = (headword, senses = [{ tags: ["n"], glosses: [headword] }]) => ({ headword, inline: senses });
const pattern = (id) => ({ id, pattern: `〜${id}`, title: id });

test("composition is lossless, deterministic, and keys cannot collide across kinds", () => {
  const input = {
    query: "に",
    kanji: { character: "に" },
    particles: [{ surface: "に" }],
    patterns: [pattern("a"), pattern("b"), pattern("c"), pattern("d"), pattern("e")],
    words: [word("に"), word("二")],
    terms: [{ id: "prt", label: "Particle", aliases: ["particle"] }],
  };
  const results = composeReferenceResults(input);
  assert.deepEqual(results.map((result) => result.type), [
    "kanji", "particle", "grammar", "grammar", "grammar", "grammar",
    "word", "word", "grammar", "term",
  ]);
  assert.equal(new Set(results.map((result) => result.key)).size, results.length);
  assert.deepEqual(facetCounts(results), { all: 10, words: 2, grammar: 6, kanji: 1, terms: 1 });
  assert.deepEqual(facetResults(results, "words").map((result) => result.value.headword), ["に", "二"]);
});

test("selection abstains for homographs but opens one uniquely direct authored result", () => {
  const homographs = composeReferenceResults({ query: "seal", words: [word("印"), word("海豹")] });
  assert.equal(automaticSelection(homographs), null);
  const direct = composeReferenceResults({
    query: "passive",
    words: [word("受身")],
    terms: [{ id: "passive", label: "Passive", aliases: ["passive"] }],
  });
  assert.match(automaticSelection(direct), /^term:/);
});

test("a sole result opens regardless of kind", () => {
  const results = composeReferenceResults({ query: "猫", words: [word("猫")] });
  assert.equal(automaticSelection(results), "word:0");
});

test("POS membership reads every sense while the display heading stays canonical", () => {
  const mixed = word("勉強", [
    { tags: ["n"], glosses: ["study"] },
    { tags: ["vs"], glosses: ["to study"] },
  ]);
  assert.deepEqual(wordKinds(mixed), ["Noun", "Verb"]);
  assert.equal(canonicalWordKind(mixed), "Noun");
});

/**
 * **A word is often several parts of speech at once, and the badge now names all of them.**
 *
 * Two reports came out of naming only the first. 道中 is a noun *and* an adverb; filtering to
 * Adverbs listed it correctly and badged it NOUNS, because the heading was `wordKinds(word)[0]`
 * and that index is `WORD_KINDS` declaration order rather than anything about the word. The
 * same omission hid why the category counts overlap: 15 results reporting 15 nouns, 2 adverbs
 * and 1 verb sums to 18, which reads as three results gone missing.
 */
test("every kind a word belongs to is available to its badge", () => {
  const adverbialNoun = word("道中", [
    { tags: ["n"], glosses: ["journey; trip; travels"] },
    { tags: ["adv"], glosses: ["during the journey; on the way; while travelling"] },
  ]);
  assert.deepEqual(wordKinds(adverbialNoun), ["Noun", "Adverb"]);

  // The old single heading. The counts row still leads with it; the badge does not use it any
  // more, and this records that difference rather than leaving it implied.
  assert.equal(canonicalWordKind(adverbialNoun), "Noun");

  // One kind stays one label, so the common card gains nothing to read.
  const single = word("早い", [{ tags: ["adj-i"], glosses: ["fast; early"] }]);
  assert.deepEqual(wordKinds(single), ["Adjective"]);

  // An untagged entry still says something rather than nothing.
  const untagged = word("相槌", [{ tags: [], glosses: ["aizuchi"] }]);
  assert.deepEqual(wordKinds(untagged), ["Other"]);
});

/**
 * **A filtered card describes itself in the terms that were asked for.**
 *
 * Part of speech in Japanese belongs to a sense, not to a word: 道中 has a noun sense meaning
 * *journey* and an adverb sense meaning *on the way*. All-sense membership decided whether a
 * result appeared and nothing downstream knew which sense earned it, so filtering to Adverb
 * listed 道中 correctly and then printed its noun glosses under it.
 */
test("the senses shown for a word follow the kinds being filtered by", () => {
  const journey = { tags: ["n"], glosses: ["journey; trip; travels"] };
  const onTheWay = { tags: ["adv"], glosses: ["during the journey; on the way"] };
  const adverbialNoun = word("道中", [journey, onTheWay]);

  assert.deepEqual(senseKinds(journey), ["Noun"]);
  assert.deepEqual(senseKinds(onTheWay), ["Adverb"]);

  // The bug: asked for adverbs, shown the adverb sense only.
  assert.deepEqual(sensesForKinds(adverbialNoun, ["Adverb"]), [onTheWay]);
  assert.deepEqual(sensesForKinds(adverbialNoun, ["Noun"]), [journey]);

  // Several kinds selected: every sense either one matched, in the word’s own order.
  assert.deepEqual(sensesForKinds(adverbialNoun, ["Noun", "Adverb"]), [journey, onTheWay]);

  // Nothing asked, so nothing to narrow — the card reads as it always did.
  assert.deepEqual(sensesForKinds(adverbialNoun, []), adverbialNoun.inline);
  assert.deepEqual(sensesForKinds(adverbialNoun), adverbialNoun.inline);
});

/**
 * **A word under `Other` keeps its meanings.**
 *
 * `Other` is the fallback for a word no sense of which carries a recognised tag — roughly a
 * third of the dictionary. Every `senseKinds` is empty for such a word, so a rule that returned
 * only matching senses would hand back nothing and leave the card with no definition at all.
 */
test("an unclassified word keeps every sense when its own kind is filtered for", () => {
  const untagged = word("相槌", [{ tags: [], glosses: ["aizuchi; a listener’s interjection"] }]);
  assert.deepEqual(wordKinds(untagged), ["Other"]);
  assert.deepEqual(senseKinds(untagged.inline[0]), []);
  assert.deepEqual(sensesForKinds(untagged, ["Other"]), untagged.inline);
});

/**
 * **The colour has to agree with the filter, or the page contradicts itself.**
 *
 * Three surfaces read this now — the filter chip, the kind badge on a card, and the rule beside a
 * sense — and they must name the same five things. A kind whose token does not exist would draw
 * nothing at all, silently, because an undefined custom property is not an error.
 */
test("every kind the filter offers has a colour, and only Other has none", () => {
  for (const label of WORD_KIND_LABELS) {
    const colour = kindColour(label);
    if (label === "Other") {
      // Not a kind but the absence of one — roughly a third of the dictionary. Giving it a hue
      // would make "we do not know" look like a sixth answer.
      assert.equal(colour, "var(--text-faint)");
    } else {
      assert.equal(colour, `var(--kind-${label.toLowerCase()})`, label);
    }
  }
  // A label nothing produces still returns something drawable rather than `undefined`.
  assert.equal(kindColour("Nonsense"), "var(--text-faint)");
});

/**
 * A sense wears one rule, and it is the first kind in filter order.
 *
 * The row is scanned left to right, so the kind shown should be the one a reader narrowing by that
 * row would look for first. `null` where no tag names a kind, which draws no rule at all — an
 * absent classification should look absent rather than neutral-coloured.
 */
test("a sense is drawn as the first kind the filter row would offer it under", () => {
  assert.equal(leadingKind({ tags: ["n"] }), "Noun");
  assert.equal(leadingKind({ tags: ["adv"] }), "Adverb");
  // `exp` and `v5r` both match; Expression leads in `WORD_KINDS` order, which is the filter's.
  assert.equal(leadingKind({ tags: ["v5r", "exp"] }), "Expression");
  assert.equal(leadingKind({ tags: [] }), null);
  assert.equal(leadingKind({}), null);
});
