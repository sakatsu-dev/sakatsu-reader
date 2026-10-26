//! The runner fails when it should, which is the only reason to believe it when it passes.
//!
//! # Why these are the tests that matter
//!
//! Every condition below has a green-looking shape. A gate with no cases selected prints a rate of
//! nothing and a cheerful summary. A case that panicked leaves no result, and a report that counts
//! only the results it has will call that a clean sweep. A missing dictionary reads as a smaller
//! corpus. Each of these has actually happened in this repository, which is why the runner treats
//! all of them as failures and why each one is pinned here.
//!
//! These need nothing installed: the runner's accounting is about bookkeeping, not about Japanese,
//! so it gates an ordinary `cargo test` run.

#[path = "common/runner.rs"]
mod runner;
#[path = "common/scratch.rs"]
mod scratch;

use runner::{Outcome, Profile, Requirement, Run, Stage, Verdict};

fn ids(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

/// The ordinary case: everything declared, everything answered.
///
/// Hermetic declares no required resource, because that is what hermetic means — and the profile
/// now enforces it rather than accepting the label.
#[test]
fn a_complete_run_passes() {
    let mut run = Run::new(Profile::Hermetic, ids(&["a", "b", "c"])).engine("lindera");
    run.record("a", Outcome::Passed);
    run.record("b", Outcome::Passed);
    run.record("c", Outcome::skipped("no gold label for this span"));
    assert_eq!(run.finish(), Verdict::Passed);
}

/// A profile is checked against what the run declared, not taken from the label.
///
/// **A name is not a property.** Calling a run `InstalledResources` while declaring nothing
/// described no configuration at all, and calling one `Hermetic` while requiring an installed
/// dictionary described the opposite of what it claimed. Both now fail.
#[test]
fn a_profile_must_match_what_the_run_declared() {
    let mut hermetic =
        Run::new(Profile::Hermetic, ids(&["a"])).needs(Requirement::required("jitendex", true));
    hermetic.record("a", Outcome::Passed);
    assert!(
        hermetic.finish().failed(),
        "a hermetic profile that requires an installed dictionary is not hermetic"
    );

    let mut installed = Run::new(Profile::InstalledResources, ids(&["a"]));
    installed.record("a", Outcome::Passed);
    assert!(
        installed.finish().failed(),
        "an installed-resources profile that declares no resource describes nothing"
    );
}

/// **Every case skipped is a failure**, and it reads differently from an empty selection.
///
/// A gate that reached its whole corpus and declined it, one case at a time, has not passed. This
/// was reachable before: `Skipped` shared an arm with `Passed`, so seventeen skips reported
/// success.
#[test]
fn a_run_that_skips_every_case_fails() {
    let mut run = Run::new(Profile::Hermetic, ids(&["a", "b", "c"]));
    for id in ["a", "b", "c"] {
        run.record(id, Outcome::skipped("no gold label"));
    }
    let verdict = run.finish();
    assert!(verdict.failed(), "an all-skipped run must fail");
    let Verdict::Failed { reasons } = verdict else {
        unreachable!()
    };
    assert!(
        reasons.iter().any(|reason| reason.contains("was skipped")),
        "and must say so in its own words, not as an empty selection: {reasons:?}"
    );
    assert!(
        !reasons
            .iter()
            .any(|reason| reason.contains("no cases were selected")),
        "an all-skipped run and an empty selection are different bugs: {reasons:?}"
    );
}

/// **A run where nothing could be aligned has compared nothing**, and it is a third distinct bug.
///
/// Every case attempted, every case answered on both sides, and not one pair that could be put in
/// correspondence. It reaches the same place as an empty selection and an all-skipped run by a
/// different route, and a report that cannot say which route it took cannot be diagnosed.
#[test]
fn a_run_where_every_case_is_unalignable_fails() {
    let mut run = Run::new(Profile::Hermetic, ids(&["a", "b", "c"]));
    for id in ["a", "b", "c"] {
        run.record(id, Outcome::unalignable("one side produced no spans"));
    }
    let verdict = run.finish();
    assert!(verdict.failed(), "a run that compared nothing must fail");
    let Verdict::Failed { reasons } = verdict else {
        unreachable!()
    };
    assert!(
        reasons.iter().any(|reason| reason.contains("unalignable")),
        "and must say so in its own words: {reasons:?}"
    );
    for other in ["was skipped", "no cases were selected"] {
        assert!(
            !reasons.iter().any(|reason| reason.contains(other)),
            "an all-unalignable run is not {other:?}; they are different bugs: {reasons:?}"
        );
    }
}

/// **Some cases unalignable is a finding, not a failure.**
///
/// This is the whole reason the outcome exists. Filing it as `Failed` would inflate the
/// disagreement count with cases where nothing disagreed; filing it as `Skipped` would shrink the
/// denominator silently; filing it as `Incomplete` would blame the machine for a property of the
/// output.
#[test]
fn some_cases_unalignable_is_reported_and_does_not_fail_the_run() {
    let mut run = Run::new(Profile::Hermetic, ids(&["a", "b"]));
    run.record("a", Outcome::Passed);
    run.record("b", Outcome::unalignable("spans do not tile the text"));
    assert!(run.finish().passed());
}

/// Per-case engine and spans reach the artifact, because a comparison has no single engine and a
/// boundary disagreement carried as an `i64` no longer says *which* boundary.
#[test]
fn a_case_carries_its_own_engine_and_spans() {
    let scratch = scratch::Scratch::new("evaluation-runner", "per-case-detail");
    let dir = scratch.join("evaluation");
    let mut run = Run::new(Profile::Hermetic, ids(&["a"])).artifact_dir(&dir);
    run.attribute("a", "lindera-display vs ichiran-display");
    run.spans(
        "a",
        [runner::Span {
            engine: "lindera-display".into(),
            start: 0,
            end: 6,
            surface: "それ".into(),
            reading: "それ".into(),
            looked_up: true,
        }],
    );
    run.record("a", Outcome::Passed);
    assert!(run.finish().passed());

    let written = std::fs::read_dir(&dir)
        .expect("the artifact directory exists")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .expect("a run writes an artifact");
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(written).expect("read")).expect("parse");
    let case = &document["cases"][0];
    assert_eq!(case["engine"], "lindera-display vs ichiran-display");
    assert_eq!(case["spans"][0]["surface"], "それ");
    assert_eq!(case["spans"][0]["engine"], "lindera-display");
    assert_eq!(case["spans"][0]["looked_up"], true);
}

