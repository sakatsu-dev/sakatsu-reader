import assert from "node:assert/strict";
import test from "node:test";
import { promoteSelected } from "./promote.js";

const key = (item) => item.key;

/**
 * **Promotion is an arrangement, not an answer.**
 *
 * Rows lifts the opened result to the front so a detail always appears in the same place. What
 * makes that safe is that it changes one position and nothing else: no result is dropped, none
 * is duplicated, and everything the search ranked below the promoted one stays in the order the
 * search put it in — so collapsing gives the answer back exactly as composed.
 */
test("promoting the selected result moves one card and disturbs no other", () => {
  const results = ["a", "b", "c", "d", "e"].map((key) => ({ key }));

  const moved = promoteSelected(results, "c", key);
  assert.deepEqual(moved.map((result) => result.key), ["c", "a", "b", "d", "e"]);
  assert.equal(moved.length, results.length, "promotion never drops or duplicates a result");
  assert.deepEqual(
    new Set(moved.map((result) => result.key)),
    new Set(results.map((result) => result.key)),
  );
  assert.deepEqual(
    moved.slice(1).map((result) => result.key),
    ["a", "b", "d", "e"],
    "the rest keep the order the search gave them",
  );

  assert.deepEqual(promoteSelected(results, "e", key).map((result) => result.key), ["e", "a", "b", "c", "d"]);
});

/**
 * **A fresh array every time, including when nothing moves.**
 *
 * This looks like a wasteful thing to pin until you know what identity cost. Handing the input
 * back unchanged meant a keyed `{#each}` saw no update, and with no update Svelte runs no
 * animation — so expanding the *first* card was the one expansion where the cards below it jumped
 * into place instead of sliding, because it is the one case where promotion reorders nothing.
 * Measured: the card below travelled through one position, against eleven for any other card.
 *
 * `assert.notEqual` is the reference check that pins it; `deepEqual` would pass either way and
 * prove nothing.
 */
test("promotion hands back a new list even when it changes nothing", () => {
  const results = ["a", "b", "c"].map((key) => ({ key }));
  for (const [selected, why] of [
    [null, "nothing selected"],
    [undefined, "no key yet"],
    ["missing", "a selection filtered out of view"],
    ["a", "already at the front"],
  ]) {
    const out = promoteSelected(results, selected, key);
    assert.deepEqual(out, results, why);
    assert.notEqual(out, results, `${why}: a new array, or the list will not animate`);
  }
  // The one shortcut that has to stay: `items[-1]` is `undefined`, and promoting a key that is
  // not in this list would otherwise put a hole at the front of it.
  assert.deepEqual(promoteSelected(results, "missing", key).map(key), ["a", "b", "c"]);
  assert.deepEqual(promoteSelected([], "a", key), []);
});

/**
 * **Both pages, one rule.** Glossary keys on `term.id` and Reference on `result.key`, which is
 * the whole reason `identify` is a parameter. Pin that the behaviour does not depend on which
 * field the caller uses — the alternative is two copies that drift, which is what putting this
 * in a module both import was meant to prevent.
 */
test("promotion is indifferent to which field carries the identity", () => {
  const terms = ["predicate", "copula", "noun", "pronoun"].map((id) => ({ id }));
  const byId = (term) => term.id;
  assert.deepEqual(
    promoteSelected(terms, "noun", byId).map(byId),
    ["noun", "predicate", "copula", "pronoun"],
  );
  assert.deepEqual(promoteSelected(terms, "predicate", byId), terms, "already at the front");
  assert.deepEqual(promoteSelected(terms, "adverb", byId), terms, "not in this section");
});
