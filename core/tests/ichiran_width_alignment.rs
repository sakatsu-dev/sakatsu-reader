//! Width rewrites must change neither source text nor which occurrence a token covers.
//! The short kaomoji fixture was captured from the installed `ichiran-cli -f`; it is a
//! constructed diagnostic, not a copy of the user's lyrics. Other echoes below are synthetic.
use sakatsu_core::analyzer::ichiran::{convert, raw};
use sakatsu_core::analyzer::{validate_analysis, Analyzer, IchiranAnalyzer};
use sakatsu_core::model::Analysis;

fn word(text: &str) -> raw::Segment {
    raw::Segment::Candidates(vec![raw::Candidate(
        vec![raw::WordEntry(
            String::new(),
            raw::WordInfo {
                text: text.into(),
                reading: text.into(),
                kana: text.into(),
                ..Default::default()
            },
            vec![],
        )],
        1.0,
    )])
}

fn checked(source: &str, segments: &[raw::Segment]) -> Analysis {
    let analysis = convert::to_analysis(source, segments);
    assert_eq!(analysis.text, source);
    assert!(
        validate_analysis(&analysis).is_empty(),
        "{:?}",
        validate_analysis(&analysis)
    );
    analysis
}

#[test]
fn captured_halfwidth_words_and_kaomoji_keep_their_source() {
    let source = "ｶﾞｯﾂ (┗(ﾟ Дﾟ )┛ﾜｯｼｮｲ ┗( ﾟД ﾟ)┛ﾜｯｼｮｲ)";
    let segments = raw::parse(include_str!("fixtures/ichiran/halfwidth-kaomoji.json")).unwrap();
    let analysis = checked(source, &segments);
    let words: Vec<_> = analysis
        .tokens
        .iter()
        .filter(|token| !token.senses.is_empty())
        .collect();
    assert_eq!(
        words
            .iter()
            .map(|token| token.surface.as_str())
            .collect::<Vec<_>>(),
        ["ｶﾞｯﾂ", "ﾜｯｼｮｲ", "ﾜｯｼｮｲ"]
    );
    assert_eq!(words[0].reading, "ガッツ");
    assert_eq!(words[0].dict_form, "ガッツ");
    assert!(words[1].end.unwrap() < words[2].start.unwrap());
}

#[test]
fn voiced_pairs_and_standalone_marks_have_exact_source_extents() {
    let analysis = checked(
        "ｶﾞﾊﾟ ﾟﾞ",
        &[
            word("ガ"),
            word("パ"),
            raw::Segment::Unanalyzed(" ゜゛".into()),
        ],
    );
    assert_eq!(
        (analysis.tokens[0].start, analysis.tokens[0].end),
        (Some(0), Some(6))
    );
    assert_eq!(
        (analysis.tokens[1].start, analysis.tokens[1].end),
        (Some(6), Some(12))
    );
    assert_eq!(analysis.tokens[2].surface, " ﾟﾞ");
}

#[test]
fn an_earlier_width_variant_wins_over_a_later_literal_occurrence() {
    for source in ["ｶﾞ ガ", "ガ ｶﾞ", "Ａ A", "A Ａ"] {
        let echo = if source.contains('ガ') { "ガ" } else { "A" };
        let analysis = checked(source, &[word(echo), word(echo)]);
        assert_eq!(analysis.tokens[0].start, Some(0));
        assert_eq!(analysis.tokens[1].end, Some(source.len()));
    }
}

#[test]
fn unrelated_compatibility_characters_are_not_invented_as_matches() {
    let analysis = convert::to_analysis("①猫", &[word("1"), word("猫")]);
    assert!(analysis.tokens[0].start.is_none());
    assert!(!validate_analysis(&analysis).is_empty());
}

#[test]
fn rewritten_punctuation_cannot_jump_to_a_later_repetition() {
    let source = "猫！(犬！ (鳥";
    let analysis = checked(
        source,
        &[
            word("猫"),
            raw::Segment::Unanalyzed("! (".into()),
            word("犬"),
            raw::Segment::Unanalyzed("! (".into()),
            word("鳥"),
        ],
    );
    assert_eq!(
        analysis
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect::<Vec<_>>(),
        ["猫", "！(", "犬", "！ (", "鳥"]
    );
}

/// Local replay only: no supplied passage is embedded in tracked source. Both the analyzer and
/// the production display/validation path must accept every byte of the same input.
#[test]
#[ignore = "needs installed Ichiran and SAKATSU_ICHIRAN_SPAN_TEXT pointing at a UTF-8 input file"]
fn supplied_text_passes_the_production_span_boundary() {
    let path = std::env::var_os("SAKATSU_ICHIRAN_SPAN_TEXT").expect("input file required");
    let text = std::fs::read_to_string(path).expect("input must be readable UTF-8");
    let analyzer = IchiranAnalyzer::default();
    // An explicitly provided capture permits deterministic replay of the exact installed
    // engine response. Otherwise run the engine. No text or echo is embedded in this test.
    let raw = if let Some(path) = std::env::var_os("SAKATSU_ICHIRAN_SPAN_JSON") {
        let json = std::fs::read_to_string(path).expect("captured JSON must be readable");
        convert::to_analysis(
            text.trim(),
            &raw::parse(&json).expect("captured JSON must parse"),
        )
    } else {
        analyzer.analyze(&text).expect("Ichiran must run")
    };
    assert!(
        validate_analysis(&raw).is_empty(),
        "{} faults, first: {:?}",
        validate_analysis(&raw).len(),
        validate_analysis(&raw).first()
    );
    struct Replay(Analysis);
    impl Analyzer for Replay {
        fn analyze(&self, _: &str) -> Result<Analysis, sakatsu_core::analyzer::AnalyzeError> {
            Ok(self.0.clone())
        }
        fn engine(&self) -> &str {
            "ichiran-capture"
        }
    }
    let dictionaries = std::env::var_os("SAKATSU_ICHIRAN_SPAN_DICT")
        .map(|path| {
            let dictionaries: Vec<_> =
                sakatsu_core::dict::index::Dictionary::discover(std::path::Path::new(&path))
                    .iter()
                    .map(|path| {
                        sakatsu_core::dict::index::Dictionary::open(path)
                            .expect("dictionary must open")
                    })
                    .collect();
            assert!(
                !dictionaries.is_empty(),
                "explicit dictionary path must contain dictionaries"
            );
            dictionaries
        })
        .unwrap_or_default();
    let shown = sakatsu_core::study::analyse(&text, &Replay(raw.clone()), &dictionaries)
        .expect("production must accept the source");
    assert_eq!(raw.text, shown.text);
    assert!(validate_analysis(&shown).is_empty());
    println!(
        "{} source characters, {} raw tokens, {} displayed tokens, no span faults",
        shown.text.chars().count(),
        raw.tokens.len(),
        shown.tokens.len()
    );
}
