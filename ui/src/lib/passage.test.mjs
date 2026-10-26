import assert from "node:assert/strict";
import test from "node:test";

import { lineUnits, passageLines } from "./passage.js";

/** Tokenise a passage on the given surfaces, with the byte offsets Rust would report. */
function tokensFor(text, surfaces) {
  const encoder = new TextEncoder();
  let searched = 0;
  return surfaces.map((surface) => {
    const at = text.indexOf(surface, searched);
    assert.notEqual(at, -1, `${surface} is not in the passage`);
    searched = at + surface.length;
    return {
      surface,
      start: encoder.encode(text.slice(0, at)).length,
      end: encoder.encode(text.slice(0, at + surface.length)).length,
    };
  });
}

const PARTS = ["提出", "締め切り", "！", "つぎ", "の", "ステップ", "へ"];

test("a passage with no newline is one line", () => {
  // The bug this exists for: one line of input came back as two, split after the ！, with the
  // width of the window empty beside each.
  const text = "提出締め切り！つぎのステップへ";
  assert.deepEqual(passageLines(text, tokensFor(text, PARTS)), [[0, 1, 2, 3, 4, 5, 6]]);
});

test("a newline in the passage is a line break", () => {
  const text = "提出締め切り！\nつぎのステップへ";
  assert.deepEqual(passageLines(text, tokensFor(text, PARTS)), [
    [0, 1, 2],
    [3, 4, 5, 6],
  ]);
});

test("the offsets are read as bytes, not as JavaScript indices", () => {
  // **The trap.** Rust reports UTF-8 byte offsets; JS strings index UTF-16 code units. They agree
  // for ASCII, so a test written in English passes either way — and every real Japanese passage
  // splits in the wrong place. 提出締め切り！ is 7 characters and 21 bytes.
  const text = "提出締め切り！\nつぎのステップへ";
  const tokens = tokensFor(text, PARTS);
  assert.equal(tokens[3].start, 22, "つぎ starts after 21 bytes of text and one of newline");

  // Fed JS indices instead, the break would land in the wrong gap. This asserts the real ones work.
  assert.deepEqual(passageLines(text, tokens)[0], [0, 1, 2]);
});

test("several blank lines do not make several empty lines", () => {
  const text = "提出締め切り！\n\n\nつぎのステップへ";
  assert.deepEqual(passageLines(text, tokensFor(text, PARTS)), [
    [0, 1, 2],
    [3, 4, 5, 6],
  ]);
});

test("a Windows line ending breaks once, not twice", () => {
  const text = "提出締め切り！\r\nつぎのステップへ";
  assert.deepEqual(passageLines(text, tokensFor(text, PARTS)), [
    [0, 1, 2],
    [3, 4, 5, 6],
  ]);
});

test("without the passage every token lands on one line", () => {
  // Guessing at breaks is the behaviour this replaced, so the fallback is not to guess.
  const tokens = PARTS.map((surface, index) => ({ surface, start: index, end: index + 1 }));
  assert.deepEqual(passageLines(null, tokens), [[0, 1, 2, 3, 4, 5, 6]]);
});

test("a token the analyzer could not ground does not invent a break", () => {
  const text = "提出締め切り！つぎのステップへ";
  const tokens = tokensFor(text, PARTS);
  tokens[3] = { ...tokens[3], start: null, end: null };
  assert.deepEqual(passageLines(text, tokens), [[0, 1, 2, 3, 4, 5, 6]]);
});

test("no tokens is no lines, not one empty one", () => {
  assert.deepEqual(passageLines("", []), []);
});

/**
 * The reader flows a capture as one paragraph unless asked otherwise.
 *
 * A manga panel is broken by the width of its bubble, so honouring those breaks spends height on
 * the artwork's shape rather than the sentence's. The passage that prompted this was one sentence
 * set as two columns, and it arrived as two lines with the pane empty beside both.
 */
test("without keepBreaks every token lands on one flowing line", () => {
  // Written as a template literal with a real line break in it, so no escape has to survive
  // being read back — the same passage that started this, 秘めたる処も入念に責め立つるぞぉ.
  const text = `秘めたる処も
入念に責め立つるぞぉ`;
  const tokens = tokensFor(text, ["秘め", "たる", "処", "も", "入念", "に", "責め立つる", "ぞぉ"]);
  const flat = [0, 1, 2, 3, 4, 5, 6, 7];

  assert.deepEqual(passageLines(text, tokens, true), [[0, 1, 2, 3], [4, 5, 6, 7]]);
  assert.deepEqual(passageLines(text, tokens, false), [flat]);
  // The parameter defaults to the strict behaviour; the app passes the setting explicitly.
  assert.deepEqual(passageLines(text, tokens), [[0, 1, 2, 3], [4, 5, 6, 7]]);
});

test("flowing a passage with nothing in it still produces no lines at all", () => {
  assert.deepEqual(passageLines("", [], false), []);
  assert.deepEqual(passageLines(null, [], false), []);
});

/** The reader's rule, with words told apart from marks by whether they hold a letter. */
const units = (surfaces) =>
  lineUnits(
    surfaces.map((_, index) => index),
    surfaces.map((surface) => ({ surface })),
    (token) => /[ぁ-ゖァ-ヺ一-鿿A-Za-z]/.test(token.surface),
  ).map((unit) => unit.map((index) => surfaces[index]).join(""));

test("a closing mark stays with the word before it, so 。 never begins a line", () => {
  // The screenshot case: 座っていた。 at the end of a line sent the 。 down on its own.
  assert.deepEqual(units(["ベンチ", "に", "座っていた", "。"]), ["ベンチ", "に", "座っていた。"]);
  assert.deepEqual(units(["帰る", "と", "、", "姉"]), ["帰る", "と、", "姉"]);
  // Several closing marks in a row all stay with the word.
  assert.deepEqual(units(["行く", "」", "。"]), ["行く」。"]);
});

test("an opening mark stays with the word after it, so 「 never ends a line", () => {
  assert.deepEqual(units(["彼", "は", "「", "行く", "」", "と"]), ["彼", "は", "「行く」", "と"]);
  // An opening mark that closes the line is still shown, on its own.
  assert.deepEqual(units(["彼", "「"]), ["彼", "「"]);
});

test("grouping never drops, repeats or reorders a token", () => {
  const surfaces = ["「", "お", "天気", "いい", "し", "、", "散歩", "に", "行こう", "かな", "。", "」"];
  const indices = lineUnits(
    surfaces.map((_, index) => index),
    surfaces.map((surface) => ({ surface })),
    (token) => /[ぁ-ゖ一-鿿]/.test(token.surface),
  ).flat();
  assert.deepEqual(indices, surfaces.map((_, index) => index));
  // And a mark at the very start of a line, with no word yet to join, is a unit of its own.
  assert.deepEqual(units(["。", "彼"]), ["。", "彼"]);
});
