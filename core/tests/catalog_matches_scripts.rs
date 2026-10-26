//! The catalog and the fetch scripts must not disagree.
//!
//! `core/src/setup/catalog.rs` holds the same URLs as `scripts/fetch-*.ps1`, because the
//! scripts remain the development path while the app now fetches for itself. Two copies of
//! a URL is two places to update, and the failure mode is silent: an upstream project
//! moves a file, whoever hits it fixes the script they were running, and the *other* path
//! keeps pointing at a 404 until someone tries it — which, for the app, means a learner
//! with no dictionary and no idea why.
//!
//! So this reads the scripts as text and checks every URL still appears in the one it
//! belongs to. Same idea as `model.golden.json` pinning the TypeScript mirror to the Rust
//! types: the duplication is allowed to exist, but not to drift.
//!
//! It is a substring check rather than a parse, because the scripts build some URLs by
//! interpolation — `"$base/$($file.Name)"` over a `$repository` variable — and there is no
//! whole URL in the file to compare against.
//!
//! **But it demands the whole URL wherever the script writes one.** Checking only the host
//! and the file name would let a version bump through: `.../v1.1/wnjpn-ok.tab.gz` becoming
//! `v1.2` in the script alone still matches `github.com` and `wnjpn-ok.tab.gz`, and the
//! app would go on fetching a version nobody maintains. `assert_the_test_has_teeth` below
//! pins that this is really what happens, because a drift test that cannot detect drift is
//! worse than none: it is the same green tick with none of the cover.

use sakatsu_core::setup::catalog;

fn script(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../scripts")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Every `http(s)://…` written out in the script, however it is quoted.
fn literal_urls(script: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = script;
    while let Some(at) = rest
        .find("http://")
        .into_iter()
        .chain(rest.find("https://"))
        .min()
    {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | '`' | ')' | ',' | '<'))
            .unwrap_or(tail.len());
        found.push(tail[..end].trim_end_matches(['.', ';', ':']));
        rest = &tail[end.max(1)..];
    }
    found
}

/// Whether a script accounts for one catalog URL.
///
/// **Which check applies is decided by the script, not by whether the URL matches.** If
/// the script writes a literal URL for this file, that URL must be the catalog's — the
/// earlier version asked "does the whole URL appear, and otherwise settle for the host
/// and file name", which let a drifted version satisfy the fallback it had just caused.
fn satisfied(url: &str, script: &str) -> Result<(), String> {
    let (base, file) = url.rsplit_once('/').expect("a URL with a path");

    let literals: Vec<&str> = literal_urls(script)
        .into_iter()
        .filter(|candidate| candidate.ends_with(file))
        .collect();

    if !literals.is_empty() {
        return if literals.contains(&url) {
            Ok(())
        } else {
            Err(format!(
                "the script fetches {} — every character has to agree",
                literals[0]
            ))
        };
    }

    // Assembled from variables, so there is no whole URL to compare. Demand the pieces
    // that say where the bytes come from.
    let mut pieces = vec![file.to_string()];
    if let Some(host) = base
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
    {
        pieces.push(host.to_string());
    }
    if let Some(repository) = huggingface_repository(url) {
        pieces.push(repository);
    }

    let missing: Vec<&str> = pieces
        .iter()
        .filter(|piece| !script.contains(piece.as_str()))
        .map(String::as_str)
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing.join(", "))
    }
}

#[test]
fn every_catalog_url_is_still_in_the_script_it_came_from() {
    let mut problems = Vec::new();

    for component in catalog::all() {
        let Some(name) = component.script else {
            continue;
        };
        let text = script(name);

        for artifact in component.artifacts {
            if let Err(missing) = satisfied(artifact.url, &text) {
                problems.push(format!(
                    "{}: {name} is missing {missing}\n      catalog has {}",
                    component.id, artifact.url
                ));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "the catalog and the scripts have drifted:\n  {}",
        problems.join("\n  ")
    );
}

/// The test above must fail when the two really differ.
///
/// Written after noticing the first version of it could not: it checked the host and the
/// file name, so `.../v1.1/wnjpn-ok.tab.gz` drifting to `v1.2` in one place matched
/// happily in both.
#[test]
fn assert_the_test_has_teeth() {
    let real = "https://github.com/bond-lab/wnja/releases/download/v1.1/wnjpn-ok.tab.gz";
    let script = format!("$WNJA = '{real}'\n");

    assert!(satisfied(real, &script).is_ok(), "the true URL passes");

    // A version bump in the script and not the catalog: same host, same file name.
    let bumped = script.replace("v1.1", "v1.2");
    assert!(
        satisfied(real, &bumped).is_err(),
        "a changed version must be caught, not waved through on the file name"
    );

    // A repository moving under an assembled HuggingFace URL.
    let hugging = "https://huggingface.co/mayocream/manga-ocr-onnx/resolve/24b12778d85800835e2ca409236de281b8ab7b9f/decoder_model.onnx";
    let assembled = "$repository = 'mayocream/manga-ocr-onnx'\n$base = \"https://huggingface.co/$repository/resolve/main\"\n@{ Name = 'decoder_model.onnx' }";
    assert!(satisfied(hugging, assembled).is_ok());
    assert!(
        satisfied(hugging, &assembled.replace("mayocream", "someone-else")).is_err(),
        "a moved repository must be caught"
    );
}

/// `https://huggingface.co/owner/name/resolve/main/file` → `owner/name`.
fn huggingface_repository(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://huggingface.co/")?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    Some(format!("{owner}/{name}"))
}

/// Every script the catalog names must actually be on disk — including the one that is
/// only ever quoted as instructions, which is the easiest to delete by accident.
#[test]
fn every_script_the_catalog_names_exists() {
    for component in catalog::all() {
        if let Some(name) = component.script {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../scripts")
                .join(name);
            assert!(
                path.is_file(),
                "{} names a missing script: {name}",
                component.id
            );
        }
    }
}
