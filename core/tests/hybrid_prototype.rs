//! The hybrid prototype: regions, candidates, and the selector's acceptance cases.

mod common;

#[path = "common/runner.rs"]
mod runner;

use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, LinderaAnalyzer};
use sakatsu_core::dict::Dictionary;
use sakatsu_core::hybrid::{self, Engine};
use sakatsu_core::pipeline::Pipeline;
use sakatsu_core::Analysis;
use sha2::{Digest, Sha256};

/// Transcribed from the reader's screenshot. Carries `あなたたち`, `神姫` and `双星`.
const SUPPLIED: &str = "双星神姫の私があなたたちを裁いてやるわ！";

/// **Ichiran is concurrency-1, and these tests bypass the scheduler that enforces that.**
///
/// Production routes every request through `work::Lane::Ichiran`, which admits one at a time.
/// Calling `IchiranAnalyzer::analyze` directly from parallel test threads violates that, and the
/// sidecar does not fail cleanly when it happens — it returns malformed output
/// (`BadOutput("unexpected JSON: invalid type: integer 0")`), which surfaces as an unrelated-looking
/// parse error in whichever test happened to collide. Serialised here for the same reason the lane
/// exists.
static ICHIRAN: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Bench {
    dictionaries: Vec<Dictionary>,
    lindera: LinderaAnalyzer,
    ichiran: IchiranAnalyzer,
}

impl Bench {
    fn new() -> Option<Bench> {
        let dictionaries = common::installed_dictionaries();
        if dictionaries.is_empty() {
            return None;
        }
        let lindera = LinderaAnalyzer::new().ok()?;
        let ichiran = IchiranAnalyzer::new();
        // Warming opens the persistent session, which is process-global — so it is inside the
        // lane too, not just the queries that follow it.
        let reachable = {
            let _lane = ICHIRAN.lock().unwrap_or_else(|held| held.into_inner());
            sakatsu_core::analyzer::ichiran::server::set_persistent(true);
            ichiran.warm();
            ichiran.analyze("これって").is_ok()
                && sakatsu_core::analyzer::ichiran::server::last_route().is_session()
        };
        if !reachable {
            return None;
        }
        Some(Bench {
            dictionaries,
            lindera,
            ichiran,
        })
    }

    /// The promoted capture carrying the paired `なんで` / `怒んなきゃ` case.
    fn paired(&self) -> String {
        std::fs::read_to_string(common::corpus_dir().join("xnviewmp_bu75dRV6Zb.txt"))
            .expect("the promoted capture is required")
            .trim()
            .to_string()
    }

    /// Both engines on **the same normalised string**, each through the pipeline exactly once.
    ///
    /// Panics on an engine failure, which is what the focused acceptance tests want: there, an
    /// engine that will not answer is the finding. The sweep uses [`Bench::try_analyse`] instead
    /// and accounts for failures, because one flaky response over 140 transcripts should be
    /// reported, not allowed to erase the other 139.
    fn analyse(&self, text: &str) -> (Analysis, Analysis) {
        self.try_analyse(text)
            .unwrap_or_else(|error| panic!("both engines must answer: {error}"))
    }

    fn try_analyse(&self, text: &str) -> Result<(Analysis, Analysis), String> {
        let text = sakatsu_core::analyzer::lines::normalise_breaks(text)
            .trim()
            .to_string();
        // Borrowed here rather than held: `Pipeline` borrows the dictionaries, and a struct that
        // owns both would have to name the lifetime for no gain.
        let pipeline = Pipeline::new(&self.dictionaries);
        let mut left = self
            .lindera
            .analyze(&text)
            .map_err(|error| format!("lindera: {error:?}"))?;
        pipeline.display(&mut left);
        let mut right = {
            let _lane = ICHIRAN.lock().unwrap_or_else(|held| held.into_inner());
            self.ichiran
                .analyze(&text)
                .map_err(|error| format!("ichiran: {error:?}"))?
        };
        pipeline.display(&mut right);
        Ok((left, right))
    }
}

fn describe(candidate: &sakatsu_core::hybrid::Candidate) -> String {
    let parts: Vec<String> = candidate
        .assistance
        .iter()
        .map(|part| {
            let mut marks = vec![format!("{:?}", part.answer)];
            if let Some(error) = part.failure() {
                marks.push(format!("LOOKUP FAILED: {error}"));
            }
            if !part.whole_answered {
                marks.push("no whole answer".to_string());
            }
            if let Some(offer) = &part.expression_offer {
                marks.push(format!(
                    "offers {} over {}..{}",
                    offer.expression, offer.start, offer.end
                ));
            }
            format!(
                "{}({} {})",
                part.surface,
                part.displayed.len(),
                marks.join(", ")
            )
        })
        .collect();
    parts.join(" | ")
}

/// Print every region's decision and the passage it assembles.
fn report(decisions: &[hybrid::Decision], assembled: &Analysis) {
    println!(
        "
=== decisions ==="
    );
    for decision in decisions {
        println!(
            "  {}..{} {:?}
    L: {}
    I: {}",
            decision.region.start,
            decision.region.end,
            decision.choice,
            describe(&decision.lindera),
            describe(&decision.ichiran)
        );
    }
    let preferences = decisions
        .iter()
        .filter(|decision| decision.choice.is_a_preference())
        .count();
    let abstentions = decisions
        .iter()
        .filter(|decision| matches!(decision.choice, hybrid::Choice::Abstain { .. }))
        .count();
    println!(
        "  {} regions: {preferences} decided by a rule, {abstentions} abstained",
        decisions.len()
    );
    println!(
        "=== assembled ===
  {}",
        assembled
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect::<Vec<_>>()
            .join(" | ")
    );
}

#[test]
#[ignore = "needs the installed dictionaries and Ichiran"]
fn probe_the_acceptance_cases() {
    let Some(bench) = Bench::new() else {
        panic!("this prototype needs the dictionaries and Ichiran");
    };

    let paired = bench.paired();

    for text in [paired.as_str(), SUPPLIED] {
        println!("\n######## {text} ########");
        let (left, right) = bench.analyse(text);
        let regions = hybrid::regions(&left, &right).expect("same source, grounded spans");
        let prepared_left = hybrid::Prepared::new(Engine::Lindera, &left);
        let prepared_right = hybrid::Prepared::new(Engine::Ichiran, &right);
        for region in regions {
            let lindera = hybrid::candidate(&prepared_left, region, &bench.dictionaries);
            let ichiran = hybrid::candidate(&prepared_right, region, &bench.dictionaries);
            println!(
                "\n  region {}..{} {:?}",
                region.start,
                region.end,
                hybrid::decide(&lindera, &ichiran)
            );
            println!("    L: {}", describe(&lindera));
            println!("    I: {}", describe(&ichiran));
        }
    }
}

/// Find the decision covering a surface, by what either candidate produced there.
fn decision_for<'a>(decisions: &'a [hybrid::Decision], needle: &str) -> &'a hybrid::Decision {
    decisions
        .iter()
        .find(|decision| {
            decision.lindera.surfaces().concat().contains(needle)
                || decision.ichiran.surfaces().concat().contains(needle)
        })
        .unwrap_or_else(|| panic!("no region covers {needle}"))
}

/// **The production lookup is provably the same call.**
///
/// `in_context` is now `with_evidence(…).view`. This is what makes that claim checkable rather
/// than merely stated: every token of both acceptance passages is looked up both ways and the
/// serialized answers compared. A later reimplementation of either — the usual way a diagnostics
/// branch quietly stops matching what ships — fails here.
#[test]
#[ignore = "needs the installed dictionaries and Ichiran"]
fn the_evidence_split_leaves_the_production_answer_identical() {
    let Some(bench) = Bench::new() else {
        panic!("this prototype needs the dictionaries and Ichiran");
    };
    let mut compared = 0usize;
    for text in [bench.paired().as_str(), SUPPLIED] {
        let (left, right) = bench.analyse(text);
        for analysis in [&left, &right] {
            for index in 0..analysis.tokens.len() {
                let baseline =
                    sakatsu_core::lookup::in_context(&analysis.tokens, index, &bench.dictionaries);
                let split = sakatsu_core::lookup::with_evidence(
                    &analysis.tokens,
                    index,
                    &bench.dictionaries,
                );
                match (baseline, split) {
                    (Ok(was), Ok(now)) => {
                        assert_eq!(
                            serde_json::to_string(&was).expect("serializes"),
                            serde_json::to_string(&now.view).expect("serializes"),
                            "token {index} of {text:?} changed"
                        );
                        compared += 1;
                    }
                    (Err(_), Err(_)) => compared += 1,
                    (was, now) => panic!(
                        "one path failed where the other did not: {:?} vs {:?}",
                        was.is_ok(),
                        now.is_ok()
                    ),
                }
            }
        }
    }
    assert!(compared > 40, "only {compared} tokens compared");
    println!("{compared} lookups identical across the split");
}

/// **Acceptance case 1 — both engines contribute to one sentence.**
///
/// This is the case a whole-analysis choice cannot win, and it is why the unit is a region.
#[test]
#[ignore = "needs the installed dictionaries and Ichiran"]
fn each_engine_wins_a_different_region_of_the_same_sentence() {
    let Some(bench) = Bench::new() else {
        panic!("this prototype needs the dictionaries and Ichiran");
    };
    let text = bench.paired();
    let (left, right) = bench.analyse(&text);
    let (decisions, assembled) =
        hybrid::combine(&left, &right, &bench.dictionaries).expect("comparable analyses");

    report(&decisions, &assembled);

    // Ichiran's region: `なんで` whole, against Lindera's `なん`+`で` — where `なん` reaches 難.
    let nande = decision_for(&decisions, "なんで");
    assert_eq!(
        nande.choice,
        hybrid::Choice::PreferIchiran {
            rule: "exact-expression-for-the-region"
        }
    );

    // Lindera's region, decided by a **different** rule — and this is an explicit preference, not
    // the abstention fallback. The distinction is the point: an abstention here would keep the
    // same tokens while deciding nothing.
    let angry = decision_for(&decisions, "怒んなきゃ");
    assert_eq!(
        angry.choice,
        hybrid::Choice::PreferLindera {
            rule: "leaves-no-content-token-unanswered"
        }
    );
    assert!(angry.choice.is_a_preference(), "not an abstention");

    // And the assembled passage really is spliced from both.
    let surfaces: Vec<&str> = assembled
        .tokens
        .iter()
        .map(|token| token.surface.as_str())
        .collect();
    assert!(surfaces.contains(&"なんで"), "ichiran's region is in it");
    assert!(surfaces.contains(&"怒んなきゃ"), "lindera's region is too");
    assert_eq!(
        surfaces.concat(),
        assembled.text,
        "the assembled sequence still spells the passage"
    );
}

