//! Tests against a **real** dictionary.
//!
//! Ignored by default: they need `scripts/fetch-dictionaries.ps1` to have run, and CI
//! has no dictionaries. Run them after touching the Yomitan parser:
//!
//! ```text
//! cargo test -p sakatsu-core --test jitendex_live -- --ignored --nocapture
//! ```
//!
//! These exist because hand-built fixtures agree with whatever the author believed.
//! Real Jitendex uses labels no documentation mentioned — `sense-group`,
//! `part-of-speech-info`, `redirect-glossary` — and the parser was wrong about two of
//! them until it met them here.

use std::path::PathBuf;

use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer};
use sakatsu_core::dict::{merge, yomitan, Dictionary, MatchKind, SourceResult, TagGlossary};

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

fn jitendex() -> PathBuf {
    dictionaries_dir().join("jitendex-yomitan.zip")
}

/// Find one term's raw entry by scanning the banks.
fn raw_entry(archive_path: &PathBuf, term: &str) -> serde_json::Value {
    let file = std::fs::File::open(archive_path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}\nrun scripts/fetch-dictionaries.ps1 first",
            archive_path.display()
        )
    });
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).expect("open zip");

    let mut banks: Vec<String> = archive
        .file_names()
        .filter_map(Result::ok)
        .filter(|name| name.contains("term_bank_"))
        .map(|name| name.into_owned())
        .collect();
    banks.sort();

    for name in banks {
        let json = {
            use std::io::Read;
            let mut member = archive.by_name(&name).expect("member");
            let mut json = String::new();
            member.read_to_string(&mut json).expect("read");
            json
        };
        let rows: Vec<serde_json::Value> = serde_json::from_str(&json).expect("bank json");
        for row in rows {
            if row.get(0).and_then(|value| value.as_str()) == Some(term) {
                return row;
            }
        }
    }
    panic!("{term} not found in {}", archive_path.display());
}

/// The entry that taught the parser three things at once. Every assertion here failed
/// before real Jitendex was read: the glosses came back as one run-on line, the part
/// of speech was missing entirely, and the example translations carried footnote
/// markers.
/// Hand-built fixtures answer the whole token; the component fallback lives in `lookup::sources`.
fn whole() -> sakatsu_core::dict::Retrieved {
    sakatsu_core::dict::Retrieved::Whole {
        query: String::new(),
    }
}

#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn a_real_entry_parses_into_clean_senses() {
    let raw = raw_entry(&jitendex(), "危ない");
    let entry = yomitan::parse_term(&raw).expect("parses");

    assert_eq!(entry.term, "危ない");
    assert_eq!(entry.reading, "あぶない");
    assert_eq!(entry.rules, ["adj-i"]);

    // Each gloss is its own meaning, not a word in one long line.
    let first = &entry.definitions[0];
    assert!(
        first.glosses.len() >= 4,
        "expected several separate glosses, got {:?}",
        first.glosses
    );
    assert!(first.glosses.contains(&"dangerous".to_string()));
    assert!(
        first
            .glosses
            .iter()
            .all(|gloss| gloss.split(' ').count() < 6),
        "a gloss this long means the list items were run together: {:?}",
        first.glosses
    );

    // The part of speech is hung on the enclosing sense-group.
    assert!(
        first.tags.contains(&"adjective".to_string()),
        "tags: {:?}",
        first.tags
    );

    // A later sense-group carries a different part of speech, which must not have
    // been overwritten by, or bled into, the first.
    assert!(
        entry
            .definitions
            .iter()
            .any(|definition| definition.tags.contains(&"interjection".to_string())),
        "the 危ない! sense should be an interjection"
    );

    // Examples: furigana dropped, footnote markers dropped, nothing from the gloss.
    let example = entry
        .definitions
        .iter()
        .flat_map(|definition| definition.examples.iter())
        .next()
        .expect("this entry has example sentences");
    assert!(!example.japanese.is_empty());
    assert!(
        !example.english.contains('['),
        "a footnote marker leaked into the translation: {:?}",
        example.english
    );
    for definition in &entry.definitions {
        for gloss in &definition.glosses {
            assert!(
                !gloss.contains('。'),
                "an example sentence leaked into a gloss: {gloss:?}"
            );
        }
    }
}

