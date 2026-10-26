//! Measures the grammar match rules against a corpus they were **not** derived from.
//!
//! Ignored by default: needs `scripts/fetch-dictionaries.ps1` to have run.
//!
//! ```text
//! cargo test -p sakatsu-core --test pack_precision -- --ignored --nocapture
//! ```
//!
//! # Why this exists
//!
//! The authoring harness verifies each rule against its own entry's example
//! sentences. That is necessary — a rule that cannot find its own pattern is a typo —
//! but it is circular as a measure of precision: of course a rule fires on the
//! sentences it was derived from. Nothing there can catch a rule that *also* fires on
//! a thousand unrelated sentences.
//!
//! Jitendex's example sentences are the independent corpus. They are already on disk,
//! already parsed by this crate's Yomitan reader, already attributed (CC BY-SA 4.0,
//! `project notes` §8) — and they have nothing to do with the grammar pack.
//!
//! # What it can and cannot tell you
//!
//! There is no ground truth here, so this does **not** compute precision. It computes
//! *firing rate on independent text*, which is a screening signal: a rule tagged N1
//! that fires on several percent of everyday sentences is almost certainly matching a
//! common word that merely looks like the pattern (`以上` the noun, `ただ` the adverb),
//! not the grammar point. The samples it prints are what make that judgeable.

use std::collections::HashMap;
use std::path::PathBuf;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::dict::yomitan;
use sakatsu_core::grammar::{matcher, pack};
use sakatsu_core::model::Token;

/// Where the fetch script puts the zips. `SAKATSU_DICT_DIR` overrides.
fn dictionaries_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_DICT_DIR") {
        return PathBuf::from(dir);
    }
    let appdata = std::env::var_os("APPDATA").expect("APPDATA (or set SAKATSU_DICT_DIR)");
    PathBuf::from(appdata)
        .join("com.sakatsu.app")
        .join("dictionaries")
}

fn cache_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/jitendex-sentences.txt")
}

/// Every distinct Japanese example sentence in Jitendex.
///
/// Cached, because scanning a 36 MB archive of 400k+ entries takes long enough to be
/// annoying to do twice.
fn corpus() -> Vec<String> {
    let cache = cache_path();
    if let Ok(text) = std::fs::read_to_string(&cache) {
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        if !lines.is_empty() {
            println!("corpus: {} sentences (cached)", lines.len());
            return lines;
        }
    }

    let archive_path = dictionaries_dir().join("jitendex-yomitan.zip");
    let file = std::fs::File::open(&archive_path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}\nrun scripts/fetch-dictionaries.ps1 first",
            archive_path.display()
        )
    });
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).expect("open zip");

    let banks: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let file = archive.by_index(i).ok()?;
            let name = file.name().ok()?.into_owned();
            Some(name)
        })
        .filter(|name| name.starts_with("term_bank_") && name.ends_with(".json"))
        .collect();

    let mut seen = std::collections::HashSet::new();
    let mut sentences = Vec::new();
    for name in &banks {
        let mut entry = archive.by_name(name).expect("bank present");
        let mut json = String::new();
        std::io::Read::read_to_string(&mut entry, &mut json).expect("read bank");
        for parsed in yomitan::parse_term_bank(&json).expect("bank parses") {
            for definition in &parsed.definitions {
                for example in &definition.examples {
                    let text = example.japanese.trim();
                    // Fragments are not sentences and would skew the rate.
                    if text.chars().count() >= 6 && seen.insert(text.to_owned()) {
                        sentences.push(text.to_owned());
                    }
                }
            }
        }
    }

    println!(
        "corpus: {} sentences from {} banks",
        sentences.len(),
        banks.len()
    );
    let _ = std::fs::write(&cache, sentences.join("\n"));
    sentences
}

