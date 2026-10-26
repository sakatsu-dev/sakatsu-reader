//! Yomitan/Yomichan term parsing.
//!
//! A dictionary is a zip of `index.json` plus `term_bank_N.json` files (and
//! optionally `tag_bank_N.json`, `term_meta_bank_N.json`, `kanji_bank_N.json`).
//!
//! ## The 8-field positional array
//!
//! A term is an array, not an object — verified against Yomitan's own
//! `dictionary-term-bank-v3-schema.json`:
//!
//! ```text
//! [ term, reading, definitionTags, rules, score, glossary, sequence, termTags ]
//!    0       1           2           3      4       5         6         7
//! ```
//!
//! Fields are addressed by index here so the mapping to the schema stays visible.
//! Two traps the schema spells out: field 1 is **empty when the reading is the same
//! as the term** (so it must be filled in, not stored blank), and field 2 is
//! **nullable**.
//!
//! ## Structured content: read the labels, do not flatten
//!
//! A glossary entry is either a plain string or a structured-content tree. The tree
//! says what its own parts are: a node's `data.content` names it a `"sense"`, a
//! `"glossary"`, an `"example-sentence"` and so on, and `data.class = "tag"` marks a
//! tag span. Jitendex leans on this heavily.
//!
//! Flattening the tree to text yields the senses, the part-of-speech tags, the
//! example sentences *and the furigana of those examples* run together into one
//! unreadable line. So the labels are read instead: each sense becomes its own
//! definition, examples are collected separately and attached to their sense, and
//! attribution/audio nodes are pruned.
//!
//! Note that `data` attributes are free-form strings in the schema — these labels
//! are a (near-universal) convention, not a guarantee. Unknown labels therefore
//! degrade to plain text rather than being dropped.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One dictionary entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Entry {
    /// Field 0 — the headword as written.
    pub term: String,
    /// Field 1 — the reading in kana. Filled in from `term` when the source leaves
    /// it blank, so a reading is always present to index under.
    pub reading: String,
    /// Field 2 — tags for the definition. Nullable in the source.
    #[serde(default)]
    pub definition_tags: Vec<String>,
    /// Field 3 — deinflection rule identifiers (`v1`, `v5`, `adj-i`…).
    #[serde(default)]
    pub rules: Vec<String>,
    /// Field 4 — popularity. Negative is rarer, positive is more frequent.
    #[serde(default)]
    pub score: f64,
    /// Field 5 — the definitions, with structured content already interpreted.
    #[serde(default)]
    pub definitions: Vec<Definition>,
    /// Field 6 — entries sharing a sequence number belong together.
    #[serde(default)]
    pub sequence: i64,
    /// Field 7 — tags for the term itself.
    #[serde(default)]
    pub term_tags: Vec<String>,
}

/// One sense of an entry: what it means, how it is labelled, and how it is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Definition {
    /// Tag spans attached to this sense ("adjective", "colloquial", "archaic").
    #[serde(default)]
    pub tags: Vec<String>,
    /// The glosses. Several glosses in one sense are the same meaning said several
    /// ways ("dangerous; risky"), not several meanings.
    #[serde(default)]
    pub glosses: Vec<String>,
    /// Example sentences for this sense. Attached to the sense, never mixed into
    /// the gloss text.
    #[serde(default)]
    pub examples: Vec<Example>,
}

/// An example sentence and its translation.
///
/// `Eq` unlike [`Entry`], which cannot have it: `Entry::score` is a float.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(rename = "DictExample"))]
pub struct Example {
    pub japanese: String,
    pub english: String,
}

/// Node labels whose content is not part of a gloss. Pruned from the gloss walk;
/// examples are recovered by a separate pass that deliberately descends into
/// `extra-info`.
const NOT_GLOSS: &[&str] = &["example-sentence", "extra-info", "attribution", "audio"];

/// Labels that own their own tag scope.
///
/// Real Jitendex nests `sense-groups > sense-group > ol > li(sense)`, and hangs the
/// part of speech on the **sense-group**, outside the senses it applies to. So tags
/// are inherited downwards — but a group's tags must not reach a *sibling* group's
/// senses, which is what stopping at these labels prevents.
const TAG_SCOPES: &[&str] = &["sense", "sense-group", "sense-groups"];

