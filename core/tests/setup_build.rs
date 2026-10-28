//! The Rust table builders must agree with the PowerShell ones.
//!
//! `core/src/setup/build.rs` and `scripts/fetch-{kanjidic,wordnet}.ps1` parse the same two
//! formats. That duplication exists for a reason — the scripts are the development path
//! and a learner handed the installer cannot run them — but two implementations of a
//! format is exactly where a difference hides: one gets a fix, the other does not, and
//! nobody finds out until a reading is missing from a table nobody re-reads.
//!
//! So this builds each table from the same downloaded archive and compares the result to
//! whatever the script left in `%APPDATA%`.
//!
//! ```text
//! cargo test -p sakatsu-core --test setup_build -- --ignored --nocapture
//! ```
//!
//! Ignored, because it needs the archives, which are only there once setup has been run
//! one way or the other.

use std::path::PathBuf;

/// Exercise the real script from an empty output tree, without network or installed data.
/// Synthetic pins apply only to the temporary script copy; production pins remain unchanged.
#[cfg(windows)]
#[test]
fn fresh_kanji_script_verifies_and_publishes_only_complete_xml() {
    use sha2::Digest;
    use std::io::Write;

    let dir = scratch_guard::Scratch::new("setup-build", "fresh-script");
    let script = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/fetch-kanjidic.ps1"),
    )
    .unwrap();
    let artifact = &sakatsu_core::setup::catalog::find("kanji")
        .unwrap()
        .artifacts[0];
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?><kanjidic2><character><literal>&#39135;</literal><misc><stroke_count>9</stroke_count><grade>2</grade></misc><reading_meaning><rmgroup><reading r_type="ja_on">&#12471;&#12519;&#12463;</reading><meaning>eat</meaning></rmgroup></reading_meaning></character></kanjidic2>"#;
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(xml).unwrap();
    let valid = gzip.finish().unwrap();
    let wrapper = dir.join("run.ps1");
    std::fs::write(
        &wrapper,
        r#"param($Source, $Out, $Archive)
$ErrorActionPreference = 'Stop'
function Invoke-WebRequest {
    param($Uri, $OutFile, [switch]$UseBasicParsing)
    Copy-Item -LiteralPath $Archive -Destination $OutFile
}
try { & $Source -Out $Out } catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
"#,
    )
    .unwrap();

    for (case, bytes, wrong_hash) in [
        ("valid", valid.as_slice(), false),
        ("wrong-hash", valid.as_slice(), true),
        ("broken-gzip", b"not a gzip archive".as_slice(), false),
    ] {
        let out = dir.join(case);
        let work = out.join("work");
        std::fs::create_dir_all(&work).unwrap();
        // This script stages the supplemental archives; Rust performs their conversion.
        std::fs::write(work.join("kanjivg-20260714-main.zip"), b"staged").unwrap();
        std::fs::write(work.join("kradfile.gz"), b"staged").unwrap();
        let archive = dir.join(format!("{case}.gz"));
        std::fs::write(&archive, bytes).unwrap();
        let hash = if wrong_hash {
            "0".repeat(64)
        } else {
            sha2::Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        };
        let size_pin = format!("$KANJIDIC_BYTES = {}", artifact.expected_bytes().unwrap());
        let hash_pin = format!("$KANJIDIC_SHA256 = '{}'", artifact.sha256().unwrap());
        assert_eq!(script.matches(&size_pin).count(), 1);
        assert_eq!(script.matches(&hash_pin).count(), 1);
        let fixture = dir.join(format!("{case}.ps1"));
        std::fs::write(
            &fixture,
            script
                .replace(&size_pin, &format!("$KANJIDIC_BYTES = {}", bytes.len()))
                .replace(&hash_pin, &format!("$KANJIDIC_SHA256 = '{hash}'")),
        )
        .unwrap();
        let result = std::process::Command::new("powershell.exe")
            // A PowerShell 7 parent can export its module path; let 5.1 use its own modules.
            .env_remove("PSModulePath")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&wrapper)
            .arg("-Source")
            .arg(&fixture)
            .arg("-Out")
            .arg(&out)
            .arg("-Archive")
            .arg(&archive)
            .output()
            .unwrap();
        if case == "valid" {
            assert!(
                result.status.success(),
                "{case}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let table = std::fs::read_to_string(out.join("kanjidic.tsv")).unwrap();
            assert!(table.starts_with("食\tショク\t"), "{table}");
        } else {
            assert!(!result.status.success(), "{case} unexpectedly succeeded");
            assert!(!out.join("kanjidic.tsv").exists());
            assert!(!work.join("kanjidic2.xml").exists());
            if wrong_hash {
                assert!(String::from_utf8_lossy(&result.stderr)
                    .contains("KANJIDIC2 does not match the pinned snapshot"));
            }
        }
        assert!(!std::fs::read_dir(&work).unwrap().any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "partial")));
    }
}

