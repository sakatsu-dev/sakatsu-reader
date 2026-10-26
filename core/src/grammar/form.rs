//! The form classifier: Ichiran's prose form label → a stable form identity.
//!
//! Ichiran names forms in prose (`"Past (~ta)"`, `"Causative-Passive"`), and the
//! exact wording shifts between versions. So classification is by **keyword, never
//! equality** (`project notes` §5), and the order of the tests is load-bearing:
//!
//! - `"Causative-Passive"` contains both *causative* and *passive*, so it must be
//!   tested before either — otherwise a causative-passive is reported as a plain
//!   causative and the sentence's meaning changes.
//! - `"Non-past"` contains *past*, so it must be tested before the past.
//! - A contraction's label names the form it contracts (`"Contracted ～なければ (～なきゃ)"`), so
//!   *contracted* is tested before everything.
//!
//! The labels this was built against are the full set Ichiran emits, collected by
//! running it over one word per form: Alternative (~tari), Causative,
//! Causative-Passive, Conditional (~tara), Conjunctive (~te), Continuative (~i),
//! Imperative, Non-past, Passive, Past (~ta), Potential, Provisional (~eba),
//! Volitional — plus "Adjective Stem", which arrives on nouns Ichiran reads as
//! adjective stems.

/// A grammatical form, independent of any analyzer's wording.
///
/// [`Form::Unclassified`] is deliberate: an unknown label must not be silently
/// dropped or guessed at. Callers still hold the raw label and can show that.
///
/// Serializes as its [`id`](Form::id) — `"causative-passive"`, not `"CausativePassive"`
/// — so the UI and any grammar pack key off one spelling. A test keeps the two in
/// lockstep.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Form {
    CausativePassive,
    Causative,
    Passive,
    Potential,
    Desiderative,
    Volitional,
    Imperative,
    Provisional,
    Conditional,
    Alternative,
    Conjunctive,
    Continuative,
    Adverbial,
    Past,
    NonPast,
    AdjectiveStem,
    /// A casual or dialect contraction of the form below it on the ladder — ～ておる as ～とる,
    /// ～なければ as ～なきゃ. Never an analyzer's label: `lexical::repair` records it on a word it
    /// rejoined, as the rung after the form it contracts.
    Contraction,
    Unclassified,
}

impl Form {
    /// Classify a form label by keyword.
    pub fn classify(label: &str) -> Form {
        let label = label.to_ascii_lowercase();
        let has = |needle: &str| label.contains(needle);

        // Order matters — see the module docs.
        if has("contracted") {
            Form::Contraction
        } else if has("causative") && has("passive") {
            Form::CausativePassive
        } else if has("causative") {
            Form::Causative
        } else if has("passive") {
            Form::Passive
        } else if has("potential") {
            Form::Potential
        } else if has("desiderative") || has("tai") {
            Form::Desiderative
        } else if has("volitional") {
            Form::Volitional
        } else if has("imperative") {
            Form::Imperative
        } else if has("provisional") || has("eba") {
            Form::Provisional
        } else if has("conditional") || has("tara") {
            Form::Conditional
        } else if has("alternative") || has("tari") {
            Form::Alternative
        } else if has("conjunctive") {
            Form::Conjunctive
        } else if has("continuative") {
            Form::Continuative
        } else if has("adverbial") {
            Form::Adverbial
        } else if has("adjective stem") {
            Form::AdjectiveStem
        } else if has("non-past") || has("nonpast") {
            Form::NonPast
        } else if has("past") {
            Form::Past
        } else {
            Form::Unclassified
        }
    }

