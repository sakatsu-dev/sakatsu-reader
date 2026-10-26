//! Nothing may be depended on without being credited.
//!
//! `core/src/credits.rs` names what the About page shows, and the manifests name what the app is
//! actually built from. Two lists of the same fact is two places to update, and the failure mode
//! is the quiet one: a dependency is added, the build goes green, and the page goes on describing
//! an app that no longer exists. For the rows carrying a real obligation — Apache-2.0 wants its
//! notice, CC BY-SA wants attribution — that is not a cosmetic gap.
//!
//! So this reads the three manifests as text and asserts every direct dependency is named by some
//! credit's `packages`. Same idea as `catalog_matches_scripts.rs`, and the same idea as
//! `model.golden.json` pinning the TypeScript mirror to the Rust types: the duplication is
//! allowed to exist, but not to drift.
//!
//! **Direct dependencies only, and dev-dependencies are excluded.** The four hundred crates
//! underneath are a generated wall nobody reads; what is on the page is what was *chosen*. And a
//! dev-dependency is not shipped, so it carries nothing to a reader of the About page — both
//! `[dev-dependencies]` blocks in this workspace name crates that are direct dependencies anyway.
//!
//! `ui`'s `devDependencies` are **not** excluded, because npm's split is about install time
//! rather than about shipping: Svelte and Vite are devDependencies and their output is the
//! interface.
//!
//! `assert_the_test_has_teeth` at the bottom is not optional decoration. `catalog_matches_scripts`
//! says why in its own words — *a drift test that cannot detect drift is worse than none: it is
//! the same green tick with none of the cover* — and the first version of this one could be
//! satisfied by a manifest it had failed to parse at all, which is exactly that failure.

use sakatsu_core::credits;

fn manifest(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Every dependency name a Cargo manifest declares directly.
///
/// A hand-rolled scan rather than a TOML parser, because adding `toml` as a dev-dependency to
/// check the dependency list would be a small joke at this file's expense. The shape it has to
/// cope with is narrow and entirely visible in the two manifests it reads:
///
/// - `[dependencies]`, `[build-dependencies]` and `[target.'cfg(windows)'.dependencies]` count;
///   `[dev-dependencies]` and `[package]` do not.
/// - A value can run over several lines — `image = { …, features = [ "png", … ] }` — and the
///   `features = [` inside it must not be read as a dependency called `features`. Hence the depth
///   count: a name only counts at depth zero.
/// - `#` starts a comment, and these manifests are more comment than manifest.
fn cargo_dependencies(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut wanted = false;
    let mut depth = 0i32;

    for line in text.lines() {
        let line = match line.find('#') {
            Some(at) => &line[..at],
            None => line,
        };
        let trimmed = line.trim();

        if depth == 0 && trimmed.starts_with('[') {
            // `[target.'cfg(windows)'.dependencies]` and `[dependencies]` both end the same way,
            // which is the whole of what has to be recognised.
            wanted = trimmed.ends_with("dependencies]") && !trimmed.contains("dev-dependencies");
            continue;
        }

        if wanted && depth == 0 {
            if let Some((name, _)) = trimmed.split_once('=') {
                let name = name.trim();
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    found.push(name.to_string());
                }
            }
        }

        depth += line.matches(['{', '[']).count() as i32;
        depth -= line.matches(['}', ']']).count() as i32;
        depth = depth.max(0);
    }
    found
}

/// Every dependency `ui/package.json` declares, both kinds. See the note at the top for why both.
fn npm_dependencies(text: &str) -> Vec<String> {
    let parsed: serde_json::Value = serde_json::from_str(text).expect("ui/package.json is JSON");
    let mut found = Vec::new();
    for key in ["dependencies", "devDependencies"] {
        if let Some(map) = parsed.get(key).and_then(|value| value.as_object()) {
            found.extend(map.keys().cloned());
        }
    }
    found
}

/// What is credited, whatever it is credited under.
fn credited() -> Vec<&'static str> {
    credits::all()
        .iter()
        .flat_map(|credit| credit.packages.iter().copied())
        .collect()
}

/// A workspace member depending on another is the workspace, not a third party.
fn is_ours(name: &str) -> bool {
    matches!(name, "sakatsu-core" | "sakatsu-desktop")
}