#[path = "common/scratch.rs"]
mod scratch_guard;

fn app_data() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("com.sakatsu.app"))
}

/// Lines, not bytes.
///
/// PowerShell's `WriteAllLines` ends every line with CRLF on Windows and Rust writes LF.
/// That difference is invisible to both readers — `str::lines` strips the carriage
/// return — so comparing bytes would fail on something no code can observe, while
/// comparing lines tests the thing that actually matters.
fn lines_of(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(ToString::to_string)
        .collect()
}

/// Compare, and describe the difference in a way a person can act on.
///
/// Never `assert_eq!` on the two vectors: these hold forty thousand lines, and the
/// failure output buries the handful that differ under every line that does not. The
/// first version of this did exactly that and the real difference — a sort order — took a
/// separate script to find.
fn compare(what: &str, script: &[String], rust: &[String]) {
    println!("\n{what}");
    println!("  script wrote {} lines", script.len());
    println!("  rust   wrote {} lines", rust.len());

    let in_script: std::collections::HashSet<&String> = script.iter().collect();
    let in_rust: std::collections::HashSet<&String> = rust.iter().collect();

    let mut only_script: Vec<&&String> = in_script.difference(&in_rust).collect();
    let mut only_rust: Vec<&&String> = in_rust.difference(&in_script).collect();
    only_script.sort();
    only_rust.sort();

    let mut problems = Vec::new();
    if !only_script.is_empty() {
        problems.push(format!(
            "{} lines only the script has, e.g. {:?}",
            only_script.len(),
            only_script.iter().take(6).collect::<Vec<_>>()
        ));
    }
    if !only_rust.is_empty() {
        problems.push(format!(
            "{} lines only rust has, e.g. {:?}",
            only_rust.len(),
            only_rust.iter().take(6).collect::<Vec<_>>()
        ));
    }

    // Content, not order.
    //
    // Both files are sorted, and each is sorted reproducibly — the scripts pass an
    // ordinal comparer so the result does not depend on the machine's locale. But the two
    // ordinals are not the same ordinal: .NET compares UTF-16 code units and Rust compares
    // UTF-8 bytes, and they disagree about characters outside the basic plane. 𠀋
    // (U+2000B) is a surrogate pair to one and a four-byte sequence to the other, so it
    // lands in a different place.
    //
    // Nothing reads these files in order — both build a `HashMap` — so demanding one
    // order would be pinning an implementation detail of two standard libraries.
    assert!(
        problems.is_empty(),
        "the two implementations disagree:\n  {}",
        problems.join("\n  ")
    );
    println!("  identical ({} lines, order aside)", rust.len());
}

