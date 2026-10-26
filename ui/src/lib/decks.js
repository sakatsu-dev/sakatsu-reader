/**
 * Pure rules about the Anki deck list.
 *
 * Out here rather than inline in `Settings.svelte` because both of these are the kind of rule
 * that is wrong silently — a deck that cannot be found, or a copy that collides with a deck
 * already there — and neither is visible in a screenshot.
 */

/**
 * Anki's own deck. It is in every collection, cannot be deleted, and is where cards go when
 * nothing else is chosen — so neither offering it as a target nor letting the manage buttons
 * touch it is right. Named once because two rules depend on the exact string.
 */
const DEFAULT_DECK = "Default";

/**
 * Narrow a deck list to the ones worth offering as an export target.
 *
 * A deck is offered when it was **made here, or already holds cards from here** — and is not
 * `Default`:
 *
 * - `made_here` — it wears Sakatsu's options preset, so this app created it. This covers a deck
 *   made a moment ago with the Create button, which has no cards yet.
 * - `ours > 0` — including decks that also hold the learner's own cards. Mixed decks belong in
 *   this list: adding a card to one is harmless, and somebody who keeps their 屠つ exports inside
 *   an existing deck should not have to turn the filter off to reach it. This is deliberately
 *   *not* the same question as whether a deck may be renamed or deleted, which additionally
 *   requires that nothing in it came from anywhere else.
 * - **`Default` never qualifies, whatever it holds.** It is Anki's own deck: it exists in every
 *   collection, cannot be removed, and is where cards land when nothing else is chosen. Being
 *   the place things end up by accident is exactly what makes it a poor place to put things on
 *   purpose — and once a card had been exported there once, `ours > 0` made it a permanent,
 *   legitimate-looking target, which is how the accident renewed itself. `manageableDecks` has
 *   excluded it by name from the start, for the same reason; this is the two rules agreeing.
 *
 *   With the filter off it is offered like anything else. A switch that says it hides decks that
 *   are not Sakatsu's has an obvious answer for Anki's own.
 *
 * **The rule used to offer any empty deck**, as a stand-in for "probably one you just made" —
 * which was the best available answer before decks carried a mark, and a poor one: it also
 * offered every empty deck in the collection, so a shared deck somebody had emptied sat in a list
 * labelled "hide decks that aren't Sakatsu's". `made_here` is the signal that guess was reaching
 * for, so the guess is gone.
 *
 * # An empty answer is a real answer
 *
 * Two escapes used to keep this list from ever being empty, and **both of them sent cards into
 * other people's decks**:
 *
 * - *the current choice was kept whatever it was*, so a foreign deck that got selected once
 *   stayed selected and stayed offered — under a switch that says it hides decks that are not
 *   Sakatsu's;
 * - *the filter yielded entirely if it would leave nothing*, handing back the whole collection on
 *   exactly the install where nothing is ours, which is a fresh one.
 *
 * The reasoning was that a filter leaving nothing selectable locks the learner out. It does — and
 * that is the safe direction. Nothing selectable means no card can be filed anywhere by accident,
 * the Create button is directly below, and the filter switch is right there to turn off. Spoiling
 * a deck somebody has been building for years is not recoverable by pressing anything.
 *
 * The one bypass that stays is *not knowing yet*: before the counts arrive nothing can be judged,
 * and briefly showing "none" while the answer is still loading reads as broken.
 *
 * # `null` is not knowing; `[]` is knowing that none qualify
 *
 * **These were the same value once, and that put the foreign decks back.** `info` was an array
 * that started empty, so "I have not asked" and "I asked, and nothing here is ours" were
 * indistinguishable — and the loading bypass then handed back the whole collection for the
 * second one. The counts are the expensive call, two or three requests a deck, so it is the one
 * that fails; the failure path set the list back to empty and every deck in Anki reappeared under
 * a switch promising the opposite.
 *
 * So absence of evidence is spelled `null`, and it is the only thing that opens the list up.
 *
 * @param {string[]} decks every deck Anki reports, in its order
 * @param {{name: string, ours: number, foreign: number, made_here: boolean}[] | null} info the
 *   counts and marks — `null` while they have never been read
 * @param {boolean} onlyOurs whether the learner asked to narrow the list
 * @returns {string[]} the decks to show — possibly none
 */
