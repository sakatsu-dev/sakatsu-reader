//! Explaining tag codes.
//!
//! Two different things are called "tags", and conflating them leaves codes
//! unexplained (`project notes` §6):
//!
//! - A dictionary's own `tag_bank` explains **its** codes. JMnedict is where `place`
//!   and `surname` are defined; nothing else knows them.
//! - The `[n,vs,vt]` codes on the senses the **analyzer** emits come from JMdict, and
//!   no dictionary's tag bank carries them — Jitendex spells its parts of speech out
//!   in prose instead. So a small JMdict abbreviation table has to be shipped, with
//!   each dictionary's own bank merged over the top of it.
//!
//! The table below is the EDRDG abbreviation list (JMdict's own documented codes),
//! trimmed to what actually turns up in analysis output.

use std::collections::HashMap;

use super::index::TagInfo;

/// JMdict / EDRDG abbreviations: code → what it means in prose.
const JMDICT: &[(&str, &str)] = &[
    // Parts of speech.
    ("n", "noun"),
    ("n-adv", "adverbial noun"),
    ("n-pr", "proper noun"),
    ("n-pref", "noun used as a prefix"),
    ("n-suf", "noun used as a suffix"),
    ("n-t", "noun (temporal)"),
    ("adj-i", "い-adjective"),
    ("adj-ix", "い-adjective (よい/いい class)"),
    ("adj-na", "な-adjective"),
    ("adj-no", "noun used with の"),
    ("adj-pn", "pre-noun adjectival"),
    ("adj-t", "たる-adjective"),
    ("adj-f", "noun or verb acting prenominally"),
    ("adv", "adverb"),
    ("adv-to", "adverb taking と"),
    ("aux", "auxiliary"),
    ("aux-v", "auxiliary verb"),
    ("aux-adj", "auxiliary adjective"),
    ("conj", "conjunction"),
    ("cop", "copula"),
    ("ctr", "counter"),
    ("exp", "expression"),
    ("int", "interjection"),
    ("num", "numeric"),
    ("pn", "pronoun"),
    ("pref", "prefix"),
    ("prt", "particle"),
    ("suf", "suffix"),
    ("unc", "unclassified"),
    // Verb classes. The ones the grammar engine keys off.
    ("v1", "ichidan verb"),
    ("v1-s", "ichidan verb (くれる class)"),
    ("v5aru", "godan verb (ある class)"),
    ("v5b", "godan verb ending in ぶ"),
    ("v5g", "godan verb ending in ぐ"),
    ("v5k", "godan verb ending in く"),
    ("v5k-s", "godan verb, 行く class"),
    ("v5m", "godan verb ending in む"),
    ("v5n", "godan verb ending in ぬ"),
    ("v5r", "godan verb ending in る"),
    ("v5r-i", "godan verb ending in る (irregular)"),
    ("v5s", "godan verb ending in す"),
    ("v5t", "godan verb ending in つ"),
    ("v5u", "godan verb ending in う"),
    ("v5u-s", "godan verb ending in う (special class)"),
    ("vi", "intransitive"),
    ("vt", "transitive"),
    ("vk", "来る verb (irregular)"),
    ("vn", "irregular ぬ verb"),
    ("vr", "irregular る verb, plain form ends with り"),
    ("vs", "noun or participle taking する"),
    ("vs-c", "する verb, precursor to the modern する"),
    ("vs-i", "する verb (irregular)"),
    ("vs-s", "する verb (special class)"),
    ("vz", "ずる verb (ichidan, つうずる class)"),
    // Usage notes, which change how a word should be read.
    ("uk", "usually written in kana alone"),
    ("col", "colloquial"),
    ("pol", "polite (ていねい) language"),
    ("hon", "honorific or respectful (そんけい) language"),
    ("hum", "humble (けんじょう) language"),
    ("fam", "familiar language"),
    ("fem", "female term or language"),
    ("male", "male term or language"),
    ("arch", "archaic"),
    ("obs", "obsolete term"),
    ("rare", "rare term"),
    ("sl", "slang"),
    ("m-sl", "manga slang"),
    ("net-sl", "internet slang"),
    ("vulg", "vulgar"),
    ("sens", "sensitive"),
    ("derog", "derogatory"),
    ("joc", "jocular or humorous"),
    ("id", "idiomatic expression"),
    ("yoji", "yojijukugo — a four-character idiom"),
    ("on-mim", "onomatopoeia or mimetic word"),
    ("proverb", "proverb"),
    ("quote", "quotation"),
    ("abbr", "abbreviation"),
    ("poet", "poetical term"),
    ("chn", "children's language"),
    // Common frequency markers that arrive as term tags.
    ("ichi1", "in the top 10,000 most common words"),
    ("news1", "common in newspapers"),
    ("spec1", "marked as common"),
];

