import assert from "node:assert/strict";
import test from "node:test";

import { copyName, exportTarget, manageableDecks, offerableDecks } from "./decks.js";

const deck = (name, ours, foreign, made_here = false) => ({ name, ours, foreign, made_here });
/** A deck this app made: it wears Sakatsu's options preset. */
const mine = (name, ours, foreign) => deck(name, ours, foreign, true);

test("a deck just created is still offered, though it has no cards yet", () => {
  // The bug this file exists for, in the shape it was found: one card had ever been exported, to
  // Default, and the learner had made a deck to export into. Ticking the filter hid the new deck
  // and left Default as the only choice — the Create button and the filter cancelling out. It is
  // the mark that saves it now, not the fact that it happens to be empty.
  const decks = ["All in One Kanji", "Custom Study Session", "Default", "Sakatsu-Deck", "Core 2k"];
  const info = [
    // Empty, but somebody else's. The old rule offered this, in a list labelled "hide decks that
    // aren't Sakatsu's".
    deck("All in One Kanji", 0, 0),
    deck("Custom Study Session", 0, 0),
    deck("Default", 1, 0),
    mine("Sakatsu-Deck", 0, 0),
    deck("Core 2k", 0, 4821),
  ];

  // `Default` holds a card from here and is still not offered: it is Anki's own deck.
  assert.deepEqual(offerableDecks(decks, info, true), ["Sakatsu-Deck"]);
});

test("a deck full of somebody else's cards is left out, even when it is the only one", () => {
  const decks = ["Core 2k"];
  const info = [deck("Core 2k", 0, 4821)];
  // There used to be a bypass here that handed the whole list back rather than offer nothing.
  // It meant the one install where nothing is ours — a fresh one — got every deck in the
  // collection offered under a switch promising the opposite.
  assert.deepEqual(offerableDecks(decks, info, true), []);
});

test("a deck holding both kinds of card is still offered", () => {
  const decks = ["Mine", "Theirs"];
  const info = [deck("Mine", 3, 200), deck("Theirs", 0, 90)];
  // Offering is not the same question as owning: adding a card to a mixed deck is harmless,
  // where renaming or deleting one is not.
  assert.deepEqual(offerableDecks(decks, info, true), ["Mine"]);
});

test("nothing of ours means nothing offered, which is the safe answer", () => {
  const decks = ["Theirs", "Also Theirs"];
  const info = [deck("Theirs", 0, 12), deck("Also Theirs", 0, 5)];
  // **The bug this rewrite is for.** Anki running, no Sakatsu decks, and the app offered — and
  // selected — somebody else's. Cards went into a deck the learner had been building for years.
  // Nowhere to send a card is recoverable by pressing Create; that is not.
  assert.deepEqual(offerableDecks(decks, info, true), []);
});

test("the list is untouched before the counts have ever been read", () => {
  const decks = ["Default", "Sakatsu-Deck"];
  assert.deepEqual(offerableDecks(decks, null, true), decks);
});

test("turning the filter off shows everything, counts or no counts", () => {
  const decks = ["Default", "Core 2k"];
  const info = [deck("Default", 1, 0), deck("Core 2k", 0, 4821)];
  assert.deepEqual(offerableDecks(decks, info, false), decks);
  assert.deepEqual(offerableDecks(decks, null, false), decks);
});

test("being the deck currently chosen does not make a foreign deck offerable", () => {
  const decks = ["Core 2k", "Default"];
  const info = [deck("Core 2k", 0, 4821), deck("Default", 1, 0)];
  // It used to be kept whatever it was, so one accidental selection was permanent: the deck
  // stayed in a list labelled "hide decks that aren't Sakatsu's" because it was selected, and
  // stayed selected because it was in the list.
  //
  // And `Default` is not rescued by holding a card from here either — see the test below.
  assert.deepEqual(offerableDecks(decks, info, true), []);
});

test("there is no export target until a deck of ours is chosen", () => {
  const offered = ["Sakatsu-Deck", "Default"];
  assert.equal(exportTarget("Sakatsu-Deck", offered), "Sakatsu-Deck");
  // Nothing chosen — a fresh install, now that the shipped default is no longer `Default`.
  assert.equal(exportTarget("", offered), null);
  assert.equal(exportTarget("   ", offered), null);
  // Chosen, but not something the filter allows: a deck renamed in Anki, or somebody else's.
  assert.equal(exportTarget("All in One Kanji", offered), null);
  // And with nothing offerable at all there is no target however the setting reads.
  assert.equal(exportTarget("Sakatsu-Deck", []), null);
});

test("the order Anki gave is kept", () => {
  const decks = ["Zeta", "Alpha", "Mid"];
  const info = [mine("Zeta", 0, 0), deck("Alpha", 2, 0), mine("Mid", 0, 0)];
  assert.deepEqual(offerableDecks(decks, info, true), ["Zeta", "Alpha", "Mid"]);
});

test("only decks this app made, holding nothing else, can be managed", () => {
  const decks = ["Sakatsu-Deck", "Default", "Custom Study Session", "Core 2k", "Shared"];
  const info = [
    mine("Sakatsu-Deck", 0, 0),
    // Holds one of our cards and nothing else — but Anki keeps Default whatever happens.
    mine("Default", 1, 0),
    // Empty and made by Anki itself. Content alone would have called this ours.
    deck("Custom Study Session", 0, 0),
    deck("Core 2k", 0, 4821),
    // Ours, and then filled with the learner's own cards. Making a deck does not make its
    // contents ours.
    mine("Shared", 3, 40),
  ];
  assert.deepEqual(manageableDecks(decks, info), ["Sakatsu-Deck"]);
});

