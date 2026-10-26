import assert from "node:assert/strict";
import test from "node:test";
import {
  glossaryAliasMatch,
  glossaryHighlightSegments,
  glossaryTermMatches,
} from "./glossary-flow.js";

const term = {
  id: "conjunctive",
  label: "Conjunctive form",
  aliases: ["te-form", "連用形"],
  summary: "A form that connects one construction to another.",
  detail: "Often carries the first event into the next one.",
};

test("filtering covers every name a term has, under one normalisation rule", () => {
  for (const query of ["conjunctive", "Conjunctive form", "TE-FORM", "連用"]) {
    assert.equal(glossaryTermMatches(term, query), true, query);
  }
  assert.equal(glossaryTermMatches(term, "unrelated"), false);
});

test("what a term says about itself is not something it can be found by", () => {
  // Both are in this term's prose and neither is any of its names. Matching them is what made
  // "Noun" return Predicate, Copula, Nominaliser and Adnominal before the term called Noun.
  assert.equal(term.summary.includes("connects"), true);
  assert.equal(term.detail.includes("first event"), true);
  assert.equal(glossaryTermMatches(term, "connects"), false);
  assert.equal(glossaryTermMatches(term, "FIRST EVENT"), false);
});

test("the index discloses an alias only when the alias caused the match", () => {
  assert.equal(glossaryAliasMatch(term, "te-form"), "te-form");
  assert.equal(glossaryAliasMatch(term, "conjunctive"), null);
});

test("highlighting returns text segments and preserves compatibility-character source text", () => {
  assert.deepEqual(glossaryHighlightSegments("Te-form and te-form", "TE-FORM"), [
    { text: "Te-form", hit: true },
    { text: " and ", hit: false },
    { text: "te-form", hit: true },
  ]);
  assert.deepEqual(glossaryHighlightSegments("Ｔｅ form", "te"), [
    { text: "Ｔｅ", hit: true },
    { text: " form", hit: false },
  ]);
  assert.deepEqual(glossaryHighlightSegments("plain", "missing"), [{ text: "plain", hit: false }]);
});
