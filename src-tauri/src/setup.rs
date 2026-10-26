//! First-run setup: what is installed, what is not, and what to do about it
//! (`project notes` §9, phase 6).
//!
//! 屠つ has a built-in analyzer, but dictionaries, the two required recognition models,
//! richer optional Ichiran analysis and translation all depend on data or system software
//! it cannot ship. Together they are several gigabytes, and project notes's §6 rule is that
//! data is fetched rather than bundled.
//!
//! A fresh install can already segment pasted Japanese. The supported setup adds lookup and
//! both complementary OCR engines, while the richer analyzer and translation remain upgrades.
//! Readiness is reported per component so the setup screen can say what is missing and offer
//! the appropriate action.
//!
//! Nothing starts without a click. Downloads state their size first; Ichiran opens a
//! visible guided console because WSL, Docker and possible administrator approval are
//! decisions the user has to see rather than background work the app can hide.

use serde::Serialize;

/// One thing 屠つ needs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Component {
    /// What it is called in the UI.
    pub name: &'static str,
    pub ready: bool,
    /// What was found, or what was looked for and missing.
    pub detail: String,
    /// The command that fixes it, when one exists.
    pub fix: Option<&'static str>,
    /// Whether the supported installation is incomplete without it.
    pub required: bool,
    /// The `setup::catalog` id this row is about, so the panel can offer its Get button.
    ///
    /// An id and not the label. Joining these two lists on their prose looked fine and
    /// silently matched four rows of seven — "Recognition — manga-ocr" is called "Screen
    /// capture" in the catalog, because the two lists describe the same thing to different
    /// readers. A shared identifier cannot drift that way.
    pub catalog: Option<&'static str>,
}

/// Everything, in the order a first run should deal with it.
#[derive(Debug, Clone, Serialize)]
pub struct Readiness {
    pub components: Vec<Component>,
    /// Zips sitting in the dictionaries directory that have not been imported yet.
    pub importable: Vec<String>,
}

impl Readiness {
    /// Whether every required setup component is ready.
    pub fn required_ready(&self) -> bool {
        self.components
            .iter()
            .filter(|component| component.required)
            .all(|component| component.ready)
    }

    /// Components that are absent but merely make the app better.
    ///
    /// Distinguished from broken ones so the panel can offer an upgrade without
    /// implying something is wrong: a fresh install is *working*, not misconfigured.
    pub fn optional_upgrades(&self) -> usize {
        self.components
            .iter()
            .filter(|component| !component.required && !component.ready)
            .count()
    }
}

/// Describe a dictionary directory: how many are imported, and which zips await import.
///
/// Pure, so the awkward cases — a zip already imported, an index with no zip — are
/// testable without a filesystem full of dictionaries.
pub fn dictionary_state(zips: &[String], imported: &[String]) -> (Vec<String>, Vec<String>) {
    let already: Vec<String> = imported.to_vec();
    let pending: Vec<String> = zips
        .iter()
        .filter(|zip| {
            // `jitendex-yomitan.zip` imports into `jitendex`. Matching on the stem's
            // first word keeps that link without hard-coding either name.
            let stem = zip
                .trim_end_matches(".zip")
                .split(['-', '_', '.'])
                .next()
                .unwrap_or("")
                .to_lowercase();
            !stem.is_empty() && !already.iter().any(|name| name.to_lowercase() == stem)
        })
        .cloned()
        .collect();
    (already, pending)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zip_that_has_already_been_imported_is_not_offered_again() {
        let (imported, pending) = dictionary_state(
            &["jitendex-yomitan.zip".into(), "JMnedict.zip".into()],
            &["jitendex".into()],
        );
        assert_eq!(imported, ["jitendex"]);
        assert_eq!(pending, ["JMnedict.zip"], "only the un-imported one");
    }

    #[test]
    fn everything_imported_leaves_nothing_pending() {
        let (_, pending) = dictionary_state(
            &["jitendex-yomitan.zip".into(), "JMnedict.zip".into()],
            &["jitendex".into(), "jmnedict".into()],
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn an_index_with_no_zip_left_is_still_counted_as_imported() {
        // The zips can be deleted once imported; the index is what matters.
        let (imported, pending) = dictionary_state(&[], &["jitendex".into()]);
        assert_eq!(imported, ["jitendex"]);
        assert!(pending.is_empty());
    }

    #[test]
    fn a_missing_optional_component_is_an_upgrade_not_a_fault() {
        // The distinction the panel needs: no Ichiran means "could be better", not
        // "broken". A fresh install must not look misconfigured.
        let component = |name, ready, required| Component {
            name,
            ready,
            detail: String::new(),
            fix: None,
            required,
            catalog: None,
        };
        let readiness = Readiness {
            components: vec![
                component("Analyzer", true, false),
                component("Dictionaries", false, false),
                component("Translation", false, false),
            ],
            importable: Vec::new(),
        };
        assert!(readiness.required_ready(), "nothing required is missing");
        assert_eq!(readiness.optional_upgrades(), 2);
    }

    #[test]
    fn readiness_is_decided_by_the_required_components_alone() {
        let component = |name, ready, required| Component {
            name,
            ready,
            detail: String::new(),
            fix: None,
            required,
            catalog: None,
        };
        let readiness = Readiness {
            components: vec![
                component("Recognition — manga-ocr", true, true),
                // Optional and missing: the required installation is still complete.
                component("Translation", false, false),
            ],
            importable: Vec::new(),
        };
        assert!(readiness.required_ready());

        let blocked = Readiness {
            components: vec![component("Recognition — PP-OCRv6", false, true)],
            importable: Vec::new(),
        };
        assert!(!blocked.required_ready());
    }
}
