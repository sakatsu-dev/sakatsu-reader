//! Names, against the **real** installed dictionaries.
//!
//! Ignored by default: needs `scripts/fetch-dictionaries.ps1` to have imported both
//! Jitendex and JMnedict, which CI does not.
//!
//! ```text
//! cargo test -p sakatsu-core --test names_live -- --ignored --nocapture
//! ```
//!
//! This exists because the unit tests in `dict::merge` build their own sources, so they
//! prove the *policy* and nothing about whether the data supports it. The claim that
//! actually matters — JMnedict files セイア under the hiragana せいあ, so a katakana
//! surface only reaches it once the lookup folds — can only be checked against the real
//! 667,800-entry dictionary. It was measured before the code was written; this keeps it
//! measured.

use std::path::PathBuf;

use sakatsu_core::dict::{look_up_name, merge, merge_names, Dictionary, MatchKind, SourceResult};
use sakatsu_core::model::Token;

fn dictionaries_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_DICT_DIR") {
        return PathBuf::from(dir);
    }
    let appdata = std::env::var_os("APPDATA").expect("APPDATA (or set SAKATSU_DICT_DIR)");
    PathBuf::from(appdata)
        .join("com.sakatsu.app")
        .join("dictionaries")
}

/// Both dictionaries, in the priority order `discover_dictionaries` uses: a real
/// dictionary before a name dictionary.
fn opened() -> Option<Vec<(String, Dictionary)>> {
    let dir = dictionaries_dir();
    let mut out = Vec::new();
    for name in ["jitendex", "jmnedict"] {
        let path = dir.join(name);
        if !sakatsu_core::dict::index::Dictionary::exists_in(&path) {
            eprintln!("SKIP: no {name} at {}", path.display());
            return None;
        }
        let dictionary = Dictionary::open(&path).expect("open");
        out.push((dictionary.meta().title.clone(), dictionary));
    }
    Some(out)
}

fn look_up(dictionaries: &[(String, Dictionary)], query: &str) -> Vec<SourceResult> {
    dictionaries
        .iter()
        .filter_map(|(title, dictionary)| {
            let hits = dictionary.lookup(query).expect("lookup");
            (!hits.is_empty()).then(|| SourceResult {
                title: title.clone(),
                hits,
                // This helper queries a string directly, with no token and therefore no
                // components — so the answer is about the whole of what was asked, always.
                retrieved: sakatsu_core::dict::Retrieved::Whole {
                    query: query.to_string(),
                },
            })
        })
        .collect()
}

/// The token Ichiran actually produces for セイア, measured by running it:
/// `["seia", {"text":"セイア","score":9,"seq":2639530,"gloss":[{"gloss":"frog in a well…"}]}]`
fn seia_token() -> Token {
    Token {
        surface: "セイア".into(),
        dict_form: "セイア".into(),
        reading: "セイア".into(),
        lemma_reading: "せいあ".into(),
        senses: vec![sakatsu_core::model::Sense {
            pos: "[n]".into(),
            gloss: "frog in a well; someone with a narrow perspective".into(),
            sequence: None,
        }],
        confidence: Some(9),
        romaji: Some("seia".into()),
        ..Default::default()
    }
}

#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn a_katakana_name_reaches_jmnedict_only_by_folding() {
    let Some(dictionaries) = opened() else { return };
    let (_, jmnedict) = &dictionaries[1];

    // The fact the whole change rests on: the name is filed under hiragana, so the
    // katakana surface matches neither exact bucket.
    let hits = jmnedict.lookup("セイア").expect("lookup");
    assert!(!hits.is_empty(), "folding must find it");
    assert!(
        hits.iter().all(|hit| hit.matched == MatchKind::Folded),
        "セイア is not spelled or read that way in JMnedict — only folding reaches it"
    );

    let glosses: Vec<&str> = hits
        .iter()
        .flat_map(|hit| hit.entry.definitions.iter())
        .flat_map(|definition| definition.glosses.iter())
        .map(String::as_str)
        .collect();
    assert!(
        glosses.iter().any(|gloss| gloss.contains("Seia")),
        "expected the name, got {glosses:?}"
    );
}

