//! **Contracted verb forms, derived forward from a dictionary record — never recognised by their
//! ending.**
//!
//! Two families, each one typed rewrite over a form `grammar::formation` already builds:
//!
//! | family | built from | rewrite | e.g. |
//! |---|---|---|---|
//! | `～ておる` | the te-form (`Conjunctive`) | `て+お → と`, `で+お → ど` | 間違って + おる → 間違っとる |
//! | negative conditional | the negative provisional | `なければ → なきゃ / なけりゃ` | 怒らなければ → 怒らなきゃ |
//! | … nasalised | the same, for an **r-stem** only | `ら → ん` before `な` | 怒らなきゃ → 怒んなきゃ |
//!
//! # A complete account of the span, or nothing
//!
//! A shared ending proves nothing: `～とる` ends 取る as well as every contracted `～ておる`, and
//! `～んなきゃ` would match any string ending that way. So a derivation succeeds only if it spells
//! the **whole** source span, as three contiguous source slices:
//!
//! - **the stem** — spelled by a form the entry records: its headword, or its reading when the
//!   source is kana. The evidence for which is kept ([`StemEvidence`]);
//! - **the inflection** — what the forward derivation puts between the stem and the contraction;
//! - **the contracted tail** — `とる`, `どる`, `なきゃ`, `なけりゃ`.
//!
//! The derived **reading** is the reading of that whole span, built from the entry's own reading.
//! An okurigana variant the entry does not record, or a stem that is not one of its spellings, does
//! not derive, and the caller abstains.
//!
//! # Pieces are source slices
//!
//! The expanded grammar (`間違っておる`, `怒らなければ`) is what the derivation *means*, and it goes
//! into the conjugation and the explanation. What a reader can select are the two slices the source
//! actually spells: `間違っ | とる`, `怒ん | なきゃ`. No piece names text that is not there.

use crate::grammar::formation::{contractions, formation};
use crate::grammar::Form;
use crate::kana::to_hiragana;
use crate::model::{ContractionFamily, StemEvidence};

/// A byte range of the derived span, relative to its start, with the reading it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub start: usize,
    pub end: usize,
    pub reading: String,
}

/// A contracted form, derived and accounted for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derived {
    pub family: ContractionFamily,
    /// `ら → ん` was applied.
    pub nasal: bool,
    /// The reading of the whole span, in hiragana.
    pub reading: String,
    /// The uncontracted form this stands for, in hiragana — for the explanation, never a piece.
    pub expanded: String,
    pub stem: StemEvidence,
    /// The verb with its inflection, then the contracted tail: two slices that tile the span.
    pub pieces: [Piece; 2],
}

/// The tails a derivation can end in. A token ending in one of these is worth investigating; the
/// ending alone never authorises anything.
pub const TAILS: &[&str] = &["とる", "どる", "なきゃ", "なけりゃ"];

