fn main() {
    embed_common_controls_manifest_in_test_binaries();
    tauri_build::build();
}

/// Windows/MSVC only: give this crate's **test** binaries the same
/// Common-Controls 6 manifest that `tauri_build::build()` gives the app binary.
///
/// Tauri statically imports comctl32 v6-only symbols (`TaskDialogIndirect`,
/// `SetWindowSubclass`, `DefSubclassProc`, `RemoveWindowSubclass`). Without a
/// manifest declaring the v6 side-by-side dependency, the loader binds the process
/// to comctl32 v5.82 in System32, which does not export them — so a test harness
/// exits with STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139) before running a single
/// test. `rustc-link-arg-tests` targets only test binaries, so the app's own
/// linking is untouched.
fn embed_common_controls_manifest_in_test_binaries() {
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !(windows && msvc) {
        return;
    }

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests.manifest");
    println!("cargo:rerun-if-changed=tests.manifest");
    println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
        manifest.display()
    );
}
