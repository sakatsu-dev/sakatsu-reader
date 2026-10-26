//! One publisher at a time, across every operation that writes the resource tree.
//!
//! # Why this is wider than "one install per component"
//!
//! The reservation this grew out of guarded **components**: the compatibility `fetch_component`
//! command and the persisted setup manager serve different screens, write the same files, and would
//! otherwise both verify and rename the same artifact. That was right and too narrow. Two more
//! operations write the same tree and took no reservation at all:
//!
//! - **relocation**, which copies every resource directory and then swaps the location;
//! - **the gloss backfill**, which publishes a new generation of every dictionary.
//!
//! Both were reproduced overlapping an install in `core/tests/resource_coordination.rs` before this
//! module existed. A move started and ran to activation while an install held the dictionaries, and
//! startup cleanup deleted an old copy with an install in progress. Neither destroyed bytes; what
//! they produced was two locations disagreeing with nothing on screen to say so, which is the
//! failure this repository has learned to treat as the worse one.
//!
//! # The rule, in three claims
//!
//! [`Claim::Everything`] is the whole tree and excludes every other claim in both directions.
//! [`Claim::Dictionaries`] is every dictionary container. [`Claim::Component`] is one catalog
//! component's files, and conflicts with `Dictionaries` **only when that component installs a
//! dictionary** — read from [`After::ImportDictionary`](crate::setup::After::ImportDictionary)
//! rather than from a second list that could disagree with it, so installing an OCR model while
//! English indexes are being built is still allowed.
//!
//! # It is not re-entrant, and nothing below the orchestrator claims
//!
//! An install that imports a dictionary calls `index_secondary` itself, under the component claim
//! it already holds. If the primitive took `Dictionaries`, that install would refuse itself. So a
//! claim is taken by the **orchestrator** — the command, or the worker it spawns — and never by the
//! operations underneath it.
//!
//! # Ownership comes before the paths, not after
//!
//! The first version of this had `relocate` and the backfill claim *inside themselves*, which read
//! as safe and was not: their callers had already resolved which directories to work on. That
//! leaves a real interleaving —
//!
//! ```text
//! resolve the old location → a move completes → take the claim → write to the abandoned location
//! ```
//!
//! — in which no two operations ever overlap while holding claims and the write still lands where
//! nothing will look for it again.
//!
//! ## What the witness does and does not establish
//!
//! The operations that write take a [`Claimed`] **witness**, checked with [`Claimed::covers`]. Be
//! exact about what that buys: it proves the caller **holds** the right claim at the moment of the
//! call. It says nothing about *when* the caller worked out its paths — a caller can still resolve
//! a directory, claim afterwards, and hand the witness in. The witness removes the possibility of
//! writing with no claim at all; it cannot see the order of what came before it.
//!
//! **The ordering comes from the shape of the call site**, not from the type: `sakatsu_desktop_lib
//! ::owning` takes the claim and then runs a closure, and every path is resolved *inside* that
//! closure. So the ordering is a convention with one enforcement point rather than a guarantee the
//! compiler makes, and it is checked the way conventions have to be — by asking the production
//! commands. `src-tauri/tests/writer_ownership.rs` does that; `core/tests/resource_coordination.rs`
//! pins what the wrong order costs. A larger type-system redesign would buy little more than those
//! two do together.

use std::sync::{Mutex, OnceLock, PoisonError};

use crate::setup::catalog;

/// What one operation is writing while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// One catalog component's files.
    Component(&'static str),
    /// Every dictionary container: the gloss backfill.
    Dictionaries,
    /// The whole resource tree: a relocation, and the cleanup that follows one.
    Everything,
}

/// What the reader is told when each kind of work is already under way.
///
/// Whole sentences, because these reach the setup panel unchanged. The component wording is the
/// one the two setup screens have always used and is kept exactly.
const MOVING: &str =
    "The resources are being moved to a new location. Wait for that to finish before starting \
     anything else.";