/// **Acceptance case 2 — the joined candidate is recognised; the ranking defect is not credited.**
#[test]
#[ignore = "needs the installed dictionaries and Ichiran"]
fn the_joined_candidate_is_preferred_without_crediting_the_suffix_ranking() {
    let Some(bench) = Bench::new() else {
        panic!("this prototype needs the dictionaries and Ichiran");
    };
    let (left, right) = bench.analyse(SUPPLIED);
    let (decisions, assembled) =
        hybrid::combine(&left, &right, &bench.dictionaries).expect("comparable analyses");
    report(&decisions, &assembled);

    let plural = decision_for(&decisions, "あなたたち");
    assert_eq!(
        plural.choice,
        hybrid::Choice::PreferIchiran {
            rule: "exact-expression-for-the-region"
        },
        "the joined candidate answers the whole expression"
    );

    // **The standalone defect is untouched, and the hybrid is not credited with it.** Lindera's
    // losing candidate still strands `たち` on 太刀; selecting around it is not repairing it, and a
    // reader who chooses Built-in still sees the sword.
    let stranded = plural
        .lindera
        .assistance
        .iter()
        .find(|part| part.surface == "たち")
        .expect("lindera strands the suffix");
    assert!(
        !stranded.displayed.is_empty() && stranded.component_answer().is_none(),
        "the stranded suffix is still answered — wrongly, and by a whole-token query"
    );
    println!(
        "   standalone repair still owed: {:?} answered with {} senses in the losing candidate,          leading {:?}",
        stranded.surface,
        stranded.displayed.len(),
        stranded.displayed.first().map(|sense| sense.glosses.join("; "))
    );
}

/// **Acceptance case 3 — an exact name entry is not beaten by definitions reached through 神.**
///
/// Both engines cut `神姫` the same way, so the selector has nothing to choose and says
/// `Equivalent`. That is the honest outcome, and it is *not* the interesting part: what matters is
/// that the evidence records the difference, and that the preference rule would refuse Ichiran's
/// candidate if it were ever asked. A rule counting senses would prefer it eleven to nothing.
#[test]
#[ignore = "needs the installed dictionaries and Ichiran"]
fn component_definitions_never_qualify_as_the_whole_expression() {
    let Some(bench) = Bench::new() else {
        panic!("this prototype needs the dictionaries and Ichiran");
    };
    let (left, right) = bench.analyse(SUPPLIED);
    let (decisions, _) =
        hybrid::combine(&left, &right, &bench.dictionaries).expect("comparable analyses");

    let hime = decision_for(&decisions, "神姫");
    // **Same boundaries, different answers — and that is not equivalence.** Both engines cut
    // `神姫` identically, so no boundary rule is competent; but Lindera shows a name and Ichiran
    // shows `神`'s senses, which a reader would certainly notice. Calling this `Equivalent` would
    // retain Lindera automatically and hide the difference this module exists to surface.
    assert!(
        matches!(hime.choice, hybrid::Choice::Abstain { .. }),
        "expected an abstention that keeps both answers inspectable, got {:?}",
        hime.choice
    );

    // Lindera: an exact entry for the whole, which happens to be a name.
    assert!(
        hime.lindera.component_answers().is_empty(),
        "lindera's answer is about 神姫 itself"
    );
    assert!(
        hime.lindera.answers_the_whole_region(),
        "an exact name entry qualifies — being a name is not being wrong"
    );

    // Ichiran: eleven senses, every one reached through 神.
    assert_eq!(
        hime.ichiran.component_answers(),
        vec![("神姫", "神")],
        "ichiran's answer is retrieved through the component"
    );
    assert!(
        !hime.ichiran.answers_the_whole_region(),
        "so it does not qualify as the region's exact expression, however full the panel looks"
    );
}

/// **Acceptance case 4 — having components does not disqualify a successful whole-token answer.**
///
/// `双星` is the control that keeps case 3's rule honest. It *has* components, so a rule written
/// over "does this token have parts" would reject it; its own query succeeds, so the fallback
/// never runs and the answer is about the whole.
#[test]
#[ignore = "needs the installed dictionaries and Ichiran"]
fn whole_expression_retrieval_survives_the_token_having_components() {
    let Some(bench) = Bench::new() else {
        panic!("this prototype needs the dictionaries and Ichiran");
    };
    let (left, right) = bench.analyse(SUPPLIED);
    let (decisions, _) =
        hybrid::combine(&left, &right, &bench.dictionaries).expect("comparable analyses");

    let twin = decision_for(&decisions, "双星");
    let token = twin
        .lindera
        .tokens
        .first()
        .expect("lindera produced the token");
    assert!(
        !token.components.is_empty(),
        "the control is only a control if the token really does have components"
    );
    assert!(
        twin.lindera.component_answers().is_empty(),
        "the whole-token query succeeded, so no component answered"
    );
    assert!(
        twin.lindera.answers_the_whole_region(),
        "and it still qualifies as the region's exact expression"
    );
}

/// **Which branches are reachable, over an accounted set of captures.**
///
/// A selector that never abstains is not a selector with an abstention branch; it is one whose
/// fallback happens to be unreachable on the cases it was built from. This sweeps the promoted
/// capture transcripts and reports the outcome mix.
///
/// # What these numbers are
///
/// **Heuristic branch counts, not an evaluation result.** They say which code paths the current
/// rules take over this material. They say nothing about whether a choice improved the Japanese —
/// that is reviewed evidence, and no review has happened.
///
/// # Accounting
///
/// Every file is accounted for rather than skipped silently: the two `.txt` files in `corpus/`
/// that are not captures are excluded **by name**, as `capture_cases` does, and unreadable, empty
/// and incomparable inputs are counted and reported.
///
/// **Holdout captures are labelled, not dropped.** All seven images behind `reading-gold.json`
/// have promoted transcripts, so any sweep of `.txt` picks them up; they were read once already
/// and pretending otherwise would misrepresent the record. Their counts are reported separately
/// and never pooled with development.
#[test]
#[ignore = "needs the installed dictionaries and Ichiran"]
fn sweep_the_promoted_captures_for_the_outcome_mix() {
    /// Not captures. Excluded by name rather than by pattern luck.
    const NOT_CAPTURES: [&str; 2] = ["ocr-to-text-corpus.txt", "run-2026-08-25.txt"];

    let Some(bench) = Bench::new() else {
        panic!("this prototype needs the dictionaries and Ichiran");
    };

    // The stems the reading holdout is built on, read so they can be labelled.
    let holdout: std::collections::BTreeSet<String> =
        std::fs::read(common::corpus_dir().join("reading-gold.json"))
            .ok()
            .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
            .and_then(|document| {
                document["cases"].as_array().map(|cases| {
                    cases
                        .iter()
                        .filter_map(|case| case["source_image"].as_str())
                        .filter_map(|image| std::path::Path::new(image).file_stem())
                        .map(|stem| stem.to_string_lossy().to_string())
                        .collect()
                })
            })
            .unwrap_or_default();
    assert!(
        !holdout.is_empty(),
        "the holdout must be identifiable, or nothing below can be labelled"
    );

    #[derive(Default)]
    struct Tally {
        texts: usize,
        prefer_lindera: usize,
        prefer_ichiran: usize,
        equivalent: usize,
        abstain: usize,
        not_evaluated: usize,
    }

    let mut development = Tally::default();
    let mut held_out = Tally::default();
    let mut excluded = 0usize;
    let mut unreadable = 0usize;
    let mut empty = 0usize;
    let mut incomparable: Vec<String> = Vec::new();
    let mut engine_failed: Vec<String> = Vec::new();
    let mut abstained: Vec<String> = Vec::new();

    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(common::corpus_dir())
        .expect("the corpus is required")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"))
        .collect();
    entries.sort();

    for path in entries {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if NOT_CAPTURES.contains(&name.as_str()) {
            excluded += 1;
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            unreadable += 1;
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            empty += 1;
            continue;
        }
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let tally = if holdout.contains(&stem) {
            &mut held_out
        } else {
            &mut development
        };
        tally.texts += 1;

        let (left, right) = match bench.try_analyse(text) {
            Ok(pair) => pair,
            Err(error) => {
                // **Counted, not skipped.** Ichiran is concurrency-1 and its session is shared
                // process-wide; under parallel tests it occasionally answers with malformed JSON
                // rather than an error. That is worth seeing in the accounting rather than
                // discarding the sweep it happens to land in.
                engine_failed.push(format!("{stem}: {error}"));
                continue;
            }
        };
        let combined = hybrid::combine(&left, &right, &bench.dictionaries);
        let decisions = match combined {
            Ok((decisions, _)) => decisions,
            Err(reason) => {
                // **Reported, never skipped.** An analysis that fails its span contract is a
                // finding about the engines, not a file to pass over.
                incomparable.push(format!("{stem}: {reason:?}"));
                continue;
            }
        };
        for decision in decisions {
            match decision.choice {
                hybrid::Choice::PreferLindera { .. } => tally.prefer_lindera += 1,
                hybrid::Choice::PreferIchiran { .. } => tally.prefer_ichiran += 1,
                hybrid::Choice::Equivalent => tally.equivalent += 1,
                hybrid::Choice::NotEvaluated { .. } => tally.not_evaluated += 1,
                hybrid::Choice::Abstain { .. } => {
                    tally.abstain += 1;
                    if abstained.len() < 20 {
                        abstained.push(format!(
                            "{:?} vs {:?}",
                            decision.lindera.surfaces(),
                            decision.ichiran.surfaces()
                        ));
                    }
                }
            }
        }
    }

    let show = |label: &str, tally: &Tally| {
        println!(
            "  {label}: {} transcripts
     prefer-lindera {}, prefer-ichiran {}, equivalent {}, abstain {}, not-evaluated {}",
            tally.texts,
            tally.prefer_lindera,
            tally.prefer_ichiran,
            tally.equivalent,
            tally.abstain,
            tally.not_evaluated
        );
    };
    println!(
        "
=== heuristic branch counts (NOT an evaluation result) ==="
    );
    show("development", &development);
    show(
        "holdout (already read; reported separately, never pooled)",
        &held_out,
    );
    println!(
        "  accounted for: {excluded} excluded by name, {unreadable} unreadable, {empty} empty, {} incomparable, {} engine failures",
        incomparable.len(),
        engine_failed.len()
    );
    for reason in &incomparable {
        println!("    incomparable: {reason}");
    }
    for reason in &engine_failed {
        println!("    engine failed: {reason}");
    }
    assert!(
        engine_failed.len() * 10 < development.texts + held_out.texts,
        "engine failures are not a background rate here: {engine_failed:?}"
    );
    for case in &abstained {
        println!("    abstained: {case}");
    }

    // Every branch the rules can take is reachable on this material.
    assert!(development.prefer_lindera > 0, "no lindera preference");
    assert!(development.prefer_ichiran > 0, "no ichiran preference");
    assert!(development.abstain > 0, "no abstention");
    assert!(development.equivalent > 0, "no equivalence");
}

