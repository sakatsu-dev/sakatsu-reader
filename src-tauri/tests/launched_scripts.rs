//! The setup scripts have to survive the machine they land on.
//!
//! Everything here exists because of one bug found by running `setup-vlm.ps1` rather than by
//! reading it. The script was valid UTF-8 and parsed perfectly in PowerShell 7 — and would not
//! parse at all in Windows PowerShell 5.1, which is what the app actually spawns.
//!
//! **Windows PowerShell 5.1 reads a script with no byte-order mark in the machine's ANSI
//! codepage.** On a Japanese install that is CP932, where `0x9E` — the last byte of 語 in
//! UTF-8 — is a lead byte, so it swallowed the quote that closed the string after it. Every
//! line from there on was misparsed. No script in `scripts/` carries a BOM, and a reading tool
//! for Japanese has more users on `ja-JP` than most software does.
//!
//! It would have shipped. Nothing in the suite read the scripts, and the developer machine was
//! `pwsh`. So the shipped path is asserted here instead: that the scripts the app spawns need no
//! decoding at all, and that every script parses in the interpreter a user's machine will use.
//!
//! The script this was written for is gone — the vision reader stopped installing Ollama and
//! became an ordinary download — but the trap it found is a property of every `.ps1` here, and
//! `setup-ichiran.ps1` is still opened in a console the same way.

use std::path::{Path, PathBuf};

/// The scripts `src-tauri` spawns, as opposed to the ones a person is told to run themselves.
///
/// Launched with `powershell.exe` — Windows PowerShell 5.1, not `pwsh` — from a window the app
/// opens, where a parse error has nobody to read it and no way back.
// `setup-vlm.ps1` was the other one until the vision reader stopped installing Ollama and
// became an ordinary download. One console-opening script left.
const LAUNCHED: [&str; 1] = ["setup-ichiran.ps1"];

fn scripts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("scripts")
}

fn all_scripts() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(scripts_dir())
        .expect("scripts/ should exist beside src-tauri")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ps1"))
        .collect();
    found.sort();
    assert!(!found.is_empty(), "no .ps1 files found to check");
    found
}

/// The spawned scripts contain nothing that needs decoding, so no codepage can corrupt them.
///
/// A byte-order mark would also fix it, but no script here has one and adding it to some of them
/// would leave a convention that is true of most files and silently load-bearing on a few. ASCII
/// is checkable without knowing any of that.
#[test]
fn every_script_the_app_launches_is_pure_ascii() {
    for name in LAUNCHED {
        let path = scripts_dir().join(name);
        let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{name}: {error}"));

        // Report the line and the character, because "byte 4102 is not ASCII" sends the reader
        // looking through the whole file for something they cannot see.
        let mut line = 1;
        for byte in &bytes {
            if *byte == b'\n' {
                line += 1;
            }
            assert!(
                byte.is_ascii(),
                "{name} line {line}: byte {byte:#04x} is not ASCII.\n\
                 Windows PowerShell 5.1 reads this file in the machine's ANSI codepage, and on \
                 a Japanese install a byte like this one can swallow the quote after it and stop \
                 the script parsing. Write the character with [char]0xNNNN instead.",
            );
        }
    }
}

/// A new spawn site means a new script whose parse failure nobody would see.
///
/// The list above cannot notice that it has gone stale on its own, so the source is asked how
/// many places launch a console. One today; a second should fail here and be added deliberately.
#[test]
fn the_list_of_launched_scripts_has_not_drifted() {
    let sources = ["src/commands.rs", "src/vlm_setup.rs", "src/lib.rs"];
    let spawns: usize = sources
        .iter()
        .map(|name| {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name);
            let text =
                std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
            text.matches("Command::new(\"powershell.exe\")").count()
        })
        .sum();
    assert_eq!(
        spawns,
        LAUNCHED.len(),
        "{spawns} places spawn powershell.exe but LAUNCHED names {}. \
         A script the app opens in its own window must be listed here, or the ASCII rule that \
         keeps it parsing on a Japanese-locale machine will not be checked for it.",
        LAUNCHED.len(),
    );
}

/// Parse — never run — every script in the interpreter a user's machine actually has.
///
/// This is the check that would have caught the CP932 failure, and it is the honest one: it
/// asks the real parser rather than modelling what the real parser does. It also catches
/// ordinary syntax errors in scripts nothing else here executes.
///
/// `powershell.exe` deliberately, not `pwsh`. Windows PowerShell 5.1 ships with Windows and is
/// what the app spawns; PowerShell 7 is an optional install, and the whole bug was the two
/// disagreeing.
#[cfg(windows)]
#[test]
fn every_script_parses_in_the_powershell_that_ships_with_windows() {
    let mut broken = Vec::new();
    for path in all_scripts() {
        if let Err(report) = parse_with_windows_powershell(&path) {
            broken.push(report);
        }
    }
    assert!(
        broken.is_empty(),
        "these scripts do not parse in Windows PowerShell 5.1:\n\n{}",
        broken.join("\n\n"),
    );
}

