//! Does the reader put the **right** reading over a spelling that has more than one?
//!
//! ```text
//! cargo test -p sakatsu-core --test heteronym_readings -- --nocapture
//! ```
//!
//! # Why this is separate from `reading_accuracy.rs`
//!
//! That test asks whether a reading is *one the dictionary lists*, which is the right question for
//! a word with a single reading and much too weak for one with several. It accepts any of them and
//! says why — from a spelling alone, picking one would be my judgement dressed as the
//! dictionary's. The cost is that the error a learner is least able to catch is the one exempted:
//! 31% of the words it checks are spellings with more than one listed reading.
//!
//! Context is what settles it, and `common/heteronyms.rs` gets the context from the dictionary
//! rather than from me — a sentence filed under the sequence for 上【うえ】 is Jitendex saying
//! that 上 reads うえ there. See that file for the whole argument.
//!
//! # What a failure here means
//!
//! The reader prints `token.reading` over `token.surface`. So a disagreement is exactly what a
//! learner would be shown, in a sentence a native speaker wrote, with a translation beside it that
//! says which word it is. There is no interpretation left to do.

mod common;
#[path = "common/heteronyms.rs"]
mod heteronyms;
#[path = "common/context_retrieval.rs"]
mod retrieval;

use std::collections::BTreeMap;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::dict::readings;

use heteronyms::HETERONYMS;

/// Katakana to hiragana. The analyzer reports some readings in katakana where the dictionary
/// spells them in hiragana; that is notation, not a claim about pronunciation.
fn to_hiragana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            other => other,
        })
        .collect()
}

/// What the analyzer read the spelling as in this sentence, or `None` if it never produced the
/// spelling as a token of its own.
///
/// **Absence is not a failure.** 前学期 may come back whole, and a reading for 前 alone was then
/// never claimed — there is nothing on screen to be wrong. Only a token whose surface *is* the
/// spelling carries a reading a learner sees over that spelling.
fn read_as(analyzer: &LinderaAnalyzer, text: &str, term: &str) -> Option<String> {
    let analysis = analyzer.analyze(text).ok()?;
    analysis
        .tokens
        .iter()
        .find(|token| token.surface == term && !token.reading.is_empty())
        .map(|token| to_hiragana(&token.reading))
}

#[test]
fn a_spelling_with_two_readings_gets_the_one_the_sentence_calls_for() {
    let analyzer = common::built_in().expect("embedded UniDic loads");

    let mut checked = 0;
    let mut agreed = 0;
    let mut not_a_token = 0;
    let mut wrong: Vec<(&str, &str, String, &str)> = Vec::new();
    // Which spellings the analyzer actually distinguishes, so a form it always reads one way is
    // visible as such rather than hiding inside a total.
    let mut per_term: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for case in HETERONYMS {
        match read_as(&analyzer, case.text, case.term) {
            None => not_a_token += 1,
            Some(ours) => {
                checked += 1;
                let entry = per_term.entry(case.term).or_insert((0, 0));
                entry.1 += 1;
                if ours == to_hiragana(case.reading) {
                    agreed += 1;
                    entry.0 += 1;
                } else {
                    wrong.push((case.term, case.reading, ours, case.english));
                }
            }
        }
    }

    println!("\n=== readings in context, against the dictionary's own filing ===");
    println!("  sentences            {}", HETERONYMS.len());
    println!("  spelling was a token {checked}");
    println!("  not a token of its own (no reading shown, not counted)  {not_a_token}");
    println!("  agreed               {agreed}");
    println!("  disagreed            {}", wrong.len());
    // **The rate above is not comparable across segmentation changes, and saying so is the point.**
    // A spelling only enters the denominator if the analyzer produced it as a token of its own, so
    // anything that joins more — a counter rule, a fixed expression, a compound — moves cases out
    // of "checked" and into "not a token" and the percentage moves with them, having learned
    // nothing about readings.
    //
    // Measured: adding six segmentation rules took this from 273/189 to 220/146. The 53 cases that
    // left were 43 agreements and 10 disagreements, which accounts for the whole difference — the
    // cases still in the population agreed at exactly the rate they had. It read as a 2.8-point
    // regression and was a population change.
    //
    // So compare the two counts, not the percentage, whenever segmentation has moved.
    println!(
        "  
  NOTE: {checked} checked and {not_a_token} absorbed. Both move when segmentation does;"
    );
    println!("        compare those counts against the last run before trusting the rate.");

    if !wrong.is_empty() {
        println!("\n  disagreements — what the reader would print, and what the sentence means:");
        for (term, want, ours, english) in &wrong {
            println!("    {term}   shown: {ours}   dictionary: {want}");
            println!("        {english}");
        }
    }

    let split: Vec<_> = per_term
        .iter()
        .filter(|(_, (right, total))| right != total)
        .collect();
    if !split.is_empty() {
        println!("\n  spellings not read correctly every time:");
        for (term, (right, total)) in split {
            println!("    {term}  {right}/{total}");
        }
    }

    let rate = if checked == 0 {
        0.0
    } else {
        agreed as f64 / checked as f64
    };
    println!("\n  agreement {:.1}%\n", rate * 100.0);

    assert!(
        checked >= MIN_CHECKED,
        "only {checked} of {} sentences produced the spelling as a token; \
         below {MIN_CHECKED} this measures too little to mean anything",
        HETERONYMS.len()
    );
    assert!(
        rate >= FLOOR,
        "contextual readings agreed on {:.1}% of {checked} sentences, floor is {:.0}%",
        rate * 100.0,
        FLOOR * 100.0
    );
}