// **The extraction's behaviour is checked independently, in `study_identity.rs`.**
//
// There is deliberately no test here comparing `study` with `from_analysis`: `study` *is* the
// analysis step plus `from_analysis`, so such a comparison exercises two paths through one
// implementation and proves wiring, not preservation. `study_identity.rs` holds a frozen
// transcription of the pre-move `commands::study` and compares complete rendered responses against
// it, with and without a store — that is the independent check, and it still passes.

/// **What a reader receives from a passage assembled out of both engines.**
///
/// The earlier version of this test asserted array *lengths* and that a slot existed. Neither
/// establishes what it was written for: an array reordered but the same length passes a length
/// check, and a `ruby` entry existing says nothing about which token it describes. The `WordKey`
/// assertion was worse — it compared a token with itself and could not have failed.
///
/// So this asserts **contents at their assembled positions**, against a store whose marks are
/// deliberately nonuniform, and ends by confirming that stale or reordered arrays would fail the
/// same checks.
///
/// The passage is the one where each engine wins a different region:
///
/// ```text
/// なんで | あたし | が | それで | 怒んなきゃ | いかん | の | よ | 。
///   ^ ichiran's                   ^ lindera's
/// ```
///
/// **The losing candidates are what make the store check mean anything.** Lindera's rejected
/// analysis of the first region is `なん` + `で`; those keys are marked `Known` while the chosen
/// `なんで` is marked `Seen`. A view built from the wrong candidate's keys would report `Known` at
/// position 0.
#[test]
#[ignore = "needs the installed dictionaries and Ichiran"]
fn a_mixed_passage_carries_the_right_assistance_at_the_right_positions() {
    let Some(bench) = Bench::new() else {
        panic!("this prototype needs the dictionaries and Ichiran");
    };
    let scratch = common::scratch::Scratch::new("hybrid", "mixed-passage");
    let store = sakatsu_core::store::Store::open(&scratch.join("store")).expect("open");

    let text = bench.paired();
    let (left, right) = bench.analyse(&text);
    let (decisions, assembled) =
        hybrid::combine(&left, &right, &bench.dictionaries).expect("comparable analyses");

    // Both engines really do contribute, or this is a test about Lindera alone.
    assert!(
        decisions
            .iter()
            .any(|decision| matches!(decision.choice, hybrid::Choice::PreferIchiran { .. }))
            && decisions
                .iter()
                .any(|decision| matches!(decision.choice, hybrid::Choice::PreferLindera { .. })),
        "the fixture depends on a mixed assembly"
    );

    // **Selected and rejected keys, recorded distinguishably.** Every token of every losing
    // candidate is marked `Known`; two chosen tokens are marked `Seen`. The statuses the view
    // reports then say which candidate's keys it used.
    let mut rejected = 0usize;
    for decision in &decisions {
        let losing = match decision.choice.engine() {
            hybrid::Engine::Lindera => &decision.ichiran,
            hybrid::Engine::Ichiran => &decision.lindera,
        };
        let chosen: Vec<String> = decision
            .chosen()
            .iter()
            .map(|token| token.surface.clone())
            .collect();
        for token in &losing.tokens {
            // Only tokens the winner did not also produce, or the mark would be ambiguous.
            if chosen.contains(&token.surface) {
                continue;
            }
            store
                .set_status(
                    sakatsu_core::store::WordKey::for_token(token),
                    sakatsu_core::store::WordStatus::Known,
                )
                .expect("mark");
            rejected += 1;
        }
    }
    assert!(
        rejected >= 2,
        "the rejected candidates must contribute distinct keys, or the check is vacuous"
    );

    let position = |surface: &str| {
        assembled
            .tokens
            .iter()
            .position(|token| token.surface == surface)
            .unwrap_or_else(|| panic!("{surface} is in the assembled passage"))
    };
    let joined = position("なんで");
    let held = position("怒んなきゃ");
    for surface in ["なんで", "怒んなきゃ"] {
        let at = position(surface);
        store
            .set_status(
                sakatsu_core::store::WordKey::for_token(&assembled.tokens[at]),
                sakatsu_core::store::WordStatus::Seen,
            )
            .expect("mark");
    }

    let kanji = sakatsu_core::dict::KanjiDic::default();
    let view = sakatsu_core::study::from_analysis(
        assembled,
        sakatsu_core::study::Resources {
            analyzer: &bench.lindera,
            dictionaries: &bench.dictionaries,
            kanji: &kanji,
            store: Some(&store),
        },
    );

    // ---- statuses, at their positions ----
    //
    // `Seen` at both chosen positions is the load-bearing part: the rejected `なん` and `で` carry
    // `Known`, so a view built from Lindera's losing candidate would report that here instead.
    assert_eq!(
        view.statuses[joined],
        sakatsu_core::store::WordStatus::Seen,
        "position {joined} reports the status of the token that shipped, not of a rejected one"
    );
    assert_eq!(view.statuses[held], sakatsu_core::store::WordStatus::Seen);
    assert!(
        view.statuses
            .contains(&sakatsu_core::store::WordStatus::New),
        "the marks must be nonuniform, or an array of one repeated value would compare equal \
         however it were aligned"
    );
    assert!(
        !view
            .statuses
            .contains(&sakatsu_core::store::WordStatus::Known),
        "no rejected candidate's key reached the view — **for this fixture's keys**. The marks          above were placed only on rejected tokens whose surface the winner did not also produce,          so this is evidence about these particular selected and rejected keys. It is not a claim          that two analyses always key differently: two engines can agree on a token exactly, and          then the same key is the right answer for both."
    );

    // ---- ruby, at its position ----
    //
    // `怒んなきゃ` is the only kanji-bearing token here, and furigana is a property of the surface
    // and its reading together — so an entry taken from a neighbour would be visibly wrong.
    let ruby = &view.ruby[held];
    assert!(
        ruby.iter().any(|part| part.reading.is_some()),
        "the kanji token carries a reading: {ruby:?}"
    );
    assert_eq!(
        ruby.iter()
            .map(|part| part.text.as_str())
            .collect::<String>(),
        "怒んなきゃ",
        "and the furigana describes that token, not whichever one shares its index"
    );
    assert!(
        view.ruby[joined].iter().all(|part| part.reading.is_none()),
        "a kana-only token has no furigana — which the kanji one does, so the two are not \
         interchangeable"
    );

    // ---- breakdown, at its position ----
    //
    // Recomputed against the assembled sequence: `怒んなきゃ` is a contracted negative built on
    // 怒る, and its conjugation ladder exists only if the breakdown was derived from this token.
    let breakdown = &view.breakdowns[held];
    assert!(
        breakdown.steps.iter().any(|step| step.lemma == "怒る"),
        "the breakdown at {held} is 怒んなきゃ's own: {breakdown:?}"
    );
    assert!(
        view.breakdowns[joined].steps.is_empty(),
        "and the joined particle has none, so the two cannot be swapped unnoticed"
    );

    // ---- grammar, against the assembled passage ----
    for found in &view.patterns {
        assert!(
            found.last_token_end <= view.analysis.tokens.len(),
            "pattern {} indexes the assembled list",
            found.pattern_id
        );
    }

    // ---- the control: stale or reordered arrays must fail these checks ----
    //
    // Without this the assertions above could all be true of an array that happened to line up.
    // Rotating each one by a position is the cheapest thing that preserves length and content
    // while destroying alignment.
    let rotated_statuses: Vec<_> = view.statuses[1..]
        .iter()
        .chain(view.statuses.first())
        .cloned()
        .collect();
    assert_ne!(
        rotated_statuses[joined], view.statuses[joined],
        "a rotated status array must not still satisfy the position check"
    );
    let rotated_ruby: Vec<_> = view.ruby[1..]
        .iter()
        .chain(view.ruby.first())
        .cloned()
        .collect();
    assert_ne!(
        rotated_ruby[held]
            .iter()
            .map(|part| part.text.as_str())
            .collect::<String>(),
        "怒んなきゃ",
        "a rotated ruby array must not still describe the kanji token"
    );
    let rotated_breakdowns: Vec<_> = view.breakdowns[1..]
        .iter()
        .chain(view.breakdowns.first())
        .cloned()
        .collect();
    assert!(
        !rotated_breakdowns[held]
            .steps
            .iter()
            .any(|step| step.lemma == "怒る"),
        "a rotated breakdown array must not still carry 怒る at that position"
    );

    drop(store);
    println!(
        "mixed passage: {} tokens, statuses {:?}",
        view.analysis.tokens.len(),
        view.statuses
    );
}

