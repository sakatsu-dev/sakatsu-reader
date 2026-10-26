//! Persisted first-run setup orchestration.
//!
//! The old UI owned a queue in JavaScript, so closing the window erased intent and progress.
//! This state lives beside the small roaming settings store while downloaded resources live in
//! LocalAppData (or the explicitly chosen fixed drive). BITS job ids and HTTP partials carry the
//! byte-level resume state; this module carries the learner-facing operation.

use crate::commands::{self, AnalyzerState};
use sakatsu_core::setup::resources::copy_tree;
pub use sakatsu_core::setup::transport::{
    SetupArtifactState, SetupComponentState, SetupPhase, SetupPreflight, SetupSnapshot,
    StorageSummary,
};
use sakatsu_core::setup::{catalog, human_size, Tier};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct PersistedSetup {
    phase: SetupPhase,
    generation: u64,
    consented: bool,
    analyzer_setup_pending: bool,
    welcome_seen: bool,
    text_only: bool,
    current_component: Option<String>,
    current_issue: Option<String>,
    completed: BTreeSet<String>,
    verified: BTreeSet<String>,
}

impl Default for PersistedSetup {
    fn default() -> Self {
        Self {
            phase: SetupPhase::Checking,
            generation: 0,
            consented: false,
            analyzer_setup_pending: false,
            welcome_seen: false,
            text_only: false,
            current_component: None,
            current_issue: None,
            completed: BTreeSet::new(),
            verified: BTreeSet::new(),
        }
    }
}

impl PersistedSetup {
    /// Record consent to download the essentials, and enrol the Analyzer step for a **new** reader.
    ///
    /// Only a new essentials journey introduces that step. `established` says an essential was
    /// already installed when this run began — a repair, or an installation from before this file
    /// existed, whose record reads `consented: false` because nothing ever wrote it. Without it, a
    /// repair on such a machine would push a long-standing reader back into first-run onboarding.
    fn consent(&mut self, established: bool) {
        self.analyzer_setup_pending |= !self.consented && !established;
        self.consented = true;
    }
}

/// Whether any essential is already installed: an installation being repaired or resumed rather
/// than a reader arriving for the first time. See [`PersistedSetup::consent`].
fn essentials_established(analyzer: &AnalyzerState) -> bool {
    catalog::all().iter().any(|component| {
        component.tier == Tier::Essential && commands::installed(component.id, analyzer)
    })
}

#[derive(Clone)]
pub struct SetupState {
    path: PathBuf,
    inner: Arc<Mutex<PersistedSetup>>,
    running: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
}

impl SetupState {
    pub fn load() -> Self {
        let path = crate::app_data_dir().join("setup-state.json");
        let mut persisted: PersistedSetup = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        if matches!(
            persisted.phase,
            SetupPhase::Downloading | SetupPhase::Installing | SetupPhase::Verifying
        ) {
            persisted.phase = SetupPhase::PausedRetrying;
            persisted.current_issue = Some("Setup was interrupted and is ready to resume.".into());
        }
        Self {
            path,
            inner: Arc::new(Mutex::new(persisted)),
            running: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
        }
    }

