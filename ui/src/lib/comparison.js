/**
 * Move a comparison selection through one click.
 *
 * A comparison is a question about one sentence. Adding within that sentence grows the question;
 * adding in another sentence starts a new question there. Removing a selected word only removes
 * that word. The function is pure so every pointer path gets the same transition and its laws can
 * be tested without mounting Svelte or starting Tauri.
 *
 * @param {ReadonlySet<number>} selected
 * @param {number} index
 * @param {ReadonlyArray<{ first_token: number, last_token_end: number }>} sentences
 */
export function toggleComparisonSelection(selected, index, sentences) {
  const next = new Set(selected);
  if (next.delete(index)) return next;

  const sentence = sentences.find(
    (range) => index >= range.first_token && index < range.last_token_end,
  );
  // A click from Reader always names a real token. Keeping the old state makes this helper total
  // without turning corrupt analysis metadata into an unrelated destructive transition.
  if (!sentence) return next;

  for (const selectedIndex of next) {
    if (selectedIndex < sentence.first_token || selectedIndex >= sentence.last_token_end) {
      next.delete(selectedIndex);
    }
  }
  next.add(index);
  return next;
}
