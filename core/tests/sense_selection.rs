//! When a word has several entries, does the app offer the right one first?
//!
//! ```text
//! cargo test -p sakatsu-core --test sense_selection -- --ignored --nocapture
//! ```
//!
//! # The gap this fills
//!
//! Everything else measured here is upstream of meaning: where a word starts and ends, how it is
//! read, what form it is in. None of it asks whether the reader is shown the *right word*, which
//! is closer to what this app is for. 前 is まえ "before" and ぜん "the former"; 方 is かた
//! "person" and ほう "direction". Segmenting and reading them perfectly and then offering the
//! wrong entry still leaves a learner with the wrong sentence.
//!
//! # Where the ground truth comes from
//!
//! **A Jitendex example sentence is filed under an entry, and an entry is a JMdict sequence with
//! its own reading and senses.** So a sentence stored beneath sequence 1579110 前【まえ】 is the
//! dictionary saying 前 means まえ *in that sentence* — an editorial judgement made by people, for
//! a purpose unrelated to ranking. `scripts/harvest-sentences.py --heteronym` already leans on
//! exactly this, and for the same reason it is not circular: the filing is editorial, the ordering
//! under test is by score and frequency, and neither knows about the other.
//!
//! # What is scored, and on which subset
//!
//! A term with one entry is right by construction and would only pad the number, so the figure
//! that matters is the **contested** one: terms whose lookup returns more than one distinct
//! sequence, where the app actually had to choose. Both are printed, because the gap between them
//! says how often the question even arises.
//!
//! # What the residue turned out to be, and why no rule was written for it
//!
//! The reading settles two thirds of the contested lookups. The rest was expected to be nouns
//! needing sentence evidence — the plan was rules in `dict::context::evidence` keyed on the
//! neighbouring particle and on the WordNet noun-category table `grammar::particles::Categories`
//! already loads.
//!
//! It is not nouns. **287 of the 296 same-reading cases carry no kanji at all**, and the terms
//! are ああ, いや, そう, うん, これ, あれ, がりがり — interjections and onomatopoeia — together
//! with katakana loanwords whose English sources collided: ランチ is lunch, launch *and* ranch;
//! コンパ is a party and a Haitian dance music.
//!
//! Neither signal reaches that. A particle cannot separate lunch from launch, and a noun-category
//! table has nothing to say about an interjection. Telling them apart needs to know what the
//! sentence is *about*, which is a different problem from the one this file was opened on, so the
//! rules were not written. The counts above are what a later attempt should be aimed by.
//!
//! **Two populations, and this file used to blur them.** The paragraph above is about the
//! *same-reading* residue — 296 cases, 287 without kanji, interjections and loanwords. The
//! contextual ranker is measured over a different and much larger set: 2,030 multi-hit lookups,
//! including 735 with only one distinct sequence; only 14% are kana-only and 95% lack conjugation.
//! The held-out retrieval probe reports the 1,295 distinct-entry cases separately. So "it is not
//! nouns" and "every term that ranks wrong is a noun" are both true, of different things, and
//! reading either as a claim about the whole file gets the diagnosis backwards.
//!
//! # What this cannot say
//!
//! It scores the *entry*, not the sense within it. An entry with six numbered senses counts as
//! correct when any of them was meant, so this is a floor on the harder question and should not be
//! read as "the right meaning was shown". It also inherits Jitendex's example coverage: common
//! words have examples and rare ones do not, so the sample leans easy.

mod common;
#[path = "common/context_retrieval.rs"]
mod retrieval;

use std::collections::BTreeMap;

use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::dict::frequency::{Direction, Frequencies};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::dict::{context, merge, SourceResult};
use sakatsu_core::kana::{is_kanji, to_hiragana};

/// One example sentence and the entry it is filed under.
struct Filed {
    term: String,
    sequence: i64,
}

/// Every fixture here answers the whole token: the component fallback lives in `lookup::sources`,
/// which is the only thing that can take it, so nothing assembled by hand could carry one honestly.
fn whole() -> sakatsu_core::dict::Retrieved {
    sakatsu_core::dict::Retrieved::Whole {
        query: String::new(),
    }
}