/// **Measured, reviewed, and still a low number.**
///
/// 69.2% — 189 of 273. For comparison `reading_accuracy.rs` reports 100%, because it accepts any
/// reading the dictionary lists and 31% of the words it checks have more than one. Both numbers
/// are true; this is the one describing what a learner is shown.
///
/// The first run read 66.6%. The difference is not the analyzer improving: all 100 disagreements
/// were read against the English that came with each sentence, and 16 turned out to be the
/// *fixture's* fault — a Tatoeba link on the wrong JMdict sequence, asserting a reading the
/// sentence does not have. 上手 was filed under うわて for a line reading "come on from stage
/// left", where かみて is right and the analyzer gave it. Those are recorded in `MISFILED` in
/// `scripts/harvest-sentences.py`, with the reason for each, so regenerating reproduces the
/// reviewed set rather than re-importing the noise.
///
/// What remains is the analyzer's: 市場 as しじょう where the English says "marketplace" (いちば),
/// 金 as かね for "gold" three times over, 面 as めん in "water off a duck's back" (つら), 人気 as
/// にんき for "such a lonely place" (ひとけ).
///
/// The floor sits under the measurement rather than at it — the fixture regenerates from whichever
/// Jitendex is installed, so the exact figure moves a little. Raise it when readings improve.
/// **Never lower it to make a run pass**: this number is the claim that furigana can be trusted,
/// and lowering it is choosing to mislead someone quietly.
const FLOOR: f64 = 0.66;

/// Guards the rate against becoming vacuous. `not_a_token` is legitimate, but if segmentation
/// changed such that almost nothing were checked, a high rate over four words would look like
/// health. The suite has been green while testing nothing before — `project notes` says so.
const MIN_CHECKED: usize = 150;