    fn read(&self) -> PersistedSetup {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Change the state and write it, **while holding the lock**.
    ///
    /// The write used to happen after the lock was released, so two writers could land on disk in
    /// the opposite order to the one they happened in. A setup worker's snapshot taken before
    /// [`setup_finish_analyzer`] could then overwrite the saved choice: memory said chosen, the file
    /// said pending, and the Analyzer step came back after a restart. Holding the lock across the
    /// write makes the file follow the same order as memory. The state is a few hundred bytes.
    ///
    /// **A closed store gate defers the write, never the change.** An update being prepared closes
    /// the gate while it waits for setup to finish; returning early then dropped the change from
    /// memory too, so a component that finished in that window was forgotten when the update gave
    /// up and setup carried on. The change always lands in memory, and since every write is the
    /// whole state, the next one that the gate admits carries it to disk.
    fn update(&self, change: impl FnOnce(&mut PersistedSetup)) {
        let write = crate::lifecycle::gates().enter(crate::lifecycle::Gate::Store);
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        change(&mut inner);
        match write {
            Ok(_write) => {
                if let Err(error) = persist(&self.path, &inner) {
                    sakatsu_core::log_warn!("setup", "could not persist setup state: {error}");
                }
            }
            Err(_) => sakatsu_core::log_warn!(
                "setup",
                "setup state kept in memory while an update is prepared; the next change writes it"
            ),
        }
    }

    fn update_confirmed(&self, change: impl FnOnce(&mut PersistedSetup)) -> Result<(), String> {
        let _write = crate::lifecycle::gates()
            .enter(crate::lifecycle::Gate::Store)
            .map_err(|e| e.to_string())?;
        let mut state = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let mut next = state.clone();
        change(&mut next);
        persist(&self.path, &next)?;
        *state = next;
        Ok(())
    }
}

#[tauri::command(async)]
pub fn setup_snapshot(
    setup: State<'_, SetupState>,
    analyzer: State<'_, AnalyzerState>,
) -> SetupSnapshot {
    refresh_verified(&setup, &analyzer);
    build_snapshot(&setup, &analyzer)
}

#[tauri::command(async)]
pub fn setup_preflight(
    setup: State<'_, SetupState>,
    analyzer: State<'_, AnalyzerState>,
) -> SetupPreflight {
    let snapshot = build_snapshot(&setup, &analyzer);
    preflight(&snapshot.storage)
}

/// Only an explicit analyzer choice dismisses this step. Completion is persisted before the
/// interface advances; a failed write must not masquerade as a choice that survived a restart.
#[tauri::command]
pub fn setup_finish_analyzer<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    setup: State<'_, SetupState>,
) -> Result<(), String> {
    setup.update_confirmed(|state| state.analyzer_setup_pending = false)?;
    emit_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn setup_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    setup: State<'_, SetupState>,
    analyzer: State<'_, AnalyzerState>,
) -> bool {
    let Ok(lifecycle) = crate::lifecycle::gates().enter(crate::lifecycle::Gate::Publication) else {
        return false;
    };
    if setup.running.swap(true, Ordering::AcqRel) {
        return false;
    }
    let snapshot = build_snapshot(&setup, &analyzer);
    let check = preflight(&snapshot.storage);
    if let Some(issue) = check.blocking_issue {
        setup.update(|state| {
            state.phase = SetupPhase::NeedsAttention;
            state.current_issue = Some(issue);
            state.welcome_seen = true;
        });
        setup.running.store(false, Ordering::Release);
        emit_changed(&app);
        return false;
    }

    let missing_ids = snapshot
        .components
        .iter()
        .filter(|component| {
            component.required && (!component.installed || component.verification != "verified")
        })
        .map(|component| component.id.as_str())
        .collect::<BTreeSet<_>>();
    let missing = catalog::all()
        .iter()
        .filter(|component| missing_ids.contains(component.id))
        .collect::<Vec<_>>();
    let established = snapshot
        .components
        .iter()
        .any(|component| component.required && component.installed);
    let worker = setup.inner().clone();
    worker.pause.store(false, Ordering::Release);
    let generation = worker.read().generation.saturating_add(1);
    if let Err(error) = worker.update_confirmed(|state| {
        state.phase = SetupPhase::Downloading;
        state.generation = generation;
        state.consent(established);
        state.welcome_seen = true;
        state.current_issue = None;
    }) {
        fail(
            &worker,
            &app,
            &format!("Could not save setup progress: {error}"),
        );
        return false;
    }
    emit_changed(&app);

    std::thread::spawn(move || {
        let _lifecycle = lifecycle;
        for component in missing {
            if worker.pause.load(Ordering::Acquire) {
                worker.update(|state| {
                    state.phase = SetupPhase::PausedRetrying;
                    state.current_issue =
                        Some("Setup is paused. Your download progress is saved.".into());
                });
                worker.running.store(false, Ordering::Release);
                emit_changed(&app);
                return;
            }
            worker.update(|state| {
                state.phase = SetupPhase::Downloading;
                state.current_component = Some(component.id.into());
                state.current_issue = None;
            });
            emit_changed(&app);
            let mut report_phase = |phase| {
                worker.update(|state| {
                    state.phase = match phase {
                        commands::InstallPhase::Installing => SetupPhase::Installing,
                        commands::InstallPhase::Verifying => SetupPhase::Verifying,
                    };
                });
                emit_changed(&app);
            };
            // `install_component` owns the component, resolves its destination, fetches and adopts
            // — in that order, under one claim. The destination used to be resolved out here,
            // before `run_fetch` claimed anything.
            match commands::install_component(
                &app,
                component,
                &worker.pause,
                Some(generation),
                Some(&mut report_phase),
            ) {
                Ok(_) => {
                    worker.update(|state| {
                        state.completed.insert(component.id.into());
                        state.verified.insert(component.id.into());
                    });
                }
                Err(error) if error == "paused" => {
                    worker.update(|state| {
                        state.phase = SetupPhase::PausedRetrying;
                        state.current_issue =
                            Some("Setup is paused. Your download progress is saved.".into());
                    });
                    worker.running.store(false, Ordering::Release);
                    emit_changed(&app);
                    return;
                }
                Err(error) => {
                    fail(&worker, &app, &error);
                    return;
                }
            }
        }

        worker.update(|state| {
            state.phase = SetupPhase::Complete;
            state.current_component = None;
            state.current_issue = None;
            state.text_only = false;
        });
        worker.running.store(false, Ordering::Release);
        emit_changed(&app);
    });
    true
}

#[tauri::command]
pub fn setup_pause<R: tauri::Runtime>(app: tauri::AppHandle<R>, setup: State<'_, SetupState>) {
    setup.pause.store(true, Ordering::Release);
    setup.update(|state| {
        state.phase = SetupPhase::PausedRetrying;
        state.current_issue = Some("Pausing after the current disk operation…".into());
    });
    emit_changed(&app);
}

#[tauri::command]
pub fn setup_use_text_only<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    setup: State<'_, SetupState>,
) {
    setup.pause.store(true, Ordering::Release);
    setup.update(|state| {
        state.text_only = true;
        state.welcome_seen = true;
        if state.phase != SetupPhase::Complete {
            state.phase = SetupPhase::PausedRetrying;
        }
        state.current_issue = None;
    });
    emit_changed(&app);
}

#[tauri::command]
pub fn setup_repair<R: tauri::Runtime>(app: tauri::AppHandle<R>, setup: State<'_, SetupState>) {
    setup.update(|state| {
        state.verified.clear();
        state.phase = SetupPhase::Checking;
        state.current_issue = None;
    });
    emit_changed(&app);
}

#[tauri::command]
pub fn setup_choose_storage<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    setup: State<'_, SetupState>,
    path: String,
) -> Result<(), String> {
    let path = PathBuf::from(path);
    ensure_writable(&path)?;
    // Changing where everything resolves is a write to the whole tree even though no bytes move:
    // an install publishing while this lands would finish into a location nothing looks at again.
    crate::owning([sakatsu_core::setup::claim::Claim::Everything], |_owned| {
        crate::set_device_data_dir(&path).map_err(|error| error.to_string())
    })??;
    setup.update(|state| {
        state.phase = SetupPhase::ReadyToInstall;
        state.current_issue = None;
    });
    emit_changed(&app);
    Ok(())
}

