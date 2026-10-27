import { test, expect } from "@playwright/test";
import { probeFonts, requireJapaneseFont } from "./fixtures";

test("Japanese layout fonts are proved by rendered glyphs", async ({ browser }) => {
  const environment = await probeFonts(browser);
  requireJapaneseFont(environment);
  expect(environment.fonts.japanese.rendered.some((font) => font.glyphCount > 0)).toBe(true);
  expect(environment.fonts.previousSerif.rendered.length).toBeGreaterThan(0);
  expect(environment.browserVersion).not.toBe("");
});

test("a missing requested font cannot silently pass through fallback", async ({ browser }) => {
  const environment = await probeFonts(browser, "SakatsuDeliberatelyMissingFont");
  expect(() => requireJapaneseFont(environment)).toThrow("actual fonts:");
});

test("a requested family without matching rendered glyphs is refused", async ({ browser }) => {
  const environment = await probeFonts(browser);
  environment.fonts.japanese.rendered = environment.fonts.japanese.rendered.map((font) => ({
    ...font,
    familyName: "Unexpected fallback",
  }));
  expect(() => requireJapaneseFont(environment)).toThrow("actual fonts:");
  environment.fonts.japanese.rendered = [];
  expect(() => requireJapaneseFont(environment)).toThrow("actual fonts:");
  const mixed = await probeFonts(browser);
  mixed.fonts.japaneseInProse.rendered = mixed.fonts.japaneseInProse.rendered.map((font) => ({
    ...font,
    familyName: "Unexpected prose fallback",
  }));
  expect(() => requireJapaneseFont(mixed)).toThrow("japaneseInProse");
});
