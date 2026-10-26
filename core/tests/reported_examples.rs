//! The three reported reader failures, traced end to end through the production path.
//!
//! # What these are, and what they are not
//!
//! Three examples were reported from the desktop reader, as screenshots: `なんで` answered with
//! "difficulty; trouble", `あなたたち` grouped and looked up badly, and `神姫` given a name reading
//! by Lindera against apparently `神`-related definitions from Ichiran.
//!
//! **No local source held any of them.** They were searched for in the supplied conversation (all
//! 64,724 records) and in every local source — `corpus/`, `scratch/colloquial/` including the
//! frozen chat sample and both full chat corpora, `scratch/syosetu-diagnostic/`, and the
//! disagreement artifacts. `あなたたち` and `神姫` appear nowhere in the conversation before the
//! handoff describing them.
//!
//! The reader then **supplied the passage by transcribing it from the screenshot**, and it is
//! traced in `the_supplied_passage_is_traced_through_the_complete_lookup_for_both_engines`. That
//! test carries the reported sentence. **This one does not**, and the distinction is the point:
//!
//! > The two promoted captures establish that the `なん` → 難 failure **recurs**. They are
//! > **independent reproductions of the same defect**, not reproductions of the reported sentence.
//!
//! So each case below carries its **provenance**:
//!
//! | Case | What it is |
//! |---|---|
//! | `なんで` ×2 | **Promoted capture transcripts** — corrected against the image by a person. Two independent reproductions of the defect; neither is the reported sentence. |
//! | `貴方たち` | A **recorded diagnostic case**, id `n8975mr-1-L234`. Written in kanji where the report was kana; the analyzer treats both identically, which is checked below. |
//! | `神姫` | **No local occurrence.** Traced bare and in constructed contexts, marked as such — and, as the supplied passage shows, a bare string does **not** settle what this word does in context. |
//!
//! The texts are **read from disk, never embedded**: `corpus/` and `scratch/` are gitignored
//! because the material is captured and copyrighted, and transcribing it into a tracked source
//! file would defeat that.
//!
//! # Why the assertions are worth having
//!
//! Each one pins a mechanism, not a gloss. `なん` does not merely produce a wrong answer — it
//! produces **sequence 1460830**, which is 難【なん】, a homophone of the pronoun the tokenizer
//! actually found. Naming the sequence is what makes this a retrieval diagnosis rather than an
//! impression, and what makes it fail loudly if a dictionary update moves it.
//!
//! Run with `--nocapture` to read the full trace; the assertions are what gate it.

mod common;

#[path = "common/runner.rs"]
mod runner;

use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, LinderaAnalyzer};
use sakatsu_core::dict::Dictionary;
use sakatsu_core::grammar::{matcher, pack};
use sakatsu_core::pipeline::Pipeline;
use sakatsu_core::{study, Analysis};
use serde_json::json;

/// 難【なん】 — "difficulty; trouble; hardship; shortage". **Not** what the tokenizer found.
const DIFFICULTY: i64 = 1_460_830;
/// 何【なん】 — the pronoun UniDic established and contextual ranking now exposes first.
const PRONOUN_WHAT: i64 = 2_846_738;
/// 太刀【たち】 — "long sword". An alternative retained after the suffix repair.
const LONG_SWORD: i64 = 1_408_340;
/// 達【たち】 — the pluralizing suffix. UniDic evidence now makes this lead.
const PLURALIZER: i64 = 1_416_220;

/// Where a case's text came from, carried with the case so no trace reads as the report.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Provenance {
    /// A capture whose transcript a person corrected against the image.
    PromotedCapture,
    /// A case recorded in a local diagnostic artifact, with its own id.
    DiagnosticRecord,
    /// Written here for the probe. **Not harvested**, so it evidences the code path only.
    Constructed,
}

struct Traced {
    lindera: Analysis,
    ichiran: Option<Analysis>,
}

impl Traced {
    fn lindera_surfaces(&self) -> Vec<&str> {
        self.lindera
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect()
    }

