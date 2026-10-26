//! What grouping does to the measured workloads — on the path the reader is actually served by.
//!
//! # Three token sequences, and why all three are named
//!
//! | name | how | who sees it |
//! |---|---|---|
//! | **gate** | analyzer + `Pipeline::segmentation` | nobody — it is what `ud_gold` and `segmentation_gold` score |
//! | **before** | analyzer + `Pipeline::display`, validated | the reader, before this layer existed |
//! | **after** | `study::analyse_with_grouping` | the reader, now |
//!
//! An earlier version of this file compared *gate* against *gate + grouping* and called the
//! difference the production delta. It is not: `through_pipeline` runs the joins alone, while
//! production runs the whole display pipeline — including reading correction, which changes what
//! the analyzer said a span reads and therefore what grouping is allowed to conclude about it.
//! The historical figure is still measured, because the gates' floors are stated against it; what
//! is *compared* is **before** against **after**, through the same function the commands call.
//!
//! # Nothing is skipped quietly
//!
//! Every input is counted as selected, and then either completed or failed with a reason. A
//! directory that will not open, a capture that will not read, an analysis that errors and an
//! assembly that was discarded all fail the run. A harness that skipped them would report a
//! smaller workload as a clean one.
//!
//! # What may appear in tracked output
//!
//! UD Japanese GSD is a public corpus and this project already quotes it in tracked source, so
//! news divergences are pinned with their surfaces. The capture corpus is gitignored
//! (`project notes` §8): capture measurements report **ids, spans and counts only**, never text.

mod common;

use std::collections::BTreeSet;

use common::gold::{long_units, test_and_dev};
use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::Analysis;

// ---------------------------------------------------------------------------- accounting

/// Every input, and what became of it. **Failures are failures**, not a shorter workload.
#[derive(Default)]
struct Accounting {
    selected: usize,
    completed: usize,
    failed: Vec<String>,
}

impl Accounting {
    fn select(&mut self) {
        self.selected += 1;
    }

    fn complete(&mut self) {
        self.completed += 1;
    }

    fn fail(&mut self, id: &str, why: impl std::fmt::Display) {
        self.failed.push(format!("{id}: {why}"));
    }

    /// Print the account, then require it to be clean.
    fn require(&self, what: &str) {
        println!(
            "  {what}: {} selected, {} completed, {} failed",
            self.selected,
            self.completed,
            self.failed.len()
        );
        assert!(
            self.failed.is_empty(),
            "{what}: {} input(s) failed: {:?}",
            self.failed.len(),
            self.failed
        );
        assert_eq!(
            self.selected, self.completed,
            "{what}: {} selected but {} completed, with nothing recorded as failed — something \
             was skipped silently",
            self.selected, self.completed
        );
        assert!(self.selected > 0, "{what}: no inputs at all");
    }
}

/// A short, stable identity for a piece of text, so a pinned record names a sentence without
/// depending on a position in a file.
fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    sakatsu_core::hashing::hex(Sha256::digest(text.as_bytes()))[..12].to_string()
}

fn spans(analysis: &Analysis) -> BTreeSet<(usize, usize)> {
    analysis
        .tokens
        .iter()
        .filter_map(|token| Some((token.start?, token.end?)))
        .collect()
}

fn resources(dictionaries: &[Dictionary]) {
    println!("  resources:");
    for dictionary in dictionaries {
        println!(
            "    {} — {}",
            dictionary.installed_as(),
            dictionary
                .content_fingerprint()
                .map(|print| print[..16].to_string())
                .unwrap_or_else(|error| format!("unfingerprinted: {error}"))
        );
    }
}

// ---------------------------------------------------------------------------- divergences

/// One grouping decision, with everything needed to recognise *this* decision again.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Divergence {
    /// Digest of the sentence the decision was made in.
    sentence: String,
    start: usize,
    end: usize,
    surface: String,
    /// The interpretation selection chose.
    dict_form: String,
    route: String,
    origin: String,
    /// Gold spans this decision cost, and gained, within its own sentence.
    lost: Vec<(usize, usize)>,
    gained: Vec<(usize, usize)>,
}

