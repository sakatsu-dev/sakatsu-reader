//! One writer for user-selected dictionaries and publisher updates. Checks retain the
//! generation they described; installation refuses if that generation has since changed.
use crate::commands::{self, DictionaryState};
use sakatsu_core::dict::managed::{
    self, DictionaryManagerSnapshot, DictionaryOperation, DictionaryPreferences, ImportPreview,
    KanjiSource, ManagedDictionary, ResourceKind,
};
use sakatsu_core::dict::{Dictionary, DictionaryMeta};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::{Emitter, Manager};

const KANJI: &str = "kanjidic2";
const KANJI_URL: &str = "https://www.edrdg.org/kanjidic/kanjidic2.xml.gz";

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    preferences: DictionaryPreferences,
    last_check: u64,
    consent: BTreeMap<String, String>,
}

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Result<Self, String> {
        Self::within(&std::env::temp_dir())
    }
    fn within(parent: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let root = parent.join(format!(".sakatsu-dictionary-{}", next_id()));
        std::fs::create_dir(&root).map_err(|e| e.to_string())?;
        Ok(Self(root))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Pending {
    preview: ImportPreview,
    file: PathBuf,
    bases: BTreeMap<String, String>,
    _temporary: Temporary,
}
#[derive(Clone)]
struct Candidate {
    revision: String,
    index: String,
    download: String,
    base: String,
    automatic: bool,
}
#[derive(Default)]
struct Inner {
    saved: Option<Saved>,
    pending: BTreeMap<String, Pending>,
    candidates: BTreeMap<String, Candidate>,
    status: BTreeMap<String, (String, String)>,
    operation: Option<DictionaryOperation>,
    cancel: Arc<AtomicBool>,
    automatic_attempts: BTreeMap<String, u64>,
    active_automatic: bool,
}
#[derive(Default, Clone)]
pub struct DictionaryManagerState(Arc<Mutex<Inner>>);

fn next_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        now(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn lock(state: &DictionaryManagerState) -> std::sync::MutexGuard<'_, Inner> {
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}
fn manager_state<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> DictionaryManagerState {
    app.state::<DictionaryManagerState>().inner().clone()
}
fn load(inner: &mut Inner) -> Result<Saved, String> {
    if let Some(saved) = &inner.saved {
        return Ok(saved.clone());
    }
    let path = crate::app_data_dir().join("dictionary-management.json");
    let saved = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("Dictionary preferences could not be read: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Saved::default(),
        Err(e) => return Err(e.to_string()),
    };
    inner.saved = Some(saved);
    Ok(inner.saved.as_ref().expect("loaded").clone())
}
fn save(inner: &mut Inner, saved: Saved) -> Result<(), String> {
    let _store = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Store)
        .map_err(|e| e.to_string())?;
    let temporary = Temporary::within(&crate::app_data_dir())?;
    std::fs::write(
        temporary.0.join("dictionary-management.json"),
        serde_json::to_vec_pretty(&saved).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    sakatsu_core::setup::publication::publish_set(
        &temporary.0,
        &crate::app_data_dir(),
        &["dictionary-management.json"],
    )?;
    inner.saved = Some(saved);
    Ok(())
}
fn emit<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let _ = app.emit("dictionary-manager-changed", ());
}

