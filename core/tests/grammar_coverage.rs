//! N2/N1 grammar coverage, and the two properties that make it trustworthy.
//!
//! The imported JLPT pack shipped 828 explained grammar points that nothing could
//! find, because none of them carried a match rule. These tests pin what the first
//! authored batch actually detects — on **held-out sentences**, none of which is an
//! example from the pack, so they measure whether the rules generalise rather than
//! whether they memorised their own corpus.
//!
//! The rules are surface runs (`match_text`) rather than single-token surfaces, for a
//! reason worth stating: the two analyzers disagree about what a token is. Ichiran
//! hands `には` over as one token, Lindera's UniDic as `に` + `は`. A rule written
//! against either one alone is silently dead under the other — and Lindera is what a
//! user without the sidecar gets.

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::grammar::{matcher, pack};

fn fired(text: &str) -> Vec<String> {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let tokens = analyzer.analyze(text).expect("analyses").tokens;
    matcher::find_across(&tokens, &pack::all())
        .iter()
        .map(|m| m.pattern_id.clone())
        .collect()
}

fn assert_finds(text: &str, pattern_id: &str) {
    let found = fired(text);
    assert!(
        found.iter().any(|id| id == pattern_id),
        "expected {pattern_id} in {text}, got {found:?}"
    );
}

#[test]
fn n2_patterns_are_found_in_held_out_sentences() {
    assert_finds(
        "雨にもかかわらず、試合は行われた。",
        "hanabira-n2-ni-mo-kakawarazu",
    );
    assert_finds(
        "彼が来たからといって、何も変わらない。",
        "hanabira-n2-kara-to-itte",
    );
    // Deliberately without 必ずしも: the pack has a more specific 必ずしも〜とは限らない
    // at N3, and it rightly wins where it applies. This tests the bare pattern.
    assert_finds("高い物が良いとは限らない。", "hanabira-n2-to-wa-kagiranai");
    assert_finds(
        "疲れていたものだから、つい寝てしまった。",
        "hanabira-n2-mono-dakara",
    );
    assert_finds("調査の結果次第で方針を変える。", "hanabira-n2-shidai-de");
    assert_finds("説明を聞いた上で判断したい。", "hanabira-n2-ue-de");
}

#[test]
fn n1_patterns_are_found_in_held_out_sentences() {
    assert_finds("資料を作成するべく、徹夜した。", "hanabira-n1-verb-ru-beku");
    assert_finds(
        "東京を皮切りにして全国を回る。",
        "hanabira-n1-noun-wo-kawakiri-ni-shite",
    );
    // Was `hanabira-n1-no-koto-dakara`. The pack shipped this point twice — once at N1 and once
    // at N2, same title and same formation — and `grammar_accuracy.rs` now refuses that. The N2
    // entry survived the merge because its rule (`ことだから`) already matches everything the N1
    // one's (`のことだから`) did, so the sentence is still found; only the id it is found under
    // changed.
    assert_finds(
        "彼のことだから、きっと遅れるだろう。",
        "hanabira-n2-noun-no-koto-dakara",
    );
    // The analyzers may keep the verb stem and やしない separate or fuse the whole
    // expression. The ending rule covers both without matching inside a token.
    assert_finds("彼はそんな嘘を信じやしない。", "hanabira-n1-yashinai");
}

/// The reason `match_text` exists. Both spellings of the same sentence structure must
/// find the same point, whichever analyzer segmented it.
#[test]
fn a_rule_fires_whether_the_analyzer_merges_or_splits() {
    // Lindera splits these into に + は and こと + が + できる; Ichiran hands each over
    // as a single token. One rule, both segmentations.
    assert_finds("彼には時間がない", "particle-niwa");
    assert_finds("見ることができる", "koto-ga-dekiru");
    assert_finds("家では何もしなかった", "particle-dewa");

    // This one carries both kinds of rule at once, and needs to. Ichiran hands over
    // 行かなければ as a single token whose *conjugation* is a negative provisional,
    // which only the element rule can express; Lindera splits it into なけれ/ば/なら/
    // ない, which only a surface run can span. Neither rule alone covers both.
    assert_finds("行かなければならない", "nakereba-naranai");
}

/// The levels the curated pack already reached, where the imported pack adds depth.
///
/// Authored under a stricter filter than N1/N2 needed, because N3–N5 is where the two
/// packs overlap and where the pack lists a good deal of *vocabulary* as grammar.
/// さっき and こちら are words; てください and べきだ are constructions. Only what the
/// written pattern shows attaching to something was taken.
#[test]
fn n3_to_n5_patterns_are_found_in_held_out_sentences() {
    assert_finds(
        "この書類を確認してください。",
        "hanabira-n5-verb-te-kudasai",
    );
    assert_finds("早く寝なさい。", "hanabira-n4-nasai");
    assert_finds("もっと勉強するべきだ。", "hanabira-n3-beki-da");
    assert_finds("どうすればいいですか。", "hanabira-n4-ba-ii");
    assert_finds("少しずつ進めよう。", "hanabira-n4-zutsu");
    assert_finds("彼も来るだろう。", "hanabira-n4-darou");
    assert_finds("宿題を忘れちゃった。", "hanabira-n3-chatta");
    assert_finds("一度食べてごらん。", "hanabira-n3-te-goran");
}