impl Divergence {
    fn pinned(&self) -> String {
        format!(
            "        Divergence {{ sentence: \"{}\".into(), start: {}, end: {}, \
             surface: \"{}\".into(), dict_form: \"{}\".into(), route: \"{}\".into(), \
             origin: \"{}\".into(), lost: vec!{:?}, gained: vec!{:?} }},",
            self.sentence,
            self.start,
            self.end,
            self.surface,
            self.dict_form,
            self.route,
            self.origin,
            self.lost,
            self.gained
        )
    }
}

/// Every way two sets of decisions differ, named one at a time.
///
/// **Not a count.** Totals are exactly what a substituted decision preserves, so a comparison that
/// checked them would accept a different set of changes as the reviewed one.
fn disagreement(observed: &[Divergence], reviewed: &[Divergence]) -> Vec<String> {
    let mut differences = Vec::new();
    let (seen, expected): (BTreeSet<_>, BTreeSet<_>) =
        (observed.iter().collect(), reviewed.iter().collect());
    for extra in seen.difference(&expected) {
        differences.push(format!(
            "not reviewed: {} {}..{} {} → {}",
            extra.sentence, extra.start, extra.end, extra.surface, extra.dict_form
        ));
    }
    for missing in expected.difference(&seen) {
        differences.push(format!(
            "reviewed but absent: {} {}..{} {} → {}",
            missing.sentence, missing.start, missing.end, missing.surface, missing.dict_form
        ));
    }
    differences.sort();
    differences
}

/// **The comparison itself**, checked without any data on disk.
///
/// A substituted decision keeps every total identical — same number of changes, same losses, same
/// gains — and is a different set of decisions. This is the control the reviewer asked for, and it
/// is a unit test because the property belongs to the comparison, not to the corpus.
#[test]
fn a_substituted_decision_is_not_the_reviewed_one() {
    let one = Divergence {
        sentence: "aaaaaaaaaaaa".into(),
        start: 0,
        end: 9,
        surface: "気持ちよく".into(),
        dict_form: "気持ちいい".into(),
        route: "lexical".into(),
        origin: "inflection-recovery".into(),
        lost: vec![(0, 3)],
        gained: vec![],
    };
    // Same sentence, same spans, same losses and gains — a different interpretation chosen.
    let substituted = Divergence {
        dict_form: "気持ち良い".into(),
        ..one.clone()
    };
    // …and the same decision moved to a different sentence.
    let elsewhere = Divergence {
        sentence: "bbbbbbbbbbbb".into(),
        ..one.clone()
    };

    assert!(disagreement(std::slice::from_ref(&one), std::slice::from_ref(&one)).is_empty());
    for other in [&substituted, &elsewhere] {
        let differences = disagreement(std::slice::from_ref(other), std::slice::from_ref(&one));
        assert_eq!(
            differences.len(),
            2,
            "a substituted decision must be reported as both an unreviewed change and a missing \
             reviewed one, not absorbed: {differences:?}"
        );
    }

    // And a set with the same *size* but one member swapped is rejected.
    let reviewed = vec![one.clone(), elsewhere.clone()];
    let observed = vec![one, substituted];
    assert!(!disagreement(&observed, &reviewed).is_empty());
}

// ---------------------------------------------------------------------------- the news workload

/// Every decision grouping makes on the news workload — **proposed, not accepted**.
///
/// # What this list is
///
/// The seven groupings measured on the production path, each pinned by the sentence it was made
/// in, the span it claimed, the interpretation selection chose, and the exact gold spans it cost
/// and gained. Pinning the set is what makes a *different* set of decisions fail; totals alone
/// would not, because a substituted decision preserves them.
///
/// # What this list is not
///
/// An accepted divergence. Every unit here is a form a dictionary records — that establishes the
/// reading is **available**, and nothing about whether it is the right reading *in that
/// sentence*. Nothing in this harness looks at context, so "these disagree with UD's convention
/// rather than with Japanese" is a claim it cannot support and is not made.
///
/// # The case for them, stated correctly
///
/// Not that UD's splits are wrong. `水` and `っぽく` are both real, and a segmentation standard
/// that separates them is describing the same text a different way — confusing that with the
/// *pronunciation* being impossible would be confusing two different things. The case is that
/// the joined unit is one the dictionary answers for as a whole, so grouping buys whole-
/// expression assistance, **and the parts stay reachable**: the constituents are kept on the
/// token and the panel offers them, so nothing the reader could look up before is lost.
///
/// That is a trade for a reading tool to weigh, not a correctness result.
///
/// An earlier acceptance was given in-session for an earlier version of this measurement, taken
/// on the wrong pipeline stage and presented with an unsupported claim. It is treated as
/// superseded rather than carried forward: the evidence it rested on has changed.
/// One pinned decision: sentence digest, span, surface, chosen dictionary form, origin, and the
/// gold spans it cost and gained.
type PinnedDecision = (
    &'static str,
    usize,
    usize,
    &'static str,
    &'static str,
    &'static str,
    &'static [(usize, usize)],
    &'static [(usize, usize)],
);

