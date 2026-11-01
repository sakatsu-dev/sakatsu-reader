//! Release tooling reads the same pins as setup rather than parsing Rust source.
fn main() {
    let components: Vec<_> = sakatsu_core::setup::catalog::all()
        .iter()
        .filter(|component| component.tier == sakatsu_core::setup::Tier::Essential)
        .map(|component| {
            let artifacts: Vec<_> = component.artifacts.iter().map(|artifact| serde_json::json!({
                "file": artifact.file,
                "bytes": artifact.expected_bytes(),
                "sha256": artifact.sha256(),
                "url": artifact.url,
            })).collect();
            serde_json::json!({"id": component.id, "licences": [component.licence], "artifacts": artifacts})
        })
        .collect();
    println!("{}", serde_json::json!({"components": components}));
}
