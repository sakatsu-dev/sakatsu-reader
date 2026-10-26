import assert from "node:assert/strict";
import { test } from "node:test";

import { isAllKana, isKanaLetter, readingWorthShowing, splitReading, surfaceWorthShowing, writtenReading } from "./kana.js";

test("a word with kanji in it is not all kana", () => {
  assert.equal(isAllKana("気持ち"), false);
  assert.equal(isAllKana("伝わる"), false);
  // ヶ is a small katakana ke, not a kanji — but 月 is, and that is what decides it.
  assert.equal(isAllKana("ヶ月"), false);
  assert.equal(isAllKana("々"), false);
});

test("kana with the marks kana words are written with is still all kana", () => {
  assert.equal(isAllKana("つたわる"), true);
  assert.equal(isAllKana("コーヒー"), true, "the ー length mark belongs to the word");
  assert.equal(isAllKana("セイア・ミサキ"), true, "and so does the ・ between name parts");
  assert.equal(isAllKana("ラーメン"), true);
});

/**
 * The mistake `core/src/kana.rs` records having made: the katakana block holds punctuation as
 * well as letters, so a test of the block alone answers yes for a token that is nothing but
 * marks. The analyzer emits bare punctuation as its own token, so this is reachable.
 */
test("marks alone are not a kana word", () => {
  assert.equal(isAllKana("・"), false);
  assert.equal(isAllKana("ー"), false);
  assert.equal(isAllKana(""), false);
});

test("a kana letter is a letter, and a mark is not", () => {
  assert.equal(isKanaLetter("あ"), true);
  assert.equal(isKanaLetter("ヲ"), true);
  assert.equal(isKanaLetter("ー"), false);
  assert.equal(isKanaLetter("漢"), false);
});

test("a reading is shown when the headword has something left to explain", () => {
  assert.equal(readingWorthShowing("伝わる", "つたわる"), true);
  assert.equal(readingWorthShowing("気持ち", "きもち"), true);
  assert.equal(readingWorthShowing("ヶ月", "かげつ"), true);
});

test("a kana headword explains itself", () => {
  assert.equal(readingWorthShowing("つたわる", "つたわる"), false);
  assert.equal(
    readingWorthShowing("ラーメン", "らーめん"),
    false,
    "the same redundancy as つたわる/つたわる, spelled across two syllabaries",
  );
  assert.equal(readingWorthShowing("コーヒー", "コーヒー"), false);
});

test("nothing to show is not shown", () => {
  assert.equal(readingWorthShowing("伝わる", null), false);
  assert.equal(readingWorthShowing("伝わる", ""), false);
  assert.equal(readingWorthShowing("伝わる", undefined), false);
});

/** A headword identical to its reading is redundant whatever it is made of. */
test("a reading that repeats its headword is never shown", () => {
  assert.equal(readingWorthShowing("ABC", "ABC"), false);
  assert.equal(readingWorthShowing("々", "々"), false);
});

/**
 * **The negative case is the one that matters.** A chip that appears on every card says nothing;
 * this one earns its space only when the analyzer changed the word on the reader's behalf.
 */
test("the typed form is worth showing only when it differs from the answer", () => {
  assert.equal(surfaceWorthShowing("走る", "走った"), true, "a conjugated form was resolved");
  assert.equal(surfaceWorthShowing("走る", "走る"), false, "nothing happened, so there is nothing to report");
  // Every way a caller can arrive without a surface: an English hit makes no such claim, and a
  // token can carry an empty one.
  assert.equal(surfaceWorthShowing("走る", ""), false);
  assert.equal(surfaceWorthShowing("走る", null), false);
  assert.equal(surfaceWorthShowing("走る", undefined), false);
});

test("a suffix reading is marked with a wave dash rather than a hyphen", () => {
  assert.equal(writtenReading("-がわ"), "〜がわ");
});

test("a prefix reading is marked at the end", () => {
  assert.equal(writtenReading("お-"), "お〜");
});

test("an ordinary reading is untouched, and so is the okurigana dot", () => {
  assert.equal(writtenReading("かわ"), "かわ");
  assert.equal(writtenReading("たっと.い"), "たっと.い");
  assert.equal(writtenReading("キ"), "キ");
});

test("a hyphen inside a reading is not a marker and is left alone", () => {
  // KANJIDIC only ever puts it at an end. Anything in the middle is not this rule's business.
  assert.equal(writtenReading("あ-い"), "あ-い");
});

test("nothing at all is nothing, not the string null", () => {
  assert.equal(writtenReading(null), "");
  assert.equal(writtenReading(undefined), "");
});

test("a reading with okurigana comes back in two parts", () => {
  assert.deepEqual(splitReading("はし.る"), { stem: "はし", okurigana: "る" });
  assert.deepEqual(splitReading("たてまつ.る"), { stem: "たてまつ", okurigana: "る" });
});

test("a reading with no okurigana is all stem", () => {
  assert.deepEqual(splitReading("かわ"), { stem: "かわ", okurigana: "" });
  assert.deepEqual(splitReading("ソウ"), { stem: "ソウ", okurigana: "" });
});

test("an affix mark survives the split and stays with the stem", () => {
  assert.deepEqual(splitReading("-がき"), { stem: "〜がき", okurigana: "" });
});

test("an affix reading that also has okurigana keeps both marks", () => {
  assert.deepEqual(splitReading("-づ.ける"), { stem: "〜づ", okurigana: "ける" });
});

test("only the first period splits; a later one belongs to the okurigana", () => {
  assert.deepEqual(splitReading("あ.て.る"), { stem: "あ", okurigana: "て.る" });
});

test("nothing at all splits into nothing", () => {
  assert.deepEqual(splitReading(null), { stem: "", okurigana: "" });
});
