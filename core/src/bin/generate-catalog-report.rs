fn main() {
    let target =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/catalog-download-sizes.md");
    std::fs::write(&target, sakatsu_core::setup::catalog_report::markdown())
        .expect("write catalog report");
    println!("wrote {}", target.display());
}
