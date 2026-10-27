#[test]
fn documented_download_sizes_are_derived_from_the_catalog() {
    assert_eq!(
        include_str!("fixtures/catalog-download-sizes.md").replace("\r\n", "\n"),
        sakatsu_core::setup::catalog_report::markdown(),
        "regenerate with cargo run -p sakatsu-core --bin generate-catalog-report"
    );
}

#[test]
fn ci_dictionary_fixture_uses_the_catalog_pin() {
    let specification: serde_json::Value =
        serde_json::from_str(include_str!("../../scripts/ci-test-data.json")).unwrap();
    let catalog = sakatsu_core::setup::catalog::all();
    let artifact = catalog
        .iter()
        .flat_map(|component| component.artifacts)
        .find(|artifact| Some(artifact.file) == specification["file"].as_str())
        .expect("CI fixture is a known catalog artifact");
    assert_eq!(Some(artifact.url), specification["url"].as_str());
    assert_eq!(artifact.expected_bytes, specification["bytes"].as_u64());
    assert_eq!(artifact.sha256, specification["sha256"].as_str());
}
