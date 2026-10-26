//! The built-in analyzer against **gold**, rather than against another analyzer.
//!
//! ```text
//! pwsh -File scripts/fetch-ud-japanese.ps1
//! cargo test -p sakatsu-core --test ud_gold -- --ignored --nocapture
//! ```
//!
//! # Why this exists
//!
//! Every other cross-analyzer number in this repository measures **agreement**, and
//! `analyzer_corpus.rs` says so in its own header: nobody annotated the corpus, so nothing there
//! can be right or wrong, only different. Two analyzers agreeing at 96% may be wrong the same
//! way, and no amount of running them against each other will say so.
//!
//! UD_Japanese-GSD is hand-annotated, so this file can use the word *accuracy* and mean it.
//!
//! # What gold can settle here, and what it cannot
//!
//! It settles **boundaries**, at two granularities. GSD is a short-unit-word treebank and also
//! carries `LUWBILabel`, the long-unit grouping, on the same tokens. That second layer is what
//! the rejoining question has been stuck on: measured over 1,858 sentences the built-in analyzer
//! splits finer than Ichiran on 13.2% of spans and the dictionary knows the whole word in 69.9%
//! of them — but あの人 and ある日 are also words the dictionary knows, and a learner wants those
//! split. Dictionary membership cannot tell the two cases apart. Annotators did.
//!
//! It settles **lemmas**, which is half of what the conjugation comparison comes down to:
//! `analyzer_corpus::form_of` is `lemma[forms]`, and the lemma is a gold column here.
//!
//! **It does not settle the form chain**, and this file does not pretend otherwise. GSD leaves
//! FEATS empty and puts the auxiliaries in separate tokens — 示し | まし | た — so recovering
//! "polite past" from it means writing a mapping from UniDic auxiliary lexemes onto this app's
//! `Form` vocabulary. That mapping would be mine, and scoring the app against a mapping I wrote
//! is the circularity `pack_precision.rs` exists to avoid. The auxiliary *lemmas* are reported
//! instead, unmapped, as material for whoever writes that mapping deliberately.
//!
//! It cannot settle **readings** at all: GSD's come from UniDic, the suspect in the one reading
//! dispute worth having. See `modern_reading` in `analyzer/lindera/mod.rs`.
//!
//! # The register is wrong for this app, on purpose
//!
//! GSD is news and blogs. This app reads manga and games. A boundary rule validated here is
//! validated as a *mechanism*, not as fit for the domain — the domain corpus is the gitignored
//! one, and it has no gold. Both facts are true and neither replaces the other.

mod common;
#[path = "common/runner.rs"]
mod runner;

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use common::gold::{gold_dir, long_units, read_conllu, GoldSentence};
use runner::{Outcome, Profile, Requirement, Run, Stage};
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::{contains_japanese, is_kana_free};
use sakatsu_core::IchiranAnalyzer;

#[derive(Default)]
struct Tally {
    sentences: usize,
    gold_short: usize,
    gold_long: usize,
    produced: usize,
    matched_short: usize,
    matched_long: usize,
    /// Spans matching gold exactly, and whether the dictionary form matched too.
    lemma_checked: usize,
    lemma_agreed: usize,
    /// Gold spans the analyzer split into several, and the reverse.
    split_finer: usize,
    ran_together: usize,
}

#[test]
#[ignore = "needs scripts/fetch-ud-japanese.ps1"]
fn the_built_in_analyzer_against_hand_annotated_japanese() {
    let dir = gold_dir();
    let mut sentences = Vec::new();
    for split in ["test", "dev"] {
        let path = dir.join(format!("ja_gsd-ud-{split}.conllu"));
        let found = read_conllu(&path);
        if !found.is_empty() {
            println!("{} sentences from {}", found.len(), path.display());
        }
        sentences.extend(found);
    }
    // **Missing data fails rather than skips, because this is a gate.** A run that finds no
    // treebank and returns green reports "the floor held" when nothing was measured at all — the
    // exact shape `project notes` warns about, where a machine with nothing installed shows a wall of
    // green. Anyone who wants the report without the gate can simply not run an ignored test.
    assert!(
        !sentences.is_empty(),
        "no treebank in {} — run scripts/fetch-ud-japanese.ps1, or set SAKATSU_UD_GSD",
        dir.display()
    );
    verify_fixtures(&dir);

    // **The reference has never been scored either.** Every cross-analyzer number in this
    // repository is agreement *with Ichiran*, read as though agreement meant correctness — and
    // the capture corpus shows Ichiran splitting 加藤 into 加 | 藤 and mangling からず〜っと, both
    // of which the built-in analyzer reads correctly. Until the sidecar is measured against the
    // same gold, "surpassing Ichiran" has no operational meaning.
    //
    // It is capped separately because it costs about 0.9 s a sentence against the built-in
    // analyzer's 0.1 ms; `SAKATSU_UD_ICHIRAN` sets how many sentences it sees, default 300.
    // Shared with the application: `Dictionary::discover` decides what counts and in what order,
    // and a dictionary that will not open fails the run rather than shrinking the corpus quietly.
    let dictionaries: Vec<Dictionary> = common::installed_dictionaries();
    // **A prerequisite, not an optional extra.** The lookup-rate section below is one of the
    // numbers this gate reports, and without a dictionary it reports nothing while the run still
    // ends green — the same false pass the missing-treebank case used to give.
    assert!(
        !dictionaries.is_empty(),
        "no dictionaries at {} — run scripts/fetch-dictionaries.ps1",
        common::dictionaries_dir().display()
    );

    // **What this run was, declared before any number is produced.** The identity used to be a
    // block of `println!`s beside the gate, which meant it could drift from what the run actually
    // did and could not be compared with the next run at all. It goes through the runner now, into
    // a written artifact, with one case per sentence.
    let manifest: Vec<String> = sentences
        .iter()
        .map(|sentence| sentence.id.clone())
        .collect();
    let mut run = Run::new(Profile::InstalledResources, manifest)
        .engine("lindera (built-in)")
        .needs(Requirement::required(
            "UD_Japanese-GSD",
            !sentences.is_empty(),
        ))
        .needs(Requirement::required(
            "dictionaries",
            !dictionaries.is_empty(),
        ));
    for (name, _, _) in FIXTURES {
        run = run.fixture(
            *name,
            runner::digest_of(&dir.join(name)).unwrap_or_else(|| "unreadable".into()),
        );
    }
    for dictionary in &dictionaries {
        run = run.dictionary(format!(
            "{} ({} entries; sha256 {})",
            dictionary.meta().title,
            dictionary.meta().entry_count,
            dictionary
                .content_fingerprint()
                .expect("resource content identity is required")
        ));
    }

    report_lookup_rates(&sentences, &dictionaries);

    let built_in = measure(
        "built-in (lindera)",
        &common::built_in().expect("compiled in"),
        &sentences,
        &dictionaries,
    );

    // One case per sentence, named by the treebank's own `sent_id`. A sentence that matched every
    // long unit passed; one that lost any of them is a failure the aggregate floor is entitled to
    // absorb — which is why this run is judged by `accounted_for` and not by `finish`.
    for scored in &built_in.per_sentence {
        let outcome = match &scored.error {
            Some(error) => Outcome::incomplete(format!("the analyzer failed: {error}")),
            None if scored.matched_long == scored.gold_long => Outcome::Passed,
            None => Outcome::failed(
                // The joins run between the analyzer and this comparison, so naming a stage here
                // would be a guess.
                Stage::Unknown,
                format!("{} of {} long units", scored.matched_long, scored.gold_long),
            ),
        };
        run.record(&scored.id, outcome);
        run.tally(
            &scored.id,
            [
                ("gold_long".to_string(), scored.gold_long as i64),
                ("matched_long".to_string(), scored.matched_long as i64),
            ],
        );
    }

    // **Accounting, not the verdict.** `hold_the_line` below is the gate: the floor, its pinned
    // configuration and its verified exceptions. What the floor cannot see is whether every
    // sentence was reached — a corpus that quietly shrank produces a rate that looks like a rate
    // that held.
    run.accounted_for().expect_success();

    hold_the_line(
        &built_in.tally,
        &sentences,
        &common::built_in().expect("compiled in"),
        &dictionaries,
    );

    let ichiran = IchiranAnalyzer::new();
    if ichiran.analyze("テスト").is_err() {
        println!(
            "
SKIP: Ichiran did not answer — it needs WSL and its PostgreSQL database."
        );
        return;
    }
    let limit = std::env::var("SAKATSU_UD_ICHIRAN")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(300)
        .min(sentences.len());
    println!(
        "
  Ichiran over the first {limit} of them — set SAKATSU_UD_ICHIRAN to change it"
    );
    measure("ichiran", &ichiran, &sentences[..limit], &dictionaries);

    // The built-in analyzer again over exactly those sentences, so the two rows compare.
    measure(
        "built-in, same subset",
        &common::built_in().expect("compiled in"),
        &sentences[..limit],
        &dictionaries,
    );
}

