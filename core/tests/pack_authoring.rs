//! TEMPORARY authoring harness — not a test of behaviour.
//!
//! The imported JLPT pack ships 828 explained grammar points with no match rules, so
//! none of them is ever found in a sentence. Writing those rules by hand is guesswork;
//! this derives a candidate surface run from each entry's `pattern` field and then
//! **verifies it against that entry's own example sentences**, which is the part that
//! makes the result trustworthy. A candidate that cannot find the pattern in the
//! pack's own examples is not a rule, it is a typo.
//!
//! It deliberately runs the real `matcher::find` rather than a reimplementation, so a
//! candidate accepted here behaves identically in the app.
//!
//!     cargo test -p sakatsu-core --test pack_authoring -- --nocapture

mod common;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::grammar::matcher;
use sakatsu_core::grammar::pack::{self, Element, GrammarPattern, Pack};
use sakatsu_core::model::Token;

/// Which levels this run is authoring for. All of them now: earlier passes took the
/// uncontested entries level by level, and what remains — contested runs and
/// discontinuous patterns — is spread across every level. Entries that already carry a
/// rule are skipped when the output is applied.
const TARGET_LEVELS: &[&str] = &["N1", "N2", "N3", "N4", "N5"];

/// Candidates a reader has looked at and turned down, with what they caught.
///
/// The gates above are rates and counts; they cannot see that a run is also an ordinary word.
/// `ところ` passes the N4 ceiling and is spelled the same as ところ "place", so a rule for
/// "Verb る ところ" would label every place in a passage as "about to". Each rejection here was
/// made by reading the rule's corpus samples (`core/tests/rule_workbench.rs`, 2026-10-17); a
/// re-run must not quietly re-accept it.
const REVIEWED_OUT: &[(&str, &str)] = &[
    (
        "hanabira-n5-a-demo-b",
        "でも: mostly 'even / any' (誰でも, 日中でも), not sentence-initial 'but'",
    ),
    (
        "hanabira-n5-na-adjective-ni-shimasu",
        "します: fires on every します",
    ),
    (
        "hanabira-n4-you-ni",
        "ように: seems / in that way / as one wishes outnumber 'so that'",
    ),
    (
        "hanabira-n4-verb-ru-tokoro",
        "ところ: every sample was a place or a point (今のところ, 正直なところ)",
    ),
    (
        "hanabira-n4-bun-a-soreni-bun-b",
        "それに: mostly the pronoun 'to it' (それに違和感), not 'besides'",
    ),
    (
        "hanabira-n3-koto-da",
        "ことだ: 'it is a … thing' (普通のことだ), not the advice sense",
    ),
    (
        "hanabira-n3-kurai",
        "くらい: never fires; an earlier entry takes the same run",
    ),
    (
        "hanabira-n3-sore-to",
        "それと: それとなく and それとは, not 'and also'",
    ),
    (
        "hanabira-n3-tokoro-ga",
        "ところが: '…なところがある', not 'however'",
    ),
    (
        "hanabira-n3-ni-shite-wa",
        "にしては: 鵜呑みにしてはならない, おしまいにしては",
    ),
    (
        "hanabira-n2-sae-ba",
        "さえ: never fires; an earlier entry takes the same run",
    ),
    (
        "hanabira-n1-noun-kara-suru",
        "からする: never fires; an earlier entry takes the same run",
    ),
    (
        "hanabira-n1-kara-aru",
        "からある: literal 'since when' (いつからある), not 'as much as'",
    ),
    (
        "hanabira-n1-noun-wa-oroka-mo",
        "はおろか: mostly おろかにも, 'foolishly'",
    ),
    (
        "hanabira-n2-toiu-no-wa",
        "というのは: mid-sentence 'the thing called', not sentence-initial 'that is because'",
    ),
    (
        "hanabira-n3-nanka",
        "なんか: mostly the filler 'somehow' (なんか服が油臭い), not 'things like'",
    ),
    (
        "hanabira-n3-wa-mochiron-mo",
        "はもちろん: half are the adverb 'of course'",
    ),
    (
        "hanabira-n3-sono-kekka",
        "その結果: half are literal 'the result' (その結果に失望した)",
    ),
    (
        "hanabira-n3-koto-wa-nai",
        "ことはない: 'never have' and 'nothing good', not 'no need to'",
    ),
    (
        "hanabira-n5-noun-ni-shimasu",
        "します: fires on every します",
    ),
    (
        "hanabira-n4-verb-ta-tokoro",
        "ところ: every sample was a place or a point, as for Verb る ところ",
    ),
    (
        "hanabira-n4-nakya-ikenai",
        "なきゃいけない: the curated `nakya` takes the いけない after なきゃ and wins the span",
    ),
    (
        "hanabira-n3-shika-nai",
        "しかない: shadowed under the built-in analyzer by an earlier entry for the same span",
    ),
    // Rules that were already in the pack and died under the built-in analyzer, which now joins
    // a noun and its suffix into one word (家族ぐるみ, ボクサー並み), so a run for the suffix alone
    // has no word to match. Widening them to the word's ending was measured and rejected: the
    // only ぐるみ in the corpus is ぬいぐるみ, and most 並み are 町並み, 家並み and 月並み.
    (
        "hanabira-n1-gurumi",
        "ぐるみ: dead under the built-in analyzer; widened, it catches ぬいぐるみ",
    ),
    (
        "hanabira-n1-nami",
        "並み: dead under the built-in analyzer; widened, it catches 町並み and 月並み",
    ),
];