/// Labels that are editorial marks rather than part of a sentence. `[3]` after
/// "His life is in danger." is a footnote, not part of the translation.
const NOT_SENTENCE: &[&str] = &["attribution-footnote", "attribution"];

/// Parse one `term_bank_N.json`: an array of 8-field arrays.
pub fn parse_term_bank(json: &str) -> Result<Vec<Entry>, serde_json::Error> {
    let raw: Vec<Value> = serde_json::from_str(json)?;
    Ok(raw.iter().filter_map(parse_term).collect())
}

/// Parse one term array. Returns `None` for anything that is not a usable entry,
/// rather than failing the whole bank for one malformed row — a 400,000-entry
/// dictionary should not be unusable because of one bad line.
pub fn parse_term(value: &Value) -> Option<Entry> {
    let fields = value.as_array()?;
    let term = fields.first()?.as_str()?.to_string();
    if term.is_empty() {
        return None;
    }

    // Field 1 is empty when the reading matches the term.
    let reading = match fields.get(1).and_then(Value::as_str).unwrap_or("") {
        "" => term.clone(),
        reading => reading.to_string(),
    };

    Some(Entry {
        term,
        reading,
        definition_tags: space_separated(fields.get(2)),
        rules: space_separated(fields.get(3)),
        score: fields.get(4).and_then(Value::as_f64).unwrap_or(0.0),
        definitions: fields
            .get(5)
            .and_then(Value::as_array)
            .map(|glossary| glossary.iter().flat_map(definitions_from).collect())
            .unwrap_or_default(),
        sequence: fields.get(6).and_then(Value::as_i64).unwrap_or(0),
        term_tags: space_separated(fields.get(7)),
    })
}

