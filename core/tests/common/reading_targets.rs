//! Reviewed reading targets: what the displayed surface reading must be, and why.
//!
//! # Why this is a fixture and not a corpus
//!
//! The defects these pin were *found* in two novel chapters, and that text is copyrighted, private
//! evaluation material under an ignored directory. It is discovery data: it says which families are
//! broken, and it cannot be the evidence that they are fixed, because a rule tuned against the same
//! sentences that suggested it has been checked against nothing.
//!
//! So every case here is stated on an **independently authored sentence** — Tatoeba, bundled by
//! Jitendex, CC BY-SA 4.0 and already credited in `project notes` §8 — chosen by reading the English.
//! Two forms have no Tatoeba example at all and appear as the bare word, which is recorded in
//! `source` rather than quietly passed off as a harvested sentence.
//!
//! # Why the controls outnumber the targets
//!
//! Each target is one member of a family, and the family is what gets repaired; the danger is never
//! that the repair misses, it is that it reaches further than its evidence. So the controls are not
//! decoration — every one of them is a case that a slightly wider version of the same rule breaks,
//! and each `evidence` line says which wider rule it is there to defeat.

#![allow(dead_code)]

/// What kind of case this is, which decides what its failure means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A demonstrated defect. It was wrong, and it must now be right.
    Conversion,
    /// It was already right, and a wider rule would break it. It must not move.
    Control,
}

/// One reviewed expectation about one token.
#[derive(Debug, Clone, Copy)]
pub struct ReadingTarget {
    /// Stable case id, so a run can be diffed against the last one.
    pub id: &'static str,
    /// The passage the token is read in.
    pub text: &'static str,
    /// The token surface, exactly as the analyzer emits it.
    pub surface: &'static str,
    /// The reading it must have.
    pub reading: &'static str,
    pub kind: Kind,
    /// **Where the expectation comes from** — not that it looks right, but what says so.
    pub evidence: &'static str,
    /// Where the sentence comes from.
    pub source: &'static str,
}

const TATOEBA: &str = "Tatoeba via Jitendex, CC BY-SA 4.0";
const BARE: &str = "the bare form; no Tatoeba example exists for it";

