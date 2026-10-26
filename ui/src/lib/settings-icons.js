/**
 * The icon beside each section in the settings nav, as stroke paths in a 24-unit box.
 *
 * # Why a module of path data and not markup
 *
 * Seven icons drawn inline would put about forty lines of `<path>` into the nav's `{#each}`, and
 * the nav is the part of `Settings.svelte` you read to find out what the page *is*. Here they are
 * one entry per section id, the component draws every one the same way — `fill="none"`,
 * `currentColor`, a 1.9 stroke with round ends, the rail's drawing — and
 * `settings-notes.test.mjs` can check that every section has one.
 *
 * Kept apart from `SETTINGS_SECTIONS` in `model.ts` because that list's entries are parsed as
 * exact `{ id, label }` literals by `settings-index.test.mjs`; a third field would break it.
 *
 * # How they are drawn
 *
 * Paths only, no `<circle>` or `<rect>`, so one shape of data covers all seven. A dot is a
 * zero-length segment (`h.01`) that the round cap turns into a disc the width of the stroke.
 *
 * **The ink is centred on (12, 12), and that is measured, not eyeballed** —
 * `settings-layout.spec.ts` unions each icon's `getBBox()`, grows it by half the stroke, and
 * holds its centre to half a unit of the box's. `traps.md` records three rail icons that shipped
 * a unit off, and the gear that sat visibly left of its button.
 *
 * @type {Record<string, string[]>}
 */
export const SECTION_ICONS = {
  // Sliders: three tracks, each broken around its knob so the track does not show through it.
  reading: [
    "M4 7h3",
    "M11 7h9",
    "M9 5a2 2 0 1 0 0 4a2 2 0 1 0 0-4",
    "M4 12h9",
    "M17 12h3",
    "M15 10a2 2 0 1 0 0 4a2 2 0 1 0 0-4",
    "M4 17h4",
    "M12 17h8",
    "M10 15a2 2 0 1 0 0 4a2 2 0 1 0 0-4",
  ],
  // A painter's palette with its thumb notch on the right, and three dabs of paint.
  appearance: [
    "M12 4a8 8 0 1 0 0 16c1.1 0 2-.9 2-2c0-.5-.2-1-.5-1.3c-.3-.4-.5-.8-.5-1.3c0-1.1 .9-2 2-2h2.4a2.6 2.6 0 0 0 2.6-2.6C20 7.2 16.4 4 12 4z",
    "M8 11.5h.01",
    "M9.5 7.8h.01",
    "M13.8 7.3h.01",
  ],
  // A viewfinder: four corners, and a line of text inside them — what a capture is pointed at.
  capture: [
    "M4 9V6a2 2 0 0 1 2-2h3",
    "M15 4h3a2 2 0 0 1 2 2v3",
    "M20 15v3a2 2 0 0 1-2 2h-3",
    "M9 20H6a2 2 0 0 1-2-2v-3",
    "M8.5 10.5h7",
    "M8.5 13.5h4.5",
  ],
  // A magnifier. The lens sits up and left so the lens and handle together are centred.
  search: ["M10.25 4.25a6 6 0 1 0 0 12a6 6 0 1 0 0-12", "M14.5 14.5l5.25 5.25"],
  // An eye: recognition is the app looking at the screen.
  recognition: [
    "M3 12s3.5-6.5 9-6.5s9 6.5 9 6.5s-3.5 6.5-9 6.5S3 12 3 12z",
    "M12 9.5a2.5 2.5 0 1 0 0 5a2.5 2.5 0 1 0 0-5",
  ],
  // Two cards, one behind the other: a deck.
  anki: [
    "M7 7.5h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z",
    "M8 4.5h9a2 2 0 0 1 2 2v9",
  ],
  // A pulse trace: the app's vital signs.
  diagnostics: ["M3 12h4l2.5-6l5 12l2.5-6h4"],
  updates: ["M12 4v11", "M8 11l4 4 4-4", "M4 17v3h16v-3"],
};