fn stamp(dictionary: &Dictionary) -> String {
    // root resolves to the leased generation, not its display title or revision.
    dictionary.root().to_string_lossy().into_owned()
}
fn kanji_source() -> Result<Option<KanjiSource>, String> {
    let path = crate::kanji_dir().join("kanjidic-source.json");
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
fn kanji_stamp() -> Result<String, String> {
    let path = crate::kanji_dir().join("kanjidic.tsv");
    if path.exists() {
        let table = managed::sha256(&path)?;
        let receipt = crate::kanji_dir().join("kanjidic-source.json");
        let source = if receipt.is_file() {
            managed::sha256(&receipt)?
        } else {
            String::new()
        };
        Ok(format!("{table}:{source}"))
    } else {
        Ok(String::new())
    }
}

fn source(id: &str, meta: &DictionaryMeta) -> Option<(String, String, bool)> {
    if meta.is_updatable {
        if let (Some(index), Some(download)) = (&meta.index_url, &meta.download_url) {
            let official = (id == "jitendex" && index == "https://jitendex.org/static/yomitan.json")
                || (id == "jmnedict" && index == "https://github.com/yomidevs/jmdict-yomitan/releases/latest/download/JMnedict.json");
            return Some((index.clone(), download.clone(), official));
        }
    }
    // Only imports whose bytes match a bundled artifact may acquire missing publisher metadata.
    let component = sakatsu_core::setup::catalog::find("dictionaries")?;
    let known = component
        .artifacts
        .iter()
        .any(|a| a.sha256() == meta.source_sha256.as_deref());
    if !known {
        return None;
    }
    match id {
        "jitendex" => Some(("https://jitendex.org/static/yomitan.json".into(), "https://github.com/stephenmk/stephenmk.github.io/releases/latest/download/jitendex-yomitan.zip".into(), true)),
        "jmnedict" => Some(("https://github.com/yomidevs/jmdict-yomitan/releases/latest/download/JMnedict.json".into(), "https://github.com/yomidevs/jmdict-yomitan/releases/latest/download/JMnedict.zip".into(), true)),
        _ => None,
    }
}

#[tauri::command(async)]
pub fn dictionary_manager<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<DictionaryManagerSnapshot, String> {
    let state = manager_state(&app);
    let mut inner = lock(&state);
    let saved = load(&mut inner)?;
    let dictionaries = app.state::<DictionaryState>().0.now();
    let mut rows = Vec::new();
    for dictionary in &dictionaries.dictionaries {
        let physical_id = dictionary.installed_as();
        let id = format!("words:{physical_id}");
        let meta = dictionary.meta();
        let route = source(&physical_id, meta);
        let endpoint = route.as_ref().map(|r| r.0.clone());
        let consented = route
            .as_ref()
            .is_some_and(|r| r.2 || saved.consent.get(&id) == Some(&r.0));
        let (phase, detail) = inner.status.get(&id).cloned().unwrap_or_else(|| {
            (
                if endpoint.is_some() {
                    "unchecked"
                } else {
                    "unavailable"
                }
                .into(),
                if endpoint.is_some() {
                    ""
                } else {
                    "This dictionary has no supported update metadata."
                }
                .into(),
            )
        });
        rows.push(ManagedDictionary {
            id: id.clone(),
            kind: ResourceKind::Words,
            title: meta.title.clone(),
            revision: meta.revision.clone(),
            generation: stamp(dictionary),
            source: endpoint,
            home: meta.url.clone(),
            entries: u32::try_from(meta.entry_count).ok(),
            sha256: meta.source_sha256.clone(),
            consented,
            phase,
            detail,
            available_revision: inner.candidates.get(&id).map(|c| c.revision.clone()),
        });
    }
    if crate::kanji_dir().join("kanjidic.tsv").is_file() {
        let receipt = kanji_source()?;
        let (phase, detail) = inner
            .status
            .get(KANJI)
            .cloned()
            .unwrap_or(("unchecked".into(), String::new()));
        rows.push(ManagedDictionary {
            id: KANJI.into(),
            kind: ResourceKind::Kanjidic,
            title: "KANJIDIC2".into(),
            revision: receipt
                .as_ref()
                .map(|r| r.revision.clone())
                .unwrap_or_default(),
            generation: kanji_stamp()?,
            source: Some(KANJI_URL.into()),
            home: Some("https://www.edrdg.org/wiki/index.php/KANJIDIC_Project".into()),
            entries: None,
            sha256: receipt.map(|r| r.sha256),
            consented: true,
            phase,
            detail,
            available_revision: inner.candidates.get(KANJI).map(|c| c.revision.clone()),
        });
    }
    Ok(DictionaryManagerSnapshot {
        preferences: saved.preferences,
        dictionaries: rows,
        operation: inner.operation.clone(),
    })
}

#[tauri::command]
pub fn dictionary_preferences<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    preferences: DictionaryPreferences,
) -> Result<(), String> {
    let state = manager_state(&app);
    let mut inner = lock(&state);
    let mut saved = load(&mut inner)?;
    saved.preferences = preferences;
    if saved.preferences.automatic {
        saved.preferences.check_updates = true;
    }
    let automatic = saved.preferences.automatic;
    save(&mut inner, saved)?;
    if !automatic && inner.active_automatic {
        inner.cancel.store(true, Ordering::Release);
    }
    drop(inner);
    emit(&app);
    Ok(())
}

