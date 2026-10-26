//! Do the grammar rules fire under the analyzer the app actually selects?
//!
//! Ignored by default: needs a working `ichiran-cli`. Check with
//! `scripts/setup-ichiran.ps1`, then:
//!
//! ```text
//! cargo test -p sakatsu-core --test ichiran_coverage -- --ignored --nocapture
//! ```
//!
//! # Why this exists
//!
//! Every rule in the pack was authored and verified against Lindera, on the belief that
//! Lindera was what ran here. It is not: `choose_analyzer()` prefers Ichiran, and
//! Ichiran is installed. So the rules were verified against the fallback and shipped to
//! run under the primary.
//!
//! The two disagree in *both* directions. Lindera splits `には` into `に` + `は`, which
//! `match_text` surface runs exist to absorb. Ichiran fuses in the other direction —
//! `早く寝なさい` arrives as one token `寝なさい` — and no run of tokens equals `なさい`,
//! so a surface run cannot find it. That is what `surface_ends` is for.
//!
//! This harness names which rules fall in the second category, so companions can be
//! written for exactly those rather than guessed at.
//!
//! Ichiran costs ~600 ms per call and has no batch mode, so analyses are cached; the
//! first run takes minutes and later ones are immediate.

mod common;

use std::collections::HashMap;
use std::path::PathBuf;

use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer};
use sakatsu_core::grammar::{matcher, pack};
use sakatsu_core::model::Analysis;

fn cache_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/ichiran-analyses.json")
}

/// Analyses keyed by sentence, persisted between runs.
struct Cache {
    entries: HashMap<String, Analysis>,
    analyzer: IchiranAnalyzer,
    fetched: usize,
}

impl Cache {
    fn load() -> Cache {
        let entries = std::fs::read_to_string(cache_path())
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Cache {
            entries,
            analyzer: IchiranAnalyzer::new(),
            fetched: 0,
        }
    }

    fn get(&mut self, text: &str) -> Option<&Analysis> {
        if !self.entries.contains_key(text) {
            let analysis = analyze_waking_wsl(&self.analyzer, text)?;
            self.entries.insert(text.to_owned(), analysis);
            self.fetched += 1;
            // Flushed periodically: a run interrupted after twenty minutes of
            // subprocess calls should not throw all of them away.
            if self.fetched.is_multiple_of(100) {
                self.save();
            }
        }
        self.entries.get(text)
    }

    fn save(&self) {
        if let Ok(json) = serde_json::to_string(&self.entries) {
            let _ = std::fs::write(cache_path(), json);
        }
    }
}

/// Analyse, retrying once.
///
/// On Windows Ichiran lives in WSL, and WSL shuts its distribution down when idle. The
/// first call after that fails while the distribution starts — `wsl` exits 0xffffffff —
/// and the second succeeds. Without the retry a run that happens to begin cold either
/// aborts, or worse, quietly files every rule it could not analyse as unverifiable.
fn analyze_waking_wsl(analyzer: &IchiranAnalyzer, text: &str) -> Option<Analysis> {
    match analyzer.analyze(text) {
        Ok(analysis) => Some(analysis),
        Err(_) => analyzer.analyze(text).ok(),
    }
}

/// What became of a rule on a sentence it ought to match.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// The rule matched.
    Fires,
    /// Another rule claimed the same text. The pattern *is* detected, by a rule the
    /// matcher judged better — `きり` losing to the more specific `たきり`, say. Correct
    /// behaviour, and counting it as a gap overstates the problem.
    Shadowed(String),
    /// Nothing matched that text. The only bucket that is work.
    Undetected,
}

/// Where in the sentence the rule's own text sits, as a token range.
///
/// Used to ask whether *something* covered it, which is what separates being shadowed
/// from being missed. Without this the harness can only ask "did this id fire", and
/// reports a pattern that was found under a better name as though it were lost.
fn outcome(analysis: &Analysis, pattern_id: &str, wanted: &str) -> Outcome {
    let found = matcher::find_across(&analysis.tokens, &pack::all());
    if found.iter().any(|m| m.pattern_id == pattern_id) {
        return Outcome::Fires;
    }

    // The tokens whose surfaces make up the text this pattern is written as. Anything
    // overlapping them is covering the same ground.
    let mut covered = Vec::new();
    for (index, token) in analysis.tokens.iter().enumerate() {
        if wanted.contains(token.surface.as_str()) || token.surface.contains(wanted) {
            covered.push(index);
        }
    }
    for m in &found {
        if covered
            .iter()
            .any(|i| *i >= m.first_token && *i < m.last_token_end)
        {
            return Outcome::Shadowed(m.pattern_id.clone());
        }
    }
    Outcome::Undetected
}