#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn the_reported_case_leads_with_the_name_instead_of_frog_in_a_well() {
    let Some(dictionaries) = opened() else { return };

    let token = seia_token();
    let sources = look_up(&dictionaries, &token.dict_form);
    let view = merge(&token, &sources);

    let name = view.name.expect("JMnedict has せいあ");
    assert!(name.leads, "Ichiran scored this 9 — it does not believe it");
    assert!(
        name.glosses.iter().any(|gloss| gloss.contains("Seia")),
        "got {:?}",
        name.glosses
    );
    assert_eq!(
        name.tags,
        ["fem"],
        "JMnedict's own code, chipped like any other tag"
    );
    println!("セイア → {} ({})", name.glosses.join("; "), name.source);
}

/// **The regression guard, and it caught a real bug.**
///
/// バー scores 10 — the same low band as セイア — and JMnedict *does* file バー, as the
/// place name "Wah (Pakistan)". A rule guarded only by "a name dictionary has it" told the
/// reader that バー means Wah. What saves it is that バー is spelled バー: Jitendex matches
/// it exactly, so no fold is involved and the word stands.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn an_exactly_spelled_loanword_is_never_displaced_by_a_name() {
    let Some(dictionaries) = opened() else { return };

    let token = Token {
        surface: "バー".into(),
        dict_form: "バー".into(),
        senses: vec![sakatsu_core::model::Sense {
            pos: "[n]".into(),
            gloss: "bar; tavern".into(),
            sequence: None,
        }],
        confidence: Some(10),
        ..Default::default()
    };
    let view = merge(&token, &look_up(&dictionaries, &token.dict_form));

    // The name is real and is still offered — it just may not lead.
    if let Some(name) = &view.name {
        assert!(
            !name.leads,
            "バー is spelled バー; {:?} may not take the lead",
            name.glosses
        );
        println!("バー also lists: {}", name.glosses.join("; "));
    }
    assert!(!view.likely_name, "Jitendex has it exactly — not a guess");
    assert_eq!(view.inline[0].glosses, ["bar", "tavern"]);
}

/// アカリ is genuinely both 明かり and Akari, and Ichiran is sure of the word (36).
/// Both are shown, and the word keeps the lead.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn a_word_that_is_also_a_name_offers_both_without_reordering() {
    let Some(dictionaries) = opened() else { return };

    let token = Token {
        surface: "アカリ".into(),
        dict_form: "アカリ".into(),
        lemma_reading: "あかり".into(),
        senses: vec![sakatsu_core::model::Sense {
            pos: "[n]".into(),
            gloss: "light; glow; gleam".into(),
            sequence: None,
        }],
        confidence: Some(36),
        ..Default::default()
    };
    let view = merge(&token, &look_up(&dictionaries, &token.dict_form));

    let name = view.name.expect("JMnedict has あかり");
    assert!(!name.leads, "36 is a reading Ichiran is sure of");
    assert_eq!(view.inline[0].glosses, ["light", "glow", "gleam"]);
    println!("アカリ → light… + {}", name.glosses.join("; "));
}

/// シロコ is in no *name* dictionary, and the only thing that finds it at all is a fold
/// onto 白子 ("soft roe"). Not unknown, but not trustworthy either — so the gloss stays and
/// the caveat is added beside it.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn a_name_no_dictionary_knows_is_called_a_likely_name() {
    let Some(dictionaries) = opened() else { return };

    let token = Token {
        surface: "シロコ".into(),
        dict_form: "シロコ".into(),
        confidence: Some(9),
        romaji: Some("shiroko".into()),
        ..Default::default()
    };
    let sources = look_up(&dictionaries, &token.dict_form);
    // Whatever was found, none of it is spelled シロコ.
    assert!(
        sources.iter().all(|source| source
            .hits
            .iter()
            .all(|hit| hit.matched == MatchKind::Folded)),
        "nothing is actually spelled シロコ"
    );

    let view = merge(&token, &sources);
    assert!(view.name.is_none(), "no name dictionary has it");
    assert!(view.likely_name, "so the page must not assert a meaning");
    assert_eq!(view.romaji.as_deref(), Some("shiroko"));
    println!(
        "シロコ → likely a name (shiroko); folded gloss kept: {:?}",
        view.inline
    );
}

