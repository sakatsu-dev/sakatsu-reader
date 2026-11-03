//! The same grammar, whichever analyzer read the sentence.
//!
//! # Why this exists
//!
//! The two analyzers segment differently, and the grammar a reader was shown depended on it.
//! Ichiran keeps 来そうにない, よね, だけど and なんだ as single words, so rules over whole tokens
//! could not reach the そうにない, the ね, the けど or the explanatory なん inside them — the
//! built-in analyzer showed them and Ichiran, the engine the app prefers, did not. In the other
//! direction the built-in analyzer splits どこか, なんか, 急に and でも, so a lone particle rule
//! claimed the か, the に or the で and explained a question marker, a destination or a place
//! that was not there.
//!
//! Each case here is checked under **both** analyzers through [`grammar::displayed`], the
//! function the reader's panel is built from: the built-in analyzer directly, Ichiran through its
//! real output captured by `scripts/capture-ichiran-fixtures.sh` — so this gates the Ichiran path
//! in an ordinary run, without Ichiran installed.
//!
//! The sentences are Tatoeba's (CC BY 2.0 FR), chosen by reading the English; the number on each
//! case is the Tatoeba sentence id, and the capture script records what each one is for.

mod common;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::grammar;
use sakatsu_core::model::Analysis;

struct Case {
    /// The captured Ichiran fixture, and the Tatoeba sentence it holds.
    fixture: &'static str,
    text: &'static str,
    /// Grammar points the reader must be shown.
    finds: &'static [&'static str],
    /// Grammar points that would misexplain this sentence.
    never: &'static [&'static str],
}