pub const READING_TARGETS: &[ReadingTarget] = &[
    // ---- Potential forms: 語彙素 is the plain verb, 書字形基本形 is the potential one. ----
    ReadingTarget {
        id: "potential-hashireru",
        text: "これらの船は数日間低気圧の中にいることができるくらい速く走れる。",
        surface: "走れる",
        reading: "はしれる",
        kind: Kind::Conversion,
        evidence: "UniDic 発音形出現形 ハシレル against 語彙素 走る / 書字形基本形 走れる",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "potential-hashirenai",
        text: "世界最速の走者でさえ、空腹だったら走れない。",
        surface: "走れない",
        reading: "はしれない",
        kind: Kind::Conversion,
        evidence: "UniDic 発音形出現形 ハシレ on 走れ; the negative attaches unchanged",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "potential-kakeru",
        text: "誰もが名文を書けるわけではない。",
        surface: "書ける",
        reading: "かける",
        kind: Kind::Conversion,
        evidence: "UniDic 発音形出現形 カケル against 語彙素 書く",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "potential-modoreru",
        text: "戻れる",
        surface: "戻れる",
        reading: "もどれる",
        kind: Kind::Conversion,
        evidence: "UniDic 発音形出現形 モドレル against 語彙素 戻る",
        source: BARE,
    },
    // Potential forms that were **already correct**, by accident: the suffix strip failed and the
    // pronunciation fallback answered. They are controls because a rule that changed how the
    // fallback is reached would move them, and nothing should.
    ReadingTarget {
        id: "potential-oyogemasu",
        text: "あなたは泳げますよね。",
        surface: "泳げます",
        reading: "およげます",
        kind: Kind::Control,
        evidence: "already correct: およぐ has no る to strip, so the alignment already declined",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "potential-yomenai",
        text: "私はメガネがなくては読めない。",
        surface: "読めない",
        reading: "よめない",
        kind: Kind::Control,
        evidence: "already correct by the same declined alignment",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "potential-hanasemasu",
        text: "何ヶ国の言葉を話せますか。",
        surface: "話せます",
        reading: "はなせます",
        kind: Kind::Control,
        evidence: "already correct by the same declined alignment",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "potential-ikemasen",
        text: "ここから先には行けません。",
        surface: "行けません",
        reading: "いけません",
        kind: Kind::Control,
        evidence: "already correct by the same declined alignment",
        source: TATOEBA,
    },
    // ---- Contracted written variants: 突き込む is the lexeme, 突っ込む the written base. ----
    ReadingTarget {
        id: "contracted-tsukkomi",
        text: "彼女は何にでも首を突っ込みたがる。",
        surface: "突っ込み",
        reading: "つっこみ",
        kind: Kind::Conversion,
        evidence: "UniDic 発音形出現形 ツッコミ against 語彙素 突き込む; Jitendex lists つっこむ",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "contracted-tsukkomu",
        text: "人の問題に鼻を突っ込むのはよしてくれよ。",
        surface: "突っ込む",
        reading: "つっこむ",
        kind: Kind::Conversion,
        evidence: "Jitendex files 突っ込む under つっこむ; UniDic 発音形出現形 agrees",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "contracted-massaichuu",
        text: "真っ最中",
        surface: "真っ最中",
        reading: "まっさいちゅう",
        kind: Kind::Conversion,
        evidence: "UniDic 発音形出現形 マッ on the 接頭辞 真っ, against 語彙素 真",
        source: BARE,
    },
    // Contracted verbs whose lexeme **is** the written base. The repair must not reach them just
    // because they contain っ.
    ReadingTarget {
        id: "contract-control-hipparu",
        text: "この生地は引っ張るとすぐ伸びる。",
        surface: "引っ張る",
        reading: "ひっぱる",
        kind: Kind::Control,
        evidence: "語彙素 引っ張る == 書字形基本形, so the lexeme reading is already this word's",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "contract-control-hikkoshita",
        text: "去年の秋、私たちはニューヨークに引っ越した。",
        surface: "引っ越した",
        reading: "ひっこした",
        kind: Kind::Control,
        evidence: "語彙素 引っ越す == 書字形基本形",
        source: TATOEBA,
    },
    // ---- ずる / じる: a real pair of words, so the ヅ guard below must stay one-directional. ----
    ReadingTarget {
        id: "zuru-kanjiru",
        text: "私はときどき悲しく感じる。",
        surface: "感じる",
        reading: "かんじる",
        kind: Kind::Conversion,
        evidence: "語彙素 感ずる is a different verb; 発音形出現形 カンジル is this one",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "zuru-shinjiru",
        text: "私は霊魂の不滅を信じる。",
        surface: "信じる",
        reading: "しんじる",
        kind: Kind::Conversion,
        evidence: "語彙素 信ずる is a different verb; 発音形出現形 シンジル is this one",
        source: TATOEBA,
    },
    // ---- ヅ / ヂ: 発音形 normalizes them onto ズ / ジ and so cannot be trusted as a spelling. ----
    ReadingTarget {
        id: "dz-chikazuite",
        text: "お盆が近づいてきた。",
        surface: "近づいてきた",
        reading: "ちかづいてきた",
        kind: Kind::Control,
        evidence: "発音形出現形 says チカズイ; taking it would spell 近づく as ちかずく",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "dz-kizuita",
        text: "私は油断していたことに気づいた。",
        surface: "気づいた",
        reading: "きづいた",
        kind: Kind::Control,
        evidence: "発音形出現形 says キズイ; 気づく is written with ヅ",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "dz-katazuke",
        text: "おもちゃを片づけなさい。",
        surface: "片づけ",
        reading: "かたづけ",
        kind: Kind::Control,
        evidence: "発音形出現形 says カタズケ",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "dz-kozutsumi",
        text: "彼は小包を右脇に抱えている。",
        surface: "小包",
        reading: "こづつみ",
        kind: Kind::Control,
        evidence: "発音形出現形 says コズツミ; the compound voices 包 to ヅ in writing",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "dz-hizuke",
        text: "その手紙の日付はいつですか。",
        surface: "日付",
        reading: "ひづけ",
        kind: Kind::Control,
        evidence: "発音形出現形 says ヒズケ",
        source: TATOEBA,
    },
    // ---- Long vowels: 発音形 writes ー, which cannot be spelled back. ----
    ReadingTarget {
        id: "long-benkyou",
        text: "彼は勉強を怠けている。",
        surface: "勉強",
        reading: "べんきょう",
        kind: Kind::Control,
        evidence: "発音形出現形 says ベンキョー; the ー cannot say whether う or お follows",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "long-gakusei",
        text: "彼は法科の学生だ。",
        surface: "学生",
        reading: "がくせい",
        kind: Kind::Control,
        evidence: "発音形出現形 says ガクセー",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "long-koujou",
        text: "工場はフル稼動している。",
        surface: "工場",
        reading: "こうじょう",
        kind: Kind::Control,
        evidence: "発音形出現形 says コージョー — two long vowels in one word",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "long-kaijou",
        text: "私たちは会場の後ろに座った。",
        surface: "会場",
        reading: "かいじょう",
        kind: Kind::Control,
        evidence: "発音形出現形 says カイジョー",
        source: TATOEBA,
    },
    // ---- Irregulars: refused before any of this is reached, and must stay refused. ----
    ReadingTarget {
        id: "irregular-kite",
        text: "私と共に来てくれませんか。",
        surface: "来てくれません",
        reading: "きてくれません",
        kind: Kind::Control,
        evidence: "カ行変格 is declined outright; 来 is こ / き / く under one spelling",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "irregular-ritoushita",
        text: "彼は離党した。",
        surface: "離党した",
        reading: "りとうした",
        kind: Kind::Control,
        evidence: "サ行変格 is declined outright",
        source: TATOEBA,
    },
    // ---- Kana keeps its own spelling. ----
    ReadingTarget {
        id: "kana-koohii",
        text: "このコーヒーはぬるいです。",
        surface: "コーヒー",
        reading: "コーヒー",
        kind: Kind::Control,
        evidence: "a word already in kana is its own reading; こーひー would be wrong ruby",
        source: TATOEBA,
    },
    // ---- The measured overrides must survive: the pronunciation column is not a better source
    // ---- than the evidence that put them there.
    ReadingTarget {
        id: "modern-watashi",
        text: "私は学生です。",
        surface: "私",
        reading: "わたし",
        kind: Kind::Control,
        evidence: "発音形出現形 says ワタクシ; 129 measured occurrences put わたし first",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "modern-nihon",
        text: "日本",
        surface: "日本",
        reading: "にほん",
        kind: Kind::Control,
        evidence: "KWDLC reads 日本 as にほん 34 times out of 34",
        source: BARE,
    },
    // ---- A width difference in 語彙素 is not a different word. ----
    ReadingTarget {
        id: "width-latin-h",
        text: "h",
        surface: "h",
        reading: "えいち",
        kind: Kind::Control,
        evidence: "語彙素 Ｈ against 書字形基本形 h is one letter; 発音形 offers えっち",
        source: BARE,
    },
    // ---- Counted numbers: composition ran the digits' readings together and a number's parts
    // ---- do not spell it. `numbers::counted` assembles the place values and applies the counter.
    ReadingTarget {
        id: "counted-13-sai",
        text: "初潮は13歳の時でした。",
        surface: "13歳",
        reading: "じゅうさんさい",
        kind: Kind::Conversion,
        evidence: "place values: 13 is じゅうさん, not いち + さん",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "counted-15-fun",
        text: "栗は少なくとも15分は茹でなくてはいけない。",
        surface: "15分",
        reading: "じゅうごふん",
        kind: Kind::Conversion,
        evidence: "place values; 分 does not alternate after 5",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "counted-1963-nen",
        text: "ケニアは1963年に独立した。",
        surface: "1963年",
        reading: "せんきゅうひゃくろくじゅうさんねん",
        kind: Kind::Conversion,
        evidence: "four place values, including 900 as きゅうひゃく",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "counted-10-ka",
        text: "彼氏が留置所に入って10日経ちました。",
        surface: "10日",
        reading: "とおか",
        kind: Kind::Conversion,
        evidence: "十日 is its own word; no rule assembles とおか from ten and 日",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "counted-4-nin",
        text: "ケンは4人のうちで一番年下です。",
        surface: "4人",
        reading: "よにん",
        kind: Kind::Conversion,
        evidence: "人 takes よ after 4, not よん",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "counted-25-nin",
        text: "このマイクロバスは25人乗りです。",
        surface: "25人",
        reading: "にじゅうごにん",
        kind: Kind::Conversion,
        evidence: "place values; 人 does not alternate after 5",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "counted-8-satsu",
        text: "ポール、8冊までなら借りられるからね。",
        surface: "8冊",
        reading: "はっさつ",
        kind: Kind::Conversion,
        evidence: "冊 geminates 8 to はっ",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "counted-34-sai",
        text: "34歳",
        surface: "34歳",
        reading: "さんじゅうよんさい",
        kind: Kind::Conversion,
        evidence: "the report's own case; it was さんよんさい",
        source: BARE,
    },
    ReadingTarget {
        id: "counted-20-pun",
        text: "20分",
        surface: "20分",
        reading: "にじゅっぷん",
        kind: Kind::Conversion,
        evidence: "the report's own case; it was にぜろふん. 分 takes ぷん after a bare 十",
        source: BARE,
    },
    // Counted numbers that were **already right**, and the two rules that must not reach further.
    ReadingTarget {
        id: "counted-control-8-ji",
        text: "校門は8時に開く。",
        surface: "8時",
        reading: "はちじ",
        kind: Kind::Control,
        evidence: "時 does not geminate 8; a blanket か行 rule would give はっじ",
        source: TATOEBA,
    },
    ReadingTarget {
        id: "counted-control-percent",
        text: "５％",
        surface: "５％",
        reading: "ごぱーせんと",
        kind: Kind::Control,
        evidence: "an unrecorded counter keeps its own reading; no euphony is invented for it",
        source: BARE,
    },
    ReadingTarget {
        id: "counted-control-lone-digit",
        text: "４",
        surface: "４",
        reading: "よん",
        kind: Kind::Control,
        evidence: "a digit with nothing counting it is not composed at all",
        source: BARE,
    },
    ReadingTarget {
        id: "counted-control-yaku",
        text: "約80分",
        surface: "80分",
        reading: "はちじゅっぷん",
        kind: Kind::Conversion,
        evidence: "place values, with 約 staying outside the count rather than joining it",
        source: BARE,
    },
    // ---- The native series. These were already right, and the cardinal renderer very nearly
    // ---- overrode them: 一つ is ひとつ, never いちつ. Caught by measuring, guarded here.
    ReadingTarget {
        id: "counted-control-hitotsu",
        text: "１つ",
        surface: "１つ",
        reading: "ひとつ",
        kind: Kind::Control,
        evidence: "the ～つ counter uses the native numerals; Sino-Japanese いち is wrong here",
        source: BARE,
    },
    ReadingTarget {
        id: "counted-control-futatsu",
        text: "２つ",
        surface: "２つ",
        reading: "ふたつ",
        kind: Kind::Control,
        evidence: "same series; UniDic supplied ふつ contextually and the cardinal must not win",
        source: BARE,
    },
    ReadingTarget {
        id: "counted-control-mittsu",
        text: "３つ",
        surface: "３つ",
        reading: "みっつ",
        kind: Kind::Control,
        evidence: "same series",
        source: BARE,
    },
];