/// **Does the app know it is guessing?**
///
/// The test above measures how often the pick is right. This one asks the question the reader
/// actually needs answered: when the pick is a guess, is it *marked* as one? A 69% reading that
/// admits which 31% it is unsure of is a different thing from a 69% reading that does not.
///
/// Against the installed Jitendex, because that is what decides. `correct` is the only layer with
/// a dictionary — an analyzer has none — so this is where the flag is set and the only place it
/// can be tested honestly.
#[test]
#[ignore = "needs the imported Jitendex"]
fn a_guessed_reading_says_so() {
    let dir = common::dictionaries_dir().join("jitendex");
    if !sakatsu_core::dict::index::Dictionary::exists_in(&dir) {
        println!("SKIP: no imported Jitendex at {}", dir.display());
        return;
    }
    let dictionaries: Vec<Dictionary> = Dictionary::open(&dir).into_iter().collect();
    let analyzer = common::built_in().expect("embedded UniDic loads");

    let mut flagged = 0;
    let (mut tied_controls, mut distant_controls) = (0, 0);
    let mut silent: Vec<(&str, String)> = Vec::new();

    for case in HETERONYMS {
        let mut analysis = analyzer.analyze(case.text).expect("analyses");
        readings::correct(&mut analysis, &dictionaries);
        let Some(token) = analysis
            .tokens
            .iter()
            .find(|token| token.surface == case.term)
        else {
            continue;
        };
        // The old aggregate floor assumed every alternative deserved a caveat. The production
        // policy now excludes distant, rare readings. Exercise both unambiguous policy extremes
        // against real entries, without reproducing or tuning its intermediate score threshold.
        let headword = if token.dict_form.is_empty() {
            &token.surface
        } else {
            &token.dict_form
        };
        let candidates = readings::candidates(headword, &dictionaries);
        if let Some((_, best)) = candidates.first() {
            if candidates.get(1).is_some_and(|(_, score)| score == best) {
                tied_controls += 1;
                assert!(
                    token.reading_ambiguous,
                    "equally ranked readings of {headword} need a caveat"
                );
            } else if candidates
                .iter()
                .skip(1)
                .all(|(_, score)| best - score >= 200.0)
            {
                distant_controls += 1;
                assert!(
                    !token.reading_ambiguous,
                    "{headword} has no competitive alternative"
                );
            }
        }
        if token.reading_ambiguous {
            flagged += 1;
        } else {
            silent.push((case.term, to_hiragana(&token.reading)));
        }
    }

    println!("\n=== does the reader admit the guess? ===");
    println!("  contested spellings marked   {flagged}");
    println!("  shown with no caveat         {}", silent.len());
    if !silent.is_empty() {
        println!("\n  unmarked diagnostic — rare rivals do not necessarily need a caveat:");
        for (term, ours) in silent.iter().take(20) {
            println!("    {term}   shown: {ours}");
        }
    }

    println!("  checked policy extremes: {tied_controls} tied leaders, {distant_controls} without a competitive rival");
    assert!(
        flagged + silent.len() >= MIN_CHECKED,
        "too few aligned tokens to measure caveat coverage"
    );
    assert!(
        tied_controls > 0 && distant_controls > 0,
        "both sides of the caveat policy must be exercised"
    );
}

/// **Flagging an inflected word must not become correcting one.**
///
/// `dict::readings` now looks a dictionary form up for inflected tokens, where before it returned
/// early and looked at nothing. That is what lets 開いた carry a caveat. It must not also let the
/// dictionary's reading for 開く be written over the surface's own — a headword's reading is not
/// an inflected form's, and 撮り the 連用形 (とり) against the dictionary's 〜撮り suffix (どり) is
/// the case that guard was built for.
///
/// The unit tests in `readings.rs` cannot check this: they hold no dictionary, so nothing is
/// looked up and any structure passes. This can.
#[test]
#[ignore = "needs the imported Jitendex"]
fn an_inflected_reading_is_flagged_but_never_rewritten() {
    let dir = common::dictionaries_dir().join("jitendex");
    if !sakatsu_core::dict::index::Dictionary::exists_in(&dir) {
        println!("SKIP: no imported Jitendex at {}", dir.display());
        return;
    }
    let dictionaries: Vec<Dictionary> = Dictionary::open(&dir).into_iter().collect();
    let analyzer = common::built_in().expect("embedded UniDic loads");

    let mut inflected = 0;
    let mut rewritten: Vec<(String, String, String)> = Vec::new();

    for case in HETERONYMS {
        let before = analyzer.analyze(case.text).expect("analyses");
        let mut after = before.clone();
        readings::correct(&mut after, &dictionaries);

        for (was, now) in before.tokens.iter().zip(after.tokens.iter()) {
            let is_inflected = was.conjugation.is_some()
                || (!was.dict_form.is_empty() && was.dict_form != was.surface);
            if !is_inflected || was.reading.is_empty() {
                continue;
            }
            inflected += 1;
            if was.reading != now.reading {
                rewritten.push((
                    was.surface.clone(),
                    was.reading.clone(),
                    now.reading.clone(),
                ));
            }
        }
    }

    println!("\n=== inflected tokens through the readings pass ===");
    println!("  seen        {inflected}");
    println!("  rewritten   {}", rewritten.len());
    for (surface, was, now) in rewritten.iter().take(10) {
        println!("    {surface}   {was} -> {now}");
    }

    assert!(inflected > 0, "no inflected tokens reached the pass at all");
    assert!(
        rewritten.is_empty(),
        "{} inflected reading(s) were overwritten with a headword's; the correction guard has \
         stopped holding",
        rewritten.len()
    );
}

