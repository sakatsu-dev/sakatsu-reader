//! Grammar packs: the one source of explanatory text.
//!
//! Every pattern's title, explanation and examples live here and nowhere else, so
//! the vocabulary breakdown and the grammar view cannot drift apart (`project notes`
//! §5). Code decides *whether* a pattern applies; the pack decides what is *said*
//! about it.
//!
//! A pack is data, not Rust, so it can be reviewed and extended without touching the
//! engine — and so a fetched or user-authored pack can be loaded the same way later.
//! The curated `core` pack is compiled in via `include_str!`.
//!
//! ## Licensing
//!
//! The core pack is written fresh. Imabi and Tae Kim are all-rights-reserved: they
//! may be *linked* through [`Reference`] but never quoted (`project notes` §8). A larger
//! pack derived from the CC-licensed hanabira material is a separate import, and
//! would arrive as its own pack file carrying its own attribution.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::model::Token;

use super::form::Form;

/// A named collection of grammar patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub patterns: Vec<GrammarPattern>,
}

/// One grammar point.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarPattern {
    /// Stable identifier, referenced from code (e.g. the auxiliary table).
    pub id: String,
    /// How the pattern is written, e.g. `"〜てくる"`.
    pub pattern: String,
    pub title: String,
    #[serde(default)]
    pub jlpt: Option<String>,
    /// How it is put together, when saying so helps.
    #[serde(default)]
    pub formation: Option<String>,
    pub explanation: String,
    /// Short renderings, for the **vocabulary** panel rather than the grammar one.
    ///
    /// A construction that groups into a single selectable token is answered from this pack
    /// instead of from a dictionary — see [`crate::dict::merge`] — and it lands in the panel
    /// every ordinary word lands in, where the slot it occupies holds *glosses*: a short line
    /// beside chips reading `expression` or `colloquial`. Given `explanation` to show, it put
    /// a paragraph of grammar prose where "be; is" goes, and printed that same paragraph a
    /// second time in the grammar card beside it.
    ///
    /// A list, and separated the way a dictionary separates them, because that is the shape
    /// of the field it fills ([`crate::dict::merge::Sense::glosses`]) — one string carrying
    /// its own semicolons would render the same and mean less.
    ///
    /// Empty for almost every pattern, because almost none is ever selected as a word. The
    /// ones that are must carry it, and `a_constructions_answer_reads_like_a_words` in
    /// [`crate::dict::merge`] is what requires it of them.
    #[serde(default)]
    pub glosses: Vec<String>,
    /// The longer version, for a learner who wants more than a line. Optional: the
    /// curated pack keeps its explanations short, imported packs often do not.
    #[serde(default)]
    pub detail: Option<String>,
    /// Explicit links into the learner glossary. Empty is meaningful: relationships are
    /// reviewed by hand rather than guessed from words in the explanation.
    #[serde(default)]
    pub term_ids: Vec<String>,
    /// Classification codes for the vocabulary panel's chip row, written as JMdict writes
    /// them, and used only by a pattern that is answered as a word.
    ///
    /// The same codes a dictionary sense carries, so a construction wears the chips every
    /// other entry wears instead of arriving with a bare numbered line. **Every code must be
    /// explainable**: the interface draws a resolved one as a chip that opens its meaning and
    /// an unresolved one as a dead monospace pill, so an invented code would read as a bug
    /// sitting beside working chips. `the_bundled_core_pack_is_valid` checks them against the
    /// glossary, which is also the table `explain_tags` answers from — so a chip here works on
    /// an install with no dictionaries, which is the condition the construction answer exists
    /// to hold.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub examples: Vec<Example>,
    /// A link out. Link only — never quoted text.
    #[serde(default)]
    pub reference: Option<Reference>,
    /// The token sequence that identifies this pattern. Absent when the pattern is
    /// found structurally instead — the 〜て auxiliaries are detected from a
    /// compound's shape, not by matching surfaces.
    #[serde(default, rename = "match")]
    pub match_tokens: Option<Vec<Element>>,
    /// Surface runs that identify this pattern, each matched across **one or more
    /// whole tokens** whose surfaces concatenate to it.
    ///
    /// This exists because the two analyzers disagree about what a token is. Ichiran
    /// merges aggressively and hands over `には` and `ことができる` as single tokens;
    /// Lindera's UniDic splits them into `に`+`は` and `こと`+`が`+`できる`. A rule
    /// written as one `match` element therefore fires under Ichiran and is silently
    /// dead under Lindera — which is the analyzer a user without the sidecar gets.
    ///
    /// Written here once, a pattern fires under both. Whole tokens are still the unit,
    /// so the `にはとり` protection holds: that is one token, and no run of tokens
    /// starting there concatenates to exactly `には`.
    #[serde(default)]
    pub match_text: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub japanese: String,
    pub english: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub label: String,
    pub url: String,
}