/// The exact binary that produced a run, to the byte.
///
/// `runner::executable_digest` is a *human-readable* summary — sixteen hex characters, a size in
/// megabytes, a profile — which is right for a console line and wrong for an artifact anyone might
/// later want to reproduce. An abbreviated hash and a rounded length do not identify a build.
///
/// A constant `RULESET` does not freeze the implementation either: it is a name someone remembers
/// to change. The binary's own digest is not.
fn executable_identity() -> serde_json::Value {
    let path = std::env::current_exe().ok();
    let bytes = path.as_ref().and_then(|path| std::fs::read(path).ok());
    serde_json::json!({
        "sha256": bytes.as_ref().map(|bytes| sakatsu_core::hashing::hex(Sha256::digest(bytes))),
        "bytes": bytes.as_ref().map(|bytes| bytes.len()),
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "path": path.as_ref().map(|path| path.display().to_string()),
        // Kept alongside so a packet can still be matched against console output.
        "summary": runner::executable_digest(),
    })
}

/// Publish a JSON document under a name derived from its own content, atomically.
///
/// **Two properties, and both matter for evidence.**
///
/// - **Unique filename.** A fixed name means the next run overwrites the last, so a packet someone
///   is halfway through reviewing can vanish under them. The digest in the name also makes two
///   packets trivially comparable: same name, same bytes.
/// - **Atomic publication.** Written to a temporary file in the same directory and renamed, so a
///   reader never sees a half-written packet and an interrupted run leaves the previous one
///   intact rather than a truncated file wearing its name.
///
/// Returns the digest and the path.
fn publish(
    directory: &std::path::Path,
    stem: &str,
    body: &serde_json::Value,
) -> (String, std::path::PathBuf) {
    let rendered = serde_json::to_string_pretty(body).expect("serializes");
    let digest = sakatsu_core::hashing::hex(Sha256::digest(rendered.as_bytes()));
    let path = directory.join(format!("{stem}-{}.json", &digest[..16]));
    publish_at(&path, &rendered);
    (digest, path)
}

/// Atomic publication at an exact path, for artifacts whose name is decided by something other
/// than their own content — the judgments file is named for the *packet* it reviews, so that the
/// name can be predicted before the file exists and an existing review can be recognised.
fn publish_at(path: &std::path::Path, rendered: &str) {
    let directory = path.parent().expect("has a parent");
    std::fs::create_dir_all(directory).expect("the artifact directory must exist");
    let staging = directory.join(format!(
        ".{}.partial",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    std::fs::write(&staging, rendered)
        .unwrap_or_else(|error| panic!("the artifact must be written: {error}"));
    std::fs::rename(&staging, path)
        .unwrap_or_else(|error| panic!("the artifact must be published: {error}"));
}

/// What timing evidence exists, and whether any of it actually applies to this packet.
///
/// **Available and applicable are different questions**, and answering only the first is how a
/// packet comes to say "measured" on the strength of a debug run, or a run of a different
/// experiment, or one taken against dictionaries that have since changed.
///
/// A stored measurement is applicable only when **all** of these match:
///
/// - **profile** is `release` — a debug number is not evidence about shipped performance;
/// - **rule set**, because the selector's decisions are what was being timed;
/// - **method**, so a harness that has since been corrected does not lend its numbers to the new
///   one — this is the check the first version of this harness would have failed;
/// - **workload**, hashed from the passages themselves rather than trusted as a label;
/// - **resources**, the dictionary content fingerprints;
/// - **implementation**, the source-tree digest.
///
/// Anything found but not applicable is still listed, with the reasons — a superseded measurement
/// is part of the record, not something to hide.
fn timing_status(evaluation: &std::path::Path, dictionaries: &[Dictionary]) -> serde_json::Value {
    let workload = timing_workload();
    let expected_method = timing_method_id(&workload);
    let expected_resources = resource_id(dictionaries);
    let expected_source = runner::source_digest();

    let Ok(entries) = std::fs::read_dir(evaluation) else {
        return serde_json::json!({
            "applicable_release_measurement": serde_json::Value::Null,
            "status": "pending",
            "reason": "no evaluation directory yet",
            "available_artifacts": [],
        });
    };
    let mut available = Vec::new();
    let mut applicable = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if !name.starts_with("hybrid-timing-") || !name.ends_with(".json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(document) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        // A document that records no identity at all is not a measurement of this experiment —
        // most likely an earlier harness, or a record *about* measurements. Saying "different"
        // where the truth is "absent" would misdescribe it.
        if document["method_id"].is_null() {
            available.push(serde_json::json!({
                "artifact": name,
                "sha256": sakatsu_core::hashing::hex(Sha256::digest(raw.as_bytes())),
                "applies_here": false,
                "why_not": ["records no method identity; not a measurement of this experiment"],
            }));
            continue;
        }
        let mut mismatches = Vec::new();
        let differs = |field: &serde_json::Value, expected: &str| -> Option<&'static str> {
            if field.is_null() {
                Some("absent")
            } else if field != expected {
                Some("different")
            } else {
                None
            }
        };
        if document["executable"]["profile"] != "release" {
            mismatches.push("profile is not release".to_string());
        }
        if let Some(how) = differs(&document["ruleset"], sakatsu_core::hybrid::RULESET) {
            mismatches.push(format!("rule set {how}"));
        }
        if let Some(how) = differs(&document["method_id"], &expected_method) {
            mismatches.push(format!("method or workload {how}"));
        }
        if let Some(how) = differs(&document["resource_id"], &expected_resources) {
            mismatches.push(format!("dictionaries {how}"));
        }
        if let Some(how) = differs(&document["source_tree"], &expected_source) {
            mismatches.push(format!("source tree {how}"));
        }
        let record = serde_json::json!({
            "artifact": name,
            "sha256": sakatsu_core::hashing::hex(Sha256::digest(raw.as_bytes())),
            "ruleset": document["ruleset"].clone(),
            "method": document["method"].clone(),
            "profile": document["executable"]["profile"].clone(),
            "applies_here": mismatches.is_empty(),
            "why_not": mismatches,
        });
        if record["applies_here"] == serde_json::Value::Bool(true) {
            applicable.push(record.clone());
        }
        available.push(record);
    }
    let status = if applicable.is_empty() {
        "pending"
    } else {
        "measured"
    };
    serde_json::json!({
        "status": status,
        "applicable_release_measurement": applicable.first().cloned(),
        "reason": if applicable.is_empty() {
            Some("no timing artifact matches this packet's rule set, method, workload,                   dictionaries and source tree in a release profile")
        } else {
            None
        },
        "available_artifacts": available,
    })
}

/// Render one token's assistance compactly enough for a person to read a whole passage of it.
fn assistance_json(part: &sakatsu_core::hybrid::TokenAssistance) -> serde_json::Value {
    serde_json::json!({
        "surface": part.surface,
        "answer": format!("{:?}", part.answer),
        "whole_answered": part.whole_answered,
        "reading": part.reading,
        "name": part.name,
        "favoured": part.favoured,
        "lookup_failed": part.lookup_failed,
        "expression_failed": part.expression_failed,
        "expression_offer": part.expression_offer.as_ref().map(|offer| serde_json::json!({
            "pattern_id": offer.pattern_id,
            "expression": offer.expression,
            "start": offer.start,
            "end": offer.end,
        })),
        "grammar": part.grammar,
        "senses": part.displayed.iter().map(|sense| serde_json::json!({
            "sequence": sense.sequence,
            "tags": sense.tags,
            "glosses": sense.glosses,
        })).collect::<Vec<_>>(),
    })
}

/// **The review packet: four kinds of evidence, never merged into one number.**
///
/// | Evidence | What it establishes |
/// |---|---|
/// | Original candidate assistance | **why a preference rule fired** — each engine measured inside its own analysis, which is the context it saw |
/// | Assembled passage assistance | **what the reader would actually receive** — recomputed against the spliced token list, whose neighbours differ from either original |
/// | Reviewed judgment | **whether that is better** — absent; this packet exists to obtain it |
/// | Timing | **what it costs** — reported separately, and pending here |
///
/// The first two are the pair most easily confused, and conflating them is how a prototype comes
/// to report assistance for a passage it never assembled. They are written to separate fields and
/// the difference between them is computed, so a reviewer can see where splicing changed the
/// answer rather than having to take it on trust.
///
/// Every judgment field is left empty. **Agent proposals are not review**, and a packet that
/// arrived pre-scored would be one.
#[test]
#[ignore = "needs the installed dictionaries and Ichiran"]
fn write_the_review_packet() {
    let Some(bench) = Bench::new() else {
        panic!("this prototype needs the dictionaries and Ichiran");
    };

    let evaluation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");

    let mut passages = Vec::new();
    let mut sources: Vec<(String, String)> = vec![(
        "screenshot-transcription/supplied".into(),
        SUPPLIED.to_string(),
    )];
    for stem in ["xnviewmp_bu75dRV6Zb", "xnviewmp_rBfma1a8nx"] {
        let text = std::fs::read_to_string(common::corpus_dir().join(format!("{stem}.txt")))
            .expect("the promoted capture is required");
        sources.push((format!("promoted-capture/{stem}"), text.trim().to_string()));
    }

    for (id, text) in &sources {
        let (left, right) = bench.analyse(text);
        let (decisions, assembled) = match hybrid::combine(&left, &right, &bench.dictionaries) {
            Ok(pair) => pair,
            Err(reason) => {
                passages.push(serde_json::json!({
                    "id": id,
                    "text": text,
                    "not_comparable": format!("{reason:?}"),
                }));
                continue;
            }
        };

        // **Recomputed against what was assembled**, not carried over from either engine.
        let received = hybrid::assistance_of(&assembled, &bench.dictionaries);

        let regions: Vec<serde_json::Value> = decisions
            .iter()
            .map(|decision| {
                serde_json::json!({
                    "span": [decision.region.start, decision.region.end],
                    "choice": format!("{:?}", decision.choice),
                    "decided_by_a_rule": decision.choice.is_a_preference(),
                    // Why the rule fired: each candidate as its own engine saw it.
                    "candidates": {
                        "lindera": {
                            "surfaces": decision.lindera.surfaces(),
                            "assistance": decision.lindera.assistance.iter()
                                .map(assistance_json).collect::<Vec<_>>(),
                        },
                        "ichiran": {
                            "surfaces": decision.ichiran.surfaces(),
                            "assistance": decision.ichiran.assistance.iter()
                                .map(assistance_json).collect::<Vec<_>>(),
                        },
                    },
                    "judgment": {
                        "boundaries": null,
                        "learner_grouping": null,
                        "readings": null,
                        "lookup_relevance": null,
                        "reviewer": null,
                    },
                })
            })
            .collect();

        // Where splicing changed what the reader is told, relative to the candidate the rule was
        // decided on. A non-empty list here is the thing a reviewer most needs to see.
        let mut moved = Vec::new();
        for (index, part) in received.iter().enumerate() {
            let from_candidate = decisions.iter().find_map(|decision| {
                let chosen = match decision.choice.engine() {
                    hybrid::Engine::Lindera => &decision.lindera,
                    hybrid::Engine::Ichiran => &decision.ichiran,
                };
                chosen
                    .assistance
                    .iter()
                    .find(|candidate| candidate.surface == part.surface)
            });
            if let Some(before) = from_candidate {
                let same = before.displayed.len() == part.displayed.len()
                    && format!("{:?}", before.answer) == format!("{:?}", part.answer)
                    && before.favoured == part.favoured
                    && before.grammar == part.grammar;
                if !same {
                    moved.push(serde_json::json!({
                        "index": index,
                        "surface": part.surface,
                        "in_candidate": assistance_json(before),
                        "in_assembled_passage": assistance_json(part),
                    }));
                }
            }
        }

        passages.push(serde_json::json!({
            "id": id,
            "text": text,
            "assembled": assembled.tokens.iter()
                .map(|token| token.surface.clone()).collect::<Vec<_>>(),
            "regions": regions,
            "reader_receives": received.iter().map(assistance_json).collect::<Vec<_>>(),
            "assistance_changed_by_assembly": moved,
        }));
    }

    // **Timing is linked, never asserted.** Hard-coding "pending" here would leave a successful
    // release run wearing yesterday's disk-space explanation; the packet reports whichever timing
    // artifact exists for this rule set, and says pending only when there is none.
    let timing = timing_status(&evaluation, &bench.dictionaries);

    let packet = serde_json::json!({
        "what_this_is": "A hybrid prototype review packet. Every judgment field is empty by \
                         design: agent proposals are not review.",
        "ruleset": sakatsu_core::hybrid::RULESET,
        "executable": executable_identity(),
        "source_tree": runner::source_digest(),
        "dictionaries": bench.dictionaries.iter().map(|dictionary| serde_json::json!({
            "title": dictionary.meta().title,
            "entries": dictionary.meta().entry_count,
            "sha256": dictionary.content_fingerprint().expect("content identity"),
        })).collect::<Vec<_>>(),
        "ichiran_route": sakatsu_core::analyzer::ichiran::server::last_route().name(),
        "evidence_kinds": {
            "candidate_assistance": "why a preference rule fired; measured inside each engine's own analysis",
            "reader_receives": "what the reader would receive; recomputed against the assembled token list",
            "judgment": "whether the result is better; ABSENT — this packet exists to obtain it",
            "timing": "what it costs; reported separately",
        },
        "timing": timing,
        "passages": passages,
    });

    // **A failed write fails the run.** An artifact that was only printed is not an artifact.
    let (digest, path) = publish(&evaluation, "hybrid-review-packet", &packet);
    println!("wrote {} ({})", path.display(), &digest[..16]);

    // **Judgments live in their own file, keyed to this packet's digest.** Rerunning the generator
    // publishes a new packet under a new name and cannot touch review already recorded — which a
    // fixed filename could not promise.
    let judgments = evaluation.join(format!(
        "hybrid-review-judgments-for-{}.json",
        &digest[..16]
    ));
    if judgments.exists() {
        println!(
            "judgments already recorded at {}, left untouched",
            judgments.display()
        );
    } else {
        publish_at(
            &judgments,
            &serde_json::to_string_pretty(&serde_json::json!({
                "reviews_packet_sha256": digest,
                "ruleset": sakatsu_core::hybrid::RULESET,
                "instructions": "One entry per region judged. Boundaries, learner grouping,                                  readings and lookup relevance stay separate judgments; multiple                                  acceptable analyses are allowed. Leave anything unjudged absent                                  rather than guessing.",
                "reviewer": null,
                "judgments": [],
            }))
            .expect("serializes"),
        );
        println!("judgment template at {}", judgments.display());
    }

    let written = std::fs::read_to_string(&path).expect("the packet reads back");
    assert_eq!(
        sakatsu_core::hashing::hex(Sha256::digest(written.as_bytes())),
        digest,
        "the published bytes are the bytes that were hashed"
    );
    assert!(
        written.contains(sakatsu_core::hybrid::RULESET),
        "the packet names the rule set it was produced under"
    );
}

/// Summarise a set of durations the way a reader of a benchmark needs them.
fn summarise(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
    let at = |q: f64| -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        let index = ((samples.len() as f64 - 1.0) * q).round() as usize;
        samples[index]
    };
    serde_json::json!({
        "n": samples.len(),
        "median_ms": at(0.5),
        "p95_ms": at(0.95),
        "min_ms": samples.first().copied().unwrap_or(0.0),
        "max_ms": samples.last().copied().unwrap_or(0.0),
        // Spread as the gap between quartiles: a single slow sample moves max and not this.
        "iqr_ms": at(0.75) - at(0.25),
    })
}