const PROPOSED_NEWS: &[PinnedDecision] = &[
    (
        "10933d5901bc",
        78,
        93,
        "気持ちよく",
        "気持ちいい",
        "inflection-recovery",
        &[(78, 87), (87, 93)],
        &[],
    ),
    (
        "2cdd6fd1f61c",
        21,
        36,
        "間違いなく",
        "間違いない",
        "inflection-recovery",
        &[],
        &[(21, 36)],
    ),
    (
        "a7669dd507b2",
        9,
        24,
        "申し訳ない",
        "申し訳ない",
        "lexical-lookup",
        &[],
        &[(9, 24)],
    ),
    (
        "ad3cc54c4da4",
        21,
        42,
        "気持ちよかった",
        "気持ちいい",
        "inflection-recovery",
        &[(21, 30)],
        &[],
    ),
    (
        "b17ba704678a",
        120,
        132,
        "水っぽく",
        "水っぽい",
        "inflection-recovery",
        &[],
        &[(120, 132)],
    ),
    (
        "e0e391aeee40",
        102,
        114,
        "問題無く",
        "問題ない",
        "inflection-recovery",
        &[(102, 108), (108, 114)],
        &[],
    ),
    (
        "fb94710aa9dd",
        6,
        21,
        "申し分ない",
        "申し分ない",
        "lexical-lookup",
        &[(6, 15), (15, 21)],
        &[],
    ),
];

/// The sentence the gate's one permitted exception is granted for.
///
/// **Grouping must not touch it.** The allowance is verified by scoring that sentence exactly as
/// recorded; a unit formed inside it would change the score the allowance was granted against,
/// and the allowance would then be absorbing a loss nobody reviewed.
const EXCEPTION_SENTENCE: &str = "大きさが全く違う海老とか調理側は何故平気なんだろ。";

