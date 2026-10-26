//! Is what the grammar packs *say* true, and said the same way twice?
//!
//! ```text
//! cargo test -p sakatsu-core --test grammar_accuracy                       # the gates
//! cargo test -p sakatsu-core --test grammar_accuracy -- --ignored --nocapture   # the reports
//! ```
//!
//! # Why this exists
//!
//! Five test files already ask whether the grammar *matcher* works — whether a rule fires
//! (`grammar_coverage`), whether it over-fires (`pack_precision`), whether both analyzers agree
//! (`jlpt_coverage`), whether a rule can find its own examples (`pack_authoring`), whether a
//! particle sense still names a real dictionary line (`particle_glosses`).
//!
//! None of them asks whether the **content is correct**. A rule can fire perfectly on the right
//! sentence and then explain the grammar wrongly, and until this file nothing would have noticed.
//! 872 entries ship; 828 of them were imported rather than written here.
//!
//! # Licensing — read before adding to this file
//!
//! Imabi, Tae Kim and Pomax are **all rights reserved**: link only, never copy (`project notes` §8,
//! and `grammar/pack.rs` says it again where the `Reference` type is defined). Consulting them to
//! decide what is true is fine and is how the citations in `core.json` were arrived at. Pasting
//! their sentences — prose *or* example sentences — into this repo is not.
//!
//! So every check here is **the pack against itself**, which needs no outside text at all. What
//! those sources settled — which of two levels is conventional, whether an explanation is true —
//! was decided by reading them and is recorded in `local documentation`. They are credited
//! once on the About page (`credits::Group::Consulted`) rather than linked from each entry.
//!
//! Example sentences, if any are ever needed here, come from Jitendex (CC BY-SA 4.0, already on
//! disk, already attributed) and are chosen by reading the English — never invented.

use std::collections::BTreeMap;

use sakatsu_core::grammar::pack::{self, GrammarPattern};

/// Every pattern in every compiled-in pack, with the pack it came from.
fn every_pattern() -> Vec<(&'static str, &'static GrammarPattern)> {
    pack::all()
        .into_iter()
        .flat_map(|p| {
            let name: &'static str = Box::leak(p.name.clone().into_boxed_str());
            p.patterns.iter().map(move |pattern| (name, pattern))
        })
        .collect()
}

/// A title reduced to the grammar it names.
///
/// Titles are written `"～としたら (〜to shitara)"` — the point, then a romaji gloss. The gloss is
/// a rendering of the same thing, so it carries no information the comparison wants, and it is
/// spelled inconsistently enough (`〜to shitara`, `to shitara`, `~to shitara`) to defeat a
/// straight equality test on its own.
///
/// The wave dash is stripped for the same reason and one more: `～` (U+FF5E) and `〜` (U+301C) are
/// different characters that render almost identically, and the packs use both.
fn spine(title: &str) -> String {
    let without_gloss = match title.rfind('(') {
        // Only when the parenthesis actually closes at the end — a title like `Noun (A) のこと`
        // is not a gloss and must keep its brackets.
        Some(open) if title.trim_end().ends_with(')') => &title[..open],
        _ => title,
    };
    without_gloss
        .chars()
        .filter(|c| !matches!(c, '～' | '〜' | '~' | '、' | ',' | '。' | ' ' | '　'))
        .collect()
}

/// A formation string reduced to its shape, so `A + B / C + D` and `A + B, C + D` compare equal.
///
/// The delimiter genuinely carries no meaning here — the two `から` entries below use `/` and `,`
/// for the identical list — and treating it as if it did would let a pure formatting difference
/// hide a real contradiction.
fn shape(formation: Option<&String>) -> String {
    formation
        .map(|f| {
            f.chars()
                .filter(|c| !matches!(c, ' ' | '　' | '～' | '〜' | '~'))
                .map(|c| if c == '/' || c == '、' { ',' } else { c })
                .collect()
        })
        .unwrap_or_default()
}