#[test]
#[ignore = "needs the fetched dictionaries"]
fn firing_rate_on_an_independent_corpus() {
    let sentences = corpus();
    assert!(
        !sentences.is_empty(),
        "no example sentences found in Jitendex"
    );

    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let tokenized: Vec<Vec<Token>> = sentences
        .iter()
        .map(|s| analyzer.analyze(s).expect("analyses").tokens)
        .collect();

    // Which pack an id came from, and what level it claims.
    let mut level: HashMap<&str, &str> = HashMap::new();
    for entry in &pack::hanabira().patterns {
        level.insert(&entry.id, entry.jlpt.as_deref().unwrap_or("?"));
    }

    let packs = pack::all();
    let mut hits: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, tokens) in tokenized.iter().enumerate() {
        for found in matcher::find_across(tokens, &packs) {
            hits.entry(found.pattern_id).or_default().push(i);
        }
    }

    let total = sentences.len() as f64;
    let mut imported: Vec<(&String, &Vec<usize>)> = hits
        .iter()
        .filter(|(id, _)| id.starts_with("hanabira-"))
        .collect();
    imported.sort_by_key(|(_, where_)| std::cmp::Reverse(where_.len()));

    // Rules of *either* form.
    //
    // This counted only `match_text` and so could not see a single element rule — which
    // is most of what has been written recently, and every rule converted to that form to
    // cope with Ichiran's fused tokens. A dead element rule was invisible here, which is
    // the one thing this test exists to notice.
    let has_rule = |p: &&sakatsu_core::grammar::pack::GrammarPattern| {
        !p.match_text.is_empty() || p.match_tokens.is_some()
    };
    let ruled = pack::hanabira().patterns.iter().filter(has_rule).count();
    println!(
        "\n{} of {ruled} imported rules fired at least once on independent text\n",
        imported.len()
    );

    println!("=== highest firing rate — the rules most likely to be wrong ===");
    let detail = std::env::var("SAMPLES").ok().and_then(|s| s.parse().ok());
    for (id, where_) in imported.iter().take(20) {
        let rate = where_.len() as f64 / total * 100.0;
        let lvl = level.get(id.as_str()).copied().unwrap_or("?");
        println!("  {lvl}  {:>6.3}%  {:>5} hits  {id}", rate, where_.len());
        for &i in where_.iter().take(detail.unwrap_or(2)) {
            println!("           {}", sentences[i]);
        }
    }

    println!("\n=== rules that never fired on independent text ===");
    let silent: Vec<&sakatsu_core::grammar::pack::GrammarPattern> = pack::hanabira()
        .patterns
        .iter()
        .filter(|p| has_rule(p) && !hits.contains_key(&p.id))
        .collect();
    println!("  {} of {ruled}", silent.len());

    // "Rare grammar is expected to be rare" was the whole of what this said, and it is
    // true of some of them. It is not a way to tell a rule nobody writes from a rule that
    // cannot match anything — and those need completely different work.
    //
    // Every entry carries its own examples, so ask each silent rule to find the pattern in
    // the sentence it was written from. Failing that is not rarity; it is broken.
    let mut broken = Vec::new();
    let mut rare = Vec::new();
    for pattern in &silent {
        let mut examples = pattern.examples.iter().map(|e| e.japanese.as_str());
        let fires = examples.any(|text| {
            let Ok(analysis) = analyzer.analyze(text) else {
                return false;
            };
            matcher::find_across(&analysis.tokens, &pack::all())
                .iter()
                .any(|found| found.pattern_id == pattern.id)
        });
        if pattern.examples.is_empty() {
            continue; // nothing to judge it against
        }
        if fires {
            rare.push(pattern.id.as_str());
        } else {
            broken.push((pattern.jlpt.as_deref().unwrap_or("?"), pattern.id.as_str()));
        }
    }

    broken.sort_unstable();
    println!(
        "    {} match their own example — rare, not broken",
        rare.len()
    );
    println!("    {} cannot match even their own example:", broken.len());
    for (level, id) in &broken {
        println!("      {level}  {id}");
    }
    assert!(
        broken.is_empty(),
        "rules that cannot match their own examples are dead: {:?}",
        broken.iter().map(|(_, id)| id).collect::<Vec<_>>()
    );

    let flooding: Vec<(&&String, f64)> = imported
        .iter()
        .map(|(id, w)| (id, w.len() as f64 / total))
        .filter(|(id, rate)| *rate > ceiling_for(level.get(id.as_str()).copied().unwrap_or("N1")))
        .filter(|(id, _)| !EXAMINED_AND_CORRECT.contains(&id.as_str()))
        .collect();
    println!(
        "\n{} rules fire above the ceiling for their level",
        flooding.len()
    );
    // With samples, always. A rule that fails the gate is precisely the one whose hits
    // have to be read before deciding, and ranking it out of the top-20 list left
    // nothing to judge by.
    for (id, rate) in &flooding {
        println!("  {:>6.3}%  {id}", rate * 100.0);
        for &i in hits[id.as_str()].iter().take(6) {
            println!("           {}", sentences[i]);
        }
    }

    // A gate, not a report. Every rule above the ceiling that has been examined and
    // found wrong is quarantined; a *new* one appearing means the next authoring batch
    // reintroduced the same class of defect — a surface run that is also an ordinary
    // compositional sequence (noun+particle, particle+verb stem, a literal reading).
    assert!(
        flooding.is_empty(),
        "these rules fire too often on text they were not derived from; check the \
         samples above and either quarantine them or constrain their neighbours: {:?}",
        flooding.iter().map(|(id, _)| id).collect::<Vec<_>>()
    );
}

/// How often a rule may fire on ordinary text before it is suspect — by level,
/// because the whole justification for a ceiling is level-specific.
///
/// N1 and N2 grammar is, by construction, not everyday language, so a rule at those
/// levels firing on 1 sentence in 500 is probably matching an ordinary word sequence
/// that happens to spell the same thing. N4 and N5 grammar is the opposite: しか〜ない
/// means "only", and "only" is not a rare thing to say. Holding beginner grammar to a
/// rare-grammar threshold flags correct rules — which it did, for しか〜ない, every one
/// of whose sampled hits was the real pattern.
///
/// These screen; they do not convict. A rule over its ceiling gets read, not deleted.
fn ceiling_for(level: &str) -> f64 {
    match level {
        "N5" | "N4" => 0.02,
        "N3" => 0.005,
        _ => 0.001,
    }
}

/// Rules over [`CEILING`] whose hits were read and found to be genuinely the grammar
/// point. Obligation is ordinary language, so these are common on their merits:
///
/// - `てはならない` — 路上に出店してはならない ("must not set up a stall on the road")
/// - `ねばならない` — 高い料金を払わねばならない ("must pay a high fee")
/// - `により` — 介護保険により負担されます ("is covered through care insurance")
///
/// Add to this list only after looking at the sentences the rule actually matched.
const EXAMINED_AND_CORRECT: &[&str] = &[
    "hanabira-n2-te-wa-naranai",
    "hanabira-n2-neba-naranai",
    "hanabira-n2-ni-yori",
];
