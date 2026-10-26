//! Does a recognised construction identify the meaning a reader needs?
//!
//! ```text
//! cargo test --release -p sakatsu-core --test expression_lookup -- --ignored --nocapture
//! ```
//!
//! # The question this has to answer before anything is built
//!
//! The colloquial pilot found `のに` and `には` in a promising state: a grammar pattern covers the
//! whole span, the dictionaries hold an entry for the expression, and token-local lookup never
//! reaches it — so the reader selecting `の` is handed "indicates possessive" while an entry reading
//! "although, despite, even though" sits unused.
//!
//! It is tempting to read that as *connect the two and the reader is served*. **It is not enough**,
//! and this file exists to find out where it breaks before any of it ships.
//!
//! `のに` is at least three different things in running text:
//!
//! | | Example | What the reader needs |
//! |---|---|---|
//! | concessive | 顔は可愛い**のに** | "although, even though" |
//! | purpose | 掃除する**のに**便利です | "in order to" |
//! | not のに at all | 独特**の**│**におい** | nothing; the string is a coincidence |
//!
//! **A correction, recorded because it nearly became a conclusion.** The first version of this file
//! said the entry carries only the concessive sense and that exposing it on a purpose usage would
//! therefore be wrong. That was an artefact of reading the first four glosses. `のに` (seq 1009980)
//! has four senses and the fourth is **"in order to"**; `には` (seq 2215430) has "in order to, for"
//! at its second. So the entry already describes both readings, and offering it on a purpose usage
//! offers a polysemous particle with its senses listed — the ordinary, correct outcome.
//!
//! That moves the risk somewhere else, and somewhere narrower. What would genuinely mislead is the
//! **coincidence**: a span where のに is not the construction at all, and an entry for it is offered
//! anyway. So the question these tests answer is not "does the pattern pick the sense" — the sense
//! list does that — but **how often the pattern fires where there is no construction**.
//!
//! # Where the material comes from
//!
//! Real utterances from the **development** side of the frozen chat partition, published under
//! CC BY-SA 4.0 by RealPersonaChat and Multi-Relational Multi-Party Chat. Harvested by searching
//! for the string and reading the results, not composed — the standing rule against inventing
//! Japanese fixtures applies, and a constructed example would prove only that the construction
//! behaves as constructed.
//!
//! No capture text appears here. `corpus/` is gitignored because the images are copyrighted and
//! some are adult.

mod common;

use common::installed_dictionaries;
use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::pipeline::Pipeline;

/// What a reader should be offered over the `のに` in each of these, as adjudicated by reading it.
///
/// **These are agent proposals.** Each is a judgment about Japanese made by reading the sentence,
/// and none has been adjudicated by a person. They are recorded so the measurement below has
/// something to be measured against, not as gold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sense {
    /// 〜のに = although, despite. The entry's sense 0.
    Concessive,
    /// 〜のに = for, in order to. **Also in the entry**, at sense 3 — which is why a purpose usage
    /// is not a failure of this approach.
    Purpose,
    /// The string is a coincidence of adjacent tokens — の plus something beginning に.
    NotTheConstruction,
}

/// Harvested from the development side of the frozen chat partition, then read and classified.
const NONI: &[(&str, Sense)] = &[
    // Concessive: something holds and the clause runs against it.
    (
        "ちゃんと千円以上出せば、20年ぐらい持つのに！",
        Sense::Concessive,
    ),
    (
        "そう。アイフォンって直感的に使えるっていわれてるのに。",
        Sense::Concessive,
    ),
    ("今だとホントは夏野菜が安いはずなのに。", Sense::Concessive),
    (
        "顔は可愛いのに、勢いで相殺された感じでした",
        Sense::Concessive,
    ),
    (
        "家事！面倒なことなのに、毎日って、大変ですよね。",
        Sense::Concessive,
    ),
    // Purpose: the clause is what the following cost, difficulty or convenience is *for*.
    ("家全部を掃除するのに便利です！", Sense::Purpose),
    (
        "ライセンスとるのにクラスをとる必要があるんでしょうかね。",
        Sense::Purpose,
    ),
    (
        "大きいプールだとお水を張るのにも時間がかかりますしね。",
        Sense::Purpose,
    ),
    ("乾かすのに大変でした。", Sense::Purpose),
    // Not the construction: の followed by a word that begins with に.
    (
        "八ツ橋って独特のにおいしますよね？",
        Sense::NotTheConstruction,
    ),
    (
        "洗面器みたいなのに入れて置くといいかもですね。",
        Sense::NotTheConstruction,
    ),
];

