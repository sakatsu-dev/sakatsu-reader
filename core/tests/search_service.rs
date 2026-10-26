//! Search behavior at the core boundary, with real indexes and no installed resources.
use sakatsu_core::analyzer::LinderaAnalyzer;
use sakatsu_core::dict::frequency::{Direction, Frequencies};
use sakatsu_core::dict::{Dictionary, KanjiDic};
use sakatsu_core::search::{search, Resources};
use std::io::Write;

#[path = "common/scratch.rs"]
mod scratch_guard;

#[test]
fn search_keeps_caps_enrichment_and_one_dictionary_generation() {
    let root = scratch_guard::Scratch::new("search-service", "response");
    let archive = root.join("fixture.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    let mut rows: Vec<_> = (0..21)
        .map(|i| {
            serde_json::json!([
                format!("word{i}"),
                "reading",
                "n",
                "",
                20,
                ["shared gloss"],
                100 + i,
                ""
            ])
        })
        .collect();
    rows.push(serde_json::json!([
        "\u{5e02}\u{5834}",
        "\u{3057}\u{3058}\u{3087}\u{3046}",
        "n",
        "",
        50,
        ["market"],
        1,
        ""
    ]));
    for (name, value) in [
        (
            "index.json",
            serde_json::json!({"title":"Search fixture", "format":3}),
        ),
        ("term_bank_1.json", serde_json::json!(rows)),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(value.to_string().as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let path = root.join("dictionary");
    drop(Dictionary::import(&archive, &path).unwrap());
    // Model a release imported before gloss indexing, before acquiring any reader.
    let generation = std::fs::read_to_string(path.join("current")).unwrap();
    let gloss = path
        .join(generation.trim())
        .join("gloss")
        .canonicalize()
        .unwrap();
    assert!(gloss.starts_with(root.canonicalize().unwrap()));
    std::fs::remove_dir_all(gloss).unwrap();
    let held = [Dictionary::open(&path).unwrap()];
    let analyzer = LinderaAnalyzer::new().unwrap();
    let kanji_path = root.join("kanjidic.tsv");
    std::fs::write(
        &kanji_path,
        "\u{5e02}\t\u{30b7}\t\u{3044}\u{3061}\tmarket\t5\t2\n",
    )
    .unwrap();
    let kanji = KanjiDic::load(&kanji_path);
    let run = |query, dictionaries, frequency| {
        search(
            query,
            Resources {
                preferred: &analyzer,
                built_in: &analyzer,
                dictionaries,
                kanji: &kanji,
                frequency,
            },
        )
        .unwrap()
    };
    let absent = run("shared", &held, None);
    assert!(absent.needs_gloss_index);
    assert!(absent.words.is_empty());
    held[0].index_secondary().unwrap();
    let current = [Dictionary::open(&path).unwrap()];
    assert!(
        run("shared", &held, None).needs_gloss_index,
        "the held generation remains internally consistent"
    );
    let mut frequency = Frequencies::new("Test corpus", Direction::Rank);
    frequency.insert("word19", None, 1.0);
    let found = run("shared", &current, Some(&frequency));
    assert_eq!(found.words.len(), 20);
    assert!(found.capped);
    assert_eq!(found.ranked_by.as_deref(), Some("Test corpus"));
    assert_eq!(found.words[0].word.headword, "word19");
    assert!(found
        .words
        .iter()
        .all(|word| word.ranking.is_none() && word.surface.is_none()));
    let japanese = run("\u{5e02}\u{5834}", &current, Some(&frequency));
    assert!(japanese.japanese);
    assert!(!japanese.capped);
    assert!(japanese.ranked_by.is_none());
    assert_eq!(japanese.words.len(), 1);
    assert_eq!(
        japanese.words[0].surface.as_deref(),
        Some("\u{5e02}\u{5834}")
    );
    assert_eq!(japanese.words[0].chars.len(), 1);
    assert!(run(" ", &current, None).words.is_empty());
}