#[test]
#[ignore = "needs a working ichiran-cli"]
fn every_rule_fires_under_ichiran() {
    let analyzer = IchiranAnalyzer::new();
    assert!(
        analyze_waking_wsl(&analyzer, "水").is_some(),
        "ichiran-cli is not answering — run scripts/setup-ichiran.ps1"
    );

    let mut cache = Cache::load();
    let mut works = Vec::new();
    let mut shadowed: Vec<(&str, String)> = Vec::new();
    let mut fails: Vec<(&str, &str, String)> = Vec::new();
    let mut unverifiable = Vec::new();

    for entry in pack::hanabira()
        .patterns
        .iter()
        .chain(&pack::core().patterns)
    {
        let ruled = !entry.match_text.is_empty() || entry.match_tokens.is_some();
        if !ruled {
            continue;
        }
        // The pack's own examples are the right probe here: the question is not whether
        // the rule is *correct* (the independent corpus answers that) but whether this
        // analyzer's segmentation lets it fire at all.
        //
        // Several examples per rule, not just the first. The pack does not promise every
        // example contains the pattern verbatim — なにも〜ません is illustrated with
        // なにも食べませんでした, whose tail is でした — so judging on one example alone
        // files a working rule as undetected. A rule that fires on any of its own
        // examples is one this analyzer can find.
        if entry.examples.is_empty() {
            unverifiable.push(entry.id.as_str());
            continue;
        }
        const TRIED: usize = 3;

        // What the rule is written as, for locating it in the sentence.
        let wanted = entry
            .match_text
            .first()
            .cloned()
            .unwrap_or_else(|| entry.pattern.clone());

        // Best outcome across the examples tried: firing anywhere settles it, and a
        // shadowed reading beats an undetected one.
        let mut best: Option<Outcome> = None;
        let mut worst_evidence: Option<(&str, String)> = None;
        for example in entry.examples.iter().take(TRIED) {
            let Some(analysis) = cache.get(&example.japanese) else {
                continue;
            };
            let result = outcome(analysis, &entry.id, &wanted);
            if matches!(result, Outcome::Undetected) && worst_evidence.is_none() {
                let surfaces: Vec<&str> = analysis
                    .tokens
                    .iter()
                    .map(|token| token.surface.as_str())
                    .collect();
                worst_evidence = Some((example.japanese.as_str(), surfaces.join(" / ")));
            }
            let better = match (&best, &result) {
                (None, _) => true,
                (Some(Outcome::Fires), _) => false,
                (Some(_), Outcome::Fires) => true,
                (Some(Outcome::Undetected), Outcome::Shadowed(_)) => true,
                _ => false,
            };
            if better {
                best = Some(result);
            }
        }

        match best {
            Some(Outcome::Fires) => works.push(entry.id.as_str()),
            Some(Outcome::Shadowed(by)) => shadowed.push((entry.id.as_str(), by)),
            Some(Outcome::Undetected) => {
                let (sentence, segmentation) = worst_evidence.expect("undetected has evidence");
                fails.push((entry.id.as_str(), sentence, segmentation));
            }
            None => unverifiable.push(entry.id.as_str()),
        }
    }
    cache.save();

    let total = works.len() + shadowed.len() + fails.len();
    println!(
        "\nunder Ichiran, of {total} rules ({} fetched this run, {} had no example):\n  \
         fires:      {}\n  shadowed:   {} (found, but reported under a better rule)\n  \
         undetected: {} (nothing matched — the only bucket that is work)\n",
        cache.fetched,
        unverifiable.len(),
        works.len(),
        shadowed.len(),
        fails.len()
    );

    println!("=== shadowed, which is correct behaviour ===");
    for (id, by) in shadowed.iter().take(12) {
        println!("  {id}\n     claimed by {by}");
    }

    println!("\n=== undetected under Ichiran ===");
    for (id, sentence, segmentation) in &fails {
        println!("  {id}\n     {sentence}\n     {segmentation}");
    }

    println!("\n{} rules need an Ichiran companion", fails.len());

    // A gate, not a report. Ichiran is the analyzer the app selects when it is present,
    // and the whole reason this gap existed is that nothing measured it — rules were
    // authored against the fallback and nobody noticed they were invisible in the app.
    //
    // The ones below are known and understood, not merely tolerated. (`n2-tsutsu` and
    // `n2-ijou`, which this list used to name, are found now.)
    //
    // - `n4-nikui` — Ichiran fuses the verb, にくい and the copula into one word
    //   (読みにくいです). A suffix rule for にくい would find it and also every みにくい
    //   (醜い) written in kana, so the built-in analyzer's rule is left as it is.
    // - `manma` (2026-10-17) — Ichiran misreads its one native example, 写真のまんまだね,
    //   as まん | まだ. No rule can repair a segmentation; the built-in analyzer reads it
    //   right.
    // - one more slot, so a single new entry does not fail the build before it can be
    //   looked at.
    const ALLOWED_UNDETECTED: usize = 3;
    assert!(
        fails.len() <= ALLOWED_UNDETECTED,
        "{} rules are undetected under Ichiran, more than the {ALLOWED_UNDETECTED} known \
         ones. A new rule is invisible in the app: {:?}",
        fails.len(),
        fails.iter().map(|(id, _, _)| id).collect::<Vec<_>>()
    );

    // Derive the companions rather than hand-writing 55 of them. For each failing rule
    // the sentence that failed is right here, already segmented, so the shape the rule
    // needs can be read off it directly.
    let mut companions = Vec::new();
    for (id, sentence, _) in &fails {
        let Some(entry) = pack::hanabira().get(id).or_else(|| pack::core().get(id)) else {
            continue;
        };
        let Some(run) = entry.match_text.first() else {
            continue; // already an element rule; those are hand-authored
        };
        let Some(analysis) = cache.entries.get(*sentence) else {
            continue;
        };
        if let Some(rule) = companion_for(analysis, run) {
            companions.push(serde_json::json!({ "id": id, "match": rule }));
        }
    }

    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/ichiran-companions.json");
    std::fs::write(&out, serde_json::to_string_pretty(&companions).unwrap()).expect("write");
    println!(
        "derived {} companions of {} failures -> {}",
        companions.len(),
        fails.len(),
        out.display()
    );
}

