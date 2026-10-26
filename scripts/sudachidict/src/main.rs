//! Build SudachiDict into Lindera's compiled format, exactly as `lindera-sudachidict`'s own
//! `build.rs` does (its `metadata.json` is beside this), then load it back as a check.
//!
//! ```text
//! cargo run --release --manifest-path scripts/sudachidict/Cargo.toml -- <source dir> <output dir>
//! ```
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        return Err("usage: build-sudachidict <sudachidict-20260723 source dir> <output dir>".into());
    };
    let (input, output) = (PathBuf::from(input), PathBuf::from(output));
    std::fs::create_dir_all(&output)?;
    let metadata_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sudachidict-metadata.json");
    let metadata: lindera_dictionary::dictionary::metadata::Metadata =
        serde_json::from_str(&std::fs::read_to_string(metadata_path)?)?;
    let started = std::time::Instant::now();
    lindera_dictionary::builder::DictionaryBuilder::new(metadata).build_dictionary(&input, &output)?;
    println!("built in {:.0}s -> {}", started.elapsed().as_secs_f64(), output.display());
    // Loads, or the build is not usable.
    lindera::dictionary::load_fs_dictionary(&output)?;
    println!("loads");
    Ok(())
}