/// Where the `noni` pattern fires, and over what.
///
/// **An analysis failure is not an absent construction**, and an earlier version of this returned
/// an empty vector for both — so a broken analyzer would have read as "the pattern did not fire",
/// which is the shape of a measurement that cannot fail. The error is surfaced instead.
fn noni_spans(
    text: &str,
    lindera: &LinderaAnalyzer,
    pipeline: &Pipeline<'_>,
) -> Result<Vec<(usize, usize)>, String> {
    let mut analysis = lindera
        .analyze(text)
        .map_err(|error| format!("analysis failed for {text:?}: {error:?}"))?;
    pipeline.display(&mut analysis);
    Ok(sakatsu_core::grammar::patterns(&analysis)
        .into_iter()
        .filter(|found| found.pattern_id == "noni")
        .filter_map(|found| Some((found.start?, found.end?)))
        .collect())
}

/// **How often does the pattern fire where there is no construction?**
///
/// The sense split is handled by the entry, which carries both readings. What the entry cannot
/// handle is being offered for a span that is not のに at all — 独特**の**におい is の followed by a
/// noun, and an entry glossed "although, despite" over it is assistance about a word that is not
/// there.
///
/// So the concessive and purpose rows below are **not** failures: both are the construction and
/// both are in the entry. The row that decides whether this is safe to build is `NOT のに`.
///
/// This reports rather than asserts a target. The number it produces is the input to the decision,
/// and pinning an expected value before the decision is made would be pinning the conclusion.
#[test]
#[ignore = "needs the installed dictionaries"]
fn whether_the_noni_pattern_separates_the_senses() {
    let dictionaries = installed_dictionaries();
    assert!(
        !dictionaries.is_empty(),
        "this measures what the pattern does against real entries"
    );
    let pipeline = Pipeline::new(&dictionaries);
    let lindera = LinderaAnalyzer::new().expect("the dictionary is compiled in");

    println!("\n=== where does `noni` fire, and is there a construction there? ===");
    println!("  material: development-side chat utterances, harvested and then read");
    println!("  the entry carries BOTH senses (0 concessive, 3 'in order to'), so a purpose");
    println!("  usage is served by the same entry. The row that matters is `NOT のに`.\n");

    let mut fired: std::collections::BTreeMap<&str, (usize, usize)> =
        std::collections::BTreeMap::new();
    for (text, sense) in NONI {
        let spans = noni_spans(text, &lindera, &pipeline).expect("analysis and matching succeed");
        let label = match sense {
            Sense::Concessive => "concessive",
            Sense::Purpose => "PURPOSE",
            Sense::NotTheConstruction => "NOT のに",
        };
        let entry = fired.entry(label).or_insert((0, 0));
        entry.1 += 1;
        if !spans.is_empty() {
            entry.0 += 1;
        }
        let shown = spans
            .iter()
            .map(|(start, end)| text.get(*start..*end).unwrap_or("?"))
            .collect::<Vec<_>>();
        println!(
            "  {:<11} noni {:<5} {:?}  {}",
            label,
            if spans.is_empty() { "no" } else { "FIRES" },
            shown,
            text
        );
    }

    println!("\n  summary — how often `noni` fires, by the sense a reader would need:");
    for (label, (fires, total)) in &fired {
        println!("    {label:<11} {fires} of {total}");
    }
    let (false_fires, false_total) = fired.get("NOT のに").copied().unwrap_or((0, 0));
    println!(
        "\n  Firing on both senses is fine: the entry lists both. The cost is the {false_fires} of \
         {false_total} case(s)\n  where のに is not the construction and an entry would be offered \
         for it anyway."
    );
}