/// **The news workload, on the production path.**
///
/// Raw and adjusted are reported separately. The gate's allowance mechanism grants one verified
/// exception (`dev-s234`); the raw figure is what is measured and the adjusted figure is raw plus
/// that allowance, which is the only form comparable with the 14,585 floor.
#[test]
#[ignore = "needs the UD gold data and the installed dictionaries; run under --release"]
fn grouping_measured_on_the_production_path_over_news() {
    let dictionaries: Vec<Dictionary> = common::installed_dictionaries();
    let analyzer = LinderaAnalyzer::new().expect("compiled in");
    let sentences = test_and_dev();
    resources(&dictionaries);

    let mut account = Accounting::default();
    let (mut gold_total, mut gate_matched, mut before_matched, mut after_matched) =
        (0usize, 0usize, 0usize, 0usize);
    let (mut changed, mut units) = (0usize, 0usize);
    let mut observed: Vec<Divergence> = Vec::new();

    for sentence in &sentences {
        account.select();
        let id = digest(&sentence.text);

        // The historical gate's sequence, kept so the continuity figure is still measured here.
        let gate = match analyzer.analyze(&sentence.text) {
            Ok(mut raw) => {
                raw.tokens = common::through_pipeline(&raw.tokens, &dictionaries);
                raw
            }
            Err(error) => {
                account.fail(&id, format!("the analyzer failed: {error}"));
                continue;
            }
        };
        // What the reader received before this layer, and what they receive now — the same two
        // functions the commands call.
        let before = match sakatsu_core::study::analyse_without_grouping(
            &sentence.text,
            &analyzer,
            &dictionaries,
        ) {
            Ok(analysis) => analysis,
            Err(error) => {
                account.fail(&id, format!("the ungrouped path failed: {error}"));
                continue;
            }
        };
        let produced = match sakatsu_core::study::analyse_with_grouping(
            &sentence.text,
            &analyzer,
            &dictionaries,
        ) {
            Ok(produced) => produced,
            Err(error) => {
                account.fail(&id, format!("the production path failed: {error}"));
                continue;
            }
        };
        if !produced.grouping.is_sound() {
            account.fail(&id, produced.grouping.summary());
            continue;
        }

        let (gate_spans, before_spans, after_spans) =
            (spans(&gate), spans(&before), spans(&produced.analysis));
        let wanted: BTreeSet<(usize, usize)> = long_units(&sentence.tokens).into_iter().collect();
        gold_total += wanted.len();
        gate_matched += wanted.intersection(&gate_spans).count();
        before_matched += wanted.intersection(&before_spans).count();
        after_matched += wanted.intersection(&after_spans).count();

        units += produced.grouping.applied.len();
        if !produced.grouping.applied.is_empty() {
            changed += 1;
            let lost: Vec<(usize, usize)> = wanted
                .intersection(&before_spans)
                .filter(|span| !after_spans.contains(span))
                .copied()
                .collect();
            let gained: Vec<(usize, usize)> = wanted
                .intersection(&after_spans)
                .filter(|span| !before_spans.contains(span))
                .copied()
                .collect();
            for applied in &produced.grouping.applied {
                observed.push(Divergence {
                    sentence: id.clone(),
                    start: applied.start,
                    end: applied.end,
                    surface: applied.surface.clone(),
                    dict_form: applied.dict_form.clone(),
                    route: applied.route.name().to_string(),
                    origin: applied.origin.name().to_string(),
                    // Attributed to the sentence, not to the individual unit: with one unit per
                    // sentence here they coincide, and claiming per-unit attribution where two
                    // units shared a sentence would be a guess.
                    lost: lost.clone(),
                    gained: gained.clone(),
                });
            }
        }
        account.complete();
    }

    observed.sort();
    account.require("news sentences");

    // **Raw, then adjusted, and never one presented as the other.**
    const ALLOWANCE: usize = 1;
    const FLOOR_ADJUSTED: usize = 14_585;
    println!("\n  gold long units {gold_total}");
    println!("  gate    (analyzer + joins)      raw {gate_matched}");
    println!("  before  (production, no group)  raw {before_matched}");
    println!("  after   (production path)       raw {after_matched}");
    println!(
        "  after adjusted = raw + allowance {ALLOWANCE} = {}  (floor {FLOOR_ADJUSTED})",
        after_matched + ALLOWANCE
    );
    println!("  sentences changed {changed}, units formed {units}");

    println!("\n  decisions observed:");
    for divergence in &observed {
        println!("{}", divergence.pinned());
    }

    // **The harness reproduces the gate.** `ud_gold` reports this same raw figure over this same
    // workload; if it did not, the comparison below would be against a baseline nothing else
    // recognises. (Its prose still names an older 14,584 — that is narrative, not the live
    // number.)
    const GATE_RAW: usize = 14_586;
    assert_eq!(
        gate_matched, GATE_RAW,
        "the historical gate figure moved; this harness no longer reproduces `ud_gold`"
    );
    assert_eq!(
        before_matched, GATE_RAW,
        "the full display pipeline changed the long-unit count, so `gate` and `before` are no \
         longer the same measurement and both must be reported separately"
    );

    // **The floor, stated in the only form comparable with it.**
    const SHORTFALL: usize = FLOOR_ADJUSTED - (14_582 + ALLOWANCE);
    assert_eq!(
        after_matched + ALLOWANCE + SHORTFALL,
        FLOOR_ADJUSTED,
        "the shortfall against the 14,585 floor changed from {SHORTFALL}; grouping adjusted is \
         {} against a floor of {FLOOR_ADJUSTED}",
        after_matched + ALLOWANCE
    );
    println!(
        "\n  ** the {FLOOR_ADJUSTED} floor is NOT met: {} adjusted, short by {SHORTFALL} **",
        after_matched + ALLOWANCE
    );

    let proposed: Vec<Divergence> = PROPOSED_NEWS
        .iter()
        .map(
            |(sentence, start, end, surface, dict_form, origin, lost, gained)| Divergence {
                sentence: (*sentence).to_string(),
                start: *start,
                end: *end,
                surface: (*surface).to_string(),
                dict_form: (*dict_form).to_string(),
                route: "lexical".to_string(),
                origin: (*origin).to_string(),
                lost: lost.to_vec(),
                gained: gained.to_vec(),
            },
        )
        .collect();
    let differences = disagreement(&observed, &proposed);
    assert!(
        differences.is_empty(),
        "the set of decisions is not the proposed one:\n    {}",
        differences.join("\n    ")
    );

    // **The exception's sentence is untouched**, so the allowance still measures what it was
    // granted for.
    assert!(
        observed
            .iter()
            .all(|divergence| divergence.sentence != digest(EXCEPTION_SENTENCE)),
        "grouping formed a unit inside the sentence the permitted exception is granted for; the \
         allowance would then absorb a loss nobody reviewed"
    );
}

