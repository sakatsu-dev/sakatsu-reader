import assert from "node:assert/strict";
import test from "node:test";

import { endsSentence, expandForContext, resolvePath } from "./component-context.js";

const utf8 = (text) => new TextEncoder().encode(text).length;

/**
 * Lay words out over `text` left to right, giving each its UTF-8 byte span, and every part the
 * span of its own surface inside its word. A part given as `{ expanded: "いる" }` is a grammatical
 * form the text does not spell: it has no span and is not selectable. `{ withheld: true }` keeps a
 * grounded part's span but marks it not selectable. A word may carry `line_break`.
 */
function passage(text, ...words) {
  let cursor = 0;
  const place = (word, from) => {
    const at = text.indexOf(word.surface, from);
    assert.ok(at >= 0, `${word.surface} is in ${text}`);
    const start = utf8(text.slice(0, at));
    const end = start + utf8(word.surface);
    let inner = at;
    const components = (word.parts ?? []).map((part) => {
      if (part.expanded) {
        return { surface: part.expanded, start: null, end: null, selectable: false, components: [], line_break: "none" };
      }
      const placed = place(part, inner);
      inner = text.indexOf(part.surface, inner) + part.surface.length;
      return placed;
    });
    return {
      surface: word.surface,
      start,
      end,
      selectable: !word.withheld,
      line_break: word.line_break ?? "none",
      components,
    };
  };
  return words.map((word) => {
    const placed = place(word, cursor);
    cursor = text.indexOf(word.surface, cursor) + word.surface.length;
    return placed;
  });
}

const w = (surface, ...parts) => ({ surface, parts });
const surfaces = (tokens) => tokens.map((token) => token.surface);

/** Where sentences end, as source byte offsets — core's rule, applied to a token sequence. */
const sentenceEnds = (tokens) => tokens.filter(endsSentence).map((token) => token.end);

// --------------------------------------------------------------------------------- the resolver

test("a path naming a withheld part resolves to the word it was taken from", () => {
  const text = "食べてる";
  const [word] = passage(text, w("食べてる", w("食べて"), { expanded: "いる" }));
  const grammar = resolvePath(word, [1]);
  assert.deepEqual(grammar.path, []);
  assert.equal(grammar.token, word, "the heading, status and lookup all stay on the whole word");

  const real = resolvePath(word, [0]);
  assert.deepEqual(real.path, [0]);
  assert.equal(real.token.surface, "食べて");
});

test("a path whose validity changed between analyses is truncated, not followed", () => {
  const text = "かっこいい曲";
  const [before] = passage(text, w("かっこいい", w("かっこ"), w("いい")), w("曲"));
  // The same word re-analysed with its second part withheld — same shape, different verdict.
  const [after] = passage(text, w("かっこいい", w("かっこ"), { ...w("いい"), withheld: true }), w("曲"));
  assert.deepEqual(resolvePath(before, [1]).path, [1]);
  assert.deepEqual(resolvePath(after, [1]).path, []);
  // And the lookup agrees with the resolver in both cases.
  const tokensAfter = passage(text, w("かっこいい", w("かっこ"), { ...w("いい"), withheld: true }), w("曲"));
  const asked = expandForContext(tokensAfter, 0, [1], text);
  assert.deepEqual(asked.path, []);
  assert.equal(asked.tokens, tokensAfter);
  assert.equal(asked.tokens[asked.index].surface, "かっこいい");
});

test("a withheld step deep in a path keeps the steps before it", () => {
  const text = "ACDY";
  const tokens = passage(text, w("ACD", w("A"), w("CD", w("C"), { ...w("D"), withheld: true })), w("Y"));
  assert.deepEqual(resolvePath(tokens[0], [1, 1]).path, [1]);
  const asked = expandForContext(tokens, 0, [1, 1], text);
  assert.deepEqual(asked.path, [1]);
  assert.equal(asked.tokens[asked.index].surface, "CD");
});

// ------------------------------------------------------------------------------- the context

test("a part is asked about with the rest of its word still there", () => {
  const text = "かっこいい曲";
  const tokens = passage(text, w("かっこいい", w("かっこ"), w("いい")), w("曲"));
  const first = expandForContext(tokens, 0, [0], text);
  assert.deepEqual(surfaces(first.tokens), ["かっこ", "いい", "曲"]);
  assert.equal(first.tokens[first.index].surface, "かっこ");
  assert.deepEqual(first.path, [0]);
});

/**
 * **The defect this rewrite exists for.** 食べてる's grammatical parts are 食べて and いる. The
 * old expansion put いる — a word not in the text — into the context and lost the る that is.
 */
test("an expanded grammatical part is not put into the context, and the text it stands for is kept", () => {
  const text = "もう食べてるよ";
  const tokens = passage(text, w("もう"), w("食べてる", w("食べて"), { expanded: "いる" }), w("よ"));
  const part = expandForContext(tokens, 1, [0], text);
  assert.deepEqual(surfaces(part.tokens), ["もう", "食べて", "る", "よ"]);
  assert.equal(part.tokens[part.index].surface, "食べて");
  assert.equal(surfaces(part.tokens).join(""), text, "every character kept");
  const filler = part.tokens[2];
  assert.deepEqual([filler.start, filler.end], [utf8("もう食べて"), utf8("もう食べてる")]);
  assert.equal(filler.selectable, false);
});