#[tauri::command(async)]
pub async fn setup_move_resources(target: String, remove_old: bool) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || move_resources(Path::new(&target), remove_old))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn setup_restart_artifact<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    setup: State<'_, SetupState>,
    component_id: String,
) -> Result<(), String> {
    if setup.running.load(Ordering::Acquire) {
        return Err("Pause setup before starting a component over.".into());
    }
    // **`running` is this screen's flag, not the tree's.** The compatibility `fetch_component`
    // command installs the same files and never sets it, so pausing setup said nothing about
    // whether a download was in flight — and this deletes that download's work directory.
    let component =
        catalog::find(&component_id).ok_or_else(|| "Unknown setup component".to_string())?;
    crate::owning(
        [sakatsu_core::setup::claim::Claim::Component(component.id)],
        |_owned| {
            // Resolved inside, because `destination` answers from the device-data location.
            let destination = commands::destination(component.id)
                .ok_or_else(|| "No storage destination is configured".to_string())?;
            let work = commands::archives_in(&destination);
            for artifact in component.artifacts {
                for name in [
                    artifact.file.to_string(),
                    format!("{}.partial", artifact.file),
                    format!("{}.partial.json", artifact.file),
                    format!("{}.bits-download", artifact.file),
                    format!("{}.bits-job", artifact.file),
                ] {
                    let _ = std::fs::remove_file(work.join(name));
                }
            }
            Ok::<(), String>(())
        },
    )??;
    // **Only a required component reopens setup.** Discarding an optional download's partial
    // leaves required setup exactly where it was: a finished install must not start reporting
    // itself incomplete because a pronunciation pack was set aside.
    let required = component.tier == Tier::Essential;
    setup.update(|state| {
        state.completed.remove(component.id);
        state.verified.remove(component.id);
        if required {
            state.phase = SetupPhase::ReadyToInstall;
            state.current_issue = None;
        }
    });
    emit_changed(&app);
    Ok(())
}

#[derive(Debug, Deserialize)]
struct OfflineManifest {
    package_version: String,
    compatible_app_versions: Vec<String>,
    components: Vec<OfflineComponent>,
}

#[derive(Debug, Deserialize)]
struct OfflineComponent {
    id: String,
    artifacts: Vec<OfflineArtifact>,
    licences: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OfflineArtifact {
    file: String,
    bytes: u64,
    sha256: String,
}

/// Import a browser-resumable essentials package without trusting paths or its manifest alone.
/// Every identifier, filename, byte count and hash must exist in this build's embedded release
/// catalog. All payload bytes are validated before any live directory moves.
#[tauri::command]
pub fn setup_import_offline<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    setup: State<'_, SetupState>,
    analyzer: State<'_, AnalyzerState>,
    package_path: String,
) -> bool {
    let Ok(lifecycle) = crate::lifecycle::gates().enter(crate::lifecycle::Gate::Publication) else {
        return false;
    };
    if setup.running.swap(true, Ordering::AcqRel) {
        return false;
    }
    let established = essentials_established(&analyzer);
    let worker = setup.inner().clone();
    let generation = worker.read().generation.saturating_add(1);
    if let Err(error) = worker.update_confirmed(|state| {
        state.phase = SetupPhase::Installing;
        state.generation = generation;
        state.consent(established);
        state.welcome_seen = true;
        state.current_issue = None;
    }) {
        fail(
            &worker,
            &app,
            &format!("Could not save setup progress: {error}"),
        );
        return false;
    }
    emit_changed(&app);
    std::thread::spawn(move || {
        let _lifecycle = lifecycle;
        match import_offline(&app, &worker, Path::new(&package_path)) {
            // Adoption happened inside `import_offline`, under the reservation that covers the
            // publication. Doing it here would put an unowned interval between the two.
            Ok(installed) => {
                worker.update(|state| {
                    state.completed.extend(installed.iter().cloned());
                    state.verified.extend(installed);
                    state.phase = SetupPhase::Complete;
                    state.current_component = None;
                    state.current_issue = None;
                    state.text_only = false;
                });
            }
            Err(error) => worker.update(|state| {
                state.phase = SetupPhase::NeedsAttention;
                state.current_issue = Some(error);
            }),
        }
        worker.running.store(false, Ordering::Release);
        emit_changed(&app);
    });
    true
}

/// What the offline dictionary preparation reports through.
///
/// The same `detail` line the online install emits, so the panel reads identically whichever way
/// the dictionaries arrived. The other two hooks belong to components this path does not take:
/// nothing verifies a model or unpacks a runtime while preparing a dictionary generation, and a
/// call to either would mean the dispatch above sent the wrong component here.
struct OfflineHooks<'a, R: tauri::Runtime> {
    app: &'a tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> sakatsu_core::setup::install::InstallHooks for OfflineHooks<'_, R> {
    fn detail(&mut self, id: &str, detail: &str) {
        commands::say(self.app, id, detail);
    }
    fn phase(&mut self, _phase: commands::InstallPhase) {}
    fn verify_model(&mut self, id: &str, _work: &Path) -> Result<(), String> {
        Err(format!("{id} is not a dictionary component"))
    }
    fn install_runtime(&mut self, _into: &Path) -> Result<String, String> {
        Err("no runtime is installed while preparing dictionaries".into())
    }
}

/// Take up what an offline package installed, **while its reservation is still held**.
///
/// # Why this is a function that demands the guard
///
/// Adoption used to happen at the caller, after `import_offline` had returned and dropped its
/// reservation. Nothing in the code said the two belonged together, so nothing stopped them
/// drifting apart — and an unowned interval between publishing files and rebuilding the handles
/// that read them is exactly where a move lands.
///
/// Taking `&Claimed` puts that in the signature: adoption cannot be written anywhere the
/// reservation is not in scope, and [`Claimed::require`] refuses at runtime if the guard in scope
/// is not one that covers this component. It is checked by
/// `adopting_without_the_reservation_is_refused`.
fn adopt_installed<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    installed: &[String],
    owned: &sakatsu_core::setup::claim::Claimed,
) {
    for catalogued in owned_components(installed, owned) {
        commands::adopt(app, catalogued);
    }
}

