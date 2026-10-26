//! Download-size prose derived from the same artifact metadata as setup.

pub fn markdown() -> String {
    let mut output = String::from("# Catalog download sizes\n\nGenerated from `core/src/setup/catalog.rs`; do not edit the table by hand.\nRegenerate with `cargo run -p sakatsu-core --bin generate-catalog-report`.\nThese are transfer sizes, not resident memory or peak installation space.\nSetup preflight computes remaining disk requirements for the selected location.\n\n| Component | Download bytes | MiB | Basis |\n|---|---:|---:|---|\n");
    let mut essential = 0;
    for component in super::catalog::all() {
        let exact: Option<u64> = if component.artifacts.is_empty() {
            None
        } else {
            component
                .artifacts
                .iter()
                .map(|artifact| artifact.expected_bytes())
                .sum()
        };
        let bytes = exact.unwrap_or(component.bytes);
        if component.tier == super::Tier::Essential {
            essential += bytes;
        }
        output.push_str(&format!(
            "| {} | {bytes} | {:.1} | {} |\n",
            component.label,
            bytes as f64 / 1_048_576.0,
            if exact.is_some() {
                "pinned artifacts"
            } else {
                "catalog estimate"
            }
        ));
    }
    output.push_str(&format!("\nEssential components total: **{essential} bytes ({:.1} MiB)**. Optional components are separate.\n", essential as f64 / 1_048_576.0));
    output
}
