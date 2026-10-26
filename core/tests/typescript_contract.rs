//! The tracked TypeScript declarations still say what the Rust types say.
//!
//! ```text
//! cargo test -p sakatsu-core --features typescript --test typescript_contract
//! ```
//!
//! # This test does not run by default, and that is the hazard it carries
//!
//! The `TS` derives are behind the optional `typescript` feature so a shipped build carries no
//! generation machinery. The cost is that `cargo test --workspace` compiles none of this — a green
//! ordinary run says nothing about whether the contract machinery still compiles, let alone
//! whether it agrees with the frontend. So the feature is named explicitly in the verification
//! commands in `DEVELOPMENT.md`, `project notes` and `project notes`, and this header exists to say why anyone
//! who removes it has broken something silently.
//!
//! # It writes nothing
//!
//! Generation is `cargo run --bin generate-bindings`. It is deliberately not an `#[ignore]`d
//! "regenerate" test: this repository runs broad `-- --ignored` sweeps for measurements, and one
//! of those would have rewritten a tracked contract file as a side effect of asking how long
//! something takes. Routine verification must never rewrite what it is verifying.

#![cfg(feature = "typescript")]

use sakatsu_core::bindings;

fn tracked() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/bindings.ts");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Compare content, not the checkout's line-ending convention.
fn lines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// The one line that declares `name`, so a check about an enum is a check about *that* enum.
fn declaration_of(rendered: &str, name: &str) -> String {
    rendered
        .lines()
        .find(|line| line.starts_with(&format!("export type {name} = ")))
        .unwrap_or_else(|| panic!("{name} is not declared at all in the generated bindings"))
        .to_string()
}

/// The tracked declarations are what the generator produces.
#[test]
fn the_tracked_bindings_are_what_the_rust_types_produce() {
    assert_eq!(
        lines(&tracked()),
        lines(&bindings::render()),
        "\nui/src/bindings.ts no longer matches the Rust types. It is generated — regenerate it \
         rather than editing it:\n  \
         cargo run -p sakatsu-core --features typescript --bin generate-bindings\n  \
         npm --prefix ui run check\n"
    );
}