/// Import the whole thing and look words up in it.
///
/// The index is built under `<dictionaries>/jitendex` and left there — importing
/// 400,000 entries takes long enough that the next run should reuse it. Delete that
/// directory to force a fresh import.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1; imports the full dictionary"]
fn the_whole_dictionary_imports_and_answers_lookups() {
    let out = dictionaries_dir().join("jitendex");
    let dictionary = if sakatsu_core::dict::index::Dictionary::exists_in(&out) {
        println!("reusing the existing index at {}", out.display());
        Dictionary::open(&out).expect("open")
    } else {
        let started = std::time::Instant::now();
        let dictionary = Dictionary::import(&jitendex(), &out).expect("import");
        println!(
            "imported {} entries in {:.1}s",
            dictionary.meta().entry_count,
            started.elapsed().as_secs_f64()
        );
        dictionary
    };

    let meta = dictionary.meta();
    println!("title      = {}", meta.title);
    println!("entries    = {}", meta.entry_count);
    println!(
        "attribution= {}",
        meta.attribution
            .as_deref()
            .unwrap_or("(none)")
            .lines()
            .next()
            .unwrap_or("")
    );

    assert!(meta.title.contains("Jitendex"));
    assert!(
        meta.entry_count > 100_000,
        "Jitendex should be large, got {}",
        meta.entry_count
    );
    // CC BY-SA 4.0 requires attribution, so losing this would be a licence problem,
    // not just a missing field (project notes §8).
    assert!(
        meta.attribution
            .as_deref()
            .is_some_and(|text| text.contains("CC BY-SA")),
        "attribution must survive import: {:?}",
        meta.attribution
    );

    // A word looked up by its spelling.
    let hits = dictionary.lookup("食べる").expect("lookup");
    assert!(!hits.is_empty(), "食べる should be in Jitendex");
    assert_eq!(hits[0].matched, MatchKind::Spelling);
    let glosses: Vec<&str> = hits[0].entry.definitions[0]
        .glosses
        .iter()
        .map(String::as_str)
        .collect();
    println!("食べる    = {glosses:?}");
    assert!(glosses.iter().any(|gloss| gloss.contains("eat")));

    // The homophone case, on real data: が is read the same as 蛾, 我 and 雅.
    let ga = dictionary.lookup("が").expect("lookup");
    assert!(!ga.is_empty());
    assert!(
        ga.iter().all(|hit| hit.entry.term == "が"),
        "a spelling lookup must not return homophones: {:?}",
        ga.iter()
            .map(|hit| hit.entry.term.as_str())
            .collect::<Vec<_>>()
    );
    println!("が hits    = {} (all spelled が)", ga.len());

    // A reading-only query falls back to the reading bucket.
    let tabe = dictionary.lookup("たべる").expect("lookup");
    assert!(!tabe.is_empty(), "たべる should resolve by reading");
    assert!(tabe.iter().all(|hit| hit.matched == MatchKind::Reading));
    println!("たべる    = {} hits by reading", tabe.len());

    let missing = dictionary.lookup("ヷヷヷヷ").expect("lookup");
    assert!(missing.is_empty(), "a miss is empty, not an error");
}

