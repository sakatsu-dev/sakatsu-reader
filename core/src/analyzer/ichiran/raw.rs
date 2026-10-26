//! Serde types mirroring `ichiran-cli -f` output verbatim.
//!
//! Nothing here interprets anything — that is [`super::convert`]'s job. Keeping
//! the wire shape separate from the normalized model means Ichiran's quirks stay
//! in one place, and the day an embeddable analyzer replaces it, this file is what
//! gets deleted.
//!
//! The shape, confirmed against real output in `core/tests/fixtures/ichiran`:
//!
//! ```text
//! [                       ← segments, in source order
//!   "…",                  ← a segment Ichiran could not analyse (punctuation)
//!   [                     ← otherwise: candidate parses, best first
//!     [                   ← one candidate: [words, score]
//!       [ ["mizu", {word}, []], ... ],
//!       178
//!     ]
//!   ]
//! ]
//! ```

use serde::Deserialize;

/// One segment of the input: either text Ichiran declined to analyse, or its
/// ranked candidate parses.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Segment {
    /// Punctuation, romaji, digits — anything Ichiran passes through unanalysed.
    /// `…` in 先生ってほんと…情けないですね comes back exactly like this.
    Unanalyzed(String),
    /// Candidate parses, highest score first.
    Candidates(Vec<Candidate>),
}

/// A candidate parse of one segment: its words and Ichiran's score for them.
#[derive(Debug, Clone, Deserialize)]
pub struct Candidate(pub Vec<WordEntry>, #[allow(dead_code)] pub f64);

/// One word in a candidate: `["romaji", {info}, []]`.
///
/// The third element is empty in every output observed. It is typed loosely so a
/// future Ichiran that starts populating it does not break parsing — but nothing
/// reads it, so if it turns out to carry alternative readings, that is data 屠つ is
/// currently discarding.
#[derive(Debug, Clone, Deserialize)]
pub struct WordEntry(
    /// The romaji, e.g. `"seia"`. The only level that has it — `WordInfo` does not.
    pub String,
    pub WordInfo,
    #[allow(dead_code)] pub Vec<serde_json::Value>,
);

/// Everything Ichiran knows about one word.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WordInfo {
    /// The surface text as written.
    ///
    /// Defaulted because an `alternative` wrapper has no text of its own — the
    /// text lives on each entry inside it.
    #[serde(default)]
    pub text: String,
    /// `"食べる 【たべる】"`, or just `"きます"` when there is no kanji.
    #[serde(default)]
    pub reading: String,
    /// The surface reading in kana. Drives furigana — never normalized.
    #[serde(default)]
    pub kana: String,
    /// The JMdict sequence, which Ichiran states on the word and not on its `conj` levels.
    #[serde(default)]
    pub seq: i64,
    /// Dictionary senses. **Empty when the word is conjugated**: then the senses
    /// live on `conj` (or, for a compound, on each component's `conj`).
    #[serde(default)]
    pub gloss: Vec<Gloss>,
    #[serde(default)]
    pub conj: Vec<Conj>,
    /// A compound's surface parts, e.g. `["食べさせられて", "いた"]`.
    #[serde(default)]
    pub compound: Vec<String>,
    /// A compound's parts, analysed. Parallel to `compound`.
    #[serde(default)]
    pub components: Vec<WordInfo>,
    /// Competing dictionary entries for one ambiguous surface. とり in にはとりが鳴く
    /// arrives as eight of these — 鳥, 取り, 撮り… — and the whole word entry is then
    /// *only* this list: no `text`, no `gloss`, no `conj` at the outer level.
    ///
    /// This is common in ordinary sentences (なかった and 家 both produce it), so
    /// mishandling it does not degrade an edge case, it fails the whole passage.
    #[serde(default)]
    pub alternative: Vec<WordInfo>,
    /// Ichiran's confidence. Only used to rank `alternative` entries, which arrive
    /// highest-first — but this sorts on it rather than trusting the order.
    #[serde(default)]
    pub score: f64,
    /// Prose note Ichiran attaches to an auxiliary component ("indicates action
    /// that had been continuing up till now"). Not used: the grammar engine must
    /// decide auxiliaries from the normalized model alone, so that it keeps
    /// working when the analyzer is swapped.
    #[serde(default)]
    pub suffix: Option<String>,
}

/// One dictionary sense.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Gloss {
    /// Bracketed POS/tag list as JMdict writes it, e.g. `"[v5m,vt]"`.
    #[serde(default)]
    pub pos: String,
    pub gloss: String,
    /// Usage note, e.g. "after the -te form of a verb".
    #[serde(default)]
    pub info: Option<String>,
    /// Field marker, e.g. `"{sumo}"`.
    #[serde(default)]
    pub field: Option<String>,
}

/// A conjugation entry.
///
/// It carries `reading` + `gloss` when *this* level is the dictionary word, and/or
/// `via` when the form is built on something deeper. Both happen: `きます` has a
/// reading and no via; `言わせて` has via and no reading of its own.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Conj {
    #[serde(default)]
    pub prop: Vec<Prop>,
    /// The lemma of this level, `"来る 【クる】"`-style, when present.
    #[serde(default)]
    pub reading: Option<String>,
    #[serde(default)]
    pub gloss: Vec<Gloss>,
    /// Deeper links — or, when a link carries its own `reading`, competing
    /// *alternative parses*. See `super::convert::choose_branch`.
    #[serde(default)]
    pub via: Vec<Conj>,
}

/// One morphological property of a conjugated form.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Prop {
    /// The verb/adjective class: `"v1"`, `"v5u"`, `"vk"`, `"adj-i"`.
    #[serde(default)]
    pub pos: String,
    /// The form as prose: `"Past (~ta)"`, `"Causative-Passive"`, `"Non-past"`.
    /// Classified by keyword, never by equality — the wording shifts between
    /// Ichiran versions (`project notes` §4).
    #[serde(rename = "type", default)]
    pub form: String,
    #[serde(default)]
    pub neg: bool,
    #[serde(default)]
    pub fml: bool,
}

/// Parse `ichiran-cli -f` output.
pub fn parse(json: &str) -> Result<Vec<Segment>, serde_json::Error> {
    serde_json::from_str(json)
}

/// Split a `reading` field into its spelling and its kana.
///
/// `"食べる 【たべる】"` → `("食べる", "たべる")`; `"きます"` → `("きます", "きます")`,
/// because Ichiran omits the bracket when the word is already all kana.
pub fn split_reading(reading: &str) -> (&str, &str) {
    match reading.find('【') {
        Some(open) => {
            let spelling = reading[..open].trim();
            let kana = reading[open + '【'.len_utf8()..]
                .trim_end()
                .trim_end_matches('】');
            (spelling, kana.trim())
        }
        None => {
            let trimmed = reading.trim();
            (trimmed, trimmed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_splits_into_spelling_and_kana() {
        assert_eq!(split_reading("食べる 【たべる】"), ("食べる", "たべる"));
        // The katakana-stem case; normalization happens later, not here.
        assert_eq!(split_reading("来る 【クる】"), ("来る", "クる"));
        // All-kana words carry no bracket.
        assert_eq!(split_reading("きます"), ("きます", "きます"));
        assert_eq!(split_reading(""), ("", ""));
    }

    #[test]
    fn an_unanalyzed_segment_parses_as_a_bare_string() {
        let segments = parse(r#"["…"]"#).expect("parse");
        assert_eq!(segments.len(), 1);
        assert!(matches!(&segments[0], Segment::Unanalyzed(s) if s == "…"));
    }
}