#[cfg(windows)]
fn parse_with_windows_powershell(path: &Path) -> Result<(), String> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let full = std::fs::canonicalize(path)
        .map_err(|error| format!("{name}: {error}"))?
        .display()
        .to_string()
        // `canonicalize` returns the \\?\ extended-length form, which the PowerShell parser
        // does not accept as a path literal.
        .trim_start_matches(r"\\?\")
        .to_string();

    let command = format!(
        "$errors = $null; \
         $null = [System.Management.Automation.Language.Parser]::ParseFile('{full}', [ref]$null, [ref]$errors); \
         if ($errors) {{ $errors | ForEach-Object {{ \"  line $($_.Extent.StartLineNumber): $($_.Message)\" }}; exit 1 }}"
    );

    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()
        .map_err(|error| format!("{name}: could not run powershell.exe: {error}"))?;

    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{name}:\n{}",
        String::from_utf8_lossy(&output.stdout).trim_end()
    ))
}

/// No script contains a carriage return that is not part of a line ending.
///
/// **A stray `\r` is invisible and it moves statement boundaries.** PowerShell ends a statement
/// at a bare CR, so one in the middle of a comment ends the comment there and parses the rest of
/// the line as a command. `# … under target\release is …` became a comment followed by a call to
/// a program named `elease`, and the script died at the point of use with "the term 'elease' is
/// not recognized" — naming something that appears nowhere in the file when you read it.
///
/// Not covered by the parse check above, and that is the reason this exists rather than being
/// folded into it: the result is *syntactically valid*. A bareword on its own line is a command
/// invocation, so the parser is content and only running it finds out. Checked as bytes, because
/// this is a property no amount of reading the text will reveal.
#[test]
fn no_script_contains_a_stray_carriage_return() {
    for path in all_scripts() {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{name}: {error}"));

        let mut line = 1;
        for (at, byte) in bytes.iter().enumerate() {
            if *byte == b'\n' {
                line += 1;
                continue;
            }
            if *byte == b'\r' && bytes.get(at + 1) != Some(&b'\n') {
                panic!(
                    "{name} line {line}: a carriage return that is not part of a line ending.\n\
                     PowerShell ends a statement there, so whatever follows on that line is \
                     parsed as a command — including inside a comment. It is valid syntax, so \
                     nothing else here will catch it.",
                );
            }
        }
    }
}

#[test]
fn the_checkpoint_launcher_is_ascii_and_its_checklist_is_explicit_utf8() {
    let script = std::fs::read(scripts_dir().join("desktop-checkpoint.ps1")).unwrap();
    assert!(
        script.is_ascii(),
        "put localized text in desktop-checkpoint-text.json"
    );
    let script = std::str::from_utf8(&script).unwrap();
    assert!(script.contains("[System.Text.Encoding]::UTF8"));
    let checklist: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(scripts_dir().join("desktop-checkpoint-text.json")).unwrap(),
    )
    .unwrap();
    let rows = checklist.as_object().unwrap();
    for key in rows.keys() {
        assert!(
            script.contains(&format!("Get-CheckpointText \"{key}\"")),
            "unused checklist row {key}"
        );
    }
    assert!(rows.values().any(|text| text.as_str().unwrap().contains("\u{5e02}\u{5834}\u{3067}\u{91ce}\u{83dc}\u{3092}\u{8cb7}\u{3044}\u{307e}\u{3057}\u{305f}\u{3002}")));
}

#[test]
#[cfg(windows)]
fn windows_powershell_prints_the_real_japanese_checklist_without_launching() {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(scripts_dir().join("desktop-checkpoint.ps1"))
        .arg("-PrintChecklist")
        .creation_flags(0x08000000)
        .output()
        .expect("Windows PowerShell");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let printed = String::from_utf8(output.stdout).expect("UTF-8 output");
    let rows: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(scripts_dir().join("desktop-checkpoint-text.json")).unwrap(),
    )
    .unwrap();
    for (key, value) in rows.as_object().unwrap() {
        if key.starts_with("row_") {
            assert!(
                printed.contains(value.as_str().unwrap()),
                "{key} was misdecoded"
            );
        }
    }
}