    /// A stable identifier, for grammar-pack lookups and UI keys. Never changes,
    /// even if Ichiran renames its labels.
    pub fn id(self) -> &'static str {
        match self {
            Form::CausativePassive => "causative-passive",
            Form::Causative => "causative",
            Form::Passive => "passive",
            Form::Potential => "potential",
            Form::Desiderative => "desiderative",
            Form::Volitional => "volitional",
            Form::Imperative => "imperative",
            Form::Provisional => "provisional",
            Form::Conditional => "conditional",
            Form::Alternative => "alternative",
            Form::Conjunctive => "conjunctive",
            Form::Continuative => "continuative",
            Form::Adverbial => "adverbial",
            Form::Past => "past",
            Form::NonPast => "non-past",
            Form::AdjectiveStem => "adjective-stem",
            Form::Contraction => "contraction",
            Form::Unclassified => "unclassified",
        }
    }

    /// The heading a learner sees.
    pub fn title(self) -> &'static str {
        match self {
            Form::CausativePassive => "Causative-passive",
            Form::Causative => "Causative",
            Form::Passive => "Passive",
            Form::Potential => "Potential",
            Form::Desiderative => "Desiderative (〜たい)",
            Form::Volitional => "Volitional",
            Form::Imperative => "Imperative",
            Form::Provisional => "Provisional (〜ば)",
            Form::Conditional => "Conditional (〜たら)",
            Form::Alternative => "Alternative (〜たり)",
            Form::Conjunctive => "Te-form (〜て)",
            Form::Continuative => "Continuative stem",
            Form::Adverbial => "Adverbial",
            Form::Past => "Past",
            Form::NonPast => "Non-past",
            Form::AdjectiveStem => "Adjective stem",
            Form::Contraction => "Contracted form",
            Form::Unclassified => "Other form",
        }
    }

    /// One line of what the form does to the meaning. Written fresh for 屠つ:
    /// Imabi and Tae Kim are all-rights-reserved and may only be linked
    /// (`project notes` §8).
    pub fn meaning(self) -> &'static str {
        match self {
            Form::CausativePassive => "was made to do it by someone else",
            Form::Causative => "made or let someone else do it",
            Form::Passive => "had it done to them",
            Form::Potential => "can do it",
            Form::Desiderative => "wants to do it",
            Form::Volitional => "let's do it, or intends to",
            Form::Imperative => "a blunt command to do it",
            Form::Provisional => "if or when this happens",
            Form::Conditional => "if it happens, or once it has",
            Form::Alternative => "doing this among other things",
            Form::Conjunctive => "joins on to whatever follows",
            Form::Continuative => "the stem, used to join or to nominalize",
            Form::Adverbial => "used as an adverb",
            Form::Past => "already happened",
            Form::NonPast => "happens now, or will happen",
            Form::AdjectiveStem => "the stem an adjective is built on",
            Form::Contraction => "the same form, said casually or in dialect",
            Form::Unclassified => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn causative_passive_beats_causative_and_passive() {
        // The ordering trap: this label contains both other keywords, and getting
        // it wrong changes who did what to whom.
        assert_eq!(Form::classify("Causative-Passive"), Form::CausativePassive);
        assert_eq!(Form::classify("Causative"), Form::Causative);
        assert_eq!(Form::classify("Passive"), Form::Passive);
    }

    #[test]
    fn non_past_beats_past() {
        // "Non-past" contains "past".
        assert_eq!(Form::classify("Non-past"), Form::NonPast);
        assert_eq!(Form::classify("Past (~ta)"), Form::Past);
    }

    #[test]
    fn every_label_ichiran_actually_emits_is_classified() {
        // Collected by running ichiran-cli over one word per form; if Ichiran adds
        // a label, this list is what to extend.
        let labels = [
            ("Alternative (~tari)", Form::Alternative),
            ("Causative", Form::Causative),
            ("Causative-Passive", Form::CausativePassive),
            ("Conditional (~tara)", Form::Conditional),
            ("Conjunctive (~te)", Form::Conjunctive),
            ("Continuative (~i)", Form::Continuative),
            ("Imperative", Form::Imperative),
            ("Non-past", Form::NonPast),
            ("Passive", Form::Passive),
            ("Past (~ta)", Form::Past),
            ("Potential", Form::Potential),
            ("Provisional (~eba)", Form::Provisional),
            ("Volitional", Form::Volitional),
            ("Adjective Stem", Form::AdjectiveStem),
        ];
        for (label, expected) in labels {
            assert_eq!(Form::classify(label), expected, "label {label:?}");
            assert_ne!(expected, Form::Unclassified);
        }
    }

    #[test]
    fn a_form_serializes_as_its_id() {
        // The UI keys off these strings, and grammar packs match on them, so the
        // wire spelling and `id()` must not drift apart.
        for form in [
            Form::CausativePassive,
            Form::NonPast,
            Form::AdjectiveStem,
            Form::Conjunctive,
            Form::Contraction,
            Form::Unclassified,
        ] {
            let json = serde_json::to_string(&form).expect("serialize");
            assert_eq!(json, format!("\"{}\"", form.id()));
        }
    }

    /// The label names the form it contracts, and ～なければ's romanisation carries "eba": the
    /// contraction must win, or the rung would be read as a second provisional.
    #[test]
    fn a_contraction_label_is_a_contraction_whatever_form_it_names() {
        for label in [
            "Contracted ～ておる (～とる)",
            "Contracted ～なければ (～なきゃ)",
            "Contracted provisional (~eba → ~ya)",
        ] {
            assert_eq!(Form::classify(label), Form::Contraction, "{label}");
        }
    }

    #[test]
    fn classification_survives_rewording_and_admits_ignorance() {
        // Keyword matching, so a reworded label still lands.
        assert_eq!(Form::classify("past tense (plain)"), Form::Past);
        assert_eq!(Form::classify("te form"), Form::Unclassified);
        // Something genuinely unknown is not guessed at.
        assert_eq!(Form::classify("Gerundive Optative"), Form::Unclassified);
        assert_eq!(Form::Unclassified.meaning(), "");
    }
}