/// The provenance union is multiline in ts-rs output. Check its own declaration, not
/// coincidental occurrences of these words in unrelated types or comments.
#[test]
fn lexical_roles_and_retrieval_scopes_remain_literal() {
    let rendered = bindings::render();
    let role = declaration_of(&rendered, "LexicalRole");
    for value in ["pronoun", "suffix", "noun"] {
        assert!(
            role.contains(&format!("\"{value}\"")),
            "role lost {value}: {role}"
        );
    }

    let provenance = rendered
        .split_once("export type Retrieved = ")
        .expect("Retrieved declaration")
        .1
        .split_once("export type DictExample = ")
        .expect("Retrieved declaration terminator")
        .0;
    for value in ["unknown", "whole", "component"] {
        assert!(
            provenance.contains(&format!("\"kind\": \"{value}\"")),
            "retrieval scope lost {value}: {provenance}"
        );
    }
    assert!(
        provenance.matches("query: string").count() == 2,
        "whole and component queries must remain explicit: {provenance}"
    );
}
/// Every enum variant the reader can meet is a literal **in its own declaration**.
///
/// # Why this is pinned separately from the file comparison
///
/// The comparison catches a variant rename, because the file changes. What it cannot do is say
/// *which* guarantee broke, and a diff of a two-hundred-line generated file does not either. These
/// name the three enums the frontend switches on, so a rename fails with the variant in the
/// message.
///
/// **Scoped to the declaration, not the whole file.** An earlier version searched the rendered text
/// for `"wrap"` anywhere, which any other type's doc comment or string literal could satisfy — so
/// it could have gone on passing while `LineBreak` lost the variant entirely. A check that an
/// unrelated line can satisfy is not checking the thing it names.
///
/// **This is the gap the golden cannot close.** `model.golden.json` is a JSON import, so
/// TypeScript widens its string literals away — `model.contract.ts` has to widen `line_break`,
/// `statuses` and `Step.form` back to `string` for that reason. A variant renamed in Rust would
/// change the golden, the golden would be regenerated, and the contract check would still pass
/// while the frontend went on using the old name. Generated literal unions are what stop that.
#[test]
fn every_enum_variant_reaches_the_generated_declarations() {
    let rendered = bindings::render();

    // `Form` is sampled rather than listed in full: seventeen variants is enough that a wholesale
    // change is caught, and few enough that adding a form does not fail this for no reason.
    let expected: &[(&str, &[&str])] = &[
        ("LineBreak", &["\"none\"", "\"wrap\"", "\"break\""]),
        (
            "WordStatus",
            &["\"new\"", "\"seen\"", "\"known\"", "\"ignored\""],
        ),
        (
            "Form",
            &[
                "\"past\"",
                "\"non-past\"",
                "\"potential\"",
                "\"unclassified\"",
            ],
        ),
        // The setup screen switches on both of these to decide what it draws and which tier a
        // component is offered under.
        (
            "SetupPhase",
            &[
                "\"checking\"",
                "\"ready_to_install\"",
                "\"downloading\"",
                "\"paused_retrying\"",
                "\"installing\"",
                "\"verifying\"",
                "\"complete\"",
                "\"needs_attention\"",
            ],
        ),
        ("Tier", &["\"essential\"", "\"optional\"", "\"advanced\""]),
        // Rejections and recovery. Neither appears in any successful response, so a golden cannot
        // reach them — which is exactly why they are generated and pinned here.
        (
            "ErrorCode",
            &[
                "\"store_unavailable\"",
                "\"settings_read_only\"",
                "\"write_failed\"",
                "\"stale_recovery\"",
            ],
        ),
        (
            "DictionaryRecoveryState",
            &[
                "\"unresolved\"",
                "\"conflicting\"",
                "\"cleanup_only\"",
                "\"never_switched\"",
                "\"unreadable\"",
            ],
        ),
        (
            "InstallRecoveryState",
            &[
                "\"restored\"",
                "\"retained\"",
                "\"failed\"",
                "\"competing\"",
                "\"unidentified\"",
            ],
        ),
        (
            "LoadingOutcome",
            &["\"loaded\"", "\"failed\"", "\"not_needed\"", "\"unknown\""],
        ),
        (
            "ConstructionIdentity",
            &[
                "\"explanatory_nan\"",
                "\"contracted_motion\"",
                "\"copula_recall\"",
            ],
        ),
        (
            "ConstructionLookup",
            &["\"headword\"", "\"grammar_pattern\""],
        ),
        ("ConstructionGrammar", &["\"sentence\"", "\"auxiliary\""]),
    ];

    for (name, variants) in expected {
        let declaration = declaration_of(&rendered, name);
        for variant in *variants {
            assert!(
                declaration.contains(variant),
                "{name} lost {variant}; the reader switches on these.\n  declared as: \
                 {declaration}"
            );
        }
    }
}

/// The generated file is a literal union, not a widened `string`.
///
/// If ts-rs ever emitted `type LineBreak = string`, every test above would still pass and the
/// whole point of generating would be gone.
#[test]
fn the_enums_are_literal_unions_and_not_strings() {
    let rendered = bindings::render();
    for name in [
        "LineBreak",
        "WordStatus",
        "Form",
        "SetupPhase",
        "Tier",
        "ErrorCode",
        "DictionaryRecoveryState",
        "InstallRecoveryState",
        "LoadingOutcome",
        "ConstructionIdentity",
        "ConstructionLookup",
        "ConstructionGrammar",
    ] {
        let declaration = declaration_of(&rendered, name);
        assert!(
            declaration.contains('"'),
            "{name} is not a literal union — a widened declaration catches no renames: \
             {declaration}"
        );
        assert!(
            !declaration.contains("= string"),
            "{name} widened to string: {declaration}"
        );
    }
}

/// A JMdict sequence is a JSON number on the wire, so it is a `number` here.
///
/// ts-rs maps `i64` to `bigint`, which is right about the Rust range and wrong about what serde
/// emits. `model.golden.json` shows a plain number. The rule is that the golden decides, so
/// `Sense::sequence` carries `#[ts(as = "Option<i32>")]` — `as` and not `type`, because
/// `ts(type = "number")` replaces the whole declaration and silently drops the `| null` the
/// `Option` earns. That mistake was made here first and caught by `npm run check`; this pins the
/// correction.
#[test]
fn the_wire_number_did_not_become_a_bigint() {
    let rendered = bindings::render();
    assert!(
        !rendered.contains("bigint"),
        "a bigint reached the declarations; serde emits JSON numbers, and the golden is the \
         authority on that. Give the offending field \
         #[cfg_attr(feature = \"typescript\", ts(as = \"Option<i32>\"))] — `as`, because \
         `ts(type = ...)` replaces the whole declaration and drops the `| null` an `Option` earns."
    );
}
