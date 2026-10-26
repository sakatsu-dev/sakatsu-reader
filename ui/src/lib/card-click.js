// @ts-check
/**
 * Telling a press apart from a drag, for cards that are content *and* click targets.
 *
 * **Asking whether text is selected is the wrong question**, and it broke the feature in the way
 * a reader hits first: Chromium selects a word on the second click of a double-click, so a card
 * checking `getSelection()` swallowed that click and every one after it until the selection
 * happened to clear — which from the reader's side is a card that stops responding until they
 * move the mouse "a certain way". The selection is also page-wide, so text selected anywhere at
 * all could freeze a card somewhere else.
 *
 * What actually distinguishes selecting from clicking is **travel**: dragging across a definition
 * moves the pointer, and pressing a card does not. Measuring that asks about this gesture only,
 * needs no global state, and cannot be confused by a selection someone left behind.
 */

/**
 * How far a pointer may drift between press and release and still count as a click.
 *
 * Not zero: a hand resting on a mouse moves it a pixel or two on the way down, and a click that
 * needed perfect stillness would fail for people with tremor before anyone else.
 */
export const DRAG_SLOP = 4;

/** @typedef {{ x: number, y: number }} Point */

/**
 * Did the pointer travel far enough between press and release to be selecting rather than
 * clicking?
 *
 * A missing origin answers `false`. A click with no press behind it is a synthetic one — assistive
 * technology, or a script — and those mean exactly what they say; a drag that *began* elsewhere
 * never delivers its click here anyway, because the event lands on the nearest ancestor the press
 * and release have in common.
 *
 * @param {Point | null} from Where the press landed.
 * @param {Point} at Where the release landed.
 */
export function travelled(from, at) {
  if (!from) return false;
  return Math.abs(at.x - from.x) > DRAG_SLOP || Math.abs(at.y - from.y) > DRAG_SLOP;
}

/**
 * What a press has to miss for the card to answer it.
 *
 * Two different reasons, kept in one list because they are one question — *did this press mean the
 * card?*
 *
 * The controls are the obvious half: a press on a button, a link, a field or a fold belongs to
 * that thing, and copying a headword must not open the entry underneath it.
 *
 * `.inline-detail` is the other half and less obvious. A shut card is a single target and a reader
 * should not have to learn which part of it responds — but once it is open, the detail is the
 * answer they asked for, and a stray press in the middle of it taking that away is the same
 * mistake as making the whole card a button was. The preview closes an open card; prose does not.
 */
export const CARD_CLICK_IGNORES = "button, a, input, summary, .inline-detail";