/// The code behind a known prose tag, for linguistic matching rather than display.
///
/// Jitendex's promoted senses carry labels such as `1-dan` and `na-adj`, while Ichiran
/// carries JMdict codes. Comparing the labels directly made the contextual ranker's
/// inflection evidence unreachable under the built-in analyzer. Match complete labels:
/// splitting `noun or verb acting prenominally` would manufacture two unrelated classes.
/// Unknown dictionary labels remain unknown; a substring is not a part-of-speech claim.
pub fn canonical_code(label: &str) -> Option<&'static str> {
    let label = label.trim();
    JMDICT
        .iter()
        .find(|(code, prose)| label.eq_ignore_ascii_case(code) || label.eq_ignore_ascii_case(prose))
        .map(|(code, _)| *code)
        .or_else(|| {
            // Jitendex's abbreviated labels, observed in the imported definitions. `5-dan`
            // does not name an ending, so it must not become the more specific `v5r` etc.
            match label.to_ascii_lowercase().as_str() {
                "1-dan" => Some("v1"),
                "5-dan" | "5-dan (spec.)" => Some("v5"),
                "suru" => Some("vs"),
                "zuru" => Some("vz"),
                "na-adj" => Some("adj-na"),
                "no-adj" => Some("adj-no"),
                "adjective" => Some("adj-i"),
                "to-adverb" => Some("adv-to"),
                "taru" => Some("adj-t"),
                "adjectival" => Some("adj-pn"),
                "aux-verb" => Some("aux-v"),
                _ => None,
            }
        })
}

/// The codes that mark an entry as a **name** rather than a word.
///
/// These are JMnedict's name types, and the list is the complete set actually present in
/// the shipped dictionary — counted from its 667,800 entries rather than transcribed from
/// the DTD, so nothing here is speculative and nothing real is missing.
///
/// JMnedict's own `tags.json` files every one of them under `"category": "name"`, which is
/// the authority; this list exists because [`super::merge`] decides per hit and does not
/// carry a bank. **If that ever changes, prefer the bank** — a dictionary is the authority
/// on its own codes (see this module's header).
///
/// The collision to know about: `fem` and `masc` also exist in JMdict meaning "female/male
/// term or language" (あたし is one). That is harmless here because the codes are read from
/// `definition_tags`, and Jitendex writes only twelve codes there — `form`, `old`, `kanji`,
/// `★`, `priority`, `rarely`, `used`, `irregular`, `reading`, `obsolete`, `ateji`,
/// `special` — and none of them is a name type. Checked across every entry of both.
const NAME_TYPES: &[&str] = &[
    "place",
    "unclass",
    "surname",
    "fem",
    "person",
    "given",
    "masc",
    "station",
    "organization",
    "work",
    "company",
    "product",
    "serv",
    "char",
    "ev",
    "fict",
    "dei",
    "group",
    "myth",
    "obj",
    "creat",
    "ship",
    "leg",
    "doc",
];

/// Whether any of these tag codes marks the entry as a name.
pub fn marks_a_name(codes: &[String]) -> bool {
    codes
        .iter()
        .any(|code| NAME_TYPES.contains(&normalise(code).as_str()))
}

/// A tag code as it should be looked up.
///
/// Jitendex writes its bank keys with **non-breaking spaces** — `obsolete\u{a0}reading`,
/// `priority\u{a0}form` — and the same tag arrives from a sense with an ordinary one. Left
/// alone the two never meet, and an entry that is sitting right there goes unexplained.
fn normalise(code: &str) -> String {
    code.trim().replace('\u{a0}', " ")
}

/// Explains tag codes, from the built-in JMdict table plus any dictionary's own bank.
#[derive(Debug, Clone)]
pub struct TagGlossary {
    entries: HashMap<String, String>,
}

impl Default for TagGlossary {
    fn default() -> Self {
        Self::jmdict()
    }
}

impl TagGlossary {
    /// The built-in JMdict abbreviations — what the analyzer's `[n,vs,vt]` needs.
    pub fn jmdict() -> TagGlossary {
        TagGlossary {
            entries: JMDICT
                .iter()
                .map(|(code, meaning)| (code.to_string(), meaning.to_string()))
                .collect(),
        }
    }