/// Every sense the dictionaries record for these expressions, not the first few.
///
/// The measurement above says the pattern cannot separate のに's senses. Whether that matters
/// depends on something not yet checked: **if the entry itself carries the purpose sense**, then
/// exposing the entry is not exposing a wrong gloss — it is exposing a word with more than one
/// meaning, which is the ordinary case and what a sense list is for.
///
/// Truncating to four glosses is exactly how that distinction gets lost, so this prints all of them.
#[test]
#[ignore = "needs the installed dictionaries"]
fn every_sense_these_expressions_actually_carry() {
    let dictionaries = installed_dictionaries();
    println!("\n=== what the dictionaries record, in full ===");
    for expression in ["のに", "には", "とか", "よね", "かな"] {
        println!("\n  {expression}");
        for dictionary in &dictionaries {
            let Ok(hits) = dictionary.lookup(expression) else {
                continue;
            };
            for hit in hits.iter().filter(|hit| hit.entry.term == expression) {
                println!(
                    "    [{}] {} 【{}】 seq {}",
                    dictionary.meta().title,
                    hit.entry.term,
                    hit.entry.reading,
                    hit.entry.sequence
                );
                for (index, definition) in hit.entry.definitions.iter().enumerate() {
                    println!(
                        "      sense {index}: {:?}  tags {:?}",
                        definition.glosses, definition.tags
                    );
                }
            }
        }
    }
}

/// What the tokens actually are where `noni` fires, so exclusion can be designed rather than hoped.
///
/// The coincidence and the genuine concessive both follow な, so "look for な before it" would
/// reject both. This prints the token sequence around each match, which is the evidence that says
/// whether any structural test separates them at all.
#[test]
#[ignore = "needs the installed dictionaries"]
fn what_the_tokens_are_where_noni_fires() {
    let dictionaries = installed_dictionaries();
    let pipeline = Pipeline::new(&dictionaries);
    let lindera = LinderaAnalyzer::new().expect("the dictionary is compiled in");

    println!("\n=== token sequences around every `noni` match ===");
    for (text, sense) in NONI {
        let Ok(mut analysis) = lindera.analyze(text) else {
            println!("  ANALYSIS FAILED: {text}");
            continue;
        };
        pipeline.display(&mut analysis);
        let matches: Vec<_> = sakatsu_core::grammar::patterns(&analysis)
            .into_iter()
            .filter(|found| found.pattern_id == "noni")
            .collect();
        if matches.is_empty() {
            println!("  {sense:?} — no match: {text}");
            continue;
        }
        for found in matches {
            let from = found.first_token.saturating_sub(2);
            let window: Vec<String> = analysis.tokens
                [from..found.last_token_end.min(analysis.tokens.len())]
                .iter()
                .enumerate()
                .map(|(offset, token)| {
                    let at = from + offset;
                    let mark = if at >= found.first_token { "*" } else { " " };
                    // `Token` carries no part of speech — the normalized model deliberately does
                    // not — so what is printed is the dictionary form, which is what distinguishes
                    // a nominaliser の from the のに particle in the cases below.
                    format!("{mark}{}<{}>", token.surface, token.dict_form)
                })
                .collect();
            println!("  {sense:?}");
            println!(
                "      tokens {}..{}  {}",
                found.first_token,
                found.last_token_end,
                window.join(" ")
            );
        }
    }
}