/// Where the reading a sentence calls for sits in the dictionary's **own** ranking.
///
/// ```text
/// cargo test --release -p sakatsu-core --test heteronym_readings -- --ignored --nocapture ranking
/// ```
///
/// # The question this exists to answer
///
/// The test above measures 146 of 220. Its failures are single-kanji heteronyms where context
/// decides — 頭 0/3, 金 1/4, 種 1/3 — and the fix depends entirely on something nobody had
/// checked: **is the right reading the one the dictionary already ranks first?**
///
/// `dict::readings::correct` currently keeps the analyzer's pick for a contested spelling, and
/// only replaces it when the pick is one the dictionary never lists at all. If the sentence's
/// reading is usually the dictionary's top candidate, then the analyzer is being preferred over a
/// better-informed source and preferring the dictionary is a rule — cheap, local, testable.
///
/// If it is usually buried, or absent, then no static ranking settles it and only the sentence
/// can. That is a different and much larger piece of work, and this test exists so the choice
/// between them is made on a number.
///
/// # The answer, so nobody derives it twice
///
/// **13 fixed against 40 broken — a net loss of 27.** Preferring the dictionary would take the
/// reading score from 66.4% to about 54%. The dictionary ranks the right reading first in only
/// 18% of the failures, and in 40 cases where the analyzer is *right* the dictionary's first
/// choice is something else: 前 correct as まえ would become ぜん, 中 as ちゅう would become なか,
/// 分 as ぶん would become ふん.
///
/// The reason is structural. UniDic picks from a lattice, so its choice is made **in context**;
/// the dictionary's order is context-free frequency. On contested spellings the analyzer is
/// already better than any static ranking, which is exactly what `dict::readings::correct` assumes
/// when it keeps the analyzer's pick and only replaces a reading the dictionary never lists —
/// a design the module argued for and could not previously measure.
///
/// So readings are not reachable by ranking. What is left is sentence context, and that is the
/// model question deferred elsewhere. This test stays as the evidence for not trying the cheap
/// version again.
///
/// # Separate from the test above, deliberately
///
/// That one needs **nothing installed** — a committed fixture and the embedded UniDic — which
/// `project notes` names as the property worth copying, since "a machine with nothing installed shows
/// green while testing almost nothing." This needs Jitendex, so it is `#[ignore]`d and kept apart
/// rather than taking the other one's independence with it.
#[test]
#[ignore = "needs the dictionaries"]
fn where_the_right_reading_sits_in_the_dictionarys_ranking() {
    let dir = common::dictionaries_dir().join("jitendex");
    let dictionaries: Vec<Dictionary> = Dictionary::open(&dir).into_iter().collect();
    if dictionaries.is_empty() {
        eprintln!("SKIP: no Jitendex at {}", dir.display());
        return;
    }
    let analyzer = common::built_in().expect("embedded UniDic loads");

    let (mut first, mut buried, mut absent, mut sole) = (0usize, 0usize, 0usize, 0usize);
    let mut detail: Vec<String> = Vec::new();
    // **The other direction, without which the first is unreadable.** A rule preferring the
    // dictionary changes every contested reading, not only the wrong ones, so the cases it would
    // *break* have to be counted beside the ones it would fix. Counting only the fixes is how a
    // rule that nets negative comes to look like a win.
    let (mut safe, mut broken) = (0usize, 0usize);
    let mut casualties: Vec<String> = Vec::new();

    for case in HETERONYMS {
        let Some(ours) = read_as(&analyzer, case.text, case.term) else {
            continue;
        };
        let ranked = readings::candidates(case.term, &dictionaries);
        if ours == to_hiragana(case.reading) {
            if ranked.len() > 1 {
                match ranked.first() {
                    Some((best, _)) if to_hiragana(best) == ours => safe += 1,
                    Some((best, _)) => {
                        broken += 1;
                        casualties.push(format!(
                            "  {:<6} correct as {ours}, the dictionary would impose {best}",
                            case.term
                        ));
                    }
                    None => {}
                }
            }
            continue;
        }
        let at = ranked
            .iter()
            .position(|(r, _)| to_hiragana(r) == to_hiragana(case.reading));
        match at {
            _ if ranked.len() < 2 => sole += 1,
            Some(0) => {
                first += 1;
                detail.push(format!(
                    "  {:<6} shown {ours}, dictionary ranks {} FIRST",
                    case.term, case.reading
                ));
            }
            Some(n) => {
                buried += 1;
                detail.push(format!(
                    "  {:<6} shown {ours}, dictionary ranks {} at #{}  [{}]",
                    case.term,
                    case.reading,
                    n + 1,
                    ranked
                        .iter()
                        .map(|(r, score)| format!("{r} {score:.0}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            None => {
                absent += 1;
                detail.push(format!(
                    "  {:<6} shown {ours}, dictionary does not list {}",
                    case.term, case.reading
                ));
            }
        }
    }

    let total = first + buried + absent + sole;
    println!("\n=== where the sentence's reading sits in the dictionary's ranking ===");
    println!("  failures examined      {total}");
    println!("  dictionary ranks it 1st {first}   <- a rule would fix these");
    println!("  ranked lower            {buried}   <- a ranking cannot settle these");
    println!("  not listed at all       {absent}");
    println!("  only one reading listed {sole}   <- the dictionary has no opinion to prefer");
    println!(
        "\n  a rule preferring the dictionary reaches {:.0}% of the failures",
        100.0 * first as f64 / total.max(1) as f64
    );

    println!(
        "
=== and what such a rule would cost ==="
    );
    println!("  currently right, dictionary agrees    {safe}   (unchanged)");
    println!("  currently right, dictionary disagrees {broken}   <- these would break");
    println!(
        "  net: {first} fixed against {broken} broken = {}{}",
        if first >= broken { "+" } else { "-" },
        first.abs_diff(broken)
    );
    for line in casualties.iter().take(15) {
        println!("{line}");
    }

    println!();
    for line in detail.iter().take(40) {
        println!("{line}");
    }
}

/// Sentence retrieval over Aozora ruby, measured at two explicitly different points: raw
/// Lindera (the historical 146/220 baseline), and the app's dictionary-corrected, already
/// caveated surface. The proposed integration may act only on the latter, never on an inflected
/// token or its lemma reading. Both fixed and broken answers count, including abstention.
#[test]
#[ignore = "needs the cached Aozora exemplars, Jitendex and the local embedding model"]
fn aozora_neighbours_choose_readings() {
    let Some(digest) = retrieval::installed_digest() else {
        eprintln!(
            "SKIP: installed {} unavailable at {}",
            common::EMBEDDER,
            common::endpoint()
        );
        return;
    };
    let Some(bank) = reading_bank() else {
        return;
    };
    let dictionaries: Vec<_> = Dictionary::open(&common::dictionaries_dir().join("jitendex"))
        .into_iter()
        .collect();
    if dictionaries.is_empty() {
        eprintln!("SKIP: no imported Jitendex");
        return;
    }
    let analyzer = common::built_in().expect("embedded UniDic");
    let vectors = retrieval::Vectors::prepare(
        bank.values()
            .flat_map(|readings| readings.values().flatten().cloned())
            .chain(HETERONYMS.iter().map(|case| case.text.to_string())),
        &digest,
    );
    let mut raw = retrieval::Score::default();
    let mut corrected = retrieval::Score::default();
    let mut eligible = retrieval::Score::default();
    let mut complete = retrieval::Score::default();
    let mut margins = [
        (0.02, retrieval::Score::default()),
        (0.05, retrieval::Score::default()),
    ];
    let (mut missing_token, mut not_eligible, mut no_bank) = (0, 0, 0);
    for case in HETERONYMS {
        let mut analysis = analyzer.analyze(case.text).expect("analyze fixture");
        let Some(index) = analysis
            .tokens
            .iter()
            .position(|token| token.surface == case.term && !token.reading.is_empty())
        else {
            missing_token += 1;
            continue;
        };
        let raw_reading = to_hiragana(&analysis.tokens[index].reading);
        readings::correct(&mut analysis, &dictionaries);
        let token = &analysis.tokens[index];
        let baseline = to_hiragana(&token.reading);
        let gold = to_hiragana(case.reading);
        let allowed = token.reading_ambiguous
            && token.conjugation.is_none()
            && token.surface == token.dict_form;
        let candidates = candidate_examples(case.term, &bank, &dictionaries);
        if candidates.values().all(Vec::is_empty) {
            no_bank += 1;
        }
        let prediction = vectors.nearest(case.text, &candidates);
        let right = prediction.as_ref().map(|(reading, _, _)| *reading == gold);
        let random = 1.0 / candidates.len().max(1) as f64;
        raw.observe(raw_reading == gold, right, random);
        corrected.observe(baseline == gold, allowed.then_some(right).flatten(), random);
        if !allowed {
            not_eligible += 1;
            continue;
        }
        eligible.observe(baseline == gold, right, random);
        let query_key = retrieval::sentence_key(case.text);
        let all_covered = candidates.values().all(|examples| {
            examples
                .iter()
                .any(|text| retrieval::sentence_key(text) != query_key)
        });
        if all_covered {
            complete.observe(baseline == gold, right, random);
            for (minimum, score) in &mut margins {
                let right = prediction
                    .as_ref()
                    .filter(|(_, margin, _)| margin >= minimum)
                    .map(|(reading, _, _)| *reading == gold);
                score.observe(baseline == gold, right, random);
            }
        }
        if let Some((reading, margin, represented)) = prediction {
            if reading != baseline {
                println!("  change {}: {} -> {} gold {} margin {:.4}; {represented}/{} readings represented", case.term, baseline, reading, gold, margin, candidates.len());
            }
        }
    }
    println!("\n=== Aozora reading retrieval: {} fixtures; missing exact token {missing_token}; outside doubt/inflection guard {not_eligible}; no exemplar bank {no_bank} ===", HETERONYMS.len());
    raw.report("raw analyzer; unconstrained diagnostic (comparable baseline, not a shipping rule)");
    corrected.report(
        "dictionary-corrected baseline; act only on already-ambiguous uninflected surfaces",
    );
    eligible.report("eligible subset; incomplete rival coverage allowed (diagnostic)");
    complete.report("eligible subset with every dictionary reading represented");
    for (margin, score) in margins {
        score.report(&format!(
            "complete coverage, predeclared margin >= {margin}; diagnostic"
        ));
    }
    report_capture_reading_gold();
}

type ReadingBank = BTreeMap<String, BTreeMap<String, Vec<String>>>;

fn reading_bank() -> Option<ReadingBank> {
    let path = std::env::var_os("SAKATSU_AOZORA_READINGS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| retrieval::scratch().join("aozora/readings.tsv"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("SKIP: no Aozora exemplar bank at {}; run scripts/harvest-aozora-readings.py --download", path.display());
        return None;
    };
    let mut bank: ReadingBank = BTreeMap::new();
    for (line, row) in text.lines().enumerate().skip(1) {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 5, "malformed exemplar TSV line {}", line + 1);
        bank.entry(fields[0].to_string())
            .or_default()
            .entry(to_hiragana(fields[1]))
            .or_default()
            .push(fields[2].to_string());
    }
    assert!(!bank.is_empty(), "empty exemplar bank");
    Some(bank)
}

fn candidate_examples(
    term: &str,
    bank: &ReadingBank,
    dictionaries: &[Dictionary],
) -> BTreeMap<String, Vec<String>> {
    readings::candidates(term, dictionaries)
        .into_iter()
        .map(|(reading, _)| {
            let reading = to_hiragana(&reading);
            let examples = bank
                .get(term)
                .and_then(|readings| readings.get(&reading))
                .cloned()
                .unwrap_or_default();
            (reading, examples)
        })
        .collect()
}

fn report_capture_reading_gold() {
    let segmented = std::fs::read_dir(common::corpus_dir())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().to_string_lossy().ends_with(".tok.mine"))
        .count();
    println!("  capture-domain gate: {segmented} .tok.mine files provide draft segmentation, not gold readings; no capture reading-accuracy claim can be made from them.");
}

/// Independent JUMAN-lineage reading gold, strictly measurement-only. KWDLC annotations and
/// their embeddings remain outside committed data; they never enter the Aozora exemplar bank.
/// This checks a second register, not manga/game accuracy. Only exact gold/analyzer spans are
/// scored, so disagreement about word boundaries cannot masquerade as a reading error.
#[test]
#[ignore = "needs external KWDLC, cached Aozora exemplars, Jitendex and the local embedding model"]
fn aozora_reading_retrieval_against_kwdlc() {
    let Some(digest) = retrieval::installed_digest() else {
        eprintln!("SKIP: installed embedding model unavailable");
        return;
    };
    let Some(bank) = reading_bank() else {
        return;
    };
    let dictionaries: Vec<_> = Dictionary::open(&common::dictionaries_dir().join("jitendex"))
        .into_iter()
        .collect();
    if dictionaries.is_empty() {
        eprintln!("SKIP: no imported Jitendex");
        return;
    }
    let root = std::env::var_os("SAKATSU_KWDLC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            common::dictionaries_dir()
                .parent()
                .expect("dictionary parent")
                .join("corpora/kwdlc")
        });
    let mut files = Vec::new();
    let mut pending = vec![root.join("knp")];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "knp") {
                files.push(path);
            }
        }
    }
    if files.is_empty() {
        eprintln!("SKIP: no external KWDLC annotations");
        return;
    }
    files.sort();
    let mut sentences = Vec::new();
    for file in files {
        sentences.extend(knp_readings(
            &std::fs::read_to_string(file).expect("read KWDLC annotation"),
        ));
    }
    let limit = std::env::var("SAKATSU_KWDLC_SAMPLE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(2000)
        .max(1);
    let stride = sentences.len().div_ceil(limit).max(1);
    let analyzer = common::built_in().expect("embedded UniDic");
    let (mut sampled, mut eligible, mut aligned, mut uncovered) = (0, 0, 0, 0);
    let mut cases = Vec::new();
    for (text, gold) in sentences.iter().step_by(stride) {
        sampled += 1;
        let mut analysis = analyzer.analyze(text).expect("analyze held-out sentence");
        readings::correct(&mut analysis, &dictionaries);
        for token in &analysis.tokens {
            if !token.reading_ambiguous
                || token.conjugation.is_some()
                || token.surface != token.dict_form
            {
                continue;
            }
            eligible += 1;
            let Some((_, _, reading)) = gold
                .iter()
                .find(|(start, end, _)| Some(*start) == token.start && Some(*end) == token.end)
            else {
                continue;
            };
            aligned += 1;
            let candidates = candidate_examples(&token.surface, &bank, &dictionaries);
            if candidates.values().all(Vec::is_empty) {
                uncovered += 1;
            }
            cases.push((
                text.clone(),
                to_hiragana(&token.reading),
                to_hiragana(reading),
                candidates,
            ));
        }
    }
    assert!(
        !cases.is_empty(),
        "no eligible aligned KWDLC readings were scored"
    );
    let vectors = retrieval::Vectors::prepare(
        cases.iter().flat_map(|(text, _, _, candidates)| {
            std::iter::once(text.clone()).chain(candidates.values().flatten().cloned())
        }),
        &digest,
    );
    let mut all = retrieval::Score::default();
    let mut complete = retrieval::Score::default();
    for (text, before, gold, candidates) in &cases {
        let predicted = vectors
            .nearest(text, candidates)
            .map(|(reading, _, _)| reading == *gold);
        let random = 1.0 / candidates.len().max(1) as f64;
        all.observe(before == gold, predicted, random);
        if candidates.values().all(|examples| {
            examples
                .iter()
                .any(|sentence| retrieval::sentence_key(sentence) != retrieval::sentence_key(text))
        }) {
            complete.observe(before == gold, predicted, random);
        }
    }
    println!("\n=== KWDLC measurement only: {} sentences, stride {stride}, sampled {sampled}, eligible {eligible}, exact gold spans {aligned}, no bank {uncovered} ===", sentences.len());
    all.report("corrected ambiguous uninflected tokens; partial coverage diagnostic");
    complete.report("same eligibility, every reading represented");
}

