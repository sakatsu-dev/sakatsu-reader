/**
 * Whether a word's reading is worth printing beside it.
 *
 * # Why this is its own module
 *
 * Two surfaces ask it — the Analysis panel's `Vocabulary.svelte` and the Reference page's
 * `WordHeader.svelte` — and they used to answer differently. `WordHeader` compared the reading
 * against the headword; `Vocabulary` printed whatever it was given. So the same kana word showed
 * one reading on one page and two on the other, which is the drift a shared rule exists to stop.
 * It is pure, so it lives here where `node --test` can reach it.
 *
 * # The rule, and why it is not "are these two strings equal"
 *
 * A reading exists to say **how the characters sound**. A word written entirely in kana already
 * says that: the characters *are* the sound. Printing つたわる under つたわる is the obvious case,
 * and equality catches it — but it misses ラーメン under らーめん, which is the same redundancy
 * spelled differently, and a learner who can read one syllabary can read the other.
 *
 * So the question is about the **headword**, not about the pair: has it anything left to explain?
 * 気持ち has 気 and 持; ヶ月 has 月. つたわる and コーヒー have nothing.
 */

/**
 * A kana letter — hiragana or katakana, excluding punctuation and the ー length mark.
 *
 * The same ranges as `core/src/kana.rs`'s `is_kana`, and deliberately so: two definitions of
 * "this is kana" in one app is one too many, and this one exists only because the answer is
 * needed in the browser.
 *
 * @param {string} c one character
 */
export function isKanaLetter(c) {
  const at = c.codePointAt(0) ?? 0;
  return (at >= 0x3041 && at <= 0x3096) || (at >= 0x30a1 && at <= 0x30fa);
}

/**
 * The marks a kana word is written *with*, which are not letters and do not disqualify it.
 *
 * ー is the length mark (コーヒー), ・ separates the parts of a foreign name (セイア・ミサキ),
 * ゛ ゜ are combining voice marks, and ゝゞヽヾ are iteration marks. `core/src/kana.rs` makes
 * the same allowance for the same reason, in `is_katakana_word`.
 */
const MARKS = new Set(["ー", "・", "゛", "゜", "ゝ", "ゞ", "ヽ", "ヾ", "〜", "～"]);

/**
 * Whether a word is written in kana and nothing else.
 *
 * **At least one kana letter**, so a token that is nothing but marks — a bare ・ comes through
 * the analyzer as its own token — does not answer yes. That mistake has already been made once
 * on the Rust side and is written up in `kana.rs`.
 *
 * @param {string} word
 */
export function isAllKana(word) {
  const characters = [...word];
  return (
    characters.some(isKanaLetter) &&
    characters.every((c) => isKanaLetter(c) || MARKS.has(c))
  );
}

/**
 * Whether to draw the reading under `headword`.
 *
 * Both conditions earn their place. A kana headword has nothing for a reading to explain; a
 * reading identical to its headword is the same redundancy arriving by another route, and cheap
 * to catch here rather than trusting every dictionary to have suppressed it.
 *
 * @param {string} headword
 * @param {string | null | undefined} reading
 */
export function readingWorthShowing(headword, reading) {
  if (!reading) return false;
  if (reading === headword) return false;
  return !isAllKana(headword);
}

/**
 * Whether to say what the reader actually typed, beside the headword it resolved to.
 *
 * The two differ exactly when the analyzer did something — 走った resolving to 走る — and on a page
 * for learners that difference *is* the lesson rather than plumbing to hide. When they are the
 * same there is nothing to say, and saying it anyway would put a chip on every card.
 *
 * Shared because two surfaces draw it: the Analysis page's selected-word header and a Reference
 * result card. It began as an inline comparison in the first of those, and a second copy in the
 * second is how they come to disagree about a trimmed or empty surface.
 *
 * @param {string} headword the form the dictionary answered with
 * @param {string | null | undefined} surface the form that was typed or clicked
 */
export function surfaceWorthShowing(headword, surface) {
  if (!surface) return false;
  return surface !== headword;
}

/**
 * A kanji reading, written the way Japanese writes a partial word.
 *
 * KANJIDIC2 marks a reading that only ever appears attached to something else with an ASCII
 * hyphen: `-がわ` is the 川 of 小川, never a word on its own, and `お-` is a prefix. The mark
 * carries real information — it is the difference between a reading a learner can use and one
 * they will only meet inside a compound — so it stays. What changes is the character.
 *
 * **`〜`, because that is the app's own convention and the language's.** Grammar patterns are
 * written 〜ている, 〜ば and 〜そうだ throughout the packs; the wave dash is what a Japanese
 * dictionary, a textbook and this app all use to say "something goes here". A hyphen beside kana
 * reads as punctuation from a different alphabet, and next to `たっと.い` — where the dot is
 * already doing a different job — two kinds of ASCII marker in one line is one too many.
 *
 * Leading and trailing only, which is everywhere KANJIDIC puts it; nothing inside a reading is
 * touched, so the `.` that separates stem from okurigana is left exactly as it is.
 *
 * **Display only.** `dict::kanji::candidates` in the Rust core still trims `'-'` when it matches
 * a reading against a word, and it reads the stored data rather than this. Rewriting the stored
 * form instead would have silently broken that matching.
 *
 * @param {string | null | undefined} reading
 * @returns {string}
 */
export function writtenReading(reading) {
  return String(reading ?? "").replace(/^-/u, "〜").replace(/-$/u, "〜");
}

/**
 * A kanji reading in its two parts: what the character supplies, and what is written beside it.
 *
 * KANJIDIC2 separates them with a period — `はし.る` is 走 read はし with る written as kana after
 * it — and that period is the thing this exists to stop relying on. At the size these are drawn a
 * baseline dot between two kana is easy to miss entirely, and missing it means misreading which
 * sound the kanji is actually responsible for, which is the one fact the row is there to teach.
 *
 * **So the split is returned rather than printed.** The caller draws the okurigana a step fainter
 * than the stem, and the difference in tone says what the dot was saying. Nothing is inserted
 * between them, which matters more here than it looks: every mark that could go there is already
 * spoken for or unsafe — `〜` marks an affix reading, `・` joins alternate forms in the spellings
 * row, and any dash sits next to kana looking like `ー`, the prolonged sound mark, so `はし—る`
 * invites a long vowel that is not in the word.
 *
 * Split at the first period only. A reading has at most one in KANJIDIC, and anything further
 * along belongs to the okurigana rather than starting a third part.
 *
 * @param {string | null | undefined} reading
 * @returns {{ stem: string, okurigana: string }}
 */
export function splitReading(reading) {
  const written = writtenReading(reading);
  const at = written.indexOf(".");
  if (at < 0) return { stem: written, okurigana: "" };
  return { stem: written.slice(0, at), okurigana: written.slice(at + 1) };
}