const CASES: &[Case] = &[
    // #86361 "It isn't likely that she will come." そうにない is a part of Ichiran's 来そうにない.
    Case {
        fixture: "grammar-sou-ni-nai",
        text: "彼女は来そうにない。",
        finds: &["hanabira-n1-verb-sou-ni-nai"],
        never: &[],
    },
    // #10614923 "You are joking, right?"
    Case {
        fixture: "grammar-yone",
        text: "冗談だよね？",
        finds: &["particle-yo", "particle-ne"],
        never: &[],
    },
    // #171741 "It is very cold today, isn't it?"
    Case {
        fixture: "grammar-wane",
        text: "今日はとても寒いわね。",
        finds: &["particle-ne"],
        never: &[],
    },
    // #13074558 "Japanese is difficult."
    Case {
        fixture: "grammar-yona",
        text: "日本語って難しいよな。",
        finds: &["particle-yo"],
        never: &[],
    },
    // #10733588 "Does your arm hurt?"
    Case {
        fixture: "grammar-noka",
        text: "腕が痛いのか？",
        finds: &["particle-no", "particle-ka"],
        never: &[],
    },
    // #81561 "Is that true?"
    Case {
        fixture: "grammar-nano",
        text: "本当なの？",
        finds: &["particle-no"],
        never: &[],
    },
    // #10006250 "I'm anemic." The explanatory なん, fused by Ichiran into one auxiliary.
    Case {
        fixture: "grammar-nandesu",
        text: "貧血なんです。",
        finds: &[grammar::EXPLANATORY_NAN],
        never: &[],
    },
    // #347294 "What's this?" Control: なん is the pronoun "what", not an explanation.
    Case {
        fixture: "grammar-nanda-what",
        text: "これはなんだ？",
        finds: &[],
        never: &[grammar::EXPLANATORY_NAN],
    },
    // #11483472 "Snow is pretty, but it's cold." だけど joining two clauses is the copula and
    // けど; the sentence-opening だけど entry would misdescribe it.
    Case {
        fixture: "grammar-dakedo-medial",
        text: "雪はきれいだけど、寒い。",
        finds: &["kedo"],
        never: &["hanabira-n3-dakedo"],
    },
    // #12486177 "However, there's a problem."
    Case {
        fixture: "grammar-dakedo-initial",
        text: "だけど、問題があります。",
        finds: &["hanabira-n3-dakedo"],
        never: &[],
    },
    // #202115 "But they are all people." でも opening a sentence is "but", not で + も.
    Case {
        fixture: "grammar-demo-initial",
        text: "でも、みんな人間です。",
        finds: &["hanabira-n5-a-demo-b"],
        never: &["particle-de", "particle-mo"],
    },
    // #85538 "She's also a beauty." Control: here it is で and も, and not "but".
    Case {
        fixture: "grammar-demo-also",
        text: "美人でもある。",
        finds: &[],
        never: &["hanabira-n5-a-demo-b"],
    },
    // #201037 "Let's meet up somewhere." The か of どこか is not a question marker.
    Case {
        fixture: "grammar-dokoka",
        text: "どこかで会いましょう。",
        finds: &["hanabira-n5-dokoka"],
        never: &["particle-ka"],
    },
    // #4789 "Why do you ask?" The で of なんで marks no place or means.
    Case {
        fixture: "grammar-nande",
        text: "なんで聞くの？",
        finds: &["hanabira-n5-nande"],
        never: &["particle-de"],
    },
    // #1490034 "I hate snow!"
    Case {
        fixture: "grammar-nanka",
        text: "雪なんか嫌いだ！",
        finds: &["nanka"],
        never: &["particle-ka"],
    },
    // #2250894 "He quit without notice." The に of 急に makes an adverb; it marks no destination.
    Case {
        fixture: "grammar-kyuu-ni",
        text: "彼は急にやめた。",
        finds: &["hanabira-n5-adjective-verb"],
        never: &["particle-ni"],
    },
    // #11831595 "What month is it now?"
    Case {
        fixture: "grammar-kke",
        text: "今何月だっけ？",
        finds: &["hanabira-n3-kke"],
        never: &[],
    },
    // #183185 "I think I'm going to faint." A verb's stem and そう.
    Case {
        fixture: "grammar-sou-verb",
        text: "気絶しそうだ。",
        finds: &["hanabira-n4-sou-da"],
        never: &[],
    },
    // #9478098 "You look busy." An adjective's stem and そう, with no copula after it.
    Case {
        fixture: "grammar-sou-adjective",
        text: "忙しそうね。",
        finds: &["hanabira-n4-sou-da"],
        never: &[],
    },
    // #2171510 "That's right." Control: そう is the reply "so", with no stem before it.
    Case {
        fixture: "grammar-sou-desu",
        text: "そうです。",
        finds: &[],
        never: &["hanabira-n4-sou-da"],
    },
    // #9524715 "You can say that again." Control: そう after an adverb.
    Case {
        fixture: "grammar-sou-hontou",
        text: "本当にそうだね。",
        finds: &[],
        never: &["hanabira-n4-sou-da"],
    },
    // #12156144 "I don't think I can." そうもない as a whole, not 〜そうだ and a stray も.
    Case {
        fixture: "grammar-sou-mo-nai",
        text: "できそうもない。",
        finds: &["hanabira-n1-sou-mo-nai"],
        never: &["hanabira-n4-sou-da"],
    },
    // #10976114 "I was really tired." 本当 is not a な-adjective: its に is the particle (UD tags
    // it 格助詞), explained by the particle's adverb sense rather than by the な-adjective entry.
    Case {
        fixture: "grammar-hontou-ni",
        text: "本当に疲れた。",
        finds: &[],
        never: &["hanabira-n5-adjective-verb"],
    },
    // #1619851 "How about tomorrow night?" One とか is an example, not と and a question.
    Case {
        fixture: "grammar-toka",
        text: "明日の夜とかどう？",
        finds: &["hanabira-n2-to-ka"],
        never: &["particle-to", "particle-ka"],
    },
    // #10580672 "We'll manage somehow." One word.
    Case {
        fixture: "grammar-nantoka",
        text: "なんとかなるさ。",
        finds: &[],
        never: &["particle-to", "particle-ka"],
    },
    // #8838408 "Are you Tom by any chance?" One word.
    Case {
        fixture: "grammar-moshikashite",
        text: "もしかして、トムじゃない？",
        finds: &[],
        never: &["particle-ka"],
    },
    // #172252 "It is especially cold this morning." One word.
    Case {
        fixture: "grammar-tokuni",
        text: "今朝は特に寒い。",
        finds: &[],
        never: &["particle-ni"],
    },
    // #8579928 "Both of us ought to do that." Control: ふたりとも is "both", not たりとも.
    Case {
        fixture: "grammar-futari-tomo",
        text: "ふたりともそうすべきだよ。",
        finds: &[],
        never: &["hanabira-n1-tari-tomo-nai"],
    },
    // #3643242 "Which do you prefer, dogs or cats?"
    Case {
        fixture: "grammar-docchi-ga",
        text: "犬と猫どっちが好き？",
        finds: &["hanabira-n5-a-to-b-to-docchi"],
        never: &[],
    },
    // #10156114 "They're both good." Control: どっちも is "both", not a choice between two.
    Case {
        fixture: "grammar-docchi-mo",
        text: "どっちもいいね。",
        finds: &[],
        never: &["hanabira-n5-a-to-b-to-docchi"],
    },
    // #205709 "And, we get each other's company to boot." それに opening a sentence, before a comma.
    Case {
        fixture: "grammar-soreni-comma",
        text: "それに、みんなに仲間もできるしね。",
        finds: &["hanabira-n4-bun-a-soreni-bun-b"],
        never: &[],
    },
    // #205634 "Don't touch it." Control: それ and に, "on it".
    Case {
        fixture: "grammar-soreni-touch",
        text: "それに触るな。",
        finds: &[],
        never: &["hanabira-n4-bun-a-soreni-bun-b"],
    },
    // #3367124 "Something's wrong, isn't it?" なんか as "something".
    Case {
        fixture: "grammar-nanka-something",
        text: "なんかおかしくない？",
        finds: &["nanka"],
        never: &["particle-ka"],
    },
    // #138220 "Stay thin." よう is a な-adjective stem before the copula's に, but ように is its
    // own pattern, and the な-adjective entry would misdescribe it. Neither analyzer shows a
    // ように card: the rule for one fired on every ように, a quarter of them uses its entry does
    // not teach, and was quarantined (pack_authoring's REVIEWED_OUT).
    Case {
        fixture: "grammar-you-ni",
        text: "太らないように。",
        finds: &[],
        never: &["hanabira-n5-adjective-verb", "particle-ni"],
    },
    // #190255 "Do not stay away from English, even for a day."
    Case {
        fixture: "grammar-tari-tomo",
        text: "一日たりとも英語から離れるな。",
        finds: &["hanabira-n1-tari-tomo-nai"],
        never: &[],
    },
    // #176705 "The crowd calmed down." 静か + に + なる.
    Case {
        fixture: "grammar-na-naru",
        text: "群衆は静かになった。",
        finds: &["hanabira-n5-na-adjective-ni-narimasu"],
        never: &[],
    },
    // #91184 "She became a doctor." Control: the noun's に is the particle, no な-adjective.
    Case {
        fixture: "grammar-noun-naru",
        text: "彼女は医者になった。",
        finds: &[],
        never: &[
            "hanabira-n5-na-adjective-ni-narimasu",
            "hanabira-n5-adjective-verb",
        ],
    },
    // #12662593 "I'm not hungry. I just had breakfast." Control: this なんで is なので.
    Case {
        fixture: "grammar-nande-because",
        text: "お腹は空いてないです。朝ごはん食べたばっかりなんで。",
        finds: &[],
        never: &["hanabira-n5-nande"],
    },
    // #76323 "I'm not getting involved in this." Control: なんで after a word is "since".
    Case {
        fixture: "grammar-nande-after-word",
        text: "俺この件に関してノータッチなんで。",
        finds: &[],
        never: &["hanabira-n5-nande"],
    },
    // #8612406 "Don't get so angry." Control: そんなに is no な-adjective.
    Case {
        fixture: "grammar-sonnani",
        text: "そんなに怒るなよ。",
        finds: &[],
        never: &["hanabira-n5-adjective-verb"],
    },
    // #139686 "I managed to get in." Control: 何とか is one word, with no とか inside it.
    Case {
        fixture: "grammar-kanji-nantoka",
        text: "何とか入れた。",
        finds: &[],
        never: &["hanabira-n2-to-ka", "particle-to", "particle-ka"],
    },
    // #1484936 "Tom had plenty of chances to apologize, but he didn't." いくらでも is one word.
    Case {
        fixture: "grammar-ikurademo",
        text: "トムは謝るチャンスがいくらでもあったのに、謝らなかった。",
        finds: &["noni"],
        never: &["particle-de", "particle-mo"],
    },
    // #11178798 "Huh, I wanted to send a reply but it wouldn't let me." はぁ is a sigh.
    Case {
        fixture: "grammar-haa",
        text: "はぁ。返事が送りたかったんですが、送れませんでした。",
        finds: &[],
        never: &["particle-wa"],
    },
    // #9447696 "Do it this way." Control: こんな風に is the noun 風, "manner".
    Case {
        fixture: "grammar-konna-fuu-ni",
        text: "こんな風にやってね。",
        finds: &[],
        never: &["hanabira-n5-adjective-verb"],
    },
    // #169456 "See to it that you do not leave your umbrella behind." Control: 様に is ように.
    Case {
        fixture: "grammar-you-ni-kanji",
        text: "傘を忘れない様にしなさい。",
        finds: &[],
        never: &["hanabira-n5-adjective-verb"],
    },
];