#[tauri::command]
pub fn dictionary_source_consent<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
    endpoint: String,
    allowed: bool,
) -> Result<(), String> {
    let row = dictionary_manager(app.clone())?
        .dictionaries
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("The dictionary is no longer installed.")?;
    if row.source.as_deref() != Some(endpoint.as_str()) {
        return Err("The publisher endpoint changed. Refresh and review it again.".into());
    }
    https(&endpoint)?;
    let state = manager_state(&app);
    let mut inner = lock(&state);
    let mut saved = load(&mut inner)?;
    if allowed {
        saved.consent.insert(id.clone(), endpoint);
    } else {
        saved.consent.remove(&id);
    }
    save(&mut inner, saved)?;
    if !allowed
        && inner
            .operation
            .as_ref()
            .is_some_and(|op| op.target == id || op.target == "all")
    {
        inner.cancel.store(true, Ordering::Release);
    }
    drop(inner);
    emit(&app);
    Ok(())
}

#[tauri::command]
pub fn dictionary_open_source<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
) -> Result<(), String> {
    if id == "resource-licences" {
        let path = app
            .path()
            .resource_dir()
            .map_err(|e| e.to_string())?
            .join("essentials/RESOURCE-NOTICES.txt");
        if !path.is_file() {
            return Err("Resource notices are unavailable in this build.".into());
        }
        #[cfg(windows)]
        {
            std::process::Command::new("explorer")
                .arg(path)
                .spawn()
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        #[cfg(not(windows))]
        return Err("Opening resource notices is only supported on Windows.".into());
    }
    let row = dictionary_manager(app)?
        .dictionaries
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("The dictionary is no longer installed.")?;
    let url = row
        .home
        .or(row.source)
        .ok_or("No publisher page was supplied.")?;
    https(&url)?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("The browser could not be opened: {e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    Err("Opening publisher pages is only supported on Windows.".into())
}

pub fn https(value: &str) -> Result<tauri::Url, String> {
    let url = tauri::Url::parse(value).map_err(|_| "The publisher URL is invalid.")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(
            "Dictionary publishers must use HTTPS URLs without embedded credentials.".into(),
        );
    }
    Ok(url)
}

fn request(url: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
    let mut current = https(url)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(90)))
        .build()
        .into();
    for _ in 0..8 {
        #[cfg(all(debug_assertions, feature = "dictionary-publisher-tests"))]
        let response = if let Some(response) = publisher_tests::response(current.as_str()) {
            response?
        } else {
            agent
                .get(current.as_str())
                .call()
                .map_err(|e| e.to_string())?
        };
        #[cfg(not(all(debug_assertions, feature = "dictionary-publisher-tests")))]
        let response = agent
            .get(current.as_str())
            .call()
            .map_err(|e| e.to_string())?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get("location")
                .ok_or("The publisher returned a redirect without a destination.")?
                .to_str()
                .map_err(|e| e.to_string())?;
            let next = current.join(location).map_err(|e| e.to_string())?;
            current = https(next.as_str())?;
        } else if response.status().is_success() {
            return Ok(response);
        } else {
            return Err(format!(
                "The publisher returned HTTP {}.",
                response.status()
            ));
        }
    }
    Err("The publisher redirected too many times.".into())
}

