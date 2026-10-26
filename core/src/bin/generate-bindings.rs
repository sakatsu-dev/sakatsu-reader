//! Write `ui/src/bindings.ts` from the Rust types.
//!
//! ```text
//! cargo run -p sakatsu-core --features typescript --bin generate-bindings
//! ```
//!
//! **A command, not a test.** An `#[ignore]`d "regenerate" test would be swept up by the broad
//! `-- --ignored` runs this repository uses for measurements, and would rewrite a tracked contract
//! file as a side effect of asking how long something takes. Checking lives in
//! `core/tests/typescript_contract.rs`, which writes nothing.
//!
//! `required-features` in `core/Cargo.toml` keeps this out of an ordinary build entirely.

fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/bindings.ts");
    let rendered = sakatsu_core::bindings::render();
    if let Err(error) = std::fs::write(&path, rendered.as_bytes()) {
        eprintln!("could not write {}: {error}", path.display());
        std::process::exit(1);
    }
    println!("wrote {}", path.display());
    println!("now run: npm --prefix ui run check");
}