/// Every way `surface` derives from the verb recorded as `spelling` / `reading` of class `class`.
///
/// `class` is a verb class code (`v1`, `v5`, `v5r`, …); anything else derives nothing. Both of the
/// entry's recorded spellings may be tried by the caller — the headword for a kanji source, the
/// reading for a kana one — and `stem` says which this call was.
pub fn derive(
    surface: &str,
    spelling: &str,
    reading: &str,
    class: &str,
    stem: StemEvidence,
) -> Vec<Derived> {
    let class = class.trim().to_ascii_lowercase();
    if !(class.starts_with("v5") || class.starts_with("v1")) {
        return Vec::new();
    }
    let reading = to_hiragana(reading.trim());
    let spelling = spelling.trim();
    // The okurigana the dictionary form ends in: one kana for a godan verb, る for an ichidan one.
    // The spelling must end in exactly that kana, or its stem cannot be told from its inflection.
    let Some(last) = reading.chars().last() else {
        return Vec::new();
    };
    let (Some(spelling_stem), Some(reading_stem)) =
        (spelling.strip_suffix(last), reading.strip_suffix(last))
    else {
        return Vec::new();
    };
    if spelling_stem.is_empty() || !surface.starts_with(spelling_stem) {
        return Vec::new();
    }

    let mut found = Vec::new();
    // What the source spells after the stem must be exactly the derived kana after the reading's
    // stem — the same kana, since everything past the stem is okurigana.
    let mut accept =
        |family: ContractionFamily, nasal: bool, derived: String, expanded: String, tail: &str| {
            let Some(after_stem) = derived.strip_prefix(reading_stem) else {
                return;
            };
            if format!("{spelling_stem}{after_stem}") != surface {
                return;
            }
            let Some(inflected) = after_stem.strip_suffix(tail) else {
                return;
            };
            let split = spelling_stem.len() + inflected.len();
            found.push(Derived {
                family,
                nasal,
                expanded,
                stem,
                pieces: [
                    Piece {
                        start: 0,
                        end: split,
                        reading: format!("{reading_stem}{inflected}"),
                    },
                    Piece {
                        start: split,
                        end: surface.len(),
                        reading: tail.to_string(),
                    },
                ],
                reading: derived,
            });
        };

    // The two bases, each built forward by `formation`, then contracted by the one shared table —
    // the same table the ladder uses to explain the word, so what is accepted here is exactly what
    // is shown there.
    for (form, neg) in [(Form::Conjunctive, false), (Form::Provisional, true)] {
        let Some(built) = formation(&reading, &class, form, neg, false) else {
            continue;
        };
        for contracted in contractions(&built.result, form, neg, &class) {
            accept(
                contracted.family,
                contracted.nasal,
                contracted.formation.result,
                contracted.formation.from,
                contracted.tail,
            );
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slices<'a>(surface: &'a str, derived: &Derived) -> [&'a str; 2] {
        [
            &surface[derived.pieces[0].start..derived.pieces[0].end],
            &surface[derived.pieces[1].start..derived.pieces[1].end],
        ]
    }

    #[test]
    fn a_contracted_te_oru_is_derived_with_its_source_slices() {
        let surface = "間違っとる";
        let found = derive(surface, "間違う", "まちがう", "v5", StemEvidence::Headword);
        assert_eq!(found.len(), 1, "{found:?}");
        let derived = &found[0];
        assert_eq!(derived.family, ContractionFamily::TeOru);
        assert_eq!(derived.reading, "まちがっとる");
        assert_eq!(
            derived.expanded, "まちがっておる",
            "the expansion is for the explanation"
        );
        assert_eq!(
            slices(surface, derived),
            ["間違っ", "とる"],
            "pieces are what the text spells"
        );
        assert_eq!(derived.pieces[0].reading, "まちがっ");
    }

    #[test]
    fn a_nasalised_negative_conditional_is_derived_for_an_r_stem() {
        let surface = "怒んなきゃ";
        let found = derive(surface, "怒る", "おこる", "v5", StemEvidence::Headword);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].nasal);
        assert_eq!(found[0].reading, "おこんなきゃ");
        assert_eq!(found[0].expanded, "おこらなければ");
        assert_eq!(slices(surface, &found[0]), ["怒ん", "なきゃ"]);
        // And the un-nasalised form derives from the same record, as itself.
        let plain = derive("怒らなきゃ", "怒る", "おこる", "v5", StemEvidence::Headword);
        assert_eq!(plain.len(), 1);
        assert!(!plain[0].nasal);
    }

    /// The nasalisation is the godan r-row's alone. An ichidan verb has no ら to nasalise.
    #[test]
    fn an_ichidan_verb_takes_the_contraction_but_not_the_nasalisation() {
        assert_eq!(
            derive(
                "食べなきゃ",
                "食べる",
                "たべる",
                "v1",
                StemEvidence::Headword
            )
            .len(),
            1
        );
        assert!(derive(
            "食べんなきゃ",
            "食べる",
            "たべる",
            "v1",
            StemEvidence::Headword
        )
        .is_empty());
        let te = derive("食べとる", "食べる", "たべる", "v1", StemEvidence::Headword);
        assert_eq!(te.len(), 1);
        assert_eq!(te[0].reading, "たべとる");
    }

    #[test]
    fn a_voiced_te_form_contracts_to_doru() {
        let found = derive("読んどる", "読む", "よむ", "v5", StemEvidence::Headword);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].reading, "よんどる");
        assert_eq!(slices("読んどる", &found[0]), ["読ん", "どる"]);
    }

    #[test]
    fn a_kana_source_derives_from_the_recorded_reading() {
        let found = derive(
            "かかっとる",
            "かかる",
            "かかる",
            "v5",
            StemEvidence::Reading,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].stem, StemEvidence::Reading);
        assert_eq!(slices("かかっとる", &found[0]), ["かかっ", "とる"]);
    }

    /// **A shared ending is not a derivation.** The stem must be one of the entry's spellings.
    #[test]
    fn a_shared_ending_alone_derives_nothing() {
        // The source spells only part of the stem.
        assert!(derive(
            "違っとる",
            "間違う",
            "まちがう",
            "v5",
            StemEvidence::Headword
        )
        .is_empty());
        // The source's stem is a different word's.
        assert!(derive("回っとる", "する", "する", "vs-i", StemEvidence::Headword).is_empty());
        assert!(derive(
            "して回っとる",
            "為る",
            "する",
            "vs-i",
            StemEvidence::Headword
        )
        .is_empty());
        // A plain verb ending in とる is not a contraction of anything.
        assert!(derive("取る", "取る", "とる", "v5", StemEvidence::Headword).is_empty());
    }

    /// An okurigana variant the entry does not record is not guessed at.
    #[test]
    fn an_unrecorded_okurigana_variant_does_not_derive() {
        assert!(derive(
            "間違っとる",
            "間違る",
            "まちがう",
            "v5",
            StemEvidence::Headword
        )
        .is_empty());
        assert!(derive(
            "間違っとる",
            "間違う",
            "まちがう",
            "n",
            StemEvidence::Headword
        )
        .is_empty());
    }
}