/// The corpus these numbers describe, pinned by content.
///
/// **A floor is meaningless without the fixture it was measured on.** Counting sentences or
/// summing gold units only catches a corpus that changed *size*; an edited annotation of the same
/// length would move the score with nothing to say it had. The hash is what makes "14,546 against
/// a baseline of 14,547" a statement about a known input rather than about whatever is on disk.
const FIXTURES: &[(&str, &str, u64)] = &[
    (
        "ja_gsd-ud-dev.conllu",
        "18d266e29336ef619787a928d89d20f2e64188be70d5b37829e3aadc5bb6b841",
        2_961_591,
    ),
    (
        "ja_gsd-ud-test.conllu",
        "5f50ee6ed45c7ebda3787e593eaf5e9a225f25e53f6b4b32df778031862c843f",
        3_111_982,
    ),
];

/// Refuse to score against a corpus this gate has not been calibrated on.
fn verify_fixtures(dir: &std::path::Path) {
    for (name, expected, bytes) in FIXTURES {
        let path = dir.join(name);
        let contents =
            std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        if let Err(complaint) = fixture_matches(&contents, expected, *bytes) {
            panic!("{name}: {complaint}");
        }
    }
    println!("  fixtures verified by content: {} files", FIXTURES.len());
}

/// Whether these bytes are the fixture the floors were derived from.
///
/// Separated from [`verify_fixtures`] so `assert_the_gate_has_teeth` can hand it bytes that are
/// wrong on purpose. A drift check that cannot be shown to detect drift is worse than none: it is
/// the same green tick with none of the cover.
fn fixture_matches(contents: &[u8], expected: &str, bytes: u64) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    if contents.len() as u64 != bytes {
        return Err(format!(
            "{} bytes, not {bytes}; the floors describe a different corpus",
            contents.len()
        ));
    }
    let digest = sakatsu_core::hashing::hex(Sha256::digest(contents));
    if digest != expected {
        return Err(format!(
            "hashes {digest}, not {expected}; re-derive the floors before trusting them"
        ));
    }
    Ok(())
}

/// Whether a scored run clears the baseline once its recorded exceptions are counted.
///
/// A function rather than an inline comparison so the arithmetic can be tested directly. The
/// property that matters is that an allowance is worth **exactly** what it says: one permitted
/// loss covers one, and the second loss fails whatever it was caused by.
fn within_baseline(matched: usize, allowed: usize, baseline: usize) -> bool {
    matched + allowed >= baseline
}