/// One part of a match rule. Every constraint present must hold; a part with no
/// constraints at all matches any token, which is how a gap is written.
///
/// The shape follows Corpus Query Language, the query language corpus tools have used
/// for this for decades (`[word="make"] [] [tag="J.*"]`): a sequence of token
/// constraints, each with a repetition range, and an empty constraint as the wildcard.
/// Adopting its semantics rather than inventing a bespoke gap syntax means the two
/// remaining problems — patterns that need to constrain their neighbours, and patterns
/// that are discontinuous — are one mechanism instead of two.
///
/// Constraints are limited to what **both** analyzers report. JMdict part-of-speech
/// tags would be the obvious way to say "preceded by a verb", but the Lindera analyzer
/// deliberately leaves `senses` empty and lets the dictionaries supply them, so a
/// part-of-speech rule would work under Ichiran and silently fail under the default —
/// the exact asymmetry `match_text` exists to remove. Conjugation is reported by both.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Element {
    /// A surface run across one or more *whole* tokens, as in [`GrammarPattern::match_text`].
    /// When present, the constraints below are ignored and the repetition range does
    /// not apply — the run itself decides how many tokens are consumed.
    #[serde(default)]
    pub text: Option<String>,
    /// The token's surface, exactly.
    #[serde(default)]
    pub surface: Option<String>,
    /// Any one of these surfaces. Used where Ichiran merges a pattern with what
    /// follows it — はず arrives as はずです just as often.
    #[serde(default)]
    pub surface_any: Vec<String>,
    /// A run of whole tokens whose surfaces *end with* this.
    ///
    /// The looser sibling of [`Element::text`], and the one that copes with either
    /// analyzer. `text` requires the run to spell the value exactly, which finds
    /// `ませ` + `ん` where Lindera splits it but not `知りません` where Ichiran fuses it.
    /// This requires only that the run *ends* with the value, so both are found: one
    /// token that swallowed the ending, or several that spell it out.
    ///
    /// It does **not** breach the `project notes` §5 rule against matching inside a token.
    /// The match still covers whole tokens — all of `知りません` is reported and
    /// highlighted, not the four characters at its end. What a regex over the sentence
    /// would have done, and this cannot, is match those four characters alone.
    #[serde(default)]
    pub surface_ends: Option<String>,
    /// Any one of several endings, for a run that can finish more than one way.
    ///
    /// Japanese voices て to で after certain stems — 呼ん**で**ください beside
    /// 送っ**て**ください — and when an analyzer fuses the whole thing into one token
    /// there is no way to reach both with a single [`Element::surface_ends`]. Two
    /// elements cannot do it either: under Ichiran `呼んでください` is one token, and one
    /// token cannot satisfy two parts.
    ///
    /// That alternation is everywhere in the language, so it is worth a field rather than
    /// a rule per spelling. The longest matching run wins, as it does elsewhere.
    #[serde(default)]
    pub surface_ends_any: Vec<String>,
    #[serde(default)]
    pub dict_form: Option<String>,
    /// A [`Form`] id, e.g. `"provisional"` — matched against the token's
    /// conjugation rather than its spelling.
    #[serde(default)]
    pub form: Option<String>,
    #[serde(default)]
    pub neg: Option<bool>,
    /// Whether the token inflects at all — true for a verb or adjective, false for a
    /// noun or particle.
    ///
    /// This is what separates `調べた上に` (the grammar point, after a clause) from
    /// `机の上に` (a shelf, after a noun and の), without needing part of speech.
    #[serde(default)]
    pub conjugated: Option<bool>,
    /// Fewest tokens this part may consume. Defaults to 1; `0` makes it optional.
    #[serde(default)]
    pub min: Option<usize>,
    /// Most tokens this part may consume. Defaults to 1. A part with no constraints
    /// and a `max` above 1 is a gap.
    #[serde(default)]
    pub max: Option<usize>,
}

