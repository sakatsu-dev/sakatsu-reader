//! One updater operation per app. Only signed, backend-held bytes can reach install.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sakatsu_core::update::{should_check, UpdatePhase, UpdateStatus};
use tauri::{Emitter, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

struct Inner {
    status: UpdateStatus,
    busy: bool,
    update: Option<Update>,
    bytes: Option<Arc<Vec<u8>>>,
}

pub struct UpdateState(Mutex<Inner>);

/// How often download progress reaches the interface. The plugin reports every network chunk,
/// thousands for one installer, and each event repaints the Updates screen.
#[doc(hidden)]
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// Lets one progress event through per interval. The first is always due.
#[doc(hidden)]
pub struct Throttle {
    interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
        }
    }

    fn due(&mut self, now: Instant) -> bool {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < self.interval)
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

impl Default for UpdateState {
    fn default() -> Self {
        Self::new(false)
    }
}

impl UpdateState {
    pub fn new(configured: bool) -> Self {
        Self(Mutex::new(Inner {
            status: UpdateStatus::new(configured),
            busy: false,
            update: None,
            bytes: None,
        }))
    }

    fn change<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        change: impl FnOnce(&mut Inner),
    ) -> UpdateStatus {
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        change(&mut inner);
        inner.status.generation = inner.status.generation.saturating_add(1);
        let status = inner.status.clone();
        // Emitting while holding this lock keeps the event order equal to mutation order.
        let _ = app.emit("update-status", &status);
        status
    }

    /// Count a downloaded chunk, and tell the interface at most once per `throttle` interval. The
    /// count and generation always move, so `update_status` and the final event are exact.
    #[doc(hidden)]
    pub fn chunk<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        throttle: &mut Throttle,
        now: Instant,
        chunk: usize,
        total: Option<u64>,
    ) {
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        inner.status.downloaded += chunk as f64;
        inner.status.total = total.map(|n| n as f64);
        inner.status.generation = inner.status.generation.saturating_add(1);
        if throttle.due(now) {
            let _ = app.emit("update-status", &inner.status);
        }
    }

    fn begin<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        phase: UpdatePhase,
    ) -> Result<(), String> {
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if inner.status.phase == UpdatePhase::NotConfigured {
            return Err("Updates are not configured in this build.".into());
        }
        if inner.busy {
            return Err("An update operation is already running.".into());
        }
        inner.busy = true;
        inner.status.generation = inner.status.generation.saturating_add(1);
        inner.status.phase = phase;
        inner.status.error = None;
        let _ = app.emit("update-status", &inner.status);
        Ok(())
    }
}

#[tauri::command]
pub fn update_status(state: tauri::State<'_, UpdateState>) -> UpdateStatus {
    state
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .status
        .clone()
}

pub fn configured<R: Runtime>(app: &tauri::AppHandle<R>) -> bool {
    app.config().plugins.0.contains_key("updater")
}

pub fn initialize<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), Box<dyn std::error::Error>> {
    let configured = configured(app);
    if configured {
        let config = &app.config().plugins.0["updater"];
        if config.get("requireSignedVersion").and_then(|v| v.as_bool()) != Some(true)
            || [
                "dangerousInsecureTransportProtocol",
                "dangerousAcceptInvalidCerts",
                "dangerousAcceptInvalidHostnames",
            ]
            .iter()
            .any(|key| config.get(*key).and_then(|v| v.as_bool()) == Some(true))
        {
            return Err("The updater configuration does not meet the release transport and signature policy.".into());
        }
        // No guest updater permissions: all operations go through our guarded commands.
        app.plugin(tauri_plugin_updater::Builder::new().build())?;
    }
    app.manage(UpdateState::new(configured));
    if configured
        && app
            .state::<crate::commands::StoreState>()
            .settings()
            .check_updates_on_startup
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = check(app, false).await;
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn update_check<R: Runtime>(
    app: tauri::AppHandle<R>,
    manual: bool,
) -> Result<UpdateStatus, String> {
    check(app, manual).await
}

async fn check<R: Runtime>(app: tauri::AppHandle<R>, manual: bool) -> Result<UpdateStatus, String> {
    let state = app.state::<UpdateState>();
    if !configured(&app) {
        return Ok(update_status(state));
    }
    let directory = app
        .state::<crate::commands::StoreState>()
        .store
        .as_ref()
        .map(|store| store.root().to_path_buf())
        .ok_or("The settings store is unavailable, so the update check could not be recorded.")?;
    let path = directory.join("update-check.json");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let last = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<u64>(&b).ok());
    let enabled = app
        .state::<crate::commands::StoreState>()
        .settings()
        .check_updates_on_startup;
    if !manual && !should_check(now, last, enabled) {
        return Ok(update_status(state));
    }
    state.begin(&app, UpdatePhase::Checking)?;
    // A failed attempt is also recorded, so an offline launch does not retry continuously.
    let saved = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&directory)?;
        let temporary = path.with_extension("partial");
        std::fs::write(&temporary, now.to_string())?;
        std::fs::rename(temporary, &path)
    })();
    if saved.is_err() {
        return Ok(state.change(&app, |i| {
            i.busy = false;
            i.status.phase = UpdatePhase::Failed;
            i.status.error = Some("The update-check time could not be saved.".into());
        }));
    }
    let secure_transport = !app.config().plugins.0["updater"]
        .get("dangerousInsecureTransportProtocol")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let result = match app
        .updater_builder()
        .configure_client(move |client| client.https_only(secure_transport))
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };
    Ok(state.change(&app, |i| {
        i.busy = false;
        match result {
            Ok(Some(update))
                if update.version.contains('-')
                    || update.download_url.scheme() != "https"
                    || !update.download_url.username().is_empty()
                    || update.download_url.password().is_some() =>
            {
                i.status.phase = UpdatePhase::Failed;
                i.status.error = Some(
                    "The release did not provide a stable version with a secure download address."
                        .into(),
                );
            }
            Ok(Some(update)) => {
                i.status.phase = UpdatePhase::Available;
                i.status.version = Some(update.version.clone());
                i.status.notes = update.body.clone().unwrap_or_default();
                i.status.downloaded = 0.0;
                i.status.total = None;
                i.update = Some(update);
                i.bytes = None;
            }
            Ok(None) => {
                i.status.phase = UpdatePhase::Current;
                i.status.version = None;
                i.status.notes.clear();
                i.update = None;
                i.bytes = None;
            }
            Err(error) => {
                i.status.phase = match &error {
                    tauri_plugin_updater::Error::Reqwest(e) if e.is_connect() || e.is_timeout() => {
                        UpdatePhase::Offline
                    }
                    _ => UpdatePhase::Failed,
                };
                i.status.error =
                    Some("The release service could not be checked. Try again later.".into());
            }
        }
    }))
}

