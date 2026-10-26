//! Installation policy shared by online and offline setup, without an application handle.
//! Platform transfer, runtime health checks and events are lent through explicit hooks.

use crate::dict::Dictionary;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub enum InstallPhase {
    Installing,
    Verifying,
}

/// Only platform effects are supplied by the shell; conversion and publication live here.
pub trait InstallHooks {
    fn detail(&mut self, id: &str, detail: &str);
    fn phase(&mut self, phase: InstallPhase);
    fn verify_model(&mut self, id: &str, work: &std::path::Path) -> Result<(), String>;
    fn install_runtime(&mut self, into: &std::path::Path) -> Result<String, String>;
}

/// One publisher per component across every setup surface.
///
/// The compatibility `fetch_component` command and the persisted setup manager have separate
/// learner-facing state because they serve different screens. They still write the same files.
/// Guarding only either screen lets a legacy Get button race the onboarding worker and gives two
/// downloaders permission to verify and rename the same artifact. BITS deduplicates its own job,
/// but it cannot make the subsequent import/publication atomic across callers.
///
/// **This is now one shape of a wider claim.** Components were the only thing reserved, so a
/// relocation or a gloss backfill could run straight through an install; see
/// [`crate::setup::claim`] for the rule and for the reproductions that motivated widening it. The
/// wording of this refusal is unchanged, because the setup screens show it.
pub type ComponentInstallGuard = crate::setup::claim::Claimed;

pub fn reserve_component_installs(
    ids: impl IntoIterator<Item = &'static str>,
) -> Result<ComponentInstallGuard, String> {
    crate::setup::claim::claim(ids.into_iter().map(crate::setup::claim::Claim::Component))
}

/// Publish a raw model as one set. This is the path the old setup missed: downloads landed in
/// `work/`, while OCR readiness looked in the parent directory. A rollback keeps an existing
/// healthy model intact if antivirus or another process locks one file mid-publication.
pub fn publish_raw_artifacts(
    component: &'static crate::setup::Component,
    work: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), String> {
    let names: Vec<_> = component
        .artifacts
        .iter()
        .map(|artifact| artifact.file)
        .collect();
    crate::setup::publication::publish_set(work, destination, &names)
}

/// Publish the four mutually dependent kanji outputs as one recoverable operation.
///
/// `rename` cannot replace an existing file on Windows. More importantly, publishing only the
/// new KANJIDIC table before a later diagram move failed left the installed details describing
/// two different releases. Validation happens in `stage`; this step moves the previous complete
/// set aside, installs every new output, and rolls the old set back on any I/O failure.
pub fn publish_kanji_details(
    stage: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), String> {
    const OUTPUTS: [&str; 4] = ["kanjidic.tsv", "krad.tsv", "krad-reverse.tsv", "kanjivg"];
    crate::setup::publication::publish_set(stage, destination, &OUTPUTS)?;
    let _ = std::fs::remove_dir(stage);
    Ok(())
}

/// Import the zips this component just fetched, and index them.
///
/// **This component's artifacts, not every zip in the directory.** Scanning the directory
/// meant a second Get re-imported everything — minutes of work for no change — and that an
/// unrelated zip somebody had parked there was imported as a dictionary under a name
/// derived from its filename.
fn import_downloaded(
    hooks: &mut dyn InstallHooks,
    component: &'static crate::setup::Component,
    dir: &std::path::Path,
    work: &std::path::Path,
    cancel: &Arc<AtomicBool>,
) -> Result<String, String> {
    let mut imported = Vec::new();
    for file in component
        .artifacts
        .iter()
        .map(|artifact| artifact.file.to_string())
    {
        if cancel.load(Ordering::Relaxed) {
            return Err("paused".into());
        }
        let name = dictionary_name(&file);

        hooks.detail("dictionaries", &format!("importing {file}…"));
        let dictionary = Dictionary::import(&work.join(&file), &dir.join(&name))
            .map_err(|error| format!("{file}: {error}"))?;

        // English search needs this and it is cheap next to the import. Doing it now
        // means one wait rather than a second one the learner has to discover.
        hooks.detail("dictionaries", &format!("indexing {name} for English…"));
        dictionary
            .index_secondary()
            .map_err(|error| format!("{file}: {error}"))?;
        imported.push(format!(
            "{name} ({} entries)",
            dictionary.meta().entry_count
        ));
    }

    if imported.is_empty() {
        return Err("nothing to import".into());
    }
    Ok(imported.join(", "))
}

