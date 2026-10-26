//! The pinned KanjiVG release goes through the installer's own import, every diagram of it.
//!
//! ```text
//! $env:SAKATSU_KANJIVG_ZIP = '<path to kanjivg-20260714-main.zip>'
//! cargo test -p sakatsu-core --test kanjivg_release -- --ignored --nocapture
//! ```
//!
//! **Why this exists.** The diagram parser was tested only on small hand-made SVGs. On the real
//! release it refused the DOCTYPE every one of the 6,702 diagrams carries, and then the whole
//! archive for one upstream drawing mistake (參). So the "Kanji details" essential could not
//! install at all, and nothing noticed until a desktop check on 2026-10-13 ran setup on a clean
//! profile. The archive is fetched, never committed (CC BY-SA 3.0, 12 MB),
//! so this runs on demand against the file setup downloads, at the pinned digest.
//!
//! r20260714 (pinned 2026-10-18) fixed 參 upstream: every diagram imports and nothing is skipped.
//! The skip stays in the importer for the next release that draws one wrong.

use sakatsu_core::setup::enrichment::SKIPPED_DIAGRAMS;
use sakatsu_core::setup::kanjivg_files;

/// `core/src/setup/catalog.rs` pins this file by size and sha256; any other bytes are not the
/// release this test is about.
const PINNED_SHA256: &str = "b5df6cd2bc249dd49b8041eb99e38ba9f8dc6b9ea57962a80084b4a315d1a7fc";

#[test]
#[ignore = "needs the pinned KanjiVG archive (SAKATSU_KANJIVG_ZIP)"]
fn every_diagram_of_the_pinned_release_imports() {
    let archive = std::path::PathBuf::from(
        std::env::var_os("SAKATSU_KANJIVG_ZIP").expect("set SAKATSU_KANJIVG_ZIP"),
    );
    let bytes = std::fs::read(&archive).expect("read the archive");
    let digest = {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(&bytes);
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest,
        PINNED_SHA256,
        "{} is not the pinned release",
        archive.display()
    );

    let out = std::env::temp_dir().join(format!("kanjivg-release-{}", std::process::id()));
    let written = kanjivg_files(&archive, &out).expect("the installer imports the pinned release");
    let diagrams = std::fs::read_dir(&out)
        .expect("output")
        .filter(|entry| {
            entry.as_ref().is_ok_and(|entry| {
                entry.path().extension().and_then(|e| e.to_str()) == Some("json")
            })
        })
        .count();
    // The list is written only when something is skipped.
    let skipped = std::fs::read_to_string(out.join(SKIPPED_DIAGRAMS)).ok();
    let _ = std::fs::remove_dir_all(&out);
    println!("imported {written} diagrams; skipped: {skipped:?}");
    // 6,703 in the release, 參 (053c3) among them now that its strokes are numbered once each.
    assert_eq!(written, 6_703);
    assert_eq!(diagrams, written);
    assert_eq!(skipped, None, "the pinned release imports without skips");
}
