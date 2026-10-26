//! The grammar packs measured against two public JLPT grammar lists.
//!
//! ```text
//! python scripts/fetch-grammar-lists.py --download                           # once
//! cargo test -p sakatsu-core --test grammar_lists -- --ignored --nocapture   # the reports
//! ```
//!
//! Four questions, each written to its own review sheet in `target/evaluation/grammar-lists/`:
//!
//! 1. **Levels** — where the pack's JLPT level disagrees with the lists'.
//! 2. **Coverage** — which listed points the pack has no entry for, or has one that never fires.
//! 3. **Casual forms** — edewakaru's 口語形 list (〜ちゃう, 〜なきゃ …): does the pack have each
//!    one, and does it fire on real sentences containing it?
//! 4. **Confusable pairs** — edewakaru's 類似文法比較 list: where both rules of a pair fire on one
//!    span, a reader is shown two competing explanations of the same words.
//!
//! # What the lists are, and are not
//!
//! 絵でわかる日本語 (edewakaru.com) and the JLPT Grammar List (jlptgrammarlist.neocities.org)
//! declare no licence, so they have the standing `project notes` §8 gives KWDLC, Imabi and Tae Kim:
//! **consult, never import**. `scripts/fetch-grammar-lists.py` keeps only names, levels, glosses
//! and links, outside the repository; the reports are written under `target/`, which is not
//! committed. Nothing here copies an explanation or an example sentence from either site.
//!
//! # What it can and cannot tell you
//!
//! - **No list is authoritative.** There has been no official JLPT grammar list since 2010. Two
//!   independent lists agreeing with each other and not with the pack is the strongest signal
//!   available — a reason to look, never a verdict. What a reading settles goes in
//!   `local documentation`, as the level decisions there always have.
//! - **Names are matched, not meanings.** A point both lists spell differently is reported as
//!   unmatched, never guessed at, because a wrong match would pin a level disagreement on the wrong
//!   entry. So the coverage gaps hold naming differences as well as real gaps.
//! - **Corpus sentences are found by substring, never by the matcher**, which is what keeps the
//!   firing rates from measuring the matcher against itself. They are a screening signal: the
//!   printed samples, with their English, are what make a rate judgeable. Sentences that become
//!   tests are chosen by reading that English, as everywhere else here.

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::PathBuf;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::grammar::pack;
use sakatsu_core::grammar::{compounds, matcher};

const LEVELS: [&str; 5] = ["N5", "N4", "N3", "N2", "N1"];
const SAMPLES: usize = 3;

fn lists_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_GRAMMAR_LISTS") {
        return PathBuf::from(dir);
    }
    common::dictionaries_dir()
        .parent()
        .map(|parent| parent.join("corpora").join("grammar-lists"))
        .unwrap_or_default()
}

fn report_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/evaluation/grammar-lists")
}

/// One line of `grammar-lists.tsv`, as `scripts/fetch-grammar-lists.py` writes it.
#[derive(Clone)]
struct Row {
    source: String,
    list: String,
    name: String,
    sense: String,
    level: String,
    url: String,
    note: String,
}

fn rows() -> Option<Vec<Row>> {
    let text = std::fs::read_to_string(lists_dir().join("grammar-lists.tsv")).ok()?;
    Some(
        text.lines()
            .skip(1)
            .filter_map(|line| {
                let field: Vec<&str> = line.split('\t').collect();
                (field.len() == 8).then(|| Row {
                    source: field[0].into(),
                    list: field[1].into(),
                    name: field[2].into(),
                    sense: field[3].into(),
                    level: field[4].into(),
                    url: field[6].into(),
                    note: field[7].into(),
                })
            })
            .collect(),
    )
}

fn is_japanese(c: char) -> bool {
    // Hiragana and katakana (the long-vowel mark included), the CJK block, and 々.
    matches!(c as u32, 0x3041..=0x30FA | 0x30FC..=0x30FF | 0x4E00..=0x9FFF | 0x3005)
}