/// `には`, adjudicated independently — the contextual investigation above principally tested `のに`.
///
/// Harvested the same way, from the development side. What is notable is what is **absent**: every
/// occurrence found is the genuine fused particle, and no coincidence shape turned up. That is a
/// reported observation over this sample, not a claim that none exists.
const NIWA: &[(&str, Sense)] = &[
    ("実はシーには行ったこと無いんですよー。", Sense::Concessive),
    ("楽天市場には頭が上がらない", Sense::Concessive),
    ("子供にはわかりやすいです！", Sense::Concessive),
    (
        "でも健康のためには野菜って外せない感じがして",
        Sense::Purpose,
    ),
    (
        "筋肉痛にはストレッチポールなどもおすすめです。",
        Sense::Concessive,
    ),
    ("お家の近くには川はありますか？", Sense::Concessive),
];

/// **The whole offer, end to end, against the adjudicated set.**
///
/// The printing tests above are diagnostics. This one asserts, because the decisions it covers are
/// the ones the feature rests on:
///
/// - every component of a recognised construction reaches the expression;
/// - the reproduced `なの` + `に` coincidence gets **nothing**;
/// - `には` is covered on its own evidence rather than by assuming it behaves like `のに`;
/// - an analysis or lookup failure is reported, never counted as an absence.
///
/// Three outcomes are counted separately, because they are three different things: an offer, a
/// decline by the `な`+`の`+`に` rule, and **no match at all** — which is the analyzer saying there
/// is no such construction here, not this feature declining one.
#[test]
#[ignore = "needs the installed dictionaries"]
fn the_offer_reaches_genuine_constructions_and_declines_the_coincidence() {
    let dictionaries = installed_dictionaries();
    assert!(!dictionaries.is_empty(), "the offer is a dictionary answer");
    let pipeline = Pipeline::new(&dictionaries);
    let lindera = LinderaAnalyzer::new().expect("the dictionary is compiled in");

    let (mut offered, mut declined, mut unmatched) = (0usize, 0usize, 0usize);

    println!(
        "
=== the offer, over the adjudicated set ==="
    );
    for (text, sense) in NONI.iter().chain(NIWA.iter()) {
        let wanted = if NIWA.iter().any(|(candidate, _)| candidate == text) {
            "particle-niwa"
        } else {
            "noni"
        };
        let mut analysis = lindera
            .analyze(text)
            .unwrap_or_else(|error| panic!("analysis failed for {text:?}: {error:?}"));
        pipeline.display(&mut analysis);
        let matches = sakatsu_core::grammar::patterns(&analysis);
        let Some(found) = matches.iter().find(|found| found.pattern_id == wanted) else {
            unmatched += 1;
            println!("  no {wanted} match at all                      {text}");
            continue;
        };

        // **Every component of the match**, not one chosen by surface — which is how an earlier
        // version of this test picked the は in 川は and blamed the feature for its own mistake.
        let mut offers = Vec::new();
        for index in found.first_token..found.last_token_end {
            offers.push(
                sakatsu_core::expression::offered(&analysis.tokens, index, &matches, &dictionaries)
                    .unwrap_or_else(|error| panic!("lookup failed for {text:?}: {error:?}")),
            );
        }

        match (offers.first().and_then(Option::as_ref), sense) {
            (Some(offer), Sense::NotTheConstruction) => panic!(
                "the coincidence must not be offered {:?} for {text:?}",
                offer.expression
            ),
            (Some(offer), _) => {
                offered += 1;
                assert!(
                    offers.iter().all(Option::is_some),
                    "every component of {text:?} must reach the expression, not just the first"
                );
                assert!(
                    !offer.senses.is_empty(),
                    "an offer with no senses is not an answer"
                );
                assert_eq!(
                    text.get(offer.start..offer.end),
                    Some(offer.expression.as_str()),
                    "the offer must be anchored to the text it names"
                );
                println!(
                    "  OFFER  {:<4} {} sense(s), {} component(s)  {}",
                    offer.expression,
                    offer.senses.len(),
                    offers.len(),
                    text
                );
            }
            (None, Sense::NotTheConstruction) => {
                declined += 1;
                println!("  declined (correctly)                        {text}");
            }
            (None, _) => {
                declined += 1;
                println!("  declined by the な+の+に rule                 {text}");
            }
        }
    }

    assert!(
        offered >= 8,
        "only {offered} offered; the feature is barely exercised"
    );
    assert_eq!(
        declined, 3,
        "one coincidence plus the two genuine concessives the な+の+に rule gives up"
    );
    assert_eq!(
        unmatched, 2,
        "独特のにおい has no に token, and ためには composes に into ために — neither is this          feature declining anything"
    );
    println!(
        "
  {offered} offered, {declined} declined, {unmatched} with no match at all"
    );
}