const INDEXING: &str =
    "The dictionaries are being indexed for English search. Wait for that to finish.";

fn installing(id: &str) -> String {
    format!("{id} is already being installed")
}

impl Claim {
    /// Whether this component's install writes into the dictionary containers.
    fn writes_dictionaries(id: &str) -> bool {
        catalog::all()
            .iter()
            .find(|component| component.id == id)
            .is_some_and(|component| matches!(component.after, catalog::After::ImportDictionary))
    }

    /// What somebody is told when `self` is already held and `incoming` asks to start.
    ///
    /// The message describes **what is running**, never what was refused: a reader who is told
    /// "the move was refused" learns nothing they can act on.
    fn refuses(&self, incoming: &Claim) -> Option<String> {
        match (self, incoming) {
            (Claim::Everything, _) => Some(MOVING.to_string()),
            (Claim::Dictionaries, Claim::Everything | Claim::Dictionaries) => {
                Some(INDEXING.to_string())
            }
            (Claim::Dictionaries, Claim::Component(id)) if Claim::writes_dictionaries(id) => {
                Some(INDEXING.to_string())
            }
            (Claim::Component(id), Claim::Everything) => Some(installing(id)),
            (Claim::Component(id), Claim::Dictionaries) if Claim::writes_dictionaries(id) => {
                Some(installing(id))
            }
            (Claim::Component(held), Claim::Component(wanted)) if held == wanted => {
                Some(installing(held))
            }
            _ => None,
        }
    }
}

static HELD: OnceLock<Mutex<Vec<Claim>>> = OnceLock::new();

fn held() -> &'static Mutex<Vec<Claim>> {
    HELD.get_or_init(Default::default)
}

/// Everything one operation holds, released together when it finishes or unwinds.
#[derive(Debug)]
pub struct Claimed {
    mine: Vec<Claim>,
}

impl Claimed {
    /// Whether this holds a claim that includes `wanted`.
    ///
    /// The witness the writing operations check. `Everything` covers anything; a component claim
    /// covers that component; `Dictionaries` covers itself and any component that imports
    /// dictionaries — the same relation [`Claim::refuses`] is written from, read the other way
    /// round, so the two cannot drift into disagreeing about what one claim includes.
    ///
    /// This answers "is the caller holding this?" and nothing about when the caller resolved its
    /// paths — see the module documentation on where the ordering actually comes from.
    pub fn covers(&self, wanted: Claim) -> bool {
        self.mine.iter().any(|held| match (held, wanted) {
            (Claim::Everything, _) => true,
            (Claim::Dictionaries, Claim::Dictionaries) => true,
            (Claim::Dictionaries, Claim::Component(id)) => Claim::writes_dictionaries(id),
            (Claim::Component(id), Claim::Dictionaries) => Claim::writes_dictionaries(id),
            (Claim::Component(held), Claim::Component(wanted)) => *held == wanted,
            _ => false,
        })
    }

    /// Refuse to go on without the ownership this operation was promised.
    ///
    /// A panic rather than an error: a caller reaching a resource writer without having claimed is
    /// a programming mistake in this repository, not a condition a reader can be told about, and
    /// the alternative — an error variant every caller would `unwrap` — buys nothing.
    pub fn require(&self, wanted: Claim) {
        assert!(
            self.covers(wanted),
            "this writes resources that {wanted:?} covers, and the caller holds {:?}. Claim before \
             resolving any path — see core::setup::claim.",
            self.mine
        );
    }
}

impl Drop for Claimed {
    fn drop(&mut self) {
        let mut held = held().lock().unwrap_or_else(PoisonError::into_inner);
        for claim in &self.mine {
            // One instance, not every match: two operations may legitimately hold the same
            // component claim only if the rules above allowed it, and removing all of them would
            // release somebody else's.
            if let Some(at) = held.iter().position(|existing| existing == claim) {
                held.remove(at);
            }
        }
    }
}