/// English search, on the dictionary a learner actually has.
///
/// This is the test the ranking was designed against. Every rule in `GlossRank` was
/// arrived at by running the alternatives over these 433,885 entries and *reading the
/// answers* — the first version put ブック above 本, マウンテン above 山 and スクール
/// above 学校 for every query, because JMdict's low sequence numbers are the katakana
/// block and the tiebreak was file order. No hand-built fixture would have shown that.
///
/// What it asserts is deliberately weak: that the everyday word is **among** the first
/// few, not that it is first. Jitendex's score saturates at 200 for 22,553 entries and
/// carries no frequency list, so 食べる, 喫する and 食う are genuinely indistinguishable
/// to it. Asserting an exact order would be pinning an accident of file layout.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1; builds the gloss index"]
fn english_search_finds_the_everyday_word_on_the_real_dictionary() {
    let out = dictionaries_dir().join("jitendex");
    if !sakatsu_core::dict::index::Dictionary::exists_in(&out) {
        println!("SKIP: no imported dictionary at {}", out.display());
        return;
    }
    let dictionary = Dictionary::open(&out).expect("open");

    if !dictionary.has_glosses() {
        let started = std::time::Instant::now();
        let count = dictionary.index_secondary().expect("index glosses");
        println!(
            "indexed {count} entries in {:.1}s (from terms.ndjson — no re-import)",
            started.elapsed().as_secs_f64()
        );
    }
    assert!(dictionary.has_glosses());

    // The index's own files live in the generation the reader resolved, not under `out`.
    let bucket = dictionary.root().join("gloss");
    let shards: Vec<_> = std::fs::read_dir(&bucket)
        .expect("gloss bucket")
        .filter_map(Result::ok)
        .collect();
    let bytes: u64 = shards
        .iter()
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum();
    println!(
        "gloss bucket: {} shards, {:.1} MB",
        shards.len(),
        bytes as f64 / (1024.0 * 1024.0)
    );

    // The word a learner wants, and how far down it sits. Printed either way, so a
    // regression in the ranking is visible in the output before it trips an assertion.
    let expected: &[(&str, &str)] = &[
        ("eat", "食べる"),
        ("water", "水"),
        ("cat", "猫"),
        ("dog", "犬"),
        ("book", "本"),
        ("mountain", "山"),
        ("river", "川"),
        ("teacher", "先生"),
        ("school", "学校"),
        ("beautiful", "美しい"),
        ("buy", "買う"),
        ("hand", "手"),
    ];

    let mut misses = Vec::new();
    for (query, wanted) in expected {
        let hits = dictionary.search_glosses(query, 12).expect("search");
        let terms: Vec<&str> = hits.iter().map(|hit| hit.entry.term.as_str()).collect();
        let at = terms.iter().position(|term| term == wanted);
        println!(
            "  {query:<10} {:<4} {}",
            match at {
                Some(index) => format!("#{}", index + 1),
                None => "—".to_string(),
            },
            terms.iter().take(6).cloned().collect::<Vec<_>>().join(" ")
        );
        match at {
            Some(index) if index < 6 => {}
            _ => misses.push(*query),
        }
    }

    assert!(
        misses.is_empty(),
        "the everyday word fell out of the first six for: {misses:?}"
    );

    // Every word of the query has to match, or an English search is a thesaurus of
    // whatever shares a preposition with it.
    let both = dictionary
        .search_glosses("mountain river", 10)
        .expect("search");
    println!("'mountain river' = {} hits", both.len());

    assert!(dictionary
        .search_glosses("qwertyuiop", 10)
        .expect("search")
        .is_empty());
}