#[tauri::command]
pub async fn update_download<R: Runtime>(app: tauri::AppHandle<R>) -> Result<UpdateStatus, String> {
    let state = app.state::<UpdateState>();
    // Inspect and reserve under one lock. No check can replace the selected Update mid-download.
    let update = {
        let mut i = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if i.busy {
            return Err("An update operation is already running.".into());
        }
        let update = i
            .update
            .clone()
            .ok_or("Check for an available update first.")?;
        i.busy = true;
        i.status.generation = i.status.generation.saturating_add(1);
        i.status.phase = UpdatePhase::Downloading;
        i.status.downloaded = 0.0;
        i.status.error = None;
        let _ = app.emit("update-status", &i.status);
        update
    };
    let mut throttle = Throttle::new(PROGRESS_INTERVAL);
    let result = update
        .download(
            |chunk, total| state.chunk(&app, &mut throttle, Instant::now(), chunk, total),
            || {},
        )
        .await;
    Ok(state.change(&app, |i| {
        i.busy = false;
        match result {
            Ok(bytes) => {
                i.bytes = Some(Arc::new(bytes));
                i.status.phase = UpdatePhase::Ready;
            }
            Err(_) => {
                i.bytes = None;
                i.status.phase = UpdatePhase::Failed;
                i.status.error = Some(
                    "The update could not be downloaded or its signature was refused. Try again."
                        .into(),
                );
            }
        }
    }))
}

#[tauri::command(async)]
pub fn update_install<R: Runtime>(app: tauri::AppHandle<R>) -> Result<UpdateStatus, String> {
    let state = app.state::<UpdateState>();
    let (update, bytes) = {
        let mut i = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if i.busy {
            return Err("An update operation is already running.".into());
        }
        let update = i.update.clone().ok_or("Download an update first.")?;
        let bytes = i.bytes.clone().ok_or("Download an update first.")?;
        i.busy = true;
        (update, bytes)
    };
    let outcome = guarded_handoff(
        || {
            update
                .verify(&bytes)
                .map_err(|_| "The update signature was refused.".to_string())
        },
        || {
            update.install(bytes.as_slice()).map_err(|_| {
                "The update installer could not be started. You can retry.".to_string()
            })
        },
    );
    Ok(state.change(&app, |i| {
        i.busy = false;
        i.status.phase = UpdatePhase::Ready;
        i.status.error = outcome.err();
    }))
}

/// One exclusion boundary for verified installer handoff. No callback runs if work cannot drain.
/// A failed verification leaves reading runtimes alone; every returned failure reopens the gates.
/// The background availability probe is disposable and is cancelled before the drain.
pub fn guarded_handoff(
    verify: impl FnOnce() -> Result<(), String>,
    install: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    // Prevent a new background probe from racing the drain, and cancel only its private child.
    // Foreground analysis and setup keep their entries until they finish.
    let _probes = crate::analyzer_probe::suspend().map_err(|error| error.to_string())?;
    // Setup and resource downloads hold the publication gate for their whole transfer, so this is
    // also what refuses an install in the middle of one.
    let _exclusive = crate::lifecycle::gates()
        .exclusive(&crate::lifecycle::ALL, Duration::from_secs(3))
        .map_err(|_| {
            "The update was not installed because reading, setup or another task is still \
             running. Try again when it has finished."
                .to_string()
        })?;
    verify()?;
    crate::vlm_setup::stop_managed_runtime();
    sakatsu_core::analyzer::ichiran::server::shutdown();
    install()
}