/// Cases only the built-in analyzer can show, each because Ichiran reads the span as one
/// dictionary word: the vocabulary answer explains it there, and there is no part to match.
const BUILT_IN_ONLY: &[Case] = &[
    // #3662503 "You look sleepy." Ichiran's 眠そう is the な-adjective "sleepy-looking".
    Case {
        fixture: "grammar-sou-sleepy",
        text: "眠そうだね。",
        finds: &["hanabira-n4-sou-da"],
        never: &[],
    },
    // #103147 "He walked quietly." Ichiran's 静かに is one adverb; the built-in analyzer has the
    // copula's に after the な-adjective 静か, which UD Japanese-GSD tags 助動詞 as well.
    Case {
        fixture: "grammar-shizuka-ni",
        text: "彼は静かに歩いた。",
        finds: &["hanabira-n5-adjective-verb"],
        never: &[],
    },
];

fn shown(analysis: &Analysis) -> Vec<String> {
    grammar::displayed(analysis)
        .into_iter()
        .map(|found| found.pattern_id)
        .collect()
}

fn check(engine: &str, text: &str, found: &[String], case: &Case, failures: &mut Vec<String>) {
    for wanted in case.finds {
        if !found.iter().any(|id| id == wanted) {
            failures.push(format!(
                "{engine}: {text} should show {wanted}, shows {found:?}"
            ));
        }
    }
    for wrong in case.never {
        if found.iter().any(|id| id == wrong) {
            failures.push(format!(
                "{engine}: {text} must not show {wrong}, shows {found:?}"
            ));
        }
    }
}