fn begin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    target: &str,
    phase: &str,
) -> Result<(String, Arc<AtomicBool>), String> {
    let state = manager_state(app);
    let mut inner = lock(&state);
    if inner
        .operation
        .as_ref()
        .is_some_and(|op| !matches!(op.phase.as_str(), "complete" | "failed" | "cancelled"))
    {
        return Err("Another dictionary operation is running.".into());
    }
    let id = next_id();
    inner.cancel = Arc::new(AtomicBool::new(false));
    inner.active_automatic = false;
    inner.operation = Some(DictionaryOperation {
        id: id.clone(),
        target: target.into(),
        phase: phase.into(),
        detail: String::new(),
        bytes: 0,
    });
    let cancel = Arc::clone(&inner.cancel);
    drop(inner);
    emit(app);
    Ok((id, cancel))
}
fn report<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    id: &str,
    phase: &str,
    detail: &str,
    bytes: u32,
) {
    let state = manager_state(app);
    let mut inner = lock(&state);
    if let Some(op) = inner.operation.as_mut().filter(|op| op.id == id) {
        op.phase = phase.into();
        op.detail = detail.into();
        op.bytes = bytes;
    }
    drop(inner);
    emit(app);
}
fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Cancelled. The installed resource was kept.".into())
    } else {
        Ok(())
    }
}

#[tauri::command]
pub fn dictionary_cancel<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    operation_id: String,
) -> Result<(), String> {
    let state = manager_state(&app);
    let mut inner = lock(&state);
    if inner.pending.remove(&operation_id).is_some() {
        return Ok(());
    }
    if let Some(op) = &inner.operation {
        if op.id != operation_id {
            return Err("This operation is no longer current.".into());
        }
        inner.cancel.store(true, Ordering::Release);
    }
    // Dismissing a preview is also cancellation; its private copy is dropped here.
    inner.pending.remove(&operation_id);
    Ok(())
}

#[tauri::command]
pub async fn dictionary_preview<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    file: String,
) -> Result<ImportPreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let current = app.state::<DictionaryState>().0.now();
        let mut bases = current
            .dictionaries
            .iter()
            .map(|d| (format!("words:{}", d.installed_as()), stamp(d)))
            .collect::<BTreeMap<_, _>>();
        bases.insert(KANJI.into(), kanji_stamp()?);
        let temporary = Temporary::new()?;
        let name = Path::new(&file)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Choose a dictionary archive.")?;
        let lower = name.to_ascii_lowercase();
        let kind = if lower.ends_with(".xml.gz") {
            ResourceKind::Kanjidic
        } else if lower.ends_with(".zip") {
            ResourceKind::Words
        } else {
            return Err("Choose a Yomitan word ZIP or KANJIDIC2 .xml.gz file.".into());
        };
        let target = temporary.0.join(if kind == ResourceKind::Words {
            "dictionary.zip"
        } else {
            "kanjidic2.xml.gz"
        });
        let mut source = std::fs::File::open(&file).map_err(|e| e.to_string())?;
        if !source.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Choose a regular dictionary archive file.".into());
        }
        let mut output = std::fs::File::create(&target).map_err(|e| e.to_string())?;
        let copied = std::io::copy(
            &mut Read::by_ref(&mut source).take(managed::MAX_ARCHIVE + 1),
            &mut output,
        )
        .map_err(|e| e.to_string())?;
        if copied > managed::MAX_ARCHIVE {
            return Err("The archive exceeds the import limit.".into());
        }
        drop(output);
        let (title, revision, source) = if kind == ResourceKind::Words {
            let meta = managed::inspect_words(&target)?;
            (meta.title, meta.revision, meta.index_url)
        } else {
            (
                "KANJIDIC2".into(),
                managed::inspect_kanji(&target)?,
                Some(KANJI_URL.into()),
            )
        };
        let preview = ImportPreview {
            id: next_id(),
            kind,
            title,
            revision,
            source,
            sha256: managed::sha256(&target)?,
        };
        let state = manager_state(&app);
        let mut inner = lock(&state);
        // Only one chooser preview is needed; superseded private files are reclaimed.
        inner.pending.clear();
        inner.pending.insert(
            preview.id.clone(),
            Pending {
                preview: preview.clone(),
                file: target,
                bases,
                _temporary: temporary,
            },
        );
        Ok(preview)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn adopt<R: tauri::Runtime>(app: &tauri::AppHandle<R>, kind: &ResourceKind) {
    let component = if *kind == ResourceKind::Words {
        "dictionaries"
    } else {
        "kanji"
    };
    if let Some(component) = sakatsu_core::setup::catalog::find(component) {
        commands::adopt(app, component);
    }
}

