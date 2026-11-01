import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const character = readFileSync(new URL("./Character.svelte", import.meta.url), "utf8");

const reference = readFileSync(new URL("./Reference.svelte", import.meta.url), "utf8");

test("stroke diagrams stay typed and lazy instead of injecting downloaded markup", () => {
  assert.match(character, /invoke<KanjiDiagram \| null>\("kanji_diagram"/);
  assert.match(character, /diagram\.strokes as stroke/);
  assert.match(character, /<path class="stroke"/);
  assert.doesNotMatch(character, /\{@html\}/);
});

test("the complete stroke order is static until a learner asks to play it", () => {
  assert.match(character, /playing \? "Replay stroke order" : "Play stroke order"/);
  assert.match(character, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(character, /\.diagram\.playing \.stroke/);
  assert.doesNotMatch(character, /autoplay/i);
});

test("name readings cannot inherit the used-here treatment", () => {
  const names = character.slice(
    character.indexOf("{#if character.nanori.length"),
    character.indexOf("{#if character.meanings.length"),
  );
  // The label moved into a `<dl>`; what this test is actually about is the line below it.
  assert.match(names, /<dt>Nanori<\/dt>/);
  assert.doesNotMatch(names, /class:here/);
});

test("components disclose their meaning and related characters use current-tab lookup", () => {
  assert.match(character, /<h4 class="section">Components<\/h4>/);
  assert.match(character, /not necessarily traditional radicals/);
  assert.match(character, /invoke<RelatedKanjiResult>\("kanji_related"/);
  assert.match(character, /limit: 24/);
  assert.match(character, /onclick=\{\(\) => onlookup\(item\.character\)\}/);
  const choices = readFileSync(new URL("./WordCharacters.svelte", import.meta.url), "utf8");
  assert.match(reference, /<WordCharacters characters={word\.chars}/);
  assert.match(choices, /{#each characters as character/);
  assert.match(choices, /<Character character={active} {onlookup} editorial/);
});
