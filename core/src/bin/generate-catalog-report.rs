use std::ffi::OsString;
use std::path::PathBuf;

fn output(args: Vec<OsString>) -> Result<PathBuf, &'static str> {
    match args.as_slice() {
        [] => Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/catalog-download-sizes.md")),
        [flag, path] if flag == "--output" => Ok(PathBuf::from(path)),
        _ => Err("usage: generate-catalog-report [--output PATH]"),
    }
}

fn main() -> std::process::ExitCode {
    let result = output(std::env::args_os().skip(1).collect())
        .map_err(str::to_owned)
        .and_then(|target| {
            std::fs::write(&target, sakatsu_core::setup::catalog_report::markdown())
                .map_err(|error| format!("could not write {}: {error}", target.display()))?;
            println!("wrote {}", target.display());
            Ok(())
        });
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[test]
fn default_and_explicit_destinations_are_validated() {
    assert!(output(vec![])
        .unwrap()
        .ends_with("tests/fixtures/catalog-download-sizes.md"));
    assert_eq!(
        output(vec!["--output".into(), "report with spaces.md".into()]).unwrap(),
        PathBuf::from("report with spaces.md")
    );
    assert!(output(vec!["--output".into()]).is_err());
    assert!(output(vec!["unknown".into()]).is_err());
}