fn install<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    preview: &ImportPreview,
    file: &Path,
    target: Option<&str>,
    base: Option<&str>,
    cancel: &AtomicBool,
    automatic: bool,
) -> Result<(), String> {
    let _lifecycle = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Publication)
        .map_err(|e| e.to_string())?;
    let claim = if preview.kind == ResourceKind::Words {
        sakatsu_core::setup::claim::Claim::Dictionaries
    } else {
        sakatsu_core::setup::claim::Claim::Component("kanji")
    };
    crate::owning([claim], |_owned| {
        cancelled(cancel)?;
        if managed::sha256(file)? != preview.sha256 {
            return Err("The previewed archive has changed. Choose it again.".into());
        }
        if preview.kind == ResourceKind::Words {
            managed::inspect_words(file)?;
            let id = target
                .map(str::to_string)
                .unwrap_or_else(|| format!("import-{}", next_id()));
            if id == KANJI {
                return Err("A word dictionary cannot replace KANJIDIC2.".into());
            }
            let dir = if let Some(target) = target {
                let current = app.state::<DictionaryState>().0.now();
                let dictionary = current
                    .dictionaries
                    .iter()
                    .find(|d| format!("words:{}", d.installed_as()) == target)
                    .ok_or("The target dictionary is no longer installed.")?;
                if base.is_some_and(|expected| stamp(dictionary) != expected) {
                    return Err("The installed dictionary changed. Check again.".into());
                }
                crate::dictionaries_dir().join(dictionary.installed_as())
            } else {
                crate::dictionaries_dir().join(&id)
            };
            let mut prepared = Dictionary::prepare(file, &dir).map_err(|e| e.to_string())?;
            if prepared.entry_count() == 0 {
                return Err("The archive contains no usable word entries.".into());
            }
            cancelled(cancel)?;
            let _idle = if automatic {
                Some(
                    crate::lifecycle::gates()
                        .exclusive(
                            &[
                                crate::lifecycle::Gate::Analysis,
                                crate::lifecycle::Gate::Recognition,
                                crate::lifecycle::Gate::Translation,
                            ],
                            std::time::Duration::ZERO,
                        )
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };
            prepared.activate().map_err(|e| e.to_string())?;
            // The new readers must exist before obsolete generations may be reclaimed.
            adopt(app, &preview.kind);
            if let Err(error) = prepared.settle() {
                sakatsu_core::log_warn!(
                    "dictionaries",
                    "replacement is active; deferred cleanup: {error}"
                );
            }
        } else {
            if target.is_some_and(|id| id != KANJI) {
                return Err("KANJIDIC2 cannot replace a word dictionary.".into());
            }
            if base.is_some_and(|expected| kanji_stamp().as_deref() != Ok(expected)) {
                return Err("KANJIDIC2 changed. Check again.".into());
            }
            let revision = managed::inspect_kanji(file)?;
            let destination = crate::kanji_dir();
            let temporary = Temporary::within(&destination)?;
            let count = sakatsu_core::setup::kanji_table(file, &temporary.0.join("kanjidic.tsv"))
                .map_err(|e| e.to_string())?;
            if count == 0 {
                return Err("KANJIDIC2 contains no usable character records.".into());
            }
            let receipt = KanjiSource {
                revision,
                sha256: preview.sha256.clone(),
                source: preview
                    .source
                    .clone()
                    .unwrap_or_else(|| "User import".into()),
            };
            std::fs::write(
                temporary.0.join("kanjidic-source.json"),
                serde_json::to_vec(&receipt).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            cancelled(cancel)?;
            let _idle = if automatic {
                Some(
                    crate::lifecycle::gates()
                        .exclusive(
                            &[
                                crate::lifecycle::Gate::Analysis,
                                crate::lifecycle::Gate::Recognition,
                                crate::lifecycle::Gate::Translation,
                            ],
                            std::time::Duration::ZERO,
                        )
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };
            sakatsu_core::setup::publication::publish_set(
                &temporary.0,
                &destination,
                &["kanjidic.tsv", "kanjidic-source.json"],
            )?;
            adopt(app, &preview.kind);
        }
        Ok(())
    })?
}

#[tauri::command]
pub fn dictionary_import<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    preview_id: String,
    target: Option<String>,
) -> Result<String, String> {
    let (operation, cancel) = begin(&app, target.as_deref().unwrap_or("new"), "importing")?;
    let state = manager_state(&app);
    let pending = lock(&state).pending.remove(&preview_id);
    let Some(pending) = pending else {
        report(
            &app,
            &operation,
            "failed",
            "This preview expired. Choose the archive again.",
            0,
        );
        return Err("This preview expired. Choose the archive again.".into());
    };
    let id = operation.clone();
    std::thread::spawn(move || {
        // Capture the owner as a whole. Disjoint field capture otherwise drops the temporary
        // directory at the command boundary, before this worker can open its archive.
        let pending = pending;
        let base = target.as_ref().and_then(|id| pending.bases.get(id));
        let result = if target.is_some() && base.is_none() {
            Err(
                "The target was not present when this preview was made. Choose the file again."
                    .into(),
            )
        } else {
            install(
                &app,
                &pending.preview,
                &pending.file,
                target.as_deref(),
                base.map(String::as_str),
                &cancel,
                false,
            )
        };
        if result.is_ok() {
            if let Some(target) = &target {
                let manager = manager_state(&app);
                let mut inner = lock(&manager);
                inner.candidates.remove(target);
                inner.status.remove(target);
            }
        }
        finish(&app, &id, result, &cancel);
    });
    Ok(operation)
}

fn finish<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    operation: &str,
    result: Result<(), String>,
    cancel: &AtomicBool,
) {
    match result {
        Ok(()) => {
            let is_check = lock(&manager_state(app))
                .operation
                .as_ref()
                .is_some_and(|op| op.id == operation && op.target == "all");
            report(
                app,
                operation,
                "complete",
                if is_check {
                    "Publisher checks finished."
                } else {
                    "Dictionary ready."
                },
                0,
            );
        }
        Err(error) => report(
            app,
            operation,
            if cancel.load(Ordering::Acquire) {
                "cancelled"
            } else {
                "failed"
            },
            &error,
            0,
        ),
    }
}

