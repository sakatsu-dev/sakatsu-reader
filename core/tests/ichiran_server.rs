//! The persistent sidecar must answer exactly what the one-shot one answers.
//!
//! ```text
//! cargo test -p sakatsu-core --test ichiran_server -- --ignored --nocapture
//! ```
//!
//! # Why equality is the whole test
//!
//! `analyzer/ichiran/server.rs` exists to make Ichiran faster, and speed is the easy half to
//! check. The half that matters is that it changed *when* the process starts and nothing about
//! what it says: the driver calls `(princ (jsown:to-json (romanize* input :limit 1)))`, which is
//! character-for-character what `ichiran-cli -f` runs in `~/ichiran/cli.lisp`, so the JSON on the
//! wire should be the same JSON and the [`Analysis`] built from it should compare equal.
//!
//! If that ever stops being true the optimisation has become a second analyzer with its own
//! opinions, which is the one outcome that would make it not worth having. `Analysis` is `Eq`, so
//! this asks the question directly rather than through a summary statistic.
//!
//! # The passages are chosen to break the framing, not to be representative
//!
//! Three of them exist only because a newline-delimited protocol would have got them wrong — a
//! multi-line passage, one holding a quotation mark and a backslash, and one that is a single
//! character. The rest are the casual register the corpus work is aimed at. This is not a sample
//! of anything and no rate should be computed from it.

use std::time::Instant;

use sakatsu_core::analyzer::ichiran::server;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::IchiranAnalyzer;

/// Passages that would expose a framing bug, followed by ordinary casual dialogue.
const PASSAGES: &[&str] = &[
    // A multi-line capture is one passage, and a line protocol would have analysed only 食べた.
    "食べた\n水を飲む",
    // Quote and backslash: nothing here escapes anything, and this is what proves it.
    "彼は\"やめろ\"と言った",
    // One character, so the frame header and the payload are both minimal.
    "水",
    // And the register the whole corpus effort is about.
    "食べちゃったよ",
    "約80分前にこのSUVに乗った",
    "文字通りじゃないんだ",
    "バレなきゃ大丈夫だって",
    "ここって禁足地なんじゃなかったっけ",
    "こうなっちまったら二度と元に戻ることはねぇ",
    "父上がおっしゃるならそうなのでしょう",
];

#[test]
#[ignore = "needs Ichiran installed"]
fn the_persistent_sidecar_answers_exactly_what_the_one_shot_one_answers() {
    let analyzer = IchiranAnalyzer::new();

    // One call to find out, rather than discovering it ten times.
    server::set_persistent(false);
    if analyzer.analyze("水").is_err() {
        eprintln!("SKIP: Ichiran did not answer — it needs WSL and its PostgreSQL database.");
        return;
    }

    // ── One process per passage, which is the reference ──────────────────────────────────────
    server::set_persistent(false);
    assert!(
        !server::persistent(),
        "the reference run must not be routed"
    );

    let started = Instant::now();
    let one_shot: Vec<_> = PASSAGES
        .iter()
        .map(|text| analyzer.analyze(text).expect("one-shot analyses"))
        .collect();
    let one_shot_seconds = started.elapsed().as_secs_f64();

    // ── The same passages through one live process ───────────────────────────────────────────
    server::set_persistent(true);
    assert!(server::persistent());

    let started = Instant::now();
    let persistent: Vec<_> = PASSAGES
        .iter()
        .map(|text| analyzer.analyze(text).expect("persistent analyses"))
        .collect();
    let persistent_seconds = started.elapsed().as_secs_f64();

    // **The session may have declined**, and then this test proves nothing about it. Saying so is
    // the difference between a green tick that means something and one that means the driver
    // failed to install and everything fell back to the path already being compared.
    let routed = server::persistent();
    server::set_persistent(false);

    let n = PASSAGES.len() as f64;
    println!("passages            {}", PASSAGES.len());
    println!(
        "one process each    {one_shot_seconds:.2}s total, {:.0} ms each",
        one_shot_seconds / n * 1000.0
    );
    println!(
        "one live process    {persistent_seconds:.2}s total, {:.0} ms each",
        persistent_seconds / n * 1000.0
    );
    if routed {
        println!(
            "speedup             {:.1}x",
            one_shot_seconds / persistent_seconds
        );
    }

    assert!(
        routed,
        "the live session switched itself off, so the two runs above are the same code path and \
         the comparison below proves nothing — check the sidecar log for why the driver failed"
    );

    for (index, (a, b)) in one_shot.iter().zip(&persistent).enumerate() {
        assert_eq!(
            a, b,
            "passage {index} ({:?}) analysed differently through the live session",
            PASSAGES[index]
        );
    }
}

/// A passage the sidecar cannot handle must not poison the session for the next one.
///
/// The driver reports a failed passage in-band as `{"error": …}` rather than dying, precisely so
/// the stream stays aligned. If it ever stops doing that, one bad capture would cost every
/// analysis after it until the app restarted — a far worse failure than the one it came from.
#[test]
#[ignore = "needs Ichiran installed"]
fn the_session_survives_a_passage_it_cannot_analyse() {
    let analyzer = IchiranAnalyzer::new();

    server::set_persistent(false);
    if analyzer.analyze("水").is_err() {
        eprintln!("SKIP: Ichiran did not answer — it needs WSL and its PostgreSQL database.");
        return;
    }

    server::set_persistent(true);
    let before = analyzer.analyze("水").expect("a plain passage analyses");

    // Not Japanese, and not anything Ichiran has an entry for. Whatever it does with this, the
    // next passage is what is being asked about.
    let _ = analyzer.analyze("!!!???...---");

    let after = analyzer
        .analyze("水")
        .expect("the session still answers afterwards");
    server::set_persistent(false);

    assert_eq!(
        before, after,
        "the same passage analysed differently before and after an awkward one, so the stream \
         desynchronised"
    );
}