type GoldReadingSentence = (String, Vec<(usize, usize, String)>);

fn knp_readings(body: &str) -> Vec<GoldReadingSentence> {
    let mut out = Vec::new();
    let (mut text, mut spans) = (String::new(), Vec::new());
    for line in body.lines() {
        if line == "EOS" {
            if !spans.is_empty() {
                out.push((std::mem::take(&mut text), std::mem::take(&mut spans)));
            }
            continue;
        }
        if line.starts_with(['#', '*', '+', '@']) {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 11 {
            continue;
        }
        let start = text.len();
        text.push_str(fields[0]);
        spans.push((start, text.len(), fields[1].to_string()));
    }
    out
}

#[test]
fn reading_gold_preserves_utf8_spans_and_sentence_boundaries() {
    // Synthetic KNP-shaped text, not copied corpus material.
    let gold = knp_readings("# sentence\n* 0 -1D\n+ 0 -1D\n山 やま 山 名詞 6 普通名詞 1 * 0 * 0 NIL\n道 みち 道 名詞 6 普通名詞 1 * 0 * 0 NIL\nEOS\n@ ignored alternate\n");
    assert_eq!(
        gold,
        [(
            "山道".into(),
            vec![(0, 3, "やま".into()), (3, 6, "みち".into())]
        )]
    );
}