/// Every open dictionary, as `look_up_name` wants them.
fn just_dictionaries(opened: &[(String, Dictionary)]) -> Vec<&Dictionary> {
    opened.iter().map(|(_, dictionary)| dictionary).collect()
}

/// One shattered piece, with the score Ichiran gave it.
fn piece(surface: &str, reading: &str, confidence: i32) -> Token {
    Token {
        surface: surface.to_string(),
        dict_form: surface.to_string(),
        reading: reading.to_string(),
        lemma_reading: reading.to_string(),
        confidence: Some(confidence),
        ..Token::default()
    }
}

/// The reported bug, against the real dictionary: 善福寺川 read as ぜん・ふく・てら・かわ.
///
/// The four tokens and their scores are what Ichiran actually returned, so this drives the merge
/// with the input that failed rather than a plausible imitation of it.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn a_shattered_river_is_rejoined_from_the_real_dictionary() {
    let Some(opened) = opened() else { return };
    let dictionaries = just_dictionaries(&opened);

    // What JMnedict holds, before any policy runs.
    let found = look_up_name(dictionaries.iter().copied(), "善福寺川")
        .expect("JMnedict has 善福寺川 — it was read off disk before this test was written");
    assert_eq!(found.reading, "ぜんぷくじがわ");
    assert!(
        found.tags.iter().any(|tag| tag == "place"),
        "tagged as a place: {:?}",
        found.tags
    );

    // And the policy, on Ichiran's real output.
    let shattered = vec![
        piece("善", "ぜん", 21),
        piece("福", "ふく", 20),
        piece("寺", "てら", 24),
        piece("川", "かわ", 23),
    ];
    let merged = merge_names(&shattered, |surface| {
        look_up_name(dictionaries.iter().copied(), surface)
    });

    assert_eq!(merged.len(), 1, "four characters became one river");
    assert_eq!(merged[0].surface, "善福寺川");
    assert_eq!(merged[0].reading, "ぜんぷくじがわ");
    assert!(
        !merged[0].reading_unverified,
        "a dictionary vouched for this one"
    );
    println!(
        "善福寺川 → {} (was ぜん・ふく・てら・かわ)",
        merged[0].reading
    );
}

/// Two more rivers, shattered into *uneven* pieces, against the real dictionary.
///
/// Measured: 神田川 → 神(12) 田川(33), 多摩川 → 多摩(65) 川(23). Neither is made of single
/// characters, which is why the fragment test is a score and not a length. Both are in JMnedict.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn rivers_shattered_into_uneven_pieces_are_rejoined() {
    let Some(opened) = opened() else { return };
    let dictionaries = just_dictionaries(&opened);

    for (name, pieces, expected) in [
        (
            "神田川",
            vec![piece("神", "かみ", 12), piece("田川", "たがわ", 33)],
            "かんだがわ",
        ),
        (
            "多摩川",
            vec![piece("多摩", "たま", 65), piece("川", "かわ", 23)],
            "たまがわ",
        ),
    ] {
        let merged = merge_names(&pieces, |surface| {
            look_up_name(dictionaries.iter().copied(), surface)
        });
        assert_eq!(merged.len(), 1, "{name} should be one token");
        assert_eq!(merged[0].surface, name);
        assert_eq!(merged[0].reading, expected, "{name}");
        println!("{name} → {}", merged[0].reading);
    }
}