/// The gate can detect what it claims to detect.
///
/// **Not ignored, and it needs nothing installed.** The gate itself needs the treebank, so it can
/// only run where the corpus is; this half of it is arithmetic and hashing, and both belong in an
/// ordinary run. It exists because every other drift check in this repository carries one, and
/// because an allowance that silently widened would be invisible in exactly the way the whole
/// exception mechanism was built to prevent.
#[test]
fn assert_the_gate_has_teeth() {
    // The recorded state passes, and one further loss does not. This is the property the whole
    // exception mechanism rests on: an allowance granted for one span cannot absorb a second.
    assert!(
        within_baseline(14_584, 1, 14_585),
        "the recorded state holds"
    );
    assert!(
        !within_baseline(14_583, 1, 14_585),
        "a second loss must fail rather than borrow the allowance granted for the first"
    );
    assert!(
        !within_baseline(14_000, 1, 14_585),
        "and a large regression must not pass either"
    );
    // With nothing permitted, the baseline is the baseline.
    assert!(within_baseline(14_585, 0, 14_585));
    assert!(!within_baseline(14_584, 0, 14_585));

    // A fixture that is the wrong size, or the right size with different content, is refused.
    use sha2::{Digest, Sha256};
    let body = b"gold, as measured";
    let digest = sakatsu_core::hashing::hex(Sha256::digest(body));
    assert!(fixture_matches(body, &digest, body.len() as u64).is_ok());
    assert!(
        fixture_matches(body, &digest, body.len() as u64 + 1).is_err(),
        "a size change is refused"
    );
    assert!(
        fixture_matches(b"gold, as amended!", &digest, body.len() as u64).is_err(),
        "same length, different bytes: exactly the edit a size check cannot see"
    );

    // **An allowance cannot be inherited by an unrelated change.** This is the property the
    // exception mechanism exists for, and it is checked here rather than only in the scored run,
    // because the scored run needs the treebank and this needs nothing.
    let pinned = Exception {
        id: "example",
        text: "…",
        tokens: &["平気", "なん", "だろ"],
        gold_long: 14,
        matched_long: 10,
        allowance: 1,
        why: "an example, so the arithmetic can be checked without the corpus",
    };
    let observed = ["調理", "側", "は", "何故", "平気", "なん", "だろ", "。"];

    assert_eq!(
        earned(&pinned, &observed, 14, 10),
        Ok(1),
        "the state the exception was granted for still earns it"
    );
    assert!(
        earned(&pinned, &observed, 14, 9).is_err(),
        "a further loss in the same sentence must REFUSE the allowance rather than inherit it — \
         the span still looks right, and that is exactly the case a surface check cannot catch"
    );
    assert!(
        earned(&pinned, &observed, 14, 11).is_err(),
        "and an improvement must refuse it too: the allowance describes a loss that is no longer \
         happening, so it is stale rather than earned"
    );
    assert!(
        earned(&pinned, &observed, 15, 10).is_err(),
        "a changed denominator means a changed sentence, so the pinned loss is about other text"
    );
    assert!(
        earned(
            &pinned,
            &["調理", "側", "は", "平気", "な", "ん", "だろ"],
            14,
            10
        )
        .is_err(),
        "and a segmentation that no longer produces the span cannot claim its allowance"
    );
}

/// The segmentation floor, asserted on counts rather than on a printed rate.
///
/// **A rounded F1 hides a real change.** 14,548 matches and 14,547 both print as 73.9; unrounded
/// they are 73.92652 and 73.92332. A gate reading the log string would have called that no
/// movement, so it reads the tally instead — and the denominators are pinned too, because a
/// number that improves because the corpus shrank is not an improvement.
///
/// **`BASELINE_MATCHED` is the count this work started from and is not lowered.** It already
/// carries one earlier decision — widening the head gate to `接尾辞-形容詞的` cost the `た` of
/// `話やすかった`, where gold labels `やすかっ` `LUWBILabel=I` and `た` `B`. That one is folded into
/// the constant; every later one is named individually and checked against the scored pipeline.
///
/// **The raw figures above are never adjusted.** `report` prints gold, matched and F1 exactly as
/// measured; the allowance is applied here, separately and labelled.
fn hold_the_line(
    tally: &Tally,
    sentences: &[GoldSentence],
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
) {
    // **Raised from 14,547, and the reason is the harness rather than the analyzer.** This gate
    // used to open Jitendex by name. It now opens what the application opens, which on this
    // machine is Jitendex *and* JMnedict — and a news corpus is full of names, so the name pass
    // recovers 38 more long units. Nothing in the analyzer moved; the gate simply stopped
    // measuring a configuration the app never runs.
    //
    // SUW fell from 15,990 to 15,881 in the same run, which is the documented trade rather than a
    // regression: joining a name produces one longer span where gold's short units are several.
    // `project notes` §4a — LUW is the score, SUW is the diagnostic.
    // Raw + allowance, so the permitted exception is exactly **consumed** and not free slack.
    // Setting this to the raw 14,584 would leave the dev-s234 allowance unspent, and a one-unit
    // regression would then pass in silence — the quiet weakening this whole mechanism exists to
    // prevent. The previous pair was 14,546 raw against a 14,547 baseline for the same reason.
    const BASELINE_MATCHED: usize = 14_585;
    const GOLD_LONG: usize = 19_588;
    const GOLD_SHORT: usize = 24_834;

    // **A floor is only meaningful for the resources it was measured over**, and "measured over"
    // means these exact releases in this exact order — not two titles that happen to contain the
    // right words.
    //
    // The first version asserted a title *contained* `jitendex` and `jmnedict`. That permitted a
    // different release of either, permitted extra dictionaries alongside, and ignored order — so
    // configurations that score differently all satisfied it, and the floor spoke for a set it had
    // never seen. The entry counts are pinned for the same reason: a re-import of the same release
    // with different content keeps the title and moves the number.
    //
    // Order is part of the pin because it decides which dictionary answers first, which decides
    // which reading a compound gets.
    const MEASURED_OVER: &[(&str, usize)] = &[
        ("Jitendex.org [2026-07-09]", 433_885),
        ("JMnedict [2026-07-29]", 667_800),
    ];

    assert_eq!(
        (tally.gold_long, tally.gold_short),
        (GOLD_LONG, GOLD_SHORT),
        "the corpus changed under the gate; the baseline below describes a different denominator"
    );

    let opened: Vec<(String, usize)> = dictionaries
        .iter()
        .map(|dictionary| {
            (
                dictionary.meta().title.clone(),
                dictionary.meta().entry_count,
            )
        })
        .collect();
    let pinned: Vec<(String, usize)> = MEASURED_OVER
        .iter()
        .map(|(title, count)| ((*title).to_string(), *count))
        .collect();
    assert_eq!(
        opened, pinned,
        "the baseline below was measured over exactly these dictionaries, in this order, and this \
         run opened something else. That is a different CONFIGURATION, not a different accuracy \
         — the number below cannot speak for it. Install the pinned releases, or re-derive the \
         baseline for the set you have and pin that instead."
    );

    let allowed = permitted_exceptions(sentences, analyzer, dictionaries);
    println!("\n=== segmentation gate ===");
    println!("  raw matched                {}", tally.matched_long);
    println!("  baseline for this work     {BASELINE_MATCHED}");
    println!("  permitted exceptions       {allowed}  (each verified, not a lowered floor)");
    println!(
        "  exception-adjusted         {}  — a policy figure, not a gold accuracy figure",
        tally.matched_long + allowed
    );
    assert!(
        within_baseline(tally.matched_long, allowed, BASELINE_MATCHED),
        "long-unit matches fell to {} against a baseline of {BASELINE_MATCHED} with {allowed} \
         permitted; that is a regression beyond the recorded exceptions. Find the changed spans \
         before deciding whether it is right, and record the decision as a pinned exception \
         rather than by moving this number.",
        tally.matched_long
    );
}