impl Element {
    /// The repetition range, defaulting to exactly one token.
    pub fn repeat(&self) -> (usize, usize) {
        let min = self.min.unwrap_or(1);
        (min, self.max.unwrap_or(min.max(1)))
    }

    /// Whether this part constrains nothing — a gap.
    ///
    /// Gaps refuse to cross a sentence boundary. Without that, `だれも〜ません` would
    /// happily pair a `だれも` in one sentence with a `ません` two sentences later, and
    /// a passage is analysed as a whole here, not a sentence at a time.
    pub fn is_gap(&self) -> bool {
        self.text.is_none()
            && self.surface.is_none()
            && self.surface_any.is_empty()
            && self.surface_ends.is_none()
            && self.surface_ends_any.is_empty()
            && self.dict_form.is_none()
            && self.form.is_none()
            && self.neg.is_none()
            && self.conjugated.is_none()
    }

    /// The constraints that describe a token rather than its spelling.
    ///
    /// A part carrying `text` or `surface_ends` spans a run of tokens, so its spelling
    /// is already settled by the run; these are checked against the token the run ends
    /// on. Without this they were silently ignored, which is the failure mode
    /// `deny_unknown_fields` exists to prevent one level up — a rule that reads as
    /// though it constrains something and does not.
    pub fn matches_apart_from_spelling(&self, token: &Token) -> bool {
        if let Some(conjugated) = self.conjugated {
            if token.conjugation.is_some() != conjugated {
                return false;
            }
        }
        if let Some(dict_form) = &self.dict_form {
            if &token.dict_form != dict_form {
                return false;
            }
        }
        if let Some(form) = &self.form {
            if !props(token).any(|prop| Form::classify(&prop.form).id() == form) {
                return false;
            }
        }
        if let Some(neg) = &self.neg {
            if !props(token).any(|prop| prop.neg == *neg) {
                return false;
            }
        }
        true
    }

    /// Whether this element matches one token.
    pub fn matches(&self, token: &Token) -> bool {
        // `line_break` first, because a break carries no surface of its own and the list
        // below can only see surfaces. Its newline entry never matched anything and is gone.
        if self.is_gap()
            && (token.line_break == crate::model::LineBreak::Break
                || SENTENCE_END.contains(&token.surface.as_str()))
        {
            return false;
        }
        if let Some(conjugated) = self.conjugated {
            if token.conjugation.is_some() != conjugated {
                return false;
            }
        }
        if let Some(surface) = &self.surface {
            if &token.surface != surface {
                return false;
            }
        }
        if !self.surface_any.is_empty() && !self.surface_any.contains(&token.surface) {
            return false;
        }
        if let Some(dict_form) = &self.dict_form {
            if &token.dict_form != dict_form {
                return false;
            }
        }
        if let Some(form) = &self.form {
            if !props(token).any(|prop| Form::classify(&prop.form).id() == form) {
                return false;
            }
        }
        if let Some(neg) = &self.neg {
            if !props(token).any(|prop| prop.neg == *neg) {
                return false;
            }
        }
        true
    }
}

/// What a gap will not step over. A grammar point does not span two sentences.
const SENTENCE_END: &[&str] = &["。", "．", ".", "！", "!", "？", "?"];