/// How early a learner meets a level. Used to settle a contested run.
fn level_order(level: &str) -> u8 {
    match level {
        "N5" => 0,
        "N4" => 1,
        "N3" => 2,
        "N2" => 3,
        _ => 4,
    }
}

/// Must match `pack_precision.rs`. A rule accepted here has to survive the gate there,
/// and two different thresholds would mean authoring rules that immediately fail.
fn ceiling_for(level: &str) -> f64 {
    match level {
        "N5" | "N4" => 0.02,
        "N3" => 0.005,
        _ => 0.001,
    }
}

fn is_japanese(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{309F}'      // hiragana
        | '\u{30A0}'..='\u{30FF}'    // katakana
        | '\u{4E00}'..='\u{9FFF}'    // kanji
        | '\u{3400}'..='\u{4DBF}'
        | '々')
}

/// Candidate surface runs for a pattern, taken as the maximal Japanese-script runs of
/// its `pattern` field. Everything else there is scaffolding — `A`, `B`, `～`, `。`
/// and spaces are placeholders for what the learner supplies.
fn candidates(pattern: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut current = String::new();
    for c in pattern.chars() {
        if is_japanese(c) {
            current.push(c);
        } else if !current.is_empty() {
            found.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        found.push(current);
    }
    // One character is never specific enough to be a rule of its own — those are the
    // bare particles, which the curated core pack already covers.
    found.retain(|run| run.chars().count() >= 2);
    found
}

/// Distinct runs, longest first. Kept separate from [`candidates`] because the *count*
/// of runs must include repeats: `A とも B とも` is a correlative pattern with two
/// slots, and collapsing it to one `とも` would misfile it as a simple continuous run.
fn distinct_candidates(pattern: &str) -> Vec<String> {
    let mut found = candidates(pattern);
    found.sort_by_key(|run| std::cmp::Reverse(run.chars().count()));
    found.dedup();
    found
}

/// Every surface run the curated pack already claims. The curated entries are
/// hand-written and have the better explanations, so an imported entry that would
/// match the same text is redundant at best and shadowing at worst.
fn core_claims() -> Vec<String> {
    pack::core()
        .patterns
        .iter()
        .flat_map(|p| p.match_text.iter().cloned())
        .collect()
}

/// Whether the written pattern shows this run *attaching* to something.
///
/// This is what separates a grammar point from a word. `Verb て ください`, `～だろう`
/// and `Noun だけ〜` are constructions: something comes before them, and that is the
/// grammar. `どこ～`, `さっき～` and `こちら～` are vocabulary — the run leads, and the
/// placeholder merely says a sentence continues.
///
/// It matters because the pack lists both, and marking every `さっき` in a passage as a
/// grammar point would bury the real ones. Words are the dictionary's job; this pane
/// is for what the words are doing.
fn is_bound(pattern: &str, candidate: &str) -> bool {
    match pattern.find(candidate) {
        // Anything before it at all — a placeholder, a particle, another word.
        Some(at) => pattern[..at].chars().any(|c| !c.is_whitespace()),
        None => false,
    }
}

/// Whether the curated pack already covers this run.
///
/// Not equality — containment. At N1 and N2 the two packs barely overlap, but N3 to N5 is
/// exactly where the curated pack lives, so `とき` against a curated `ときに` is a collision
/// even though the strings differ: the curated rule shadows the shorter one wherever both
/// match, and which one a reader is shown is an accident of where the sentence goes next.
///
/// **One direction only (2026-10-17).** This used to block containment either way, and the
/// other way caught every run that merely *contains* a curated particle: せいで contains で,
/// かわりに contains に, というと and ということだ contain と, でしょう contains で. Under
/// longest-match a run longer than a curated claim always wins at its own position and can
/// never be shadowed by it — it is the more specific point, and the particle still fires
/// everywhere else. So a candidate is covered only when a curated claim contains it: equal to
/// one (a duplicate) or shorter than one (shadowed). Measured by `core/tests/grammar_lists.rs`,
/// where entries blocked this way never fired on the corpus at all.
fn covered_by_core(candidate: &str, claims: &[String]) -> bool {
    claims.iter().any(|claim| claim.contains(candidate))
}

/// A pack holding exactly one surface-run rule, so the real matcher can be asked
/// whether that run occurs — on token boundaries, as the app would find it.
fn probe_pack(text: &str) -> Pack {
    Pack {
        name: "probe".into(),
        description: String::new(),
        patterns: vec![GrammarPattern {
            id: "probe".into(),
            pattern: text.into(),
            title: String::new(),
            jlpt: None,
            formation: None,
            explanation: String::new(),
            glosses: Vec::new(),
            detail: None,
            term_ids: Vec::new(),
            tags: Vec::new(),
            examples: Vec::new(),
            reference: None,
            match_tokens: None,
            match_text: vec![text.into()],
        }],
    }
}

fn fires(tokens: &[Token], candidate: &str) -> bool {
    !matcher::find(tokens, &probe_pack(candidate)).is_empty()
}

/// The gap between the two halves of a discontinuous pattern.
///
/// Six tokens is what `だれも〜ません` and `〜ば〜ほど` need in practice — enough for a
/// short clause, short enough that the two halves still plausibly belong to each other.
/// The gap refuses to cross sentence-final punctuation regardless, so this bounds how
/// far apart they may sit *within* a sentence.
const GAP: usize = 6;

/// A pack holding one gap rule, for a pattern written in two halves.
///
/// The tail is a suffix predicate rather than a surface run, so one rule covers both
/// analyzers: `だれも … ません` must find a standalone `ません` where Lindera splits it
/// and a fused `知りません` where Ichiran does not. "Ends with" is true of both. The
/// head stays a run because it is the analyzer-independent half — `だれも` is either one
/// token or `だれ` + `も`, and a run absorbs that either way.
fn gap_probe_pack(first: &str, second: &str) -> Pack {
    let mut pattern = probe_pack(first);
    pattern.patterns[0].match_text.clear();
    pattern.patterns[0].match_tokens = Some(gap_rule(first, second));
    pattern
}

fn gap_rule(first: &str, second: &str) -> Vec<Element> {
    vec![
        // The head is a suffix predicate for the same reason the tail is: Ichiran fuses
        // it into the preceding word. `Verb ないことがある` needs its head to match a
        // standalone `ない` under Lindera and a fused `出席しない` under Ichiran, and
        // "ends with" is true of both. The tail keeps the rule specific, which is what
        // makes a loose head safe here — verified by the independent-corpus gate.
        Element {
            surface_ends: Some(first.into()),
            ..Default::default()
        },
        Element {
            min: Some(0),
            max: Some(GAP),
            ..Default::default()
        },
        Element {
            surface_ends: Some(second.into()),
            ..Default::default()
        },
    ]
}

fn gap_fires(tokens: &[Token], first: &str, second: &str) -> bool {
    !matcher::find(tokens, &gap_probe_pack(first, second)).is_empty()
}

/// Ignored by default: this is an authoring tool, not a test of behaviour. It takes
/// ~15s and writes a file. Run it deliberately when extending the pack:
///
///     cargo test -p sakatsu-core --test pack_authoring -- --ignored --nocapture
#[test]
#[ignore = "authoring tool, not a behaviour test"]
fn derive_and_verify_match_rules() {
    let lindera = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let imported = pack::hanabira();

    // The independent corpus, not the pack's own examples. Screening a candidate
    // against the sentences it was derived from is circular and cannot catch a run
    // that also matches a thousand unrelated ones — which is how 上に got in claiming
    // furniture. The pack's examples are still used, but only to ask the other
    // question: does this run find its own pattern at all?
    let corpus: Vec<Vec<Token>> = common::jitendex_sentences()
        .iter()
        .map(|s| lindera.analyze(s).expect("lindera analyses").tokens)
        .collect();
    println!("independent corpus: {} sentences\n", corpus.len());

    struct Row {
        id: String,
        jlpt: String,
        pattern: String,
        /// How many Japanese runs the written pattern has. More than one means it is
        /// discontinuous (`だれも～ません`), and a single run can only anchor part of it.
        runs: usize,
        candidate: String,
        self_hits: usize,
        self_total: usize,
        corpus_hits: usize,
    }
    let mut rows = Vec::new();

    for entry in &imported.patterns {
        let own: Vec<Vec<Token>> = entry
            .examples
            .iter()
            .map(|e| {
                lindera
                    .analyze(&e.japanese)
                    .expect("lindera analyses")
                    .tokens
            })
            .collect();
        if own.is_empty() {
            continue;
        }

        // The *most distinctive* run wins, not the longest. `だれも～ません` yields
        // both `だれも` and `ません`; the second is the polite negative ending, shared
        // with every negative sentence in the language, and picking it would have four
        // unrelated entries claiming one rule. Ranking by how rarely a run occurs in
        // the corpus picks `だれも` — the part that actually identifies the pattern.
        let scored: Vec<(String, usize, usize)> = distinct_candidates(&entry.pattern)
            .into_iter()
            .map(|c| {
                let self_hits = own.iter().filter(|t| fires(t, &c)).count();
                let corpus_hits = corpus.iter().filter(|t| fires(t, &c)).count();
                (c, self_hits, corpus_hits)
            })
            .filter(|(_, self_hits, _)| *self_hits > 0)
            .collect();

        let best = scored
            .iter()
            .min_by_key(|(c, self_hits, corpus_hits)| {
                // Confirmed by its own examples first, then rarest, then longest.
                (
                    std::cmp::Reverse(*self_hits),
                    *corpus_hits,
                    std::cmp::Reverse(c.chars().count()),
                )
            })
            .cloned();

        if let Some((candidate, self_hits, corpus_hits)) = best {
            rows.push(Row {
                id: entry.id.clone(),
                jlpt: entry.jlpt.clone().unwrap_or_default(),
                pattern: entry.pattern.clone(),
                runs: candidates(&entry.pattern).len(), // repeats included, deliberately
                candidate,
                self_hits,
                self_total: own.len(),
                corpus_hits,
            });
        }
    }

    // A run two entries both claim can only ever report one of them — the matcher
    // breaks ties by pack order, so the loser is dead weight. Drop both rather than
    // silently pick a winner.
    let mut claims: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for row in &rows {
        *claims.entry(row.candidate.as_str()).or_default() += 1;
    }
    let contested: Vec<&Row> = rows
        .iter()
        .filter(|r| claims[r.candidate.as_str()] > 1)
        .collect();

    // Settle a contested run rather than dropping every claimant.
    //
    // Dropping both was the cautious first pass, and it cost 133 entries. Looking at
    // what actually contests, the caution was misplaced: these are overwhelmingly the
    // *same* grammar point listed twice — としたら at N3 and again at N2, くせに at N3
    // and N2, それで written as both `文A。それで 文B` and `A。それで B。`. They collide
    // precisely because they mean the same thing, so one explanation serves both.
    //
    // The winner is the level a learner meets first, so the text shown is the one
    // pitched at whoever is most likely to need it. Ties go to the lower id, so a
    // re-run picks the same entry.
    //
    // **Only among claimants that could be accepted (2026-10-17).** The winner used to be
    // chosen first and checked after, so a claimant that could never be accepted still won
    // the run and took it with it: `Noun や Noun など～` (N5, a two-part pattern, so no
    // single-run rule) beat `～など` (N3), was then rejected, and など was left with no rule
    // at all. Eligibility is everything below except the contest itself.
    //
    // **And an entry that already has a rule claims only what that rule spells.** `Noun や
    // Noun など～` carries the gap rule や…など; it was still entered for the bare run など, won
    // it, and — being skipped when the output is applied, as every ruled entry is — took the run
    // away from `～など` for nothing.
    let already_core = core_claims();
    let ruled_elsewhere = |r: &Row| {
        imported
            .patterns
            .iter()
            .find(|p| p.id == r.id)
            .is_some_and(|p| {
                (p.match_tokens.is_some() || !p.match_text.is_empty())
                    && !p.match_text.contains(&r.candidate)
            })
    };
    let eligible = |r: &Row| {
        // Every example confirms it, the curated pack does not already cover it, the
        // written pattern is a single continuous run (a discontinuous one needs a gap rule,
        // authored by hand), it does not flood independent text, and a reader has not
        // already looked at what it catches and turned it down.
        !ruled_elsewhere(r)
            && !REVIEWED_OUT.iter().any(|(id, _)| *id == r.id)
            && r.self_hits == r.self_total
            && !covered_by_core(&r.candidate, &already_core)
            && r.runs == 1
            && (r.corpus_hits as f64) / (corpus.len() as f64) <= ceiling_for(&r.jlpt)
            && is_bound(&r.pattern, &r.candidate)
            && TARGET_LEVELS.contains(&r.jlpt.as_str())
    };
    let mut winner: std::collections::HashMap<&str, &Row> = std::collections::HashMap::new();
    for row in rows.iter().filter(|r| eligible(r)) {
        let candidate = row.candidate.as_str();
        let beats = match winner.get(candidate) {
            Some(current) => {
                (level_order(&row.jlpt), &row.id) < (level_order(&current.jlpt), &current.id)
            }
            None => true,
        };
        if beats {
            winner.insert(candidate, row);
        }
    }

    let accepted: Vec<&Row> = rows
        .iter()
        .filter(|r| {
            eligible(r)
                && winner
                    .get(r.candidate.as_str())
                    .is_some_and(|w| w.id == r.id)
        })
        .collect();
    // Why named entries were not accepted, one line each — `SAKATSU_AUTHOR_IDS=id,id`.
    // Only the reasons; deciding what to do about them is the reader's job.
    if let Ok(ids) = std::env::var("SAKATSU_AUTHOR_IDS") {
        for id in ids.split(',').map(str::trim) {
            let Some(r) = rows.iter().find(|r| r.id == id) else {
                println!("  why {id}: no self-confirming candidate");
                continue;
            };
            let mut reasons = Vec::new();
            if r.self_hits != r.self_total {
                reasons.push(format!("own {}/{}", r.self_hits, r.self_total));
            }
            match winner.get(r.candidate.as_str()) {
                Some(w) if w.id != r.id => reasons.push(format!("contested, won by {}", w.id)),
                _ => {}
            }
            if covered_by_core(&r.candidate, &already_core) {
                let by: Vec<&String> = already_core
                    .iter()
                    .filter(|c| c.contains(r.candidate.as_str()))
                    .collect();
                reasons.push(format!("covered by core {by:?}"));
            }
            if r.runs != 1 {
                reasons.push(format!("{} runs", r.runs));
            }
            let rate = (r.corpus_hits as f64) / (corpus.len() as f64);
            if rate > ceiling_for(&r.jlpt) {
                reasons.push(format!("rate {rate:.4} over {}", ceiling_for(&r.jlpt)));
            }
            if !is_bound(&r.pattern, &r.candidate) {
                reasons.push("not bound".into());
            }
            println!(
                "  why {id} [{}] '{}': {}",
                r.jlpt,
                r.candidate,
                if reasons.is_empty() {
                    "accepted".into()
                } else {
                    reasons.join("; ")
                }
            );
        }
    }
    let deferred = rows.len() - accepted.len();

    println!(
        "{} of {} entries produced a self-confirming candidate.\n  \
         accepted:   {}\n  contested:  {} (two or more entries claim the same run)\n  \
         deferred:   {} (discontinuous, too common, or not confirmed by every example)\n",
        rows.len(),
        imported.patterns.len(),
        accepted.len(),
        contested.len(),
        deferred
    );

    let mut by_level = accepted.clone();
    by_level.sort_by_key(|r| std::cmp::Reverse(r.corpus_hits));
    println!("=== accepted, most frequent first ===");
    for row in by_level.iter().take(45) {
        println!(
            "  {:<3} {:<16} self {}/{}  corpus {:>3}   {}",
            row.jlpt, row.candidate, row.self_hits, row.self_total, row.corpus_hits, row.pattern
        );
    }

    println!("\n=== accepted by level ===");
    for level in ["N1", "N2", "N3", "N4", "N5"] {
        let n = accepted.iter().filter(|r| r.jlpt == level).count();
        let total = imported
            .patterns
            .iter()
            .filter(|p| p.jlpt.as_deref() == Some(level))
            .count();
        println!("  {level}: {n} of {total}");
    }

    // ---- discontinuous patterns -------------------------------------------------
    //
    // A written pattern with two Japanese runs is one construction in two halves —
    // だれも〜ません, あまり〜ありません, A とも B とも. No surface run can express that,
    // which is why the whole class had no rules. A gap rule can, and the halves come
    // straight out of the written pattern in order, so this is a pipeline rather than
    // the hand work it first looked like.
    let mut gap_rows = Vec::new();
    for entry in &imported.patterns {
        if entry.match_tokens.is_some() || !entry.match_text.is_empty() {
            continue; // already ruled
        }
        // In order of appearance, repeats kept: `A とも B とも` is two slots, not one.
        let halves = candidates(&entry.pattern);
        if halves.len() != 2 {
            continue;
        }
        let (first, second) = (&halves[0], &halves[1]);

        let own: Vec<Vec<Token>> = entry
            .examples
            .iter()
            .map(|e| lindera.analyze(&e.japanese).expect("analyses").tokens)
            .collect();
        if own.is_empty() {
            continue;
        }
        let self_hits = own.iter().filter(|t| gap_fires(t, first, second)).count();
        if self_hits != own.len() {
            continue;
        }

        let corpus_hits = corpus
            .iter()
            .filter(|t| gap_fires(t, first, second))
            .count();
        let jlpt = entry.jlpt.clone().unwrap_or_default();
        if (corpus_hits as f64) / (corpus.len() as f64) > ceiling_for(&jlpt) {
            continue;
        }
        gap_rows.push((
            entry.id.clone(),
            jlpt,
            first.clone(),
            second.clone(),
            corpus_hits,
            entry.pattern.clone(),
        ));
    }

    // Two entries claiming the same pair would shadow each other exactly as two
    // claiming the same run do.
    let mut pair_claims: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    for row in &gap_rows {
        *pair_claims
            .entry((row.2.clone(), row.3.clone()))
            .or_default() += 1;
    }
    gap_rows.retain(|row| pair_claims[&(row.2.clone(), row.3.clone())] == 1);
    gap_rows.sort_by_key(|row| std::cmp::Reverse(row.4));

    println!("\n=== discontinuous patterns, accepted as gap rules ===");
    for (_, jlpt, first, second, hits, pattern) in &gap_rows {
        println!("  {jlpt:<3} {first} … {second:<10} corpus {hits:>3}   {pattern}");
    }
    println!("  {} accepted", gap_rows.len());

    let gap_json: Vec<_> = gap_rows
        .iter()
        .map(|(id, _, first, second, _, _)| {
            serde_json::json!({ "id": id, "match": gap_rule(first, second) })
        })
        .collect();
    let gap_out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/accepted-gap-rules.json");
    std::fs::write(&gap_out, serde_json::to_string_pretty(&gap_json).unwrap()).expect("write");
    println!(
        "wrote {} gap rules to {}",
        gap_json.len(),
        gap_out.display()
    );

    println!("\n=== a sample of what was rejected, and why ===");
    for row in contested.iter().take(8) {
        println!("  contested  {:<12} {}", row.candidate, row.pattern);
    }
    for row in rows.iter().filter(|r| r.runs > 1).take(8) {
        println!("  multi-run  {:<12} {}", row.candidate, row.pattern);
    }

    // Written out so the accepted rules can be applied to the pack without this test
    // and the applying script disagreeing about what was accepted.
    let json: Vec<_> = accepted
        .iter()
        .map(|r| serde_json::json!({ "id": r.id, "match_text": [r.candidate] }))
        .collect();
    // Into target/, not the source tree: this is an intermediate the applying script
    // consumes, not something to commit.
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/accepted-rules.json");
    std::fs::write(&out, serde_json::to_string_pretty(&json).unwrap()).expect("write");
    println!("\nwrote {} accepted rules to {}", json.len(), out.display());
}