/// **The seven decisions, with everything an acceptance judgement needs.**
///
/// A report, not a gate. What the pinned list above establishes is that the *set* has not
/// changed; what this prints is what each change actually does to the sentence it is in — which
/// is the only basis on which a person can accept or reject one.
///
/// For each: the whole sentence, the baseline segmentation and the grouped one, both readings,
/// what the dictionary answers for the unit, and which parts stay reachable. Gains and losses are
/// printed apart, because "UD agreed" and "UD split" are different questions and averaging them
/// is how a score hides a decision.
///
/// UD Japanese GSD is public and already quoted in tracked source, so sentences appear in full.
#[test]
#[ignore = "needs the UD gold data and the installed dictionaries; run under --release"]
fn the_news_decisions_in_full_for_acceptance() {
    let dictionaries: Vec<Dictionary> = common::installed_dictionaries();
    let analyzer = LinderaAnalyzer::new().expect("compiled in");
    let sentences = test_and_dev();
    resources(&dictionaries);

    let pinned: BTreeSet<&str> = PROPOSED_NEWS.iter().map(|row| row.0).collect();
    let (mut gains, mut losses) = (Vec::new(), Vec::new());

    for sentence in &sentences {
        let id = digest(&sentence.text);
        if !pinned.contains(id.as_str()) {
            continue;
        }
        let Ok(before) =
            sakatsu_core::study::analyse_without_grouping(&sentence.text, &analyzer, &dictionaries)
        else {
            continue;
        };
        let Ok(after) =
            sakatsu_core::study::analyse_with_grouping(&sentence.text, &analyzer, &dictionaries)
        else {
            continue;
        };

        let wanted: BTreeSet<(usize, usize)> = long_units(&sentence.tokens).into_iter().collect();
        for applied in &after.grouping.applied {
            let unit = after
                .analysis
                .tokens
                .iter()
                .find(|token| token.start == Some(applied.start))
                .expect("the unit is in the assembled tokens");
            let ud_agrees = wanted.contains(&(applied.start, applied.end));

            let mut lines = Vec::new();
            lines.push(format!("  {id}  {}", sentence.text));
            lines.push(format!(
                "    baseline  {:?}",
                before
                    .tokens
                    .iter()
                    .map(|token| token.surface.as_str())
                    .collect::<Vec<_>>()
            ));
            lines.push(format!(
                "    grouped   {:?}",
                after
                    .analysis
                    .tokens
                    .iter()
                    .map(|token| token.surface.as_str())
                    .collect::<Vec<_>>()
            ));
            lines.push(format!(
                "    unit      {} → {}   reading {} → {}   via {} ({})",
                applied.surface,
                applied.dict_form,
                unit.reading,
                unit.lemma_reading,
                applied.route.name(),
                applied.origin.name()
            ));
            // What UD annotates over exactly this stretch, which is the disagreement stated.
            let covered: Vec<&str> = wanted
                .iter()
                .filter(|(start, end)| *start >= applied.start && *end <= applied.end)
                .map(|(start, end)| &sentence.text[*start..*end])
                .collect();
            lines.push(format!(
                "    UD        {}  {covered:?}",
                if ud_agrees { "agrees" } else { "splits" }
            ));
            // The assistance the reader gets for the unit, and what stays reachable under it.
            // The assistance the reader actually receives for the unit, asked the way the
            // reader's own lookup asks it — for the token, not for a string.
            lines.push(match sakatsu_core::lookup::word(unit, &dictionaries) {
                Ok(found) => format!(
                    "    dictionary {} sense(s) for {} ({}) from {:?}{}",
                    found.inline.len(),
                    found.headword,
                    found.reading.as_deref().unwrap_or("-"),
                    found.inline_source.as_deref().unwrap_or("nothing"),
                    // Whole-word or component: the distinction the reader sees as a definition of
                    // the word against a definition of a part of it.
                    match found.inline_retrieved {
                        sakatsu_core::dict::merge::Retrieved::Whole { .. } =>
                            "  [answers for the whole unit]",
                        sakatsu_core::dict::merge::Retrieved::Component { .. } =>
                            "  [answers only for a component]",
                        sakatsu_core::dict::merge::Retrieved::Unknown =>
                            "  [provenance unrecorded]",
                    }
                ),
                Err(error) => format!("    dictionary FAILED: {error}"),
            });
            lines.push(format!(
                "    parts     {:?}",
                unit.components
                    .iter()
                    .map(|part| part.surface.as_str())
                    .collect::<Vec<_>>()
            ));

            if ud_agrees {
                gains.push(lines.join("\n"));
            } else {
                losses.push(lines.join("\n"));
            }
        }
    }

    println!("\n=== UD agrees: {} decision(s) ===", gains.len());
    for entry in &gains {
        println!("{entry}\n");
    }
    println!("=== UD splits: {} decision(s) ===", losses.len());
    for entry in &losses {
        println!("{entry}\n");
    }
    assert_eq!(
        gains.len() + losses.len(),
        PROPOSED_NEWS.len(),
        "every pinned decision must be reported for review"
    );
}