/// **Two entries that teach the same point must not disagree about its level.**
///
/// This is an invariant rather than a judgement: whatever the right JLPT level for 〜わけだ is, the
/// app cannot coherently say N3 in one place and N2 in another, and a learner sorting by level
/// meets it twice.
///
/// It is a gate rather than a report because the failure is not a matter of degree. Nine of these
/// shipped — every one a pair of entries with the same title *and* the same formation, differing
/// only in level and in how the explanation was paraphrased. They came from an import that merged
/// two level-tagged lists without reconciling them.
///
/// Same title *and* same formation, deliberately. Title alone is too loose: 〜から the reason
/// marker and から the starting-point particle are genuinely different points that share a name,
/// and a gate that could not tell them apart would be one nobody could keep green.
#[test]
fn no_two_entries_teach_the_same_point_at_different_levels() {
    let mut seen: BTreeMap<(String, String), Vec<&GrammarPattern>> = BTreeMap::new();
    for (_, pattern) in every_pattern() {
        seen.entry((spine(&pattern.title), shape(pattern.formation.as_ref())))
            .or_default()
            .push(pattern);
    }

    let mut clashes = Vec::new();
    for ((title, _), group) in &seen {
        let levels: std::collections::BTreeSet<_> = group
            .iter()
            .map(|p| p.jlpt.as_deref().unwrap_or("—"))
            .collect();
        if group.len() > 1 && levels.len() > 1 {
            let who: Vec<String> = group
                .iter()
                .map(|p| format!("{} ({})", p.id, p.jlpt.as_deref().unwrap_or("—")))
                .collect();
            clashes.push(format!("{title}: {}", who.join(" vs ")));
        }
    }

    assert!(
        clashes.is_empty(),
        "the same grammar point is taught at two different levels:\n  {}\n\n\
         Resolve by keeping one entry — prefer the one carrying a match rule, so nothing stops \
         being findable — and cite the level in its `reference`.",
        clashes.join("\n  ")
    );
}

// ---- the reports ---------------------------------------------------------------------------

/// The stem names a formation string mentions, as written.
fn descriptors(formation: &str) -> Vec<String> {
    formation
        .split(['+', ',', '、', '/', '。'])
        .map(str::trim)
        .filter(|part| {
            part.starts_with("Verb")
                || part.starts_with("Adjective")
                || part.starts_with("い-Adjective")
                || part.starts_with("な-Adjective")
        })
        .map(str::to_string)
        .collect()
}

/// The idea a stem name is reaching for, with the spelling washed out.
///
/// `Verb-て`, `Verb-てform`, `Verb て-form`, `Verb-te` and `Verb-て-form` are one thing written
/// five ways. This maps all of them to `verb:て` so the report can count the spellings.
fn idea(descriptor: &str) -> String {
    let mut lowered: String = descriptor.to_lowercase();
    // "stem" is NOT junk — `Verb-stem` is the ます-stem and bare `Verb` is not, and the negative
    // stem (未然形, 書か-) is not the ない form. Stripping it merged three distinct claims.
    for junk in [" ", "-", "(", ")", "form"] {
        lowered = lowered.replace(junk, "");
    }
    let (kind, rest) = if let Some(rest) = lowered.strip_prefix("verb") {
        ("verb", rest)
    } else if let Some(rest) = lowered.strip_prefix("い-adjective") {
        ("i-adj", rest)
    } else if let Some(rest) = lowered.strip_prefix("な-adjective") {
        ("na-adj", rest)
    } else {
        ("other", lowered.as_str())
    };

    // The synonym table. Each group is one grammatical idea the packs name several ways.
    //
    // **`casual` and `dictionary` are deliberately not merged**, though an earlier version of this
    // did merge them and reported 221 entries as one inconsistency. They are different claims:
    // the plain form is any non-polite form, the dictionary form is specifically 辞書形. 〜から
    // takes 降ったから, which is plain and is not the dictionary form — so an entry saying
    // `Verb-casual` and one saying `Verb-dictionary form` may both be right, and rewriting one
    // into the other would be changing what the pack teaches rather than how it spells it.
    let sense = match rest {
        "て" | "te" | "って" => "て",
        "ます" | "masu" => "ます",
        "ない" | "nai" | "negative" => "ない",
        "casual" | "plain" => "plain",
        "dictionary" | "dict" | "る" => "dictionary",
        "た" | "past" | "casualpast" => "た",
        "volitional" | "よう" => "volitional",
        "ば" | "provisional" => "ば",
        other => other,
    };
    // The bare `Verb`/`Verb-stem` cases, which say only "a verb goes here".
    let sense = if sense.is_empty() { "bare" } else { sense };
    format!("{kind}:{sense}")
}

/// **One grammatical idea, spelled one way.**
///
/// A reader meeting three entries in a row should not see the ます-stem called `Verb-ますstem`,
/// `Verb-ます stem` and `Verb (ます-stem)`. This does not decide which spelling is right — that is
/// a house-style call — it reports every idea the packs name more than one way, commonest first,
/// so the choice is made once and applied.
///
/// It is a report and not a gate because there are roughly a hundred offenders and no single
/// correct answer; a gate here would be an opinion with a build behind it.
#[test]
#[ignore = "a report to read, not a pass/fail"]
fn one_idea_should_be_spelled_one_way() {
    let mut spellings: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for (_, pattern) in every_pattern() {
        let Some(formation) = &pattern.formation else {
            continue;
        };
        for descriptor in descriptors(formation) {
            *spellings
                .entry(idea(&descriptor))
                .or_default()
                .entry(descriptor)
                .or_default() += 1;
        }
    }

    let mut split: Vec<_> = spellings
        .into_iter()
        .filter(|(_, ways)| ways.len() > 1)
        .collect();
    split.sort_by_key(|(_, ways)| std::cmp::Reverse(ways.values().sum::<usize>()));

    println!("\n  ideas the packs spell more than one way:\n");
    for (idea, ways) in &split {
        let total: usize = ways.values().sum();
        println!(
            "  {idea}  — {} spellings across {total} entries",
            ways.len()
        );
        let mut by_use: Vec<_> = ways.iter().collect();
        by_use.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        for (spelling, n) in by_use {
            println!("      {n:4}  {spelling}");
        }
        println!();
    }
    println!("  {} ideas are spelled inconsistently", split.len());
}

