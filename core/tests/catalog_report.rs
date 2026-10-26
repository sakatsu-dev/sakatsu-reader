#[test]
fn documented_download_sizes_are_derived_from_the_catalog() {
    let file =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/catalog-download-sizes.md");
    assert_eq!(
        std::fs::read_to_string(file)
            .expect("catalog report")
            .replace("\r\n", "\n"),
        sakatsu_core::setup::catalog_report::markdown(),
        "regenerate with cargo run -p sakatsu-core --bin generate-catalog-report"
    );
}