/// One span this project has decided to score differently from the treebank.
struct Exception {
    /// The treebank's own sentence id, for the message. The text identifies it here, because
    /// `GoldSentence` does not carry the id.
    id: &'static str,
    text: &'static str,
    /// A window of analyzer surfaces that must still appear, so the allowance describes a
    /// segmentation this build actually produces.
    tokens: &'static [&'static str],
    /// What scoring **this sentence alone** yields. Pinned, because an allowance justified by a
    /// span has to be checked where the gate reads its numbers.
    gold_long: usize,
    matched_long: usize,
    allowance: usize,
    why: &'static str,
}

/// Losses this project accepts, each pinned to one span and verified in the scored pipeline.
///
/// **An allowance is earned per run, never assumed.** Each is granted only when its sentence is
/// still in the fixture, the analyzer still produces the segmentation it was granted for, **and
/// scoring that sentence on its own still yields exactly the pinned numbers**. The last of those
/// is what stops an allowance drifting onto a different regression: a change elsewhere in the same
/// sentence moves `matched_long`, and the exception then fails rather than silently covering it.
///
/// **These are differences in what a unit is for, not gold being wrong.** UD's long units are a
/// treebank's constituency; this app's are what a learner can select and look up. Where the two
/// disagree the app answers to its reader, and the raw number stays unadjusted so the
/// disagreement stays visible.
fn permitted_exceptions(
    sentences: &[GoldSentence],
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
) -> usize {
    const EXCEPTIONS: &[Exception] = &[Exception {
        id: "dev-s234",
        text: "大きさが全く違う海老とか調理側は何故平気なんだろ。",
        tokens: &["平気", "なん", "だろ"],
        // Scoring this sentence alone, today. Not decoration: the corpus arithmetic says the
        // loss is one match, and this says *where*. Change anything else in this sentence and
        // these move, so the allowance fails rather than quietly absorbing the new loss.
        gold_long: 14,
        matched_long: 10,
        allowance: 1,
        why:
            "learner unit against gold's んだろ long unit — gold labels な B and ん B with だろ I, \
              grouping the nominaliser forward, while the explanatory-なん rule groups it back \
              onto the copula",
    }];

    let mut granted = 0;
    for exception in EXCEPTIONS {
        let Some(sentence) = sentences
            .iter()
            .find(|sentence| sentence.text == exception.text)
        else {
            println!(
                "  NOTE: {} is absent from the fixture; its exception is not granted",
                exception.id
            );
            continue;
        };

        let analysis = analyzer
            .analyze(&sentence.text)
            .unwrap_or_else(|error| panic!("{}: {error}", exception.id));
        let surfaces: Vec<&str> = analysis
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect();

        // **The scored check, and why this takes an analyzer at all.** Matching surfaces says the
        // segmentation is what it was; it does not say the *loss* is. Scoring this sentence
        // through the same code the whole corpus goes through does.
        let one = score(analyzer, std::slice::from_ref(sentence), dictionaries).tally;

        match earned(exception, &surfaces, one.gold_long, one.matched_long) {
            Ok(allowance) => {
                println!("  granted: {} — {}", exception.id, exception.why);
                granted += allowance;
            }
            Err(refusal) => panic!("{refusal}"),
        }
    }
    granted
}

/// Whether one exception is earned by what was actually observed, and what it is worth.
///
/// **Pure, so allowance-inheritance can be proven without the treebank.** The property this has to
/// have is that an allowance granted for one span cannot be spent on a different loss: change
/// anything else in the same sentence and the pinned score moves, so the exception must be refused
/// rather than quietly cover the new difference. That property is arithmetic, and arithmetic
/// belongs in a test that runs everywhere — see `assert_the_gate_has_teeth`.
fn earned(
    exception: &Exception,
    surfaces: &[&str],
    gold_long: usize,
    matched_long: usize,
) -> Result<usize, String> {
    if !surfaces
        .windows(exception.tokens.len())
        .any(|window| window == exception.tokens)
    {
        return Err(format!(
            "{} no longer produces {:?}, so the exception granted for it does not describe this \
             build. Re-inspect the span rather than leaving the allowance in place.",
            exception.id, exception.tokens
        ));
    }
    if (gold_long, matched_long) != (exception.gold_long, exception.matched_long) {
        return Err(format!(
            "{} scores {matched_long} of {gold_long} long units, not the pinned {} of {}. The \
             allowance describes a different loss than the one now occurring; re-derive it rather \
             than keeping it.",
            exception.id, exception.matched_long, exception.gold_long
        ));
    }
    Ok(exception.allowance)
}