/// **An entry should be able to show what it is talking about.**
///
/// Every entry carries example sentences. The pattern it teaches ought to appear in at least one
/// of them — an entry whose examples do not contain its own grammar cannot illustrate itself, and
/// on the Reference page the examples are most of what a learner sees.
///
/// This reaches where `pack_authoring.rs` cannot. That harness verifies a *match rule* against the
/// entry's examples, so it can say nothing at all about the 453 entries that have no rule — which
/// is 55% of the imported pack.
///
/// # Two things this deliberately does not treat as failures
///
/// **A pattern with placeholders is discontinuous.** `A と B と どちら` never appears as a run of
/// characters in any sentence, because A and B stand for words. So the pattern is cut at every
/// placeholder and the fragments are looked for *in order* — `と`, then `と`, then `どちら`.
/// Searching for the pattern whole reported a fifth of the pack as broken, all of it this.
///
/// **A pattern written in kana may be written in kanji in the example.** `A が いちばん` is
/// illustrated by 寿司が一番好きです, which is the same grammar and does not contain いちばん. Some
/// residue of this is unavoidable without readings for the example sentences, which the pack does
/// not carry — so a few entries here are correct and the report is wrong about them, and that is
/// cheaper than the alternative of asserting nothing.
#[test]
#[ignore = "a report to read, not a pass/fail"]
fn every_entry_can_illustrate_itself() {
    /// The placeholders a pattern uses for "a word goes here", longest first so that
    /// `い-Adjective` is consumed before `Adjective` can match inside it.
    const PLACEHOLDERS: &[&str] = &[
        "い-Adjective",
        "な-Adjective",
        "Adjective",
        "Sentence 1",
        "Sentence 2",
        "Sentence A",
        "Sentence B",
        "Sentence",
        // 文 is "sentence" — the pack uses it where other entries write `Sentence`, which is
        // itself one of the inconsistencies the sibling report counts.
        "文A",
        "文B",
        "文1",
        "文2",
        "Noun1",
        "Noun2",
        "Noun A",
        "Noun B",
        "Noun",
        // Before the bare `Verb`: `Verb る` means "the verb in dictionary form", where る stands
        // for whatever that verb's ending is — 解く in the example, not 解る.
        "Verb る",
        "Verb",
        "A",
        "B",
    ];

    let mut orphans = Vec::new();
    let mut considered = 0usize;

    for (source, pattern) in every_pattern() {
        let mut cut = pattern.pattern.clone();
        for placeholder in PLACEHOLDERS {
            cut = cut.replace(placeholder, "\u{0}");
        }
        // **The wave dash is a placeholder too**, and missing that reported half of N5 as broken.
        // `あまり～ありません` is not a run of characters — the ～ is where 疲れてい goes. A leading
        // one means only "this attaches to what precedes it" and cuts nothing, which falls out for
        // free: it produces an empty first fragment, and empty fragments are dropped.
        //
        // A space is a gap for the same reason: the pack writes `～に あげます`, and the sentence
        // that illustrates it is 私は友達に本をあげました — に and あげ are not adjacent, and the
        // space is exactly where 本を goes. Cutting there can only turn a reported failure into a
        // pass, never the reverse, since the fragments are still required in order. That makes the
        // check more permissive than it looks, which is the right trade for a report whose job is
        // to hand a human a short list worth reading.
        for gap in ['～', '〜', '~', ' ', '　', '…', '‥'] {
            cut = cut.replace(gap, "\u{0}");
        }
        let fragments: Vec<String> = cut
            .split('\u{0}')
            .map(|fragment| {
                fragment
                    .chars()
                    .filter(|c| !matches!(c, ' ' | '　' | '、' | '。' | ',' | '+' | ':'))
                    .collect::<String>()
            })
            // A fragment with no Japanese in it is prose, not pattern text — the title
            // `い-Adjective: Negative Polite Form` describes a form rather than spelling one.
            .filter(|fragment| fragment.chars().any(sakatsu_core::kana::is_japanese))
            // **A pattern is cited in one form and illustrated in another.** 〜にあげます is shown
            // as 本をあげました; 〜てあげる as 持ってあげた; 〜ことがある as 行ったことがあります.
            // The citation form is a convention of how grammar is written down, not a claim that
            // the sentence must contain those exact characters — so the final inflecting kana is
            // dropped and the stem is what gets looked for.
            //
            // This is the loosest thing the check does. It is still worth having: every entry it
            // now passes was one a human would have had to read and dismiss.
            .map(|fragment| {
                let trimmed = fragment
                    .strip_suffix("ます")
                    .map(str::to_string)
                    .or_else(|| {
                        fragment
                            .strip_suffix(['る', 'う', 'く', 'ぐ', 'す', 'つ', 'ぬ', 'ぶ', 'む'])
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| fragment.clone());
                // Never trim away the whole fragment — `を` + `` would match everything.
                if trimmed.chars().any(sakatsu_core::kana::is_japanese) {
                    trimmed
                } else {
                    fragment
                }
            })
            .collect();

        // Nothing left to look for once the placeholders are gone — the pattern is a shape
        // (`Noun + Noun`) rather than a string, and this check has no opinion about it.
        if fragments.is_empty() || pattern.examples.is_empty() {
            continue;
        }
        considered += 1;

        let shown = pattern.examples.iter().any(|example| {
            let japanese: String = example
                .japanese
                .chars()
                .filter(|c| !matches!(c, ' ' | '　'))
                .collect();
            // Every fragment, in the order the pattern writes them.
            let mut from = 0usize;
            for fragment in &fragments {
                match japanese[from..].find(fragment.as_str()) {
                    Some(at) => from += at + fragment.len(),
                    None => return false,
                }
            }
            true
        });
        if !shown {
            orphans.push((source, pattern, fragments));
        }
    }

    println!("\n  entries whose own examples do not contain their pattern:\n");
    for (source, pattern, fragments) in orphans.iter().take(50) {
        println!(
            "  {:3} {:44} {}",
            pattern.jlpt.as_deref().unwrap_or("—"),
            pattern.id,
            pattern.title
        );
        println!(
            "        looked for {fragments:?} in {} examples ({source})",
            pattern.examples.len()
        );
        if let Some(first) = pattern.examples.first() {
            println!("        e.g. {}", first.japanese);
        }
    }
    println!(
        "\n  {} of {considered} entries cannot illustrate themselves ({:.1}%)",
        orphans.len(),
        100.0 * orphans.len() as f64 / considered.max(1) as f64
    );
}

/// **How much of the explanatory text is boilerplate, and how much is duplicated.**
///
/// Two measures, both of them about whether the words are worth reading:
///
/// - a `detail` that opens with a stock frame — "The X grammar point is used to express…" — is
///   the import's template rather than an explanation, and 71% of them did at the time of writing;
/// - an `explanation` that appears verbatim on another entry means two distinct grammar points are
///   being given the same meaning. 〜によらず and 〜のいかんにかかわらず were both "regardless of,
///   irrespective of", which is true of both and tells a learner nothing about choosing between
///   them — the difference is entirely register.
#[test]
#[ignore = "a report to read, not a pass/fail"]
fn explanations_should_be_written_rather_than_generated() {
    let patterns = every_pattern();

    let stock = |detail: &str| {
        let d = detail.trim_start();
        d.starts_with("The ") && d.contains("grammar point is used")
            || d.starts_with("This grammar point")
    };
    let templated = patterns
        .iter()
        .filter(|(_, p)| p.detail.as_deref().is_some_and(stock))
        .count();
    let with_detail = patterns.iter().filter(|(_, p)| p.detail.is_some()).count();

    println!(
        "\n  detail written from a template: {templated} of {with_detail} ({:.0}%)",
        100.0 * templated as f64 / with_detail.max(1) as f64
    );

    let mut by_explanation: BTreeMap<String, Vec<&GrammarPattern>> = BTreeMap::new();
    for (_, pattern) in &patterns {
        by_explanation
            .entry(pattern.explanation.trim().to_lowercase())
            .or_default()
            .push(pattern);
    }
    let shared: Vec<_> = by_explanation
        .iter()
        .filter(|(text, group)| group.len() > 1 && !text.is_empty())
        .collect();

    println!("\n  explanations used on more than one entry:\n");
    for (text, group) in &shared {
        println!("  \"{text}\"");
        for pattern in group.iter() {
            println!(
                "      {:3} {:44} {}",
                pattern.jlpt.as_deref().unwrap_or("—"),
                pattern.id,
                pattern.title
            );
        }
        println!();
    }
    println!(
        "  {} texts shared across {} entries",
        shared.len(),
        shared.iter().map(|(_, g)| g.len()).sum::<usize>()
    );
}