/// A run nothing could rejoin is caveated rather than presented as a reading.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn a_run_no_dictionary_knows_is_marked_unverified() {
    let Some(opened) = opened() else { return };
    let dictionaries = just_dictionaries(&opened);

    // Three characters that are each a word and together are nothing any dictionary files.
    let pieces = vec![
        piece("猫", "ねこ", 19),
        piece("窓", "まど", 20),
        piece("靴", "くつ", 21),
    ];
    let merged = merge_names(&pieces, |surface| {
        look_up_name(dictionaries.iter().copied(), surface)
    });

    if merged.len() == pieces.len() {
        assert!(
            merged.iter().all(|token| token.reading_unverified),
            "an unrejoinable run must say its reading is unconfirmed"
        );
        println!("猫窓靴 stayed apart and is marked unverified");
    } else {
        // If some dictionary really does file a join here, that is worth knowing about.
        println!(
            "joined into {:?} — a real entry exists for it",
            merged.iter().map(|t| t.surface.clone()).collect::<Vec<_>>()
        );
    }
}

/// The merge must not start joining things that were already right.
///
/// Run against the real 741,346-entry name dictionary, which is the only place a false positive
/// will show up — a hand-built fixture cannot contain the entry that surprises you.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn ordinary_sentences_survive_the_merge() {
    let Some(opened) = opened() else { return };
    let dictionaries = just_dictionaries(&opened);

    // Scores are Ichiran's for words it is sure of; see `NAME_WINS_BELOW`'s own measurements.
    let sentences: Vec<(&str, Vec<Token>)> = vec![
        (
            "東京都",
            vec![piece("東京", "とうきょう", 325), piece("都", "と", 30)],
        ),
        (
            "私はあいさつした",
            vec![
                piece("私", "わたし", 40),
                piece("は", "は", 4),
                piece("あいさつ", "あいさつ", 60),
            ],
        ),
        (
            "水を飲む",
            vec![
                piece("水", "みず", 32),
                piece("を", "を", 4),
                piece("飲む", "のむ", 50),
            ],
        ),
        // A hypothetical split — Ichiran actually returns 白子(208) as one token — kept because it
        // is the shape that would put a Chiba town where the soft roe was. JMnedict files 白子 as a
        // place and Jitendex has it as a word, so `look_up_name` must refuse the join.
        ("白子", vec![piece("白", "しろ", 12), piece("子", "こ", 11)]),
    ];

    for (what, tokens) in sentences {
        let before = tokens.len();
        let merged = merge_names(&tokens, |surface| {
            look_up_name(dictionaries.iter().copied(), surface)
        });
        let surfaces: Vec<String> = merged.iter().map(|t| t.surface.clone()).collect();
        if merged.len() != before {
            println!("{what}: joined into {surfaces:?} — worth a look");
        }
        assert_eq!(merged.len(), before, "{what}: joined into {surfaces:?}");
    }
}

/// Folding must not start answering questions that already had answers.
#[test]
#[ignore = "needs scripts/fetch-dictionaries.ps1"]
fn ordinary_words_are_unaffected_by_the_fold() {
    let Some(dictionaries) = opened() else { return };

    for (query, expected) in [("コーヒー", "coffee"), ("猫", "cat"), ("食べる", "eat")] {
        let hits = dictionaries[0].1.lookup(query).expect("lookup");
        assert!(!hits.is_empty(), "{query} should still be found");
        assert!(
            hits.iter().all(|hit| hit.matched != MatchKind::Folded),
            "{query} is an exact match and must not arrive folded"
        );
        let glosses: Vec<&str> = hits
            .iter()
            .flat_map(|hit| hit.entry.definitions.iter())
            .flat_map(|definition| definition.glosses.iter())
            .map(String::as_str)
            .collect();
        assert!(
            glosses.iter().any(|gloss| gloss.contains(expected)),
            "{query}: expected {expected:?} in {glosses:?}"
        );
    }
}