/// Build every dictionary this component carries **without publishing any of them**.
///
/// The offline sibling of [`import_downloaded`], and the difference is the whole of the defect it
/// exists to repair. That one imports — build, validate, publish, prune — one dictionary at a
/// time, which is correct when the import *is* the operation. An offline package is a transaction
/// over several components, and the setup path implemented that by copying the live dictionary
/// directory, importing into the copy, and swapping the copy in. Leases are held against the
/// original paths, so pruning inside the copy removed a generation a live reader was still using,
/// and the swap then took those bytes away: the reader answered warm words from its cache and
/// **nothing** for anything it had not already looked up.
///
/// So `dir` here is the live container and nothing is copied. Each dictionary is prepared in
/// place, and `crate::setup::publication::publish_offline` decides — after every component has
/// been prepared — whether any of them is activated at all.
pub fn prepare_dictionaries(
    hooks: &mut dyn InstallHooks,
    component: &'static crate::setup::Component,
    dir: &std::path::Path,
    work: &std::path::Path,
    cancel: &Arc<AtomicBool>,
) -> Result<(Vec<crate::dict::PreparedGeneration>, String), String> {
    let mut prepared = Vec::new();
    let mut described = Vec::new();
    for file in component
        .artifacts
        .iter()
        .map(|artifact| artifact.file.to_string())
    {
        if cancel.load(Ordering::Relaxed) {
            return Err("paused".into());
        }
        let name = dictionary_name(&file);

        hooks.detail("dictionaries", &format!("importing {file}…"));
        // `prepare` builds the English gloss and kanji buckets too, so there is no second publish
        // and no window where a freshly activated dictionary cannot answer an English query.
        let generation = crate::dict::Dictionary::prepare(&work.join(&file), &dir.join(&name))
            .map_err(|error| format!("{file}: {error}"))?;
        described.push(format!("{name} ({} entries)", generation.entry_count()));
        prepared.push(generation);
    }

    if prepared.is_empty() {
        return Err("nothing to import".into());
    }
    Ok((prepared, described.join(", ")))
}

/// The container name a dictionary archive installs into.
///
/// Shared by both paths so an offline install lands where an online one would, rather than
/// beside it under a name derived slightly differently.
fn dictionary_name(file: &str) -> String {
    file.trim_end_matches(".zip")
        .split(['-', '_', '.'])
        .next()
        .unwrap_or("dictionary")
        .to_lowercase()
}

pub fn install_staged(
    component: &'static crate::setup::Component,
    into: &std::path::Path,
    work: &std::path::Path,
    cancel: &Arc<AtomicBool>,
    hooks: &mut dyn InstallHooks,
) -> Result<String, String> {
    use crate::setup::catalog::After;

    if cancel.load(Ordering::Acquire) {
        return Err("paused".into());
    }
    let count = component.artifacts.len();
    hooks.phase(InstallPhase::Installing);
    let mut verification_reported = false;
    // Downloaded is not the same as usable, and the difference is not the learner's to
    // care about.
    let detail = match component.after {
        After::Nothing => {
            hooks.phase(InstallPhase::Verifying);
            verification_reported = true;
            hooks.verify_model(component.id, work)?;
            publish_raw_artifacts(component, work, into)?;
            format!("{} verified files", count)
        }
        After::ImportDictionary => import_downloaded(hooks, component, into, work, cancel)?,
        After::BuildKanjiDetails => {
            let stage = into.join(".details-pending");
            if stage.exists() {
                std::fs::remove_dir_all(&stage).map_err(|error| error.to_string())?;
            }
            std::fs::create_dir_all(&stage).map_err(|error| error.to_string())?;
            let result = (|| {
                hooks.detail(component.id, "reading characters and name readings…");
                let characters = crate::setup::kanji_table(
                    &work.join("kanjidic2.xml.gz"),
                    &stage.join("kanjidic.tsv"),
                )
                .map_err(|error| error.to_string())?;
                hooks.detail(component.id, "indexing visual components…");
                let components = crate::setup::krad_tables(
                    &work.join("kradfile.gz"),
                    &stage.join("krad.tsv"),
                    &stage.join("krad-reverse.tsv"),
                )
                .map_err(|error| error.to_string())?;
                hooks.detail(component.id, "sanitizing stroke diagrams…");
                let diagrams = crate::setup::kanjivg_files(
                    &work.join("kanjivg-20260714-main.zip"),
                    &stage.join("kanjivg"),
                )
                .map_err(|error| error.to_string())?;
                Ok::<_, String>((characters, components, diagrams))
            })();
            let (characters, components, diagrams) = match result {
                Ok(counts) => counts,
                Err(error) => {
                    let _ = std::fs::remove_dir_all(&stage);
                    return Err(error);
                }
            };
            publish_kanji_details(&stage, into)?;
            format!("{characters} characters, {components} component rows, {diagrams} diagrams")
        }
        After::UnpackFrequency => {
            hooks.detail(component.id, "unpacking the word list…");
            let zip = work.join(component.artifacts[0].file);
            let banks =
                crate::dict::frequency::unpack(&zip, into).map_err(|error| error.to_string())?;
            // Loaded at startup, so it is not in use until the app is restarted. Said
            // here rather than left as a mystery when the ordering does not change.
            format!("{banks} files — restart to order results by it")
        }
        After::BuildWordNetTable => {
            hooks.detail(component.id, "joining the two halves…");
            let built = crate::setup::wordnet_table(
                &work.join("wnjpn-ok.tab.gz"),
                &work.join("WNdb-3.0.tar.gz"),
                &into.join("categories.tsv"),
            )
            .map_err(|error| error.to_string())?;
            format!("{built} words")
        }
        After::UnpackRuntime => {
            // The weights are already where they belong — `publish_raw_artifacts` moves every
            // downloaded file across, and a GGUF is used exactly as it arrives. Only the
            // runtime needs opening.
            hooks.detail(component.id, "unpacking the local server…");
            hooks.verify_model(component.id, work)?;
            publish_raw_artifacts(component, work, into)?;

            hooks.install_runtime(into)?
        }
        After::BuildAudioPack => {
            hooks.detail(component.id, "indexing exact pronunciation matches…");
            let recordings =
                crate::setup::audio_pack(&work.join("tofugu-pronunciation-master.zip"), into)
                    .map_err(|error| error.to_string())?;
            format!("{recordings} offline recordings")
        }
    };
    // Derived artifacts validate while they are parsed. Report the explicit verification edge
    // after that work and before cleanup so the persisted state machine still records the full
    // journey; raw OCR reports it before the slower session-and-fixture health check above.
    if !verification_reported {
        hooks.phase(InstallPhase::Verifying);
    }
    // Completed artifacts are reproducible from the catalog and waste hundreds of megabytes
    // when left beside the installed result. Unfinished `.partial` files never reach here and
    // remain available for resume.
    let _ = std::fs::remove_dir_all(work);
    Ok(detail)
}