/// The passages this experiment runs, chosen before anything is timed.
///
/// Shared with [`timing_status`] so applicability can be *checked* rather than asserted: a stored
/// measurement is only about this experiment if it ran this workload.
fn timing_workload() -> Vec<(String, String)> {
    let mut promoted: Vec<(String, String)> = std::fs::read_dir(common::corpus_dir())
        .expect("the corpus is required")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"))
        .filter(|path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            name != "ocr-to-text-corpus.txt" && name != "run-2026-08-25.txt"
        })
        .filter_map(|path| {
            let stem = path.file_stem()?.to_string_lossy().to_string();
            let text = std::fs::read_to_string(&path).ok()?;
            let text = text.trim().to_string();
            (!text.is_empty()).then_some((stem, text))
        })
        .collect();
    promoted.sort();

    let short = ("short/supplied".to_string(), SUPPLIED.to_string());
    let long = promoted
        .iter()
        .max_by_key(|(_, text)| text.chars().count())
        .cloned()
        .map(|(stem, text)| (format!("long/{stem}"), text))
        .expect("a longest capture exists");
    let pilot = promoted
        .iter()
        .take(10)
        .cloned()
        .map(|(stem, text)| (format!("pilot/{stem}"), text));
    std::iter::once(short)
        .chain(std::iter::once(long))
        .chain(pilot)
        .collect()
}

/// How this experiment is run, as an identity rather than a description.
///
/// Changing the workload, the repetition count or what each configuration does changes what the
/// numbers mean. Hashing them is what lets a later packet decide whether a stored measurement is
/// *about* it, instead of trusting a label.
const TIMING_METHOD: &str = "interleaved-3rep-endtoend-v2";

fn timing_method_id(workload: &[(String, String)]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(TIMING_METHOD.as_bytes());
    for (id, text) in workload {
        hasher.update(id.as_bytes());
        hasher.update(text.len().to_le_bytes());
    }
    sakatsu_core::hashing::hex(hasher.finalize())
}

fn resource_id(dictionaries: &[Dictionary]) -> String {
    let mut hasher = Sha256::new();
    for dictionary in dictionaries {
        hasher.update(dictionary.meta().title.as_bytes());
        hasher.update(
            dictionary
                .content_fingerprint()
                .expect("content identity")
                .as_bytes(),
        );
    }
    sakatsu_core::hashing::hex(hasher.finalize())
}

