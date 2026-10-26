use super::*;

/// Where the fetch leaves the frequency zips, per `setup::catalog`.
///
/// **Resolved rather than hardcoded.** This was an absolute path into one machine's temporary
/// directory, so on any other it found nothing, printed "not downloaded" and skipped — a green
/// tick over a test that never ran, which is the failure `scripts/test-all.ps1` exists to expose.
///
/// Both roots are searched because a device that installed before the local-data root existed
/// still keeps its dictionaries under the roaming one. `SAKATSU_DICT_DIR` overrides, as it does
/// everywhere else.
fn installed_zip(name: &str) -> Option<std::path::PathBuf> {
    let roots = std::env::var_os("SAKATSU_DICT_DIR")
        .map(|dir| vec![std::path::PathBuf::from(dir)])
        .unwrap_or_else(|| {
            ["LOCALAPPDATA", "APPDATA"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(|root| {
                    std::path::PathBuf::from(root)
                        .join("com.sakatsu.app")
                        .join("dictionaries")
                })
                .collect()
        });
    roots
        .into_iter()
        .map(|root| {
            root.join("frequency")
                .join(name)
                .join("work")
                .join(format!("{name}-frequency.zip"))
        })
        .find(|candidate| candidate.is_file())
}

fn read(json: &str, direction: Direction) -> Frequencies {
    let mut into = Frequencies::new("Test Corpus", direction);
    read_meta_bank(json, &mut into).expect("the bank parses");
    into
}

#[test]
fn every_shape_a_frequency_row_comes_in_is_understood() {
    // All five appear in published lists. A shape that fell through would leave those words
    // unranked — sorted last, silently, as though the corpus had never heard of them.
    let bank = r#"[
        ["食べる", "freq", 384],
        ["飲む", "freq", "512"],
        ["見る", "freq", {"value": 96, "displayValue": "96ゕ"}],
        ["行く", "freq", {"reading": "いく", "frequency": 12}],
        ["来る", "freq", {"reading": "くる", "frequency": {"value": 24}}]
    ]"#;
    let freq = read(bank, Direction::Rank);

    assert_eq!(
        freq.len(),
        7,
        "five terms, two of which also key by reading"
    );
    assert_eq!(freq.rank("食べる", ""), Some(384.0));
    assert_eq!(freq.rank("飲む", ""), Some(512.0));
    assert_eq!(freq.rank("見る", ""), Some(96.0));
    assert_eq!(freq.rank("行く", "いく"), Some(12.0));
    assert_eq!(freq.rank("来る", "くる"), Some(24.0));
}

#[test]
fn a_row_that_is_not_a_frequency_is_left_alone() {
    // Pitch accent and IPA ride in the same file and are somebody else's feature.
    let bank = r#"[
        ["食べる", "pitch", {"reading": "たべる", "pitches": [{"position": 2}]}],
        ["食べる", "ipa", {"reading": "たべる", "transcriptions": []}],
        ["食べる", "freq", 384],
        ["", "freq", 1],
        ["broken"],
        [42, "freq", 1]
    ]"#;
    let freq = read(bank, Direction::Rank);
    assert_eq!(freq.len(), 1, "only the one real frequency row counts");
    assert_eq!(freq.rank("食べる", ""), Some(384.0));
}

/// The test this module exists for.
///
/// The same numbers mean opposite things depending on how the corpus counted, and nothing
/// in the data says which. Read a count list as a rank and the commonest word in Japanese
/// sorts last, with every part of the machinery working perfectly.
#[test]
fn a_rank_and_a_count_order_the_same_numbers_opposite_ways() {
    let bank = r#"[["食べる", "freq", 5], ["喫する", "freq", 900]]"#;

    let ranked = read(bank, Direction::Rank);
    assert!(
        ranked.rank("食べる", "").unwrap() < ranked.rank("喫する", "").unwrap(),
        "in a rank list, 5th place beats 900th"
    );

    let counted = read(bank, Direction::Count);
    assert!(
        counted.rank("食べる", "").unwrap() > counted.rank("喫する", "").unwrap(),
        "in a count list, 900 occurrences beats 5"
    );
}

#[test]
fn a_word_the_corpus_never_saw_has_no_rank_at_all() {
    // Not "very rare". A list that stops at 30,000 words says nothing about the 30,001st,
    // and the difference matters: unranked words keep the order they already had rather
    // than being sorted to the bottom as though they had been measured and found wanting.
    let freq = read(r#"[["食べる", "freq", 1]]"#, Direction::Rank);
    assert_eq!(freq.rank("喫する", ""), None);
}

#[test]
fn a_reading_is_used_when_it_is_known_and_not_required_when_it_is_not() {
    // 辛い is からい or つらい and they are not equally common. A lookup that knows which
    // should get that answer; one that does not should still get something.
    let bank = r#"[
        ["辛い", "freq", {"reading": "からい", "frequency": 100}],
        ["辛い", "freq", {"reading": "つらい", "frequency": 20}]
    ]"#;
    let freq = read(bank, Direction::Rank);

    assert_eq!(freq.rank("辛い", "からい"), Some(100.0));
    assert_eq!(freq.rank("辛い", "つらい"), Some(20.0));
    assert_eq!(
        freq.rank("辛い", ""),
        Some(20.0),
        "with no reading to go on, the commoner of the two is the better guess"
    );
    assert_eq!(
        freq.rank("辛い", "むり"),
        Some(20.0),
        "a reading the list does not carry falls back rather than failing"
    );
}

