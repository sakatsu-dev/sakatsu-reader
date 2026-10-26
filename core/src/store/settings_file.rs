//! What `settings.json` looks like on disk, and how an older or newer one is handled.
//!
//! # Why an envelope rather than a field
//!
//! [`Settings`](super::Settings) carries `#[serde(default)]` **at struct level**, which is
//! load-bearing for everything else: a field absent from an older file is filled from
//! `Settings::default()` rather than failing the parse. That is exactly why a `version` field
//! inside `Settings` cannot work — every legacy file would be filled with the *current* version,
//! and a v0 file would be indistinguishable from a current one. The one thing a version number
//! has to do, it could not do.
//!
//! So the version lives outside:
//!
//! ```json
//! { "version": 1, "settings": { … } }
//! ```
//!
//! A file with no such wrapper is version 0, which is every file written before this existed.
//!
//! # What happens to each shape
//!
//! | on disk | what happens |
//! |---|---|
//! | no envelope | read as v0, backed up once, rewritten as the current version |
//! | envelope, current version | read, nothing written |
//! | envelope, **newer** version | read as best it can, and **never written back** |
//! | one field invalid | that field defaults, every other value is preserved |
//! | unreadable as JSON at all | set aside by `Store::open`, as before |
//!
//! The newer-version row is the one worth dwelling on. Before this, a settings file from a
//! future build would fail to parse, be set aside whole, and be replaced with defaults — so
//! opening an older build once would cost a learner every preference they had. Now it is read
//! for what can be read and left alone.
//!
//! # Migration is backed up, and publishes only on success
//!
//! The original is copied to `settings.json.v0` **before** the migrated file is written, and the
//! write goes through `write_atomically`, so an interruption leaves either the original or the
//! complete migrated file and never a half of one. Restarting re-migrates, which is safe because
//! migration is a pure function of what is on disk.

use serde_json::{Map, Value};

use super::Settings;

/// The settings schema this build writes.
///
/// **Version 1 is not a change of shape.** It is the first version to *say* what shape it is, so
/// that version 2 has something to migrate from. The v0 → v1 step alters no field: the existing
/// `#[serde(alias)]`es and `normalise()` already handle everything that has changed so far, and
/// pretending otherwise by rewriting values here would duplicate them.
pub const SETTINGS_VERSION: u32 = 1;

/// The key the version lives under, and the key the settings live under.
const VERSION: &str = "version";
const SETTINGS: &str = "settings";

/// What was read, and what had to be done to read it.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    /// The version the file declared. `0` for a file written before the envelope existed.
    pub found: u32,
    /// Field names that could not be read and fell back to their default.
    ///
    /// **Named individually, because "your settings were reset" is a different sentence from
    /// "your font size was reset".** The first is what a learner used to be told when one value
    /// was out of shape.
    pub repaired: Vec<String>,
}

impl Loaded {
    /// Whether this file was written by a build newer than this one.
    ///
    /// A store in this state must not write settings: doing so would silently downgrade a file
    /// whose extra content this build cannot even see, let alone preserve.
    pub fn from_the_future(&self) -> bool {
        self.found > SETTINGS_VERSION
    }

    /// Whether the file on disk needs rewriting to reach the current version.
    pub fn needs_migration(&self) -> bool {
        self.found < SETTINGS_VERSION
    }
}

/// Read a settings file of any version.
///
/// Only a document that is not JSON at all is an error; everything else is recovered as far as it
/// can be, because the alternative is discarding preferences a learner chose.
pub fn read(json: &str) -> Result<Loaded, serde_json::Error> {
    let document: Value = serde_json::from_str(json)?;
    let (found, body) = unwrap_envelope(document);

    match serde_json::from_value::<Settings>(body.clone()) {
        Ok(settings) => Ok(Loaded {
            settings,
            found,
            repaired: Vec::new(),
        }),
        Err(_) => {
            let (settings, repaired) = field_by_field(body);
            Ok(Loaded {
                settings,
                found,
                repaired,
            })
        }
    }
}

/// Serialise settings as the current version's file.
pub fn write(settings: &Settings) -> Result<Vec<u8>, serde_json::Error> {
    let mut document = Map::new();
    document.insert(VERSION.into(), Value::from(SETTINGS_VERSION));
    document.insert(SETTINGS.into(), serde_json::to_value(settings)?);
    serde_json::to_vec_pretty(&Value::Object(document))
}

/// Write known settings while retaining fields serde ignored in the original document.
///
/// The original stays at the file boundary, not in the IPC DTO: callers need not echo fields
/// their build cannot name. Recognition uses the actual Settings deserializer. In particular,
/// an alias inserted beside its canonical field is rejected as a duplicate, not mistaken for
/// an extension that should be restored on every save.
pub fn write_preserving(
    settings: &Settings,
    original: Option<&str>,
) -> Result<Vec<u8>, serde_json::Error> {
    let Some(original) = original else {
        return write(settings);
    };
    let original: Value = serde_json::from_str(original)?;
    let (_, old_body) = unwrap_envelope(original.clone());
    let known = serde_json::to_value(settings)?;
    let mut body = known.clone();
    preserve_ignored(&old_body, &known, &known, &mut body, "");
    let mut document = match original {
        Value::Object(mut object)
            if object.get(VERSION).and_then(Value::as_u64).is_some()
                && object.contains_key(SETTINGS) =>
        {
            object.remove(VERSION);
            object.remove(SETTINGS);
            object
        }
        _ => Map::new(),
    };
    document.insert(VERSION.into(), Value::from(SETTINGS_VERSION));
    document.insert(SETTINGS.into(), body);
    serde_json::to_vec_pretty(&Value::Object(document))
}