/// Which of `installed` this reservation covers — refusing rather than answering if it does not.
///
/// Separate from the adoption itself so the requirement can be exercised without an application:
/// the desktop library's own test binary cannot build a mock app, and a check nothing runs is a
/// comment with a type signature.
fn owned_components(
    installed: &[String],
    owned: &sakatsu_core::setup::claim::Claimed,
) -> Vec<&'static sakatsu_core::setup::Component> {
    installed
        .iter()
        .filter_map(|id| catalog::find(id))
        .inspect(|catalogued| {
            owned.require(sakatsu_core::setup::claim::Claim::Component(catalogued.id))
        })
        .collect()
}

#[cfg(test)]
mod offline_adoption_tests {
    use super::*;

    /// The claim registry is process-global, which is what makes it a policy. Two tests holding
    /// claims at once report each other's reservations, so these run one at a time.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serialised() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The guard is required, not decorative.
    ///
    /// A claim on some other component is still a claim, and still not permission to take up what
    /// this package installed. Without this, `adopt_installed` would accept any guard that happened
    /// to be in scope and the signature would be saying something it did not mean.
    #[test]
    #[should_panic(expected = "Claim before resolving any path")]
    fn adopting_without_the_reservation_is_refused() {
        let _order = serialised();
        let unrelated =
            sakatsu_core::setup::claim::claim([sakatsu_core::setup::claim::Claim::Component(
                "manga-ocr",
            )])
            .expect("nothing else is running");
        let _ = owned_components(&["dictionaries".to_string()], &unrelated);
    }

    /// And the reservation the offline import actually takes does cover them.
    #[test]
    fn the_offline_reservation_covers_what_it_installs() {
        let _order = serialised();
        let installed: Vec<String> = sakatsu_core::setup::catalog::all()
            .iter()
            .filter(|component| component.tier == sakatsu_core::setup::Tier::Essential)
            .map(|component| component.id.to_string())
            .collect();
        let installs = commands::reserve_component_installs(installed.iter().map(|id| {
            sakatsu_core::setup::catalog::all()
                .iter()
                .find(|component| component.id == id)
                .expect("from the catalog")
                .id
        }))
        .expect("nothing else is running");
        assert_eq!(
            owned_components(&installed, &installs).len(),
            installed.len(),
            "every component the package installs is one this reservation covers"
        );
    }
}

