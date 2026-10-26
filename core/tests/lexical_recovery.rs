//! Does the candidate layer *generate* the interpretation a reader wanted?
//!
//! # The question this answers, and the one it does not
//!
//! **Recovery only.** Whether a supported interpretation is proposed at all. Nothing here selects
//! anything, so a proposal appearing is not a claim that it should replace the baseline — that is a
//! separate decision with its own requirements.
//!
//! **A count is not a result, and neither is a family.** An earlier version of this report read the
//! proposals, saw particles among them, and called them harmful. Candidate counts establish neither
//! contextual correctness nor harm, and grouping retains component selection in any case. So this
//! reports proposals **by structural family and evidence category**, and classifies none of them as
//! correct or wrong.
//!
//! # Where it runs
//!
//! Against an isolated dictionary copy whose **capabilities are verified**, not an environment
//! variable named "upgraded": the sequence bucket must be present, or cross-reference resolution is
//! inert and a family like `かっこ良い` (filed at −1577490) answers with a pointer. The reader's own
//! store is never opened.
//!
//! Enumeration is `common::captures`, shared with the production coverage gate, so a corpus that
//! half-failed to load fails the run instead of being measured over whatever survived.

mod common;

#[path = "common/runner.rs"]
mod runner;

use std::collections::BTreeMap;
use std::path::PathBuf;

use runner::{Outcome, Profile, Requirement, Run};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::lexical::{self, Candidate};
use serde_json::json;

/// The isolated copy, with the capability this measurement depends on actually checked.
fn verified_dictionaries() -> Vec<Dictionary> {
    let dir = PathBuf::from(
        std::env::var_os("SAKATSU_LEXICAL_DICT")
            .expect("SAKATSU_LEXICAL_DICT must point at an isolated dictionary copy"),
    );
    let real = PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
        .join("com.sakatsu.app")
        .join("dictionaries");
    if let (Ok(trial), Ok(real)) = (dir.canonicalize(), real.canonicalize()) {
        assert!(
            !trial.starts_with(&real),
            "{} is inside the reader's own store; this measurement needs a copy",
            trial.display()
        );
    }
    let found = Dictionary::discover(&dir);
    assert!(!found.is_empty(), "no dictionary under {}", dir.display());
    let opened: Vec<Dictionary> = found
        .iter()
        .map(|path| Dictionary::open(path).expect("the copy opens"))
        .collect();
    assert!(
        opened.iter().any(Dictionary::has_sequence),
        "no dictionary in {} has a sequence bucket, so cross-reference resolution is inert and \
         this measurement would confound the layer with that absence",
        dir.display()
    );
    opened
}

/// The production analysis boundary: one shared path, not a reconstructed analyzer-plus-pipeline.
fn studied(text: &str, dictionaries: &[Dictionary]) -> Option<sakatsu_core::Analysis> {
    let analyzer = sakatsu_core::analyzer::LinderaAnalyzer::new().ok()?;
    sakatsu_core::study::analyse(text, &analyzer, dictionaries).ok()
}

/// Which scripts a proposal is written in.
///
/// **A description, not a structure.** Kana and kanji say nothing about noun+adjective versus
/// particle or auxiliary — that needs morphology §2 has not supplied yet, and calling this a
/// "structural family" claimed a categorisation this code cannot make.
fn script_category(candidate: &Candidate) -> &'static str {
    let kana_only = |text: &str| text.chars().all(|c| ('\u{3040}'..='\u{30ff}').contains(&c));
    if kana_only(&candidate.surface) {
        "all-kana"
    } else if candidate
        .surface
        .chars()
        .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    {
        "contains-kanji"
    } else {
        "other-script"
    }
}

