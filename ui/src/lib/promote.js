// @ts-check
/**
 * Lifting the opened item to the front of the list it is in.
 *
 * **Here rather than in either page, because both must agree.** Reference and Glossary draw
 * different things — a heterogeneous answer, a curated concept index — but they expand the same
 * way, and an expansion that behaved differently on the two pages would be a difference the
 * reader has to learn for no reason. `traps.md` records the rule: a list two components must
 * agree on cannot live in either of them.
 */

/**
 * The same items, with the selected one moved to the front.
 *
 * **A presentation move, not a re-ranking.** Whatever decided the order — the search's own
 * ranking in Reference, a category's authored sequence in Glossary — is untouched. One item is
 * lifted out, and put back the moment nothing is selected, so collapsing restores the original
 * order exactly.
 *
 * Order-preserving for everything else, and lossless: same members, same relative sequence, one
 * position changed.
 *
 * **Always a new array, including when nothing moves.** It used to hand the input back by identity
 * for the no-op cases — no key, a key not here, a key already at the front — to save a keyed list
 * from reconciling. That saving is what made expanding the *first* card the one expansion with no
 * animation: with no new array there is no update, with no update `animate:flip` never runs, and
 * the cards below it jumped to their new positions while every other card's expansion slid. The
 * callers' inputs are freshly filtered arrays anyway, so the shortcut was saving almost nothing
 * and costing the one case where the difference showed.
 *
 * The `at < 0` guard is *not* the same shortcut and cannot go: `items[-1]` is `undefined`, and
 * promoting a key that is not in this list would put a hole at the front of it.
 *
 * `identify` is required rather than defaulted: the two callers key on different fields
 * (`result.key`, `term.id`) and a default would quietly favour one of them.
 *
 * @template T
 * @param {T[]} items
 * @param {string | null | undefined} selectedKey
 * @param {(item: T) => string} identify
 * @returns {T[]}
 */
export function promoteSelected(items, selectedKey, identify) {
  if (!selectedKey) return [...items];
  const at = items.findIndex((item) => identify(item) === selectedKey);
  if (at < 0) return [...items];
  return [items[at], ...items.slice(0, at), ...items.slice(at + 1)];
}

/**
 * Bring a promoted item's **head** into view inside its scroller.
 *
 * `scrollIntoView({ block: "nearest" })` cannot do this: for an element taller than the scroller
 * with none of it showing, the nearest edge is its *bottom*, so it lands past the thing that was
 * just opened. See `traps.md`. `anchor` is what should top out — the item itself where it is the
 * collection's own first child, or the section around it where the list is grouped under
 * headings and the heading is part of what identifies the item.
 *
 * @param {HTMLElement} scroller
 * @param {HTMLElement} anchor
 */
export function revealHead(scroller, anchor) {
  // **The top of the scroller, not the top of the anchor**, when the anchor is already the first
  // thing in it. A first child sits a collapsed margin below the scroller's edge — 11px, for the
  // glossary's section headings — and aligning to its box would shave that margin off, on every
  // expansion, for no reason at all.
  // **The top of the scroller, not the top of the anchor**, when the anchor is already the first
  // thing in it. A first child sits a collapsed margin below the scroller's edge — 11px, for the
  // glossary's section headings — and aligning to its box would shave that margin off, on every
  // expansion, for no reason at all.
  if (anchor === scroller.firstElementChild) {
    scroller.scrollTop = 0;
    return;
  }
  scroller.scrollTop += anchor.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
}