/// Skipping *some* cases is still fine — that is what the outcome is for.
#[test]
fn skipping_some_cases_is_not_a_failure() {
    let mut run = Run::new(Profile::Hermetic, ids(&["a", "b"]));
    run.record("a", Outcome::Passed);
    run.record("b", Outcome::skipped("no gold label"));
    assert!(run.finish().passed());
}

/// **Desktop acceptance can never pass here.** No amount of software makes a person look.
#[test]
fn desktop_acceptance_is_outstanding_and_never_passed() {
    let mut run = Run::new(Profile::DesktopAcceptance, ids(&["import-flow"]))
        .needs(Requirement::observation(
            "the setup panel lists the new dictionary",
        ))
        .needs(Requirement::observation("no error toast on re-import"));
    run.record("import-flow", Outcome::Passed);

    match run.finish() {
        Verdict::Outstanding { unobserved } => {
            assert_eq!(
                unobserved.len(),
                2,
                "every unobserved row is named: {unobserved:?}"
            );
        }
        other => panic!("desktop acceptance must be outstanding, not {other:?}"),
    }
}

/// A desktop-acceptance run with a real failure in it fails, rather than hiding behind outstanding.
#[test]
fn desktop_acceptance_still_fails_on_a_real_failure() {
    let mut run = Run::new(Profile::DesktopAcceptance, ids(&["a", "b"]))
        .needs(Requirement::observation("somebody looks at the reader"));
    run.record("a", Outcome::Passed);
    // "b" is never recorded.
    assert!(
        run.finish().failed(),
        "an unaccounted case fails even a profile that could not have passed anyway"
    );
}