/// **Either component reaches the expression, and the component itself stays reachable.**
#[test]
#[ignore = "needs the installed dictionaries"]
fn both_components_reach_the_expression_and_the_component_answer_survives() {
    let dictionaries = installed_dictionaries();
    let pipeline = Pipeline::new(&dictionaries);
    let lindera = LinderaAnalyzer::new().expect("the dictionary is compiled in");
    let text = "顔は可愛いのに、勢いで相殺された感じでした";

    let mut analysis = lindera.analyze(text).expect("analyzes");
    pipeline.display(&mut analysis);
    let matches = sakatsu_core::grammar::patterns(&analysis);

    let no = analysis
        .tokens
        .iter()
        .position(|token| token.surface == "の")
        .expect("の is a token");
    let ni = analysis
        .tokens
        .iter()
        .position(|token| token.surface == "に")
        .expect("に is a token");

    for index in [no, ni] {
        let offer =
            sakatsu_core::expression::offered(&analysis.tokens, index, &matches, &dictionaries)
                .expect("lookup succeeds")
                .expect("both components reach the expression");
        assert_eq!(offer.expression, "のに");
        assert_eq!(text.get(offer.start..offer.end), Some("のに"));

        // **The component answer is untouched.** The offer is beside it, never instead of it.
        let component = sakatsu_core::lookup::word(&analysis.tokens[index], &dictionaries)
            .expect("the component still answers");
        assert!(
            !component.inline.is_empty(),
            "selecting {:?} must still return its own entry",
            analysis.tokens[index].surface
        );
    }

    // And token boundaries are what they were: nothing was joined to make this work.
    assert_eq!(
        analysis.tokens[no].surface, "の",
        "the offer must not regroup the tokens"
    );
    assert_eq!(analysis.tokens[ni].surface, "に");
}

/// Diagnostic: the tokens and matches for the `には` cases that declined.
#[test]
#[ignore = "needs the installed dictionaries"]
fn why_the_niwa_cases_declined() {
    let dictionaries = installed_dictionaries();
    let pipeline = Pipeline::new(&dictionaries);
    let lindera = LinderaAnalyzer::new().expect("the dictionary is compiled in");
    println!("\n=== には: tokens and matches ===");
    for (text, _) in NIWA {
        let mut analysis = lindera.analyze(text).expect("analyzes");
        pipeline.display(&mut analysis);
        let matches = sakatsu_core::grammar::patterns(&analysis);
        let niwa: Vec<String> = matches
            .iter()
            .filter(|found| found.pattern_id == "particle-niwa")
            .map(|found| {
                format!(
                    "{}..{} @{:?}..{:?}",
                    found.first_token, found.last_token_end, found.start, found.end
                )
            })
            .collect();
        let tokens: Vec<String> = analysis
            .tokens
            .iter()
            .enumerate()
            .map(|(at, token)| format!("{at}:{}", token.surface))
            .collect();
        println!("  {text}");
        println!("    tokens  {}", tokens.join(" "));
        println!("    niwa    {niwa:?}");
    }
}