fn hiragana(text: &str) -> String {
    text.chars()
        .map(|c| match c as u32 {
            0x30A1..=0x30F6 => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// Every spelling a grammar point's name stands for, reduced to what both lists write alike.
///
/// Drops what one list writes and the other does not: bracketed notes and sense numbers
/// (`〜だけ①（VれるだけV）`), the 〜 placeholder, the English placeholders the pack uses (`Verb`,
/// `Noun`, `A`), punctuation. Splits on the separators both use between alternatives — `・`, `／`
/// and `/`. Nothing fuzzier, deliberately: see the module note on why an unmatched name is better
/// than a guessed one.
fn spellings(name: &str) -> Vec<String> {
    let mut kept = String::new();
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '（' | '(' => depth += 1,
            '）' | ')' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            _ => kept.push(c),
        }
    }
    let mut out: Vec<String> = kept
        .split(['・', '／', '/'])
        .map(|part| part.chars().filter(|c| is_japanese(*c)).collect::<String>())
        .filter(|part| !part.is_empty())
        .collect();
    out.dedup();
    out
}

/// The keys one spelling is filed under: as written, in hiragana, and as the analyzer reads it —
/// which is what lets `に関して` find `にかんして`.
fn keys(analyzer: &LinderaAnalyzer, spelling: &str) -> Vec<(String, &'static str)> {
    let read = analyzer
        .analyze(spelling)
        .map(|analysis| {
            analysis
                .tokens
                .iter()
                .map(|token| {
                    if token.reading.is_empty() {
                        token.surface.clone()
                    } else {
                        token.reading.clone()
                    }
                })
                .collect::<String>()
        })
        .unwrap_or_default();
    let mut out = vec![
        (spelling.to_owned(), "spelling"),
        (hiragana(spelling), "spelling"),
    ];
    if !read.is_empty() {
        out.push((hiragana(&read), "reading"));
    }
    out
}

struct Index {
    by_key: HashMap<String, BTreeSet<String>>,
}

impl Index {
    fn build(analyzer: &LinderaAnalyzer) -> Self {
        let mut by_key: HashMap<String, BTreeSet<String>> = HashMap::new();
        for pack in pack::all() {
            for entry in &pack.patterns {
                let title = entry.title.split(" (").next().unwrap_or(&entry.title);
                for spelling in spellings(&entry.pattern)
                    .into_iter()
                    .chain(spellings(title))
                {
                    for (key, _) in keys(analyzer, &spelling) {
                        by_key.entry(key).or_default().insert(entry.id.clone());
                    }
                }
            }
        }
        Index { by_key }
    }

    /// The pack entries a listed name resolves to, and whether it took the analyzer's reading.
    fn resolve(&self, analyzer: &LinderaAnalyzer, name: &str) -> (BTreeSet<String>, &'static str) {
        let mut found = BTreeSet::new();
        let mut how = "spelling";
        for spelling in spellings(name) {
            for (key, kind) in keys(analyzer, &spelling) {
                if let Some(ids) = self.by_key.get(&key) {
                    if kind == "reading" && found.is_empty() {
                        how = "reading";
                    }
                    found.extend(ids.iter().cloned());
                }
            }
        }
        (found, how)
    }
}

/// The corpus, analysed once: each sentence's matches as (pattern id, first token, token end).
struct Corpus {
    sentences: Vec<(String, String)>,
    matches: Vec<Vec<(String, usize, usize)>>,
    tokens: Vec<Vec<String>>,
    fires: HashMap<String, Vec<usize>>,
}

impl Corpus {
    fn load(analyzer: &LinderaAnalyzer) -> Self {
        let sentences = common::jitendex_examples();
        let packs = pack::all();
        let mut matches = Vec::with_capacity(sentences.len());
        let mut tokens = Vec::with_capacity(sentences.len());
        let mut fires: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, (japanese, _)) in sentences.iter().enumerate() {
            let Ok(analysis) = analyzer.analyze(japanese) else {
                matches.push(Vec::new());
                tokens.push(Vec::new());
                continue;
            };
            // The rules, and the helpers found inside compound words — 〜ている in 見ている —
            // which the grammar panel shows beside them (`grammar::mod`, `compounds::detect`).
            // Counting only the rules would call every helper unexplained.
            let mut found: Vec<(String, usize, usize)> =
                matcher::find_across(&analysis.tokens, &packs)
                    .into_iter()
                    .map(|m| (m.pattern_id, m.first_token, m.last_token_end))
                    .collect();
            for (at, token) in analysis.tokens.iter().enumerate() {
                for helper in compounds::detect(token) {
                    found.push((helper.pattern_id.to_owned(), at, at + 1));
                }
            }
            for (id, _, _) in &found {
                let list = fires.entry(id.clone()).or_default();
                if list.last() != Some(&i) {
                    list.push(i);
                }
            }
            matches.push(found);
            tokens.push(analysis.tokens.into_iter().map(|t| t.surface).collect());
        }
        Corpus {
            sentences,
            matches,
            tokens,
            fires,
        }
    }

    fn fires(&self, id: &str) -> usize {
        self.fires.get(id).map_or(0, Vec::len)
    }
}