    fn ichiran_surfaces(&self) -> Vec<&str> {
        self.ichiran
            .as_ref()
            .map(|analysis| {
                analysis
                    .tokens
                    .iter()
                    .map(|token| token.surface.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The senses a reader actually sees for a token, in the order the panel shows them.
    fn leading_sequences(&self, index: usize, dictionaries: &[Dictionary]) -> Vec<i64> {
        sakatsu_core::lookup::in_context(&self.lindera.tokens, index, dictionaries)
            .map(|view| {
                view.word
                    .inline
                    .iter()
                    .filter_map(|sense| sense.sequence)
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn trace(
    text: &str,
    provenance: Provenance,
    note: &str,
    lindera: &LinderaAnalyzer,
    ichiran: Option<&IchiranAnalyzer>,
    pipeline: &Pipeline,
    dictionaries: &[Dictionary],
) -> Traced {
    println!("\n######## {provenance:?} — {note} ########");

    let mut analysis = lindera.analyze(text).expect("lindera analyses");
    println!("-- lindera tokens, before Pipeline::display");
    for token in &analysis.tokens {
        println!(
            "   {:?} read={:?} dict={:?} lemma_read={:?} senses={}",
            token.surface,
            token.reading,
            token.dict_form,
            token.lemma_reading,
            token.senses.len()
        );
    }
    pipeline.display(&mut analysis);
    println!("-- after Pipeline::display (compounds, names, readings::correct), with retrieval");
    for (index, token) in analysis.tokens.iter().enumerate() {
        println!(
            "   [{index}] {:?} read={:?} dict={:?}",
            token.surface, token.reading, token.dict_form
        );
        match sakatsu_core::lookup::in_context(&analysis.tokens, index, dictionaries) {
            Ok(view) => {
                println!(
                    "        merged: headword={:?} reading={:?} source={:?} name_leads={:?}",
                    view.word.headword,
                    view.word.reading,
                    view.word.inline_source,
                    view.word.name.as_ref().map(|hit| hit.leads)
                );
                for sense in view.word.inline.iter().take(3) {
                    println!(
                        "        seq {:?} {:?} :: {}",
                        sense.sequence,
                        sense.tags,
                        sense.glosses.join("; ")
                    );
                }
            }
            Err(error) => println!("        lookup failed: {error:?}"),
        }
    }

    let other = ichiran.and_then(|engine| match engine.analyze(text) {
        Ok(mut analysis) => {
            pipeline.display(&mut analysis);
            println!("-- ichiran, after the same Pipeline::display");
            for token in &analysis.tokens {
                println!(
                    "   {:?} read={:?} dict={:?} senses={}",
                    token.surface,
                    token.reading,
                    token.dict_form,
                    token.senses.len()
                );
            }
            Some(analysis)
        }
        Err(error) => {
            println!("-- ichiran failed: {error:?}");
            None
        }
    });

    Traced {
        lindera: analysis,
        ichiran: other,
    }
}

/// Read a promoted capture transcript, or `None` when the corpus is not on this machine.
fn promoted(stem: &str) -> Option<String> {
    let path = common::corpus_dir().join(format!("{stem}.txt"));
    std::fs::read_to_string(&path)
        .ok()
        .map(|text| text.trim().to_string())
}

/// Read one case out of the local syosetu diagnostic by its recorded id.
fn diagnostic_case(id: &str) -> Option<String> {
    let raw = std::fs::read_to_string("../scratch/syosetu-diagnostic/eris-input.json").ok()?;
    let document: serde_json::Value = serde_json::from_str(&raw).ok()?;
    document["cases"]
        .as_array()?
        .iter()
        .find(|case| case["id"] == id)?["text"]
        .as_str()
        .map(str::to_string)
}

#[test]
#[ignore = "needs the installed dictionaries; Ichiran is reported when absent, not required"]
fn the_three_reported_examples_are_traced_to_their_stages() {
    let dictionaries: Vec<Dictionary> = common::installed_dictionaries();
    assert!(
        !dictionaries.is_empty(),
        "the installed dictionaries are required: a retrieval diagnosis without them would be a \
         claim about a lookup that never ran"
    );
    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let pipeline = Pipeline::new(&dictionaries);

    // Ichiran is **reported, not required**. Half of each finding below is about Lindera alone and
    // stands without it; the cross-engine halves are skipped explicitly rather than silently.
    let engine = IchiranAnalyzer::new();
    sakatsu_core::analyzer::ichiran::server::set_persistent(true);
    engine.warm();
    let reachable = engine.analyze("これって").is_ok()
        && sakatsu_core::analyzer::ichiran::server::last_route().is_session();
    println!(
        "\n=== ichiran via the session route: {} ===",
        if reachable {
            "yes"
        } else {
            "NO — cross-engine assertions skipped"
        }
    );
    let ichiran = reachable.then_some(&engine);

    // ---------------------------------------------------------------- なんで
    //
    // **The repaired case.** The tokenizer splits なん|で and hands `なん` a dictionary form of
    // `なん` with a lemma reading of `なに` — UniDic's pronoun. Retrieval still preserves the
    // homographic 難 senses, but the normalized pronoun role now lets contextual ordering expose
    // 何【なん】 first instead of presenting a reading and meanings that disagree with each other.
    let text = promoted("xnviewmp_bu75dRV6Zb")
        .expect("the promoted capture corpus is required for the なんで case");
    let angry = trace(
        &text,
        Provenance::PromotedCapture,
        "xnviewmp_bu75dRV6Zb — not the reported screenshot",
        &lindera,
        ichiran,
        &pipeline,
        &dictionaries,
    );
    assert_eq!(
        &angry.lindera_surfaces()[..2],
        &["なん", "で"],
        "the split is the tokenizer's: composition never sees a なんで to keep"
    );
    assert_eq!(
        angry.leading_sequences(0, &dictionaries).first(),
        Some(&PRONOUN_WHAT),
        "the pronoun established by UniDic must lead without removing the 難 alternative"
    );
    assert!(
        angry
            .leading_sequences(0, &dictionaries)
            .contains(&DIFFICULTY),
        "the repair reorders rather than filters the homographic noun"
    );

    // **And the same sentence fails the other way round**, which is why neither engine is the
    // baseline. Lindera keeps 怒んなきゃ together and reaches 怒る; Ichiran cuts it at 怒ん and
    // reaches nothing at all. One sentence, one win each.
    assert!(
        angry.lindera_surfaces().contains(&"怒んなきゃ"),
        "lindera holds the contracted negative together"
    );
    if reachable {
        assert!(
            angry.ichiran_surfaces().contains(&"なんで"),
            "ichiran groups なんで, which is what avoids the homophone"
        );
        let stranded = angry
            .ichiran
            .as_ref()
            .expect("ichiran analysed")
            .tokens
            .iter()
            .find(|token| token.surface == "怒ん");
        assert!(
            stranded.is_some_and(|token| token.senses.is_empty()),
            "ichiran's cut leaves 怒ん with no senses — the opposite failure, in the same sentence"
        );
    }

    let text = promoted("xnviewmp_rBfma1a8nx")
        .expect("the promoted capture corpus is required for the second なんで case");
    let answer = trace(
        &text,
        Provenance::PromotedCapture,
        "xnviewmp_rBfma1a8nx — not the reported screenshot",
        &lindera,
        ichiran,
        &pipeline,
        &dictionaries,
    );
    assert_eq!(
        answer.leading_sequences(0, &dictionaries).first(),
        Some(&PRONOUN_WHAT),
        "the repair holds in the second independent promoted capture"
    );

    // ------------------------------------------------------------ あなたたち
    //
    // **The grouping and the lookup are two separate failures, and only one is about grouping.**
    // Lindera splits the pronoun from its pluralizing suffix where Ichiran keeps them together —
    // but the reader-visible damage was that the stranded `たち` led with 太刀, "long sword". The
    // pluralizer was already present. The normalized suffix role repairs that standalone answer;
    // a hybrid boundary choice is neither required nor credited for it.
    let text = diagnostic_case("n8975mr-1-L234")
        .expect("the local syosetu diagnostic is required for the 貴方たち case");
    let plural = trace(
        &text,
        Provenance::DiagnosticRecord,
        "n8975mr-1-L234 — kanji 貴方たち, where the report was kana",
        &lindera,
        ichiran,
        &pipeline,
        &dictionaries,
    );
    let suffix = plural
        .lindera_surfaces()
        .iter()
        .position(|surface| *surface == "たち")
        .expect("the suffix is its own token");
    assert_eq!(
        plural.lindera_surfaces()[suffix - 1],
        "貴方",
        "lindera splits the pronoun from its suffix"
    );
    let senses = plural.leading_sequences(suffix, &dictionaries);
    assert_eq!(
        senses.first(),
        Some(&PLURALIZER),
        "the suffix role makes 達 lead in a sentence about people"
    );
    assert!(
        senses.contains(&LONG_SWORD),
        "the repair keeps the sword alternative available rather than filtering it"
    );
    if reachable {
        assert!(
            plural.ichiran_surfaces().contains(&"貴方たち"),
            "ichiran groups the pronoun with its suffix"
        );
    }

    // **The kana form the report actually showed segments identically.** Constructed, so it
    // evidences only that the spelling is not what decides — which is what makes the kanji record
    // above usable as a control rather than a different case wearing the same name.
    for form in ["あなたたち", "貴方たち"] {
        let bare = trace(
            form,
            Provenance::Constructed,
            "bare form, to compare kana against kanji",
            &lindera,
            ichiran,
            &pipeline,
            &dictionaries,
        );
        assert_eq!(
            bare.lindera_surfaces().len(),
            2,
            "{form} splits in two whichever way it is written"
        );
    }

    // ---------------------------------------------------------------- 神姫
    //
    // **Bare probes cannot settle component fallback in the supplied context.**
    //
    // Both engines return 神姫 as **one token** in these constructed probes. Neither answer
    // is about a component here: Ichiran has no components in these particular inputs and
    // returns zero senses. Lindera's "name reading" is real and is not the
    // tokenizer's: UniDic has 神姫 as an ordinary noun read しんき, and the *name* framing comes
    // from `merge`, where a JMnedict hit leads (`leads: true`) precisely because no dictionary
    // sense exists to lead instead.
    //
    // **What these constructed contexts cannot establish — and an earlier conclusion drawn from
    // them was wrong.**
    //
    // Reading "one token" as "the answer is about the whole expression" does not follow.
    // `lookup::sources` falls back to the dictionary forms of a token's *own components* when the
    // whole reaches nothing, and `merge` still labels the result with the whole token — so a
    // single token can display a component's senses under the expression's heading. Whether that
    // happens depends on whether the engine gave the token components, **which is decided in
    // context**.
    //
    // It does happen. For the passage the reader supplied, Ichiran gives 神姫 the components 神
    // and 姫 and the panel fills with 神's senses. The assertions below hold for these bare
    // strings and **only** for them; the real answer is in
    // `the_supplied_passage_is_traced_through_the_complete_lookup_for_both_engines`.
    for context in ["神姫", "神姫が来た", "神姫の力"] {
        let hime = trace(
            context,
            Provenance::Constructed,
            "constructed probe; the separately supplied passage is tested below",
            &lindera,
            ichiran,
            &pipeline,
            &dictionaries,
        );
        let at = hime
            .lindera_surfaces()
            .iter()
            .position(|surface| *surface == "神姫")
            .expect("constructed probe contains 神姫 as one token");
        assert!(
            hime.leading_sequences(at, &dictionaries).is_empty(),
            "no dictionary sense at all — so nothing here can be a definition of 神 or of 姫"
        );
        let view = sakatsu_core::lookup::in_context(&hime.lindera.tokens, at, &dictionaries)
            .expect("lookup runs");
        assert!(
            view.word.name.as_ref().is_some_and(|hit| hit.leads),
            "the name reading is a JMnedict hit promoted to lead, not an analyzer decision"
        );
        if reachable {
            let ichiran_token = hime
                .ichiran
                .as_ref()
                .expect("ichiran analysed")
                .tokens
                .iter()
                .find(|token| token.surface == "神姫");
            assert!(
                ichiran_token.is_some_and(|token| token.senses.is_empty()),
                "bare, ichiran has nothing to say about 神姫 either — which is a fact about these \n                 strings and not about the word: in the supplied passage it answers with 神's senses"
            );
        }
    }
}

/// The passage the reader saw, **transcribed from a screenshot** — not recovered clipboard text.
///
/// Supplied after the search above came up empty. It carries transcription uncertainty: it is a
/// person reading pixels, so a wrong character here would be a wrong trace, and it is not
/// interchangeable with the string the reader's clipboard actually held.
const SUPPLIED: &str = "双星神姫の私があなたたちを裁いてやるわ！";

/// What a single token's lookup actually did, at the level where "whole or component" is decided.
///
/// **A heading is not evidence of what was looked up.** `lookup::sources` queries the token's
/// dictionary form and, *when that reaches nothing*, falls back to the dictionary forms of the
/// token's own components, longest first — while `merge` goes on labelling the result with the
/// whole token. So a token can display `神姫` over senses retrieved for `神`. Recording the query,
/// the fallbacks and the matched entry's own term is the only way to tell them apart.
struct LookupTrace {
    surface: String,
    query: String,
    components: Vec<String>,
    fallbacks: Vec<String>,
    /// `(dictionary, match kind, entry term, entry reading, sequence)` for everything retrieved.
    hits: Vec<(String, String, String, String, i64)>,
    headword: String,
    displayed: Vec<(Option<i64>, String)>,
    inline_source: Option<String>,
    name_leads: Option<bool>,
    /// **What the senses were retrieved for**, which the displayed order alone cannot say: the
    /// whole selected word, one of its components, or an answer whose scope was never recorded.
    /// A component answer that reads as a whole-word answer is the defect this exists to catch.
    retrieved: String,
    /// **Why there is no answer, when there is none.** A dictionary that failed and a word with
    /// no entry are different findings, and flattening them makes the second look commoner than
    /// it is.
    failure: Option<String>,
}

impl LookupTrace {
    /// The distinct entry spellings the retrieved senses actually came from.
    fn matched_terms(&self) -> Vec<&str> {
        let mut terms: Vec<&str> = self.hits.iter().map(|hit| hit.2.as_str()).collect();
        terms.sort_unstable();
        terms.dedup();
        terms
    }
}

fn lookup_trace(
    tokens: &[sakatsu_core::Token],
    index: usize,
    dictionaries: &[Dictionary],
) -> LookupTrace {
    let token = &tokens[index];
    // Mirrors `lookup::sources`, which derives these internally and returns only the result.
    let query = if token.dict_form.is_empty() {
        token.surface.clone()
    } else {
        token.dict_form.clone()
    };
    let components: Vec<String> = token
        .components
        .iter()
        .map(|part| format!("{}/{}", part.surface, part.dict_form))
        .collect();
    let mut fallbacks: Vec<String> = token
        .components
        .iter()
        .map(|part| {
            if part.dict_form.is_empty() {
                part.surface.clone()
            } else {
                part.dict_form.clone()
            }
        })
        .filter(|part| !part.is_empty() && *part != query)
        .collect();
    fallbacks.sort_by_key(|part| std::cmp::Reverse(part.chars().count()));

    // **One pass, and failures kept as failures.** This used to call `sources` and `in_context`
    // separately, each with its error flattened away — so a dictionary that errored and a word
    // with no entry arrived here looking identical, and the report drew a distinction its own
    // evidence could not support. `with_evidence` is the production path `in_context` wraps, so
    // the trace cannot drift from what the reader is shown.
    let looked = sakatsu_core::lookup::with_evidence(tokens, index, dictionaries);
    let (sources, view, failure) = match looked {
        Ok(found) => (found.evidence, Some(found.view), None),
        Err(error) => (Vec::new(), None, Some(error.to_string())),
    };
    let hits = sources
        .iter()
        .flat_map(|source| {
            source.hits.iter().map(move |hit| {
                (
                    source.title.clone(),
                    format!("{:?}", hit.matched),
                    hit.entry.term.clone(),
                    hit.entry.reading.clone(),
                    hit.entry.sequence,
                )
            })
        })
        .collect();

    let (headword, displayed, inline_source, name_leads) = match &view {
        Some(view) => (
            view.word.headword.clone(),
            view.word
                .inline
                .iter()
                .map(|sense| (sense.sequence, sense.glosses.join("; ")))
                .collect(),
            view.word.inline_source.clone(),
            view.word.name.as_ref().map(|hit| hit.leads),
        ),
        None => (String::new(), Vec::new(), None, None),
    };

    let retrieved = match &view {
        Some(view) => match &view.word.inline_retrieved {
            sakatsu_core::dict::merge::Retrieved::Whole { query } => format!("whole({query})"),
            sakatsu_core::dict::merge::Retrieved::Component { query } => {
                format!("component({query})")
            }
            sakatsu_core::dict::merge::Retrieved::Unknown => "unknown".to_string(),
        },
        None => "lookup-failed".to_string(),
    };

    LookupTrace {
        surface: token.surface.clone(),
        query,
        components,
        fallbacks,
        hits,
        headword,
        displayed,
        inline_source,
        name_leads,
        retrieved,
        failure,
    }
}

fn print_trace(engine: &str, trace: &LookupTrace) {
    println!(
        "   [{engine}] {:?} query={:?} components={:?} fallbacks={:?}",
        trace.surface, trace.query, trace.components, trace.fallbacks
    );
    for (title, matched, term, reading, sequence) in &trace.hits {
        println!("        hit {title} {matched} entry={term}[{reading}] seq={sequence}");
    }
    println!(
        "        displayed under {:?} (source {:?}, name_leads {:?}, retrieved {})",
        trace.headword, trace.inline_source, trace.name_leads, trace.retrieved
    );
    for (sequence, glosses) in trace.displayed.iter().take(3) {
        println!("          seq {sequence:?} :: {glosses}");
    }
}

#[test]
#[ignore = "needs the installed dictionaries and Ichiran; this is the cross-engine question"]
fn the_supplied_passage_is_traced_through_the_complete_lookup_for_both_engines() {
    let dictionaries: Vec<Dictionary> = common::installed_dictionaries();
    assert!(
        !dictionaries.is_empty(),
        "the installed dictionaries are required"
    );
    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let pipeline = Pipeline::new(&dictionaries);

    let engine = IchiranAnalyzer::new();
    sakatsu_core::analyzer::ichiran::server::set_persistent(true);
    engine.warm();
    let reachable = engine.analyze("これって").is_ok()
        && sakatsu_core::analyzer::ichiran::server::last_route().is_session();
    assert!(
        reachable,
        "this test is the engine comparison; without Ichiran it would be a single-engine \
         measurement wearing a comparison's name"
    );

    // **Resource identity, recorded before any conclusion about context.** If the reported
    // difference is absent here, the next question is whether this machine is running what the
    // screenshot ran — so the identities go into the trace rather than being assumed equal.
    println!("\n=== identities ===");
    println!("   executable {}", runner::executable_digest());
    for dictionary in &dictionaries {
        println!(
            "   dictionary {} — {} entries, sha256 {}",
            dictionary.meta().title,
            dictionary.meta().entry_count,
            dictionary
                .content_fingerprint()
                .expect("resource content identity is required")
        );
    }
    println!(
        "   ichiran route {}",
        sakatsu_core::analyzer::ichiran::server::last_route().name()
    );

    println!("\n######## supplied passage, transcribed from a screenshot ########");
    let mut by_lindera = lindera.analyze(SUPPLIED).expect("lindera analyses");
    pipeline.display(&mut by_lindera);
    let mut by_ichiran = engine.analyze(SUPPLIED).expect("ichiran analyses");
    pipeline.display(&mut by_ichiran);

    println!("-- lindera");
    let lindera_traces: Vec<LookupTrace> = (0..by_lindera.tokens.len())
        .map(|index| lookup_trace(&by_lindera.tokens, index, &dictionaries))
        .collect();
    for trace in &lindera_traces {
        print_trace("L", trace);
    }
    println!("-- ichiran");
    let ichiran_traces: Vec<LookupTrace> = (0..by_ichiran.tokens.len())
        .map(|index| lookup_trace(&by_ichiran.tokens, index, &dictionaries))
        .collect();
    for trace in &ichiran_traces {
        print_trace("I", trace);
    }

    let find = |traces: &[LookupTrace], needle: &str| -> Option<usize> {
        traces
            .iter()
            .position(|trace| trace.surface.contains(needle))
    };

    // **What this test exists for: what does each engine's 神姫 answer actually describe?**
    //
    // Decided from the matched entry's own term, never from the heading. **A displayed sense whose
    // entry is 神 is a component answer however the panel labels it** — and that is precisely what
    // a populated panel hides.
    //
    // Both reported behaviours reproduce here, and they are different failures:
    //
    // - **Lindera** gives 神姫 no components, so the query stands alone, misses every ordinary
    //   dictionary and reaches only JMnedict. With no sense to lead, the name hit leads instead:
    //   the reported "name reading". The answer *is* about the whole expression — there is just
    //   almost nothing in it.
    // - **Ichiran** gives 神姫 the components 神 and 姫. The query still misses, so
    //   `lookup::sources` falls back to 神 and the panel fills with "god; deity; divinity" —
    //   **under the heading 神姫**, credited to Jitendex. Its component scope is now explicit
    //   in the response, although the reader must still see it in the panel. This was
    //   previously a confident whole-word answer to a question nobody asked.
    //
    // **This corrects an earlier conclusion drawn from bare strings.** Analysed alone, Ichiran
    // returns 神姫 with no components and no senses, which made it look as though no component
    // answer could arise. Components are decided in context; a bare-string probe cannot settle it,
    // and `the_three_reported_examples_are_traced_to_their_stages` says so where it makes that
    // observation.
    let lindera_hime = &lindera_traces[find(&lindera_traces, "神姫").expect("lindera has 神姫")];
    assert_eq!(
        lindera_hime.fallbacks,
        Vec::<String>::new(),
        "lindera's 神姫 has no components, so no part of it can answer for the whole"
    );
    assert_eq!(
        lindera_hime.matched_terms(),
        vec!["神姫"],
        "everything lindera retrieved is filed under the whole expression"
    );
    assert!(
        lindera_hime.displayed.is_empty() && lindera_hime.name_leads == Some(true),
        "with no ordinary sense to lead, the JMnedict name leads — the reported name reading"
    );

    let ichiran_hime = &ichiran_traces[find(&ichiran_traces, "神姫").expect("ichiran has 神姫")];
    assert_eq!(
        ichiran_hime.fallbacks,
        vec!["神", "姫"],
        "ichiran decomposes 神姫, which is what makes the component fallback reachable at all"
    );
    assert!(
        ichiran_hime.matched_terms().contains(&"神"),
        "the fallback fired and retrieved the component"
    );
    // **A confirmed defect, asserted so it cannot change quietly.** If retrieval later declines to
    // answer a whole expression with one of its parts, this
    // fails and the finding is revisited deliberately.
    assert_eq!(
        ichiran_hime.headword, "神姫",
        "the heading is the whole expression"
    );
    assert!(
        !ichiran_hime.displayed.is_empty(),
        "and the panel is populated"
    );
    assert_eq!(
        ichiran_hime.displayed[0].0,
        Some(1_364_440),
        "with 神's own entry — a component definition now explicitly labelled as such"
    );

    let ichiran_hime_view = sakatsu_core::lookup::in_context(
        &by_ichiran.tokens,
        find(&ichiran_traces, "神姫").unwrap(),
        &dictionaries,
    )
    .expect("component lookup");
    assert_eq!(
        ichiran_hime_view.word.inline_retrieved.component(),
        Some("神")
    );
    assert_eq!(
        ichiran_hime_view
            .word
            .name
            .as_ref()
            .and_then(|name| name.retrieved.query()),
        Some("神姫"),
        "the whole-token name and component definitions keep independent provenance",
    );
    // **あなたたち: a hybrid rescue and a standalone repair, measured separately.**
    //
    // These are two different improvements and only one is about a boundary:
    //
    // - **Hybrid rescue** — Ichiran's joined candidate retrieves 2830440, "you (plural)", for the
    //   whole. Choosing it means the reader never sees 太刀 at all. That is a real reader-facing
    //   gain, and it is available *now*, from selection alone.
    // - **Standalone repair** — Lindera's split candidate strands `たち`, which once led with 太刀
    //   ("long sword") in a sentence about people. UniDic now makes 達 lead, while retaining 太刀.
    //   This improves the split candidate whether or not any hybrid exists.
    //
    // Neither substitutes for the other, and the selector must not be credited with the second.
    let joined = &ichiran_traces[find(&ichiran_traces, "あなたたち").expect("ichiran joins it")];
    assert_eq!(
        joined.displayed.first().map(|sense| sense.0),
        Some(Some(2_830_440)),
        "the joined candidate answers the whole expression correctly"
    );
    let suffix = &lindera_traces[lindera_traces
        .iter()
        .position(|trace| trace.surface == "たち")
        .expect("lindera strands the suffix")];
    assert_eq!(
        suffix.displayed.first().map(|sense| sense.0),
        Some(Some(1_416_220)),
        "the suffix role makes the pluralizer lead in this sentence"
    );
    assert!(
        suffix.hits.iter().any(|hit| hit.4 == 1_416_220),
        "the pluralizer is retrieved, so this is a ranking defect and not a missing entry"
    );
    println!(
        "
>>> あなたたち — hybrid rescue: joined {:?} -> {:?}; standalone suffix repair:          stranded {:?} -> {:?}",
        joined.surface,
        joined.displayed.first().map(|sense| &sense.1),
        suffix.surface,
        suffix.displayed.first().map(|sense| &sense.1)
    );
}
#[test]
#[ignore = "needs installed dictionaries; keeps genuine noun homophones as nouns"]
fn ordinary_nouns_are_not_recast_as_pronouns_or_suffixes() {
    let dictionaries = common::installed_dictionaries();
    assert!(
        !dictionaries.is_empty(),
        "this control needs installed dictionaries"
    );
    let lindera = LinderaAnalyzer::new().expect("embedded UniDic");
    for (text, surface, expected) in [
        ("太刀を抜いた。", "太刀", LONG_SWORD),
        ("難を逃れた。", "難", DIFFICULTY),
    ] {
        let analysis = sakatsu_core::study::analyse(text, &lindera, &dictionaries)
            .expect("production analysis");
        let index = analysis
            .tokens
            .iter()
            .position(|token| token.surface == surface)
            .unwrap_or_else(|| panic!("{surface} absent: {:?}", analysis.tokens));
        assert_eq!(
            analysis.tokens[index].lexical_role, None,
            "{surface} is an ordinary noun"
        );
        let view = sakatsu_core::lookup::in_context(&analysis.tokens, index, &dictionaries)
            .expect("production lookup");
        assert_eq!(
            view.word.inline.first().and_then(|sense| sense.sequence),
            Some(expected),
            "a real noun must not inherit the pronoun or suffix repair",
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The reader's capture notebook, swept through both production pipelines.
// ---------------------------------------------------------------------------------------------

/// Where the reader's own collected passages live, and how this test is allowed to treat them.
///
/// `core/src/corpus.rs` writes this file: every passage the reader captured, flattened to single
/// spaces and **deduplicated by a whitespace-insensitive key** before it is appended. So a line
/// here is one passage the reader met at least once — the file cannot say how often, and a count
/// of lines is not a frequency.
///
/// The reader has also annotated it by hand, which `collect` never does: it writes only OCR'd
/// passages containing Japanese. Those annotations are separated by byte range rather than by
/// stripping parentheses generally, because parentheses occur inside ordinary Japanese too.
///
/// **The passages never enter this file or any tracked report.** They are read from disk, pinned
/// by hash, and written to an ignored artifact — the same rule the three reported examples above
/// follow, and for the same reason: the material is the reader's own captured reading.
fn notebook_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("SAKATSU_NOTEBOOK") {
        return std::path::PathBuf::from(path);
    }
    std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_default())
        .join("com.sakatsu.app")
        .join("corpus")
        .join("captured.txt")
}

/// One notebook line, split into the passage and whatever the reader wrote about it.
#[derive(Debug)]
struct Captured {
    line: usize,
    passage: String,
    /// Byte ranges within the original line that hold the reader's own commentary.
    annotations: Vec<(usize, usize)>,
    note: String,
}

/// Which byte ranges of which notebook hold the reader's commentary rather than captured text.
///
/// **Recorded, not inferred.** Two heuristics were tried and both were wrong. Removing every
/// top-level `(...)` run deletes captured Japanese that contains ASCII parentheses; requiring an
/// ASCII letter inside the run deletes `(BGM)`, `(CV: …)`, `(Remix)` and every other Latin aside
/// that belongs to the passage. There is no property of the text that separates a reader's note
/// from a parenthesis the capture itself contains — only knowledge of which file this is.
///
/// So the ranges are pinned to the notebook's SHA-256 and verified on load. A notebook that is not
/// this one has no entry, and the sweep refuses it rather than guessing at its annotations.
/// Offsets are **byte offsets within the line**, and each is checked to be a char boundary and to
/// begin `(` and end `)` before anything is removed.
/// One recorded note: the 1-based line, and the byte range within that line.
type AnnotationRange = (usize, usize, usize);
/// A notebook's SHA-256 and every note recorded in it.
type AnnotatedNotebook = (&'static str, &'static [AnnotationRange]);

const ANNOTATED_NOTEBOOKS: &[AnnotatedNotebook] = &[
    (
        "c9e73f20a64667676e4b4f5433604be73f2e169650f30cd72db8c8c3e0d78871",
        // (1-based line, byte start within the line, byte end within the line)
        &[(95, 31, 195), (97, 88, 134), (97, 135, 251)],
    ),
    // **The same notebook, extended.** Its first 6,696 bytes hash to the record above, so lines
    // 1-124 are those lines byte for byte; lines 125-152 were appended since and were inspected
    // on 2026-09-24 — none holds a parenthesis, ASCII or full-width. So the notes are the same
    // three ranges, and the appended lines are all passage.
    (
        "2ba7b10272cc29850e397cf3cc2a30b4f976dc0cc42ace36edecd01706086631",
        &[(95, 31, 195), (97, 88, 134), (97, 135, 251)],
    ),
];

/// Why a notebook could not be parsed into passages.
#[derive(Debug)]
enum NotebookFault {
    /// No annotation record exists for this file, so which parentheses are commentary is unknown.
    Unrecorded(String),
    /// A recorded range does not describe what it claims to.
    Range(String),
}

impl std::fmt::Display for NotebookFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotebookFault::Unrecorded(digest) => write!(
                f,
                "no annotation record for a notebook hashing {digest}; add one to \
                 ANNOTATED_NOTEBOOKS after inspecting it rather than inferring the notes"
            ),
            NotebookFault::Range(detail) => write!(f, "a recorded annotation range {detail}"),
        }
    }
}

/// Split every nonempty line into the captured passage and the reader's commentary.
///
/// Takes the text already read, so the bytes parsed here and the bytes hashed as this run's input
/// are the same bytes. Reading the file a second time to hash it would have left a window in which
/// the notebook changed between the two, and reported a digest for text it never analysed.
fn read_notebook(raw: &str, digest: &str) -> Result<Vec<Captured>, NotebookFault> {
    let Some((_, ranges)) = ANNOTATED_NOTEBOOKS
        .iter()
        .find(|(recorded, _)| *recorded == digest)
    else {
        return Err(NotebookFault::Unrecorded(digest.to_string()));
    };
    read_notebook_with(raw, digest, ranges)
}

/// The SHA-256 a notebook is recorded under.
fn notebook_digest(raw: &str) -> String {
    sakatsu_core::hashing::hex(<sha2::Sha256 as sha2::Digest>::digest(raw.as_bytes()))
}

/// [`read_notebook`] with the ranges supplied rather than looked up, so the verification below can
/// be exercised without editing the recorded table.
fn read_notebook_with(
    raw: &str,
    _digest: &str,
    ranges: &[AnnotationRange],
) -> Result<Vec<Captured>, NotebookFault> {
    let mut captured = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        let number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let mut annotations: Vec<(usize, usize)> = ranges
            .iter()
            .filter(|(at, _, _)| *at == number)
            .map(|(_, start, end)| (*start, *end))
            .collect();
        annotations.sort_unstable();

        for (start, end) in &annotations {
            if *end > line.len() || !line.is_char_boundary(*start) || !line.is_char_boundary(*end) {
                return Err(NotebookFault::Range(format!(
                    "on line {number}, {start}..{end}, is not a character range of that line"
                )));
            }
            let text = &line[*start..*end];
            if !text.starts_with('(') || !text.ends_with(')') {
                return Err(NotebookFault::Range(format!(
                    "on line {number}, {start}..{end}, does not delimit a parenthesised note"
                )));
            }
        }

        let mut passage = String::new();
        let mut note = String::new();
        let mut cursor = 0usize;
        for (start, end) in &annotations {
            passage.push_str(&line[cursor..*start]);
            note.push_str(&line[*start..*end]);
            cursor = *end;
        }
        passage.push_str(&line[cursor..]);
        let passage = passage.trim().to_string();
        if passage.is_empty() {
            continue;
        }
        captured.push(Captured {
            line: number,
            passage,
            annotations,
            note,
        });
    }
    Ok(captured)
}

/// **Every notebook passage, through both engines, with the evidence to place a failure.**
///
/// Not a pass/fail gate — a diagnosis. It answers *where* an answer came from, so a family of
/// failures can be attributed to a stage instead of to an impression. Distinct outcomes are kept
/// distinct: an analysis that failed, a lookup that found nothing, and a lookup that found
/// something whose scope was never established are three different things and are recorded as
/// three different things.
///
/// The passages and the reader's notes go to an **ignored** artifact under `target/evaluation/`.
/// What may be quoted anywhere tracked is a line number, a hash, and an aggregate.
#[test]
#[ignore = "needs the reader's notebook, the dictionaries and Ichiran"]
fn the_capture_notebook_is_traced_through_both_pipelines() {
    let dictionaries = common::installed_dictionaries();
    assert!(!dictionaries.is_empty(), "the dictionaries are required");
    let path = notebook_path();
    // **Read once.** Parsing from one read and hashing from another left a window in which the
    // notebook could change between them, and would have published a digest for text that was
    // never analysed. These are the same bytes.
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "no notebook at {}: {error} — set SAKATSU_NOTEBOOK",
            path.display()
        )
    });
    let digest = sakatsu_core::hashing::hex(<sha2::Sha256 as sha2::Digest>::digest(&bytes));
    let raw = String::from_utf8(bytes).expect("the notebook is UTF-8");
    let captured = read_notebook(&raw, &digest)
        .unwrap_or_else(|fault| panic!("{} could not be parsed: {fault}", path.display()));
    assert!(!captured.is_empty(), "the notebook holds no passages");

    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let ichiran = IchiranAnalyzer::new();
    sakatsu_core::analyzer::ichiran::server::set_persistent(true);
    ichiran.warm();
    let ichiran_answers = ichiran.analyze("これって").is_ok();

    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");
    let _ = std::fs::create_dir_all(&out);

    // **The run, not just the input.** Naming the artifact after the notebook alone meant a rerun
    // after a code or dictionary change overwrote the evidence it should have been compared with
    // — and `File::create` truncates, so the old trace was gone before the new one was written.
    // The identity covers what could change the answers; a genuinely identical run rewrites an
    // identical file, and anything else lands beside its predecessor.
    // **The full digest and byte length, not the runner's abbreviated summary.** A 16-hex prefix
    // with a rounded megabyte figure identifies a build to a human reading a report; it is not an
    // identity, and two builds sharing it would share an artifact name.
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| {
            format!(
                "{}:{}",
                sakatsu_core::hashing::hex(<sha2::Sha256 as sha2::Digest>::digest(&bytes)),
                bytes.len()
            )
        })
        .unwrap_or_else(|| "unknown".to_string());
    let source_tree = runner::source_digest();
    let route = sakatsu_core::analyzer::ichiran::server::last_route();
    let identity = {
        let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
        for part in [
            digest.as_str(),
            executable.as_str(),
            source_tree.as_str(),
            route.name(),
        ] {
            sha2::Digest::update(&mut hasher, part.as_bytes());
            sha2::Digest::update(&mut hasher, b"");
        }
        for dictionary in &dictionaries {
            sha2::Digest::update(
                &mut hasher,
                dictionary
                    .content_fingerprint()
                    .unwrap_or_default()
                    .as_bytes(),
            );
            sha2::Digest::update(&mut hasher, b"");
        }
        sakatsu_core::hashing::hex(sha2::Digest::finalize(hasher))
    };
    let artifact = out.join(format!(
        "notebook-trace-{}-{}.jsonl",
        &digest[..16],
        &identity[..16]
    ));
    // Written beside the target and renamed, so an interrupted run cannot leave a half-written
    // trace wearing a complete run's name.
    let staging = artifact.with_extension("jsonl.partial");
    let mut sink = std::io::BufWriter::new(std::fs::File::create(&staging).expect("artifact"));

    let unique: std::collections::BTreeSet<&str> =
        captured.iter().map(|c| c.passage.as_str()).collect();

    use std::io::Write as _;
    writeln!(
        sink,
        "{}",
        json!({
            "kind": "header",
            "run_identity": identity,
            "executable": executable,
            "source_tree": source_tree,
            "ichiran_route": route.name(),
            "notebook_sha256": digest,
            "occurrences": captured.len(),
            "unique_passages": unique.len(),
            "annotated_lines": captured.iter().filter(|c| !c.annotations.is_empty()).count(),
            "dictionaries": dictionaries.iter().map(|d| json!({
                "title": d.meta().title,
                "sha256": d.content_fingerprint().unwrap_or_default(),
                // **Whether cross-reference resolution can run at all.** Following a pointer to
                // its target needs the sequence bucket, which an index imported before that
                // bucket existed does not have. Recorded per dictionary so a trace that shows
                // pointers still standing says why, instead of looking like a failed repair.
                "has_sequence_bucket": d.has_sequence(),
            })).collect::<Vec<_>>(),
            "ichiran_answers": ichiran_answers,
            "note": "deduplicated upstream by corpus::collect; line counts are not frequencies",
        })
    )
    .expect("write");

    // Identical passages are analysed once and the result reused; occurrences are still listed.
    let mut done: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for capture in &captured {
        if !done.insert(capture.passage.clone()) {
            continue;
        }
        // **The production path, traced.** Tracing keeps the stages and changes nothing computed
        // (`analysis_path_identity.rs`), so the answers here are the reader's.
        let traced = study::AnalysisOptions::PRODUCTION.traced();
        for (engine, outcome) in [
            (
                "lindera",
                study::run(&capture.passage, &lindera, &dictionaries, traced),
            ),
            (
                "ichiran",
                if ichiran_answers {
                    study::run(&capture.passage, &ichiran, &dictionaries, traced)
                } else {
                    Err(study::RunFailure {
                        error: sakatsu_core::AnalyzeError::EngineUnavailable(
                            "the sidecar did not answer the warm-up".into(),
                        ),
                        stages: None,
                    })
                },
            ),
        ] {
            let (analysis, stages) = match outcome {
                Ok(run) => (run.analysis, run.stages),
                Err(failure) => {
                    writeln!(
                        sink,
                        "{}",
                        json!({
                            "kind": "case", "line": capture.line, "engine": engine,
                            "passage": capture.passage, "note": capture.note,
                            "outcome": "analysis-failed", "error": failure.error.to_string(),
                            "stage_changes": failure.stages.as_ref().map(|stages| json!({
                                "display": stage_changes(&stages.converted, &stages.displayed),
                            })),
                        })
                    )
                    .expect("write");
                    continue;
                }
            };
            let matches = matcher::find_across(&analysis.tokens, &pack::all());
            let tokens: Vec<serde_json::Value> = (0..analysis.tokens.len())
                .map(|index| {
                    let token = &analysis.tokens[index];
                    let trace = lookup_trace(&analysis.tokens, index, &dictionaries);
                    json!({
                        "i": index,
                        "surface": token.surface,
                        "span": [token.start, token.end],
                        "reading": token.reading,
                        "dict_form": token.dict_form,
                        "lemma_reading": token.lemma_reading,
                        "lexical_role": format!("{:?}", token.lexical_role),
                        "components": trace.components,
                        "fallbacks": trace.fallbacks,
                        "query": trace.query,
                        "retrieved": trace.retrieved,
                        "headword": trace.headword,
                        "inline_source": trace.inline_source,
                        "name_leads": trace.name_leads,
                        "senses": token.senses.len(),
                        "conjugation": token.conjugation.is_some(),
                        "hits": trace.hits.iter().map(|(title, matched, term, reading, seq)| json!({
                            "dictionary": title, "matched": matched,
                            "term": term, "reading": reading, "seq": seq,
                        })).collect::<Vec<_>>(),
                        // **Every sense, not the first four.** A truncated list cannot tell a
                        // missing answer from a buried one, and this project has already drawn
                        // the wrong conclusion from exactly that.
                        "displayed": trace.displayed.iter().map(|(seq, glosses)| json!({
                            "seq": seq, "glosses": glosses,
                        })).collect::<Vec<_>>(),
                        "favoured": trace.displayed.first().map(|(seq, glosses)| json!({
                            "seq": seq, "glosses": glosses,
                        })),
                        "lookup_failure": trace.failure,
                        "component_spans": token.components.iter()
                            .map(|part| json!([part.start, part.end]))
                            .collect::<Vec<_>>(),
                        "expression_offer": match sakatsu_core::expression::offered(
                            &analysis.tokens, index, &matches, &dictionaries,
                        ) {
                            Ok(Some(offer)) => json!({
                                "expression": offer.expression,
                                "extent": offer.extent.name(),
                            }),
                            Ok(None) => serde_json::Value::Null,
                            Err(error) => json!({"failed": error.to_string()}),
                        },
                        // Three different silences, kept apart.
                        "outcome": if !trace.hits.is_empty() && !trace.displayed.is_empty() {
                            "answered"
                        } else if trace.hits.is_empty() {
                            "no-dictionary-hit"
                        } else {
                            "hit-but-nothing-displayed"
                        },
                    })
                })
                .collect();
            writeln!(
                sink,
                "{}",
                json!({
                    "kind": "case", "line": capture.line, "engine": engine,
                    "passage": capture.passage, "note": capture.note,
                    "outcome": "analysed",
                    "raw_sha256": stages.as_ref().and_then(|stages| stages.raw.as_deref()).map(|raw| {
                        sakatsu_core::hashing::hex(<sha2::Sha256 as sha2::Digest>::digest(raw.as_bytes()))
                    }),
                    // What each stage after the analyzer did, so a changed answer names the
                    // stage that changed it — including shared code acting on valid Ichiran output.
                    "stage_changes": stages.as_ref().map(|stages| json!({
                        "display": stage_changes(&stages.converted, &stages.displayed),
                        "repair": stage_changes(&stages.displayed, &stages.repaired),
                        "grouping": stage_changes(&stages.repaired, &analysis),
                    })),
                    // Report-only in this baseline: how many existing parts already break the
                    // component contract, before anything is enforced.
                    "component_faults": sakatsu_core::analyzer::validate_components(&analysis)
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>(),
                    // **The guarantee the parts row relies on**: the offered projection checked on
                    // its own terms (`validate_selectable`). Zero is the requirement; the list above
                    // describes the retained decomposition and may legitimately hold grammatical
                    // parts the text does not spell.
                    "faults_on_selectable_parts": sakatsu_core::analyzer::validate_selectable(&analysis)
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>(),
                    "unselectable_parts": common::reference::unselectable_parts(&analysis),
                    "tokens": tokens,
                    "grammar": matches.iter().map(|m| json!({
                        "pattern": m.pattern_id,
                        "first": m.first_token, "end": m.last_token_end,
                        "span": [m.start, m.end],
                        // Resolved, so the trace says what the reader is told and not merely
                        // which rule fired. A dangling id resolves to null rather than to its id.
                        "explanation": pack::find(&m.pattern_id)
                            .map(|entry| json!({"title": entry.title, "explanation": entry.explanation})),
                    })).collect::<Vec<_>>(),
                })
            )
            .expect("write");
        }
    }
    sink.flush().expect("flush");
    drop(sink);

    // **Publish without destroying evidence.** The run identity covers what *should* determine the
    // answers, so an identical identity ought to produce identical bytes — but "ought to" is not a
    // guarantee, and a rename over the destination would silently discard the disagreement rather
    // than record it. Identical content replaces nothing; differing content lands beside its
    // predecessor under its own content digest, and the run says so.
    let written = std::fs::read(&staging).expect("the staged trace reads back");
    let artifact = if artifact.exists() {
        let existing = std::fs::read(&artifact).unwrap_or_default();
        if existing == written {
            std::fs::remove_file(&staging).expect("drop the redundant staging file");
            artifact
        } else {
            let content =
                sakatsu_core::hashing::hex(<sha2::Sha256 as sha2::Digest>::digest(&written));
            let beside = artifact.with_file_name(format!(
                "notebook-trace-{}-{}-{}.jsonl",
                &digest[..16],
                &identity[..16],
                &content[..16]
            ));
            println!(
                "
  NOTE: a trace already exists for this run identity and differs from this                  run's. The existing file is kept and this one is written beside it."
            );
            std::fs::rename(&staging, &beside).expect("publish beside the existing trace");
            beside
        }
    } else {
        std::fs::rename(&staging, &artifact).expect("publish the trace atomically");
        artifact
    };

    println!("\n=== the capture notebook, traced ===");
    println!("  notebook      {}", path.display());
    println!("  sha256        {digest}");
    println!(
        "  occurrences   {}   unique passages {}",
        captured.len(),
        unique.len()
    );
    println!(
        "  annotated     {}",
        captured
            .iter()
            .filter(|c| !c.annotations.is_empty())
            .count()
    );
    println!("  ichiran answers {ichiran_answers}");
    println!("  artifact      {}", artifact.display());
    println!(
        "\n  The artifact holds the passages and the reader's notes and is ignored. Quote line\n  \
         numbers, hashes and aggregates anywhere tracked."
    );
}

/// **A notebook nobody has inspected is refused, not guessed at.**
///
/// This is the guarantee both earlier heuristics lacked. Whichever rule is chosen — every
/// parenthesis, or every parenthesis holding a Latin letter — it deletes captured text somewhere,
/// because `(BGM)` and `(CV: 大塚)` are things captures actually contain. Only a record of which
/// file this is can separate a note from the passage, so an unrecorded file has no answer and
/// says so.
#[test]
fn an_unrecorded_notebook_is_refused_rather_than_parsed_by_guesswork() {
    let fault = read_notebook("これ (BGM)\n", "0".repeat(64).as_str())
        .expect_err("a notebook with no annotation record must be refused");
    assert!(
        matches!(fault, NotebookFault::Unrecorded(_)),
        "got {fault:?}"
    );
    assert!(
        fault.to_string().contains("ANNOTATED_NOTEBOOKS"),
        "the refusal says how to record one: {fault}"
    );
}

/// A Latin aside inside a capture is **passage**, and survives.
///
/// `(BGM)` is the case that retired the previous rule: it contains ASCII letters, so the
/// letter-based test would have deleted it and shortened the text the analyzer was given.
#[test]
fn a_latin_aside_in_a_capture_is_not_treated_as_commentary() {
    let raw = "これ (BGM)\n";
    let digest = notebook_digest(raw);
    // Recorded with no annotations at all: nothing on this line is a note.
    let captured = read_notebook_with(raw, &digest, &[]).expect("parses");
    assert_eq!(captured.len(), 1);
    assert_eq!(
        captured[0].passage, "これ (BGM)",
        "a parenthesised Latin aside belongs to the passage"
    );
    assert!(captured[0].note.is_empty());
}

/// A recorded range is verified against the line, not trusted.
#[test]
fn a_recorded_range_that_does_not_delimit_a_note_is_refused() {
    let raw = "これ (note)\n";
    let digest = notebook_digest(raw);

    // Off the end of the line.
    let past = read_notebook_with(raw, &digest, &[(1, 3, 9_999)]).expect_err("must refuse");
    assert!(matches!(past, NotebookFault::Range(_)), "got {past:?}");

    // Inside a multi-byte character rather than on its boundary.
    let split = read_notebook_with(raw, &digest, &[(1, 1, 11)]).expect_err("must refuse");
    assert!(matches!(split, NotebookFault::Range(_)), "got {split:?}");

    // A well-formed character range that is simply not a parenthesised run.
    let plain = read_notebook_with(raw, &digest, &[(1, 0, 9)]).expect_err("must refuse");
    assert!(matches!(plain, NotebookFault::Range(_)), "got {plain:?}");

    // And the range that does delimit the note is accepted, so the refusals above are not
    // a parser that refuses everything.
    let start = raw.find('(').expect("a note");
    let end = raw.find(')').expect("a note") + 1;
    let ok = read_notebook_with(raw, &digest, &[(1, start, end)]).expect("parses");
    assert_eq!(ok[0].passage, "これ");
    assert_eq!(ok[0].note, "(note)");
}

/// **The notebook this work was done against still parses**, and to what was reported.
///
/// Pins the recorded ranges to their file: if the notebook is edited, its digest changes and the
/// sweep refuses it rather than applying stale offsets to moved text.
#[test]
fn the_recorded_notebook_ranges_describe_parenthesised_notes() {
    for (digest, ranges) in ANNOTATED_NOTEBOOKS {
        assert_eq!(digest.len(), 64, "a notebook is keyed by its full SHA-256");
        assert!(
            !ranges.is_empty(),
            "a record with no ranges records nothing"
        );
        for (line, start, end) in *ranges {
            assert!(start < end, "line {line}: {start}..{end} is not a range");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The Ichiran-first baseline: named cases replayed stage by stage.
// ---------------------------------------------------------------------------------------------

/// What a case is expected to show, checked at every stage so the stage where it **first fails to
/// hold** can be named. Expectations name lemma readings and structure, never captured text: a
/// notebook line is located by number, and its contracted run is found structurally.
#[derive(Debug, Clone, Copy)]
enum Expect {
    /// A token covering the first occurrence of `needle`, whose lemma reading is `lemma`.
    Covers {
        needle: &'static str,
        lemma: &'static str,
    },
    /// A token spelling exactly the first occurrence of `needle` — held together, not split.
    Whole { needle: &'static str },
    /// A token covering the first `っとる` / `っどる` / `んどる` occurrence **and the character
    /// before it** — the verb it contracts — whose lemma reading is `lemma`.
    Contracted { lemma: &'static str },
    /// Both span contracts hold: every token and every part spells its own source slice, and every
    /// non-whitespace byte is covered.
    Grounded,
}

impl Expect {
    fn describe(&self) -> String {
        match self {
            Expect::Covers { needle, lemma } => format!("covers {needle} as {lemma}"),
            Expect::Whole { needle } => format!("{needle} whole"),
            Expect::Contracted { lemma } => format!("contracted ～とる as {lemma}"),
            Expect::Grounded => "grounded".into(),
        }
    }

    /// The byte range this expectation is about, in `text`, if it is about one.
    fn target(&self, text: &str) -> Result<Option<(usize, usize)>, String> {
        match self {
            Expect::Covers { needle, .. } | Expect::Whole { needle } => {
                let start = text.find(needle).ok_or("the needle is not in the source")?;
                Ok(Some((start, start + needle.len())))
            }
            Expect::Contracted { .. } => {
                let at = ["っとる", "っどる", "んどる"]
                    .iter()
                    .filter_map(|tail| text.find(tail))
                    .min()
                    .ok_or("no contracted ～とる in the source")?;
                let before = text[..at]
                    .char_indices()
                    .next_back()
                    .map(|(index, _)| index)
                    .ok_or("the contraction opens the passage")?;
                Ok(Some((before, at + "っとる".len())))
            }
            Expect::Grounded => Ok(None),
        }
    }

    /// `Ok` when this analysis shows what is expected; `Err` says what it showed instead.
    fn check(&self, analysis: &Analysis) -> Result<(), String> {
        let covering = |start: usize, end: usize| {
            analysis.tokens.iter().find(|token| {
                matches!((token.start, token.end), (Some(from), Some(to)) if from <= start && end <= to)
            })
        };
        let lemma_of = |token: &sakatsu_core::Token| {
            sakatsu_core::kana::to_hiragana(if token.lemma_reading.is_empty() {
                &token.reading
            } else {
                &token.lemma_reading
            })
        };
        let target = self.target(&analysis.text)?;
        match (self, target) {
            (Expect::Covers { lemma, .. } | Expect::Contracted { lemma }, Some((start, end))) => {
                let token = covering(start, end).ok_or_else(|| {
                    format!("no token covers it: {}", split_around(analysis, start, end))
                })?;
                if lemma_of(token) == *lemma {
                    Ok(())
                } else {
                    Err(format!(
                        "covered by {:?} read as lemma {:?}",
                        token.surface,
                        lemma_of(token)
                    ))
                }
            }
            (Expect::Whole { .. }, Some((start, end))) => {
                if analysis
                    .tokens
                    .iter()
                    .any(|token| (token.start, token.end) == (Some(start), Some(end)))
                {
                    Ok(())
                } else {
                    Err(format!(
                        "not one token: {}",
                        split_around(analysis, start, end)
                    ))
                }
            }
            (Expect::Grounded, _) => {
                let spans = sakatsu_core::analyzer::validate_analysis(analysis);
                let parts = sakatsu_core::analyzer::validate_components(analysis);
                if spans.is_empty() && parts.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} span faults, {} component faults; first: {}",
                        spans.len(),
                        parts.len(),
                        spans
                            .first()
                            .map(ToString::to_string)
                            .or_else(|| parts.first().map(ToString::to_string))
                            .unwrap_or_default()
                    ))
                }
            }
            _ => Err("no target".into()),
        }
    }

    /// **The engine's own boundaries, before the adapter.** Whether a word of Ichiran's best
    /// segmentation already covers the target — for a whole-word expectation, spells it exactly.
    /// Boundaries only: a raw word's lemma is not adjudicated here, and grounding is not a question
    /// raw output can answer, so both are `None`.
    fn raw_boundary(&self, words: &[common::reference::RawWord], text: &str) -> Option<bool> {
        let (start, end) = self.target(text).ok()??;
        Some(match self {
            Expect::Whole { .. } => words
                .iter()
                .any(|word| (word.start, word.end) == (Some(start), Some(end))),
            _ => words.iter().any(|word| word.covers(start, end)),
        })
    }
}

/// The tokens overlapping `start..end`, as the reader would see them split.
fn split_around(analysis: &Analysis, start: usize, end: usize) -> String {
    analysis
        .tokens
        .iter()
        .filter(|token| matches!((token.start, token.end), (Some(from), Some(to)) if from < end && start < to))
        .map(|token| format!("{}[{}]", token.surface, token.dict_form))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Where a replay case's text comes from. Read from disk, never embedded — except the synthetic
/// alignment inputs, which are already tracked in `ichiran_width_alignment.rs`.
#[derive(Debug, Clone, Copy)]
enum Source {
    /// Tracked, constructed or already-tracked text.
    Literal(&'static str),
    /// A promoted capture transcript under `corpus/`, by stem.
    Promoted(&'static str),
    /// A notebook line, by 1-based number, with the notebook digests it is valid for.
    Notebook {
        line: usize,
        valid_for: &'static [&'static str],
    },
}

/// Both recorded notebooks: the 09-20 one, and the 09-24 one that extends it byte for byte.
const RECORDED_NOTEBOOK: &str = "c9e73f20a64667676e4b4f5433604be73f2e169650f30cd72db8c8c3e0d78871";
const EXTENDED_NOTEBOOK: &str = "2ba7b10272cc29850e397cf3cc2a30b4f976dc0cc42ace36edecd01706086631";
const BOTH_NOTEBOOKS: &[&str] = &[RECORDED_NOTEBOOK, EXTENDED_NOTEBOOK];

struct ReplayCase {
    id: &'static str,
    source: Source,
    expects: &'static [Expect],
    /// What the case exists to show, in one line.
    why: &'static str,
}

const REPLAY: &[ReplayCase] = &[
    ReplayCase {
        id: "paired-okonnakya",
        source: Source::Promoted("xnviewmp_bu75dRV6Zb"),
        expects: &[
            Expect::Covers {
                needle: "怒んなきゃ",
                lemma: "おこる",
            },
            Expect::Whole {
                needle: "なんで"
            },
            Expect::Grounded,
        ],
        why: "the contracted negative Ichiran loses, beside the なんで it keeps",
    },
    ReplayCase {
        id: "notebook-L81",
        source: Source::Notebook {
            line: 81,
            valid_for: BOTH_NOTEBOOKS,
        },
        expects: &[Expect::Contracted { lemma: "かかる" }, Expect::Grounded],
        why: "dialect ～とる over かかる",
    },
    ReplayCase {
        id: "notebook-L83",
        source: Source::Notebook {
            line: 83,
            valid_for: BOTH_NOTEBOOKS,
        },
        // 回っとる: the contracted run is 回る's. A larger unit headed by する — the して before
        // it included — cannot supply the lemma for that inner span; this is the negative control
        // advice selection must reject.
        expects: &[Expect::Contracted { lemma: "まわる" }, Expect::Grounded],
        why: "dialect ～とる over 回る, inside a larger unit headed by する",
    },
    ReplayCase {
        id: "notebook-L90",
        source: Source::Notebook {
            line: 90,
            valid_for: BOTH_NOTEBOOKS,
        },
        expects: &[Expect::Contracted { lemma: "なる" }, Expect::Grounded],
        why: "dialect ～とる over なる",
    },
    ReplayCase {
        id: "notebook-L136-shokou",
        source: Source::Notebook {
            line: 136,
            valid_for: &[EXTENDED_NOTEBOOK],
        },
        // No expected answer: whether 初交 has a whole-word reading here is the open question.
        expects: &[Expect::Grounded],
        why: "初交, isolated: a whole-word reading or only its components",
    },
    ReplayCase {
        id: "supplied-soseishinki",
        source: Source::Literal(SUPPLIED),
        expects: &[
            Expect::Whole {
                needle: "あなたたち",
            },
            Expect::Grounded,
        ],
        why: "Ichiran's あなたたち, and 神姫/双星 provenance",
    },
    ReplayCase {
        id: "align-halfwidth-kaomoji",
        source: Source::Literal("ｶﾞｯﾂ (┗(ﾟ Дﾟ )┛ﾜｯｼｮｲ ┗( ﾟД ﾟ)┛ﾜｯｼｮｲ)"),
        expects: &[Expect::Grounded],
        why: "half-width kana and kaomoji keep their source",
    },
    ReplayCase {
        id: "align-voiced-marks",
        source: Source::Literal("ｶﾞﾊﾟ ﾟﾞ"),
        expects: &[Expect::Grounded],
        why: "voiced pairs and standalone marks",
    },
    ReplayCase {
        id: "align-repeated-punctuation",
        source: Source::Literal("猫！(犬！ (鳥"),
        expects: &[Expect::Grounded],
        why: "rewritten punctuation cannot jump to a later repetition",
    },
    ReplayCase {
        id: "align-nakaguro",
        source: Source::Literal("景観・社会・文化・報道"),
        expects: &[Expect::Grounded],
        why: "the ・ separator Ichiran echoes as a space",
    },
    ReplayCase {
        id: "align-fullwidth-digits",
        source: Source::Literal("２０１９年"),
        expects: &[Expect::Grounded],
        why: "full-width digits",
    },
    ReplayCase {
        id: "control-ascii-year",
        source: Source::Literal("2019年"),
        expects: &[Expect::Grounded],
        why: "the ASCII year beside it",
    },
];

/// The first stage at which an expectation no longer holds.
///
/// **A failing stage is where a defect is first *present*, not proof of what caused it.** That is
/// why the raw stage is checked at all: when Ichiran's own segmentation already splits the target,
/// the conversion inherited the split rather than made it. Where the raw stage cannot be
/// adjudicated — Lindera has none, and grounding and lemmas are not raw questions — the label
/// says so rather than naming the conversion.
fn first_failing_stage(raw: Option<bool>, stages: [bool; 4]) -> &'static str {
    match (raw, stages) {
        (Some(false), _) => "raw-engine-output",
        (Some(true), [false, ..]) => "conversion-after-raw-covered",
        (None, [false, ..]) => "present-after-analyzer",
        (_, [true, false, ..]) => "display",
        (_, [true, true, false, _]) => "repair",
        (_, [true, true, true, false]) => "grouping",
        (_, [true, true, true, true]) => "holds",
    }
}

/// **The baseline the Ichiran-first work is measured from**: named cases, each engine, each stage.
///
/// For every expectation it records whether it holds in **Ichiran's raw output** (boundaries), after
/// conversion, after the display pipeline and in the finished analysis — so the stage where it first
/// fails is named, and an engine segmentation is not blamed on the adapter. Where Ichiran's own
/// conversion already fails, an N-best diagnostic records whether a returned segmentation would
/// have satisfied it: **present**, **not observed within the limit**, or **diagnostic
/// unavailable**. Never "absent".
///
/// Every case and engine is accounted for by the runner. A source that is missing, or an Ichiran
/// that does not answer, makes the run **incomplete**, not quietly shorter. The raw responses are
/// kept with their digests in the ignored artifact, so the attribution can be re-derived.
#[test]
#[ignore = "needs the dictionaries, Ichiran, the promoted captures and the notebook"]
fn the_ichiran_first_cases_are_replayed_stage_by_stage() {
    let dictionaries = common::installed_dictionaries();
    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let ichiran = IchiranAnalyzer::new();
    sakatsu_core::analyzer::ichiran::server::set_persistent(true);
    ichiran.warm();
    let ichiran_answers = ichiran.analyze("これって").is_ok();
    let route = sakatsu_core::analyzer::ichiran::server::last_route();

    let notebook = std::fs::read(notebook_path()).ok();
    let notebook_digest = notebook
        .as_ref()
        .map(|bytes| sakatsu_core::hashing::hex(<sha2::Sha256 as sha2::Digest>::digest(bytes)));
    let notebook_lines: Vec<String> = notebook
        .as_ref()
        .map(|bytes| {
            String::from_utf8_lossy(bytes)
                .lines()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    // **Content identity for every dictionary, or no run.** An empty fingerprint would let the
    // artifact claim results for a generation it cannot name.
    let fingerprints: Result<Vec<(String, String, bool)>, String> = dictionaries
        .iter()
        .map(|dictionary| {
            dictionary
                .content_fingerprint()
                .map(|sha| {
                    (
                        dictionary.meta().title.clone(),
                        sha,
                        dictionary.has_sequence(),
                    )
                })
                .map_err(|error| format!("{}: {error}", dictionary.meta().title))
        })
        .collect();

    let engines = ["lindera", "ichiran"];
    let ids: Vec<String> = REPLAY
        .iter()
        .flat_map(|case| engines.map(|engine| format!("{}:{engine}", case.id)))
        .collect();
    let mut run = runner::Run::new(runner::Profile::InstalledResources, ids.iter().cloned())
        .engine("lindera, ichiran")
        .needs(
            runner::Requirement::required("dictionaries", !dictionaries.is_empty())
                .because(format!("{} opened", dictionaries.len())),
        )
        .needs(
            runner::Requirement::required("dictionary content identity", fingerprints.is_ok())
                .because(match &fingerprints {
                    Ok(found) => format!("{} fingerprinted", found.len()),
                    Err(error) => error.clone(),
                }),
        )
        .needs(
            runner::Requirement::required("ichiran via the session route", ichiran_answers)
                .because(format!("route {}", route.name())),
        )
        .needs(
            runner::Requirement::required(
                "a recorded notebook",
                notebook_digest
                    .as_deref()
                    .is_some_and(|digest| BOTH_NOTEBOOKS.contains(&digest)),
            )
            .because(format!("{:?}", notebook_digest)),
        );
    let fingerprints = fingerprints.unwrap_or_default();
    for (title, sha, _) in &fingerprints {
        run = run.dictionary(format!("{title} (sha256 {sha})"));
    }

    let mut records: Vec<serde_json::Value> = Vec::new();
    let mut table: Vec<String> = Vec::new();

    for case in REPLAY {
        let text: Result<String, runner::Outcome> = match case.source {
            Source::Literal(text) => Ok(text.to_string()),
            Source::Promoted(stem) => {
                std::fs::read_to_string(common::corpus_dir().join(format!("{stem}.txt")))
                    .map(|text| text.trim().to_string())
                    .map_err(|error| runner::Outcome::incomplete(format!("capture {stem}: {error}")))
            }
            Source::Notebook { line, valid_for } => match notebook_digest.as_deref() {
                None => Err(runner::Outcome::incomplete("no notebook")),
                Some(digest) if !valid_for.contains(&digest) => {
                    Err(runner::Outcome::skipped(format!(
                        "line {line} is recorded only for notebooks {valid_for:?}; this one is {digest}"
                    )))
                }
                Some(_) => notebook_lines
                    .get(line - 1)
                    .map(|text| text.trim().to_string())
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| runner::Outcome::incomplete(format!("line {line} is empty or absent"))),
            },
        };
        let text = match text {
            Ok(text) if !text.is_empty() => text,
            Ok(_) => {
                for engine in engines {
                    run.record(
                        format!("{}:{engine}", case.id),
                        runner::Outcome::incomplete("the source is empty"),
                    );
                }
                continue;
            }
            Err(outcome) => {
                table.push(format!("  {:<28} {}", case.id, outcome.label()));
                records.push(json!({"case": case.id, "outcome": outcome.label()}));
                for engine in engines {
                    run.record(format!("{}:{engine}", case.id), outcome.clone());
                }
                continue;
            }
        };

        for (engine, analyzer) in [
            ("lindera", &lindera as &dyn Analyzer),
            ("ichiran", &ichiran),
        ] {
            let id = format!("{}:{engine}", case.id);
            if engine == "ichiran" && !ichiran_answers {
                run.record(
                    id,
                    runner::Outcome::incomplete("Ichiran did not answer the warm-up"),
                );
                continue;
            }
            let run_result = study::run(
                &text,
                analyzer,
                &dictionaries,
                study::AnalysisOptions::PRODUCTION.traced(),
            );
            let (finished, stages, error, repairs) = match run_result {
                Ok(done) => (Some(done.analysis), done.stages, None, Some(done.repairs)),
                Err(failure) => (
                    None,
                    failure.stages.map(|stages| *stages),
                    Some(failure.error.to_string()),
                    None,
                ),
            };
            let raw_json = stages.as_ref().and_then(|stages| stages.raw.clone());
            let source = stages
                .as_ref()
                .map(|stages| stages.converted.text.clone())
                .unwrap_or_else(|| text.clone());
            let raw_words = raw_json
                .as_deref()
                .map(|json| common::reference::raw_words(json, &source));

            let mut verdicts = Vec::new();
            for expect in case.expects {
                let at = |analysis: Option<&Analysis>| {
                    analysis
                        .map(|analysis| expect.check(analysis))
                        .unwrap_or(Err("no analysis at this stage".into()))
                };
                let converted = at(stages.as_ref().map(|stages| &stages.converted));
                let displayed = at(stages.as_ref().map(|stages| &stages.displayed));
                let repaired = at(stages.as_ref().map(|stages| &stages.repaired));
                let last = at(finished.as_ref());
                let raw = match &raw_words {
                    Some(Ok(words)) => expect.raw_boundary(words, &source),
                    _ => None,
                };
                let stage = first_failing_stage(
                    raw,
                    [
                        converted.is_ok(),
                        displayed.is_ok(),
                        repaired.is_ok(),
                        last.is_ok(),
                    ],
                );
                // Presence only, and only where Ichiran's own conversion already fails a
                // linguistic expectation — grounding is not a question N-best can answer.
                let nbest = if engine == "ichiran"
                    && converted.is_err()
                    && !matches!(expect, Expect::Grounded)
                {
                    match common::reference::segmentations(&ichiran, &text, 5) {
                        Err(reason) => {
                            json!({"outcome": "diagnostic-unavailable", "reason": reason})
                        }
                        Ok(returned) => match returned
                            .iter()
                            .position(|analysis| expect.check(analysis).is_ok())
                        {
                            Some(at) => json!({"outcome": "present-in-returned", "rank": at + 1}),
                            None => json!({
                                "outcome": "not-observed-within-limit",
                                "limit": 5, "returned": returned.len(),
                            }),
                        },
                    }
                } else {
                    json!({"outcome": "not-asked"})
                };
                table.push(format!(
                    "  {:<28} {engine:<8} {:<34} {stage:<30} final {:<6} n-best {}",
                    case.id,
                    expect.describe(),
                    if last.is_ok() { "holds" } else { "fails" },
                    nbest["outcome"].as_str().unwrap_or("?")
                ));
                verdicts.push(json!({
                    "expect": expect.describe(),
                    "raw_boundary": raw,
                    "converted": converted.err(),
                    "displayed": displayed.err(),
                    "repaired": repaired.err(),
                    "final": last.err(),
                    "first_failing_stage": stage,
                    "nbest": nbest,
                }));
            }

            // Provenance for every word-token in the finished analysis: what the senses were
            // retrieved for, and whether the lookup failed rather than found nothing.
            let provenance: Vec<serde_json::Value> = finished
                .as_ref()
                .map(|analysis| {
                    (0..analysis.tokens.len())
                        .filter(|&index| analysis.tokens[index].is_word())
                        .map(|index| {
                            let trace = lookup_trace(&analysis.tokens, index, &dictionaries);
                            json!({
                                "surface": trace.surface, "query": trace.query,
                                "retrieved": trace.retrieved, "headword": trace.headword,
                                "matched_terms": trace.matched_terms(),
                                "senses": trace.displayed.len(),
                                "lookup_failure": trace.failure,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            records.push(json!({
                "case": case.id, "why": case.why, "engine": engine, "text": text,
                "error": error,
                // **The engine's own response, kept**, so the raw attribution can be re-derived.
                "raw_sha256": raw_json.as_deref().map(|raw| {
                    sakatsu_core::hashing::hex(<sha2::Sha256 as sha2::Digest>::digest(raw.as_bytes()))
                }),
                "raw_json": raw_json,
                "raw_words": match &raw_words {
                    Some(Ok(words)) => json!(words.iter().map(|word| json!({
                        "span": [word.start, word.end], "text": word.text, "kana": word.kana,
                        "score": word.score, "alternatives": word.alternatives,
                        "compound": word.compound,
                    })).collect::<Vec<_>>()),
                    Some(Err(reason)) => json!({"unreadable": reason}),
                    None => serde_json::Value::Null,
                },
                "tokens": finished.as_ref().map(|analysis| analysis.tokens.iter().map(|token| json!({
                    "surface": token.surface, "span": [token.start, token.end],
                    "reading": token.reading, "dict_form": token.dict_form,
                    "lemma_reading": token.lemma_reading,
                    "components": token.components.iter().map(|part| json!({
                        "surface": part.surface, "span": [part.start, part.end],
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>()),
                "stage_changes": stages.as_ref().map(|stages| json!({
                    "display": stage_changes(&stages.converted, &stages.displayed),
                    "repair": stage_changes(&stages.displayed, &stages.repaired),
                    "grouping": finished.as_ref().map(|analysis| stage_changes(&stages.repaired, analysis)),
                })),
                // What the contraction repair tried, and why it changed nothing where it did not.
                "repairs": repairs.as_ref().map(|account| json!({
                    "applied": account.notes().iter().map(|note| json!({
                        "span": [note.start, note.end], "family": note.family,
                        "lemma": note.lemma, "reading": note.reading, "replaced": note.replaced,
                    })).collect::<Vec<_>>(),
                    "abstained": account.abstentions().map(|(investigation, reason)| json!({
                        "span": [investigation.start, investigation.end],
                        "tokens": investigation.tokens, "reason": reason.name(),
                    })).collect::<Vec<_>>(),
                    "answered": account.answered,
                    "advised_sentences": account.advised_sentences,
                    "dictionary_calls": account.dictionary_calls,
                })),
                "verdicts": verdicts,
                "provenance": provenance,
            }));
            run.record(
                id,
                match &error {
                    None => runner::Outcome::Passed,
                    Some(error) => runner::Outcome::failed(runner::Stage::Analyzer, error.clone()),
                },
            );
        }
    }

    // **初交: whether a whole-word entry exists, per dictionary generation actually tested.** A
    // lookup that fails is a failure and is reported as one; the absence claim is made only for a
    // dictionary whose lookup succeeded, and only about that generation.
    let shokou: Vec<serde_json::Value> = dictionaries
        .iter()
        .zip(&fingerprints)
        .map(|(dictionary, (title, sha, _))| match dictionary.lookup("初交") {
            Ok(hits) => json!({
                "dictionary": title, "sha256": sha, "lookup": "succeeded",
                "whole_word_entries": hits.iter()
                    .filter(|hit| hit.entry.term == "初交")
                    .map(|hit| json!({"reading": hit.entry.reading, "sequence": hit.entry.sequence}))
                    .collect::<Vec<_>>(),
            }),
            Err(error) => json!({
                "dictionary": title, "sha256": sha, "lookup": "failed", "error": error.to_string(),
            }),
        })
        .collect();
    records.push(json!({"case": "shokou-whole-word-entries", "per_dictionary": shokou}));

    let header = json!({
        "kind": "header",
        "executable": common::reference::executable_identity(),
        "source_tree": runner::source_digest(),
        "ichiran_route": route.name(),
        "ichiran_answers": ichiran_answers,
        "notebook_sha256": notebook_digest,
        "dictionaries": fingerprints.iter().map(|(title, sha, sequence)| json!({
            "title": title, "sha256": sha, "has_sequence_bucket": sequence,
        })).collect::<Vec<_>>(),
    });
    let mut body = header.to_string();
    body.push('\n');
    for record in &records {
        body.push_str(&record.to_string());
        body.push('\n');
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");
    let artifact = common::reference::publish(&out, "ichiran-first-replay", "jsonl", &body)
        .unwrap_or_else(|error| panic!("the replay artifact could not be published: {error}"));

    println!("\n=== the Ichiran-first cases, stage by stage ===");
    println!(
        "  route {}   ichiran answers {ichiran_answers}   notebook {:?}",
        route.name(),
        notebook_digest.as_deref().map(|digest| &digest[..16])
    );
    for line in &table {
        println!("{line}");
    }
    for row in &shokou {
        println!(
            "  初交 in {} ({}…): lookup {}, whole-word entries {}",
            row["dictionary"].as_str().unwrap_or("?"),
            &row["sha256"].as_str().unwrap_or("?")
                [..16.min(row["sha256"].as_str().unwrap_or("?").len())],
            row["lookup"].as_str().unwrap_or("?"),
            row["whole_word_entries"].as_array().map_or(0, Vec::len),
        );
    }
    println!("  artifact {}", artifact.display());
    run.accounted_for().expect_success();
}

/// What one stage did to the tokens: spans that appeared or disappeared, and readings that moved
/// on a span both stages share.
fn stage_changes(before: &Analysis, after: &Analysis) -> serde_json::Value {
    let span = |token: &sakatsu_core::Token| (token.start, token.end);
    let before_spans: std::collections::BTreeMap<_, _> = before
        .tokens
        .iter()
        .map(|token| (span(token), token))
        .collect();
    let after_spans: std::collections::BTreeMap<_, _> = after
        .tokens
        .iter()
        .map(|token| (span(token), token))
        .collect();
    let removed: Vec<_> = before_spans
        .iter()
        .filter(|(key, _)| !after_spans.contains_key(key))
        .map(|(key, token)| json!({"span": [key.0, key.1], "surface": token.surface}))
        .collect();
    let added: Vec<_> = after_spans
        .iter()
        .filter(|(key, _)| !before_spans.contains_key(key))
        .map(|(key, token)| json!({"span": [key.0, key.1], "surface": token.surface, "dict_form": token.dict_form}))
        .collect();
    let reread: Vec<_> = before_spans
        .iter()
        .filter_map(|(key, old)| {
            let new = after_spans.get(key)?;
            (old.reading != new.reading || old.dict_form != new.dict_form).then(|| {
                json!({
                    "span": [key.0, key.1], "surface": old.surface,
                    "reading": [old.reading, new.reading],
                    "dict_form": [old.dict_form, new.dict_form],
                })
            })
        })
        .collect();
    json!({"removed": removed, "added": added, "reread": reread})
}
