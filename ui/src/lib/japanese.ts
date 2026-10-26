import type { Token } from "../model";

/**
 * Whether a token is vocabulary rather than punctuation.
 *
 * Mirrors `Token::is_word` in `core/src/model.rs`, including its exclusions: the ・
 * middle dot (U+30FB) and the full-width punctuation Ichiran passes through are not
 * letters, so they are not clickable and get no dictionary row.
 *
 * Duplicated in TypeScript deliberately. The alternative is shipping a derived
 * boolean over the IPC for every token on every analysis, and this rule is a fixed
 * property of Unicode rather than something the analyzer decides.
 */
const WORD_CHARACTER =
  /[ぁ-ゖゝ-ゞァ-ヺー-ヾ㐀-鿿豈-﫿ｦ-ﾝ]/;

export function isWord(token: Token): boolean {
    return WORD_CHARACTER.test(token.surface) || WORD_CHARACTER.test(token.dict_form);
}