/// **What the hybrid costs, measured rather than assumed.**
///
/// # What is separated, and why
///
/// - **Warm execution** — the steady state, and the number that answers "is this too slow to read
///   with". Each configuration is timed **end to end**, from raw text to a finished `StudyView`.
/// - **Selection and assembly alone** — the hybrid's own machinery, timed over analyses that are
///   already in hand, so it can be told apart from the engine time it sits on top of.
/// - **Repeated warm execution** — the same passage again. Deliberately *not* called a cache
///   measurement: nothing here bypasses the analyzer, so it is a second warm sample and named as
///   one.
/// - **Warm-client initialization** — constructing a client against an Ichiran session this
///   process has **already warmed**. It is not the cost of starting a sidecar and does not claim
///   to be; measuring that honestly needs an isolated child process with its own session, and this
///   harness will not stop a session the user's other work may be using.
///
/// # Why interleaved
///
/// Three repetitions, and the three configurations **rotate** across them — Lindera-first,
/// Ichiran-first, Hybrid-first — so a machine that warms up, throttles or picks up background load
/// during the run affects all three roughly equally instead of penalising whichever always went
/// last.
///
/// # Reading the summary
///
/// The pooled rows describe **this mixture of passages**, not repeatability for any one of them: a
/// wide IQR here is mostly the difference between a 5-character line and a 119-character one. The
/// per-passage rows are what show variability, and the short/long rows are called out because they
/// are the two ends of the mixture.
#[test]
#[ignore = "needs the installed dictionaries and Ichiran; run in release for meaningful numbers"]
fn measure_what_the_hybrid_costs() {
    let Some(bench) = Bench::new() else {
        panic!("this measurement needs the dictionaries and Ichiran");
    };
    let evaluation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");

    let workload = timing_workload();
    assert!(workload.len() >= 12, "the pilot subset needs material");

    let kanji = sakatsu_core::dict::KanjiDic::default();
    let resources = || sakatsu_core::study::Resources {
        analyzer: &bench.lindera,
        dictionaries: &bench.dictionaries,
        kanji: &kanji,
        store: None,
    };
    let pipeline_of = || Pipeline::new(&bench.dictionaries);

    /// One complete run of one configuration: raw text to a finished study response.
    ///
    /// **Each arm assembles exactly one `StudyView`.** An earlier version added the two standalone
    /// measurements together and called the sum the hybrid's cost, which counted three study
    /// assemblies for a path that performs one.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Config {
        Lindera,
        IchiranSession,
        Hybrid,
    }
    impl Config {
        fn key(self) -> &'static str {
            match self {
                Config::Lindera => "lindera",
                Config::IchiranSession => "ichiran_session",
                Config::Hybrid => "hybrid_end_to_end",
            }
        }
    }

    // A warm-client measurement, named for what it is. `Bench::new` has already opened the
    // persistent session, so this constructs a client against a session that exists.
    let started = std::time::Instant::now();
    let cold_lindera = LinderaAnalyzer::new().expect("embedded UniDic");
    let lindera_construct_ms = started.elapsed().as_secs_f64() * 1000.0;
    drop(cold_lindera);
    let started = std::time::Instant::now();
    let (ichiran_client_ms, ichiran_probe) = {
        let _lane = ICHIRAN.lock().unwrap_or_else(|held| held.into_inner());
        let engine = IchiranAnalyzer::new();
        engine.warm();
        // **The probe's result is kept.** Discarding it would let a failed first query be timed
        // and reported as though it had succeeded.
        let probe = engine.analyze("これって");
        (started.elapsed().as_secs_f64() * 1000.0, probe.is_ok())
    };
    assert!(ichiran_probe, "the warm-client probe must actually answer");

    let mut warm: std::collections::BTreeMap<&str, Vec<f64>> = std::collections::BTreeMap::new();
    let mut selection_only: Vec<f64> = Vec::new();
    let mut per_passage: Vec<serde_json::Value> = Vec::new();
    let mut incomparable: Vec<serde_json::Value> = Vec::new();

    // One untimed pass so no configuration pays a first-touch cost the others do not.
    for (_, text) in &workload {
        let _ = bench.try_analyse(text);
    }

    for repetition in 0..3 {
        // **The rotation.** Order changes every repetition, so no configuration is always last.
        let order = match repetition {
            0 => [Config::Lindera, Config::IchiranSession, Config::Hybrid],
            1 => [Config::IchiranSession, Config::Hybrid, Config::Lindera],
            _ => [Config::Hybrid, Config::Lindera, Config::IchiranSession],
        };
        for (id, text) in &workload {
            let normalised = sakatsu_core::analyzer::lines::normalise_breaks(text)
                .trim()
                .to_string();
            let mut timings: std::collections::BTreeMap<&str, Option<f64>> =
                std::collections::BTreeMap::new();
            let mut note = None;

            for config in order {
                match config {
                    Config::Lindera => {
                        let started = std::time::Instant::now();
                        let mut analysis = bench
                            .lindera
                            .analyze(&normalised)
                            .expect("lindera analyses");
                        pipeline_of().display(&mut analysis);
                        let _ = sakatsu_core::study::from_analysis(analysis, resources());
                        timings
                            .insert(config.key(), Some(started.elapsed().as_secs_f64() * 1000.0));
                    }
                    Config::IchiranSession => {
                        let started = std::time::Instant::now();
                        let mut analysis = {
                            let _lane = ICHIRAN.lock().unwrap_or_else(|held| held.into_inner());
                            bench
                                .ichiran
                                .analyze(&normalised)
                                .expect("ichiran analyses")
                        };
                        pipeline_of().display(&mut analysis);
                        let _ = sakatsu_core::study::from_analysis(analysis, resources());
                        timings
                            .insert(config.key(), Some(started.elapsed().as_secs_f64() * 1000.0));
                    }
                    Config::Hybrid => {
                        // **One invocation, one study assembly.** Both engines, selection, and a
                        // single `from_analysis` over the assembled tokens.
                        let started = std::time::Instant::now();
                        let mut left = bench
                            .lindera
                            .analyze(&normalised)
                            .expect("lindera analyses");
                        pipeline_of().display(&mut left);
                        let mut right = {
                            let _lane = ICHIRAN.lock().unwrap_or_else(|held| held.into_inner());
                            bench
                                .ichiran
                                .analyze(&normalised)
                                .expect("ichiran analyses")
                        };
                        pipeline_of().display(&mut right);
                        match hybrid::combine(&left, &right, &bench.dictionaries) {
                            Ok((_, assembled)) => {
                                let _ = sakatsu_core::study::from_analysis(assembled, resources());
                                timings.insert(
                                    config.key(),
                                    Some(started.elapsed().as_secs_f64() * 1000.0),
                                );
                            }
                            Err(reason) => {
                                // **Kept, not dropped.** A passage the hybrid cannot assemble is a
                                // result about the hybrid; letting its sample vanish would quietly
                                // remove the hardest inputs from the hybrid's own summary.
                                timings.insert(config.key(), None);
                                note = Some(format!("{reason:?}"));
                                incomparable.push(serde_json::json!({
                                    "repetition": repetition,
                                    "id": id,
                                    "reason": format!("{reason:?}"),
                                }));
                            }
                        }
                    }
                }
            }

            // Selection and assembly alone, over analyses already in hand. Separate from the
            // end-to-end arms and never added to them.
            if note.is_none() {
                let mut left = bench
                    .lindera
                    .analyze(&normalised)
                    .expect("lindera analyses");
                pipeline_of().display(&mut left);
                let mut right = {
                    let _lane = ICHIRAN.lock().unwrap_or_else(|held| held.into_inner());
                    bench
                        .ichiran
                        .analyze(&normalised)
                        .expect("ichiran analyses")
                };
                pipeline_of().display(&mut right);
                let started = std::time::Instant::now();
                if let Ok((_, assembled)) = hybrid::combine(&left, &right, &bench.dictionaries) {
                    let _ = sakatsu_core::study::from_analysis(assembled, resources());
                    selection_only.push(started.elapsed().as_secs_f64() * 1000.0);
                }
            }

            for (key, sample) in &timings {
                if let Some(ms) = sample {
                    warm.entry(key).or_default().push(*ms);
                }
            }
            per_passage.push(serde_json::json!({
                "repetition": repetition,
                "order": order.iter().map(|c| c.key()).collect::<Vec<_>>(),
                "id": id,
                "characters": normalised.chars().count(),
                "lindera_ms": timings.get("lindera").copied().flatten(),
                "ichiran_session_ms": timings.get("ichiran_session").copied().flatten(),
                "hybrid_end_to_end_ms": timings.get("hybrid_end_to_end").copied().flatten(),
                "hybrid_not_assembled": note,
            }));
        }
    }

    // Repeated warm execution of one passage — **not** a cache measurement.
    let (_, repeated) = &workload[0];
    let normalised = sakatsu_core::analyzer::lines::normalise_breaks(repeated)
        .trim()
        .to_string();
    let mut repeats = Vec::new();
    for _ in 0..3 {
        let started = std::time::Instant::now();
        let mut again = bench.lindera.analyze(&normalised).expect("analyses");
        pipeline_of().display(&mut again);
        repeats.push(started.elapsed().as_secs_f64() * 1000.0);
    }

    // Per-passage paired rows: the same passage across configurations, which is where
    // repeatability is visible and where passage difficulty stops confounding it.
    let mut paired: Vec<serde_json::Value> = Vec::new();
    for (id, text) in &workload {
        let rows: Vec<&serde_json::Value> = per_passage
            .iter()
            .filter(|row| row["id"] == serde_json::Value::String(id.clone()))
            .collect();
        let column =
            |key: &str| -> Vec<f64> { rows.iter().filter_map(|row| row[key].as_f64()).collect() };
        paired.push(serde_json::json!({
            "id": id,
            "characters": text.chars().count(),
            "lindera": summarise(column("lindera_ms")),
            "ichiran_session": summarise(column("ichiran_session_ms")),
            "hybrid_end_to_end": summarise(column("hybrid_end_to_end_ms")),
        }));
    }

    let method_id = timing_method_id(&workload);
    let artifact = serde_json::json!({
        "what_this_is": "A timing experiment for the hybrid prototype. Each configuration is timed \
                         end to end and assembles exactly one StudyView. Pooled rows describe this \
                         mixture of passages, not repeatability for one of them.",
        "ruleset": sakatsu_core::hybrid::RULESET,
        "method": TIMING_METHOD,
        "method_id": method_id,
        "resource_id": resource_id(&bench.dictionaries),
        "executable": executable_identity(),
        "source_tree": runner::source_digest(),
        "dictionaries": bench.dictionaries.iter().map(|dictionary| serde_json::json!({
            "title": dictionary.meta().title,
            "entries": dictionary.meta().entry_count,
            "sha256": dictionary.content_fingerprint().expect("content identity"),
        })).collect::<Vec<_>>(),
        "ichiran_route": sakatsu_core::analyzer::ichiran::server::last_route().name(),
        "design": {
            "repetitions": 3,
            "rotation": "configuration order rotates each repetition",
            "workload": workload.iter().map(|(id, text)| serde_json::json!({
                "id": id,
                "characters": text.chars().count(),
            })).collect::<Vec<_>>(),
            "hybrid_end_to_end": "both engines, selection, and one from_analysis — a single \
                                  invocation, not a sum of the standalone arms",
        },
        "warm_client_initialization_ms": {
            "lindera_construct": lindera_construct_ms,
            "ichiran_client_against_an_already_warm_session": ichiran_client_ms,
            "caveat": "NOT sidecar startup: this process had already warmed the session, and a \
                       new client does not create another. Measuring cold start needs an isolated \
                       child process with its own session.",
        },
        "warm_execution_pooled": warm.iter()
            .map(|(name, samples)| (name.to_string(), summarise(samples.clone())))
            .collect::<serde_json::Map<_, _>>(),
        "selection_and_assembly_only": summarise(selection_only.clone()),
        "repeated_warm_execution": {
            "note": "the same passage analysed again; no cache is bypassed or exercised",
            "samples": summarise(repeats),
        },
        "per_passage_paired": paired,
        "hybrid_not_assembled": incomparable,
        "per_passage": per_passage,
    });

    let (digest, path) = publish(&evaluation, "hybrid-timing", &artifact);
    println!("wrote {} ({})", path.display(), &digest[..16]);
    if cfg!(debug_assertions) {
        println!("NOTE: debug profile — not release evidence; the artifact records the profile");
    }
    for (name, samples) in &warm {
        let summary = summarise(samples.clone());
        println!(
            "  {name:<20} median {:>8.1} ms  p95 {:>8.1} ms  n {}",
            summary["median_ms"].as_f64().unwrap_or(0.0),
            summary["p95_ms"].as_f64().unwrap_or(0.0),
            summary["n"].as_u64().unwrap_or(0)
        );
    }
    let selection = summarise(selection_only);
    println!(
        "  {:<20} median {:>8.1} ms  p95 {:>8.1} ms  n {}",
        "selection_only",
        selection["median_ms"].as_f64().unwrap_or(0.0),
        selection["p95_ms"].as_f64().unwrap_or(0.0),
        selection["n"].as_u64().unwrap_or(0)
    );
    if !incomparable.is_empty() {
        println!(
            "  {} passages the hybrid could not assemble",
            incomparable.len()
        );
    }
}