#[test]
#[ignore = "needs the capture corpus and a verified isolated dictionary copy"]
fn candidate_recovery_across_development_material() {
    let dictionaries = verified_dictionaries();
    let (selected, text_only, faults) = common::captures::captures();

    let ids: Vec<String> = selected.iter().map(|capture| capture.id.clone()).collect();
    let mut run = Run::new(Profile::InstalledResources, ids.iter().cloned())
        .engine("lindera")
        .needs(
            Requirement::required("the capture corpus", !selected.is_empty())
                .because(format!("{} transcripts", selected.len())),
        )
        .needs(
            Requirement::required("the corpus enumerated without fault", faults.is_empty())
                .because(if faults.is_empty() {
                    "every discovered input was read".to_string()
                } else {
                    faults.join("; ")
                }),
        )
        .needs(
            Requirement::required(
                "a dictionary with a sequence bucket",
                dictionaries.iter().any(Dictionary::has_sequence),
            )
            .because("cross-reference resolution is otherwise inert"),
        );
    for dictionary in &dictionaries {
        run = run.dictionary(format!(
            "{} installed as {} generation {} (sequence bucket {})",
            dictionary.meta().title,
            dictionary.installed_as(),
            dictionary
                .root()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            dictionary.has_sequence()
        ));
    }

    // Reported by partition, never pooled: holdout material has already been read once, and a
    // total that mixed it with development would misstate what this mechanism was designed on.
    let mut by_partition: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    // Transcripts and passages are different counts — 28 draft captures contribute 54 passages —
    // and a column headed one while holding the other is how "draft 54" read as a transcript
    // count. Each tier also carries its own limit accounting, so the promoted row is the
    // like-for-like comparison with the cohort the earlier 539/250 was measured over.
    #[derive(Default)]
    struct Tally {
        transcripts: usize,
        passages: usize,
        proposals: usize,
        supported: usize,
        unsearched: usize,
        refused: usize,
    }
    let mut by_tier: BTreeMap<&str, Tally> = BTreeMap::new();
    let mut by_script: BTreeMap<(&str, usize), usize> = BTreeMap::new();
    let mut by_evidence: BTreeMap<String, usize> = BTreeMap::new();
    let mut unsearched = 0usize;
    let mut refused = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();
    let mut examples: Vec<String> = Vec::new();

    for capture in &selected {
        let mut passages = 0usize;
        let mut proposals = 0usize;
        let mut supported = 0usize;

        for line in capture.text.lines().filter(|line| !line.trim().is_empty()) {
            let Some(analysis) = studied(line, &dictionaries) else {
                invalid.push(format!(
                    "{}: the production boundary refused a passage",
                    capture.id
                ));
                continue;
            };
            passages += 1;
            let found = match lexical::candidates(&analysis, &dictionaries) {
                Ok(found) => found,
                Err(faults) => {
                    invalid.push(format!("{}: {faults:?}", capture.id));
                    continue;
                }
            };
            unsearched += found.unsearched.len();
            refused += found.refused.len();
            let tier = by_tier.entry(capture.tier).or_default();
            tier.unsearched += found.unsearched.len();
            tier.refused += found.refused.len();
            failures.extend(found.failures.iter().map(|failure| {
                format!(
                    "{} {:?}: {}",
                    failure.dictionary, failure.query, failure.error
                )
            }));

            for candidate in found.groupings() {
                proposals += 1;
                *by_script
                    .entry((script_category(candidate), candidate.joins()))
                    .or_default() += 1;
                for kind in candidate.evidence_kinds() {
                    *by_evidence.entry(kind.name().to_string()).or_default() += 1;
                }
                if candidate.supported_as_lexical_unit() {
                    supported += 1;
                    if examples.len() < 20 && capture.partition == "development" {
                        examples.push(format!(
                            "{}  {:?} joins {} script={} evidence={:?}",
                            capture.id,
                            candidate.surface,
                            candidate.joins(),
                            script_category(candidate),
                            candidate
                                .evidence_kinds()
                                .iter()
                                .map(|kind| kind.name())
                                .collect::<Vec<_>>()
                        ));
                    }
                }
            }
        }

        let partition_entry = by_partition.entry(capture.partition).or_default();
        partition_entry.0 += passages;
        partition_entry.1 += proposals;
        partition_entry.2 += supported;

        let tier_entry = by_tier.entry(capture.tier).or_default();
        tier_entry.transcripts += 1;
        tier_entry.passages += passages;
        tier_entry.proposals += proposals;
        tier_entry.supported += supported;
        run.record(capture.id.clone(), Outcome::Passed);
    }

    println!("\n=== candidate recovery, by partition ===");
    println!("    partition     passages  proposals  supported-as-lexical");
    for (partition, (passages, proposals, supported)) in &by_partition {
        println!("    {partition:<12} {passages:>9} {proposals:>10} {supported:>21}");
    }
    println!(
        "
=== the same, by transcript tier ==="
    );
    println!("    tier         transcripts  passages  proposals  supported  unsearched  refused");
    for (tier, tally) in &by_tier {
        println!(
            "    {tier:<12} {:>11} {:>9} {:>10} {:>10} {:>11} {:>8}",
            tally.transcripts,
            tally.passages,
            tally.proposals,
            tally.supported,
            tally.unsearched,
            tally.refused
        );
    }
    println!("\n  transcripts selected {}", selected.len());
    if !text_only.is_empty() {
        println!("  text-only stems excluded {:?}", text_only);
    }
    println!("  searches with an admissible extension unexamined  {unsearched}");
    println!("  spans the source rules out                        {refused}");
    println!(
        "
  The earlier 539/250 ran over the 141 promoted transcripts only, so the"
    );
    println!("  promoted row above carries that cohort's own unsearched and refused counts.");
    println!("  The totals here include draft material as well and are not comparable with it.");

    println!("\n  proposals by script category and tokens joined (not a structural claim)");
    for ((script, joins), count) in &by_script {
        println!("    {script:<16} {joins} tokens  {count}");
    }
    println!("\n  evidence categories present");
    for (kind, count) in &by_evidence {
        println!("    {kind:<22} {count}");
    }
    println!(
        "\n  **Families and counts only.** Nothing here is classified as correct or harmful:\n  \
         availability is not applicability, and selection is a separate decision."
    );
    println!("\n  examples, development partition only");
    for line in &examples {
        println!("    {line}");
    }

    let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");
    let _ = std::fs::create_dir_all(&artifact);
    // **Identity over the inputs, not just the source tree.** The filename used to depend on the
    // source digest alone and publication used a plain write, so a run over different dictionaries
    // or edited capture text overwrote the evidence it should have been compared with — without a
    // line of source changing.
    let dictionary_identities: Vec<serde_json::Value> = dictionaries
        .iter()
        .map(|dictionary| {
            // A fingerprint that cannot be taken is a failure, not an empty string. Silently
            // publishing `""` would make two different dictionaries share an identity.
            let fingerprint = dictionary.content_fingerprint().unwrap_or_else(|error| {
                panic!(
                    "{} could not be fingerprinted: {error}. A report cannot claim an identity it \
                     does not have.",
                    dictionary.installed_as()
                )
            });
            json!({
                "title": dictionary.meta().title,
                "installed_as": dictionary.installed_as(),
                "generation": dictionary
                    .root()
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                "has_sequence": dictionary.has_sequence(),
                "fingerprint": fingerprint,
            })
        })
        .collect();

    // The workload, in order, by content — so edited capture text is a different run.
    let mut workload = <sha2::Sha256 as sha2::Digest>::new();
    for capture in &selected {
        for part in [capture.id.as_str(), capture.tier, capture.partition] {
            sha2::Digest::update(&mut workload, part.as_bytes());
            sha2::Digest::update(&mut workload, [0x1f]);
        }
        sha2::Digest::update(&mut workload, capture.text.as_bytes());
        sha2::Digest::update(&mut workload, [0x1e]);
    }
    let workload = sakatsu_core::hashing::hex(sha2::Digest::finalize(workload));

    let mut identity = <sha2::Sha256 as sha2::Digest>::new();
    for part in [
        runner::executable_digest(),
        runner::source_digest(),
        workload.clone(),
    ] {
        sha2::Digest::update(&mut identity, part.as_bytes());
        sha2::Digest::update(&mut identity, [0x1f]);
    }
    for dictionary in &dictionary_identities {
        sha2::Digest::update(&mut identity, dictionary.to_string().as_bytes());
        sha2::Digest::update(&mut identity, [0x1f]);
    }
    let identity = sakatsu_core::hashing::hex(sha2::Digest::finalize(identity));

    let report = json!({
        "what_this_is": "candidate recovery only; no selection, no correctness judgment",
        "run_identity": identity,
        "executable": runner::executable_digest(),
        "source_tree": runner::source_digest(),
        "workload_sha256": workload,
        "dictionaries": dictionary_identities,
        "transcripts": selected.len(),
        "by_partition": by_partition.iter().map(|(partition, (passages, proposals, supported))| {
            (partition.to_string(), json!({
                "passages": passages, "proposals": proposals, "supported_as_lexical": supported,
            }))
        }).collect::<serde_json::Map<_, _>>(),
        // The per-tier table, in the artifact and not only on the console — the promoted row is
        // the cohort the earlier 539/250 was measured over.
        "by_tier": by_tier.iter().map(|(tier, tally)| {
            (tier.to_string(), json!({
                "transcripts": tally.transcripts,
                "passages": tally.passages,
                "proposals": tally.proposals,
                "supported_as_lexical": tally.supported,
                "unsearched_extensions": tally.unsearched,
                "source_refusals": tally.refused,
            }))
        }).collect::<serde_json::Map<_, _>>(),
        "by_script_category": by_script.iter().map(|((script, joins), count)| {
            (format!("{script}/{joins}"), json!(count))
        }).collect::<serde_json::Map<_, _>>(),
        "script_categories_are_not_structure": true,
        "evidence_categories": by_evidence,
        "unsearched_extensions": unsearched,
        "source_refusals": refused,
        "enumeration_faults": faults,
        "dictionary_failures": failures,
        "invalid_baselines": invalid,
    });

    let body = serde_json::to_string_pretty(&report).expect("report");
    let path = artifact.join(format!("lexical-recovery-{}.json", &identity[..16]));
    // Written beside the target and renamed, and an existing file for this identity is only
    // replaced when its bytes already match. Differing content lands beside its predecessor.
    let staging = path.with_extension("json.partial");
    std::fs::write(&staging, &body).expect("stage the recovery report");
    let published = if path.exists() {
        let existing = std::fs::read(&path).unwrap_or_default();
        if existing == body.as_bytes() {
            std::fs::remove_file(&staging).expect("drop the redundant staging file");
            path
        } else {
            let content =
                sakatsu_core::hashing::hex(<sha2::Sha256 as sha2::Digest>::digest(body.as_bytes()));
            let beside = path.with_file_name(format!(
                "lexical-recovery-{}-{}.json",
                &identity[..16],
                &content[..16]
            ));
            println!(
                "\n  NOTE: a report exists for this identity and differs. The existing file is \
                 kept and this one is written beside it."
            );
            std::fs::rename(&staging, &beside).expect("publish beside the existing report");
            beside
        }
    } else {
        std::fs::rename(&staging, &path).expect("publish the report");
        path
    };
    println!("\n  artifact {}", published.display());

    assert!(
        failures.is_empty(),
        "a dictionary failure is not an absence: {failures:?}"
    );
    assert!(
        invalid.is_empty(),
        "a baseline that fails its own span contract is an error: {invalid:?}"
    );
    run.accounted_for().expect_success();
}