/// A token's conjugation properties, including those of a compound's components —
/// a compound's inflection lives on its parts, so ignoring them would make
/// form-based rules silently never fire on compounds.
fn props(token: &Token) -> impl Iterator<Item = &crate::model::Prop> {
    let own = token
        .conjugation
        .iter()
        .flat_map(|conjugation| conjugation.props.iter());
    let parts = token.components.iter().flat_map(|component| {
        component
            .conjugation
            .iter()
            .flat_map(|conjugation| conjugation.props.iter())
    });
    own.chain(parts)
}

impl Pack {
    /// Parse a pack. Unknown fields are rejected, so a misspelled key in a pack file
    /// is an error rather than a rule that silently never fires.
    pub fn from_json(json: &str) -> Result<Pack, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn get(&self, id: &str) -> Option<&GrammarPattern> {
        self.patterns.iter().find(|pattern| pattern.id == id)
    }

    /// Every rule in the pack that the matcher can try, paired with its pattern.
    ///
    /// A pattern may carry both kinds: an element sequence for constraints the
    /// surface cannot express (form, negation) and surface runs for the spellings.
    pub fn matchable(&self) -> impl Iterator<Item = (&GrammarPattern, Rule<'_>)> {
        self.patterns.iter().flat_map(|pattern| {
            let elements = pattern
                .match_tokens
                .as_deref()
                .filter(|elements| !elements.is_empty())
                .map(|elements| (pattern, Rule::Tokens(elements)));
            let texts = pattern
                .match_text
                .iter()
                .filter(|text| !text.is_empty())
                .map(move |text| (pattern, Rule::Text(text)));
            elements.into_iter().chain(texts)
        })
    }
}

/// One way of recognising a pattern in a token sequence.
#[derive(Debug, Clone, Copy)]
pub enum Rule<'a> {
    /// One element per token, each constraining that token.
    Tokens(&'a [Element]),
    /// A surface run spanning one or more whole tokens.
    Text(&'a str),
}

/// The bundled core pack, as JSON.
pub const CORE_JSON: &str = include_str!("packs/core.json");

/// The imported JLPT reference pack, as JSON. See `scripts/import-grammar-pack.ps1`.
pub const HANABIRA_JSON: &str = include_str!("packs/hanabira.json");

/// The bundled core pack, parsed once.
///
/// Panicking here would mean the compiled-in pack is malformed, which
/// `the_bundled_core_pack_is_valid` makes impossible to ship.
pub fn core() -> &'static Pack {
    static CORE: OnceLock<Pack> = OnceLock::new();
    CORE.get_or_init(|| Pack::from_json(CORE_JSON).expect("the bundled core pack parses"))
}

/// The imported reference pack, parsed once.
pub fn hanabira() -> &'static Pack {
    static HANABIRA: OnceLock<Pack> = OnceLock::new();
    HANABIRA.get_or_init(|| Pack::from_json(HANABIRA_JSON).expect("the hanabira pack parses"))
}

/// Every bundled pack, in precedence order.
///
/// The curated pack comes first because it is the one the engine keys off: its ids are
/// referenced from code (the auxiliary table), and its text was written for the panels
/// it appears in. The imported pack is a reference shelf — hundreds of JLPT grammar
/// points with no match rules, there to be looked up rather than detected.
pub fn all() -> [&'static Pack; 2] {
    [core(), hanabira()]
}

/// Find a pattern by id in any bundled pack.
pub fn find(id: &str) -> Option<&'static GrammarPattern> {
    all().into_iter().find_map(|pack| pack.get(id))
}