#[test]
fn every_dependency_is_credited() {
    let credited = credited();
    let mut problems = Vec::new();

    let sources = [
        (
            "core/Cargo.toml",
            cargo_dependencies(&manifest("Cargo.toml")),
        ),
        (
            "src-tauri/Cargo.toml",
            cargo_dependencies(&manifest("../src-tauri/Cargo.toml")),
        ),
        (
            "ui/package.json",
            npm_dependencies(&manifest("../ui/package.json")),
        ),
    ];

    for (where_, names) in &sources {
        for name in names {
            if is_ours(name) || credited.contains(&name.as_str()) {
                continue;
            }
            problems.push(format!(
                "{where_} depends on {name}, which credits.rs does not name"
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "the About page and the manifests have drifted:\n  {}\n\n\
         Add it to `core/src/credits.rs` — under an existing credit's `packages` if it belongs to \
         a family already listed, or as a row of its own with a line saying what it is for.",
        problems.join("\n  ")
    );
}

/// The other direction: a credit naming a package nothing depends on any more.
///
/// Less serious than the first — a stale row misinforms rather than under-attributes — but it is
/// the thing that happens when a dependency is dropped, and nothing else would notice.
#[test]
fn every_credited_package_is_still_depended_on() {
    let mut declared = cargo_dependencies(&manifest("Cargo.toml"));
    declared.extend(cargo_dependencies(&manifest("../src-tauri/Cargo.toml")));
    declared.extend(npm_dependencies(&manifest("../ui/package.json")));

    let stale: Vec<&str> = credited()
        .into_iter()
        .filter(|package| !declared.iter().any(|name| name == package))
        .collect();

    assert!(
        stale.is_empty(),
        "credits.rs names packages nothing depends on any more: {}",
        stale.join(", ")
    );
}

/// Both tests above must fail when the lists really differ — and the parser must really parse.
///
/// The interesting case is the third one. A `cargo_dependencies` that returned nothing at all —
/// a section header spelt differently, a manifest that moved — would satisfy
/// `every_dependency_is_credited` perfectly, because a list with nothing missing from it is a
/// list with nothing in it.
#[test]
fn assert_the_test_has_teeth() {
    let manifest = "\
[package]
name = \"sakatsu-core\"
version = \"0.1.0\"

[dependencies]
# A comment mentioning some-crate = \"1\", which is not a dependency.
serde = { version = \"1\", features = [\"derive\"] }
image = { version = \"0.25.6\", default-features = false, features = [
  \"png\",
  \"jpeg\",
] }
zip = \"4.2.0\"

[dev-dependencies]
only-for-tests = \"1\"

[target.'cfg(windows)'.dependencies]
windows = { version = \"0.62\", features = [\"Win32_Foundation\"] }
";

    let found = cargo_dependencies(manifest);

    // The parser reads what is there …
    assert!(found.contains(&"serde".to_string()));
    assert!(found.contains(&"image".to_string()));
    assert!(found.contains(&"zip".to_string()));
    assert!(
        found.contains(&"windows".to_string()),
        "a target-specific section is still a dependency section"
    );

    // … and only what is there.
    assert!(
        !found.contains(&"features".to_string()),
        "`features = [` inside a multi-line value is not a dependency"
    );
    assert!(
        !found.contains(&"only-for-tests".to_string()),
        "dev-dependencies are excluded — see the note at the top"
    );
    assert!(
        !found.contains(&"some-crate".to_string()),
        "a crate named in a comment is not a dependency"
    );
    assert!(
        !found.contains(&"name".to_string()) && !found.contains(&"version".to_string()),
        "`[package]` is not a dependency section"
    );

    // And the count is pinned, because "reads what is there" and "reads only what is there" can
    // both pass on a parser that also invents a row.
    assert_eq!(found.len(), 4, "found {found:?}");

    // The npm side, including the split the note at the top explains.
    let package_json =
        r#"{"dependencies":{"@tauri-apps/api":"^2"},"devDependencies":{"svelte":"^5"}}"#;
    let npm = npm_dependencies(package_json);
    assert!(npm.contains(&"@tauri-apps/api".to_string()));
    assert!(
        npm.contains(&"svelte".to_string()),
        "npm devDependencies are not excluded — their output is the interface"
    );
}

/// **Not shown is not the same as not credited.**
///
/// The About page stopped printing the plumbing group — serde, chrono, zip and the rest — because
/// naming what every desktop application in this language is made of says nothing about this one.
/// The rows stayed in `credits::all`, and they have to: that is what the tests above read, so
/// deleting them to tidy the page would quietly remove eight direct dependencies from the record
/// while every check here went on passing for the ones that remained.
///
/// This pins both halves at once — the group is hidden, and its packages are still credited.
#[test]
fn the_hidden_group_is_still_credited() {
    use sakatsu_core::credits::Group;

    assert!(
        !Group::Plumbing.on_about(),
        "the plumbing group is not shown"
    );
    assert!(
        Group::Frame.on_about() && Group::Japanese.on_about(),
        "the other two are"
    );

    let hidden: Vec<&str> = credits::all()
        .iter()
        .filter(|credit| !credit.group.on_about())
        .flat_map(|credit| credit.packages.iter().copied())
        .collect();
    assert!(
        !hidden.is_empty(),
        "nothing is hidden, so this test is asserting nothing — did the group go?"
    );
    for package in ["serde", "chrono", "zip"] {
        assert!(
            hidden.contains(&package),
            "{package} should be credited under the group the page does not print"
        );
    }
    // And what is hidden is still in the list the manifest checks read.
    for package in hidden {
        assert!(
            credited().contains(&package),
            "{package} fell out of the record"
        );
    }
}
