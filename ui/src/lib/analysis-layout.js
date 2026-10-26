/**
 * The passage's content-fitted share of the Analysis workspace.
 *
 * The floor deliberately outranks the cap: a complete Japanese line is more important than
 * satisfying a percentage. Invalid measurements return zero so an element that has not been
 * laid out yet cannot publish NaN into the persisted divider state.
 *
 * @param {{ workspace: number, content: number, floor: number, cap?: number, max?: number }} input
 */
export function fitPassageRatio({ workspace, content, floor, cap = 0.4, max = 0.8 }) {
  if (!Number.isFinite(workspace) || workspace <= 0) return 0;
  const safeContent = Number.isFinite(content) ? Math.max(0, content) : 0;
  const safeFloor = Number.isFinite(floor) ? Math.max(0, floor) : 0;
  const safeCap = Number.isFinite(cap) ? Math.max(0, cap) : 0.4;
  const safeMax = Number.isFinite(max) ? Math.max(0, max) : 0.8;
  const desired = Math.max(safeFloor, Math.min(safeContent, workspace * safeCap));
  // The floor is a physical legibility constraint, not another preference to clamp. If a
  // very short workspace needs more than `max` to show one complete reader line, give it that
  // space (but never publish a ratio larger than the workspace itself).
  return Math.min(1, Math.max(safeFloor / workspace, Math.min(safeMax, desired / workspace)));
}

/**
 * The passage divider is a projection of Japanese reader geometry only.
 *
 * The object shape is deliberate: callers may hold other measured study surfaces, but adding a
 * Translation card must be incapable of changing what is handed to `fitPassageRatio`.
 */
/** @param {{ japanese: number }} input */
export function passageFitContent({ japanese }) {
  return Number.isFinite(japanese) ? Math.max(0, japanese) : 0;
}

/**
 * The lower panes stop sharing a row below forty-two usable rem.
 * @param {number} width
 * @param {number} remPixels
 */
export function isConstrainedStudy(width, remPixels) {
  return (
    Number.isFinite(width) &&
    Number.isFinite(remPixels) &&
    width > 0 &&
    remPixels > 0 &&
    width < 42 * remPixels
  );
}