/// Take every claim or none, and say what is already running if it cannot.
pub fn claim(wanted: impl IntoIterator<Item = Claim>) -> Result<Claimed, String> {
    let mut mine: Vec<Claim> = wanted.into_iter().collect();
    mine.sort_by_key(|claim| match claim {
        Claim::Component(id) => (0, *id),
        Claim::Dictionaries => (1, ""),
        Claim::Everything => (2, ""),
    });
    mine.dedup();

    let mut held = held().lock().unwrap_or_else(PoisonError::into_inner);
    for existing in held.iter() {
        for incoming in &mine {
            if let Some(why) = existing.refuses(incoming) {
                return Err(why);
            }
        }
    }
    held.extend(mine.iter().copied());
    Ok(Claimed { mine })
}

/// Serialises the tests that take real claims.
///
/// The registry is process-global by design — that is what makes it a policy rather than a lock
/// somebody remembered to take — so two tests holding claims at once report each other's
/// reservations. Serialising says so rather than asking for `--test-threads=1`, which a test that
/// needs a flag to be correct will eventually be run without.
#[cfg(test)]
pub(crate) fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    static ORDER: Mutex<()> = Mutex::new(());
    ORDER.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_and_an_unrelated_install_still_exclude_each_other() {
        let _order = one_at_a_time();
        let moving = claim([Claim::Everything]).expect("nothing else is running");
        let refused = claim([Claim::Component("manga-ocr")]).expect_err("the tree is being moved");
        assert!(refused.contains("moved"), "{refused}");
        drop(moving);
        assert!(claim([Claim::Component("manga-ocr")]).is_ok());
    }

    /// The one place the rule is deliberately narrow: an OCR model is not a dictionary.
    #[test]
    fn indexing_blocks_a_dictionary_install_and_lets_a_model_through() {
        let _order = one_at_a_time();
        let indexing = claim([Claim::Dictionaries]).expect("nothing else is running");
        assert!(
            claim([Claim::Component("dictionaries")]).is_err(),
            "that component imports dictionaries, which is what is being indexed"
        );
        let model = claim([Claim::Component("manga-ocr")]);
        assert!(
            model.is_ok(),
            "an OCR model shares nothing with the gloss index, and blocking it would be a rule \
             nobody could explain"
        );
        drop(model);
        drop(indexing);
    }

    /// The witness says yes to what its claim includes and no to the rest.
    #[test]
    fn a_witness_covers_exactly_what_its_claim_excludes() {
        let _order = one_at_a_time();

        let everything = claim([Claim::Everything]).expect("nothing else is running");
        assert!(everything.covers(Claim::Dictionaries));
        assert!(everything.covers(Claim::Component("manga-ocr")));
        assert!(everything.covers(Claim::Everything));
        drop(everything);

        let dictionaries = claim([Claim::Dictionaries]).expect("nothing else is running");
        assert!(dictionaries.covers(Claim::Dictionaries));
        assert!(
            dictionaries.covers(Claim::Component("dictionaries")),
            "that component imports dictionaries, so this claim already excludes it"
        );
        assert!(
            !dictionaries.covers(Claim::Component("manga-ocr")),
            "and an OCR model shares nothing with it, which is why it is allowed to run alongside"
        );
        assert!(
            !dictionaries.covers(Claim::Everything),
            "indexing the dictionaries is not permission to move the whole tree"
        );
        drop(dictionaries);
    }

    #[test]
    fn a_partly_conflicting_set_takes_nothing() {
        let _order = one_at_a_time();
        let held = claim([Claim::Component("kanji")]).expect("nothing else is running");
        assert!(claim([Claim::Component("wordnet"), Claim::Component("kanji")]).is_err());
        drop(held);
        // If the refused call had kept `wordnet`, this would fail — which is the whole point of
        // checking the whole set before extending.
        assert!(claim([Claim::Component("wordnet")]).is_ok());
    }
}
