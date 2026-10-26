//! The same passages through the built-in analyzer over each dictionary, side by side.
//!
//! A diagnostic, not a gate: it prints what each dictionary makes of passages whose handling is
//! already pinned elsewhere, so a change of dictionary can be read token by token. The passages
//! are the contraction cases `contraction_repair.rs` and `construction_annotations.rs` already
//! hold — verified examples, not text written for this file.
//!
//! ```text
//! $env:SAKATSU_LINDERA_SUDACHI = "$env:SAKATSU_EVAL_DIR\sudachidict\lindera-sudachidict-20260723"
//! cargo test -p sakatsu-core --test analyzer_dictionaries -- --ignored --nocapture
//! ```

mod common;

use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};

const PASSAGES: [&str; 3] = [
    "頭文字から間違っとるわい！！",
    "怒んなきゃ",
    "へえ今度アタシも連れてってよ",
];

fn show(name: &str, analyzer: &LinderaAnalyzer) {
    println!("=== {name}");
    for passage in PASSAGES {
        let analysis = analyzer.analyze(passage).expect("the passage analyses");
        let tokens: Vec<String> = analysis
            .tokens
            .iter()
            .map(|token| {
                format!(
                    "{}〔{}｜{}｜{}〕",
                    token.surface, token.dict_form, token.reading, token.lemma_reading
                )
            })
            .collect();
        println!("  {passage}\n    {}", tokens.join(" "));
    }
}

#[test]
#[ignore = "a diagnostic; set SAKATSU_LINDERA_SUDACHI to include SudachiDict"]
fn the_contraction_cases_under_each_dictionary() {
    show(
        "UniDic (embedded)",
        &LinderaAnalyzer::new().expect("compiled in"),
    );
    if std::env::var_os("SAKATSU_LINDERA_SUDACHI").is_some() {
        show(
            "SudachiDict",
            &common::built_in().expect("SudachiDict loads"),
        );
    }
}