// ---------------------------------------------------------------------------- hand-segmented

/// **The 17 hand-segmented fixtures**, which is a different and much smaller thing than the
/// development capture collection. Named precisely because the earlier report did not.
#[test]
#[ignore = "needs the corpus and the installed dictionaries"]
fn grouping_measured_on_the_production_path_over_hand_segmented_fixtures() {
    let dictionaries: Vec<Dictionary> = common::installed_dictionaries();
    let analyzer = LinderaAnalyzer::new().expect("compiled in");
    let (loaded, faults) = hand_segmented();
    assert!(
        faults.is_empty(),
        "the fixtures did not load cleanly: {faults:?}"
    );

    let mut account = Accounting::default();
    let (mut gold_total, mut gate_matched, mut before_matched, mut after_matched) =
        (0usize, 0usize, 0usize, 0usize);
    let (mut changed, mut units) = (0usize, 0usize);

    for (id, tier, partition, text, gold) in &loaded {
        account.select();
        let gate = match analyzer.analyze(text) {
            Ok(mut raw) => {
                raw.tokens = common::through_pipeline(&raw.tokens, &dictionaries);
                raw
            }
            Err(error) => {
                account.fail(id, format!("the analyzer failed: {error}"));
                continue;
            }
        };
        let before =
            match sakatsu_core::study::analyse_without_grouping(text, &analyzer, &dictionaries) {
                Ok(analysis) => analysis,
                Err(error) => {
                    account.fail(id, format!("the ungrouped path failed: {error}"));
                    continue;
                }
            };
        let produced =
            match sakatsu_core::study::analyse_with_grouping(text, &analyzer, &dictionaries) {
                Ok(produced) => produced,
                Err(error) => {
                    account.fail(id, format!("the production path failed: {error}"));
                    continue;
                }
            };
        if !produced.grouping.is_sound() {
            account.fail(id, produced.grouping.summary());
            continue;
        }

        let wanted: BTreeSet<(usize, usize)> = gold.iter().copied().collect();
        gold_total += wanted.len();
        gate_matched += wanted.intersection(&spans(&gate)).count();
        before_matched += wanted.intersection(&spans(&before)).count();
        after_matched += wanted.intersection(&spans(&produced.analysis)).count();
        units += produced.grouping.applied.len();
        if !produced.grouping.applied.is_empty() {
            changed += 1;
            // Ids and spans only: this text is private.
            for applied in &produced.grouping.applied {
                println!(
                    "    {id} [{tier}/{partition}] unit {}..{} via {}",
                    applied.start,
                    applied.end,
                    applied.route.name()
                );
            }
        }
        account.complete();
    }
    account.require("hand-segmented fixtures");

    println!("  gold spans {gold_total}");
    println!("  gate   raw {gate_matched}");
    println!("  before raw {before_matched}");
    println!("  after  raw {after_matched}");
    println!("  fixtures changed {changed}, units formed {units}");

    const CAPTURE_FLOOR: usize = 118;
    assert!(
        gate_matched >= CAPTURE_FLOOR,
        "the historical gate figure moved: {gate_matched}"
    );
    assert!(
        after_matched >= before_matched,
        "grouping cost {} boundary matches on the production path",
        before_matched - after_matched
    );
}

