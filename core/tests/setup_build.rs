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
    let written = sakatsu_core::setup::download(
        artifact.url,
        &target,
        artifact.min_bytes,
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
