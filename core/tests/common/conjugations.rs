//! Inflected words and the derivation each one should carry.
//!
//! # This is a regression guard, not a validation, and the difference matters
//!
//! `common/heteronyms.rs` can claim more than this file can. Its ground truth comes from the
//! dictionary — a sentence filed under the sequence for 上【うえ】 is Jitendex saying 上 reads うえ
//! there — so it validates the reader against something nobody here wrote.
//!
//! **These expectations are written by hand, so they cannot validate anything.** Scoring the
//! analyzer against a table of what I believe about Japanese would measure my beliefs, which is
//! the circularity `pack_precision.rs` and `ud_gold.rs` both refuse.
//!
//! The validation lives in `kwdlc_gold::conjugation_correctness_against_juman`, which scores the
//! form chain against **JUMAN** — a morphological tradition neither this project nor UniDic
//! derives from — and measured 96.1% once the dictionary-form convention was set aside. That is
//! the number that says the analyzer is right.
//!
//! What this file does is stop it *becoming* wrong on a machine with nothing installed. KWDLC has
//! no licence and cannot be committed, so the measurement above is `#[ignore]`d and invisible to
//! an ordinary `cargo test` — which is the gap `project notes` names as the worst one here: "a machine
//! with nothing installed shows green while testing almost nothing." These cases need only the
//! embedded dictionary, so they run every time.
//!
//! # How the expectations were arrived at
//!
//! Written from the Japanese first, then compared with the analyzer. Where the two disagreed the
//! disagreement was investigated rather than settled by editing the expectation — that is the only
//! discipline that keeps a hand-written fixture from becoming a transcript of current behaviour.
//! 行きましょう is in here because it *was* a disagreement: the analyzer called it Non-past, and
//! `助動詞-マス` turned out to discard the auxiliary's own 意志推量形.

/// `(sentence, the word in it, every form its chain must name)`.
///
/// The sentence is there because context changes the analysis — 読んで alone and 読んでいる are not
/// the same token — so each case pins the word as it appears in a real clause.
///
/// A chain may name more than the expectation lists; the assertion is that everything listed is
/// present, not that nothing else is. 食べさせられた is genuinely causative-passive *and* past, and
/// demanding an exact set would make the test brittle against a derivation growing a step.
#[allow(dead_code)]
pub const CONJUGATIONS: &[(&str, &str, &[&str])] = &[
    // ── The plain tenses ────────────────────────────────────────────────────────────────────
    ("毎日本を読む。", "読む", &["Non-past"]),
    ("昨日本を読んだ。", "読んだ", &["Past (~ta)"]),
    ("本を読んで寝た。", "読んで", &["Conjunctive (~te)"]),
    // ── The conditionals, which UniDic files under one auxiliary and one 活用形 apart ────────
    // 助動詞-タ covers both 読んだ and 読んだら; only 仮定形 separates them, and calling every たら
    // a past tense was a real bug measured at 8 occurrences in 1,858 sentences.
    ("読めば分かる。", "読めば", &["Provisional (~eba)"]),
    ("本を読んだら寝る。", "読んだら", &["Conditional (~tara)"]),
    // ── Mood ────────────────────────────────────────────────────────────────────────────────
    ("一緒に本を読もう。", "読もう", &["Volitional"]),
    ("早く読め。", "読め", &["Imperative"]),
    ("本が読みたい。", "読みたい", &["Desiderative (~tai)"]),
    // ── Voice. The derivation is what a learner most needs named. ───────────────────────────
    ("子供に本を読ませる。", "読ませる", &["Causative"]),
    ("弟に本を読まれた。", "読まれた", &["Past (~ta)"]),
    (
        "先生に本を読ませられた。",
        "読ませられた",
        &["Causative-Passive", "Past (~ta)"],
    ),
    // ── Polite, and the bug that put this file here ─────────────────────────────────────────
    ("本を読みます。", "読みます", &["Non-past"]),
    ("本を読みました。", "読みました", &["Past (~ta)"]),
    // **The regression this exists for.** ます sets politeness and nothing else, so with no later
    // auxiliary the fallback named the form Non-past — 行きましょう came out 行く[Non-past] while
    // every plain volitional was right. Measured against KWDLC's 意志形 at 50% wrong.
    ("明日行きましょう。", "行きましょう", &["Volitional"]),
    // ── Negation, which folds into the head rather than standing as its own token ───────────
    ("本を読まない。", "読まない", &["Non-past"]),
    ("本を読まなかった。", "読まなかった", &["Past (~ta)"]),
    // ── An adjective's 連用形 is adverbial where a verb's is continuative ────────────────────
    // UniDic files both as 連用形 and only the part of speech separates them; calling 高く
    // continuative told a learner it linked clauses.
    ("空が高くなった。", "高く", &["Adverbial"]),
];
