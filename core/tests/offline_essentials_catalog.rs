use std::collections::BTreeMap;

use sakatsu_core::setup::catalog::{self, Tier};
use serde::Deserialize;

#[derive(Deserialize)]
struct PackageCatalog {
    components: Vec<PackageComponent>,
}

#[derive(Deserialize)]
struct PackageComponent {
    id: String,
    licences: Vec<String>,
    artifacts: Vec<PackageArtifact>,
}

#[derive(Deserialize)]
struct PackageArtifact {
    file: String,
    bytes: u64,
    sha256: String,
}

#[test]
fn offline_package_catalog_is_the_embedded_essential_catalog() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../scripts/offline-essentials-catalog.json");
    let package: PackageCatalog =
        serde_json::from_slice(&std::fs::read(path).expect("offline package catalog"))
            .expect("valid offline package catalog");
    let packaged = package
        .components
        .iter()
        .map(|component| (component.id.as_str(), component))
        .collect::<BTreeMap<_, _>>();
    let essential = catalog::all()
        .iter()
        .filter(|component| component.tier == Tier::Essential)
        .collect::<Vec<_>>();

    assert_eq!(packaged.len(), essential.len());
    for component in essential {
        let package_component = packaged
            .get(component.id)
            .unwrap_or_else(|| panic!("{} missing from offline catalog", component.id));
        assert!(package_component
            .licences
            .iter()
            .any(|licence| licence == component.licence));
        assert_eq!(package_component.artifacts.len(), component.artifacts.len());
        for artifact in component.artifacts {
            let packaged_artifact = package_component
                .artifacts
                .iter()
                .find(|candidate| candidate.file == artifact.file)
                .unwrap_or_else(|| panic!("{}/{} missing", component.id, artifact.file));
            assert_eq!(packaged_artifact.bytes, artifact.expected_bytes().unwrap());
            assert_eq!(packaged_artifact.sha256, artifact.sha256().unwrap());
        }
    }
}

/// The checked-in offline catalog is what the generator produces.
///
/// **An up-to-date check, in the shape of `cargo fmt --check`.** The test above compares the two
/// field by field and says which one disagrees; this says something stronger and simpler — the
/// file is derived, so the only correct content is the generator's output. When it fails the fix
/// is a command, not a reconciliation:
///
/// ```text
/// cargo test -p sakatsu-core --test offline_essentials_catalog -- --ignored write_the_offline_catalog --nocapture
/// ```
///
/// `package_version` is preserved from the existing file: it names a build of the offline package
/// and has no source in the catalog, so the generator is told it rather than inventing one.
#[test]
fn the_offline_catalog_is_what_the_generator_produces() {
    let path = catalog_path();
    let existing = std::fs::read_to_string(&path).expect("offline package catalog");
    let generated = catalog::offline_package_catalog(&package_version(&existing))
        .expect("every essential artifact is pinned");
    // Line endings are the checkout's business, not the catalog's: a CRLF working tree on Windows
    // must not read as drift.
    assert_eq!(
        without_carriage_returns(&existing),
        without_carriage_returns(&generated),
        "the checked-in offline catalog is not what the catalog produces. It is generated — rerun \
         the generator rather than editing it:\n  cargo test -p sakatsu-core --test \
         offline_essentials_catalog -- --ignored write_the_offline_catalog --nocapture"
    );
}

/// Write the offline catalog from the embedded one. Run when the catalog changes.
#[test]
#[ignore = "writes scripts/offline-essentials-catalog.json"]
fn write_the_offline_catalog() {
    let path = catalog_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let generated = catalog::offline_package_catalog(&package_version(&existing))
        .expect("every essential artifact is pinned");
    std::fs::write(&path, generated.as_bytes()).expect("write the offline catalog");
    println!("wrote {}", path.display());
}

/// Compare content, not the checkout's line-ending convention.
fn without_carriage_returns(text: &str) -> String {
    text.chars()
        .filter(|character| *character != '\r')
        .collect()
}

fn catalog_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../scripts/offline-essentials-catalog.json")
}

/// The one field with no source in the catalog, kept from whatever is already there.
fn package_version(existing: &str) -> String {
    serde_json::from_str::<serde_json::Value>(existing)
        .ok()
        .and_then(|value| value["package_version"].as_str().map(str::to_string))
        .unwrap_or_else(|| "0.0.0".to_string())
}