#[test]
#[ignore = "needs the KANJIDIC2 archive; run setup once first"]
fn the_kanji_table_is_the_same_built_either_way() {
    let Some(root) = app_data() else {
        println!("SKIP: no APPDATA");
        return;
    };
    let archive = root.join("kanji/work/kanjidic2.xml.gz");
    let by_script = root.join("kanji/kanjidic.tsv");
    if !archive.is_file() || !by_script.is_file() {
        println!("SKIP: no archive at {}", archive.display());
        return;
    }

    let script = lines_of(&by_script);
    let columns = script
        .first()
        .map(|line| line.split('\t').count())
        .unwrap_or_default();
    if columns < 7 {
        println!(
            "SKIP: installed kanjidic.tsv is the legacy {columns}-column table; update Kanji \
             details before cross-checking nanori"
        );
        return;
    }

    let mine = std::env::temp_dir().join("sakatsu-kanjidic-rust.tsv");
    let count = sakatsu_core::setup::kanji_table(&archive, &mine).expect("builds");

    let rust = lines_of(&mine);
    assert_eq!(count, rust.len());
    compare("kanji", &script, &rust);
}

#[test]
#[ignore = "needs both WordNet archives; run setup once first"]
fn the_wordnet_table_is_the_same_built_either_way() {
    let Some(root) = app_data() else {
        println!("SKIP: no APPDATA");
        return;
    };
    // The script decompresses as it goes, so the `.gz` may or may not still be there.
    let japanese = root.join("wordnet/work/wnjpn-ok.tab.gz");
    let princeton = root.join("wordnet/work/WNdb-3.0.tar.gz");
    let by_script = root.join("wordnet/categories.tsv");
    if !japanese.is_file() || !princeton.is_file() || !by_script.is_file() {
        println!(
            "SKIP: no archives under {}",
            root.join("wordnet/work").display()
        );
        return;
    }

    let mine = std::env::temp_dir().join("sakatsu-wordnet-rust.tsv");
    let count = sakatsu_core::setup::wordnet_table(&japanese, &princeton, &mine).expect("builds");

    let rust = lines_of(&mine);
    assert_eq!(count, rust.len());
    compare("wordnet", &lines_of(&by_script), &rust);
}

/// The real URLs, end to end, for the smallest artefact in the catalog.
///
/// Unit tests serve their own bytes over loopback, which proves the streaming and the
/// size floor but says nothing about whether the URL still exists. That is the failure
/// this feature has that no amount of local testing catches — an upstream project moves
/// a file and every new install gets an error page.
///
/// KANJIDIC2 because it is 1.4 MB rather than 461, and because it exercises the whole
/// path: download, gunzip, parse, write a table the app then reads.
#[test]
#[ignore = "downloads from the real internet"]
fn the_catalog_urls_still_answer() {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    let component = sakatsu_core::setup::catalog::find("kanji").expect("kanji is in the catalog");
    let artifact = &component.artifacts[0];

    let dir = scratch_guard::Scratch::new("setup-build", "live-fetch");
    let target = dir.join(artifact.file);

    let started = std::time::Instant::now();
    let mut last = 0u64;
    let written = sakatsu_core::setup::download_verified(
        artifact.url,
        &target,
        artifact.min_bytes,
        sakatsu_core::setup::Verification {
            expected_bytes: Some(artifact.expected_bytes().expect("catalog size is pinned")),
            sha256: Some(artifact.sha256().expect("catalog digest is pinned")),
        },
        &Arc::new(AtomicBool::new(false)),
        &mut |progress| last = progress.done,
    )
    .unwrap_or_else(|error| panic!("{}: {error}", artifact.url));

    println!(
        "{} → {:.1} MB in {:.1}s",
        artifact.url,
        written as f64 / (1024.0 * 1024.0),
        started.elapsed().as_secs_f64()
    );
    assert_eq!(last, written, "progress ended where the file did");

    // And it is really the thing it claims to be, not merely large enough.
    let table = dir.join("kanjidic.tsv");
    let count = sakatsu_core::setup::kanji_table(&target, &table).expect("builds a table");
    println!("  {count} characters");
    assert!(count > 10_000, "only {count} characters");

    let loaded = sakatsu_core::dict::KanjiDic::load(&table);
    let food = loaded.get('食').expect("食 is in the table");
    println!("  食 on={:?} kun={:?}", food.on, food.kun);
    assert!(food.on.iter().any(|reading| reading == "ショク"));
}