fn import_offline<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    setup: &SetupState,
    package: &Path,
) -> Result<Vec<String>, String> {
    const MAX_MANIFEST: u64 = 1024 * 1024;
    let file = std::fs::File::open(package).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let mut manifest_entry = archive
        .by_name("manifest.json")
        .map_err(|_| "The package has no manifest.json".to_string())?;
    if manifest_entry.size() > MAX_MANIFEST {
        return Err("The package manifest is unexpectedly large.".into());
    }
    let mut manifest_bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(&mut manifest_entry, MAX_MANIFEST + 1),
        &mut manifest_bytes,
    )
    .map_err(|error| error.to_string())?;
    drop(manifest_entry);
    let manifest: OfflineManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("Invalid package manifest: {error}"))?;
    if manifest.package_version.trim().is_empty() {
        return Err("The package version is missing.".into());
    }
    if !manifest
        .compatible_app_versions
        .iter()
        .any(|version| version == env!("CARGO_PKG_VERSION"))
    {
        return Err(format!(
            "This package is not compatible with Sakatsu Reader {}.",
            env!("CARGO_PKG_VERSION")
        ));
    }

    let required = catalog::all()
        .iter()
        .filter(|component| component.tier == Tier::Essential)
        .collect::<Vec<_>>();
    // Reserve the whole transaction before reading payload data. Starting an online repair while
    // the package is being checked must fail cleanly, not race one component into the live tree.
    let installs =
        commands::reserve_component_installs(required.iter().map(|component| component.id))?;
    let manifest_ids = manifest
        .components
        .iter()
        .map(|component| component.id.as_str())
        .collect::<BTreeSet<_>>();
    let required_ids = required
        .iter()
        .map(|component| component.id)
        .collect::<BTreeSet<_>>();
    if manifest_ids != required_ids || manifest.components.len() != required.len() {
        return Err(
            "The package does not contain exactly this release's required components.".into(),
        );
    }

    let root = crate::device_data_dir().join(format!(
        ".offline-setup-{}-{}",
        std::process::id(),
        setup.read().generation
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let result = (|| {
        let mut expected_paths = BTreeSet::new();
        for component in &manifest.components {
            let catalogued = catalog::find(&component.id).expect("ids were checked");
            if !component
                .licences
                .iter()
                .any(|licence| licence == catalogued.licence)
            {
                return Err(format!("{} has invalid licence attribution.", component.id));
            }
            let files = component
                .artifacts
                .iter()
                .map(|artifact| artifact.file.as_str())
                .collect::<BTreeSet<_>>();
            let catalog_files = catalogued
                .artifacts
                .iter()
                .map(|artifact| artifact.file)
                .collect::<BTreeSet<_>>();
            if files != catalog_files || component.artifacts.len() != catalogued.artifacts.len() {
                return Err(format!(
                    "{} contains missing, extra, or duplicate files.",
                    component.id
                ));
            }
            for artifact in &component.artifacts {
                let catalog_artifact = catalogued
                    .artifacts
                    .iter()
                    .find(|known| known.file == artifact.file)
                    .expect("filenames were checked");
                let expected_bytes = catalog_artifact.expected_bytes().ok_or_else(|| {
                    format!("{} is not pinned by this release catalog.", artifact.file)
                })?;
                let expected_hash = catalog_artifact.sha256().ok_or_else(|| {
                    format!("{} is not hashed by this release catalog.", artifact.file)
                })?;
                if artifact.bytes != expected_bytes
                    || !expected_hash.eq_ignore_ascii_case(&artifact.sha256)
                {
                    return Err(format!(
                        "{} does not match this release catalog.",
                        artifact.file
                    ));
                }
                expected_paths.insert(format!("components/{}/{}", component.id, artifact.file));
            }
        }

        let mut expected_directories = BTreeSet::from(["components/".to_string()]);
        for component in &manifest.components {
            expected_directories.insert(format!("components/{}/", component.id));
        }
        let mut seen = BTreeSet::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(|error| error.to_string())?;
            let name = entry
                .name()
                .map_err(|error| format!("The package contains an unreadable entry name: {error}"))?
                .replace('\\', "/");
            if !seen.insert(name.clone()) {
                return Err(format!("The package contains a duplicate entry: {name}"));
            }
            if name != "manifest.json"
                && !expected_paths.contains(&name)
                && !expected_directories.contains(&name)
            {
                return Err(format!("The package contains an unexpected entry: {name}"));
            }
            if entry.enclosed_name().is_none() {
                return Err(format!("The package contains an unsafe path: {name}"));
            }
        }
        if !expected_paths.iter().all(|path| seen.contains(path)) {
            return Err("The package is incomplete.".into());
        }

        // Extract and hash every payload before parsing or publishing any component.
        for component in &manifest.components {
            let work = root.join("payload").join(&component.id);
            std::fs::create_dir_all(&work).map_err(|error| error.to_string())?;
            for artifact in &component.artifacts {
                let name = format!("components/{}/{}", component.id, artifact.file);
                let mut entry = archive.by_name(&name).map_err(|error| error.to_string())?;
                if entry.size() != artifact.bytes {
                    return Err(format!("{} has the wrong size.", artifact.file));
                }
                let target = work.join(&artifact.file);
                let mut output =
                    std::fs::File::create(&target).map_err(|error| error.to_string())?;
                let mut hash = Sha256::new();
                let mut limited = std::io::Read::take(&mut entry, artifact.bytes + 1);
                let copied = std::io::copy(
                    &mut limited,
                    &mut HashingWriter::new(&mut output, &mut hash),
                )
                .map_err(|error| error.to_string())?;
                if copied != artifact.bytes {
                    return Err(format!("{} ended unexpectedly.", artifact.file));
                }
                let got = sakatsu_core::hashing::hex(hash.finalize());
                if !got.eq_ignore_ascii_case(&artifact.sha256) {
                    return Err(format!("{} failed SHA-256 verification.", artifact.file));
                }
            }
        }

        // **Two publication mechanisms, because dictionaries are not a set of files.**
        //
        // A dictionary container holds immutable generations that open readers hold leases on.
        // This loop used to copy that container like any other, import into the copy, and swap the
        // copy in — which pruned a generation whose lease lived on the original path and then took
        // the bytes a live reader was still resolving to. See
        // `core/tests/offline_dictionary_publication.rs`.
        //
        // So dictionaries are prepared in their live container and activated by
        // `publish_offline`, inside the same rollback boundary as the swaps. Everything else is
        // unchanged.
        let mut prepared = Vec::new();
        let mut dictionaries = Vec::new();
        let mut installed = Vec::new();
        for component in required {
            setup.update(|state| state.current_component = Some(component.id.into()));
            emit_changed(app);
            let live = commands::destination(component.id)
                .ok_or_else(|| format!("No destination for {}", component.id))?;
            let payload = root.join("payload").join(component.id);
            installed.push(component.id.to_string());

            if component.after == sakatsu_core::setup::catalog::After::ImportDictionary {
                setup.update(|state| state.phase = SetupPhase::Installing);
                emit_changed(app);
                let mut hooks = OfflineHooks { app };
                let (generations, _) = sakatsu_core::setup::install::prepare_dictionaries(
                    &mut hooks,
                    component,
                    &live,
                    &payload,
                    &setup.pause,
                )?;
                dictionaries.extend(generations);
                continue;
            }

            let destination = root.join("prepared").join(component.id);
            if live.exists() {
                copy_tree(&live, &destination)?;
            } else {
                std::fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
            }
            let mut report_phase = |phase| {
                setup.update(|state| {
                    state.phase = match phase {
                        commands::InstallPhase::Installing => SetupPhase::Installing,
                        commands::InstallPhase::Verifying => SetupPhase::Verifying,
                    };
                });
                emit_changed(app);
            };
            commands::install_staged(
                app,
                component,
                &destination,
                &payload,
                &setup.pause,
                Some(&mut report_phase),
            )?;
            prepared.push((component.id.to_string(), destination, live));
        }
        sakatsu_core::setup::publication::publish_offline(&root, &prepared, dictionaries)?;

        // **Taken up before the reservation is released**, which is the whole reason adoption is
        // here rather than at the caller. Returning would drop `installs` — leaving publication and
        // adoption separated by an interval in which a move could land and this session's handles
        // could be rebuilt from a location that was no longer live. The online path was corrected
        // the same way, in `install_component`; this one is reached only by the offline package and
        // was not covered by that wrapper.
        adopt_installed(app, &installed, &installs);
        Ok(installed)
    })();
    if result.is_ok() {
        let _ = std::fs::remove_dir_all(&root);
    }
    result
}

struct HashingWriter<'a, W> {
    inner: &'a mut W,
    hash: &'a mut Sha256,
}

impl<'a, W> HashingWriter<'a, W> {
    fn new(inner: &'a mut W, hash: &'a mut Sha256) -> Self {
        Self { inner, hash }
    }
}