fn perform_check<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    operation: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let rows = dictionary_manager(app.clone())?.dictionaries;
    let mut failures = Vec::new();
    for row in rows
        .into_iter()
        .filter(|r| r.consented && r.source.is_some())
    {
        cancelled(cancel)?;
        {
            let state = manager_state(app);
            let mut inner = lock(&state);
            inner.candidates.remove(&row.id);
            inner.automatic_attempts.remove(&row.id);
            inner
                .status
                .insert(row.id.clone(), ("checking".into(), String::new()));
        }
        emit(app);
        let answer = (|| -> Result<Option<Candidate>, String> {
            let endpoint = row.source.clone().ok_or("No publisher endpoint.")?;
            let original = row.generation.clone();
            let mut response = request(&endpoint)?;
            let (revision, download, base) = if row.kind == ResourceKind::Kanjidic {
                let revision = managed::kanji_gzip_header(response.body_mut().as_reader())?;
                (revision, endpoint.clone(), kanji_stamp()?)
            } else {
                let mut bytes = Vec::new();
                response
                    .body_mut()
                    .as_reader()
                    .take(managed::MAX_METADATA + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() as u64 > managed::MAX_METADATA {
                    return Err("Publisher metadata exceeds its limit.".into());
                }
                let meta: DictionaryMeta =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if !(1..=3).contains(&meta.format) || meta.revision.is_empty() {
                    return Err("Unsupported publisher metadata.".into());
                }
                let download = meta
                    .download_url
                    .ok_or("The publisher supplied no download URL.")?;
                https(&download)?;
                let dictionaries = app.state::<DictionaryState>().0.now();
                let current = dictionaries
                    .dictionaries
                    .iter()
                    .find(|d| format!("words:{}", d.installed_as()) == row.id)
                    .ok_or("The dictionary was removed during the check.")?;
                (meta.revision, download, stamp(current))
            };
            if original != base {
                return Err("The dictionary changed during its update check. Check again.".into());
            }
            let previous = if row.kind == ResourceKind::Kanjidic {
                kanji_source()?.map(|r| r.revision).unwrap_or_default()
            } else {
                row.revision.clone()
            };
            let Some(automatic) = managed::revision_change(&previous, &revision)? else {
                return Ok(None);
            };
            Ok(Some(Candidate {
                revision,
                download,
                index: endpoint,
                base,
                automatic,
            }))
        })();
        let state = manager_state(app);
        let mut inner = lock(&state);
        if let Err(error) = cancelled(cancel) {
            inner
                .status
                .insert(row.id, ("cancelled".into(), error.clone()));
            drop(inner);
            emit(app);
            return Err(error);
        }
        match answer {
            Ok(Some(candidate)) => {
                inner.status.insert(
                    row.id.clone(),
                    (
                        "available".into(),
                        "A publisher revision is available.".into(),
                    ),
                );
                inner.candidates.insert(row.id, candidate);
            }
            Ok(None) => {
                inner
                    .status
                    .insert(row.id, ("current".into(), String::new()));
            }
            Err(error) => {
                failures.push(error.clone());
                inner.status.insert(row.id, ("failed".into(), error));
            }
        }
        drop(inner);
        emit(app);
    }
    report(app, operation, "checking", "Publisher checks finished.", 0);
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} publisher check(s) failed: {}",
            failures.len(),
            failures.join("; ")
        ))
    }
}

