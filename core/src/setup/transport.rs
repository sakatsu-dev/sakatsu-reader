//! Serialized setup responses. The shell resolves platform capabilities and lends these
//! values to the UI; the generated contract describes the bytes serde actually emits.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SetupPhase {
    Checking,
    ReadyToInstall,
    Downloading,
    PausedRetrying,
    Installing,
    Verifying,
    Complete,
    NeedsAttention,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SetupArtifactState {
    pub file: String,
    pub version: String,
    pub immutable_source: String,
    pub required: bool,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub expected_download_bytes: u64,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub expected_installed_bytes: u64,
    pub sha256: String,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub downloaded_bytes: u64,
    pub partial_file: Option<String>,
    pub transfer_job: Option<String>,
    pub verification: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SetupComponentState {
    pub id: String,
    pub label: String,
    pub required: bool,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub transfer_state: String,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub downloaded_bytes: u64,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub total_bytes: u64,
    pub verification: String,
    pub repair_eligible: bool,
    pub artifacts: Vec<SetupArtifactState>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct StorageSummary {
    pub target: String,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub download_bytes: u64,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub installed_bytes: u64,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub staging_bytes: u64,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub peak_bytes: u64,
    #[cfg_attr(feature = "typescript", ts(as = "Option<u32>"))]
    pub available_bytes: Option<u64>,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub reserve_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SetupSnapshot {
    pub phase: SetupPhase,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub generation: u64,
    pub required_ready: bool,
    /// New first-run journeys keep the analyzer decision across WSL/Windows restarts.
    pub analyzer_setup_pending: bool,
    pub resumable: bool,
    pub text_only: bool,
    pub welcome_seen: bool,
    pub existing_install: bool,
    pub current_component: Option<String>,
    pub current_issue: Option<String>,
    pub storage: StorageSummary,
    pub components: Vec<SetupComponentState>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SetupPreflight {
    pub target_location: String,
    pub writable: bool,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub required_bytes: u64,
    #[cfg_attr(feature = "typescript", ts(as = "Option<u32>"))]
    pub available_bytes: Option<u64>,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub peak_bytes: u64,
    pub enough_space: bool,
    pub network_available: bool,
    pub supported_windows: bool,
    pub supported_architecture: bool,
    pub webview2: bool,
    pub blocking_issue: Option<String>,
}

/// One independently linkable source inside a compound setup offer.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct CatalogSource {
    pub name: String,
    pub licence: String,
    pub home: String,
}

/// One offer on the setup screen.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct CatalogEntry {
    pub id: String,
    pub label: String,
    pub about: String,
    pub licence: String,
    /// Where the data comes from. A link, because for some of it a link is a licence condition.
    pub home: String,
    /// Separately linkable sources when one setup action builds several licensed artifacts.
    pub sources: Vec<CatalogSource>,
    /// Rendered, because the number is only there to be read: "50 MB".
    pub size: String,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub bytes: u64,
    pub tier: crate::setup::Tier,
    /// Whether the app has to take this up after fetching it. See `Component::needs_reload`.
    ///
    /// The interface no longer *asks* for anything on the strength of this — `commands::adopt`
    /// has already done the reload by the time `fetch-done` arrives. It is still reported
    /// because the screens use it to say what is happening while it happens, and because
    /// `tests/ipc.rs` pins the shape of this struct.
    pub needs_reload: bool,
    /// False for Ichiran, which is installed by a guided terminal rather than the
    /// ordinary artifact downloader.
    pub fetchable: bool,
    /// The absolute path of a bundled companion script, resolved under the installed
    /// resource directory. Guided system setup launches this through its own command;
    /// ordinary artifact downloads use it only as provenance and a developer fallback.
    pub script: Option<String>,
    pub ready: bool,
    /// A legacy install remains usable, but lacks the supplemental artifacts now offered.
    pub update_available: bool,
}

/// How far one artifact has got, as the `fetch-progress` event carries it.
///
/// # Why this is a type now
///
/// It was three separate `serde_json::json!` literals in the shell and a handwritten interface in
/// the frontend, and the two had already drifted: the interface declared six fields where the wire
/// carried nine, and made `step` optional because one of the three literals omitted it. Nothing
/// could have caught either — a generated declaration is checked against the Rust type, and an
/// anonymous payload has no Rust type to check.
///
/// **The three shapes become one.** Every field the frontend reads keeps its name and meaning; what
/// changes is that the values it was defaulting — `step ?? 1` — now arrive explicitly with those
/// same defaults, and `speed`, `eta` and `attempt` are declared instead of being undocumented extras
/// on two emissions out of three.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct FetchProgress {
    /// The component id the setup screens key their rows by.
    pub id: String,
    /// The file being fetched, or what is happening when there is no file — "importing…".
    pub file: String,
    pub step: u32,
    pub steps: u32,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub done: u64,
    /// Null when the server sent no length, so a bar has to cope with not knowing.
    #[cfg_attr(feature = "typescript", ts(as = "Option<u32>"))]
    pub total: Option<u64>,
    #[cfg_attr(feature = "typescript", ts(as = "Option<u32>"))]
    pub speed: Option<u64>,
    #[cfg_attr(feature = "typescript", ts(as = "Option<u32>"))]
    pub eta: Option<u64>,
    pub attempt: u32,
}

impl FetchProgress {
    /// Before the first byte: the expected size is known, nothing has been transferred.
    pub fn starting(id: &str, file: &str, step: usize, steps: usize, total: Option<u64>) -> Self {
        Self {
            id: id.to_string(),
            file: file.to_string(),
            step: step as u32,
            steps: steps as u32,
            done: 0,
            total,
            speed: None,
            eta: None,
            attempt: 1,
        }
    }

    /// During the transfer, from the downloader's own report.
    pub fn during(
        id: &str,
        file: &str,
        step: usize,
        steps: usize,
        progress: &crate::setup::Progress,
    ) -> Self {
        Self {
            id: id.to_string(),
            file: file.to_string(),
            step: step as u32,
            steps: steps as u32,
            done: progress.done,
            total: progress.total,
            speed: progress.speed_bytes_per_second,
            eta: progress.eta_seconds,
            attempt: u32::from(progress.attempt),
        }
    }

    /// Work with no measurable length — importing, indexing — where `file` says what is happening.
    ///
    /// One step of one, which is what the frontend was already substituting for the absent fields.
    pub fn working(id: &str, what: &str) -> Self {
        Self {
            id: id.to_string(),
            file: what.to_string(),
            step: 1,
            steps: 1,
            done: 0,
            total: None,
            speed: None,
            eta: None,
            attempt: 1,
        }
    }
}

/// The same progress, for the onboarding screen, as the `setup-progress` event carries it.
///
/// Separate from [`FetchProgress`] because the wire keys differ — `component` and `artifact` rather
/// than `id` and `file` — and those names are what the setup store reads. Built **from** a
/// `FetchProgress` so the two emissions cannot say different things about one transfer.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SetupProgress {
    /// The snapshot this belongs to. The store drops progress from an older generation.
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub generation: u64,
    pub component: String,
    pub artifact: String,
    pub step: u32,
    pub steps: u32,
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub done: u64,
    #[cfg_attr(feature = "typescript", ts(as = "Option<u32>"))]
    pub total: Option<u64>,
    #[cfg_attr(feature = "typescript", ts(as = "Option<u32>"))]
    pub speed: Option<u64>,
    #[cfg_attr(feature = "typescript", ts(as = "Option<u32>"))]
    pub eta: Option<u64>,
    pub attempt: u32,
}

impl SetupProgress {
    pub fn of(generation: u64, fetch: &FetchProgress) -> Self {
        Self {
            generation,
            component: fetch.id.clone(),
            artifact: fetch.file.clone(),
            step: fetch.step,
            steps: fetch.steps,
            done: fetch.done,
            total: fetch.total,
            speed: fetch.speed,
            eta: fetch.eta,
            attempt: fetch.attempt,
        }
    }
}