#[test]
#[ignore = "needs the dictionaries"]
fn the_entry_a_sentence_is_filed_under_is_the_one_offered_first() {
    let dir = common::dictionaries_dir().join("jitendex");
    let Ok(dictionary) = Dictionary::open(&dir) else {
        eprintln!(
            "SKIP: no Jitendex at {} — run scripts/fetch-dictionaries.ps1",
            dir.display()
        );
        return;
    };
    let Ok(entries) = dictionary.entries() else {
        eprintln!("SKIP: could not read entries");
        return;
    };

    // Filed examples, keyed by the sentence so one sentence is asked about once even when the
    // dictionary attaches it to several definitions of the same entry.
    let mut filed: BTreeMap<String, Filed> = BTreeMap::new();
    for entry in &entries {
        if entry.term.is_empty() || entry.sequence == 0 {
            continue;
        }
        for definition in &entry.definitions {
            for example in &definition.examples {
                if example.japanese.contains(&entry.term) {
                    filed.entry(example.japanese.clone()).or_insert(Filed {
                        term: entry.term.clone(),
                        sequence: entry.sequence,
                    });
                }
            }
        }
    }

    if filed.is_empty() {
        eprintln!("SKIP: no example sentences carry a sequence");
        return;
    }

    let (mut asked, mut first) = (0usize, 0usize);
    let (mut contested, mut contested_first, mut contested_present) = (0usize, 0usize, 0usize);
    // The contested ones that came out wrong, most frequent first — the list worth reading.
    let mut missed: BTreeMap<String, usize> = BTreeMap::new();

    for entry in filed.values() {
        let Ok(hits) = dictionary.lookup(&entry.term) else {
            continue;
        };
        // Only hits spelled as the word asked about. A reading-bucket hit is a different word
        // that happens to sound the same, and ranking is not being asked about those.
        let sequences: Vec<i64> = hits
            .iter()
            .filter(|hit| hit.entry.term == entry.term)
            .map(|hit| hit.entry.sequence)
            .collect();
        if sequences.is_empty() {
            continue;
        }

        asked += 1;
        let correct_first = sequences.first() == Some(&entry.sequence);
        if correct_first {
            first += 1;
        }

        let distinct: std::collections::BTreeSet<i64> = sequences.iter().copied().collect();
        if distinct.len() > 1 {
            contested += 1;
            if correct_first {
                contested_first += 1;
            } else {
                *missed.entry(entry.term.clone()).or_default() += 1;
            }
            if distinct.contains(&entry.sequence) {
                contested_present += 1;
            }
        }
    }

    let percent = |part: usize, whole: usize| {
        if whole == 0 {
            0.0
        } else {
            100.0 * part as f64 / whole as f64
        }
    };

    println!("\n=== the entry a sentence is filed under ===");
    println!("  example sentences with a filed entry   {}", filed.len());
    println!(
        "  offered first, all terms               {first} / {asked}  ({:.1}%)",
        percent(first, asked)
    );
    println!(
        "  offered first, contested terms only    {contested_first} / {contested}  ({:.1}%)   <- the number that means something",
        percent(contested_first, contested)
    );
    println!(
        "  the filed entry was at least present   {contested_present} / {contested}  ({:.1}%)",
        percent(contested_present, contested)
    );
    println!(
        "  A term with one entry is right by construction; the contested line is where the app\n  \
         had to choose. The gap between the two says how often the question arises at all."
    );

    // **Would the reading have decided it?**
    //
    // `index.rs` says the ordering key saturates — `Entry::score` is 200 for 22,553 entries — so
    // for exactly the common words that go wrong there is nothing to sort by, and the entries are
    // returned in whatever order the index holds them. Meanwhile the analyzer has already worked
    // out how the token is *read*, and against KWDLC gold it is right 98.3% of the time.
    //
    // 前【まえ】 and 前【ぜん】 are different sequences with different readings. If the reading the
    // analyzer produced picks the filed entry out of the candidates, then a free signal the app
    // already computes is being thrown away at the moment it would settle the question.
    //
    // Counted only where the reading actually narrows the field, because a reading shared by every
    // candidate decides nothing and counting it would flatter the idea.
    let analyzer = LinderaAnalyzer::new().expect("the dictionary is compiled in");
    let (mut narrowed, mut narrowed_correct, mut narrowed_today) = (0usize, 0usize, 0usize);
    // **The residue, split by why the reading could not speak.** These three want different
    // work and had been one number. `all_share` is the genuinely semantic case and the only one
    // that would justify sense rules for nouns; `none_match` is a reading or lookup-form problem
    // and belongs with `dict::readings`; `no_reading` is a third thing again.
    let (mut all_share, mut none_match, mut no_reading) = (0usize, 0usize, 0usize);
    // The term is in the sentence but not as a token of its own — the analyzer merged it into a
    // compound, so there is nothing to ask the reading about. Counted rather than dropped: the
    // buckets must add up to the contested total or the split is hiding something.
    let mut not_a_token = 0usize;
    // What the genuinely semantic cases look like, so a rule is written against evidence.
    let mut share_terms: BTreeMap<String, usize> = BTreeMap::new();
    let mut share_sample: Vec<String> = Vec::new();
    // Kanji or not, because it decides what evidence could ever help. A kanji noun might be
    // reachable from the particle beside it; ランチ meaning lunch, launch or ranch is a
    // transliteration collision that no syntax can separate.
    let mut share_kanaless = 0usize;
    for (sentence, entry) in &filed {
        let Ok(hits) = dictionary.lookup(&entry.term) else {
            continue;
        };
        let candidates: Vec<(i64, String)> = hits
            .iter()
            .filter(|hit| hit.entry.term == entry.term)
            .map(|hit| (hit.entry.sequence, to_hiragana(&hit.entry.reading)))
            .collect();
        let distinct: std::collections::BTreeSet<i64> =
            candidates.iter().map(|(sequence, _)| *sequence).collect();
        if distinct.len() < 2 {
            continue;
        }
        // How the analyzer reads that word in this sentence.
        let Ok(analysis) = analyzer.analyze(sentence) else {
            continue;
        };
        let Some(read) = analysis
            .tokens
            .iter()
            .find(|token| token.surface == entry.term)
            .map(|token| to_hiragana(&token.reading))
        else {
            not_a_token += 1;
            continue;
        };
        if read.is_empty() {
            no_reading += 1;
            continue;
        }
        let matching: Vec<i64> = candidates
            .iter()
            .filter(|(_, reading)| *reading == read)
            .map(|(sequence, _)| *sequence)
            .collect();
        // Narrowing means: some candidates match the reading, and some do not.
        if matching.is_empty() {
            none_match += 1;
            continue;
        }
        if matching.len() == candidates.len() {
            all_share += 1;
            *share_terms.entry(entry.term.clone()).or_default() += 1;
            if !entry.term.chars().any(is_kanji) {
                share_kanaless += 1;
            }
            if share_sample.len() < 12 {
                let rivals: Vec<String> = hits
                    .iter()
                    .filter(|hit| hit.entry.term == entry.term)
                    .map(|hit| {
                        let mark = if hit.entry.sequence == entry.sequence {
                            "*"
                        } else {
                            " "
                        };
                        let gloss = hit
                            .entry
                            .definitions
                            .first()
                            .and_then(|definition| definition.glosses.first())
                            .cloned()
                            .unwrap_or_default();
                        format!("{mark}{}", gloss.chars().take(34).collect::<String>())
                    })
                    .collect();
                share_sample.push(format!(
                    "  {sentence}
      {}",
                    rivals.join("  |  ")
                ));
            }
            continue;
        }
        narrowed += 1;
        if matching.first() == Some(&entry.sequence) {
            narrowed_correct += 1;
        }
        // **The same subset, ranked the way it is ranked today.** 87% against the 80.9% above
        // would be comparing different denominators; the only honest comparison is what the
        // present order achieves on exactly the cases the reading can speak to.
        if candidates.first().map(|(sequence, _)| *sequence) == Some(entry.sequence) {
            narrowed_today += 1;
        }
    }
    println!(
        "
  where the reading narrows the candidates — {narrowed} of {contested} contested"
    );
    println!(
        "  ranked as it is today             {narrowed_today} ({:.1}%)",
        percent(narrowed_today, narrowed)
    );
    println!(
        "  the reading picks the filed entry {narrowed_correct} ({:.1}%)",
        percent(narrowed_correct, narrowed)
    );

    // The rest of the contested set, and what would have to be built to reach it.
    let residue = all_share + none_match + no_reading;
    println!(
        "
  where the reading could not narrow — {residue} of {contested} contested"
    );
    println!(
        "  every candidate shares it  {all_share:>5}  <- genuinely semantic; noun sense rules"
    );
    println!("  no candidate matches it    {none_match:>5}  <- a reading or lookup-form problem");
    println!("  no reading produced        {no_reading:>5}  <- a third thing again");
    println!(
        "  not a token of its own     {not_a_token:>5}  <- merged into a compound; nothing to ask"
    );
    println!(
        "  {} + {residue} + {not_a_token} = {} of {contested}",
        narrowed,
        narrowed + residue + not_a_token
    );

    println!(
        "
  of the {all_share} same-reading cases, {share_kanaless} have no kanji ({:.0}%)
  the same-reading cases, most frequent term first",
        percent(share_kanaless, all_share)
    );
    let mut rows: Vec<_> = share_terms.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (term, count) in rows.iter().take(14) {
        println!("  {count:>4}x  {term}");
    }
    println!(
        "
  a sample, * marking the entry the sentence is filed under"
    );
    for line in share_sample.iter().take(6) {
        println!("{line}");
    }

    // **The lever the ordering key does not have.** `Entry::score` saturates at 200 for 22,553
    // entries, so for the common words that go wrong there is nothing to sort by. A frequency
    // list has exactly that: `Frequencies::rank` is keyed by `term\u{1}reading` before falling
    // back to the term, so it separates 前【まえ】 from 前【ぜん】 *and* says which is commoner —
    // both halves of the question, from data the app already fetches and already holds in
    // `FrequencyState`.
    //
    // Scored against the present order on the same contested set, because that is the only
    // comparison that means anything.
    let frequency_dir = common::dictionaries_dir().join("frequency").join("jpdb");
    let frequencies = Frequencies::load(&frequency_dir, Direction::Rank);
    if frequencies.is_empty() {
        println!(
            "\n  no jpdb frequency list at {} — skipping that comparison",
            frequency_dir.display()
        );
    } else {
        let (mut ranked, mut by_frequency, mut today) = (0usize, 0usize, 0usize);
        for entry in filed.values() {
            let Ok(hits) = dictionary.lookup(&entry.term) else {
                continue;
            };
            let candidates: Vec<(i64, String)> = hits
                .iter()
                .filter(|hit| hit.entry.term == entry.term)
                .map(|hit| (hit.entry.sequence, hit.entry.reading.clone()))
                .collect();
            let distinct: std::collections::BTreeSet<i64> =
                candidates.iter().map(|(sequence, _)| *sequence).collect();
            if distinct.len() < 2 {
                continue;
            }
            // Only where the list actually distinguishes the candidates. Where every candidate
            // gets the same rank — or none — frequency has said nothing and counting it would
            // flatter the idea, the same guard the reading comparison uses.
            let scored: Vec<(i64, Option<f64>)> = candidates
                .iter()
                .map(|(sequence, reading)| (*sequence, frequencies.rank(&entry.term, reading)))
                .collect();
            let values: std::collections::BTreeSet<String> =
                scored.iter().map(|(_, rank)| format!("{rank:?}")).collect();
            if values.len() < 2 {
                continue;
            }
            ranked += 1;
            if candidates.first().map(|(sequence, _)| *sequence) == Some(entry.sequence) {
                today += 1;
            }
            // Commonest first; anything the list has never heard of sorts last.
            let mut order = scored.clone();
            order.sort_by(|a, b| match (a.1, b.1) {
                (Some(left), Some(right)) => left.total_cmp(&right),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            });
            if order.first().map(|(sequence, _)| *sequence) == Some(entry.sequence) {
                by_frequency += 1;
            }
        }
        println!(
            "\n  where jpdb frequency separates the candidates — {ranked} of {contested} contested"
        );
        println!(
            "  ranked as it is today             {today} ({:.1}%)",
            percent(today, ranked)
        );
        println!(
            "  frequency picks the filed entry   {by_frequency} ({:.1}%)",
            percent(by_frequency, ranked)
        );
    }

    // Sense::sequence now preserves entry identity through merge, so a unique favourite can be
    // checked against the filed entry exactly. Keep the historical multi-hit population here to
    // measure the tag fix against the old 2,030 all-zero cases; the retrieval probe below also
    // separates lookups whose multiple hits all belong to one sequence.
    let (mut ranked_cases, mut abstained, mut favoured_from_filed) = (0usize, 0usize, 0usize);
    // Count what the abstentions are made of, rather than conflating this population with the
    // same-reading residue in the file header.
    //
    // `Token` carries no part of speech — the model deliberately keeps only what a reader is shown
    // — so "noun" here means what it means to `context::evidence` itself: no conjugation, hence
    // none of the inflectional signal the ranker is built out of.
    let (mut nounish, mut kanaless, mut no_evidence, mut tied) = (0usize, 0usize, 0usize, 0usize);
    // Measure entry-level deinflection rules separately before adding another field to the
    // shipping Sense contract. A rule belongs to the whole entry, not to one numbered sense.
    let mut rules_only = RuleTrial::default();
    let mut tags_and_rules = RuleTrial::default();
    let (mut fixed, mut broken) = (0usize, 0usize);
    for (sentence, entry) in &filed {
        let Ok(analysis) = analyzer.analyze(sentence) else {
            continue;
        };
        let Some(index) = analysis
            .tokens
            .iter()
            .position(|token| token.surface == entry.term)
        else {
            continue;
        };
        let token = &analysis.tokens[index];
        let Ok(hits) = dictionary.lookup(&entry.term) else {
            continue;
        };
        if hits.iter().filter(|h| h.entry.term == entry.term).count() < 2 {
            continue;
        }
        let sources = vec![SourceResult {
            retrieved: whole(),
            title: dictionary.meta().title.clone(),
            hits: hits.clone(),
        }];
        let word = merge::merge(token, &sources);
        let original_sequence = word.inline.first().and_then(|sense| sense.sequence);
        for (keep_tags, trial) in [(false, &mut rules_only), (true, &mut tags_and_rules)] {
            let mut projected = word.clone();
            for sense in &mut projected.inline {
                if !keep_tags {
                    sense.tags.clear();
                }
                for rule in hits
                    .iter()
                    .filter(|hit| Some(hit.entry.sequence) == sense.sequence)
                    .flat_map(|hit| &hit.entry.rules)
                {
                    if !sense.tags.contains(rule) {
                        sense.tags.push(rule.clone());
                    }
                }
            }
            trial.observe(
                &context::contextualize(projected, &analysis.tokens, index),
                entry.sequence,
            );
        }
        let contextual = context::contextualize(word, &analysis.tokens, index);
        ranked_cases += 1;
        let Some(favoured) = contextual.ranking.favoured else {
            abstained += 1;
            if token.conjugation.is_none() {
                nounish += 1;
            }
            if !entry.term.chars().any(is_kanji) {
                kanaless += 1;
            }
            // Two very different abstentions wear the same name. Either nothing scored at all —
            // the evidence never fired, and more rules of the same kind would change nothing — or
            // several senses scored equally and the tie is what blocked a winner. Only the second
            // is a tie-breaking problem, and the plan for this ranker turns on which it is.
            if contextual
                .ranking
                .senses
                .iter()
                .all(|sense| sense.score <= 0)
            {
                no_evidence += 1;
            } else {
                tied += 1;
            }
            continue;
        };
        let Some(sense) = contextual.word.inline.get(favoured) else {
            continue;
        };
        // **The real question, now that the merge carries provenance.** This compared gloss text
        // and said so: a gloss can appear under two entries, so it over-credited and was labelled
        // a ceiling rather than a score. `Sense::sequence` makes it exact.
        if sense.sequence == Some(entry.sequence) {
            favoured_from_filed += 1;
            fixed += usize::from(original_sequence != Some(entry.sequence));
        } else {
            broken += usize::from(original_sequence == Some(entry.sequence));
            println!(
                "  wrong rule winner: {} in {:?}; gold {}, picked {:?}, tags {:?}, evidence {:?}",
                entry.term,
                sentence,
                entry.sequence,
                sense.sequence,
                sense.tags,
                contextual.ranking.senses[favoured].reasons
            );
        }
    }
    println!("\n  the contextual ranker, on contested lookups it was given");
    println!("  cases                             {ranked_cases}");
    println!(
        "  abstained (no unique winner)      {abstained} ({:.1}%)",
        percent(abstained, ranked_cases)
    );
    println!(
        "    of those, no conjugation          {nounish} ({:.0}%)",
        percent(nounish, abstained)
    );
    println!(
        "    of those, no kanji                {kanaless} ({:.0}%)",
        percent(kanaless, abstained)
    );
    println!("    nothing scored at all             {no_evidence}");
    println!("    scored but tied                   {tied}");
    let decided = ranked_cases - abstained;
    println!(
        "  favoured the filed entry                 {favoured_from_filed} of {decided} decided ({:.1}%)",
        percent(favoured_from_filed, decided)
    );
    println!("  against the unranked merged order: {fixed} fixed / {broken} broken");
    println!("  Entry::rules alone (test-only projection): {rules_only:?}");
    println!("  normalized tags plus Entry::rules:         {tags_and_rules:?}");
    println!("  Entry identity is scored exactly; no-score abstentions and positive ties remain separate.");

    let mut rows: Vec<_> = missed.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("\n  contested terms ranked wrong, most frequent first");
    for (term, count) in rows.iter().take(20) {
        println!("  {count:>4}x  {term}");
    }

    // Deliberately no assertion. There is no rate this project has earned the right to require
    // yet, and a threshold invented here would be a number to satisfy rather than a fact to learn
    // from — the same reasoning `ocr_corpus.rs` records.
}