#[tauri::command]
pub fn dictionary_check<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<String, String> {
    let (id, cancel) = begin(&app, "all", "checking")?;
    let state = manager_state(&app);
    let persisted = (|| {
        let mut inner = lock(&state);
        let mut saved = load(&mut inner)?;
        saved.last_check = now();
        save(&mut inner, saved)
    })();
    if let Err(error) = persisted {
        report(&app, &id, "failed", &error, 0);
        return Err(error);
    }
    let operation = id.clone();
    std::thread::spawn(move || {
        let result = perform_check(&app, &operation, &cancel);
        finish(&app, &operation, result, &cancel);
    });
    Ok(id)
}

fn download<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    operation: &str,
    url: &str,
    file: &Path,
    limit: u64,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut response = request(url)?;
    let mut reader = response.body_mut().as_reader();
    let mut output = std::fs::File::create(file).map_err(|e| e.to_string())?;
    let mut bytes = 0u64;
    let mut buffer = [0; 65536];
    let mut last = std::time::Instant::now();
    loop {
        cancelled(cancel)?;
        let count = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > limit {
            return Err("The publisher download exceeds the import limit.".into());
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        if last.elapsed() >= std::time::Duration::from_millis(200) {
            report(
                app,
                operation,
                "downloading",
                "Downloading the publisher's dictionary.",
                bytes as u32,
            );
            last = std::time::Instant::now();
        }
    }
    output.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn dictionary_update<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
) -> Result<String, String> {
    update(app, id, false)
}

fn update<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
    automatic: bool,
) -> Result<String, String> {
    let row = dictionary_manager(app.clone())?
        .dictionaries
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("The dictionary is no longer installed.")?;
    if !row.consented {
        return Err("Allow this publisher before checking or downloading updates.".into());
    }
    let state = manager_state(&app);
    let candidate = lock(&state)
        .candidates
        .get(&id)
        .cloned()
        .ok_or("Check for updates first.")?;
    if row.source.as_deref() != Some(candidate.index.as_str()) {
        return Err("The publisher endpoint changed. Check again.".into());
    }
    let (operation, cancel) = begin(&app, &id, "downloading")?;
    {
        let manager = manager_state(&app);
        let mut inner = lock(&manager);
        inner.active_automatic = automatic;
        if automatic && !load(&mut inner)?.preferences.automatic {
            drop(inner);
            report(
                &app,
                &operation,
                "cancelled",
                "Automatic updates were switched off.",
                0,
            );
            return Err("Automatic updates were switched off.".into());
        }
    }
    let op = operation.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let _publication = crate::lifecycle::gates()
                .enter(crate::lifecycle::Gate::Publication)
                .map_err(|e| e.to_string())?;
            let temporary = Temporary::new()?;
            let file = temporary.0.join("download");
            let limit = if row.kind == ResourceKind::Words {
                managed::MAX_ARCHIVE
            } else {
                16 * 1024 * 1024
            };
            download(&app, &op, &candidate.download, &file, limit, &cancel)?;
            let revision = if row.kind == ResourceKind::Words {
                let metadata = managed::inspect_words(&file)?;
                if metadata.index_url.as_deref() != Some(candidate.index.as_str()) {
                    return Err("The downloaded dictionary names a different update source. Import it manually to review that change.".into());
                }
                metadata.revision
            } else {
                managed::inspect_kanji(&file)?
            };
            if revision != candidate.revision {
                return Err(
                    "The publisher changed its download after the check. Check again.".into(),
                );
            }
            let preview = ImportPreview {
                id: op.clone(),
                kind: row.kind,
                title: row.title,
                revision,
                source: Some(candidate.index),
                sha256: managed::sha256(&file)?,
            };
            report(&app, &op, "importing", "Preparing the replacement.", 0);
            install(
                &app,
                &preview,
                &file,
                Some(&id),
                Some(&candidate.base),
                &cancel,
                automatic,
            )?;
            let state = manager_state(&app);
            let mut inner = lock(&state);
            inner.candidates.remove(&id);
            inner.status.insert(id, ("current".into(), String::new()));
            Ok(())
        })();
        finish(&app, &op, result, &cancel);
    });
    Ok(operation)
}