    /// Merge a dictionary's own tag bank **over** this glossary: the dictionary is
    /// the authority on its own codes, and it may also carry codes nothing else has.
    pub fn merge(&mut self, tags: &HashMap<String, TagInfo>) {
        for (code, info) in tags {
            let note = info.notes.trim();
            if !note.is_empty() {
                self.entries.insert(normalise(code), note.to_string());
            }
        }
    }

    /// What one code means.
    pub fn explain(&self, code: &str) -> Option<&str> {
        self.entries
            .get(&normalise(code))
            .or_else(|| self.entries.get(code.trim()))
            .map(String::as_str)
    }

    /// Split and explain a tag list as the analyzer writes it — `"[v5m,vt]"`, or
    /// space-separated as a dictionary's own fields write it.
    ///
    /// Codes with no explanation are returned with `None` rather than dropped: an
    /// unexplained code is still information, and hiding it would be worse than
    /// showing it bare.
    pub fn explain_list(&self, codes: &str) -> Vec<(String, Option<&str>)> {
        codes
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split([',', ' ', '\t'])
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .map(|code| (code.to_string(), self.explain(code)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linguistic_tags_accept_codes_and_complete_prose_without_guessing() {
        for (code, prose) in JMDICT {
            assert_eq!(canonical_code(code), Some(*code));
            assert_eq!(canonical_code(prose), Some(*code));
        }
        assert_eq!(canonical_code(" 1-dan "), Some("v1"));
        assert_eq!(canonical_code("5-dan"), Some("v5"));
        assert_eq!(canonical_code("na-adj"), Some("adj-na"));
        assert_eq!(
            canonical_code("noun or verb acting prenominally"),
            Some("adj-f")
        );
        assert_eq!(canonical_code("unknown noun label"), None);
    }

    #[test]
    fn the_analyzers_bracketed_pos_list_is_explained() {
        // Exactly the shape sakatsu_core::model::Sense::pos carries.
        let glossary = TagGlossary::jmdict();
        let explained = glossary.explain_list("[v5m,vt]");
        assert_eq!(
            explained,
            [
                ("v5m".to_string(), Some("godan verb ending in む")),
                ("vt".to_string(), Some("transitive")),
            ]
        );
    }

    #[test]
    fn an_unknown_code_is_kept_rather_than_dropped() {
        let glossary = TagGlossary::jmdict();
        let explained = glossary.explain_list("[n,wat]");
        assert_eq!(explained[0].1, Some("noun"));
        assert_eq!(explained[1], ("wat".to_string(), None));
    }

    #[test]
    fn a_dictionarys_own_bank_wins_and_can_add_codes_of_its_own() {
        let mut glossary = TagGlossary::jmdict();
        let mut bank = HashMap::new();
        // JMnedict is the only thing that knows this one.
        bank.insert(
            "surname".to_string(),
            TagInfo {
                notes: "family or surname".into(),
                ..Default::default()
            },
        );
        // And it may disagree with the built-in wording for a code it also uses.
        bank.insert(
            "n".to_string(),
            TagInfo {
                notes: "nouns, as this dictionary means them".into(),
                ..Default::default()
            },
        );
        glossary.merge(&bank);

        assert_eq!(glossary.explain("surname"), Some("family or surname"));
        assert_eq!(
            glossary.explain("n"),
            Some("nouns, as this dictionary means them")
        );
        // Codes the bank said nothing about are untouched.
        assert_eq!(glossary.explain("vt"), Some("transitive"));
    }

    #[test]
    fn a_bank_entry_with_no_note_does_not_erase_the_built_in_meaning() {
        let mut glossary = TagGlossary::jmdict();
        let mut bank = HashMap::new();
        bank.insert(
            "vt".to_string(),
            TagInfo {
                notes: "   ".into(),
                ..Default::default()
            },
        );
        glossary.merge(&bank);
        assert_eq!(glossary.explain("vt"), Some("transitive"));
    }

    #[test]
    fn space_separated_dictionary_tag_fields_also_work() {
        let glossary = TagGlossary::jmdict();
        let explained = glossary.explain_list("ichi1 news1");
        assert_eq!(explained.len(), 2);
        assert!(explained.iter().all(|(_, meaning)| meaning.is_some()));
    }
}