/// Which kind of case a region is, so the batch can be built to cover all of them.
fn category(choice: &hybrid::Choice) -> &'static str {
    match choice {
        hybrid::Choice::PreferLindera { .. } | hybrid::Choice::PreferIchiran { .. } => "preference",
        hybrid::Choice::Equivalent => "agreement-control",
        hybrid::Choice::Abstain { .. } => "abstention",
        hybrid::Choice::NotEvaluated { .. } => "not-evaluated",
    }
}

/// How many senses appear inline before the reader is pointed at the appendix.
const SHOWN_INLINE: usize = 2;

/// The grammar a reader would actually be shown for a token: resolved patterns, and how the word
/// is built.
///
/// **A pattern id is not assistance.** `grammar: particle-de` tells a reviewer nothing about
/// whether the app explains anything; the pack's own title and explanation do. And for a
/// contracted or compound form, the dictionary entry for its lemma is not evidence either — a
/// definition of 怒る says nothing about whether 怒んなきゃ is explained as a negative conditional.
fn render_grammar(token: &sakatsu_core::Token, part: &hybrid::TokenAssistance) -> String {
    let mut out = String::new();
    for id in &part.grammar {
        match sakatsu_core::grammar::pack::find(id) {
            Some(pattern) => {
                out.push_str(&format!(
                    "        grammar — {} ({}): {}\n",
                    pattern.pattern, pattern.title, pattern.explanation
                ));
                if let Some(formation) = &pattern.formation {
                    out.push_str(&format!("          formed: {formation}\n"));
                }
            }
            // Said plainly rather than hidden: a matched pattern the packs cannot resolve is
            // assistance the reader does not get either.
            None => out.push_str(&format!(
                "        grammar — pattern `{id}` matched, but no bundled pack explains it\n"
            )),
        }
    }
    let breakdown = sakatsu_core::grammar::breakdown(token);
    if !breakdown.steps.is_empty() {
        let ladder: Vec<String> = breakdown
            .steps
            .iter()
            .map(|step| {
                let mut rung = step.label.clone();
                if step.neg {
                    rung.push_str(" (negative)");
                }
                if step.fml {
                    rung.push_str(" (polite)");
                }
                match &step.formation {
                    Some(formation) => format!(
                        "{rung}: {} → {} [{}]",
                        formation.from, formation.result, formation.change
                    ),
                    None => rung,
                }
            })
            .collect();
        out.push_str(&format!(
            "        built from {}【{}】: {}\n",
            breakdown
                .steps
                .first()
                .map(|step| step.lemma.as_str())
                .unwrap_or("?"),
            breakdown
                .steps
                .first()
                .map(|step| step.base_reading.as_str())
                .unwrap_or(""),
            ladder.join("  ·  ")
        ));
    }
    for auxiliary in &breakdown.auxiliaries {
        if let Some(pattern) = sakatsu_core::grammar::pack::find(auxiliary.pattern_id) {
            out.push_str(&format!(
                "        auxiliary — {} ({}): {}\n",
                pattern.pattern, pattern.title, pattern.explanation
            ));
        }
    }
    out
}

/// One candidate rendered for a reader: what it cuts, how it reads, and what it would tell them.
///
/// The first [`SHOWN_INLINE`] senses appear here because that is roughly what the panel leads
/// with; **the rest are in the case's appendix, never dropped.** Showing only the first two and
/// stopping would turn "the right answer is buried" into "the right answer is absent", which are
/// different defects with different fixes — and it is the first one this investigation actually
/// found.
fn render_alternative(candidate: &hybrid::Candidate) -> String {
    let mut out = String::new();
    for part in &candidate.assistance {
        let token = candidate
            .tokens
            .iter()
            .find(|token| token.surface == part.surface);
        let reading = token
            .map(|token| token.reading.clone())
            .filter(|reading| !reading.is_empty() && *reading != part.surface)
            .map(|reading| format!("【{reading}】"))
            .unwrap_or_default();
        out.push_str(&format!("      {}{}\n", part.surface, reading));
        match &part.answer {
            hybrid::Answer::Nothing => out.push_str("        (no dictionary answer)\n"),
            hybrid::Answer::Component { component, .. } => out.push_str(&format!(
                "        ⚠ definitions retrieved for the component 「{component}」, shown under \
                 the whole word\n"
            )),
            hybrid::Answer::NameOnly { .. } => {
                out.push_str("        (a name entry, no ordinary definition)\n")
            }
            _ => {}
        }
        if let Some(name) = &part.name {
            out.push_str(&format!("        name: {name}\n"));
        }
        for sense in part.displayed.iter().take(SHOWN_INLINE) {
            out.push_str(&format!("        · {}\n", sense.glosses.join("; ")));
        }
        if part.displayed.len() > SHOWN_INLINE {
            out.push_str(&format!(
                "        · … {} further senses — all of them, in order, in the appendix below\n",
                part.displayed.len() - SHOWN_INLINE
            ));
        }
        if let Some(offer) = &part.expression_offer {
            out.push_str(&format!(
                "        expression offered: 「{}」\n",
                offer.expression
            ));
        }
        if let Some(token) = token {
            out.push_str(&render_grammar(token, part));
        }
    }
    out
}

/// Every sense either analysis retrieved, in the order the panel would show them.
fn render_appendix(label: &str, candidate: &hybrid::Candidate) -> String {
    let mut out = String::new();
    for part in &candidate.assistance {
        if part.displayed.len() <= SHOWN_INLINE {
            continue;
        }
        out.push_str(&format!(
            "**{label} — {} ({} senses in full)**\n\n",
            part.surface,
            part.displayed.len()
        ));
        for (index, sense) in part.displayed.iter().enumerate() {
            let tags = if sense.tags.is_empty() {
                String::new()
            } else {
                format!(" _{}_", sense.tags.join(", "))
            };
            out.push_str(&format!(
                "{}. {}{tags}\n",
                index + 1,
                sense.glosses.join("; ")
            ));
        }
        out.push('\n');
    }
    out
}