fn check_due(saved: &Saved, at: u64) -> bool {
    saved.preferences.check_updates && at.saturating_sub(saved.last_check) >= 86400
}

/// Only the accepted instance starts this loop. No startup request is made before resources
/// are usable, and the persisted timestamp is updated before any network request is spawned.
pub fn start<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let ready = app
            .try_state::<crate::setup_manager::SetupState>()
            .zip(app.try_state::<commands::AnalyzerState>())
            .is_some_and(|(setup, analyzer)| {
                crate::setup_manager::build_snapshot(&setup, &analyzer).required_ready
            });
        if !ready {
            continue;
        }
        let state = manager_state(&app);
        let settings = {
            let mut inner = lock(&state);
            load(&mut inner)
        };
        let Ok(settings) = settings else {
            continue;
        };
        if check_due(&settings, now()) {
            let _ = dictionary_check(app.clone());
        }
        if settings.preferences.automatic {
            // Opaque custom revision schemes may be offered manually but cannot justify an
            // unattended replacement whose direction we cannot establish.
            let next = {
                let inner = lock(&state);
                inner
                    .candidates
                    .iter()
                    .find(|(id, candidate)| {
                        candidate.automatic
                            && now()
                                .saturating_sub(*inner.automatic_attempts.get(*id).unwrap_or(&0))
                                >= 1800
                    })
                    .map(|(id, _)| id.clone())
            };
            if let Some(id) = next {
                if update(app.clone(), id.clone(), true).is_ok() {
                    lock(&state).automatic_attempts.insert(id, now());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn daily_check_is_due_once_and_respects_disabled_and_backward_clocks() {
        let mut saved = Saved {
            last_check: 100_000,
            ..Default::default()
        };
        assert!(!check_due(&saved, 100_001));
        assert!(!check_due(&saved, 186_399));
        assert!(check_due(&saved, 186_400));
        assert!(!check_due(&saved, 99_999));
        saved.preferences.check_updates = false;
        assert!(!check_due(&saved, 300_000));
    }

    #[test]
    fn dictionary_urls_require_https_without_credentials() {
        for url in [
            "http://example.com/data",
            "file:///C:/data",
            "https://user:pass@example.com/data",
            "garbage",
        ] {
            assert!(https(url).is_err(), "{url}");
        }
        assert!(https("https://example.com/data").is_ok());
    }
    #[test]
    fn source_names_do_not_establish_identity_or_consent() {
        let meta = DictionaryMeta {
            title: "Jitendex.org [2099-01-01]".into(),
            ..Default::default()
        };
        assert!(source("jitendex", &meta).is_none());
        let meta = DictionaryMeta {
            is_updatable: true,
            index_url: Some("https://example.com/index".into()),
            download_url: Some("https://example.com/archive".into()),
            ..meta
        };
        assert!(!source("jitendex", &meta).unwrap().2);
    }
}

#[cfg(all(debug_assertions, feature = "dictionary-publisher-tests"))]
#[path = "dictionary_manager_tests.rs"]
pub mod publisher_tests;
