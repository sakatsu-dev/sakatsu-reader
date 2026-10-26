//! Golden tests over **real** `ichiran-cli -f` output (`project notes` §10).
//!
//! The fixtures in `tests/fixtures/ichiran` were captured from a working Ichiran
//! by `scripts/capture-ichiran-fixtures.sh`; regenerate them with that script
//! rather than editing them. Every test here names the word that taught the rule,
//! so a failure says which piece of §4 broke rather than just "parse mismatch".

mod common;

use common::{analyze, surfaces};
use sakatsu_core::analyzer::ichiran::{convert, raw};
use sakatsu_core::model::{Analysis, Token};

fn forms(token: &Token) -> Vec<(&str, u32, bool)> {
    token
        .conjugation
        .as_ref()
        .map(|conjugation| {
            conjugation
                .props
                .iter()
                .map(|prop| (prop.form.as_str(), prop.via_depth, prop.is_base))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_simple_sentence_splits_into_grounded_tokens() {
    let analysis = analyze("mizu-o-nomu", "水を飲む");
    assert_eq!(surfaces(&analysis), ["水", "を", "飲む"]);

    // Every token must point back at the exact bytes it came from: the reader
    // highlights by offset, so a wrong span mis-highlights the sentence.
    for token in &analysis.tokens {
        let (start, end) = (token.start.expect("start"), token.end.expect("end"));
        assert_eq!(&analysis.text[start..end], token.surface);
    }

    let nomu = &analysis.tokens[2];
    assert_eq!(nomu.dict_form, "飲む");
    assert_eq!(nomu.reading, "のむ");
    assert!(
        nomu.senses
            .iter()
            .any(|sense| sense.gloss.contains("drink")),
        "senses: {:?}",
        nomu.senses
    );
}

/// Ichiran's per-word score and romaji reach the model.
///
/// Both were parsed and thrown away for a long time — the romaji was even marked
/// `#[allow(dead_code)]`. The name rules in `dict::merge` are built on the score, so a
/// conversion that quietly dropped it would leave every name unpromoted and every test
/// over hand-built tokens still green. This reads them off a fixture captured from a real
/// `ichiran-cli`, which is the only place the real numbers come from.
#[test]
fn the_score_and_romaji_survive_conversion() {
    let analysis = analyze("mizu-o-nomu", "水を飲む");

    let scores: Vec<Option<i32>> = analysis
        .tokens
        .iter()
        .map(|token| token.confidence)
        .collect();
    assert_eq!(
        scores,
        [Some(25), Some(11), Some(128)],
        "the fixture's own scores, unrounded and in order"
    );

    let romaji: Vec<Option<&str>> = analysis
        .tokens
        .iter()
        .map(|token| token.romaji.as_deref())
        .collect();
    assert_eq!(romaji, [Some("mizu"), Some("wo"), Some("nomu")]);
}

/// §4: "A word with its own `gloss` IS a dictionary word." Ichiran attaches a
/// speculative conj reading 気持ち as the stem of 気持ちい; preferring it would
/// relabel a common noun as an adjective stem and invent grammar.
#[test]
fn a_word_with_its_own_gloss_keeps_its_own_lemma_and_demotes_the_conj() {
    let analysis = analyze("kimochi", "気持ち");
    let token = &analysis.tokens[0];

    assert_eq!(token.dict_form, "気持ち");
    assert_eq!(token.lemma_reading, "きもち");
    assert!(token.conjugation.is_none(), "気持ち is not conjugated");
    assert!(token
        .senses
        .iter()
        .any(|sense| sense.gloss.contains("feeling")));

    // The speculative parse survives as an alternative, not as this word's grammar.
    let alternative = &token.alternatives[0];
    assert_eq!(alternative.lemma, "気持ちい");
    assert_eq!(alternative.form, "Adjective Stem");
}

/// §4: competing `via` branches. 言わせて returns two complete parses — 言う
/// (Causative) and 言わす (Potential). Following both makes the grammar view show a
/// Potential that is not in the sentence.
#[test]
fn a_fork_follows_the_voice_branch_and_never_shows_a_phantom_potential() {
    let analysis = analyze("iwasete", "言わせて");
    let token = &analysis.tokens[0];

    // Base-verb resolution: 言わせて → 言う, not 言わす.
    assert_eq!(token.dict_form, "言う");
    assert_eq!(token.lemma_reading, "いう");

    assert_eq!(
        forms(token),
        [("Conjunctive (~te)", 0, false), ("Causative", 1, true)],
        "the chain is te-form over the causative of the dictionary verb"
    );

    // The phantom.
    assert!(
        !forms(token).iter().any(|(form, ..)| *form == "Potential"),
        "Potential must not appear in the grammar of 言わせて"
    );

    // It is still offered as "also parses as".
    let potential = token
        .alternatives
        .iter()
        .find(|alternative| alternative.form == "Potential")
        .expect("the dropped branch is kept as an alternative");
    assert_eq!(potential.lemma, "言わす");

    // The base prop carries the dictionary verb's reading — what lets the grammar
    // layer show how the form is built rather than only naming it.
    let base = token
        .conjugation
        .as_ref()
        .unwrap()
        .props
        .iter()
        .find(|prop| prop.is_base)
        .unwrap();
    assert_eq!(base.base_reading, "いう");
    assert_eq!(base.pos, "v5u");
}

/// §4: a compound's senses live on its components' `conj` — not on the compound,
/// and not on the components themselves. Reading the components' own fields is the
/// bug that once hid every compound's definition.
#[test]
fn a_compound_carries_its_parts_and_each_part_finds_its_meaning() {
    let analysis = analyze("kimochi-tsutawatte", "気持ちが伝わってきます");
    assert_eq!(surfaces(&analysis), ["気持ち", "が", "伝わってきます"]);

    let compound = &analysis.tokens[2];
    assert!(compound.is_compound());
    assert_eq!(
        compound
            .components
            .iter()
            .map(|part| part.surface.as_str())
            .collect::<Vec<_>>(),
        ["伝わって", "きます"]
    );

    // The head is the first component, so the compound is lookup-able as 伝わる.
    assert_eq!(compound.dict_form, "伝わる");

    let (tsutawatte, kimasu) = (&compound.components[0], &compound.components[1]);
    assert_eq!(tsutawatte.dict_form, "伝わる");
    assert!(
        tsutawatte
            .senses
            .iter()
            .any(|sense| sense.gloss.contains("conveyed") || sense.gloss.contains("spread")),
        "the te-form component must still find its senses: {:?}",
        tsutawatte.senses
    );
    assert_eq!(forms(tsutawatte), [("Conjunctive (~te)", 0, true)]);

    // §4's reading normalization: Ichiran writes 来る's lemma reading as 【クる】.
    assert_eq!(kimasu.dict_form, "来る");
    assert_eq!(
        kimasu.lemma_reading, "くる",
        "the lemma reading is forced to hiragana"
    );
    // ...but the surface reading, which drives furigana, is untouched.
    assert_eq!(kimasu.reading, "きます");
    assert!(kimasu
        .senses
        .iter()
        .any(|sense| sense.gloss.contains("come")));

    // Components are grounded inside the compound's own span.
    for part in &compound.components {
        let (start, end) = (part.start.expect("start"), part.end.expect("end"));
        assert_eq!(&analysis.text[start..end], part.surface);
        assert!(start >= compound.start.unwrap() && end <= compound.end.unwrap());
    }
}

/// §10's deepest case: 食べさせられていた. The derivation a learner needs is
/// 食べる → causative-passive → te → past, which spans both components.
#[test]
fn a_deep_compound_resolves_its_base_verb_and_orders_the_derivation() {
    let analysis = analyze("tabesaserareteita", "食べさせられていた");
    let compound = &analysis.tokens[0];

    assert!(compound.is_compound());
    assert_eq!(compound.dict_form, "食べる");

    let (tabesaserarete, ita) = (&compound.components[0], &compound.components[1]);

    assert_eq!(tabesaserarete.dict_form, "食べる");
    assert_eq!(
        forms(tabesaserarete),
        [
            ("Conjunctive (~te)", 0, false),
            ("Causative-Passive", 1, true)
        ],
        "causative-passive must win over plain causative or passive"
    );
    assert!(tabesaserarete
        .senses
        .iter()
        .any(|sense| sense.gloss.contains("eat")));

    // The auxiliary carries the tense, and its own lemma.
    assert_eq!(ita.dict_form, "居る");
    assert_eq!(ita.lemma_reading, "いる");
    assert_eq!(forms(ita), [("Past (~ta)", 0, true)]);
}

/// §10: 先生ってほんと…情けないですね. The ellipsis is a segment Ichiran refuses to
/// analyse, and it must survive as a non-vocabulary token rather than vanishing or
/// becoming a blank dictionary row.
#[test]
fn unanalyzed_punctuation_survives_as_a_token_but_not_as_vocabulary() {
    let analysis = analyze("sensei-nasakenai", "先生ってほんと…情けないですね");
    assert_eq!(
        surfaces(&analysis),
        ["先生", "って", "ほんと", "…", "情けないです", "ね"]
    );

    let ellipsis = &analysis.tokens[3];
    assert!(!ellipsis.is_word(), "… is punctuation, not vocabulary");
    assert!(ellipsis.senses.is_empty());

    // Tokens on both sides of the gap stay correctly grounded.
    for token in &analysis.tokens {
        let (start, end) = (token.start.expect("start"), token.end.expect("end"));
        assert_eq!(&analysis.text[start..end], token.surface);
    }

    // The polite copula is a formal non-past of 情けない, not its own headword.
    let nasakenai = &analysis.tokens[4];
    assert_eq!(nasakenai.dict_form, "情けない");
    let prop = &nasakenai.conjugation.as_ref().unwrap().props[0];
    assert!(prop.fml, "です is formal");
    assert!(
        !prop.neg,
        "情けない is lexically negative, not grammatically"
    );
}

/// §10: これを機に言わせてもらいますが — the 〜てもらう compound, whose first part is
/// the same 言わせて fork as above. The fork must resolve the same way inside a
/// compound as it does alone.
#[test]
fn the_fork_resolves_the_same_way_inside_a_compound() {
    let analysis = analyze("kore-o-ki-ni", "これを機に言わせてもらいますが");
    let compound = analysis
        .tokens
        .iter()
        .find(|token| token.is_compound())
        .expect("言わせてもらいます is a compound");

    assert_eq!(
        compound
            .components
            .iter()
            .map(|part| part.surface.as_str())
            .collect::<Vec<_>>(),
        ["言わせて", "もらいます"]
    );
    assert_eq!(compound.dict_form, "言う");

    let iwasete = &compound.components[0];
    assert_eq!(
        forms(iwasete),
        [("Conjunctive (~te)", 0, false), ("Causative", 1, true)]
    );
    assert!(!forms(iwasete).iter().any(|(form, ..)| *form == "Potential"));

    let moraimasu = &compound.components[1];
    assert_eq!(moraimasu.dict_form, "貰う");
    assert!(moraimasu.conjugation.as_ref().unwrap().props[0].fml);
}

/// An ambiguous surface arrives as an `alternative` wrapper: a word entry with no
/// `text`, `gloss` or `conj` of its own, only competing entries. This is not an edge
/// case — ordinary sentences produce it constantly — and mishandling it fails the
/// whole passage rather than one word.
#[test]
fn an_ambiguous_surface_resolves_to_its_best_entry_and_keeps_the_rest() {
    let analysis = analyze("niwatori", "にはとりが鳴く");
    assert_eq!(surfaces(&analysis), ["には", "とり", "が", "鳴く"]);

    let tori = &analysis.tokens[1];
    assert_eq!(tori.surface, "とり");
    // 鳥 scores highest (16 against a field of 12s), so it is the chosen reading.
    assert!(
        tori.senses.iter().any(|sense| sense.gloss.contains("bird")),
        "the best-scoring entry wins: {:?}",
        tori.senses
    );

    // The others survive as "also parses as", and none of them is a blank.
    assert!(
        tori.alternatives.len() >= 4,
        "expected the competing entries, got {}",
        tori.alternatives.len()
    );
    for alternative in &tori.alternatives {
        assert!(
            !alternative.lemma.is_empty() || !alternative.senses.is_empty(),
            "an empty alternative is noise: {alternative:?}"
        );
    }
}

/// Ichiran normalizes 、 to ", ", and the reader gets the 、 back.
///
/// **This test asserted the opposite until 2026-08-27**, when it expected the surface to be
/// `", "` and the token to be ungrounded. That was the bug written down as a requirement: the
/// reader renders token surfaces, so the comma somebody captured was displayed as Ichiran's
/// ASCII one. Losing the span was the *symptom* the old test pinned, and it is the same thing
/// that makes recovery possible — the ASCII is not in the source, so the span between the
/// words on either side is the original.
///
/// What it was really guarding still matters and is still checked below: a rewritten token
/// must not drag the tokens after it out of alignment.
#[test]
fn normalized_punctuation_is_restored_from_the_source_without_breaking_alignment() {
    let text = "彼には時間がなかったので、家では何もしなかった";
    let analysis = analyze("kare-niwa", text);

    // Compound particles arrive as single tokens — Ichiran merges them itself.
    assert_eq!(
        surfaces(&analysis),
        [
            "彼",
            "には",
            "時間",
            "が",
            "なかった",
            "ので",
            "、",
            "家",
            "では",
            "何も",
            "しなかった"
        ]
    );

    let comma = &analysis.tokens[6];
    assert_eq!(comma.surface, "、", "the source's comma, not Ichiran's");
    assert_eq!(
        &text[comma.start.expect("restored")..comma.end.expect("restored")],
        "、",
        "a restored surface has to point at the characters it claims"
    );
    assert!(!comma.is_word(), "punctuation is still not vocabulary");

    // Everything else still points at the right bytes, including after the gap.
    for token in &analysis.tokens {
        if let (Some(start), Some(end)) = (token.start, token.end) {
            assert_eq!(&analysis.text[start..end], token.surface);
        }
    }
    let ie = &analysis.tokens[7];
    assert_eq!(&text[ie.start.unwrap()..ie.end.unwrap()], "家");
}

/// The `neg` flag is real and must reach the model: 食べなかった is a negative past,
/// and a grammar view that drops the negation says the opposite of the sentence.
#[test]
fn a_negative_form_keeps_its_negation() {
    let analysis = analyze("tabenakatta", "食べなかった");
    let token = &analysis.tokens[0];

    assert_eq!(token.dict_form, "食べる");
    let prop = &token.conjugation.as_ref().unwrap().props[0];
    assert_eq!(prop.form, "Past (~ta)");
    assert!(prop.neg, "食べなかった is negative");
    assert!(prop.is_base);
    assert_eq!(prop.base_reading, "たべる");
}

/// **Ichiran rewrites Japanese punctuation, and the reader must not show its version.**
///
/// It normalises on the way through: 「 comes back as `"`, 、 as `,`, 〜 as `-`, 【】 as `[]`,
/// 『』 as `«»`. The reader renders token surfaces, so before this was fixed a captured
/// 「町で見かけた」 was displayed as "町で見かけた" — the app quietly changing the characters
/// somebody had just photographed.
///
/// Grounding is what makes it recoverable: the ASCII is not in the source, so those tokens
/// fail to ground, and the span between the words on either side is the original text.
#[test]
fn japanese_punctuation_survives_the_round_trip() {
    let source = "「町で見かけた」、【本】";
    let analysis = analyze("japanese-punctuation", source);

    // Nothing Ichiran substituted may reach a surface.
    for token in &analysis.tokens {
        for ascii in ['"', ',', '[', ']', '«', '»'] {
            assert!(
                !token.surface.contains(ascii),
                "token {:?} carries Ichiran's {ascii:?} instead of the source's punctuation",
                token.surface
            );
        }
    }

    // And the characters that were captured are the ones that come back.
    let joined: String = analysis.tokens.iter().map(|t| t.surface.as_str()).collect();
    for expected in ['「', '」', '、', '【', '】'] {
        assert!(
            joined.contains(expected),
            "{expected:?} is in the source but not in any token: {joined:?}"
        );
    }

    // Grounded, too — a restored surface that pointed nowhere would break furigana and
    // every offset-driven feature that reads spans.
    for token in &analysis.tokens {
        if let (Some(start), Some(end)) = (token.start, token.end) {
            assert_eq!(
                &source[start..end],
                token.surface,
                "span disagrees with surface"
            );
        }
    }
}

/// **The topic particle は arrives with an invisible character, and it must not stay.**
///
/// Ichiran writes it as `\u{200c}は` in the `kana` field — a zero-width non-joiner, presumably
/// marking that it is said "wa" — while the `reading` field is clean. The app reads `kana`,
/// so the commonest particle in the language compared unequal to itself: furigana draws ruby
/// only where the reading differs from the surface, so every は carried a redundant は over it,
/// and anything keyed on the reading (audio, Anki) looked up a string no dictionary holds.
///
/// Invisible by construction, which is why it survived so long — the fixture is committed with
/// the marker still in it so this cannot quietly stop being tested.
#[test]
fn the_topic_particle_reading_carries_no_invisible_characters() {
    let analysis = analyze("topic-particle", "私は学生");

    let wa = analysis
        .tokens
        .iter()
        .find(|token| token.surface == "は")
        .expect("は is in 私は学生");

    for ch in wa.reading.chars() {
        assert!(
            !matches!(ch, '\u{200b}' | '\u{200c}' | '\u{200d}'),
            "reading {:?} still carries U+{:04X}",
            wa.reading,
            ch as u32
        );
    }
    // The point of stripping it: kana over identical kana is not furigana.
    assert_eq!(
        wa.reading, wa.surface,
        "a kana surface and its reading have to match, or ruby is drawn over nothing"
    );
}

/// **A tie is Ichiran declining to choose, and the only place a cheaper parser may decide.**
///
/// 晴々 comes back as two entries scored 65 apiece — せいせい (seq 1596010, "feeling
/// refreshed") and はればれ (seq 1750800). Nothing in the score separates them, so which one
/// the reader sees is whichever the array happens to list first, and in this sentence that is
/// the wrong one. The embedded UniDic parser reads it はればれ, and here it gets to say so.
///
/// The whole entry is promoted, not just the reading: the lemma, senses and reading all come
/// from one dictionary record, so the definition shown underneath matches the word chosen.
#[test]
fn a_tie_is_broken_by_the_embedded_parser() {
    let analysis = analyze("harebare", "晴々とした気分");
    assert_eq!(reading_of(&analysis, "晴々"), "はればれ");

    // **The half that keeps the test honest.** Without a second opinion the same fixture
    // gives せいせい — so the assertion above is measuring the tie-break and not
    // Ichiran having listed the right entry first all along.
    let raw = convert::to_analysis(TEXT, &segments_of("harebare"));
    assert_eq!(reading_of(&raw, "晴々"), "せいせい");
}

/// **A score Ichiran was sure of is never overruled**, however plausible the other reading.
///
/// 家 in 家では何もしなかった scores いえ at 16 against うち at 11 — different
/// readings, different entries, and a real choice Ichiran made with the sentence in front of
/// it. うち is not a silly answer here, which is what makes it the right thing to reject:
/// the corpus measured the sidecar as the better parser on exactly these judgements, and a
/// tie-break that leaked into confident cases would trade those away for a cheaper opinion.
#[test]
fn a_confident_ichiran_is_never_overruled() {
    let text = "彼には時間がなかったので、家では何もしなかった";
    let analysis =
        convert::to_analysis_with(text, &segments_of("kare-niwa"), &|_| Some("うち".into()));
    assert_eq!(reading_of(&analysis, "家"), "いえ");
}

/// A second opinion that names a reading nobody offered changes nothing.
///
/// The promotion picks one of the *tied entries*; it never writes a reading in. A parser that
/// disagrees with every candidate has no vote, and Ichiran's order stands.
#[test]
fn an_answer_matching_no_candidate_leaves_the_order_alone() {
    let analysis = convert::to_analysis_with(TEXT, &segments_of("harebare"), &|_| {
        Some("はればれしい".into())
    });
    assert_eq!(reading_of(&analysis, "晴々"), "せいせい");
}

/// **The tied word is read in its sentence.** 方 ties at 16 between ほう and かた, and alone it
/// reads ほう — so the bare question answered ほう whatever the sentence said. In 投げ方 the
/// embedded parser reads it かた, and the tie-break now asks it about this occurrence.
///
/// Tatoeba via Jitendex, filed under 方【かた】: "The boy knows how to throw a curve."
#[test]
fn a_tie_is_broken_by_the_word_in_its_sentence() {
    const TEXT: &str = "その少年はカーブの投げ方を知っている。";
    let analysis = analyze("nagekata", TEXT);
    assert_eq!(reading_of(&analysis, "方"), "かた");

    // The half that shows the context did it: the same tie broken by the bare word's reading.
    let bare = convert::to_analysis_with(TEXT, &segments_of("nagekata"), &|_| Some("ほう".into()));
    assert_eq!(reading_of(&bare, "方"), "ほう");
}

/// The same for a prefix: 前 ties between まえ and ぜん, and in 前学期 the sentence says ぜん.
///
/// Tatoeba via Jitendex, filed under 前【ぜん】: "I had a good report last term."
#[test]
fn a_prefix_tie_is_read_in_its_sentence() {
    let analysis = analyze("zengakki", "前学期は成績がよかった。");
    assert_eq!(reading_of(&analysis, "前"), "ぜん");
}

/// **Where the parser has no token at the tied word's span in the sentence, the bare question still
/// answers.** It reads 大口叩き as one unit, so there is nothing at 大 to ask; alone, 大 reads
/// おお, which is right here.
///
/// Tatoeba via Jitendex, filed under 大【おお】: "Great talkers are little doers."
#[test]
fn a_tie_the_sentence_cannot_answer_falls_back_to_the_bare_word() {
    let analysis = analyze("ooguchi", "大口叩きは何もできない。");
    assert_eq!(reading_of(&analysis, "大"), "おお");
}

/// A preference naming nothing tied gives way to the next, rather than ending the search.
#[test]
fn a_preference_matching_no_candidate_gives_way_to_the_next() {
    let analysis = convert::to_analysis_resolving(TEXT, &segments_of("harebare"), &|_| {
        vec!["はればれしい".into(), "はればれ".into()]
    });
    assert_eq!(reading_of(&analysis, "晴々"), "はればれ");
}

/// **Regression controls: what the provisional contextual tie-break costs.**
///
/// Three heteronym-fixture ties the bare-word rule got right and the sentence now gets wrong —
/// the change's known price, recorded here rather than
/// left in a table. Each is asserted **both ways over the same raw output**: the bare-word rule
/// gives the dictionary's reading, the shipped rule does not. A change that repairs one of them
/// will fail here, and must update this record on purpose; one that silently regresses the old
/// rule will fail too.
///
/// Tatoeba via Jitendex, each filed under the reading named as gold:
/// - "She was wearing a thick coat against the chill." — 寒気【かんき】
/// - "The 'gold' yearned for by the Japanese synchronized swimming world was not quite reached."
///   — 金【きん】
/// - "All that glitters is not gold." — 金【きん】
#[test]
fn the_contextual_tie_break_costs_these_three_readings() {
    use sakatsu_core::analyzer::ichiran::{self, TiePolicy};
    for (fixture, text, word, gold, shipped) in [
        (
            "kanki",
            "彼女は寒気を防ぐために厚いコートを着ていた。",
            "寒気",
            "かんき",
            "さむけ",
        ),
        (
            "kin-shinkuro",
            "日本シンクロ界の悲願である金には、あと一歩で届かなかった。",
            "金",
            "きん",
            "かね",
        ),
        (
            "kin-kagayaku",
            "輝くものすべてが金とは限らない。",
            "金",
            "きん",
            "かね",
        ),
    ] {
        let segments = segments_of(fixture);
        let before = ichiran::to_analysis_under(text, &segments, TiePolicy::BareWord);
        assert_eq!(
            reading_of(&before, word),
            gold,
            "{fixture}: the bare-word rule"
        );
        let now = ichiran::to_analysis_under(text, &segments, TiePolicy::SHIPPED);
        assert_eq!(
            reading_of(&now, word),
            shipped,
            "{fixture}: the shipped rule's known cost; gold is {gold}"
        );
    }
}

/// And the gains, over the same raw output, so the trade is pinned from both sides.
#[test]
fn the_contextual_tie_break_gains_these_readings_over_the_bare_word() {
    use sakatsu_core::analyzer::ichiran::{self, TiePolicy};
    for (fixture, text, word, before, now) in [
        (
            "nagekata",
            "その少年はカーブの投げ方を知っている。",
            "方",
            "ほう",
            "かた",
        ),
        ("zengakki", "前学期は成績がよかった。", "前", "まえ", "ぜん"),
    ] {
        let segments = segments_of(fixture);
        let old = ichiran::to_analysis_under(text, &segments, TiePolicy::BareWord);
        assert_eq!(
            reading_of(&old, word),
            before,
            "{fixture}: the bare-word rule"
        );
        let shipped = ichiran::to_analysis_under(text, &segments, TiePolicy::SHIPPED);
        assert_eq!(
            reading_of(&shipped, word),
            now,
            "{fixture}: the shipped rule"
        );
    }
}

const TEXT: &str = "晴々とした気分";

/// 景観・社会・文化・報道 (UD_Japanese-GSD) — the separator Ichiran hands back as a space.
///
/// `・` does not come back as `・`. Ichiran normalises it to a single ASCII space, exactly as it
/// normalises 「 to `"` and 、 to `,`, and the adapter used to discard the segment on the spot for
/// being whitespace. The character then belonged to no token: the reader saw it, could not select
/// it, got no lookup and no furigana, and nothing said so. It cost 26 separators across 17 of the
/// 259 UD_Japanese-GSD sentences measured — every one of which the hybrid comparison had to throw
/// out for source covered by no token.
///
/// It cannot be grounded by searching, either: the surface to look for is `" "`, and the next
/// space in the source is somewhere else entirely. The span between its grounded neighbours is
/// the only thing that identifies it.
#[test]
fn a_separator_normalised_to_a_space_is_restored_from_the_source() {
    let analysis = analyze("nakaguro", "景観・社会・文化・報道");
    assert_eq!(
        surfaces(&analysis),
        ["景観", "・", "社会", "・", "文化", "・", "報道"]
    );

    // Every token grounded, and grounded to the characters it claims.
    for token in &analysis.tokens {
        let (start, end) = (token.start.expect("start"), token.end.expect("end"));
        assert_eq!(&analysis.text[start..end], token.surface);
    }

    // The whole source is covered — the property whose absence caused the exclusions, checked
    // with the function that does the excluding rather than a restatement of it.
    assert!(
        sakatsu_core::hybrid::validate(&analysis).is_empty(),
        "no span fault: {:?}",
        sakatsu_core::hybrid::validate(&analysis)
    );

    // A separator is not vocabulary. It is shown and selectable; it is not a word to study.
    for token in analysis.tokens.iter().filter(|t| t.surface == "・") {
        assert!(!token.is_word(), "・ is punctuation, not vocabulary");
        assert!(token.senses.is_empty());
    }
}

/// The control for that repair: a space that really is a space still yields no token.
///
/// The source is those same UD_Japanese-GSD words with the separators replaced by spaces, because
/// the treebank contains no space between two Japanese characters anywhere in `test` or `dev` —
/// so this case cannot be captured from it, only constructed from it. The segments are the real
/// captured ones: Ichiran returns `" "` either way, which is precisely why the echo cannot decide
/// this and the source must.
#[test]
fn a_space_that_really_is_a_space_still_yields_no_token() {
    let segments = segments_of("nakaguro");
    let analysis = sakatsu_core::analyzer::ichiran::to_analysis("景観 社会 文化 報道", &segments);
    assert_eq!(surfaces(&analysis), ["景観", "社会", "文化", "報道"]);
    assert!(
        analysis
            .tokens
            .iter()
            .all(|token| !token.surface.trim().is_empty()),
        "a blank token would put a column of gaps down the reader"
    );
    assert!(sakatsu_core::hybrid::validate(&analysis).is_empty());
}

/// 映像もYou Tubeから削除しました (UD_Japanese-GSD) — pass-through text with a space inside it.
///
/// `You Tube` comes back as one unanalysed segment, space and all. It is not whitespace-only, so
/// it grounds literally and must survive intact: the repair above removes blank tokens, and this
/// pins that it removes only blank ones.
#[test]
fn pass_through_text_containing_a_space_survives_whole() {
    let analysis = analyze("interior-space", "映像もYou Tubeから削除しました");
    assert!(
        surfaces(&analysis).contains(&"You Tube"),
        "got {:?}",
        surfaces(&analysis)
    );
    for token in &analysis.tokens {
        let (start, end) = (token.start.expect("start"), token.end.expect("end"));
        assert_eq!(&analysis.text[start..end], token.surface);
    }
    assert!(sakatsu_core::hybrid::validate(&analysis).is_empty());
}

fn segments_of(fixture: &str) -> Vec<raw::Segment> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ichiran")
        .join(format!("{fixture}.json"));
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    raw::parse(&json).expect("fixture parses as ichiran output")
}

fn reading_of<'a>(analysis: &'a Analysis, surface: &str) -> &'a str {
    analysis
        .tokens
        .iter()
        .find(|token| token.surface == surface)
        .unwrap_or_else(|| panic!("{surface} is a token"))
        .reading
        .as_str()
}