/// A space-separated tag string. Absent and `null` both mean no tags.
fn space_separated(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_str)
        .map(|text| {
            text.split_whitespace()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// Interpret one glossary item, which may be a plain string, a text node, an image,
/// or a structured-content tree holding several senses.
fn definitions_from(item: &Value) -> Vec<Definition> {
    // The simple case, and what most dictionaries use.
    if let Some(text) = item.as_str() {
        return one_gloss(text);
    }

    match item.get("type").and_then(Value::as_str) {
        Some("text") => one_gloss(item.get("text").and_then(Value::as_str).unwrap_or("")),
        // An image carries no definition text.
        Some("image") => Vec::new(),
        Some("structured-content") => from_structured(item.get("content").unwrap_or(&Value::Null)),
        // Deinflection entries and anything unrecognized: take whatever text there
        // is rather than dropping the definition entirely.
        _ => from_structured(item),
    }
}

fn one_gloss(text: &str) -> Vec<Definition> {
    let gloss = collapse(text);
    if gloss.is_empty() {
        return Vec::new();
    }
    vec![Definition {
        glosses: vec![gloss],
        ..Default::default()
    }]
}

/// Pull the senses out of a structured-content tree.
///
/// When the tree labels its senses, each becomes one definition. When it does not —
/// a dictionary that uses structured content only for formatting — the whole tree
/// becomes a single definition, which is the honest reading of an unlabelled tree.
fn from_structured(content: &Value) -> Vec<Definition> {
    let mut definitions = Vec::new();
    collect_definitions(content, &[], &mut definitions);

    if !definitions.is_empty() {
        return definitions;
    }

    let mut text = String::new();
    gloss_text(content, &mut text);
    let mut definition = Definition {
        glosses: collapse(&text)
            .split_inclusive(';')
            .map(|part| part.trim().trim_end_matches(';').to_string())
            .filter(|part| !part.is_empty())
            .collect(),
        tags: tags_in(content),
        examples: examples_in(content),
    };
    if definition.glosses.is_empty() {
        // Keep an entry that is only an example or only tags out of the results.
        definition.examples.clear();
        return Vec::new();
    }
    vec![definition]
}

/// Walk to the senses, carrying down the tags attached above them.
///
/// Descent stops at a sense, so a sense nested inside another is not reported twice.
fn collect_definitions(node: &Value, inherited: &[String], out: &mut Vec<Definition>) {
    if let Some(items) = node.as_array() {
        for item in items {
            collect_definitions(item, inherited, out);
        }
        return;
    }
    if !node.is_object() || NOT_GLOSS.contains(&content_kind(node)) {
        return;
    }

    if content_kind(node) == "sense" {
        if let Some(definition) = definition_from_sense(node, inherited) {
            out.push(definition);
        }
        return;
    }

    // Whatever is tagged at this level applies to every sense underneath it.
    let mut scope = inherited.to_vec();
    for tag in tags_at_level(node) {
        if !scope.contains(&tag) {
            scope.push(tag);
        }
    }
    collect_definitions(children(node), &scope, out);
}

fn definition_from_sense(sense: &Value, inherited: &[String]) -> Option<Definition> {
    let mut glosses = Vec::new();
    for node in find_labelled(sense, "glossary") {
        glosses.extend(glosses_in(node));
    }

    if glosses.is_empty() {
        // Some dictionaries label the sense but not the glossary inside it.
        let mut text = String::new();
        gloss_text(sense, &mut text);
        let text = collapse(&text);
        if text.is_empty() {
            return None;
        }
        glosses.push(text);
    }

    let mut tags = inherited.to_vec();
    for tag in tags_in(sense) {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }

    Some(Definition {
        tags,
        glosses,
        examples: examples_in(sense),
    })
}

/// The glosses in one `glossary` node.
///
/// **One gloss per list item.** Jitendex puts each gloss in its own `<li>`, and
/// running them together gives "in danger in jeopardy critical grave at risk" — one
/// unreadable line where there were five meanings. Only when the node is not a list
/// does its text get read whole, split on the semicolons a flat dictionary uses.
fn glosses_in(node: &Value) -> Vec<String> {
    let items = list_items(node);
    if !items.is_empty() {
        return items
            .iter()
            .filter_map(|item| {
                let mut text = String::new();
                gloss_text(item, &mut text);
                let text = collapse(&text);
                (!text.is_empty()).then_some(text)
            })
            .collect();
    }

    let mut text = String::new();
    gloss_text(node, &mut text);
    collapse(&text)
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// The outermost `<li>` descendants, so a nested list does not split one gloss.
fn list_items(node: &Value) -> Vec<&Value> {
    let mut found = Vec::new();
    collect_list_items(children(node), &mut found);
    found
}

fn collect_list_items<'a>(node: &'a Value, found: &mut Vec<&'a Value>) {
    if let Some(items) = node.as_array() {
        for item in items {
            collect_list_items(item, found);
        }
        return;
    }
    if !node.is_object() || NOT_GLOSS.contains(&content_kind(node)) {
        return;
    }
    if node.get("tag").and_then(Value::as_str) == Some("li") {
        found.push(node);
        return;
    }
    collect_list_items(children(node), found);
}

/// Tag spans attached at this level, not below a scope of their own.
fn tags_at_level(node: &Value) -> Vec<String> {
    let mut tags = Vec::new();
    collect_tags_at_level(children(node), &mut tags);
    tags
}

fn collect_tags_at_level(node: &Value, tags: &mut Vec<String>) {
    if let Some(items) = node.as_array() {
        for item in items {
            collect_tags_at_level(item, tags);
        }
        return;
    }
    if !node.is_object() {
        return;
    }
    let kind = content_kind(node);
    if TAG_SCOPES.contains(&kind) || NOT_GLOSS.contains(&kind) {
        return;
    }
    if is_tag_span(node) {
        push_tag(node, tags);
        return;
    }
    collect_tags_at_level(children(node), tags);
}

fn is_tag_span(node: &Value) -> bool {
    node.get("data")
        .and_then(|data| data.get("class"))
        .and_then(Value::as_str)
        == Some("tag")
}

fn push_tag(node: &Value, tags: &mut Vec<String>) {
    let mut text = String::new();
    gloss_text(node, &mut text);
    let text = collapse(&text);
    if !text.is_empty() && !tags.contains(&text) {
        tags.push(text);
    }
}

/// `data.content` — the node's own label for what it is.
fn content_kind(node: &Value) -> &str {
    node.get("data")
        .and_then(|data| data.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

/// A node's children live under `content`, which may be a string, a node, or an
/// array of either.
fn children(node: &Value) -> &Value {
    node.get("content").unwrap_or(&Value::Null)
}

/// Gloss text: words joined by spaces, with the non-gloss labels pruned.
fn gloss_text(node: &Value, out: &mut String) {
    if let Some(text) = node.as_str() {
        push_spaced(out, text);
        return;
    }
    if let Some(items) = node.as_array() {
        for item in items {
            gloss_text(item, out);
        }
        return;
    }
    if !node.is_object() {
        return;
    }
    if node.get("type").and_then(Value::as_str) == Some("image")
        || NOT_GLOSS.contains(&content_kind(node))
    {
        return;
    }
    if let Some(text) = node.get("text").and_then(Value::as_str) {
        push_spaced(out, text);
        return;
    }
    gloss_text(children(node), out);
}

/// Running Japanese text: joined with **nothing**, because Japanese has no spaces,
/// and with ruby readings dropped.
///
/// `rt` holds the furigana and `rp` the parenthetical fallback around it. Keeping
/// either turns 私 into 私わたし — the run-on that makes examples look worthless.
fn sentence_text(node: &Value, out: &mut String) {
    if let Some(text) = node.as_str() {
        out.push_str(text);
        return;
    }
    if let Some(items) = node.as_array() {
        for item in items {
            sentence_text(item, out);
        }
        return;
    }
    if !node.is_object() {
        return;
    }
    if matches!(
        node.get("tag").and_then(Value::as_str),
        Some("rt") | Some("rp")
    ) {
        return;
    }
    // A footnote marker is not part of the sentence: real Jitendex puts "[3]" inside
    // the English translation node.
    if NOT_SENTENCE.contains(&content_kind(node)) {
        return;
    }
    if let Some(text) = node.get("text").and_then(Value::as_str) {
        out.push_str(text);
        return;
    }
    sentence_text(children(node), out);
}

/// Every node carrying `label`, without descending into non-gloss branches.
fn find_labelled<'a>(node: &'a Value, label: &str) -> Vec<&'a Value> {
    let mut found = Vec::new();
    walk_gloss(node, &mut |candidate| {
        if content_kind(candidate) == label {
            found.push(candidate);
        }
    });
    found
}

fn walk_gloss<'a>(node: &'a Value, visit: &mut impl FnMut(&'a Value)) {
    if let Some(items) = node.as_array() {
        for item in items {
            walk_gloss(item, visit);
        }
        return;
    }
    if !node.is_object() || NOT_GLOSS.contains(&content_kind(node)) {
        return;
    }
    visit(node);
    walk_gloss(children(node), visit);
}

/// Tag spans, marked by `data.class = "tag"`.
fn tags_in(node: &Value) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    walk_gloss(node, &mut |candidate| {
        if is_tag_span(candidate) {
            push_tag(candidate, &mut tags);
        }
    });
    tags
}

/// Example sentences. This pass **does** descend into `extra-info`, which is where
/// Jitendex keeps them and which the gloss walk prunes.
fn examples_in(node: &Value) -> Vec<Example> {
    let mut found = Vec::new();
    collect_examples(node, &mut found);
    found
}

fn collect_examples(node: &Value, found: &mut Vec<Example>) {
    if let Some(items) = node.as_array() {
        for item in items {
            collect_examples(item, found);
        }
        return;
    }
    if !node.is_object() {
        return;
    }

    if content_kind(node) == "example-sentence" {
        let mut japanese = String::new();
        let mut english = String::new();
        example_parts(node, &mut japanese, &mut english);
        let japanese = japanese.trim().to_string();
        if !japanese.is_empty() {
            found.push(Example {
                japanese,
                english: collapse(&english),
            });
        }
        return;
    }

    collect_examples(children(node), found);
}

fn example_parts(node: &Value, japanese: &mut String, english: &mut String) {
    if let Some(items) = node.as_array() {
        for item in items {
            example_parts(item, japanese, english);
        }
        return;
    }
    if !node.is_object() {
        return;
    }
    match content_kind(node) {
        "example-sentence-a" => sentence_text(children(node), japanese),
        "example-sentence-b" => sentence_text(children(node), english),
        _ => example_parts(children(node), japanese, english),
    }
}

fn push_spaced(out: &mut String, text: &str) {
    if !out.is_empty() && !out.ends_with(' ') {
        out.push(' ');
    }
    out.push_str(text);
}

/// Squeeze whitespace runs and trim — structured content is full of incidental
/// newlines and indentation.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_eight_fields_are_read_by_position() {
        let term = json!([
            "食べる",
            "たべる",
            "v1 vt",
            "v1",
            132,
            ["to eat"],
            1358280,
            "ichi1 news1"
        ]);
        let entry = parse_term(&term).expect("entry");

        assert_eq!(entry.term, "食べる");
        assert_eq!(entry.reading, "たべる");
        assert_eq!(entry.definition_tags, ["v1", "vt"]);
        assert_eq!(entry.rules, ["v1"]);
        assert_eq!(entry.score, 132.0);
        assert_eq!(entry.definitions[0].glosses, ["to eat"]);
        assert_eq!(entry.sequence, 1358280);
        assert_eq!(entry.term_tags, ["ichi1", "news1"]);
    }

    #[test]
    fn an_empty_reading_field_means_the_reading_is_the_term() {
        // The schema's own wording. Storing this blank would leave the entry
        // unfindable by reading.
        let entry =
            parse_term(&json!(["ひらがな", "", null, "", 0, ["hiragana"], 1, ""])).expect("entry");
        assert_eq!(entry.term, "ひらがな");
        assert_eq!(entry.reading, "ひらがな");
        // Field 2 is nullable.
        assert!(entry.definition_tags.is_empty());
    }

    #[test]
    fn a_negative_score_and_a_malformed_row_are_both_handled() {
        let entry = parse_term(&json!(["椽", "えん", "", "", -14, ["rare word"], 1, ""])).unwrap();
        assert_eq!(entry.score, -14.0);

        // One bad row must not cost the other 400,000 entries.
        let bank = parse_term_bank(r#"[["ok","おく","","",0,["fine"],1,""],42,[],["",""]]"#)
            .expect("bank parses");
        assert_eq!(bank.len(), 1);
        assert_eq!(bank[0].term, "ok");
    }

    /// A Jitendex-shaped tree: senses, each with a labelled glossary, tag spans, and
    /// example sentences buried in `extra-info` with ruby furigana.
    fn jitendex_shaped() -> Value {
        json!([
            "危ない",
            "あぶない",
            "adj-i",
            "adj-i",
            100,
            [{
                "type": "structured-content",
                "content": [
                    {
                        "tag": "div",
                        "data": { "content": "sense" },
                        "content": [
                            { "tag": "span", "data": { "class": "tag" }, "content": "adjective" },
                            { "tag": "div", "data": { "content": "glossary" },
                              "content": "dangerous; risky" },
                            {
                                "tag": "div",
                                "data": { "content": "extra-info" },
                                "content": [{
                                    "tag": "div",
                                    "data": { "content": "example-sentence" },
                                    "content": [
                                        { "tag": "div", "data": { "content": "example-sentence-a" },
                                          "content": [
                                              { "tag": "ruby", "content": [
                                                  "危",
                                                  { "tag": "rt", "content": "あぶ" }
                                              ]},
                                              "ない!"
                                          ]},
                                        { "tag": "div", "data": { "content": "example-sentence-b" },
                                          "content": "Look out!" }
                                    ]
                                }]
                            }
                        ]
                    },
                    {
                        "tag": "div",
                        "data": { "content": "sense" },
                        "content": [
                            { "tag": "div", "data": { "content": "glossary" },
                              "content": "uncertain; unreliable" },
                            { "tag": "div", "data": { "content": "attribution" },
                              "content": "JMdict" }
                        ]
                    }
                ]
            }],
            1,
            ""
        ])
    }

    #[test]
    fn each_labelled_sense_becomes_its_own_definition() {
        let entry = parse_term(&jitendex_shaped()).expect("entry");
        assert_eq!(entry.definitions.len(), 2, "two senses, two definitions");

        let first = &entry.definitions[0];
        assert_eq!(first.glosses, ["dangerous", "risky"]);
        assert_eq!(first.tags, ["adjective"]);

        let second = &entry.definitions[1];
        assert_eq!(second.glosses, ["uncertain", "unreliable"]);
    }

    #[test]
    fn examples_attach_to_their_sense_and_never_leak_into_the_gloss() {
        let entry = parse_term(&jitendex_shaped()).expect("entry");
        let first = &entry.definitions[0];

        // The whole point of reading the labels: the gloss is the gloss.
        for gloss in &first.glosses {
            assert!(
                !gloss.contains("Look out"),
                "the example leaked into the gloss: {gloss:?}"
            );
            assert!(!gloss.contains("危"), "the example leaked: {gloss:?}");
            assert!(
                !gloss.contains("adjective"),
                "the tag leaked into the gloss: {gloss:?}"
            );
        }

        assert_eq!(first.examples.len(), 1);
        // Ruby furigana dropped: 危ない!, not 危あぶない!.
        assert_eq!(first.examples[0].japanese, "危ない!");
        assert_eq!(first.examples[0].english, "Look out!");

        // The second sense has no example of its own, and does not inherit one.
        assert!(entry.definitions[1].examples.is_empty());
    }

    #[test]
    fn attribution_and_audio_are_not_definition_text() {
        let entry = parse_term(&jitendex_shaped()).expect("entry");
        for definition in &entry.definitions {
            for gloss in &definition.glosses {
                assert!(!gloss.contains("JMdict"), "attribution leaked: {gloss:?}");
            }
        }
    }

    /// The shape **real Jitendex** uses, which none of the documentation described:
    /// `sense-groups > sense-group > ol > li(sense)`, with the part of speech hung on
    /// the sense-group, each gloss in its own `<li>`, and a footnote marker inside the
    /// example translation.
    fn jitendex_real_shape() -> Value {
        let sense = |gloss_items: Vec<&str>, extra: Value| {
            json!({
                "tag": "li",
                "data": { "content": "sense" },
                "content": [
                    { "tag": "ul", "data": { "content": "glossary" },
                      "content": gloss_items.iter()
                          .map(|gloss| json!({ "tag": "li", "content": gloss }))
                          .collect::<Vec<_>>() },
                    extra
                ]
            })
        };

        // Composed from pieces rather than one literal: a tree this deep exceeds
        // `json!`'s macro recursion limit in a single expression.
        let japanese = json!({
            "tag": "div",
            "data": { "content": "example-sentence-a" },
            "content": [
                { "tag": "ruby", "content": ["彼", { "tag": "rt", "content": "かれ" }] },
                "の命が危ない。"
            ]
        });
        let english = json!({
            "tag": "div",
            "data": { "content": "example-sentence-b" },
            "content": [
                { "tag": "span", "content": "His life is in danger." },
                { "tag": "span", "data": { "content": "attribution-footnote" }, "content": "[3]" }
            ]
        });
        let example = json!({
            "tag": "div",
            "data": { "content": "extra-info" },
            "content": [{
                "tag": "div",
                "data": { "content": "example-sentence" },
                "content": [japanese, english]
            }]
        });

        let pos = |name: &str| {
            json!({
                "tag": "span",
                "data": { "content": "part-of-speech-info", "class": "tag" },
                "content": name
            })
        };
        let group = |part_of_speech: Value, senses: Vec<Value>| {
            json!({
                "tag": "li",
                "data": { "content": "sense-group" },
                "content": [part_of_speech, { "tag": "ol", "content": senses }]
            })
        };

        let adjective = group(
            pos("adjective"),
            vec![
                sense(vec!["in danger", "in jeopardy", "at risk"], example),
                sense(
                    vec!["sketchy", "dodgy"],
                    json!({ "tag": "span", "data": { "class": "tag" }, "content": "colloquial" }),
                ),
            ],
        );
        let interjection = group(
            pos("interjection"),
            vec![sense(vec!["look out!", "be careful!"], Value::Null)],
        );

        let content = json!({
            "tag": "ul",
            "data": { "content": "sense-groups" },
            "content": [adjective, interjection]
        });

        json!([
            "危ない", "あぶない", "★", "adj-i", 200,
            [{ "type": "structured-content", "content": content }],
            1218380, ""
        ])
    }

    #[test]
    fn each_list_item_is_its_own_gloss() {
        // Joining them yields "in danger in jeopardy at risk": one unreadable line
        // where the dictionary gave three meanings.
        let entry = parse_term(&jitendex_real_shape()).expect("entry");
        assert_eq!(
            entry.definitions[0].glosses,
            ["in danger", "in jeopardy", "at risk"]
        );
    }

    #[test]
    fn a_sense_inherits_the_part_of_speech_from_its_group_but_not_from_a_sibling_group() {
        let entry = parse_term(&jitendex_real_shape()).expect("entry");
        assert_eq!(entry.definitions.len(), 3);

        // Hung on the sense-group, outside the sense it applies to.
        assert_eq!(entry.definitions[0].tags, ["adjective"]);
        // Group tag plus the sense's own.
        assert_eq!(entry.definitions[1].tags, ["adjective", "colloquial"]);
        // A different group: its senses must not pick up "adjective".
        assert_eq!(entry.definitions[2].tags, ["interjection"]);
        assert_eq!(entry.definitions[2].glosses, ["look out!", "be careful!"]);
    }

    #[test]
    fn a_footnote_marker_is_not_part_of_the_translation() {
        let entry = parse_term(&jitendex_real_shape()).expect("entry");
        let example = &entry.definitions[0].examples[0];
        assert_eq!(example.english, "His life is in danger.");
        // And the furigana is still dropped from the Japanese.
        assert_eq!(example.japanese, "彼の命が危ない。");
    }

    #[test]
    fn an_unlabelled_structured_tree_still_yields_its_text() {
        // A dictionary that uses structured content purely for formatting. The
        // honest reading is one definition, not zero.
        let term = json!([
            "hoge",
            "",
            "",
            "",
            0,
            [{
                "type": "structured-content",
                "content": { "tag": "div", "content": ["a placeholder; a thing"] }
            }],
            1,
            ""
        ]);
        let entry = parse_term(&term).expect("entry");
        assert_eq!(entry.definitions.len(), 1);
        assert_eq!(entry.definitions[0].glosses, ["a placeholder", "a thing"]);
    }

    #[test]
    fn an_unknown_label_degrades_to_text_rather_than_disappearing() {
        // `data` attributes are free-form in the schema, so a label this parser has
        // never seen must not silently swallow the definition.
        let term = json!([
            "みらい",
            "",
            "",
            "",
            0,
            [{
                "type": "structured-content",
                "content": { "tag": "div", "data": { "content": "future-label" },
                             "content": "the future" }
            }],
            1,
            ""
        ]);
        let entry = parse_term(&term).expect("entry");
        assert_eq!(entry.definitions[0].glosses, ["the future"]);
    }

    #[test]
    fn an_image_only_definition_is_dropped_but_a_text_node_is_kept() {
        let images = json!(["図", "ず", "", "", 0, [{ "type": "image", "path": "a.png" }], 1, ""]);
        assert!(parse_term(&images).unwrap().definitions.is_empty());

        let text = json!(["図", "ず", "", "", 0, [{ "type": "text", "text": "diagram" }], 1, ""]);
        assert_eq!(
            parse_term(&text).unwrap().definitions[0].glosses,
            ["diagram"]
        );
    }
}