impl<W: std::io::Write> std::io::Write for HashingWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.hash.update(&bytes[..written]);
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn move_resources(target: &Path, remove_old: bool) -> Result<String, String> {
    ensure_writable(target)?;
    // **Owned before the sources are worked out.** Every left-hand path below answers from the
    // device-data location, so resolving them first and claiming second would let another move
    // finish in between and leave this copying from directories that had just been abandoned —
    // the interleaving `crate::owning` exists to close.
    crate::owning([sakatsu_core::setup::claim::Claim::Everything], |owned| {
        let target_absolute = target
            .canonicalize()
            .map_err(|error| format!("Could not resolve the new location: {error}"))?;
        let mappings = [
            (
                crate::dictionaries_dir(),
                target_absolute.join("dictionaries"),
            ),
            (crate::kanji_dir(), target_absolute.join("kanji")),
            (crate::wordnet_dir(), target_absolute.join("wordnet")),
            (crate::model_dir(), target_absolute.join("models/manga-ocr")),
            (
                crate::paddle_model_dir(),
                target_absolute.join("models/ppocrv6"),
            ),
            (
                crate::translation_model_dir(),
                target_absolute.join("models/fugumt-ja-en"),
            ),
            (
                crate::audio_dir(),
                target_absolute.join("audio/tofugu-vocabulary"),
            ),
        ];
        sakatsu_core::setup::resources::relocate(
            owned,
            &target_absolute,
            &mappings,
            remove_old,
            |path| crate::set_device_data_dir(path).map_err(|error| error.to_string()),
        )
    })?
}

fn refresh_verified(setup: &SetupState, analyzer: &AnalyzerState) {
    let current = setup.read();
    let mut newly_verified = Vec::new();
    for component in catalog::all()
        .iter()
        .filter(|component| component.tier == Tier::Essential)
    {
        if current.verified.contains(component.id) || !commands::installed(component.id, analyzer) {
            continue;
        }
        let healthy = match component.id {
            "manga-ocr" => commands::verify_raw_component("manga-ocr", &crate::model_dir()).is_ok(),
            "paddle-ocr" => {
                commands::verify_raw_component("paddle-ocr", &crate::paddle_model_dir()).is_ok()
            }
            _ => true,
        };
        if healthy {
            newly_verified.push(component.id.to_string());
        }
    }
    if !newly_verified.is_empty() {
        setup.update(|state| state.verified.extend(newly_verified));
    }
}

pub(crate) fn build_snapshot(setup: &SetupState, analyzer: &AnalyzerState) -> SetupSnapshot {
    let mut persisted = setup.read();
    let components = catalog::all()
        .iter()
        .filter(|component| component.id != "paddle-vl")
        .map(|component| {
            let installed = commands::installed(component.id, analyzer);
            let required = component.tier == Tier::Essential;
            let current = persisted.current_component.as_deref() == Some(component.id);
            let verified = persisted.verified.contains(component.id);
            let artifacts = artifact_states(component, required, installed, verified);
            let downloaded_bytes = artifacts
                .iter()
                .map(|artifact| artifact.downloaded_bytes)
                .sum();
            let exact_total = artifacts
                .iter()
                .map(|artifact| artifact.expected_download_bytes)
                .sum::<u64>();
            let total_bytes = if exact_total == 0 {
                component.bytes
            } else {
                exact_total
            };
            SetupComponentState {
                id: component.id.into(),
                label: component.label.into(),
                required,
                installed,
                installed_version: installed.then(|| catalog_version(component)),
                transfer_state: if installed {
                    "installed"
                } else if current && setup.running.load(Ordering::Acquire) {
                    "transferring"
                } else if downloaded_bytes > 0 {
                    "paused"
                } else {
                    "not_started"
                }
                .into(),
                downloaded_bytes,
                total_bytes,
                verification: if verified {
                    "verified"
                } else if installed {
                    "pending"
                } else {
                    "not_checked"
                }
                .into(),
                repair_eligible: installed || downloaded_bytes > 0,
                artifacts,
            }
        })
        .collect::<Vec<_>>();
    let required_ready = components
        .iter()
        .filter(|component| component.required)
        .all(|component| component.installed && component.verification == "verified");
    if required_ready && persisted.phase != SetupPhase::Complete {
        setup.update(|state| {
            state.phase = SetupPhase::Complete;
            state.current_issue = None;
            state.current_component = None;
        });
        persisted = setup.read();
    } else if !required_ready && persisted.phase == SetupPhase::Checking {
        setup.update(|state| state.phase = SetupPhase::ReadyToInstall);
        persisted = setup.read();
    }
    let existing_install = components.iter().any(|component| component.installed);
    let storage = storage_summary(&components);
    SetupSnapshot {
        phase: persisted.phase,
        generation: persisted.generation,
        required_ready,
        analyzer_setup_pending: persisted.analyzer_setup_pending,
        resumable: components
            .iter()
            .any(|component| component.downloaded_bytes > 0),
        text_only: persisted.text_only,
        welcome_seen: persisted.welcome_seen,
        existing_install,
        current_component: persisted.current_component,
        current_issue: persisted.current_issue,
        storage,
        components,
    }
}

fn storage_summary(components: &[SetupComponentState]) -> StorageSummary {
    const RESERVE: u64 = 512 * 1024 * 1024;
    let missing = components
        .iter()
        .filter(|component| component.required && !component.installed)
        .collect::<Vec<_>>();
    let download_bytes: u64 = missing.iter().map(|component| component.total_bytes).sum();
    let installed_bytes = download_bytes;
    let staging_bytes = missing
        .iter()
        .map(|component| component.total_bytes)
        .max()
        .unwrap_or(0);
    let peak_bytes = download_bytes
        .saturating_add(installed_bytes)
        .saturating_add(staging_bytes)
        .saturating_add(RESERVE);
    let target = crate::device_data_dir();
    StorageSummary {
        target: target.display().to_string(),
        download_bytes,
        installed_bytes,
        staging_bytes,
        peak_bytes,
        available_bytes: available_space(&target),
        reserve_bytes: RESERVE,
    }
}