/// The 17 `.tok.mine` fixtures, with the id and holdout label the other harnesses use.
#[allow(clippy::type_complexity)]
fn hand_segmented() -> (
    Vec<(
        String,
        &'static str,
        &'static str,
        String,
        Vec<(usize, usize)>,
    )>,
    Vec<String>,
) {
    let mut faults = Vec::new();
    let root = common::corpus_dir();
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) => {
            faults.push(format!("no corpus at {}: {error}", root.display()));
            return (Vec::new(), faults);
        }
    };
    let mut loaded = Vec::new();
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(error) => {
                faults.push(format!("a directory entry could not be read: {error}"));
                continue;
            }
        };
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if !name.ends_with(".tok.mine") {
            continue;
        }
        let body = match std::fs::read_to_string(&path) {
            Ok(body) => body,
            Err(error) => {
                faults.push(format!("{name} could not be read: {error}"));
                continue;
            }
        };
        let Some((stem, tier)) = common::captures::split_tier(&name) else {
            faults.push(format!("{name} has no recognised tier suffix"));
            continue;
        };
        let partition = match common::captures::reading_holdout_stems() {
            Ok(holdout) if holdout.contains(stem) => "holdout",
            Ok(_) => "development",
            Err(reason) => {
                faults.push(format!("the reading holdout is unreadable: {reason}"));
                continue;
            }
        };
        let (mut text, mut spans) = (String::new(), Vec::new());
        for token in body.trim().split('|') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let start = text.len();
            text.push_str(token);
            spans.push((start, text.len()));
        }
        if spans.is_empty() {
            faults.push(format!("{name} has no tokens"));
            continue;
        }
        loaded.push((format!("capture/{stem}"), tier, partition, text, spans));
    }
    if loaded.is_empty() {
        faults.push("no .tok.mine fixtures".to_string());
    }
    loaded.sort_by(|left, right| left.0.cmp(&right.0));
    (loaded, faults)
}

// ---------------------------------------------------------------------------- the real captures

/// **Does grouping ever fire on the reader's own text?**
///
/// The hand-segmented fixtures are 17 files. The capture collection is the whole transcript set,
/// which is the only sample that can answer whether this layer does anything for the reader. Ids,
/// tiers and counts only — the text is private.
#[test]
#[ignore = "needs the corpus and the installed dictionaries"]
fn grouping_over_the_capture_collection() {
    let dictionaries: Vec<Dictionary> = common::installed_dictionaries();
    let analyzer = LinderaAnalyzer::new().expect("compiled in");
    let (captures, uncounted, faults) = common::captures::captures();
    assert!(
        faults.is_empty(),
        "the capture inventory faulted: {faults:?}"
    );

    let mut account = Accounting::default();
    let mut by_tier: std::collections::BTreeMap<(&str, &str), (usize, usize, usize)> =
        std::collections::BTreeMap::new();
    let mut changed_ids: Vec<String> = Vec::new();

    for capture in &captures {
        account.select();
        let produced = match sakatsu_core::study::analyse_with_grouping(
            &capture.text,
            &analyzer,
            &dictionaries,
        ) {
            Ok(produced) => produced,
            Err(error) => {
                account.fail(&capture.id, format!("the production path failed: {error}"));
                continue;
            }
        };
        if !produced.grouping.is_sound() {
            account.fail(&capture.id, produced.grouping.summary());
            continue;
        }
        let row = by_tier
            .entry((capture.tier, capture.partition))
            .or_insert((0, 0, 0));
        row.0 += 1;
        row.1 += produced.grouping.applied.len();
        row.2 += produced.grouping.abstentions.len();
        if !produced.grouping.applied.is_empty() {
            changed_ids.push(format!(
                "{} [{}/{}] ×{}",
                capture.id,
                capture.tier,
                capture.partition,
                produced.grouping.applied.len()
            ));
        }
        account.complete();
    }
    account.require("capture transcripts");

    println!("  text-only stems not counted: {}", uncounted.len());
    println!("  tier/partition  transcripts  units  abstentions");
    for ((tier, partition), (transcripts, units, abstentions)) in &by_tier {
        println!("    {tier}/{partition}  {transcripts}  {units}  {abstentions}");
    }
    println!("  transcripts grouping changed: {}", changed_ids.len());
    for id in &changed_ids {
        println!("    {id}");
    }
}

