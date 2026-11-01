//! **A stand-in for `ichiran-cli`, for the deadline tests. Not part of the app.**
//!
//! `core/tests/ichiran_deadline.rs` points [`IchiranAnalyzer`] at this binary to make the sidecar
//! misbehave on purpose — something the real one cannot be asked to do, and something that must
//! never be done to a reader's own installation to find out what happens.
//!
//! It speaks both routes the adapter uses: one process per passage (`… -f -- <text>`), and the
//! persistent session (`… -e (load "<driver>")`, then length-framed passages on stdin, one JSON
//! line back per frame). `--mode=` chooses the misbehaviour:
//!
//! | mode | one-shot | session |
//! |---|---|---|
//! | `answer` | prints a real `ichiran-cli -f` answer | answers every frame |
//! | `silent` | never answers | reads every frame, never answers |
//! | `deaf` | never answers | never reads stdin at all |
//! | `malformed` | prints something that is not JSON | answers every frame with it |
//! | `stray` | prints a real answer | in the **first** session only (counted through `FAKE_ICHIRAN_MARK`), prints one line the driver never wrote before its first answer — a backtrace frame — then answers every frame |
//! | `broken` | prints a real answer | the same, but the extra line **begins with `[`** and is not a readable answer, so a check on the first character alone would accept it |
//!
//! Answers depend on the passage — 気持ち gets the `kimochi` fixture, anything else `mizu-o-nomu`
//! — so a test can tell a passage's own answer from one queued for another.
//!
//! With `FAKE_ICHIRAN_MARK` set, every start appends `<mode> <route>` to that file, so a test can
//! prove which routes were — and were not — started. Nothing waits forever: a stalled fake exits
//! after thirty seconds, so a test that fails to kill one does not leave it behind for good.
//!
//! [`IchiranAnalyzer`]: sakatsu_core::analyzer::IchiranAnalyzer

use std::io::{BufRead, Write};
use std::time::Duration;

/// Real `ichiran-cli -f` output for 水を飲む, so an answer parses and converts like the real thing.
const ANSWER: &str = include_str!("../../tests/fixtures/ichiran/mizu-o-nomu.json");
/// And for 気持ち, so two passages have two different answers.
const KIMOCHI: &str = include_str!("../../tests/fixtures/ichiran/kimochi.json");

/// The answer for this passage, on one line.
fn answer_for(passage: &str) -> String {
    if passage.trim() == "\u{6c34}" {
        let mut answer: serde_json::Value = serde_json::from_str(ANSWER).expect("fixture JSON");
        answer[0][0][0]
            .as_array_mut()
            .expect("fixture tokens")
            .truncate(1);
        return serde_json::to_string(&answer).expect("fixture JSON");
    }
    let json = if passage.trim() == "気持ち" {
        KIMOCHI
    } else {
        ANSWER
    };
    json.chars().filter(|c| *c != '\n' && *c != '\r').collect()
}

fn stall() -> ! {
    std::thread::sleep(Duration::from_secs(30));
    std::process::exit(3);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--mode="))
        .unwrap_or("answer")
        .to_string();
    let session = args.iter().any(|arg| arg == "-e");
    let mut sessions_started = 0usize;
    if let Some(mark) = std::env::var_os("FAKE_ICHIRAN_MARK") {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&mark)
        {
            let _ = writeln!(
                file,
                "{mode} {}",
                if session { "session" } else { "one-shot" }
            );
        }
        sessions_started = std::fs::read_to_string(&mark)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.ends_with("session"))
            .count();
    }
    if !session {
        let passage = args.last().cloned().unwrap_or_default();
        match mode.as_str() {
            "answer" | "stray" | "broken" => println!("{}", answer_for(&passage)),
            "malformed" => println!("this is not json"),
            _ => stall(),
        }
        return;
    }

    if mode == "deaf" {
        stall();
    }
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut out = std::io::stdout();
    // `stray` and `broken` misbehave in the first session only, so a replacement can be seen to be
    // clean.
    let mut first = sessions_started <= 1;
    // A frame is a character count on one line, then the passage and a newline. The passages these
    // tests send hold no newline, so the passage is the next line.
    while let (Some(Ok(_header)), Some(Ok(passage))) = (lines.next(), lines.next()) {
        let answer = answer_for(&passage);
        match mode.as_str() {
            "answer" => {
                let _ = writeln!(out, "{answer}");
            }
            "malformed" => {
                let _ = writeln!(out, "this is not json");
            }
            "stray" => {
                if first {
                    let _ = writeln!(out, "0: (SB-KERNEL::CONTROL-STACK-EXHAUSTED-ERROR)");
                }
                let _ = writeln!(out, "{answer}");
            }
            "broken" => {
                if first {
                    let _ = writeln!(out, "[broken");
                }
                let _ = writeln!(out, "{answer}");
            }
            _ => continue,
        }
        let _ = out.flush();
        first = false;
    }
}