/// Would a dictionary lookup close the compounding gap, and what would it cost?
///
/// ```text
/// cargo test --release -p sakatsu-core --test ud_gold -- --ignored --nocapture compounding
/// ```
///
/// # The question, and why the obvious number is not it
///
/// After the counter, の-clause and copula rules, **82% of what the analyzer still cuts apart is
/// nominal compounding** — 世界遺産, 人工知能, バイリンガル地帯, コラーゲン鍋 — at about 1.05
/// occurrences per distinct string. Nothing enumerable reaches a tail like that, and whether
/// 名詞+名詞 is one word is not a grammatical fact anyway: it depends on whether the pair is
/// lexicalised, which is a fact about vocabulary. A dictionary is the obvious instrument.
///
/// **But "the dictionary knows the whole word" has already misled this project once.** It measures
/// 67% over all finer splits and `fetch-ud-japanese.ps1` records why that is unusable: あの人 and
/// ある日 are words the dictionary knows and a learner wants them split. A recall number alone
/// cannot tell a rule that fixes 世界遺産 from one that also welds それは together.
///
/// So this reports **both directions**, and the second is the one that decides:
///
///   recall     of the gold long units the analyzer splits, how many would the rule recover
///   precision  of the joins the rule would make, how many are gold long units at all
///
/// # The nounhood test is a proxy, deliberately
///
/// `Token` carries no analyzer-native part of speech — that is the normalized contract, and it is
/// what a rule would actually have at runtime. So nounhood is approximated by **no hiragana in
/// any part**, which is available downstream and does the job the POS tag would: それは, には,
/// のに and でも are excluded by construction, 世界+遺産 and テレビ+放送 are not. Reported beside
/// the unrestricted version so the proxy's contribution is visible rather than assumed.
#[test]
#[ignore = "needs scripts/fetch-ud-japanese.ps1 and scripts/fetch-dictionaries.ps1"]
fn dictionary_confirmed_compounding_against_gold() {
    let dir = gold_dir();
    let mut sentences = Vec::new();
    for split in ["test", "dev"] {
        sentences.extend(read_conllu(&dir.join(format!("ja_gsd-ud-{split}.conllu"))));
    }
    if sentences.is_empty() {
        println!(
            "SKIP: no treebank in {} — run scripts/fetch-ud-japanese.ps1",
            dir.display()
        );
        return;
    }

    let jitendex = common::dictionaries_dir().join("jitendex");
    let dictionaries: Vec<Dictionary> = Dictionary::open(&jitendex).into_iter().collect();
    if dictionaries.is_empty() {
        println!("SKIP: no dictionaries — run scripts/fetch-dictionaries.ps1");
        return;
    }

    let analyzer = common::built_in().expect("compiled in");
    let known = |surface: &str| -> bool {
        dictionaries.iter().any(|dictionary| {
            dictionary
                .lookup(surface)
                .map(|hits| hits.iter().any(|hit| hit.entry.term == surface))
                .unwrap_or(false)
        })
    };

    /// How many tokens a candidate join may span. Three covers 公共交通機関 as gold groups it;
    /// beyond that the search cost rises and the precision does not.
    const MAX_RUN: usize = 3;

    let mut split_units = 0usize; // gold long units the analyzer did not reproduce
    let mut tileable = 0usize; // ... whose edges its tokens actually reach
    let mut recovered_any = 0usize; // ... that an unrestricted lookup would rebuild
    let mut recovered_nokana = 0usize; // ... that the kana-free proposal would rebuild
    let mut joins_any = 0usize; // joins the unrestricted rule would make
    let mut joins_nokana = 0usize;
    let mut correct_any = 0usize; // ... that are gold long units exactly
    let mut correct_nokana = 0usize;
    // **A join that lands inside a larger gold unit is not the same failure as one that crosses a
    // boundary, and the first cut of this measurement counted them together.** That made 統一教会
    // and 株式会社 read as mistakes, when gold's unit there is simply longer — 統一教会側 — and the
    // join is right as far as it goes. Welding それは together is the failure that matters; joining
    // half of a compound is a shortfall.
    let mut partial_any = 0usize;
    let mut partial_nokana = 0usize;
    let mut wrong: BTreeMap<String, usize> = BTreeMap::new();

    for sentence in &sentences {
        let Ok(analysis) = analyzer.analyze(&sentence.text) else {
            continue;
        };
        let spans: Vec<(usize, usize)> = analysis
            .tokens
            .iter()
            .filter_map(|token| Some((token.start?, token.end?)))
            .collect();
        let gold: BTreeSet<(usize, usize)> = long_units(&sentence.tokens).into_iter().collect();
        let produced: BTreeSet<(usize, usize)> = spans.iter().copied().collect();

        // Every run of two or three adjacent tokens, as a candidate compound.
        for start in 0..spans.len() {
            for length in 2..=MAX_RUN {
                let Some(end) = spans.get(start + length - 1) else {
                    break;
                };
                let span = (spans[start].0, end.1);
                // Contiguous only: a run with a gap is not a word.
                if spans[start..start + length]
                    .windows(2)
                    .any(|pair| pair[0].1 != pair[1].0)
                {
                    break;
                }
                let surface = &sentence.text[span.0..span.1];
                if !known(surface) {
                    continue;
                }
                // Exactly a gold unit, inside one, or across the edge of one — three outcomes,
                // and only the third is a mistake.
                let inside = gold
                    .iter()
                    .any(|(s, e)| *s <= span.0 && span.1 <= *e && (*s, *e) != span);
                joins_any += 1;
                if gold.contains(&span) {
                    correct_any += 1;
                } else if inside {
                    partial_any += 1;
                }
                if is_kana_free(surface) {
                    joins_nokana += 1;
                    if gold.contains(&span) {
                        correct_nokana += 1;
                    } else if inside {
                        partial_nokana += 1;
                    } else {
                        *wrong.entry(surface.to_string()).or_default() += 1;
                    }
                }
            }
        }

        // The gold units the analyzer missed, and whether a lookup would have found them.
        for unit in &gold {
            if produced.contains(unit) {
                continue;
            }
            split_units += 1;
            let surface = &sentence.text[unit.0..unit.1];
            // Only spans the analyzer actually tiles can be rebuilt by joining its tokens.
            let tiles =
                spans.iter().any(|(s, _)| *s == unit.0) && spans.iter().any(|(_, e)| *e == unit.1);
            if !tiles {
                continue;
            }
            // **Recall belongs against what a joining rule could even be offered.** A gold unit
            // whose edges the analyzer's tokens never reach is a boundary conflict, not a missed
            // join, and leaving those in the denominator scores the rule down for a failure it was
            // never given the chance to make.
            tileable += 1;
            if !known(surface) {
                continue;
            }
            recovered_any += 1;
            if is_kana_free(surface) {
                recovered_nokana += 1;
            }
        }
    }

    println!("\n=== dictionary-confirmed compounding, against gold ===");
    println!("  gold long units not reproduced        {split_units}");
    println!(
        "  ... whose edges the tokens reach      {tileable}   <- what a joining rule can address"
    );
    println!(
        "  a lookup could rebuild                {recovered_any}  ({:.1}% of those)",
        percent(recovered_any, tileable)
    );
    println!(
        "  ... restricted to kana-free runs      {recovered_nokana}  ({:.1}% of those)",
        percent(recovered_nokana, tileable)
    );
    println!("\n  joins the rule would make, by outcome");
    println!(
        "  any run the dictionary knows          {joins_any}: {correct_any} exact ({:.1}%), {partial_any} inside a longer unit, {} across a boundary",
        percent(correct_any, joins_any),
        joins_any - correct_any - partial_any,
    );
    println!(
        "  kana-free runs only                   {joins_nokana}: {correct_nokana} exact ({:.1}%), {partial_nokana} inside a longer unit, {} across a boundary",
        percent(correct_nokana, joins_nokana),
        joins_nokana - correct_nokana - partial_nokana,
    );
    println!(
        "  kana-free, counting a partial join as harmless: {:.1}% not wrong",
        percent(correct_nokana + partial_nokana, joins_nokana)
    );

    // **The list that decides it.** A recall figure reads as an opportunity; these are what the
    // rule would break to get it, and the same kind of list is what refuted the 67% figure.
    println!("\n=== joins gold says are wrong, most frequent first ===");
    let mut rows: Vec<_> = wrong.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!(
        "  {} distinct, {} occurrences",
        rows.len(),
        rows.iter().map(|(_, count)| **count).sum::<usize>()
    );
    for (surface, count) in rows.iter().take(40) {
        println!("  {count:>4}x  {surface}");
    }
}

