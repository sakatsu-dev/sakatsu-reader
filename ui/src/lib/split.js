/**
 * Where a split's divider may sit.
 *
 * `Split.svelte` stores its position as a **share** of the container — 0.45 means the first pane
 * takes 45% — which is the right thing to persist, because it survives the window being resized.
 * It is the wrong thing to *clamp against on its own*: a share says nothing about how many pixels
 * it buys, and a pane can hold something that has a real minimum size in pixels. One line of
 * Japanese with furigana above it and a status rule below it is 85px at the default reading scale,
 * and 20% of a short window is not.
 *
 * So a pane may declare a floor in CSS, and the ratio is clamped against **both**: the share limits
 * that shape the layout, and the pixel floor that stops content being cut. Doing it here rather than
 * leaving it to flexbox matters because flexbox only wins the argument on screen — it refuses to
 * shrink the pane, but `ratio` carries on falling as the divider is dragged, and *that* is what gets
 * saved. The divider would appear to stop while the stored number kept moving, and the next taller
 * window would open at a ratio nobody chose.
 */

/**
 * Clamp a divider position.
 *
 * @param {number} value the position asked for, as a share of the container
 * @param {object} bounds
 * @param {number} bounds.min lowest share the layout allows
 * @param {number} bounds.max highest share the layout allows
 * @param {number} [bounds.span] the container's size along the split axis, in pixels
 * @param {number} [bounds.floor] the first pane's minimum size, in pixels
 * @returns {number} a share, never below the floor the first pane needs
 */
export function clampRatio(value, { min, max, span = 0, floor = 0 }) {
  const low = Math.max(min, floorShare(span, floor));
  // `low` last, so it outranks `max`. They can only cross when the container is too small to
  // honour both, and in that case the pane's contents win — a divider parked past its ceiling is
  // a layout that looks wrong, where a pane under its floor is a passage with its edges cut off.
  return Math.max(low, Math.min(max, finite(value, min)));
}

/**
 * Pull a divider onto the centre when it is already nearly there.
 *
 * Even panes are the position anybody wants *back*, and landing on exactly `0.5` by hand is not
 * something a pointer can do. So the last few pixels are taken for them. Outside the zone the
 * value is returned untouched — this is a snap, not a magnet, and a divider deliberately parked
 * a third of the way along has to stay there.
 *
 * **The threshold is in pixels, and that is the point.** A share would make the zone physically
 * wider on a wide window and narrower on a narrow one, so the same gesture would behave
 * differently depending on the window — the opposite of what a snap is for. `floorShare` is
 * reused for the conversion rather than repeating `min(1, px / span)`: its name describes its
 * first caller rather than its arithmetic, and it already carries exactly the guards this needs.
 * An unmeasured container and a zero threshold both come back `0`, which reads here as *no
 * snapping* — the right answer to "how close is close?" when nothing can be measured.
 *
 * Callers must still clamp afterwards. The centre is not guaranteed to be a position the layout
 * allows, and `clampRatio` is what decides that.
 *
 * @param {number} value the position asked for, as a share of the container
 * @param {object} snap
 * @param {number} [snap.span] the container's size along the split axis, in pixels
 * @param {number} [snap.within] how near the centre, in pixels, counts as centred
 * @returns {number} `0.5` inside the zone, and `value` everywhere else
 */
export function snapToCentre(value, { span = 0, within = 0 }) {
  const zone = floorShare(span, within);
  if (zone <= 0) return value;
  return Math.abs(value - 0.5) <= zone ? 0.5 : value;
}

/**
 * A pixel floor as a share of the container.
 *
 * Guards every way this can be asked without an answer: a container that has not been laid out yet
 * reports zero, and `parseFloat` of a CSS `min-height` of `auto` or `none` is `NaN`. Both mean *no
 * floor is known*, which must read as no floor rather than as one of infinite size.
 *
 * @param {number} span the container's size along the split axis, in pixels
 * @param {number} floor the pane's minimum size, in pixels
 * @returns {number} a share in [0, 1]
 */
export function floorShare(span, floor) {
  if (!Number.isFinite(span) || !Number.isFinite(floor)) return 0;
  if (span <= 0 || floor <= 0) return 0;
  return Math.min(1, floor / span);
}

/**
 * Keep a number, or fall back.
 *
 * A pointer event over a container of zero width divides by zero, and `NaN` propagated into a
 * stored ratio poisons the layout on every subsequent load.
 *
 * @param {number} value
 * @param {number} fallback
 * @returns {number}
 */
function finite(value, fallback) {
  return Number.isFinite(value) ? value : fallback;
}