/// The form a casual contraction stands for — a fact about the contraction, not anyone's text.
///
/// Used to ask whether the grammar panel explains the contraction through the entry for its full
/// form. `って` has no single expansion (quotation と, hearsay という, topic は), so it gets none.
/// `ちゃ`/`じゃ` are 〜ては/〜では contracted, and the entry that explains them is the casual
/// prohibition, 〜ちゃだめ. Voiced contractions (じゃう, どく, でる) are named by the entries'
/// て spelling: the pack has 〜ている, not 〜でいる, and the first version of this table asked for
/// the voiced name, so a contraction the analyzer explained still counted as unexplained.
fn expansion(form: &str, listed: &str) -> Option<&'static str> {
    Some(match form {
        "ちゃう" => "てしまう",
        "じゃう" => "てしまう",
        "とく" => "ておく",
        "どく" => "ておく",
        "てる" => "ている",
        "でる" => "ている",
        "てく" => "ていく",
        "なきゃ" => "なければ",
        "なくちゃ" => "なくては",
        "とこ" => "ところ",
        "ばっかり" | "ばっか" => "ばかり",
        "んで" => "ので",
        "じゃん" => "じゃない",
        "でしょ" => "でしょう",
        "だろ" => "だろう",
        "たげる" => "てあげる",
        "まんま" => "まま",
        "ちゃ" if listed.contains("じゃ") => "ちゃだめ",
        "じゃ" if listed.contains("ちゃ") => "ちゃだめ",
        "たって" => "ても",
        "だって" if listed.contains("たって") => "でも",
        _ => return None,
    })
}

fn level_index(level: &str) -> Option<usize> {
    LEVELS.iter().position(|l| *l == level)
}

/// Whether `level` lies in a list's `range` (`N3`, `N3-N2`). `None` for "not a JLPT pattern".
fn within(level: &str, range: &str) -> Option<bool> {
    let at = level_index(level)?;
    let (low, high) = range.split_once('-').unwrap_or((range, range));
    let (a, b) = (level_index(low)?, level_index(high)?);
    Some((a.min(b)..=a.max(b)).contains(&at))
}

/// How many levels apart the pack is from the nearest level a list gives.
fn gap(level: &str, ranges: &BTreeSet<String>) -> usize {
    let Some(at) = level_index(level) else {
        return 0;
    };
    ranges
        .iter()
        .flat_map(|range| {
            let (low, high) = range.split_once('-').unwrap_or((range, range));
            [level_index(low), level_index(high)]
        })
        .flatten()
        .map(|i| i.abs_diff(at))
        .min()
        .unwrap_or(0)
}

fn sample(corpus: &Corpus, indices: impl Iterator<Item = usize>) -> String {
    let mut out = String::new();
    for i in indices.take(SAMPLES) {
        let (japanese, english) = &corpus.sentences[i];
        let _ = writeln!(out, "  - {japanese} — *{english}*");
    }
    out
}