#[derive(Debug, Default)]
struct RuleTrial {
    no_score: usize,
    tied: usize,
    decided: usize,
    correct: usize,
}

impl RuleTrial {
    fn observe(&mut self, result: &sakatsu_core::dict::ContextualWordView, gold: i64) {
        if let Some(index) = result.ranking.favoured {
            self.decided += 1;
            self.correct += usize::from(result.word.inline[index].sequence == Some(gold));
        } else if result.ranking.senses.iter().any(|sense| sense.score > 0) {
            self.tied += 1;
        } else {
            self.no_score += 1;
        }
    }
}

/// Retrieval feasibility only: the gold entry's own sentence is removed from *every* exemplar
/// list before embedding. The ordinary population matches the earlier 2,030-case probe; that
/// probe also admits multiple hits from one sequence, which are counted separately below.
/// Report the >=2-example gold-entry subset and the stricter all-candidates-covered subset.
/// The latter is the useful gate: missing evidence for a rival must not make it lose by default.
/// Scores name entries, not numbered senses, and inherit the gold's common-word coverage bias.
#[test]
#[ignore = "needs Jitendex and the already installed local embedding model"]
fn held_out_examples_choose_between_entries() {
    use std::collections::BTreeSet;
    let Some(digest) = retrieval::installed_digest() else {
        eprintln!(
            "SKIP: installed {} unavailable at {}",
            common::EMBEDDER,
            common::endpoint()
        );
        return;
    };
    let Ok(dictionary) = Dictionary::open(&common::dictionaries_dir().join("jitendex")) else {
        eprintln!("SKIP: no imported Jitendex");
        return;
    };
    let entries = dictionary.entries().expect("read installed dictionary");
    let mut filed = BTreeMap::new();
    for entry in &entries {
        if entry.term.is_empty() || entry.sequence == 0 {
            continue;
        }
        for example in entry
            .definitions
            .iter()
            .flat_map(|definition| &definition.examples)
        {
            if example.japanese.contains(&entry.term) {
                filed.entry(example.japanese.clone()).or_insert(Filed {
                    term: entry.term.clone(),
                    sequence: entry.sequence,
                });
            }
        }
    }
    drop(entries);
    struct Case {
        query: String,
        gold: i64,
        source_right: bool,
        reading_right: bool,
        reading_narrows: bool,
        rules_abstain: bool,
        gold_has_two: bool,
        complete: bool,
        examples: BTreeMap<i64, Vec<String>>,
    }
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");
    let mut cases = Vec::new();
    let mut definition_cases = Vec::new();
    let (mut population, mut single_sequence, mut no_gold, mut removed_self) = (0, 0, 0, 0);
    for (sentence, filed) in filed {
        let analysis = analyzer.analyze(&sentence).expect("analyze example");
        let Some(index) = analysis
            .tokens
            .iter()
            .position(|token| token.surface == filed.term)
        else {
            continue;
        };
        let token = &analysis.tokens[index];
        let hits = dictionary.lookup(&filed.term).expect("lookup example term");
        if hits
            .iter()
            .filter(|hit| hit.entry.term == filed.term)
            .count()
            < 2
        {
            continue;
        }
        population += 1;
        let mut examples: BTreeMap<i64, BTreeMap<String, String>> = BTreeMap::new();
        for hit in hits
            .iter()
            .filter(|hit| hit.entry.term == filed.term && hit.entry.sequence != 0)
        {
            let bank = examples.entry(hit.entry.sequence).or_default();
            for example in hit
                .entry
                .definitions
                .iter()
                .flat_map(|definition| &definition.examples)
            {
                if !example.japanese.trim().is_empty() {
                    bank.entry(retrieval::sentence_key(&example.japanese))
                        .or_insert(example.japanese.clone());
                }
            }
        }
        if examples.len() < 2 {
            single_sequence += 1;
            continue;
        }
        let Some(gold) = examples.get(&filed.sequence) else {
            no_gold += 1;
            continue;
        };
        let gold_has_two = gold.len() >= 2;
        let query_key = retrieval::sentence_key(&sentence);
        for bank in examples.values_mut() {
            removed_self += usize::from(bank.remove(&query_key).is_some());
        }
        let examples: BTreeMap<i64, Vec<String>> = examples
            .into_iter()
            .map(|(key, bank)| (key, bank.into_values().collect()))
            .collect();
        let complete = examples.values().all(|bank| !bank.is_empty());
        let word = merge::merge(
            token,
            &[SourceResult {
                retrieved: whole(),
                title: dictionary.meta().title.clone(),
                hits: hits.clone(),
            }],
        );
        let source_right =
            word.inline.first().and_then(|sense| sense.sequence) == Some(filed.sequence);
        let ranked = context::contextualize(word, &analysis.tokens, index);
        let read = to_hiragana(&token.reading);
        let all_readings: BTreeSet<_> = hits
            .iter()
            .map(|hit| to_hiragana(&hit.entry.reading))
            .collect();
        let reading_narrows =
            !read.is_empty() && all_readings.contains(&read) && all_readings.len() > 1;
        let mut definitions = BTreeMap::<i64, (BTreeSet<String>, BTreeSet<String>)>::new();
        for hit in hits
            .iter()
            .filter(|h| examples.contains_key(&h.entry.sequence))
        {
            let entry = definitions.entry(hit.entry.sequence).or_default();
            entry.0.insert(hit.entry.reading.clone());
            entry.1.extend(
                hit.entry
                    .definitions
                    .iter()
                    .flat_map(|s| s.glosses.iter().cloned()),
            );
        }
        let mut preferred = hits;
        merge::prefer_matching_reading(&mut preferred, &read);
        let reading_word = merge::merge(
            token,
            &[SourceResult {
                retrieved: whole(),
                title: dictionary.meta().title.clone(),
                hits: preferred,
            }],
        );
        let reading_right =
            reading_word.inline.first().and_then(|sense| sense.sequence) == Some(filed.sequence);
        if complete && ranked.ranking.favoured.is_none() {
            // No example sentences are exported with the definitions. Gold and baselines are
            // evaluation-only fields and must never be included in a classifier prompt.
            definition_cases.push(serde_json::json!({
                "id": format!("entry-{}", definition_cases.len()), "text": sentence,
                "surface": token.surface, "start": token.start, "end": token.end,
                "gold": filed.sequence, "source_right": source_right, "reading_right": reading_right,
                "candidates": definitions.into_iter().map(|(id, (readings, glosses))|
                    serde_json::json!({"id":id,"readings":readings,"glosses":glosses})).collect::<Vec<_>>()
            }));
        }
        cases.push(Case {
            query: sentence,
            gold: filed.sequence,
            source_right,
            reading_right,
            reading_narrows,
            rules_abstain: ranked.ranking.favoured.is_none(),
            gold_has_two,
            complete,
            examples,
        });
    }
    assert!(
        population > 0 && !cases.is_empty(),
        "no contested examples were evaluated"
    );
    let export = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scratch/context-next");
    std::fs::create_dir_all(&export).unwrap();
    std::fs::write(
        export.join("entry-definitions.json"),
        serde_json::to_vec_pretty(&definition_cases).unwrap(),
    )
    .unwrap();
    println!("\n=== held-out entry retrieval ===\n  original probe population {population}; single sequence {single_sequence}; gold absent {no_gold}; distinct-entry cases {}; removed self-filings {removed_self}", cases.len());
    let texts = cases.iter().flat_map(|case| {
        std::iter::once(case.query.clone()).chain(case.examples.values().flatten().cloned())
    });
    let vectors = retrieval::Vectors::prepare(texts, &digest);
    let mut all = retrieval::Score::default();
    let mut two = retrieval::Score::default();
    let mut complete = retrieval::Score::default();
    let mut reading = retrieval::Score::default();
    let mut narrowed = retrieval::Score::default();
    let mut abstentions = retrieval::Score::default();
    let mut margins = [
        (0.02, retrieval::Score::default()),
        (0.05, retrieval::Score::default()),
    ];
    for case in &cases {
        let prediction = vectors.nearest(&case.query, &case.examples);
        let correct = prediction
            .as_ref()
            .map(|(winner, _, _)| *winner == case.gold);
        let random = 1.0 / case.examples.len() as f64;
        all.observe(case.source_right, correct, random);
        if case.gold_has_two {
            two.observe(case.source_right, correct, random);
        }
        if case.complete {
            complete.observe(case.source_right, correct, random);
            reading.observe(case.reading_right, correct, random);
            if case.reading_narrows {
                narrowed.observe(case.reading_right, correct, random);
            }
            if case.rules_abstain {
                abstentions.observe(case.reading_right, correct, random);
                for (minimum, score) in &mut margins {
                    let decided = prediction
                        .as_ref()
                        .filter(|(_, margin, _)| margin >= minimum)
                        .map(|(winner, _, _)| *winner == case.gold);
                    score.observe(case.reading_right, decided, random);
                }
            }
        }
    }
    all.report("all distinct entries; incomplete rival coverage allowed (diagnostic)");
    two.report("gold entry had >=2 distinct examples before withholding (diagnostic)");
    complete.report("every candidate has held-out evidence, versus source order");
    reading.report("same complete set, versus prefer_matching_reading");
    narrowed.report("complete set where reading narrows candidates");
    abstentions
        .report("eligible model path: complete evidence and rule abstention, versus reading order");
    for (margin, score) in margins {
        score.report(&format!(
            "predeclared margin >= {margin}; diagnostic, not tuned"
        ));
    }
    println!("  Integration requires improvement over source order AND reading order, plus held-out confirmation; partial coverage is not a passing result.");
}