/// Grammar points whose pattern, title or explanation matches `query`.
///
/// Searching the explanation as well as the pattern is deliberate: a learner looking for
/// なければ should find it from the kana, from the romaji in the title, or from a word in the
/// prose. What that cost, as a plain substring search, was everything else.
///
/// **A Latin query matches whole words, and must be at least three characters.** Measured
/// over the 872 patterns in the two packs, the substring version answered `a` with
/// twenty-five patterns — the "a" inside "Topic m*a*rker は" — and `to`, `in` likewise, and
/// `eat` with six, from "repeat" and "great". None of those is a question about grammar, and
/// the reference page reads "this query matched a pattern" as a signal about what the learner
/// asked for. A signal that fires on `a` is not one.
///
/// Japanese is untouched and stays a substring search. It has no word boundaries to respect,
/// and substring is exactly how なければ reaches 〜なければならない.
pub fn search(query: &str, limit: usize) -> Vec<&'static GrammarPattern> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    // `is_ascii` rather than a script test: what matters is whether the query is written in
    // something that *has* word boundaries, and everything this pack is written in that does
    // is ASCII — the kana and kanji are not.
    let latin = query.is_ascii();
    if latin && query.chars().count() < 3 {
        return Vec::new();
    }

    let mut found: Vec<&'static GrammarPattern> = Vec::new();
    for pack in all() {
        for pattern in &pack.patterns {
            if found.len() >= limit {
                return found;
            }
            let haystack = format!(
                "{} {} {}",
                pattern.pattern, pattern.title, pattern.explanation
            )
            .to_lowercase();
            if matches(&haystack, &query, latin) {
                found.push(pattern);
            }
        }
    }
    found
}