test("every deck is offered until the counts have ever been read", () => {
  const decks = ["Sakatsu-Deck", "Core 2k"];
  // A control that is briefly empty on the way to being right reads as broken.
  assert.deepEqual(manageableDecks(decks, null), decks);
});

test("a deck the counts do not mention is not assumed safe", () => {
  // Arrived after the counts were taken. Silence is not evidence, and this list gates a delete.
  const decks = ["Known", "Brand New"];
  assert.deepEqual(manageableDecks(decks, [mine("Known", 2, 0)]), ["Known"]);
});

test("a copy is named for its source and climbs until the name is free", () => {
  assert.equal(copyName(["Japanese"], "Japanese"), "Japanese copy");
  assert.equal(copyName(["Japanese", "Japanese copy"], "Japanese"), "Japanese copy 2");
  assert.equal(
    copyName(["Japanese", "Japanese copy", "Japanese copy 2"], "Japanese"),
    "Japanese copy 3",
  );
});

test("a copy of a subdeck stays under the same parent", () => {
  // `Japanese copy::Vocab` would silently start a second tree at the top level, which is not
  // what duplicating a child deck means.
  assert.equal(copyName(["Japanese::Vocab"], "Japanese::Vocab"), "Japanese::Vocab copy");
});

test("a copy name is free even when the obvious one is taken by something unrelated", () => {
  // Anki refuses a name already in use, so the check is against every deck rather than against
  // the source's own subtree.
  const existing = ["A", "A copy", "A copy 2", "B"];
  assert.equal(copyName(existing, "A"), "A copy 3");
});

/**
 * **Not knowing and knowing there are none are different answers.**
 *
 * They were the same value — an array that started empty — and that is what put the foreign decks
 * back after the fix that removed them. The counts are the expensive call, two or three requests
 * per deck, so it is the one that fails; the failure path reset them to `[]`, which read as "never
 * asked", and every deck in the collection reappeared under the switch that hides them.
 *
 * `null` opens the list. `[]` closes it. Nothing else may.
 */
test("an empty count list means none qualify, not that none are known", () => {
  const decks = ["All in One Kanji", "Core 2k", "Default"];

  // Never read: nothing can be judged, so nothing is hidden.
  assert.deepEqual(offerableDecks(decks, null, true), decks);
  assert.deepEqual(offerableDecks(decks, undefined, true), decks);

  // Read, and nothing in it is ours. That is an answer, and it is "none".
  assert.deepEqual(offerableDecks(decks, [], true), []);
});

test("the manage list does not open up because a request failed", () => {
  const decks = ["All in One Kanji", "Core 2k", "Default"];
  // Rename and Delete act on these. Offering somebody's twelve-year-old kanji deck to them
  // because one request came back empty is the wrong direction, even though Rust re-checks.
  assert.deepEqual(manageableDecks(decks, []), []);
  assert.deepEqual(manageableDecks(decks, null), decks, "but not knowing still shows them");
});

test("stale counts still hide foreign decks", () => {
  // What the interface now keeps when a refresh fails: the previous answer. It was true a moment
  // ago, and holding it is what stops the list flapping open on a failed request.
  const decks = ["All in One Kanji", "Japanese::Reading"];
  const stale = [
    { name: "All in One Kanji", ours: 0, foreign: 3521, made_here: false },
    { name: "Japanese::Reading", ours: 0, foreign: 0, made_here: true },
  ];
  assert.deepEqual(offerableDecks(decks, stale, true), ["Japanese::Reading"]);
});

/**
 * **Anki's own deck is never an export target, whatever is in it.**
 *
 * `Default` exists in every collection, cannot be deleted, and is where cards land when nothing
 * else is chosen. Being the place things end up by accident is what makes it a poor place to put
 * them on purpose — and it renewed its own accident: one card exported there gave it `ours > 0`,
 * which qualified it forever under the rule that offers decks already holding our cards.
 *
 * `manageableDecks` had excluded it by name from the start. This is the two rules agreeing.
 */
test("Default is never offered, even holding a hundred of our cards", () => {
  const decks = ["Default", "Sakatsu-Deck"];
  const info = [deck("Default", 100, 0), mine("Sakatsu-Deck", 0, 0)];
  assert.deepEqual(offerableDecks(decks, info, true), ["Sakatsu-Deck"]);

  // Not even if somebody put Sakatsu's options preset on it in Anki.
  assert.deepEqual(offerableDecks(["Default"], [mine("Default", 5, 0)], true), []);

  // And it is not a target, so the export button stays off while it is what is saved.
  assert.equal(exportTarget("Default", offerableDecks(decks, info, true)), null);
});

test("turning the filter off still offers Default, because that is what off means", () => {
  const decks = ["Default", "Core 2k"];
  const info = [deck("Default", 1, 0), deck("Core 2k", 0, 4821)];
  // The switch says it hides decks that are not Sakatsu's. Off, it hides nothing — somebody who
  // deliberately files into Anki's own deck has said so.
  assert.deepEqual(offerableDecks(decks, info, false), decks);
});