/// **Which granularity can the dictionary actually answer for?**
///
/// "What unit should a learner see" reads as a matter of taste, and it is not: a token here has to
/// be looked up, marked known, and matched by the grammar pack. The first is measurable and stands
/// in well for the others — a span with no entry is an empty vocabulary panel, and nobody marks
/// 新入 learned separately from 新入生.
///
/// So the question is asked numerically. Every span at each granularity is looked up in the
/// installed dictionary and the hit rate says which unit the dictionary is organised around.
/// Spans with no Japanese in them are skipped: punctuation and bare Latin are not vocabulary, and
/// counting them would penalise whichever level happens to isolate them.
///
/// This does not decide by itself — the target is a design decision recorded in `project notes` — but
/// it means the decision is made against a number rather than an intuition about what a word is.
fn report_lookup_rates(sentences: &[GoldSentence], dictionaries: &[Dictionary]) {
    if dictionaries.is_empty() {
        println!("\nSKIP lookup rates: no dictionaries — run scripts/fetch-dictionaries.ps1");
        return;
    }
    let analyzer = common::built_in().expect("compiled in");
    let resolves = |surface: &str| -> bool {
        dictionaries.iter().any(|dictionary| {
            dictionary
                .lookup(surface)
                .map(|hits| hits.iter().any(|hit| hit.entry.term == surface))
                .unwrap_or(false)
        })
    };

    // **Ask the way the app asks.** `dict::readings` looks an inflected token up by its
    // *dictionary form*, not its surface, because 読んでいる is in no dictionary and 読む is in
    // every one. A rate computed on surfaces alone therefore understates every span containing a
    // verb — which is most of what a long unit adds over a short one, and the reason the first
    // version of this measurement made the coarser unit look far worse than it is.
    //
    // In a Japanese verb chain the head comes first, so the span's dictionary form is the first
    // token's lemma: 読ん | で | いる has the lemma 読む on 読ん.
    let resolves_as_word = |surface: &str, lemma: &str| -> bool {
        resolves(surface) || (!lemma.is_empty() && lemma != surface && resolves(lemma))
    };

    println!("\n=== can the dictionary answer for this span? ===");
    println!("  granularity          resolved / checked      rate");
    for (label, kind) in [
        ("short unit (SUW)", 0u8),
        ("long unit  (LUW)", 1),
        ("what ships", 2),
    ] {
        let (mut checked, mut found) = (0usize, 0usize);
        for sentence in sentences {
            let lemma_at = |start: usize| -> String {
                sentence
                    .tokens
                    .iter()
                    .find(|token| token.start == start)
                    .map(|token| token.lemma.clone())
                    .unwrap_or_default()
            };
            let spans: Vec<(usize, usize, String)> = match kind {
                0 => sentence
                    .tokens
                    .iter()
                    .map(|token| (token.start, token.end, token.lemma.clone()))
                    .collect(),
                1 => long_units(&sentence.tokens)
                    .into_iter()
                    .map(|(start, end)| (start, end, lemma_at(start)))
                    .collect(),
                _ => match analyzer.analyze(&sentence.text) {
                    Ok(analysis) => analysis
                        .tokens
                        .iter()
                        .filter_map(|token| {
                            Some((token.start?, token.end?, token.dict_form.clone()))
                        })
                        .collect(),
                    Err(_) => continue,
                },
            };
            for (start, end, lemma) in spans {
                let Some(surface) = sentence.text.get(start..end) else {
                    continue;
                };
                if !contains_japanese(surface) {
                    continue;
                }
                checked += 1;
                if resolves_as_word(surface, &lemma) {
                    found += 1;
                }
            }
        }
        println!(
            "  {label:<20} {found:>7} / {checked:<7}  {:>6.1}%",
            percent(found, checked)
        );
    }
    println!(
        "  A span the dictionary cannot answer for is an empty vocabulary panel. But a *shorter*
  span is likelier to be an entry whatever it means, so the rates above largely restate that:
  生 resolves, and resolving is not the same as being the word in front of you."
    );

    // **The contested subset, which is where the question actually lives.** Wherever a long unit
    // covers several short ones the two standards disagree, and only there does the choice change
    // what a reader is shown. Everywhere else they agree and no rate can separate them.
    //
    // Two things are asked of each contested span: does the *whole* resolve, and does every one of
    // its *parts* resolve. If the whole resolves more often, the merged form is the real word and
    // the parts are fragments that happen to be in the dictionary — the 新入生 against 新入 + 生
    // case, which is the argument for the coarser unit made in numbers rather than asserted.
    let (mut contested, mut whole_resolves, mut parts_resolve, mut both) = (0usize, 0, 0, 0);
    for sentence in sentences {
        let shorts: Vec<(usize, usize)> = sentence
            .tokens
            .iter()
            .map(|token| (token.start, token.end))
            .collect();
        for (start, end) in long_units(&sentence.tokens) {
            let parts: Vec<(usize, usize)> = shorts
                .iter()
                .copied()
                .filter(|&(s, e)| s >= start && e <= end)
                .collect();
            let Some(surface) = sentence.text.get(start..end) else {
                continue;
            };
            if parts.len() < 2 || !contains_japanese(surface) {
                continue;
            }
            contested += 1;
            let lemma_of = |at: usize| -> String {
                sentence
                    .tokens
                    .iter()
                    .find(|token| token.start == at)
                    .map(|token| token.lemma.clone())
                    .unwrap_or_default()
            };
            let whole = resolves_as_word(surface, &lemma_of(start));
            let every_part = parts.iter().all(|&(s, e)| {
                sentence.text.get(s..e).is_some_and(|part| {
                    !contains_japanese(part) || resolves_as_word(part, &lemma_of(s))
                })
            });
            if whole {
                whole_resolves += 1;
            }
            if every_part {
                parts_resolve += 1;
            }
            if whole && every_part {
                both += 1;
            }
        }
    }
    println!("\n  where the two standards disagree — a long unit covering several short ones");
    println!("  contested spans          {contested:>7}");
    println!(
        "  the whole resolves       {whole_resolves:>7}  ({:.1}%)",
        percent(whole_resolves, contested)
    );
    println!(
        "  every part resolves      {parts_resolve:>7}  ({:.1}%)",
        percent(parts_resolve, contested)
    );
    println!(
        "  both resolve             {both:>7}  ({:.1}%)  <- here the choice is which answer is better",
        percent(both, contested)
    );
}