/// Whether `haystack` holds `query` — as a whole word for Latin, anywhere for Japanese.
///
/// Byte indices from `match_indices`, and that is safe next to multibyte text rather than in
/// spite of it: a UTF-8 continuation byte is never `is_ascii_alphanumeric`, so a Latin word
/// butted against a kanji reads as ending there, which is what it does.
fn matches(haystack: &str, query: &str, latin: bool) -> bool {
    if !latin {
        return haystack.contains(query);
    }
    let bytes = haystack.as_bytes();
    haystack.match_indices(query).any(|(at, hit)| {
        let before = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let end = at + hit.len();
        let after = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
        before && after
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Conjugation, Prop};

    fn token(surface: &str) -> Token {
        Token {
            surface: surface.into(),
            dict_form: surface.into(),
            ..Default::default()
        }
    }

    /// `surface_ends` claims whole tokens, never part of one, and copes with either
    /// analyzer's segmentation. The distinction from a substring match is `project notes` §5.
    #[test]
    fn a_suffix_rule_claims_whole_tokens_under_either_segmentation() {
        let pack = Pack {
            name: "probe".into(),
            description: String::new(),
            patterns: vec![GrammarPattern {
                id: "probe".into(),
                pattern: "なさい".into(),
                title: String::new(),
                jlpt: None,
                formation: None,
                explanation: String::new(),
                glosses: Vec::new(),
                detail: None,
                term_ids: Vec::new(),
                tags: Vec::new(),
                examples: Vec::new(),
                reference: None,
                match_text: Vec::new(),
                match_tokens: Some(vec![Element {
                    surface_ends: Some("なさい".into()),
                    ..Default::default()
                }]),
            }],
        };
        let found = |tokens: &[Token]| crate::grammar::matcher::find(tokens, &pack);

        // Ichiran fuses the ending into the verb: that whole token is the match.
        let fused = [token("寝なさい")];
        assert_eq!(found(&fused).len(), 1);
        assert_eq!(
            found(&fused)[0].token_count(),
            1,
            "the whole token is claimed"
        );

        // Under a split segmentation, なさい is already a whole token. The verb stem
        // contributes none of the suffix, so the shortest honest match claims only the
        // ending instead of swallowing an unrelated token to its left.
        let split = [token("寝"), token("なさい")];
        assert_eq!(found(&split).len(), 1);
        assert_eq!(found(&split)[0].token_count(), 1);

        // A token that merely contains it elsewhere is not a match — the ending has to
        // end the run, which is what keeps this from being a substring search.
        assert!(found(&[token("なさいます")]).is_empty());
        assert!(found(&[token("寝る")]).is_empty());
    }

    #[test]
    fn the_bundled_core_pack_is_valid() {
        let pack = core();
        assert_eq!(pack.name, "core");
        assert!(pack.patterns.len() > 20, "the pack should be worth having");

        let mut ids: Vec<&str> = pack.patterns.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "pattern ids must be unique");

        for pattern in &pack.patterns {
            assert!(!pattern.title.is_empty(), "{} has no title", pattern.id);
            assert!(
                !pattern.explanation.is_empty(),
                "{} has no explanation",
                pattern.id
            );
            for term_id in &pattern.term_ids {
                assert!(
                    crate::terms::get(term_id).is_some(),
                    "{} links to missing glossary term {term_id}",
                    pattern.id
                );
            }
            // A vocabulary chip is a control when the code resolves and a dead monospace
            // pill when it does not, and the glossary is what `explain_tags` asks first —
            // so an unknown code here ships as a badge nothing in the app can explain.
            for tag in &pattern.tags {
                assert!(
                    crate::terms::get(tag).is_some(),
                    "{} carries the tag {tag:?}, which no glossary entry explains",
                    pattern.id
                );
            }
            // A rule that references an unknown form id would never fire.
            for element in pattern.match_tokens.iter().flatten() {
                if let Some(form) = &element.form {
                    assert_ne!(
                        Form::classify(form),
                        Form::Unclassified,
                        "{} matches on unknown form id {form:?}",
                        pattern.id
                    );
                    assert_eq!(
                        Form::classify(form).id(),
                        form,
                        "{} should use the canonical form id",
                        pattern.id
                    );
                }
            }
        }
    }

    #[test]
    fn an_element_requires_every_constraint_it_states() {
        let token = Token {
            surface: "行かなければ".into(),
            dict_form: "行く".into(),
            conjugation: Some(Conjugation {
                lemma: "行く".into(),
                reading: "いく".into(),
                props: vec![Prop {
                    form: "Provisional (~eba)".into(),
                    neg: true,
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        assert!(Element {
            form: Some("provisional".into()),
            neg: Some(true),
            ..Default::default()
        }
        .matches(&token));

        // Right form, wrong polarity.
        assert!(!Element {
            form: Some("provisional".into()),
            neg: Some(false),
            ..Default::default()
        }
        .matches(&token));

        // Right polarity, wrong form.
        assert!(!Element {
            form: Some("past".into()),
            ..Default::default()
        }
        .matches(&token));

        // An empty element matches anything.
        assert!(Element::default().matches(&token));
    }

    #[test]
    fn the_imported_reference_pack_is_valid() {
        let pack = hanabira();
        assert_eq!(pack.name, "hanabira");
        assert!(
            pack.patterns.len() > 500,
            "expected the full JLPT set, got {}",
            pack.patterns.len()
        );
        // CC0 needs no attribution, but saying where data came from is the point of §8.
        assert!(pack.description.contains("CC0"));

        let mut ids: Vec<&str> = pack.patterns.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "pattern ids must be unique");

        for pattern in &pack.patterns {
            assert!(
                !pattern.pattern.is_empty(),
                "{} has no written form",
                pattern.id
            );
            assert!(
                !pattern.explanation.is_empty(),
                "{} has no explanation",
                pattern.id
            );
            // This pack was once a reference shelf with no rules at all, on the
            // reasoning that a rule auto-derived from a title like "A が いちばん～"
            // would misfire. It would — so rules are derived, then verified against the
            // entry's own examples and against an independent corpus
            // (`tests/pack_precision.rs`) before they land here. What this asserts now
            // is that whatever landed is well formed.
            for element in pattern.match_tokens.iter().flatten() {
                let (min, max) = element.repeat();
                assert!(min <= max, "{}: min {min} exceeds max {max}", pattern.id);
                // Spelt out rather than `is_none_or`, which needs Rust 1.82 — the
                // workspace's MSRV is 1.80.
                let text_is_usable = match &element.text {
                    Some(text) => !text.is_empty(),
                    None => true,
                };
                assert!(text_is_usable, "{}: empty text run", pattern.id);
            }
            // A rule of nothing but gaps matches every sentence in the language.
            if let Some(elements) = &pattern.match_tokens {
                assert!(
                    elements.iter().any(|element| !element.is_gap()),
                    "{}: a rule must constrain something",
                    pattern.id
                );
            }
            for text in &pattern.match_text {
                assert!(!text.is_empty(), "{}: empty match_text run", pattern.id);
            }
        }

        // Every JLPT level is represented.
        for level in ["N5", "N4", "N3", "N2", "N1"] {
            assert!(
                pack.patterns
                    .iter()
                    .any(|pattern| pattern.jlpt.as_deref() == Some(level)),
                "no {level} entries"
            );
        }
    }

    #[test]
    fn ids_cannot_collide_between_the_two_packs() {
        // `find` searches both, so a shared id would make one pack shadow the other.
        for imported in &hanabira().patterns {
            assert!(
                core().get(&imported.id).is_none(),
                "{} exists in both packs",
                imported.id
            );
        }
    }

    #[test]
    fn searching_finds_grammar_by_kana_by_romaji_and_by_meaning() {
        // The three ways a learner might come at a half-remembered grammar point.
        assert!(
            search("なければ", 20)
                .iter()
                .any(|pattern| pattern.pattern.contains("なければ")),
            "kana search found nothing"
        );
        assert!(
            !search("ichiban", 20).is_empty(),
            "romaji search found nothing"
        );
        assert!(
            !search("must", 20).is_empty(),
            "meaning search found nothing"
        );

        assert!(
            search("   ", 20).is_empty(),
            "a blank query matches nothing"
        );
        assert!(search("な", 3).len() <= 3, "the limit is respected");
    }

    /// A Latin query matches whole words.
    ///
    /// As a plain substring search this answered `a` with the cap — the "a" inside "Topic
    /// marker" — and `eat` with six, from "repeat" and "great". The reference page reads
    /// "this matched a pattern" as a signal about what the learner asked for, and a signal
    /// that fires on `a` is not one.
    ///
    /// Asserted as properties rather than counts, so editing a pack cannot break them.
    #[test]
    fn a_latin_query_matches_whole_words_only() {
        for query in ["a", "to", "in", "is"] {
            assert!(
                search(query, 25).is_empty(),
                "{query:?} is not a question about grammar"
            );
        }

        // Whole-word, not merely long enough: "eat" is in "repeat" and "great" across these
        // packs and in no pattern on its own.
        for found in search("eat", 25) {
            let haystack =
                format!("{} {} {}", found.pattern, found.title, found.explanation).to_lowercase();
            assert!(
                haystack
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .any(|word| word == "eat"),
                "{:?} matched \"eat\" inside a longer word",
                found.id
            );
        }

        // The words a learner would actually reach for still work.
        for query in ["passive", "causative", "conditional"] {
            assert!(
                !search(query, 25).is_empty(),
                "{query:?} should find patterns"
            );
        }
    }

    /// Japanese keeps substring matching: it has no word boundaries, and substring is how
    /// なければ reaches 〜なければならない. A two-character kana query is also a real one, so the
    /// length floor must not apply to it.
    #[test]
    fn japanese_is_not_held_to_word_boundaries() {
        assert!(
            search("なけれ", 20)
                .iter()
                .any(|pattern| pattern.pattern.contains("なければ")),
            "a substring of a pattern should still find it"
        );
        assert!(!search("たら", 20).is_empty(), "two kana is a real query");
    }

    #[test]
    fn find_reaches_both_packs() {
        assert!(find("te-kuru").is_some(), "the curated pack");
        let imported = &hanabira().patterns[0].id;
        assert!(find(imported).is_some(), "the imported pack");
        assert!(find("no-such-pattern").is_none());
    }

    #[test]
    fn a_misspelled_key_in_a_pack_is_rejected() {
        // `explanaiton` would otherwise be dropped silently, leaving a pattern with
        // no text and no error.
        let json = r#"{
            "name": "typo",
            "patterns": [
                { "id": "x", "pattern": "x", "title": "X", "explanaiton": "oops" }
            ]
        }"#;
        assert!(Pack::from_json(json).is_err());
    }
}