/// **A small, readable review batch — blinded.**
///
/// The point of this pass is no longer engineering. Always-consulting-both costs Ichiran-class
/// latency, so the hybrid has to justify itself on **assistance**, and nothing measured so far can
/// say whether it does. Only a person reading the alternatives can.
///
/// # What is hidden, and why
///
/// Each region is shown as **Analysis A** and **Analysis B**, in an order decided by a hash of the
/// case id — so it is stable across regenerations but carries no information. The engine that
/// produced each, which one the selector chose, and the rule it fired are **not in this file**.
/// They are in a separate key, written alongside.
///
/// A reviewer who can see that the tool preferred B is no longer judging the Japanese. That is the
/// only thing this batch is for.
///
/// # What is asked
///
/// Four separate questions, because they have different answers: an analysis can be correct and
/// less useful, or well-grouped and wrongly explained. **Every question offers "both acceptable",
/// "both inadequate" and "not enough context"** — a forced winner would manufacture a preference
/// where the honest answer is that neither is better.
#[test]
#[ignore = "needs the installed dictionaries and Ichiran"]
fn write_the_blinded_review_batch() {
    let Some(bench) = Bench::new() else {
        panic!("this batch needs the dictionaries and Ichiran");
    };
    let evaluation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");

    // The named cases come first: they are the ones the whole investigation turned on.
    let mut wanted: Vec<(&str, String)> = vec![
        ("supplied screenshot", SUPPLIED.to_string()),
        ("promoted capture", bench.paired()),
    ];
    let second = std::fs::read_to_string(common::corpus_dir().join("xnviewmp_rBfma1a8nx.txt"))
        .expect("the promoted capture is required");
    wanted.push(("promoted capture", second.trim().to_string()));

    // A few more promoted transcripts, taken in sorted order so the selection cannot drift, to
    // fill out the abstentions and agreement controls the named passages do not supply.
    let mut extra: Vec<String> = std::fs::read_dir(common::corpus_dir())
        .expect("the corpus is required")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"))
        .filter(|path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            name != "ocr-to-text-corpus.txt" && name != "run-2026-08-25.txt"
        })
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let text = text.trim().to_string();
            (text.chars().count() > 12 && text.chars().count() < 60).then_some(text)
        })
        .collect();
    extra.sort();
    for text in extra.into_iter().take(6) {
        wanted.push(("promoted capture", text));
    }

    struct Case {
        id: String,
        sentence: String,
        span: (usize, usize),
        category: &'static str,
        interest: String,
        first: hybrid::Candidate,
        second: hybrid::Candidate,
        choice: hybrid::Choice,
    }
    let mut cases: Vec<Case> = Vec::new();

    for (provenance, text) in &wanted {
        let (left, right) = bench.analyse(text);
        let Ok((decisions, _)) = hybrid::combine(&left, &right, &bench.dictionaries) else {
            continue;
        };
        for decision in decisions {
            let differs = decision.lindera.surfaces() != decision.ichiran.surfaces();
            let assistance_differs = decision.lindera.component_answers().is_empty()
                != decision.ichiran.component_answers().is_empty();
            // **Agreement controls belong in the batch.** A reviewer shown only disputes cannot
            // tell whether they are judging real differences or pattern-matching on being asked;
            // a case where the two analyses are identical is the check on that, and it has to look
            // exactly like every other case.
            let agreed = matches!(decision.choice, hybrid::Choice::Equivalent)
                && decision
                    .lindera
                    .assistance
                    .iter()
                    .any(|part| !part.displayed.is_empty());
            let keep = differs || assistance_differs || agreed;
            if !keep {
                continue;
            }
            let surfaces = decision.lindera.surfaces().concat();
            let interest = if surfaces.contains('神') {
                "whole expression or a component?".to_string()
            } else if matches!(decision.choice, hybrid::Choice::Equivalent) {
                "both engines cut this identically".to_string()
            } else {
                "the two cut this differently".to_string()
            };
            cases.push(Case {
                id: format!(
                    "{provenance}/{}-{}",
                    decision.region.start, decision.region.end
                ),
                sentence: text.clone(),
                span: (decision.region.start, decision.region.end),
                category: category(&decision.choice),
                interest,
                first: decision.lindera.clone(),
                second: decision.ichiran.clone(),
                choice: decision.choice.clone(),
            });
        }
    }

    // Cover every category. Preferences in both directions come from the named passages; the
    // remainder fills up to twelve without letting one category crowd the batch.
    let mut chosen: Vec<&Case> = Vec::new();
    for category in ["preference", "abstention", "agreement-control"] {
        for case in cases.iter().filter(|case| case.category == category) {
            if chosen.len() >= 12 {
                break;
            }
            let of_this_kind = chosen.iter().filter(|c| c.category == category).count();
            let cap = match category {
                "preference" => 6,
                "abstention" => 4,
                _ => 3,
            };
            if of_this_kind < cap {
                chosen.push(case);
            }
        }
    }
    // Deliberately no blanket top-up: filling the remainder from whatever is left would let
    // preferences crowd out the controls the batch exists to include.
    assert!(
        chosen.len() >= 10,
        "the batch needs enough cases to be worth reading: {}",
        chosen.len()
    );
    for required in ["preference", "abstention", "agreement-control"] {
        assert!(
            chosen.iter().any(|case| case.category == required),
            "the batch must contain at least one {required} case"
        );
    }

    let mut batch = String::new();
    batch.push_str(
        "# Hybrid analyzer: blinded review batch\n\n\
         Each case shows one stretch of a sentence, analysed two ways. **Which tool produced \
         which, and which one was picked, are deliberately not in this file.**\n\n\
         **Start with cases 1–4.** They are a tranche, not a sample to finish: the point of \
         stopping there is to learn whether the questions are answerable and what the work \
         actually costs, before twelve of them are committed to. The remaining eight are included \
         so nothing has to be regenerated if the first four go well.\n\n\
         Every question offers *both are fine*, *neither is good enough*, and *I can't tell from \
         this*. Those are real answers. **Linguistic uncertainty should stay explicit** — nothing \
         here asks for an analysis to be certified as correct, and an honest \"not enough \
         context\" is worth more than a guess.\n\n\
         Two things to record besides the answers: **where a question was unclear or did not fit \
         the case**, and **how long the first four actually took**. Both have fields in the \
         judgments file.\n\n---\n\n",
    );

    let mut key = Vec::new();
    for (index, case) in chosen.iter().enumerate() {
        // Stable, uninformative order.
        let flip = {
            let mut hasher = Sha256::new();
            hasher.update(case.id.as_bytes());
            hasher.finalize()[0] % 2 == 1
        };
        let (a, b) = if flip {
            (&case.second, &case.first)
        } else {
            (&case.first, &case.second)
        };
        let number = index + 1;
        let (start, end) = case.span;
        let focus = &case.sentence[start.min(case.sentence.len())..end.min(case.sentence.len())];

        batch.push_str(&format!("## Case {number}\n\n"));
        batch.push_str(&format!("**Sentence.** {}\n\n", case.sentence));
        batch.push_str(&format!(
            "**The part in question.** 「{focus}」 — {}\n\n",
            case.interest
        ));
        batch.push_str("### Analysis A\n\n");
        batch.push_str(&format!("    {}\n", a.surfaces().join(" | ")));
        batch.push_str(&render_alternative(a));
        batch.push_str("\n### Analysis B\n\n");
        batch.push_str(&format!("    {}\n", b.surfaces().join(" | ")));
        batch.push_str(&render_alternative(b));
        batch.push_str(
            "\n### Questions\n\n\
             **Q1 — segmentation and readings only.** Ignore the definitions here; judge only \
             where the words are cut and how they are read. *A is wrong / B is wrong / both are \
             acceptable / both are inadequate / not enough context.*\n\n\
             **Q2 — which grouping is more useful** to select and look up? *A / B / no difference \
             / both are poor / not enough context.*\n\n\
             **Q3a — the answer as presented.** Taking only the senses shown above, does it \
             describe the whole expression, a component, or something that does not belong here? \
             *Answer for A and for B.*\n\n\
             **Q3b — the answer available.** Now read the full list in the appendix. Is an \
             appropriate sense present anywhere in it? *Answer for A and for B.* Q3a and Q3b part \
             company when the right answer is retrieved but buried — a different defect from its \
             being missing.\n\n\
             **Q4 — is there enough context** in this sentence to decide? *yes / no.*\n\n\
             Notes:\n\n",
        );
        let appendix = format!("{}{}", render_appendix("A", a), render_appendix("B", b));
        if appendix.is_empty() {
            batch.push_str("_(No sense list was truncated in this case.)_\n\n---\n\n");
        } else {
            batch.push_str(&format!(
                "<details>\n<summary>Appendix — every sense, in the order shown</summary>\n\n\
                 {appendix}</details>\n\n---\n\n"
            ));
        }

        key.push(serde_json::json!({
            "case": number,
            "id": case.id,
            "shown_as_a": if flip { "ichiran" } else { "lindera" },
            "shown_as_b": if flip { "lindera" } else { "ichiran" },
            "selector_choice": format!("{:?}", case.choice),
            "category": case.category,
        }));
    }

    let judgments_name = "hybrid-review-judgments-blinded.json";
    batch.push_str(&format!(
        "Answers go in `{judgments_name}`. The key naming which tool produced A and B in each \
         case is held separately and should not be opened until the answers are recorded.\n"
    ));

    std::fs::create_dir_all(&evaluation).expect("create");
    let digest = sakatsu_core::hashing::hex(Sha256::digest(batch.as_bytes()));
    let batch_path = evaluation.join(format!("hybrid-review-batch-{}.md", &digest[..16]));
    publish_at(&batch_path, &batch);

    let key_document = serde_json::json!({
        "what_this_is": "The unblinding key for a review batch. Not to be read before the \
                         judgments are recorded.",
        "batch": batch_path.file_name().unwrap_or_default().to_string_lossy(),
        "batch_sha256": digest,
        "ruleset": sakatsu_core::hybrid::RULESET,
        "cases": key,
    });
    let (key_digest, key_path) = publish(&evaluation, "hybrid-review-key", &key_document);

    let judgments = evaluation.join(format!(
        "hybrid-review-judgments-blinded-for-{}.json",
        &digest[..16]
    ));
    if !judgments.exists() {
        publish_at(
            &judgments,
            &serde_json::to_string_pretty(&serde_json::json!({
                "reviews_batch_sha256": digest,
                "ruleset": sakatsu_core::hybrid::RULESET,
                "reviewer": null,
                "instructions": "One object per case: {case, q1_segmentation_and_readings,                                  q2_more_useful, q3a_as_presented_a, q3a_as_presented_b,                                  q3b_available_a, q3b_available_b, q4_enough_context, notes}.                                  Leave a field absent rather than guessing.",
                "start_with": [1, 2, 3, 4],
                "minutes_spent_on_first_four": null,
                "questions_that_were_unclear": [],
                "answers": [],
            }))
            .expect("serializes"),
        );
    }

    println!("batch  {}", batch_path.display());
    println!("key    {} ({})", key_path.display(), &key_digest[..16]);
    println!("answers go in {}", judgments.display());
    println!("{} cases: {:?}", chosen.len(), {
        let mut counts = std::collections::BTreeMap::new();
        for case in &chosen {
            *counts.entry(case.category).or_insert(0) += 1;
        }
        counts
    });
}