/// The run is written to a file, and the file describes the run.
///
/// **Per-case outcomes that exist only in scrollback cannot be diffed against the next run**,
/// which is the whole reason to record them. This asserts the artifact exists, names every
/// declared case, carries the tallies, and reconciles against the manifest.
#[test]
fn a_run_writes_an_artifact_naming_every_case() {
    let mut run = Run::new(Profile::Hermetic, ids(&["alpha", "beta", "gamma"]))
        .engine("lindera")
        .fixture("example.conllu", "abc123");
    run.record("alpha", Outcome::Passed);
    run.tally(
        "alpha",
        [
            ("gold_long".to_string(), 14),
            ("matched_long".to_string(), 14),
        ],
    );
    run.record("beta", Outcome::failed(Stage::Readings, "うけつけかかり"));
    run.tally(
        "beta",
        [
            ("gold_long".to_string(), 14),
            ("matched_long".to_string(), 10),
        ],
    );
    run.record("gamma", Outcome::skipped("no gold label"));

    let _ = run.finish();
    let path = runner::last_artifact().expect("an artifact was written");

    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read artifact")).expect("valid JSON");

    assert_eq!(document["schema"], runner::SCHEMA);
    assert_eq!(document["verdict"], "failed", "beta failed, so the run did");
    assert_eq!(document["counts"]["selected"], 3);
    assert_eq!(document["counts"]["reported"], 3);

    let cases = document["cases"].as_array().expect("cases");
    assert_eq!(
        cases.len(),
        3,
        "every declared case appears, in manifest order"
    );
    let ids: Vec<&str> = cases
        .iter()
        .map(|case| case["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["alpha", "beta", "gamma"]);
    assert_eq!(cases[1]["outcome"], "failed");
    assert_eq!(cases[1]["stage"], "readings");
    assert_eq!(cases[1]["tallies"]["matched_long"], 10);
    assert_eq!(cases[2]["outcome"], "skipped");

    assert!(
        document["identity"]["executable"]
            .as_str()
            .is_some_and(|digest| !digest.starts_with("unknown")),
        "the artifact identifies the executable that produced it: {:?}",
        document["identity"]["executable"]
    );
    assert_eq!(document["identity"]["fixtures"][0]["sha256"], "abc123");
}

/// A case that was never reached is written as `unaccounted`, not omitted.
///
/// Omitting it would make the artifact agree with a run that never selected it, which is the
/// difference the manifest exists to preserve.
#[test]
fn an_unreached_case_is_written_as_unaccounted() {
    let mut run = Run::new(Profile::Hermetic, ids(&["reached", "never"]));
    run.record("reached", Outcome::Passed);
    let _ = run.finish();

    let path = runner::last_artifact().expect("an artifact was written");
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read artifact")).expect("valid JSON");
    let cases = document["cases"].as_array().expect("cases");
    assert_eq!(
        cases.len(),
        2,
        "the unreached case is still in the artifact"
    );
    assert_eq!(cases[1]["id"], "never");
    assert_eq!(cases[1]["outcome"], "unaccounted");
    assert_eq!(document["counts"]["selected"], 2);
    assert_eq!(document["counts"]["reported"], 1);
}

/// **Zero cases is a failure.** A run that evaluated nothing has not passed; it has not run.
#[test]
fn selecting_no_cases_fails() {
    let run = Run::new(Profile::Hermetic, Vec::new());
    assert!(
        run.finish().failed(),
        "an empty selection must fail. This is the shape of a filter that matched nothing, and \
         two gates in this repository reported green while measuring exactly this much."
    );
}

/// A declared case that produced no result — the shape a crash, a timeout or an early return
/// leaves behind.
#[test]
fn a_case_with_no_result_fails() {
    let mut run = Run::new(Profile::Hermetic, ids(&["a", "b"]));
    run.record("a", Outcome::Passed);
    // "b" is never recorded, as if the loop had panicked or returned early.
    assert!(
        run.finish().failed(),
        "a selected case with no result must fail rather than leaving the denominator"
    );
}

/// A case that panicked is caught by the harness, recorded as incomplete, and fails the run.
///
/// The panic is real rather than described: `catch_unwind` is how a runner survives one case to
/// report on the rest, and the point is that surviving must not mean forgiving.
#[test]
fn a_panicking_case_fails_the_run() {
    let mut run = Run::new(Profile::Hermetic, ids(&["fine", "explodes"]));
    for id in ["fine", "explodes"] {
        let outcome = std::panic::catch_unwind(|| {
            if id == "explodes" {
                panic!("the analyzer fell over");
            }
        });
        run.record(
            id,
            match outcome {
                Ok(()) => Outcome::Passed,
                Err(_) => Outcome::incomplete("the case panicked"),
            },
        );
    }
    let verdict = run.finish();
    assert!(
        verdict.failed(),
        "a case that panicked must fail the run, not be counted as one fewer case"
    );
}

/// A case that ran out of time is incomplete, and incomplete fails.
#[test]
fn a_timed_out_case_fails_the_run() {
    let mut run = Run::new(Profile::Hermetic, ids(&["quick", "slow"]));
    run.record("quick", Outcome::Passed);
    run.record(
        "slow",
        Outcome::incomplete("exceeded the 30s budget for one passage"),
    );
    assert!(verdict_failed(run), "a timeout must fail the run");
}

/// A missing **required** resource is an unsuccessful profile result, not a skip.
#[test]
fn a_missing_required_resource_fails_the_profile() {
    let mut run = Run::new(Profile::InstalledResources, ids(&["a"]))
        .needs(Requirement::required("jitendex", false).because("not installed"));
    run.record("a", Outcome::Passed);
    assert!(
        verdict_failed(run),
        "a required dictionary that is absent must fail the profile. Reporting it as a skip is \
         how a run over a smaller corpus comes to look like a run that passed."
    );
}

/// A missing **optional** comparison leaves the run incomplete and named, and it still passes.
#[test]
fn a_missing_optional_comparison_is_incomplete_not_failed() {
    let mut run = Run::new(Profile::InstalledResources, ids(&["a"]))
        .needs(Requirement::required("jitendex", true))
        .needs(Requirement::optional("ichiran", false).because("WSL sidecar not installed"));
    run.record("a", Outcome::Passed);
    match run.finish() {
        Verdict::Incomplete { reasons } => assert!(
            reasons.iter().any(|reason| reason.contains("ichiran")),
            "the incomplete comparison must be named, not merely counted: {reasons:?}"
        ),
        other => panic!("an absent optional comparison must be incomplete, not {other:?}"),
    }
}

/// A failure carries the stage responsible, and `Unknown` is a legitimate answer.
#[test]
fn attribution_may_be_unknown_but_never_guessed() {
    let mut run = Run::new(Profile::Hermetic, ids(&["attributed", "not"]));
    run.record(
        "attributed",
        Outcome::failed(Stage::Readings, "うけつけかかり, expected うけつけがかり"),
    );
    run.record(
        "not",
        Outcome::failed(Stage::Unknown, "output differs; no single stage isolated"),
    );
    let verdict = run.finish();
    let Verdict::Failed { reasons } = verdict else {
        panic!("failures must fail");
    };
    assert!(
        reasons.iter().any(|reason| reason.contains("readings")),
        "an attributed failure names its stage: {reasons:?}"
    );
    assert!(
        reasons.iter().any(|reason| reason.contains("unknown")),
        "and an unattributed one says so rather than picking a stage: {reasons:?}"
    );
}

/// Recording a case nobody declared is refused: it means the manifest is not the manifest.
#[test]
#[should_panic(expected = "not in the expected manifest")]
fn recording_an_undeclared_case_is_refused() {
    let mut run = Run::new(Profile::Hermetic, ids(&["a"]));
    run.record("b", Outcome::Passed);
}

/// Two answers for one case means the denominator is already wrong.
#[test]
#[should_panic(expected = "recorded twice")]
fn recording_a_case_twice_is_refused() {
    let mut run = Run::new(Profile::Hermetic, ids(&["a"]));
    run.record("a", Outcome::Passed);
    run.record("a", Outcome::Passed);
}

/// The source digest identifies the tree that ran, and changes when the tree does.
///
/// It cannot be a commit hash: this branch carries two hundred uncommitted paths, so a revision
/// names source nobody has. This only checks that a digest is produced over a plausible number of
/// files — proving it changes would mean editing the crate mid-test.
#[test]
fn a_run_records_a_content_digest_of_its_source() {
    let digest = runner::source_digest();
    assert!(
        digest.contains(" files,"),
        "the digest must say how many files it covered: {digest}"
    );
    let count: usize = digest
        .split('(')
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert!(
        count > 20,
        "the digest covered {count} source files, which is too few to be the crates it claims"
    );
}

fn verdict_failed(run: Run) -> bool {
    run.finish().failed()
}

/// **A run that cannot save its report fails**, and says so as a report failure.
///
/// # Why this is not a nicety
///
/// The disk on this machine reached 100% twice during this work. A run whose artifact could not be
/// written used to return `Passed` — so the condition that makes a record most worth having was
/// exactly the condition that discarded it, silently. Persisting per-case outcomes is part of what
/// a run is now that later runs are compared against them.
///
/// The unwritable path is a **file** where the directory should be, which is what `create_dir_all`
/// refuses. Pointing the run somewhere of its own rather than breaking `target/evaluation` keeps
/// every other test in this process unaffected.
#[test]
fn a_run_that_cannot_save_its_report_fails() {
    let scratch = scratch::Scratch::new("evaluation-runner", "artifact");
    let occupied = scratch.join("not-a-directory");
    std::fs::write(&occupied, b"a file sits where the directory would go").expect("occupy");

    let mut run = Run::new(Profile::Hermetic, ids(&["a"])).artifact_dir(&occupied);
    run.record("a", Outcome::Passed);

    let verdict = run.finish();
    assert!(
        verdict.failed(),
        "every case passed, but the report could not be saved, so the evaluation did not succeed"
    );
    let Verdict::Failed { reasons } = verdict else {
        unreachable!()
    };
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("save its report")),
        "the reason must name the report rather than looking like an accuracy failure: {reasons:?}"
    );

    // The accuracy result is preserved: nothing about the case outcomes was discarded, and a
    // writable run over the same cases still passes.
    let mut writable = Run::new(Profile::Hermetic, ids(&["a"])).artifact_dir(scratch.join("ok"));
    writable.record("a", Outcome::Passed);
    assert!(
        writable.finish().passed(),
        "the same cases, somewhere writable, still pass — the failure was the report, not the run"
    );
}