#[test]
#[ignore = "needs scripts/fetch-grammar-lists.py and the fetched dictionaries"]
fn grammar_lists_report() {
    let Some(rows) = rows() else {
        println!(
            "SKIP: no lists in {} — run python scripts/fetch-grammar-lists.py --download, or set SAKATSU_GRAMMAR_LISTS",
            lists_dir().display()
        );
        return;
    };
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let index = Index::build(&analyzer);
    let entries: HashMap<&str, &pack::GrammarPattern> = pack::all()
        .iter()
        .flat_map(|p| p.patterns.iter())
        .map(|e| (e.id.as_str(), e))
        .collect();
    let corpus = Corpus::load(&analyzer);
    println!(
        "corpus: {} sentences; lists: {} rows",
        corpus.sentences.len(),
        rows.len()
    );

    let resolved: Vec<(BTreeSet<String>, &str)> = rows
        .iter()
        .map(|row| index.resolve(&analyzer, &row.name))
        .collect();
    let dir = report_dir();
    std::fs::create_dir_all(&dir).expect("report dir");

    // ── 0. Matching ──────────────────────────────────────────────────────────────────────────
    // Everything below rests on names resolving to the right entries, so how they resolved is
    // written down first: a name that reaches many entries spreads one list's level over all of
    // them, and a reading-only match is the least certain kind.
    let mut spread = [0usize; 4];
    let mut md = String::from(
        "# Matching: how each listed name resolved to pack entries\n\n\
         | resolved to | names |\n|---|---:|\n",
    );
    let mut broad = String::new();
    let mut by_reading = String::new();
    for (row, (ids, how)) in rows.iter().zip(&resolved) {
        let bucket = ids.len().min(3);
        spread[bucket] += 1;
        let listed = ids
            .iter()
            .map(|id| format!("`{id}`"))
            .collect::<Vec<_>>()
            .join(" ");
        if ids.len() > 2 {
            let _ = writeln!(
                broad,
                "- {} ({}, {}) → {listed}",
                row.name, row.source, row.level
            );
        }
        if *how == "reading" && !ids.is_empty() {
            let _ = writeln!(by_reading, "- {} ({}) → {listed}", row.name, row.source);
        }
    }
    for (label, count) in ["none", "one", "two", "three or more"].iter().zip(spread) {
        let _ = writeln!(md, "| {label} | {count} |");
    }
    let _ = writeln!(md, "\n## Three or more entries — check these first\n\n{broad}\n## Matched only by reading\n\n{by_reading}");
    std::fs::write(dir.join("matching.md"), &md).expect("write matching");
    println!(
        "matching: none {}, one {}, two {}, three or more {}",
        spread[0], spread[1], spread[2], spread[3]
    );

    // ── 1. Levels ────────────────────────────────────────────────────────────────────────────
    #[derive(Default)]
    struct Seen {
        edewakaru: BTreeSet<String>,
        jgl: BTreeSet<String>,
        links: BTreeSet<String>,
        names: BTreeSet<String>,
        /// Whether any listed name reached this entry alone. When none did, every level here came
        /// through a name the pack spends on several entries — ている, ように — and a disagreement
        /// is as likely to be a different sense as a different level.
        alone: bool,
    }
    let mut seen: BTreeMap<String, Seen> = BTreeMap::new();
    for (row, (ids, _)) in rows.iter().zip(&resolved) {
        if row.list != "level" {
            continue;
        }
        for id in ids {
            let entry = seen.entry(id.clone()).or_default();
            entry.names.insert(format!("{} ({})", row.name, row.source));
            entry.alone |= ids.len() == 1;
            if row.source == "edewakaru" {
                entry.edewakaru.insert(row.level.clone());
                entry.links.insert(row.url.clone());
            } else {
                entry.jgl.insert(row.level.clone());
            }
        }
    }
    let mut strong = Vec::new();
    let mut split = Vec::new();
    let mut single = Vec::new();
    let mut agree = 0usize;
    let mut unlevelled = 0usize;
    for (id, s) in &seen {
        let Some(level) = entries.get(id.as_str()).and_then(|e| e.jlpt.as_deref()) else {
            unlevelled += 1;
            continue;
        };
        let verdict = |ranges: &BTreeSet<String>| -> Option<bool> {
            if ranges.is_empty() {
                return None;
            }
            Some(ranges.iter().any(|r| within(level, r).unwrap_or(false)))
        };
        let (e, j) = (verdict(&s.edewakaru), verdict(&s.jgl));
        let all: BTreeSet<String> = s.edewakaru.union(&s.jgl).cloned().collect();
        let line = (gap(level, &all), id.clone(), level.to_owned());
        match (e, j) {
            (Some(false), Some(false)) => {
                // The two lists agreeing with each other is what makes this the strongest case.
                let each_other = s
                    .jgl
                    .iter()
                    .any(|j| s.edewakaru.iter().any(|r| within(j, r) == Some(true)));
                if each_other {
                    strong.push(line)
                } else {
                    split.push(line)
                }
            }
            (Some(false), _) | (_, Some(false)) => single.push(line),
            _ => agree += 1,
        }
    }
    for list in [&mut strong, &mut split, &mut single] {
        list.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    let mut md = String::from("# Levels: the pack against two public lists\n\n");
    let _ = writeln!(
        md,
        "{} pack entries matched by name; {agree} agree with every list that has them, {} have no level.\n",
        seen.len(),
        unlevelled
    );
    for (title, list) in [
        (
            "Both lists disagree with the pack, and agree with each other",
            &strong,
        ),
        (
            "Both lists disagree with the pack, and with each other",
            &split,
        ),
        ("One list disagrees", &single),
    ] {
        let _ = writeln!(md, "## {title} ({})\n\n| levels apart | entry | pattern | pack | edewakaru | jlptgrammarlist | edewakaru post |\n|---:|---|---|---|---|---|---|", list.len());
        let _ = writeln!(md, "<!-- *shared name*: every list row reaching this entry also reached another, so the disagreement may be a different sense. -->");
        for (apart, id, level) in list {
            let s = &seen[id];
            let join = |set: &BTreeSet<String>| set.iter().cloned().collect::<Vec<_>>().join(", ");
            let _ = writeln!(
                md,
                "| {apart} | `{id}`{} | {} | {level} | {} | {} | {} |",
                if s.alone { "" } else { " *shared name*" },
                entries[id.as_str()].pattern,
                join(&s.edewakaru),
                join(&s.jgl),
                join(&s.links)
            );
        }
        md.push('\n');
    }
    std::fs::write(dir.join("levels.md"), &md).expect("write levels");
    println!(
        "levels: {} matched; {agree} agree; both disagree and agree with each other {}; both disagree, differently {}; one disagrees {}",
        seen.len(),
        strong.len(),
        split.len(),
        single.len()
    );

    // ── 2. Coverage ──────────────────────────────────────────────────────────────────────────
    let mut md = String::from("# Coverage: listed points the pack lacks or never detects\n\n");
    md.push_str(
        "Unmatched names include spelling differences as well as real gaps — read before adding an entry.\n\n",
    );
    for source in ["edewakaru", "jlptgrammarlist"] {
        let mut table: BTreeMap<String, [usize; 3]> = BTreeMap::new();
        let mut missing: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut silent: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (row, (ids, _)) in rows.iter().zip(&resolved) {
            if row.source != source || row.list != "level" {
                continue;
            }
            let counts = table.entry(row.level.clone()).or_default();
            counts[0] += 1;
            let label = if row.sense.is_empty() {
                row.name.clone()
            } else {
                format!("{} {}", row.name, row.sense)
            };
            let detail = if row.note.is_empty() {
                row.url.clone()
            } else {
                row.note.clone()
            };
            if ids.is_empty() {
                missing
                    .entry(row.level.clone())
                    .or_default()
                    .push(format!("{label} — {detail}"));
            } else if ids.iter().any(|id| corpus.fires(id) > 0) {
                counts[1] += 1;
                counts[2] += 1;
            } else {
                counts[1] += 1;
                let list: Vec<String> = ids.iter().cloned().collect();
                silent
                    .entry(row.level.clone())
                    .or_default()
                    .push(format!("{label} → `{}`", list.join("`, `")));
            }
        }
        let _ = writeln!(md, "## {source}\n\n| level | listed | in the pack | fires on the corpus |\n|---|---:|---:|---:|");
        let mut totals = [0usize; 3];
        for (level, [listed, have, fire]) in &table {
            let _ = writeln!(md, "| {level} | {listed} | {have} | {fire} |");
            totals[0] += listed;
            totals[1] += have;
            totals[2] += fire;
        }
        let _ = writeln!(
            md,
            "| **all** | **{}** | **{}** | **{}** |\n",
            totals[0], totals[1], totals[2]
        );
        println!(
            "coverage {source}: {} listed, {} in the pack, {} fire on the corpus",
            totals[0], totals[1], totals[2]
        );
        let _ = writeln!(md, "### In the pack, never fires on the corpus\n");
        for (level, list) in &silent {
            let _ = writeln!(md, "**{level}** — {}\n", list.join("; "));
        }
        let _ = writeln!(md, "### Not matched to any pack entry\n");
        for (level, list) in &missing {
            let _ = writeln!(md, "**{level}** ({})\n", list.len());
            for item in list {
                let _ = writeln!(md, "- {item}");
            }
            md.push('\n');
        }
    }
    let unseen = entries.keys().filter(|id| !seen.contains_key(**id)).count();
    let _ = writeln!(
        md,
        "Pack entries neither list names: {unseen} of {}.",
        entries.len()
    );
    std::fs::write(dir.join("coverage.md"), &md).expect("write coverage");

    // ── 3. Casual forms ──────────────────────────────────────────────────────────────────────
    // A contraction is explained if the grammar panel names it — by an entry for the contraction
    // itself, or by the entry for the form it contracts. Which is why the full form matters: the
    // built-in analyzer keeps 握っちゃう whole but reports it as the plain non-past of 握る, so the
    // 〜てしまう rule firing is the only place a reader learns what ちゃう adds.
    let mut md = String::from("# Casual forms (edewakaru 口語形)\n\n");
    md.push_str(
        "*Candidates* are corpus sentences containing the form as text — a screen, not a gold set: \
         とこ also matches 所 in prose and だい also matches 大. *Explained* counts those where the \
         matcher fires an entry for the form or for its full form. *One word* counts those where the \
         form sits inside a longer token, held to the word before it. One-character forms are too \
         short to screen by text and are listed without counts.\n\n\
         | form | full form | listed as | entries | candidates | explained | one word |\n\
         |---|---|---|---|---:|---:|---:|\n",
    );
    let mut details = String::new();
    let mut unexplained = 0usize;
    for row in rows.iter().filter(|r| r.list == "casual") {
        for form in spellings(&row.name) {
            let full = expansion(&form, &row.name);
            let mut ids = index.resolve(&analyzer, &form).0;
            if let Some(full) = full {
                ids.extend(index.resolve(&analyzer, full).0);
            }
            let entry_list = if ids.is_empty() {
                "—".to_owned()
            } else {
                ids.iter()
                    .map(|id| format!("`{id}`"))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let full_label = full.unwrap_or("several");
            if form.chars().count() < 2 {
                let _ = writeln!(
                    md,
                    "| {form} | {full_label} | {} | {entry_list} | — | — | — |",
                    row.name
                );
                continue;
            }
            // でしょ is inside every でしょう: where the full form merely extends the contraction,
            // a sentence holding the full form is evidence for the full form, not the contraction.
            let extends = full.filter(|full| full.starts_with(form.as_str()));
            let candidates: Vec<usize> = corpus
                .sentences
                .iter()
                .enumerate()
                .filter(|(_, (japanese, _))| {
                    japanese.contains(&form) && extends.is_none_or(|full| !japanese.contains(full))
                })
                .map(|(i, _)| i)
                .collect();
            let explained: Vec<usize> = candidates
                .iter()
                .copied()
                .filter(|i| corpus.matches[*i].iter().any(|(id, _, _)| ids.contains(id)))
                .collect();
            let held = candidates
                .iter()
                .filter(|i| {
                    corpus.tokens[**i]
                        .iter()
                        .any(|t| t.contains(&form) && t.chars().count() > form.chars().count())
                })
                .count();
            if !candidates.is_empty() && explained.is_empty() {
                unexplained += 1;
            }
            let _ = writeln!(
                md,
                "| {form} | {full_label} | {} | {entry_list} | {} | {} | {held} |",
                row.name,
                candidates.len(),
                explained.len()
            );
            let _ = writeln!(
                details,
                "### {form} ({})\n\nExplained:\n{}Not explained:\n{}",
                row.name,
                sample(&corpus, explained.iter().copied()),
                sample(
                    &corpus,
                    candidates
                        .iter()
                        .copied()
                        .filter(|i| !explained.contains(i))
                )
            );
        }
    }
    md.push_str("\n## Samples\n\n");
    md.push_str(&details);
    std::fs::write(dir.join("casual.md"), &md).expect("write casual");
    println!("casual forms: {unexplained} with candidates and none explained");

    // ── 4. Confusable pairs ──────────────────────────────────────────────────────────────────
    // Per member rather than per pair: two members are spelled differently, so their rules almost
    // never fire on the same words, and "no competing explanations" was true and told nothing.
    // What a pair needs is both members present and both found where they occur.
    let mut md = String::from("# Confusable pairs (edewakaru 類似文法比較)\n\n");
    md.push_str(
        "Each member: the pack entries it resolves to, the corpus sentences containing it as text, and \
         how many of those any of its entries fires on. *Same span* counts sentences where two members' \
         rules fire on overlapping tokens.\n\n",
    );
    let mut absent = 0usize;
    let mut silent = 0usize;
    for row in rows.iter().filter(|r| r.list == "compare") {
        let members: Vec<String> = if row.name.contains(" vs ") {
            row.name.split(" vs ").map(str::to_owned).collect()
        } else {
            row.note
                .split('／')
                .filter(|m| !m.is_empty())
                .map(str::to_owned)
                .collect()
        };
        if members.len() < 2 {
            continue;
        }
        let ids: Vec<BTreeSet<String>> = members
            .iter()
            .map(|m| index.resolve(&analyzer, m).0)
            .collect();
        let mut same_span = 0usize;
        for found in &corpus.matches {
            let hits: Vec<Vec<&(String, usize, usize)>> = ids
                .iter()
                .map(|set| found.iter().filter(|(id, _, _)| set.contains(id)).collect())
                .collect();
            let overlap = hits.iter().enumerate().any(|(a, ha)| {
                hits.iter().enumerate().any(|(b, hb)| {
                    a < b
                        && ha
                            .iter()
                            .any(|x| hb.iter().any(|y| x.0 != y.0 && x.1 < y.2 && y.1 < x.2))
                })
            });
            same_span += usize::from(overlap);
        }
        let _ = writeln!(
            md,
            "## {}\n\nsame span: {same_span} · {}\n\n| member | entries | candidates | fires |\n|---|---|---:|---:|",
            members.join(" / "),
            row.url
        );
        for (member, set) in members.iter().zip(&ids) {
            let surface = spellings(member).into_iter().next().unwrap_or_default();
            let candidates: Vec<usize> = if surface.chars().count() < 2 {
                Vec::new()
            } else {
                corpus
                    .sentences
                    .iter()
                    .enumerate()
                    .filter(|(_, (japanese, _))| japanese.contains(&surface))
                    .map(|(i, _)| i)
                    .collect()
            };
            let fires = candidates
                .iter()
                .filter(|i| {
                    corpus.matches[**i]
                        .iter()
                        .any(|(id, _, _)| set.contains(id))
                })
                .count();
            if set.is_empty() {
                absent += 1;
            } else if !candidates.is_empty() && fires == 0 {
                silent += 1;
            }
            let entries = if set.is_empty() {
                "—".to_owned()
            } else {
                set.iter()
                    .map(|id| format!("`{id}`"))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let _ = writeln!(
                md,
                "| {member} | {entries} | {} | {fires} |",
                candidates.len()
            );
        }
        md.push('\n');
    }
    std::fs::write(dir.join("confusable.md"), &md).expect("write confusable");
    println!(
        "confusable pairs: {absent} members missing from the pack, {silent} present but never firing where they occur"
    );
    println!("reports: {}", dir.display());
}

#[test]
fn a_listed_name_reduces_to_what_both_lists_write_alike() {
    // Labels, not sentences: names as each list prints them.
    assert_eq!(spellings("〜だけ①（VれるだけV）"), vec!["だけ"]);
    assert_eq!(spellings("〜ちゃう・〜じゃう"), vec!["ちゃう", "じゃう"]);
    assert_eq!(
        spellings("Noun を皮切りに / を皮切りにして"),
        vec!["を皮切りに", "を皮切りにして"]
    );
    assert_eq!(spellings("Verb ようにする"), vec!["ようにする"]);
    assert_eq!(spellings("〜ん（〜てんの）"), vec!["ん"]);
    assert_eq!(hiragana("ナカ"), "なか");
}

#[test]
fn a_level_range_holds_the_levels_between_its_ends() {
    assert_eq!(within("N3", "N3-N2"), Some(true));
    assert_eq!(within("N3", "N2-N1"), Some(false));
    assert_eq!(within("N4", "N4"), Some(true));
    assert_eq!(within("N4", "none"), None);
    let ranges: BTreeSet<String> = ["N2-N1".to_owned()].into();
    assert_eq!(gap("N4", &ranges), 2);
}