fn preflight(storage: &StorageSummary) -> SetupPreflight {
    let target = PathBuf::from(&storage.target);
    let writable = ensure_writable(&target).is_ok();
    let enough_space = storage
        .available_bytes
        .is_some_and(|available| available >= storage.peak_bytes);
    let network_available = ("github.com", 443)
        .to_socket_addrs()
        .is_ok_and(|mut addresses| addresses.next().is_some());
    let supported_windows = cfg!(windows);
    let supported_architecture = cfg!(target_arch = "x86_64");
    let blocking_issue = if !supported_windows || !supported_architecture {
        Some("Sakatsu Reader currently supports Windows 10/11 on x64 processors.".into())
    } else if !writable {
        Some("The selected storage location is not writable. Choose another local folder.".into())
    } else if !enough_space {
        Some(format!(
            "Setup needs {} free including its safety reserve. Choose another storage location.",
            human_size(storage.peak_bytes)
        ))
    } else if !network_available {
        Some(
            "No network connection is available. Reconnect or install from an essentials package."
                .into(),
        )
    } else {
        None
    };
    SetupPreflight {
        target_location: storage.target.clone(),
        writable,
        required_bytes: storage.download_bytes,
        available_bytes: storage.available_bytes,
        peak_bytes: storage.peak_bytes,
        enough_space,
        network_available,
        supported_windows,
        supported_architecture,
        // The command is running inside WebView2, so reaching it is the prerequisite check.
        webview2: true,
        blocking_issue,
    }
}

fn artifact_states(
    component: &sakatsu_core::setup::Component,
    required: bool,
    installed: bool,
    verified: bool,
) -> Vec<SetupArtifactState> {
    let destination = commands::destination(component.id);
    let work = destination.as_deref().map(commands::archives_in);
    component
        .artifacts
        .iter()
        .map(|artifact| {
            let candidates = work
                .as_ref()
                .map(|work| {
                    [
                        work.join(artifact.file),
                        work.join(format!("{}.partial", artifact.file)),
                        work.join(format!("{}.bits-download", artifact.file)),
                    ]
                })
                .unwrap_or_else(|| [PathBuf::new(), PathBuf::new(), PathBuf::new()]);
            let partial = candidates.iter().find(|path| path.is_file());
            let downloaded_bytes = candidates
                .iter()
                .filter_map(|path| path.metadata().ok().map(|metadata| metadata.len()))
                .max()
                .unwrap_or(0);
            let transfer_job = work
                .as_ref()
                .and_then(|work| {
                    std::fs::read_to_string(work.join(format!("{}.bits-job", artifact.file))).ok()
                })
                .map(|job| job.trim().to_string())
                .filter(|job| !job.is_empty());
            let expected = artifact.expected_bytes().unwrap_or(artifact.min_bytes);
            SetupArtifactState {
                file: artifact.file.into(),
                version: artifact_version(artifact),
                immutable_source: artifact.url.into(),
                required,
                expected_download_bytes: expected,
                // Raw models install byte-for-byte; derived components need at least their
                // verified source bytes and report the component's measured peak separately.
                expected_installed_bytes: expected,
                sha256: artifact.sha256().unwrap_or_default().into(),
                downloaded_bytes,
                partial_file: partial.map(|path| path.display().to_string()),
                transfer_job,
                verification: if verified {
                    "verified"
                } else if installed {
                    "pending"
                } else if downloaded_bytes > 0 {
                    "partial"
                } else {
                    "not_checked"
                }
                .into(),
            }
        })
        .collect()
}

fn catalog_version(component: &sakatsu_core::setup::Component) -> String {
    let versions = component
        .artifacts
        .iter()
        .map(artifact_version)
        .collect::<BTreeSet<_>>();
    versions.into_iter().collect::<Vec<_>>().join(" + ")
}

fn artifact_version(artifact: &sakatsu_core::setup::Artifact) -> String {
    artifact
        .url
        .split("/releases/download/")
        .nth(1)
        .and_then(|tail| tail.split('/').next())
        .or_else(|| {
            artifact
                .url
                .split("/resolve/")
                .nth(1)
                .and_then(|tail| tail.split('/').next())
        })
        .filter(|version| *version != "main")
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            artifact
                .sha256()
                .map(|hash| format!("sha256-{}", &hash[..12]))
                .unwrap_or_else(|| "unversioned".into())
        })
}

fn ensure_writable(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|error| error.to_string())?;
    let probe = path.join(format!(".sakatsu-write-test-{}", std::process::id()));
    std::fs::write(&probe, b"ok").map_err(|error| error.to_string())?;
    std::fs::remove_file(probe).map_err(|error| error.to_string())?;
    Ok(())
}

fn available_space(path: &Path) -> Option<u64> {
    let mut current = path;
    while !current.exists() {
        current = current.parent()?;
    }
    fs2::available_space(current).ok()
}

fn persist(path: &Path, state: &PersistedSetup) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension("json.pending");
    let backup = path.with_extension("json.old");
    let bytes = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(&backup);
    if path.exists() {
        std::fs::rename(path, &backup).map_err(|error| error.to_string())?;
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::rename(&backup, path);
        return Err(error.to_string());
    }
    let _ = std::fs::remove_file(backup);
    Ok(())
}