export function offerableDecks(decks, info, onlyOurs) {
  if (!onlyOurs) return decks;
  // Never read. Nothing can be judged, and "none of these are yours" is a claim this cannot make
  // yet. An empty array is a different answer entirely and is left to the filter below.
  if (info === null || info === undefined) return decks;

  const counts = new Map(info.map((deck) => [deck.name, deck]));
  return decks.filter((name) => {
    if (name === DEFAULT_DECK) return false;
    const deck = counts.get(name);
    // Silence is not evidence. A deck the counts do not mention has not been shown to be ours.
    if (!deck) return false;
    return deck.made_here || deck.ours > 0;
  });
}

/**
 * Where a card would actually go, or `null` when there is nowhere safe to put it.
 *
 * **The question the export button should be asking**, and it was not asking it: it checked that
 * Anki was answering, which says nothing about whether the deck now selected is one this app
 * should be writing to. A saved deck that no longer exists, or that the filter excludes, is not a
 * target — and a card sent to the wrong deck is somebody else's collection spoiled.
 *
 * Returning `null` rather than falling back to the first deck is the whole point. The interface
 * had a fallback (`ankiDecks[0]`) and it is how a collection whose first deck is alphabetically
 * `All in One Kanji` ends up receiving 屠つ vocabulary.
 *
 * @param {string} chosen the deck in the settings, possibly stale or empty
 * @param {string[]} offered what {@link offerableDecks} allows right now
 * @returns {string | null}
 */
export function exportTarget(chosen, offered) {
  const name = (chosen ?? "").trim();
  if (!name) return null;
  return offered.includes(name) ? name : null;
}

/**
 * The decks the manage controls may act on.
 *
 * A deck qualifies when **this app made it and nothing else has been filed in it** — the same
 * pair that gates renaming and deleting in Rust, so every deck offered here has working buttons
 * rather than four disabled ones.
 *
 * "Made here" is not guesswork: `create_deck` puts Sakatsu's own options preset on every deck it
 * makes, and `made_here` reports whether the deck still wears it. That mark lives in Anki, so it
 * survives renaming the deck, reinstalling this app, and an AnkiWeb sync — none of which a list
 * of names we remembered creating would survive. Changing the preset in Anki is how a learner
 * takes a deck back.
 *
 * `Default` never qualifies, whatever it holds — see {@link offerableDecks}, which excludes it
 * for the same reason.
 *
 * Before the counts have ever been read `info` is `null` and every deck is offered, because a
 * control that is briefly empty on the way to being right reads as broken. **An empty array is
 * not that** — it means the counts were read and nothing qualifies, and offering every deck in
 * the collection to a row of Rename and Delete buttons on the strength of a failed request is
 * exactly the wrong direction. Rust re-checks before acting, so the cost is a refusal rather than
 * a loss, but the list should not be inviting it.
 *
 * @param {string[]} decks every deck Anki reports, in its order
 * @param {{name: string, ours: number, foreign: number, made_here: boolean}[] | null} info the
 *   counts and marks — `null` while they have never been read
 * @returns {string[]} the decks worth offering
 */
export function manageableDecks(decks, info) {
  if (info === null || info === undefined) return decks;
  const known = new Map(info.map((deck) => [deck.name, deck]));
  return decks.filter((name) => {
    const deck = known.get(name);
    // Silence is not evidence — a deck the counts do not mention has not been cleared, and this
    // list gates a delete.
    if (!deck) return false;
    return deck.made_here && deck.foreign === 0 && name !== DEFAULT_DECK;
  });
}

/**
 * A free name for a copy of `source`.
 *
 * Duplicating does not ask for a name, because being made to invent one is the whole friction the
 * button exists to remove — and the honest answer is nearly always "the same, but a copy". Anki
 * refuses a deck name that is already taken, so the number climbs until one is free rather than
 * letting a second press fail with a message about something the learner did not type.
 *
 * `::` nests decks in Anki, so the suffix goes on the whole name: a copy of `Japanese::Vocab` is
 * `Japanese::Vocab copy`, which stays a child of `Japanese` where `Japanese copy::Vocab` would
 * silently start a new tree.
 *
 * @param {string[]} existing every deck name Anki already has
 * @param {string} source the deck being copied
 * @returns {string} a name no deck currently holds
 */
export function copyName(existing, source) {
  const taken = new Set(existing);
  let candidate = `${source} copy`;
  for (let n = 2; taken.has(candidate); n += 1) {
    candidate = `${source} copy ${n}`;
  }
  return candidate;
}