/// Score one analyzer against the treebank.
/// Score and report, returning the tally **and** the per-sentence results.
///
/// The distribution lists are consumed by `report` and dropped here: they are a printed
/// shortlist, not part of what a later run is compared against.
fn measure(
    name: &str,
    analyzer: &dyn Analyzer,
    sentences: &[GoldSentence],
    dictionaries: &[Dictionary],
) -> Scored {
    let Scored {
        tally,
        split_units,
        auxiliaries,
        per_sentence,
    } = score(analyzer, sentences, dictionaries);
    report(name, &tally, &split_units, &auxiliaries);
    Scored {
        tally,
        split_units: BTreeMap::new(),
        auxiliaries: BTreeMap::new(),
        per_sentence,
    }
}

/// Score without reporting, so one sentence can be scored on its own.
///
/// **The split exists for [`permitted_exceptions`].** An allowance has to be checked against the
/// scored pipeline — the numbers the gate actually reads — and not merely against the analyzer's
/// surfaces, which is all the first version of it looked at. Scoring one sentence through the same
/// code as the corpus is the only way to say "this span lost exactly this much" without a second
/// implementation of the scoring rules to disagree with the first.
/// One sentence's contribution, kept so a run can be diffed sentence by sentence.
///
/// **The aggregate cannot say which sentence moved**, and that is the question a regression
/// actually poses. `matched_long` falling by one over 19,588 gold units is invisible in an F1
/// rounded to a tenth; naming the sentence makes it a five-minute investigation.
struct SentenceScore {
    id: String,
    gold_long: usize,
    matched_long: usize,
    /// The analyzer refused this sentence, so nothing was scored for it.
    ///
    /// Recorded rather than skipped: a sentence silently dropped from the denominator is exactly
    /// the shrinking corpus the runner exists to notice.
    error: Option<String>,
}

/// Everything one scoring pass produced.
struct Scored {
    tally: Tally,
    split_units: BTreeMap<String, usize>,
    auxiliaries: BTreeMap<String, usize>,
    per_sentence: Vec<SentenceScore>,
}

fn score(
    analyzer: &dyn Analyzer,
    sentences: &[GoldSentence],
    dictionaries: &[Dictionary],
) -> Scored {
    let mut tally = Tally::default();
    let mut per_sentence = Vec::with_capacity(sentences.len());
    // Which gold long units the analyzer splits, most frequent first. This is the rejoin
    // shortlist: every one of these is a word annotators kept whole.
    let mut split_units: BTreeMap<String, usize> = BTreeMap::new();
    // The auxiliary lemmas gold attaches to a verb, unmapped. See the header.
    let mut auxiliaries: BTreeMap<String, usize> = BTreeMap::new();

    for sentence in sentences {
        let before = (tally.gold_long, tally.matched_long);
        let mut analysis = match analyzer.analyze(&sentence.text) {
            Ok(analysis) => analysis,
            Err(error) => {
                per_sentence.push(SentenceScore {
                    id: sentence.id.clone(),
                    gold_long: 0,
                    matched_long: 0,
                    error: Some(error.to_string()),
                });
                continue;
            }
        };
        analysis.tokens = common::through_pipeline(&analysis.tokens, dictionaries);
        tally.sentences += 1;

        let short: BTreeSet<(usize, usize)> =
            sentence.tokens.iter().map(|t| (t.start, t.end)).collect();
        let long: BTreeSet<(usize, usize)> = long_units(&sentence.tokens).into_iter().collect();
        let lemma_at: BTreeMap<(usize, usize), &str> = sentence
            .tokens
            .iter()
            .map(|t| ((t.start, t.end), t.lemma.as_str()))
            .collect();

        tally.gold_short += short.len();
        tally.gold_long += long.len();

        let produced: Vec<(usize, usize)> = analysis
            .tokens
            .iter()
            .filter_map(|token| Some((token.start?, token.end?)))
            .collect();
        tally.produced += produced.len();

        for token in &analysis.tokens {
            let (Some(start), Some(end)) = (token.start, token.end) else {
                continue;
            };
            if short.contains(&(start, end)) {
                tally.matched_short += 1;
                if let Some(gold) = lemma_at.get(&(start, end)) {
                    tally.lemma_checked += 1;
                    if !token.dict_form.is_empty() && token.dict_form == *gold {
                        tally.lemma_agreed += 1;
                    }
                }
            }
            if long.contains(&(start, end)) {
                tally.matched_long += 1;
            }
        }

        // A gold long unit the analyzer cut into pieces. Named, because the list is the point:
        // these are the rejoin candidates that annotators, not a dictionary lookup, vouch for.
        for &(start, end) in &long {
            let pieces = produced
                .iter()
                .filter(|&&(s, e)| s >= start && e <= end)
                .count();
            if pieces > 1 && !produced.contains(&(start, end)) {
                tally.split_finer += 1;
                if let Some(surface) = sentence.text.get(start..end) {
                    *split_units.entry(surface.to_string()).or_default() += 1;
                }
            }
        }
        for &(start, end) in &produced {
            if !short.contains(&(start, end))
                && sentence
                    .tokens
                    .iter()
                    .filter(|t| t.start >= start && t.end <= end)
                    .count()
                    > 1
            {
                tally.ran_together += 1;
            }
        }

        // Auxiliaries as gold records them, for the mapping this file declines to invent.
        for window in sentence.tokens.windows(2) {
            if window[0].xpos.starts_with("動詞") && window[1].xpos.starts_with("助動詞") {
                *auxiliaries.entry(window[1].lemma.clone()).or_default() += 1;
            }
        }

        // This sentence's share, as the difference the whole-corpus tally just moved by. Taken
        // from the same counters the gate reads, so a per-sentence figure can never disagree with
        // the total it is part of.
        per_sentence.push(SentenceScore {
            id: sentence.id.clone(),
            gold_long: tally.gold_long - before.0,
            matched_long: tally.matched_long - before.1,
            error: None,
        });
    }

    Scored {
        tally,
        split_units,
        auxiliaries,
        per_sentence,
    }
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    100.0 * part as f64 / whole as f64
}