#[test]
fn a_duplicate_keeps_the_commoner_value_whichever_way_the_list_counts() {
    let ranked = read(
        r#"[["食べる", "freq", 900], ["食べる", "freq", 5]]"#,
        Direction::Rank,
    );
    assert_eq!(ranked.rank("食べる", ""), Some(5.0));

    let counted = read(
        r#"[["食べる", "freq", 5], ["食べる", "freq", 900]]"#,
        Direction::Count,
    );
    assert_eq!(counted.rank("食べる", ""), Some(-900.0));
}

#[test]
fn a_badge_after_the_digits_is_not_part_of_the_number() {
    // Some lists write "1234㋕" to mark which sub-corpus a figure came from.
    let freq = read(r#"[["食べる", "freq", "1234㋕"]]"#, Direction::Rank);
    assert_eq!(freq.rank("食べる", ""), Some(1234.0));
}

#[test]
fn a_malformed_bank_is_an_error_rather_than_an_empty_list() {
    // Silently empty would look exactly like a corpus that had nothing to say, and the
    // reader would get today's ordering with a caveat claiming it was ranked.
    let mut into = Frequencies::new("Test", Direction::Rank);
    assert!(read_meta_bank("{not a bank}", &mut into).is_err());
}

/// Against the two lists the app offers, unpacked from their real zips.
///
/// The tests above prove the parser handles each shape. This proves the shapes those two
/// files actually use are among them, that `unpack` picks the right files out, and — the
/// part worth the trouble — that the direction the catalog declares is the direction the
/// numbers really run. A README saying "rank" is not evidence; 食べる sorting ahead of
/// 顰蹙 is.
///
/// Ignored because it wants the zips on disk.
///
/// ```text
/// cargo test -p sakatsu-core frequency -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs the downloaded zips; run by hand"]
fn the_two_offered_lists_parse_and_run_the_way_the_catalog_says() {
    let scratch = std::env::temp_dir().join("sakatsu-frequency-check");
    for (zip, expected) in [("jpdb", "JPDB"), ("bccwj", "BCCWJ")] {
        let Some(source) = installed_zip(zip) else {
            println!("SKIP: {zip} is not downloaded");
            continue;
        };
        let out = scratch.join(format!("unpacked-{zip}"));
        let _ = std::fs::remove_dir_all(&out);
        let files = unpack(&source, &out).expect("the zip unpacks");
        assert!(files >= 2, "{zip}: expected an index and at least one bank");

        let freq = Frequencies::load(&out, Direction::Rank);
        assert!(!freq.is_empty(), "{zip}: nothing parsed out of it");
        assert!(
            freq.name.contains(expected),
            "{zip}: called itself {:?}",
            freq.name
        );

        let common = freq
            .rank("食べる", "たべる")
            .expect("食べる is in the corpus");
        let rare = freq
            .rank("顰蹙", "ひんしゅく")
            .expect("顰蹙 is in the corpus");
        assert!(
            common < rare,
            "{zip}: 食べる ({common}) must sort ahead of 顰蹙 ({rare}). If it does not, \
             the catalog has this list's direction backwards and every search is ordered \
             the wrong way round while looking like it works."
        );
        eprintln!(
            "{zip}: {} — {} entries, 食べる {common}, 顰蹙 {rare}",
            freq.name,
            freq.len()
        );
    }
}

/// Every frequency list the app offers must have a known direction.
///
/// The setting is global and getting it wrong reverses the results outright, so the two the app
/// fetches itself are read the way they are written rather than the way a setting guesses. A
/// third list added to the catalog without an arm here would silently fall back to that guess.
#[test]
fn every_offered_frequency_list_has_a_known_direction() {
    let offered: Vec<&str> = crate::setup::catalog::all()
        .iter()
        .filter_map(|component| component.id.strip_prefix("frequency-"))
        .collect();
    assert!(
        !offered.is_empty(),
        "the catalog must still offer frequency lists"
    );

    for source in &offered {
        assert_eq!(
            super::shipped_direction(source),
            Some(super::Direction::Rank),
            "`{source}` is offered by the catalog but `shipped_direction` has no answer for it, \
             so a stray `count` setting would read it backwards"
        );
    }

    // And nothing claimed that is not offered, or the two have drifted the other way.
    for known in ["jpdb", "bccwj"] {
        assert!(
            offered.contains(&known),
            "`shipped_direction` knows `{known}`, which the catalog no longer offers"
        );
    }
}