// ---------------------------------------------------------------------------- cost

/// **How much dictionary work generation does**, on the production path.
#[test]
#[ignore = "needs the corpus and the installed dictionaries"]
fn generation_lookup_counts_on_the_production_path() {
    let dictionaries: Vec<Dictionary> = common::installed_dictionaries();
    let analyzer = LinderaAnalyzer::new().expect("compiled in");
    let (loaded, faults) = hand_segmented();
    assert!(faults.is_empty(), "{faults:?}");

    let mut account = Accounting::default();
    let (mut tokens, mut candidates, mut queries, mut failures) = (0usize, 0usize, 0usize, 0usize);
    let mut worst = (0usize, String::new());
    for (id, _, _, text, _) in &loaded {
        account.select();
        let analysis =
            match sakatsu_core::study::analyse_without_grouping(text, &analyzer, &dictionaries) {
                Ok(analysis) => analysis,
                Err(error) => {
                    account.fail(id, error);
                    continue;
                }
            };
        let found = match sakatsu_core::lexical::candidates(&analysis, &dictionaries) {
            Ok(found) => found,
            Err(spans) => {
                account.fail(id, format!("the baseline is invalid: {spans:?}"));
                continue;
            }
        };
        tokens += analysis.tokens.len();
        candidates += found.candidates.len();
        queries += found.queries;
        failures += found.failures.len();
        if found.queries > worst.0 {
            worst = (found.queries, id.clone());
        }
        account.complete();
    }
    account.require("hand-segmented fixtures");

    println!("  tokens {tokens}, candidates {candidates}");
    println!(
        "  dictionary queries {queries} over {} dictionary/ies — {:.1} per token",
        dictionaries.len(),
        queries as f64 / tokens.max(1) as f64
    );
    println!("  worst single fixture: {} queries ({})", worst.0, worst.1);
    assert_eq!(failures, 0, "a dictionary failed during generation");
}

/// **Per-passage timings and an end-to-end total**, on the production path, in release.
#[test]
#[ignore = "needs the corpus and the installed dictionaries; run under --release"]
fn grouping_latency_per_passage_and_end_to_end() {
    let dictionaries: Vec<Dictionary> = common::installed_dictionaries();
    let analyzer = LinderaAnalyzer::new().expect("compiled in");
    let (loaded, faults) = hand_segmented();
    assert!(faults.is_empty(), "{faults:?}");
    resources(&dictionaries);

    const REPEATS: usize = 3;
    let mut per_passage: Vec<(String, usize, Vec<f64>, Vec<f64>)> = Vec::new();
    for (id, _, _, text, _) in &loaded {
        let mut without = Vec::new();
        let mut with = Vec::new();
        let mut tokens = 0usize;
        for _ in 0..REPEATS {
            let at = std::time::Instant::now();
            let plain =
                sakatsu_core::study::analyse_without_grouping(text, &analyzer, &dictionaries)
                    .expect("the ungrouped path");
            without.push(at.elapsed().as_secs_f64() * 1000.0);
            tokens = plain.tokens.len();

            let at = std::time::Instant::now();
            sakatsu_core::study::analyse_with_grouping(text, &analyzer, &dictionaries)
                .expect("the production path");
            with.push(at.elapsed().as_secs_f64() * 1000.0);
        }
        per_passage.push((id.clone(), tokens, without, with));
    }

    let median = |values: &[f64]| {
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
        sorted[sorted.len() / 2]
    };
    println!("\n  per passage, {REPEATS} interleaved repetitions, median ms:");
    println!("    id                          tokens   without    with     added");
    let (mut total_without, mut total_with) = (0.0f64, 0.0f64);
    for (id, tokens, without, with) in &per_passage {
        let (a, b) = (median(without), median(with));
        total_without += a;
        total_with += b;
        println!(
            "    {id:<26} {tokens:>6}  {a:>8.2}  {b:>7.2}  {:>8.2}",
            b - a
        );
    }
    println!(
        "\n  end to end over {} passages: without {:.1}ms, with {:.1}ms, added {:.1}ms",
        per_passage.len(),
        total_without,
        total_with,
        total_with - total_without
    );
    println!(
        "  added per passage, mean {:.2}ms",
        (total_with - total_without) / per_passage.len().max(1) as f64
    );
}