fn report(
    name: &str,
    tally: &Tally,
    split_units: &BTreeMap<String, usize>,
    auxiliaries: &BTreeMap<String, usize>,
) {
    let f1 = |matched: usize, gold: usize| {
        let precision = percent(matched, tally.produced);
        let recall = percent(matched, gold);
        if precision + recall == 0.0 {
            0.0
        } else {
            2.0 * precision * recall / (precision + recall)
        }
    };

    println!(
        "\n=== {name} against gold, over {} sentences ===",
        tally.sentences
    );
    println!("  the analyzer produced {} tokens", tally.produced);

    println!("\nBoundaries — at which granularity is the built-in analyzer right?");
    println!(
        "  short unit (SUW)   gold {:>6}   matched {:>6}   P {:.1}%  R {:.1}%  F1 {:.1}",
        tally.gold_short,
        tally.matched_short,
        percent(tally.matched_short, tally.produced),
        percent(tally.matched_short, tally.gold_short),
        f1(tally.matched_short, tally.gold_short),
    );
    println!(
        "  long unit  (LUW)   gold {:>6}   matched {:>6}   P {:.1}%  R {:.1}%  F1 {:.1}",
        tally.gold_long,
        tally.matched_long,
        percent(tally.matched_long, tally.produced),
        percent(tally.matched_long, tally.gold_long),
        f1(tally.matched_long, tally.gold_long),
    );
    println!(
        "  LUW is the score; SUW is the diagnostic. `project notes` §4a settles that the coarser
  unit is the target, so raising SUW F1 at LUW's expense is a regression now rather than
  a trade. (§4a no longer says *longest*: what makes a unit is §4b's evidence. This gate
  scores the analyzer and the joins — not the reader's tokens, which `lexical` may group
  afterwards; that delta is measured in `lexical_gate_delta.rs`.)"
    );

    println!("\nWhere it differs from the short units");
    println!(
        "  gold long units it cut into pieces  {:>6}  ({:.1}% of long units)",
        tally.split_finer,
        percent(tally.split_finer, tally.gold_long)
    );
    println!(
        "  spans covering several gold tokens  {:>6}",
        tally.ran_together
    );

    println!("\nLemma, where the span matched gold exactly");
    println!(
        "  dictionary form   {} / {}  ({:.1}%)",
        tally.lemma_agreed,
        tally.lemma_checked,
        percent(tally.lemma_agreed, tally.lemma_checked)
    );

    println!("\n=== gold long units the analyzer splits, most frequent first ===");
    println!("  the rejoin shortlist — annotators kept every one of these whole");
    let mut rows: Vec<_> = split_units.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));

    // **How much of the problem the head actually is, because that is what decides the remedy.**
    //
    // A shortlist printed without its share reads as the whole job. If the forty most frequent
    // strings cover most of the mis-splits, this is rule work with a known end. If they cover a
    // tenth, no plausible number of hand-written rules finishes it and the answer is a different
    // kind of tool. The list cannot say which without these two numbers.
    let total: usize = rows.iter().map(|(_, count)| **count).sum();
    // Forty is enough to read over coffee and far too few to characterise a tail of seventeen
    // hundred. `SAKATSU_GOLD_LIST` widens it for the times the question is what the tail is made
    // of rather than what the top of it says.
    let want = std::env::var("SAKATSU_GOLD_LIST")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(40);
    let shown = rows.len().min(want);
    let covered: usize = rows.iter().take(shown).map(|(_, count)| **count).sum();
    println!(
        "  {} distinct, {total} occurrences; the {shown} shown are {:.1}% of them",
        rows.len(),
        percent(covered, total),
    );

    // **By family, because a string count answers the wrong question here.** 4月, 8月 and 2011年
    // are three rows and one rule; classifying them apart makes a mechanical case look like a
    // long tail. The buckets are crude on purpose — a surface is either numeric, or part of the
    // copula-and-の cluster, or neither — and crude is enough to separate "a few rules finish
    // this" from "nothing enumerable does".
    let family = |surface: &str| -> &'static str {
        if surface
            .chars()
            .any(|c| c.is_ascii_digit() || ('０'..='９').contains(&c))
        {
            "number + counter"
        } else if [
            "である",
            "であり",
            "です",
            "のだ",
            "ので",
            "ではな",
            "でなく",
        ]
        .iter()
        .any(|marker| surface.contains(marker))
        {
            "copula and の-clause"
        } else {
            "everything else"
        }
    };
    let mut families: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for (surface, count) in &rows {
        let entry = families.entry(family(surface)).or_default();
        entry.0 += 1;
        entry.1 += **count;
    }
    let mut by_share: Vec<_> = families.iter().collect();
    by_share.sort_by_key(|entry| std::cmp::Reverse((entry.1).1));
    println!("  by family:");
    for (name, (distinct, occurrences)) in by_share {
        println!(
            "    {name:<22} {distinct:>4} distinct  {occurrences:>5} occurrences  ({:.1}%)",
            percent(*occurrences, total)
        );
    }
    println!();

    for (surface, count) in rows.iter().take(shown) {
        println!("  {count:>4}x  {surface}");
    }

    println!("\n=== auxiliaries gold attaches to a verb, unmapped ===");
    println!("  material for a Form mapping, not a mapping. See the header.");
    let mut rows: Vec<_> = auxiliaries.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (lemma, count) in rows.iter().take(20) {
        println!("  {count:>5}x  {lemma}");
    }
}