fn pointer_key(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn preserve_ignored(old: &Value, current: &Value, known: &Value, output: &mut Value, path: &str) {
    match (old, current) {
        (Value::Object(old), Value::Object(current)) => {
            for (key, value) in old {
                let child_path = format!("{path}/{}", pointer_key(key));
                if let Some(current) = current.get(key) {
                    preserve_ignored(value, current, known, output, &child_path);
                    continue;
                }
                let mut trial = known.clone();
                let Some(parent) = trial.pointer_mut(path).and_then(Value::as_object_mut) else {
                    continue;
                };
                parent.insert(key.clone(), value.clone());
                let ignored = serde_json::from_value::<Settings>(trial)
                    .and_then(serde_json::to_value)
                    .is_ok_and(|round_trip| round_trip == *known);
                if ignored {
                    if let Some(parent) = output.pointer_mut(path).and_then(Value::as_object_mut) {
                        parent.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (Value::Array(old), Value::Array(current)) => {
            // Named presets can be reordered or removed. Index matching would attach one
            // preset's extension to another. Only a unique unchanged name establishes identity;
            // deleting or renaming a preset deliberately ends that identity.
            for (index, entry) in current.iter().enumerate() {
                let Some(name) = entry.get("name").and_then(Value::as_str) else {
                    continue;
                };
                if current
                    .iter()
                    .filter(|item| item.get("name").and_then(Value::as_str) == Some(name))
                    .count()
                    != 1
                {
                    continue;
                }
                let mut matching = old
                    .iter()
                    .filter(|item| item.get("name").and_then(Value::as_str) == Some(name));
                if let (Some(previous), None) = (matching.next(), matching.next()) {
                    preserve_ignored(previous, entry, known, output, &format!("{path}/{index}"));
                }
            }
        }
        _ => {}
    }
}

/// Split a document into its declared version and its settings body.
///
/// **Both keys are required**, and that is deliberate rather than cautious. A bare v0 file is an
/// object of settings fields; treating any object with a `version` number as an envelope would
/// misread a hypothetical future setting called `version` as a schema marker and then throw the
/// rest of the file away.
fn unwrap_envelope(document: Value) -> (u32, Value) {
    let Some(object) = document.as_object() else {
        // Not an object at all — a list, a string, a number. There is nothing to recover, and
        // `field_by_field` will say so.
        return (0, document);
    };
    let version = object.get(VERSION).and_then(Value::as_u64);
    let body = object.get(SETTINGS);
    match (version, body) {
        (Some(version), Some(body)) => (version.min(u64::from(u32::MAX)) as u32, body.clone()),
        _ => (0, document),
    }
}

/// Rebuild settings from a file where at least one value is the wrong shape.
///
/// **One bad value used to cost every value.** `Settings` deserialization fails as a whole, so a
/// hand-edited `"font_size": "big"` made the entire file unparseable, `Store::open` set it aside,
/// and the learner got defaults for their deck, their palettes, their bindings and their chords.
/// Nothing about the other forty fields was wrong.
///
/// So: accept each stored field only if the accumulated result still parses. Round-tripping
/// through `Settings` per field rather than inspecting types means the acceptance test is the real
/// one — aliases, enums, clamped ranges and all — instead of a second opinion about it that could
/// disagree.
///
/// # Why this starts from nothing rather than from the defaults
///
/// It used to seed the map with `Settings::default()` serialised, so **every canonical key was
/// already present**. Inserting a legacy alias then handed serde two keys for one field —
/// `collect_sentences` and its `#[serde(alias = "log_content")]` — which is a duplicate-field
/// error. The alias was rejected as unreadable and the learner's real setting was replaced by a
/// default: the recovery path damaging exactly the files it exists to rescue, and the note saying
/// those fields "could not be read" when they were perfectly readable.
///
/// Seeding is not needed, because `Settings` carries `#[serde(default)]` at struct level: a map of
/// only the accepted keys deserializes with everything else filled from `Settings::default()`.
/// That is the same mechanism an older file already relies on, used once instead of twice.
///
/// A file naming **both** an alias and its canonical key is a real duplicate, and the second one
/// is refused — which is the honest answer, and now the only case that reaches it.
///
/// Fields are applied in the file's own order, which `serde_json` preserves for objects, so the
/// outcome does not depend on hash iteration.
fn field_by_field(body: Value) -> (Settings, Vec<String>) {
    let Some(stored) = body.as_object() else {
        return (Settings::default(), vec!["<the whole file>".to_string()]);
    };
    let mut accepted = Map::new();
    let mut repaired = Vec::new();

    for (name, value) in stored {
        let mut trial = accepted.clone();
        trial.insert(name.clone(), value.clone());
        if serde_json::from_value::<Settings>(Value::Object(trial.clone())).is_ok() {
            accepted = trial;
        } else {
            repaired.push(name.clone());
        }
    }

    let settings =
        serde_json::from_value(Value::Object(accepted)).unwrap_or_else(|_| Settings::default());
    (settings, repaired)
}

/// One sentence for the learner about what was repaired, if anything was.
pub fn repair_note(repaired: &[String]) -> Option<String> {
    match repaired.len() {
        0 => None,
        1 => Some(format!(
            "One setting could not be read ({}) and was returned to its default. Everything else \
             you had set was kept.",
            repaired[0]
        )),
        _ => Some(format!(
            "{} settings could not be read ({}) and were returned to their defaults. Everything \
             else you had set was kept.",
            repaired.len(),
            repaired.join(", ")
        )),
    }
}