/// Every case, under both analyzers, reported together so one run names every disagreement.
#[test]
fn both_analyzers_show_the_same_grammar_for_each_sentence() {
    let lindera = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let mut failures = Vec::new();
    for case in CASES {
        let built_in = lindera.analyze(case.text).expect("analyses");
        check(
            "built-in",
            case.text,
            &shown(&built_in),
            case,
            &mut failures,
        );
        let ichiran = common::analyze(case.fixture, case.text);
        check("ichiran", case.text, &shown(&ichiran), case, &mut failures);
    }
    for case in BUILT_IN_ONLY {
        let built_in = lindera.analyze(case.text).expect("analyses");
        check(
            "built-in",
            case.text,
            &shown(&built_in),
            case,
            &mut failures,
        );
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// に after 本当 or 一緒 is the case particle — UD Japanese-GSD tags it 格助詞 — but it makes an
/// adverb of the word, and that is the reading the particle card must lead with. Without the
/// cue it led with "where something ends up".
#[test]
fn the_particle_after_hontou_and_issho_reads_as_making_an_adverb() {
    use sakatsu_core::grammar::particles;
    let lindera = LinderaAnalyzer::new().expect("embedded UniDic loads");
    // #10976114 "I was really tired." / #10550166 "Let's sing together." / control: #5070732
    // "I want to go to Japan someday." — 日本に is a destination, and must not read as an adverb.
    // (Which destination-like sense leads there depends on the WordNet table, absent here.)
    for (text, adverb) in [
        ("本当に疲れた。", true),
        ("一緒に歌おう。", true),
        ("いつか日本に行きたい。", false),
    ] {
        let tokens = lindera.analyze(text).expect("analyses").tokens;
        let index = tokens
            .iter()
            .position(|token| token.surface == "に")
            .unwrap_or_else(|| panic!("{text}: no に token in {tokens:?}"));
        let reading = particles::senses(&tokens, index, &particles::Categories::default())
            .unwrap_or_else(|| panic!("{text}: no reading for に"));
        let top = reading
            .favoured
            .map(|at| reading.senses[at].id.as_str())
            .unwrap_or("none");
        assert_eq!(top == "ni-adverb", adverb, "{text}: {:?}", reading.senses);
    }
}

/// The reader's view adds to what lexical grouping and the expression offer see, and hides only
/// what lies inside a split word, such as the は of a sigh split as は / ぁ. A pattern found inside
/// one word must not become a run anything can join.
#[test]
fn the_whole_token_patterns_are_unchanged_by_the_readers_view() {
    for case in CASES {
        let ichiran = common::analyze(case.fixture, case.text);
        let whole = grammar::patterns(&ichiran);
        let shown = grammar::displayed(&ichiran);
        for found in &whole {
            let split_word =
                grammar::inside::without_split_words(&ichiran.tokens, vec![found.clone()])
                    .is_empty();
            assert!(
                shown.contains(found) || split_word,
                "{}: {found:?} dropped",
                case.text
            );
        }
        for found in shown.iter().filter(|found| !whole.contains(found)) {
            assert!(
                found.elements.is_empty() && found.token_count() == 1,
                "{}: an inside match must sit on one token with no rule elements: {found:?}",
                case.text
            );
        }
    }
}

/// The panel is built from the study view, so that is where the reader's view has to arrive.
/// Every other test here calls [`grammar::displayed`] directly, and would stay green if the study
/// view went back to the whole-token patterns alone.
#[test]
fn the_study_view_carries_the_grammar_inside_a_word() {
    let lindera = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let text = "彼女は来そうにない。";
    let view = sakatsu_core::study::from_analysis(
        common::analyze("grammar-sou-ni-nai", text),
        sakatsu_core::study::Resources {
            analyzer: &lindera,
            dictionaries: &[],
            kanji: &sakatsu_core::dict::KanjiDic::default(),
            store: None,
        },
    );
    assert!(
        view.patterns
            .iter()
            .any(|found| found.pattern_id == "hanabira-n1-verb-sou-ni-nai"),
        "{:?}",
        view.patterns
    );
}