test("gaps before, between and after the selectable parts are all kept, in order", () => {
  const text = "あかいさくら";
  const tokens = passage(text, w("あかいさくら", w("い"), w("く")));
  const expanded = expandForContext(tokens, 0, [1], text);
  assert.deepEqual(surfaces(expanded.tokens), ["あか", "い", "さ", "く", "ら"]);
  assert.equal(expanded.tokens[expanded.index].surface, "く");
});

/**
 * **Equal text is not an equal context.** Across a line that ends a sentence, the word's `break`
 * must stay where the sentence ends — on its last piece, here a trailing filler — and no piece may
 * end a sentence early.
 */
test("a multiline passage keeps its sentence ends through expansion, trailing filler included", () => {
  const text = "もう食べてる\n次は寝る";
  const tokens = passage(
    text,
    w("もう"),
    { ...w("食べてる", w("食べて"), { expanded: "いる" }), line_break: "break" },
    w("次"),
    w("は"),
    w("寝る"),
  );
  const expanded = expandForContext(tokens, 1, [0], text);
  assert.deepEqual(surfaces(expanded.tokens), ["もう", "食べて", "る", "次", "は", "寝る"]);
  assert.deepEqual(sentenceEnds(expanded.tokens), sentenceEnds(tokens), "the sentence still ends after る");
  assert.equal(expanded.tokens[2].line_break, "break", "carried by the trailing filler");
  assert.equal(expanded.tokens[1].line_break, "none");
  assert.equal(tokens[1].line_break, "break", "and the analysis itself was not touched");
  assert.equal(tokens[1].components[0].line_break, "none");
});

test("a nested expansion keeps the sentence end on the last piece at every level", () => {
  const text = "ACDE。Y";
  const tokens = passage(
    text,
    { ...w("ACDE", w("A"), w("CD", w("C"), w("D"))), line_break: "break" },
    w("。"),
    w("Y"),
  );
  // The word ends a sentence by its break; after two levels of expansion, the last piece — the
  // trailing filler E — must carry it, so the sentence still ends at the same byte.
  const inner = expandForContext(tokens, 0, [1, 0], text);
  assert.deepEqual(surfaces(inner.tokens), ["A", "C", "D", "E", "。", "Y"]);
  assert.deepEqual(inner.path, [1, 0]);
  assert.deepEqual(sentenceEnds(inner.tokens), sentenceEnds(tokens));
  assert.equal(inner.tokens[inner.index].surface, "C");
});

test("a word that would put sentence-final punctuation on an inner piece is not expanded", () => {
  const text = "A。B";
  const tokens = passage(text, w("A。B", w("A"), w("B")));
  const asked = expandForContext(tokens, 0, [0], text);
  assert.deepEqual(asked.path, [], "the boundary would move, so the path stops at the word");
  assert.equal(asked.tokens, tokens);
});

test("the whole word is asked about unchanged", () => {
  const text = "かっこいい曲";
  const tokens = passage(text, w("かっこいい", w("かっこ"), w("いい")), w("曲"));
  const whole = expandForContext(tokens, 0, [], text);
  assert.equal(whole.tokens, tokens, "no path means no new array at all");
  assert.equal(whole.index, 0);
});

test("neighbours before the word keep their places", () => {
  const text = "そのかっこいい曲";
  const tokens = passage(text, w("その"), w("かっこいい", w("かっこ"), w("いい")), w("曲"));
  const part = expandForContext(tokens, 1, [1], text);
  assert.deepEqual(surfaces(part.tokens), ["その", "かっこ", "いい", "曲"]);
  assert.equal(part.tokens[part.index].surface, "いい");
});

test("a part of a part keeps every sibling at every level", () => {
  // ACD(A, CD(C, D)) Y — selecting C must not lose A, D or Y.
  const text = "ACDY";
  const tokens = passage(text, w("ACD", w("A"), w("CD", w("C"), w("D"))), w("Y"));
  const inner = expandForContext(tokens, 0, [1, 0], text);
  assert.deepEqual(surfaces(inner.tokens), ["A", "C", "D", "Y"]);
  assert.equal(inner.tokens[inner.index].surface, "C");
  const middle = expandForContext(tokens, 0, [1], text);
  assert.deepEqual(surfaces(middle.tokens), ["A", "CD", "Y"]);
});

test("a path that outlived its analysis stops at the deepest word it still names", () => {
  const text = "かっこいい曲";
  const tokens = passage(text, w("かっこいい", w("かっこ"), w("いい")), w("曲"));
  const tooDeep = expandForContext(tokens, 0, [1, 0], text);
  assert.deepEqual(tooDeep.path, [1]);
  assert.equal(tooDeep.tokens[tooDeep.index].surface, "いい");
  const missing = expandForContext(tokens, 0, [7], text);
  assert.equal(missing.tokens, tokens);
  assert.deepEqual(missing.path, []);
});

test("an index outside the passage is left alone", () => {
  const text = "曲";
  const tokens = passage(text, w("曲"));
  assert.equal(expandForContext(tokens, 5, [0], text).tokens, tokens);
  assert.equal(expandForContext(tokens, -1, [0], text).index, -1);
});
