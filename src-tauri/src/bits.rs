//! Windows Background Intelligent Transfer Service backend for setup artifacts.
//!
//! A job id is written beside the target before the job is resumed. BITS owns the partial
//! bytes, so closing Sakatsu does not stop or discard the transfer; the next setup run attaches
//! by id. The portable HTTP downloader remains the fallback when BITS or COM is unavailable.

use sakatsu_core::setup::{accept_download, Progress, Verification};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::{GUID, HSTRING};
use windows::Win32::Networking::BackgroundIntelligentTransferService::{
    BackgroundCopyManager, IBackgroundCopyJob, IBackgroundCopyManager, BG_JOB_PROGRESS,
    BG_JOB_STATE_ACKNOWLEDGED, BG_JOB_STATE_CANCELLED, BG_JOB_STATE_ERROR, BG_JOB_STATE_SUSPENDED,
    BG_JOB_STATE_TRANSFERRED, BG_JOB_TYPE_DOWNLOAD,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED,
};

#[derive(Debug)]
pub enum BitsError {
    /// BITS cannot be used on this machine; the caller should use resumable HTTP.
    Unavailable(String),
    /// The user paused the persisted job. It remains attachable.
    Paused,
    /// BITS accepted the job but it entered a terminal error state.
    Failed(String),
}

impl std::fmt::Display for BitsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "BITS unavailable: {reason}"),
            Self::Paused => write!(f, "paused"),
            Self::Failed(reason) => write!(f, "BITS transfer failed: {reason}"),
        }
    }
}

struct ComGuard;
impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: paired with a successful CoInitializeEx on this worker thread.
        unsafe { CoUninitialize() };
    }
}

pub fn download(
    display_name: &str,
    url: &str,
    into: &Path,
    min_bytes: u64,
    verification: Verification<'_>,
    cancel: &Arc<AtomicBool>,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<u64, BitsError> {
    if let Some(parent) = into.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| BitsError::Unavailable(error.to_string()))?;
    }

    // SAFETY: this is a dedicated setup worker and COM is uninitialised by `ComGuard`.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(|error| BitsError::Unavailable(error.to_string()))?;
    let _com = ComGuard;
    // SAFETY: BackgroundCopyManager is an out-of-process COM server supplied by Windows.
    let manager: IBackgroundCopyManager =
        unsafe { CoCreateInstance(&BackgroundCopyManager, None, CLSCTX_LOCAL_SERVER) }
            .map_err(|error| BitsError::Unavailable(error.to_string()))?;

    let identity = job_path(into);
    let staged = bits_path(into);
    let job = attach_job(&manager, &identity)
        .or_else(|| create_job(&manager, display_name, url, &staged, &identity).ok());
    let Some(job) = job else {
        return Err(BitsError::Unavailable(
            "Windows could not create or reattach the transfer job".into(),
        ));
    };

    // Five minutes without a byte is an error rather than an invisible forever-job. BITS will
    // still retry transient network failures according to its own persisted schedule.
    unsafe { job.SetMinimumRetryDelay(15) }
        .map_err(|error| BitsError::Unavailable(error.to_string()))?;
    unsafe { job.SetNoProgressTimeout(5 * 60) }
        .map_err(|error| BitsError::Unavailable(error.to_string()))?;
    unsafe { job.Resume() }.map_err(|error| BitsError::Unavailable(error.to_string()))?;

    let started = Instant::now();
    let mut previous = 0;
    let mut previous_at = started;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = unsafe { job.Suspend() };
            return Err(BitsError::Paused);
        }

        let state =
            unsafe { job.GetState() }.map_err(|error| BitsError::Unavailable(error.to_string()))?;
        let mut raw = BG_JOB_PROGRESS::default();
        unsafe { job.GetProgress(&mut raw) }
            .map_err(|error| BitsError::Unavailable(error.to_string()))?;
        let now = Instant::now();
        let elapsed = now.duration_since(previous_at).as_secs_f64();
        let speed = (elapsed > 0.2 && raw.BytesTransferred >= previous)
            .then(|| ((raw.BytesTransferred - previous) as f64 / elapsed) as u64);
        let total = (raw.BytesTotal != u64::MAX).then_some(raw.BytesTotal);
        let eta = speed.and_then(|speed| {
            total.map(|total| total.saturating_sub(raw.BytesTransferred) / speed.max(1))
        });
        on_progress(Progress {
            done: raw.BytesTransferred,
            total,
            speed_bytes_per_second: speed,
            eta_seconds: eta,
            attempt: 1,
        });
        previous = raw.BytesTransferred;
        previous_at = now;

        if state == BG_JOB_STATE_TRANSFERRED {
            unsafe { job.Complete() }.map_err(|error| BitsError::Failed(error.to_string()))?;
            let _ = std::fs::remove_file(&identity);
            return accept_download(&staged, into, min_bytes, verification)
                .map_err(|error| BitsError::Failed(error.to_string()));
        }
        if state == BG_JOB_STATE_ERROR
            || state == BG_JOB_STATE_CANCELLED
            || state == BG_JOB_STATE_ACKNOWLEDGED
        {
            let _ = unsafe { job.Cancel() };
            let _ = std::fs::remove_file(&identity);
            return Err(BitsError::Failed(format!("terminal job state {}", state.0)));
        }
        if state == BG_JOB_STATE_SUSPENDED {
            unsafe { job.Resume() }.map_err(|error| BitsError::Failed(error.to_string()))?;
        }
        std::thread::sleep(Duration::from_millis(350));
    }
}

fn attach_job(manager: &IBackgroundCopyManager, path: &Path) -> Option<IBackgroundCopyJob> {
    let text = std::fs::read_to_string(path).ok()?;
    let value = u128::from_str_radix(text.trim(), 16).ok()?;
    // SAFETY: the GUID came from this manager and is persisted as its exact u128 value.
    unsafe { manager.GetJob(&GUID::from_u128(value)) }.ok()
}

fn create_job(
    manager: &IBackgroundCopyManager,
    display_name: &str,
    url: &str,
    staged: &Path,
    identity: &Path,
) -> windows::core::Result<IBackgroundCopyJob> {
    let mut id = GUID::zeroed();
    let mut job = None;
    // SAFETY: all out-pointers are valid for the duration of the call.
    unsafe {
        manager.CreateJob(
            &HSTRING::from(display_name),
            BG_JOB_TYPE_DOWNLOAD,
            &mut id,
            &mut job,
        )?;
    }
    let job = job.expect("BITS returned success without a job");
    let _ = std::fs::remove_file(staged);
    unsafe {
        job.AddFile(
            &HSTRING::from(url),
            &HSTRING::from(staged.as_os_str().to_string_lossy().as_ref()),
        )?;
    }
    if let Err(error) = std::fs::write(identity, format!("{:032x}", id.to_u128())) {
        let _ = unsafe { job.Cancel() };
        return Err(error.into());
    }
    Ok(job)
}

fn job_path(target: &Path) -> PathBuf {
    target.with_file_name(format!(
        "{}.bits-job",
        target.file_name().unwrap_or_default().to_string_lossy()
    ))
}

fn bits_path(target: &Path) -> PathBuf {
    target.with_file_name(format!(
        "{}.bits-download",
        target.file_name().unwrap_or_default().to_string_lossy()
    ))
}