fn assert_silent(text: &str, pattern_id: &str) {
    let found = fired(text);
    assert!(
        !found.iter().any(|id| id == pattern_id),
        "{pattern_id} should not fire on {text}, but got {found:?}"
    );
}

/// The reason rules can constrain a neighbour.
///
/// A surface run alone cannot tell 〜上に (the grammar point, "on top of that") from
/// 机の上に (a shelf). Both spell 上に. What separates them is what comes *before*:
/// a clause, or a noun and の. Constraining that is what let this rule come back after
/// the independent corpus caught it matching furniture.
#[test]
fn a_rule_can_require_what_precedes_it() {
    assert_finds("彼は仕事が遅い上にミスも多い。", "hanabira-n2-ue-ni");
    assert_finds("調べた上に実験までした。", "hanabira-n2-ue-ni");

    assert_silent("机の上に本がある。", "hanabira-n2-ue-ni");
    assert_silent("丘の上に城があった。", "hanabira-n2-ue-ni");

    // 以上 after a clause is "now that"; after a number it is "or more".
    assert_finds("引き受けた以上、最後までやる。", "hanabira-n2-ijou");
    assert_silent("六歳以上の子供は学校に通う。", "hanabira-n2-ijou");

    // 向け is a suffix on a noun; 向ける is a verb, and only the verb inflects.
    assert_finds("子供向けの本を買った。", "hanabira-n2-muke");
    assert_silent("彼は視線を向けた。", "hanabira-n2-muke");
}

/// The reason rules can span a gap.
///
/// だれも〜ません and 〜ば〜ほど are discontinuous: the two halves are separated by
/// whatever the sentence puts between them. No surface run can express that, which is
/// why this whole class had no rules at all.
#[test]
fn a_rule_can_span_a_gap() {
    assert_finds("だれも来ませんでした。", "hanabira-n5-daremo-masen");
    assert_finds("早く言えばよかったものを。", "hanabira-n1-ba-mono-o");
    assert_finds("読めば読むほど面白い。", "hanabira-n3-ba-hodo");

    // もの + を is a noun and a particle, not the grammar point.
    assert_silent("使っていたものを割ってしまった。", "hanabira-n1-ba-mono-o");
}

/// Constructions whose halves sit next to each other are one rule, not a gap.
///
/// The derivation split `Verb ないで ください` into two runs because a placeholder stood
/// between them, but nothing separates them in a real sentence — and Ichiran swallows
/// the lot into `吸わないでください`, where neither half can be found on its own. A single
/// suffix over the joined form finds it under both analyzers.
#[test]
fn a_contiguous_construction_is_found_whole() {
    assert_finds("タバコを吸わないでください。", "hanabira-n5-naide-kudasai");
    assert_finds(
        "彼は宿題をしているところです。",
        "hanabira-n4-verb-te-iru-tokoro",
    );
    assert_finds(
        "彼は会議に出席しないことがある。",
        "hanabira-n4-verb-nai-koto-ga-aru",
    );
}

/// A gap must not step over the end of a sentence: だれも in one sentence has nothing
/// to do with a ません in the next.
#[test]
fn a_gap_does_not_cross_a_sentence_boundary() {
    assert_silent(
        "だれも知っている。だから行きません。",
        "hanabira-n5-daremo-masen",
    );
}

/// A pattern must never be reported from *inside* a token — the `project notes` §5 rule.
///
/// Under Ichiran `にはとり` arrives whole and no run of tokens starting there equals
/// `には`, so nothing fires. Under Lindera the same string is genuinely analysed as
/// `に` / `は` / `とり`, and reporting `には` across the first two is consistent with
/// the segmentation the reader displays. What neither may do is match a substring of
/// one token, which is what a regex over the sentence would have done.
#[test]
fn a_match_never_starts_or_ends_inside_a_token() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let text = "にはとりが鳴く";
    let tokens = analyzer.analyze(text).expect("analyses").tokens;
    let matches = matcher::find_across(&tokens, &pack::all());

    for found in &matches {
        assert!(
            found.first_token < tokens.len() && found.last_token_end <= tokens.len(),
            "match {found:?} runs past the token sequence"
        );
        assert!(
            found.token_count() >= 1,
            "a match must cover at least one whole token"
        );
    }
}

/// A pattern engine that lights up every sentence is noise, not help. The imported
/// batch must not materially raise how much of a sentence is marked.
#[test]
fn the_imported_rules_do_not_flood_a_sentence() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let corpus: Vec<&str> = vec![
        "水を飲む。",
        "私は学生です。",
        "今日は天気がいいですね。",
        "彼女は本を読んでいます。",
        "明日、友達と映画を見に行きます。",
    ];
    for text in corpus {
        let tokens = analyzer.analyze(text).expect("analyses").tokens;
        let found = matcher::find_across(&tokens, &pack::all());
        assert!(
            found.len() * 2 <= tokens.len().max(2),
            "{text} marked {} of {} tokens: {found:?}",
            found.len(),
            tokens.len()
        );
    }
}