/// Work out what rule *would* have matched this run, given how Ichiran cut the
/// sentence.
///
/// Two shapes cover almost everything, and both come straight from where Ichiran put
/// its boundaries:
///
/// - the whole run is swallowed by one token (`掃除しなさい`) → a suffix predicate;
/// - the run straddles a boundary, its head fused into the previous token and its tail
///   standing alone (`勉強すれば` / `いい`) → a suffix predicate then a surface run.
///
/// Returns `None` when neither fits, which is the honest answer for a rule that needs
/// thinking about rather than a derivation.
fn companion_for(analysis: &Analysis, run: &str) -> Option<Vec<serde_json::Value>> {
    let surfaces: Vec<&str> = analysis
        .tokens
        .iter()
        .map(|token| token.surface.as_str())
        .collect();

    // Shape one: one token ends with the whole run and is longer than it.
    //
    // A bare suffix with no tail to pin it is only as specific as the kana it is made
    // of, and two characters is not enough: `まい` matched ぜんまい, ふるまい and
    // てしまい (four of five sampled hits wrong), `つつ` went over its ceiling the same
    // way. Both were caught by the independent-corpus gate and withdrawn, so the gate
    // remains the authority — this only avoids proposing the same two again.
    if run.chars().count() > 2
        && surfaces
            .iter()
            .any(|surface| surface.ends_with(run) && surface.len() > run.len())
    {
        return Some(vec![serde_json::json!({ "surface_ends": run })]);
    }

    // Shape two: split the run so its head ends a token and its tail spells the tokens
    // after. Longest head first, so the suffix carries as much as it can.
    let boundaries: Vec<usize> = (1..run.len())
        .rev()
        .filter(|at| run.is_char_boundary(*at))
        .collect();
    for at in boundaries {
        let (head, tail) = run.split_at(at);
        for (index, surface) in surfaces.iter().enumerate() {
            if !surface.ends_with(head) || surface.len() <= head.len() {
                continue;
            }
            let mut rest = String::new();
            for following in &surfaces[index + 1..] {
                rest.push_str(following);
                if rest.len() > tail.len() {
                    break;
                }
                if rest == tail {
                    return Some(vec![
                        serde_json::json!({ "surface_ends": head }),
                        serde_json::json!({ "text": tail }),
                    ]);
                }
            }
        }
    }

    None
}