/// Reading alignment over every kanji-bearing word in the real dictionary.
///
/// # What the number to watch is
///
/// **Not the alignment rate.** A high rate would be easy to reach by loosening the sound
/// changes until everything matches something, and that would be worse than useless — it
/// would mark wrong readings confidently. The rate is reported; what is *asserted* is that
/// the words it aligns are aligned correctly, checked against a list written by hand.
///
/// The abstentions are the interesting output. They should be irregular words —
/// 熟字訓 like 明日 and 大人, names, and readings the characters genuinely do not build.
/// A sample is printed so that can be judged rather than assumed.
#[test]
#[ignore = "needs scripts/fetch-kanjidic.ps1 and an imported dictionary"]
fn reading_alignment_holds_up_across_the_whole_dictionary() {
    let table = kanji_table();
    if table.is_empty() {
        println!("SKIP: no kanji table — run scripts/fetch-kanjidic.ps1");
        return;
    }
    let out = dictionaries_dir().join("jitendex");
    if !sakatsu_core::dict::index::Dictionary::exists_in(&out) {
        println!("SKIP: no imported dictionary at {}", out.display());
        return;
    }
    println!("kanji table: {} characters", table.len());

    let dictionary = Dictionary::open(&out).expect("open");
    let entries = dictionary.entries().expect("entries");

    let mut considered = 0usize;
    let mut aligned = 0usize;
    let mut fully = 0usize;
    let mut abstained: Vec<(String, String)> = Vec::new();

    for entry in &entries {
        let has_kanji = entry
            .term
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
        if !has_kanji || entry.reading.is_empty() || entry.term == entry.reading {
            continue;
        }
        considered += 1;
        match sakatsu_core::dict::align(&entry.term, &entry.reading, &table) {
            Some(marks) => {
                aligned += 1;
                // Every kanji got a reading, rather than some being left open by
                // disagreement between two alignments that both worked.
                let open = entry
                    .term
                    .chars()
                    .zip(&marks)
                    .any(|(c, mark)| ('\u{4e00}'..='\u{9fff}').contains(&c) && mark.is_none());
                if !open {
                    fully += 1;
                }
            }
            None => {
                if abstained.len() < 4000 {
                    abstained.push((entry.term.clone(), entry.reading.clone()));
                }
            }
        }
    }

    let percent = |n: usize| (n as f64) * 100.0 / (considered.max(1) as f64);
    println!("\nwords with kanji and a reading: {considered}");
    println!(
        "  aligned            {aligned:>7}  ({:.1}%)",
        percent(aligned)
    );
    println!("    every character  {fully:>7}  ({:.1}%)", percent(fully));
    println!(
        "  abstained          {:>7}  ({:.1}%)",
        considered - aligned,
        percent(considered - aligned)
    );

    println!("\n  a sample of the abstentions — these should look irregular:");
    for (term, reading) in abstained
        .iter()
        .step_by(abstained.len().max(1) / 12 + 1)
        .take(12)
    {
        println!("    {term}  {reading}");
    }

    // Spot-checked by hand: the reading each character contributes, or `None` where the
    // word is one the characters do not build.
    let expected: &[(&str, &str, &[&str])] = &[
        ("図書館", "としょかん", &["と", "しょ", "かん"]),
        // **Sound changes are recorded as they sound.** 学 is ガク and contributes がっ here;
        // 間 is カン/ケン and contributes げん. The matcher reaches both through `variants`, and
        // it used to record the dictionary form it started from — so 人間 was set にんけん over
        // a word the panel beside it correctly called にんげん.
        ("学校", "がっこう", &["がっ", "こう"]),
        ("人間", "にんげん", &["にん", "げん"]),
        ("人々", "ひとびと", &["ひと", "びと"]),
        ("新聞", "しんぶん", &["しん", "ぶん"]),
        ("食べる", "たべる", &["た"]),
        ("先生", "せんせい", &["せん", "せい"]),
        ("時間", "じかん", &["じ", "かん"]),
        ("電車", "でんしゃ", &["でん", "しゃ"]),
        // Irregular: nothing here is built from the characters' own readings.
        ("明日", "あした", &[]),
        ("大人", "おとな", &[]),
        ("今日", "きょう", &[]),
    ];

    let mut wrong = Vec::new();
    for (term, reading, want) in expected {
        let got = sakatsu_core::dict::align(term, reading, &table);
        let marks: Vec<String> = got
            .iter()
            .flatten()
            .zip(term.chars())
            // 々 is U+3005, outside the CJK block — without it the repeat mark's own reading
            // is dropped from this list and 人々 looks like a one-character word.
            .filter(|(_, c)| ('\u{4e00}'..='\u{9fff}').contains(c) || *c == '々')
            .map(|(mark, _)| mark.clone().unwrap_or_else(|| "—".into()))
            .collect();
        println!("  {term:<8} {reading:<10} {marks:?}");
        if want.is_empty() {
            if got.is_some() {
                wrong.push(format!("{term} should not align, got {marks:?}"));
            }
        } else if marks != *want {
            wrong.push(format!("{term}: wanted {want:?}, got {marks:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "alignment is wrong for:\n  {}",
        wrong.join("\n  ")
    );
}

/// The table `scripts/fetch-kanjidic.ps1` writes.
fn kanji_table() -> sakatsu_core::dict::KanjiDic {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    match appdata {
        Some(dir) => sakatsu_core::dict::KanjiDic::load(
            &dir.join("com.sakatsu.app")
                .join("kanji")
                .join("kanjidic.tsv"),
        ),
        None => sakatsu_core::dict::KanjiDic::default(),
    }
}

/// Phase 1 and Phase 2 composing: a real Ichiran analysis, looked up in a real
/// dictionary, through the real merge policy.
#[test]
#[ignore = "needs ichiran-cli and the imported dictionary"]
fn the_merge_policy_holds_on_real_data() {
    let analysis = IchiranAnalyzer::new().analyze("危ない").expect("analyze");
    let token = &analysis.tokens[0];

    let dictionary = Dictionary::open(&dictionaries_dir().join("jitendex"))
        .expect("run the_whole_dictionary_imports_and_answers_lookups first");
    let hits = dictionary.lookup(&token.dict_form).expect("lookup");
    assert!(!hits.is_empty(), "Jitendex has 危ない");

    let view = merge(
        token,
        &[SourceResult {
            retrieved: whole(),
            title: dictionary.meta().title.clone(),
            hits,
        }],
    );

    // The analyzer is primary: it shows inline, and needs no attribution line
    // because it is the source the reader and grammar view already agree with.
    assert!(!view.inline.is_empty(), "the analyzer had senses");
    assert_eq!(view.inline_source, None);
    assert_eq!(view.reading.as_deref(), Some("あぶない"));

    // Jitendex is disclosed quietly rather than duplicated inline.
    assert_eq!(view.also_in.len(), 1);
    assert!(view.also_in[0].title.contains("Jitendex"));

    // Examples come from Jitendex but belong to the word.
    assert!(
        !view.examples.is_empty(),
        "Jitendex ships example sentences for 危ない"
    );
    assert!(!view.examples[0].japanese.contains('['));

    // And the analyzer's own POS codes are explainable.
    let glossary = TagGlossary::jmdict();
    let codes: Vec<String> = view.inline[0].tags.clone();
    assert!(!codes.is_empty(), "the analyzer tagged the sense");
    println!("inline tags = {codes:?}");
    for code in &codes {
        println!("  {code} = {:?}", glossary.explain(code));
    }
    assert!(
        codes.iter().any(|code| glossary.explain(code).is_some()),
        "at least one code should be explainable: {codes:?}"
    );
}

/// JMnedict is the case the separate name answer exists for — JMdict deliberately
/// omits names — and the only source that defines codes like `surname`.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1; imports JMnedict"]
fn a_name_is_separated_from_senses_and_its_tag_codes_are_explained() {
    let out = dictionaries_dir().join("jmnedict");
    let dictionary = if sakatsu_core::dict::index::Dictionary::exists_in(&out) {
        Dictionary::open(&out).expect("open")
    } else {
        let zip = dictionaries_dir().join("JMnedict.zip");
        let started = std::time::Instant::now();
        let dictionary = Dictionary::import(&zip, &out).expect("import");
        println!(
            "imported {} JMnedict entries in {:.1}s",
            dictionary.meta().entry_count,
            started.elapsed().as_secs_f64()
        );
        dictionary
    };
    println!("title = {}", dictionary.meta().title);

    // A token the analyzer could not gloss: exactly what promotion is for.
    let token = sakatsu_core::model::Token {
        surface: "田中".into(),
        dict_form: "田中".into(),
        ..Default::default()
    };
    let hits = dictionary.lookup("田中").expect("lookup");
    assert!(!hits.is_empty(), "JMnedict has 田中");

    let view = merge(
        &token,
        &[SourceResult {
            retrieved: whole(),
            title: dictionary.meta().title.clone(),
            hits,
        }],
    );

    // A name is an answer, but not a word sense: "who is this?" and "what does this
    // word mean?" are different questions. It is kept out of both the inline sense list
    // and the ordinary source disclosure, then credited in its own answer.
    assert!(
        view.inline.is_empty(),
        "a name must not be presented as a meaning"
    );
    assert!(view.inline_source.is_none());
    assert!(
        view.also_in.is_empty(),
        "the name must not also be disclosed as a sense"
    );
    let name = view.name.as_ref().expect("JMnedict supplies a name answer");
    assert_eq!(name.source, dictionary.meta().title);
    assert!(name.leads, "with no word senses, the name is the answer");
    assert!(name.tags.iter().any(|tag| tag == "surname"));
    assert!(name.glosses.iter().any(|reading| reading == "たなか"));
    println!("田中 name readings = {:?}", name.glosses);
    println!("田中 reading = {:?}", view.reading);

    // The defect this test used to record, now fixed.
    //
    // JMnedict-in-Yomitan is a *reading* dictionary for most name entries: it leaves
    // the reading field empty and puts the kana in the **glossary**. Taken at face
    // value 田中 came back as a word whose meaning was "たなか", with the actually
    // useful reading nowhere the UI looked.
    //
    // Those entries are now recognised and their kana treated as readings.
    // The kana lives among the name readings, not among `inline` meanings.
    assert!(view
        .inline
        .iter()
        .all(|sense| !sense.glosses.contains(&"たなか".into())));
    // 田中 is read たなか, でんちゅう *and* ティエンジョン depending on which entry is
    // asked, so this source has not settled on one. Withholding is correct: in the app
    // the analyzer supplies the reading for a word met in a sentence, and this is only
    // consulted when it had none.
    assert_eq!(
        view.reading, None,
        "a source that disagrees supplies no reading"
    );

    // Its own tag bank explains its own codes; the built-in JMdict table cannot.
    // Required, and read from the resolved generation: looked for under `out`, this check
    // found nothing on any install published as a generation and passed without running.
    let tags_path = dictionary.root().join("tags.json");
    assert!(
        tags_path.exists(),
        "JMnedict's tag bank was not imported: {}",
        tags_path.display()
    );
    {
        let own: std::collections::HashMap<String, sakatsu_core::dict::TagInfo> =
            serde_json::from_str(&std::fs::read_to_string(&tags_path).expect("read tags"))
                .expect("parse tags");
        let mut glossary = TagGlossary::jmdict();
        assert!(
            glossary.explain("surname").is_none(),
            "the JMdict table should not know JMnedict's codes"
        );
        glossary.merge(&own);
        println!("surname = {:?}", glossary.explain("surname"));
        assert!(
            glossary.explain("surname").is_some(),
            "JMnedict's own bank must supply it"
        );
        // Merging must not clobber the built-in JMdict meanings.
        assert_eq!(glossary.explain("vt"), Some("transitive"));
    }
}

/// A cross-reference entry: Jitendex keeps `redirect-glossary` at the entry level,
/// outside any sense, so these have no senses at all. They must still yield their
/// text rather than parsing to nothing.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn a_real_entry_with_no_senses_still_says_something() {
    let raw = raw_entry(&jitendex(), "危い");
    let entry = yomitan::parse_term(&raw).expect("parses");
    assert!(
        !entry.definitions.is_empty(),
        "a redirect entry should still have text"
    );
    let text = entry.definitions[0].glosses.join(" ");
    assert!(!text.trim().is_empty(), "empty redirect text");
}
