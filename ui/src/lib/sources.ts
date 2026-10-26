/**
 * A dictionary's name, without the revision it carries around.
 *
 * Yomitan dictionaries declare their own title in `index.json`, and the convention is to put
 * the build date in it: `Jitendex.org [2026-07-09]`, `JMnedict [2026-07-29]`. That suffix is
 * ours to display or not — we do not compose it, and the full string is kept and shown on
 * hover wherever this is used, so nothing is lost by leaving it off the row.
 *
 * Only the *trailing* bracket, and only one. A dictionary genuinely called `Foo [bar] Two`
 * keeps its middle, and one with no bracket at all is returned unchanged.
 */
export function sourceName(title: string): string {
  return title.replace(/\s*\[[^\]]*\]\s*$/, "").trim() || title;
}