fn fail<R: tauri::Runtime>(setup: &SetupState, app: &tauri::AppHandle<R>, issue: &str) {
    setup.update(|state| {
        state.phase = SetupPhase::NeedsAttention;
        state.current_issue = Some(issue.into());
    });
    setup.running.store(false, Ordering::Release);
    emit_changed(app);
}

fn emit_changed<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let _ = app.emit("setup-snapshot-changed", ());
}

use std::net::ToSocketAddrs;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzer_step_survives_restart_without_reopening_completed_choices() {
        let old: PersistedSetup =
            serde_json::from_str(r#"{"consented":true,"phase":"complete"}"#).unwrap();
        assert!(
            !old.analyzer_setup_pending,
            "old installations are not enrolled retroactively"
        );
        let mut fresh = PersistedSetup::default();
        fresh.consent(false);
        let mut resumed: PersistedSetup =
            serde_json::from_slice(&serde_json::to_vec(&fresh).unwrap()).unwrap();
        assert!(resumed.analyzer_setup_pending);
        resumed.consent(false);
        assert!(resumed.analyzer_setup_pending);
        resumed.analyzer_setup_pending = false;
        resumed.consent(true);
        assert!(
            !resumed.analyzer_setup_pending,
            "a later repair preserves the explicit choice"
        );
    }

    /// **The case the serde default alone does not establish.** An installation from before the
    /// setup record existed — or whose record could not be read — loads with `consented: false`.
    /// Repairing it is not a new journey, and must not enrol the Analyzer step.
    #[test]
    fn repairing_an_established_installation_does_not_reopen_onboarding() {
        for record in [None, Some(r#"{"phase":"needs_attention"}"#)] {
            let mut state: PersistedSetup = record
                .map(|json| serde_json::from_str(json).unwrap())
                .unwrap_or_default();
            assert!(!state.consented, "nothing ever recorded consent");
            state.consent(true);
            assert!(
                !state.analyzer_setup_pending,
                "a repair of established essentials ({record:?}) stays out of onboarding"
            );
            assert!(state.consented);
        }
        // And a reader with nothing installed is enrolled, as before.
        let mut new = PersistedSetup::default();
        new.consent(false);
        assert!(new.analyzer_setup_pending);
    }

    /// **A saved choice is never followed on disk by an older state.** Progress writes run on other
    /// threads while the choice is saved and re-saved; straight after each save the file must say
    /// what was saved. With the write outside the lock, a progress snapshot taken just before the
    /// save could land just after it and put the step back — a race, so it is given many chances.
    #[test]
    fn a_saved_choice_is_never_followed_by_an_older_state_on_disk() {
        let dir = std::env::temp_dir().join(format!(
            "sakatsu-setup-state-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let state = SetupState {
            path: dir.join("setup-state.json"),
            inner: Arc::new(Mutex::new(PersistedSetup::default())),
            running: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
        };
        // The file is replaced by rename, so a read can land in the gap between two renames.
        let on_disk = |state: &SetupState| -> PersistedSetup {
            loop {
                if let Some(read) = std::fs::read(&state.path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                {
                    return read;
                }
                std::thread::yield_now();
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let writers: Vec<_> = (0..4)
            .map(|_| {
                let worker = state.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    let mut generation = 0;
                    // Paced like real progress writes. Back to back, four writers re-take an unfair
                    // lock faster than the saving thread can get it, and the test starves itself.
                    while !stop.load(Ordering::Acquire) {
                        generation += 1;
                        worker.update(|s| s.generation = generation);
                        std::thread::sleep(std::time::Duration::from_micros(300));
                    }
                })
            })
            .collect();
        let mut stale = 0;
        for _ in 0..200 {
            state
                .update_confirmed(|s| s.analyzer_setup_pending = true)
                .unwrap();
            state
                .update_confirmed(|s| s.analyzer_setup_pending = false)
                .unwrap();
            if on_disk(&state).analyzer_setup_pending {
                stale += 1;
            }
        }
        stop.store(true, Ordering::Release);
        for writer in writers {
            writer.join().unwrap();
        }
        assert_eq!(
            stale, 0,
            "an older state reached the file after the choice was saved"
        );
        assert!(!on_disk(&state).analyzer_setup_pending);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_analyzer_choice_write_keeps_the_step_pending() {
        // A regular file cannot be the parent directory. Nothing is created or removed here.
        let state = SetupState {
            path: Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml/setup-state.json"),
            inner: Arc::new(Mutex::new(PersistedSetup {
                analyzer_setup_pending: true,
                ..PersistedSetup::default()
            })),
            running: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
        };
        assert!(state
            .update_confirmed(|s| s.analyzer_setup_pending = false)
            .is_err());
        assert!(state.read().analyzer_setup_pending);
    }

    #[test]
    fn interrupted_phases_resume_as_paused() {
        let mut state = PersistedSetup {
            phase: SetupPhase::Downloading,
            ..PersistedSetup::default()
        };
        if matches!(
            state.phase,
            SetupPhase::Downloading | SetupPhase::Installing | SetupPhase::Verifying
        ) {
            state.phase = SetupPhase::PausedRetrying;
        }
        assert_eq!(state.phase, SetupPhase::PausedRetrying);
    }

    #[test]
    fn peak_space_includes_staging_and_reserve() {
        let components = vec![SetupComponentState {
            id: "one".into(),
            label: "one".into(),
            required: true,
            installed: false,
            installed_version: None,
            transfer_state: "not_started".into(),
            downloaded_bytes: 0,
            total_bytes: 100,
            verification: "not_checked".into(),
            repair_eligible: false,
            artifacts: Vec::new(),
        }];
        let summary = storage_summary(&components);
        assert_eq!(summary.download_bytes, 100);
        assert_eq!(summary.installed_bytes, 100);
        assert_eq!(summary.staging_bytes, 100);
        assert_eq!(summary.peak_bytes, 300 + 512 * 1024 * 1024);
    }
}