#[cfg(test)]
mod kanji_publication_tests {
    use super::{publish_kanji_details, publish_raw_artifacts, reserve_component_installs};
    use crate::setup::catalog;
    use std::path::{Path, PathBuf};

    fn root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("sakatsu-{label}-{}-{nonce}", std::process::id()))
    }

    fn write_set(root: &Path, value: &str, complete: bool) {
        std::fs::create_dir_all(root).unwrap();
        for name in ["kanjidic.tsv", "krad.tsv", "krad-reverse.tsv"] {
            std::fs::write(root.join(name), format!("{value}-{name}")).unwrap();
        }
        if complete {
            std::fs::create_dir_all(root.join("kanjivg")).unwrap();
            std::fs::write(root.join("kanjivg/065e5.json"), value).unwrap();
        }
    }

    #[test]
    fn complete_kanji_details_publish_as_one_set() {
        let root = root("kanji-publish");
        let destination = root.join("installed");
        let stage = root.join("stage");
        write_set(&destination, "old", true);
        write_set(&stage, "new", true);

        publish_kanji_details(&stage, &destination).expect("publish");

        assert_eq!(
            std::fs::read_to_string(destination.join("kanjidic.tsv")).unwrap(),
            "new-kanjidic.tsv"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("kanjivg/065e5.json")).unwrap(),
            "new"
        );
        assert!(!stage.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incomplete_staging_rolls_the_entire_old_set_back() {
        let root = root("kanji-rollback");
        let destination = root.join("installed");
        let stage = root.join("stage");
        write_set(&destination, "old", true);
        write_set(&stage, "new", false);

        assert!(publish_kanji_details(&stage, &destination).is_err());

        assert_eq!(
            std::fs::read_to_string(destination.join("kanjidic.tsv")).unwrap(),
            "old-kanjidic.tsv"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("kanjivg/065e5.json")).unwrap(),
            "old"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn raw_ocr_files_leave_work_and_land_where_readiness_looks() {
        let root = root("raw-model-publish");
        let destination = root.join("installed");
        let work = destination.join("work");
        let component = catalog::find("paddle-ocr").expect("catalog component");
        std::fs::create_dir_all(&work).unwrap();
        for artifact in component.artifacts {
            std::fs::write(
                work.join(artifact.file),
                format!("staged-{}", artifact.file),
            )
            .unwrap();
        }

        publish_raw_artifacts(component, &work, &destination).expect("publish raw model");

        for artifact in component.artifacts {
            assert!(destination.join(artifact.file).is_file());
            assert!(!work.join(artifact.file).exists());
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(windows)]
    fn a_locked_old_model_rolls_back_files_published_before_it() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = root("locked-model-publication");
        let destination = root.join("installed");
        let work = destination.join("work");
        let component = catalog::find("paddle-ocr").expect("catalog component");
        assert!(component.artifacts.len() >= 2);
        std::fs::create_dir_all(&work).unwrap();
        for artifact in component.artifacts {
            std::fs::write(destination.join(artifact.file), "old model").unwrap();
            std::fs::write(work.join(artifact.file), "new model").unwrap();
        }
        // Allow reads but not delete-sharing: this is the real Windows failure when an old
        // inference session still has a model open, after the first artifact already moved.
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(destination.join(component.artifacts[1].file))
            .unwrap();
        assert!(publish_raw_artifacts(component, &work, &destination).is_err());
        for artifact in component.artifacts {
            assert_eq!(
                std::fs::read_to_string(destination.join(artifact.file)).unwrap(),
                "old model"
            );
        }
        drop(locked);
    }

    #[test]
    fn every_setup_surface_shares_one_component_publication_guard() {
        let _order = crate::setup::claim::one_at_a_time();
        let first = reserve_component_installs(["setup-guard-test"])
            .expect("first caller reserves component");
        assert!(reserve_component_installs(["setup-guard-test"]).is_err());
        drop(first);
        assert!(reserve_component_installs(["setup-guard-test"]).is_ok());
    }
}
