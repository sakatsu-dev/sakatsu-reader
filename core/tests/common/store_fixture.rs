//! **A small, deterministic word store**, so equivalence covers status targets and known-word
//! furigana rather than a store where every word is `New`.
//!
//! Literal keys, not keys derived from an analysis: a fixture computed from the code under test
//! would move with it. `coverage` then reports which keys the analysed passages actually reach, so
//! a fixture that silently matches nothing is caught instead of passing vacuously.

use sakatsu_core::store::{Store, WordKey, WordStatus};

/// `(term, reading, status, why)`. The term and reading are what `WordKey::for_token` builds:
/// the dictionary form and the lemma reading.
pub const MARKED: &[(&str, &str, WordStatus, &str)] = &[
    (
        "間違う",
        "まちがう",
        WordStatus::Known,
        "a Known whole word (the repaired 間違っとる) whose とる part stays New",
    ),
    (
        "申し分",
        "もうしぶん",
        WordStatus::Known,
        "a Known part inside a New whole (申し分ない)",
    ),
    (
        "なきゃ",
        "なきゃ",
        WordStatus::Seen,
        "a marked part of the repaired 怒んなきゃ, whose whole stays New",
    ),
];

/// The passages the fixture is about. All already in the repository: the contraction fixtures and
/// the grouping checklist's verified passage.
pub const WORKLOAD: &[(&str, &str)] = &[
    ("store-machigattoru", "頭文字から間違っとるわい！！"),
    ("store-okonnakya", "怒んなきゃ"),
    ("store-moushibun", "味は申し分ない。"),
];

/// Open a store in `dir` and mark exactly [`MARKED`].
pub fn populated(dir: &std::path::Path) -> Store {
    let store = Store::open(dir).expect("open the fixture store");
    for (term, reading, status, _) in MARKED {
        store
            .set_status(WordKey::new(*term, *reading), *status)
            .expect("mark a fixture word");
    }
    store
}

/// A digest of [`MARKED`], for the artifact's identity block.
pub fn digest() -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for (term, reading, status, _) in MARKED {
        hasher.update(format!("{term}\u{1f}{reading}\u{1f}{status:?}\u{1e}").as_bytes());
    }
    sakatsu_core::hashing::hex(hasher.finalize())
}
