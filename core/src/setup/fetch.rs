//! Resumable, verified setup downloads.
//!
//! The important boundary is not “the request returned”; it is “verified bytes were
//! atomically published”. Partial files and their validators deliberately survive network
//! failures, cancellation and process exit so a large OCR model can continue next time.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum FetchError {
    Unreachable(String),
    Permanent(String),
    RetryAfter {
        message: String,
        seconds: u64,
    },
    TooSmall {
        got: u64,
        wanted: u64,
    },
    WrongSize {
        got: u64,
        wanted: u64,
    },
    Checksum {
        got: String,
        wanted: String,
    },
    Cancelled,
    /// The server answered in a way that does not fit the request — a range that starts
    /// elsewhere, a length that disagrees with itself, a partial answer to a whole-file request.
    /// The bytes cannot be trusted to continue, so the download starts again from nothing.
    Protocol(&'static str),
    Io(std::io::Error),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(message) => write!(f, "could not download: {message}"),
            Self::Permanent(message) => write!(f, "could not download: {message}"),
            Self::RetryAfter { message, seconds } => {
                write!(f, "{message}; retry requested after {seconds} seconds")
            }
            Self::TooSmall { got, wanted } => write!(
                f,
                "only {got} bytes arrived and at least {wanted} were expected — setup will resume this download"
            ),
            Self::WrongSize { got, wanted } => {
                write!(f, "downloaded {got} bytes but the release catalog expects {wanted}")
            }
            Self::Checksum { got, wanted } => write!(
                f,
                "download verification failed (SHA-256 {got}, expected {wanted})"
            ),
            Self::Cancelled => write!(f, "paused"),
            Self::Protocol(reason) => write!(
                f,
                "the server's answer did not fit the request ({reason}); the download starts again"
            ),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FetchError {}

impl From<std::io::Error> for FetchError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub done: u64,
    pub total: Option<u64>,
    pub speed_bytes_per_second: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub attempt: u8,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Verification<'a> {
    pub expected_bytes: Option<u64>,
    pub sha256: Option<&'a str>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResumeMeta {
    url: String,
    etag: Option<String>,
    last_modified: Option<String>,
}

const BODY_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
const BODY_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_ATTEMPTS: u8 = 8;
/// How many times one call may throw its partial away and start again after a [`FetchError::Protocol`].
/// Counted apart from network retries: a server that keeps answering the wrong question is not
/// going to start answering the right one on the ninth try.
const MAX_RESTARTS: u8 = 2;

pub fn download(
    url: &str,
    into: &Path,
    min_bytes: u64,
    cancel: &Arc<AtomicBool>,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<u64, FetchError> {
    download_verified(
        url,
        into,
        min_bytes,
        Verification::default(),
        cancel,
        on_progress,
    )
}

pub fn download_verified(
    url: &str,
    into: &Path,
    min_bytes: u64,
    verification: Verification<'_>,
    cancel: &Arc<AtomicBool>,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<u64, FetchError> {
    if let Some(parent) = into.parent() {
        fs::create_dir_all(parent)?;
    }

    if into.is_file() && verify_file(into, min_bytes, verification).is_ok() {
        return Ok(into.metadata()?.len());
    }

    let partial = partial_path(into);
    let metadata = resume_metadata_path(into);
    discard_foreign_resume(url, &partial, &metadata)?;

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_resolve(Some(CONNECT_TIMEOUT))
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_send_request(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(RESPONSE_TIMEOUT))
        .timeout_recv_body(Some(BODY_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();

    let mut last_error = None;
    let mut restarts = 0u8;
    for attempt in 1..=MAX_ATTEMPTS {
        if cancel.load(Ordering::Relaxed) {
            return Err(FetchError::Cancelled);
        }

        let mut requested_delay = None;
        match one_attempt(
            &agent,
            url,
            &partial,
            &metadata,
            verification,
            cancel,
            attempt,
            on_progress,
        ) {
            Ok(written) => match verify_file(&partial, min_bytes, verification) {
                Ok(()) => {
                    publish_file(&partial, into)?;
                    let _ = fs::remove_file(&metadata);
                    return Ok(written);
                }
                Err(error @ FetchError::Checksum { .. })
                | Err(error @ FetchError::WrongSize { .. }) => {
                    // Verification has proved these bytes cannot become the catalog artifact.
                    let _ = fs::remove_file(&partial);
                    let _ = fs::remove_file(&metadata);
                    return Err(error);
                }
                Err(error) => last_error = Some(error),
            },
            Err(FetchError::Cancelled) => return Err(FetchError::Cancelled),
            Err(error @ FetchError::Permanent(_)) => return Err(error),
            Err(error @ FetchError::Protocol(_)) => {
                // Nothing in the partial can be trusted to continue from: start again, a bounded
                // number of times, and without waiting — this is not a network that needs a rest.
                let _ = fs::remove_file(&partial);
                let _ = fs::remove_file(&metadata);
                restarts += 1;
                if restarts > MAX_RESTARTS {
                    return Err(error);
                }
                last_error = Some(error);
                continue;
            }
            Err(error) => {
                if let FetchError::RetryAfter { seconds, .. } = &error {
                    if *seconds > 0 {
                        requested_delay = Some(Duration::from_secs((*seconds).min(15 * 60)));
                    }
                }
                last_error = Some(error);
            }
        }

        if attempt < MAX_ATTEMPTS {
            wait_with_cancel(
                requested_delay.unwrap_or_else(|| retry_delay(attempt)),
                cancel,
            )?;
        }
    }

    Err(last_error
        .unwrap_or_else(|| FetchError::Unreachable("setup paused after eight attempts".into())))
}

/// Verify and atomically publish bytes delivered by a platform transfer service such as BITS.
/// The platform backend owns interruption/resume; this function owns the same trust boundary as
/// the HTTP backend so the two cannot disagree about what “installed” means.
pub fn accept_download(
    staged: &Path,
    into: &Path,
    min_bytes: u64,
    verification: Verification<'_>,
) -> Result<u64, FetchError> {
    match verify_file(staged, min_bytes, verification) {
        Ok(()) => {
            let bytes = staged.metadata()?.len();
            publish_file(staged, into)?;
            Ok(bytes)
        }
        Err(error @ FetchError::Checksum { .. }) | Err(error @ FetchError::WrongSize { .. }) => {
            let _ = fs::remove_file(staged);
            Err(error)
        }
        Err(error) => Err(error),
    }
}

#[allow(clippy::too_many_arguments)]
fn one_attempt(
    agent: &ureq::Agent,
    url: &str,
    partial: &Path,
    metadata: &Path,
    verification: Verification<'_>,
    cancel: &Arc<AtomicBool>,
    attempt: u8,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<u64, FetchError> {
    let mut offset = partial.metadata().map(|m| m.len()).unwrap_or(0);
    let resume = read_resume_metadata(metadata).filter(|saved| saved.url == url);
    let validator = resume
        .as_ref()
        .and_then(|saved| saved.etag.clone().or_else(|| saved.last_modified.clone()));
    // **Without a hash to check the finished file against, a partial is continued only under a
    // validator the server can compare.** Otherwise the bytes on disk could be the start of an
    // earlier version — a branch archive changes in place — and nothing downstream would notice.
    if offset > 0 && verification.sha256.is_none() && validator.is_none() {
        let _ = fs::remove_file(partial);
        offset = 0;
    }
    let mut request = agent.get(url);
    if offset > 0 {
        request = request.header("Range", format!("bytes={offset}-"));
        if let Some(validator) = &validator {
            request = request.header("If-Range", validator);
        }
    }

    let mut response = request
        .call()
        .map_err(|error| FetchError::Unreachable(error.to_string()))?;
    let status = response.status().as_u16();
    if status == 416 && offset > 0 {
        // "Nothing left to send" proves a length, not that these bytes are this file.
        return if a_refused_range_proves_complete(&response, offset, verification, resume.as_ref())
        {
            Ok(offset)
        } else {
            Err(FetchError::Protocol(
                "a refused range, with nothing proving the partial complete",
            ))
        };
    }
    if !(200..300).contains(&status) {
        let message = format!("server returned HTTP {status}");
        if status == 408 || status == 429 || status >= 500 {
            return Err(FetchError::RetryAfter {
                seconds: retry_after_seconds(&response).unwrap_or(0),
                message,
            });
        }
        return Err(FetchError::Permanent(message));
    }
    if status == 206 && offset == 0 {
        return Err(FetchError::Protocol(
            "a partial answer to a request for the whole file",
        ));
    }

    let mut span = None;
    let mut range_total = None;
    if offset > 0 && status == 206 {
        let (length, total) =
            check_partial_answer(&response, offset, verification, resume.as_ref())?;
        span = Some(length);
        range_total = total;
    } else if offset > 0 {
        // The origin ignored Range or changed representation. Never append a full response.
        offset = 0;
        let _ = fs::remove_file(partial);
    }

    let total = match span {
        Some(length) => Some(range_total.unwrap_or(offset + length)),
        None => response.body().content_length(),
    };
    let saved = ResumeMeta {
        url: url.to_string(),
        etag: header(&response, "etag"),
        last_modified: header(&response, "last-modified"),
    };
    write_resume_metadata(metadata, &saved)?;
    stream(
        &mut response,
        partial,
        offset,
        span,
        total,
        cancel,
        attempt,
        on_progress,
    )
}

/// The length of a `206`'s body, and the file's total when it says — or why it cannot be trusted.
///
/// A single-range `206` must say which bytes it carries (`Content-Range: bytes S-E/T`). Appending
/// is only safe when those are exactly the bytes asked for: S is the partial's length, E is not
/// before S, T (when given) is the size the catalog expects, `Content-Length` (when given) is
/// E - S + 1, and the server's ETag (when it gives one) is the one the partial was saved under.
fn check_partial_answer(
    response: &ureq::http::Response<ureq::Body>,
    offset: u64,
    verification: Verification<'_>,
    saved: Option<&ResumeMeta>,
) -> Result<(u64, Option<u64>), FetchError> {
    let value = header(response, "content-range").ok_or(FetchError::Protocol(
        "a partial answer that does not say which bytes it carries",
    ))?;
    let (start, end, total) =
        parse_content_range(&value).ok_or(FetchError::Protocol("an unreadable Content-Range"))?;
    if start != offset {
        return Err(FetchError::Protocol("a range that starts somewhere else"));
    }
    if let (Some(total), Some(wanted)) = (total, verification.expected_bytes) {
        if total != wanted {
            return Err(FetchError::Protocol("a total the catalog does not expect"));
        }
    }
    let length = end - start + 1;
    if response
        .body()
        .content_length()
        .is_some_and(|declared| declared != length)
    {
        return Err(FetchError::Protocol(
            "a length that disagrees with its range",
        ));
    }
    if let (Some(now), Some(then)) = (
        header(response, "etag"),
        saved.and_then(|saved| saved.etag.clone()),
    ) {
        if now != then {
            return Err(FetchError::Protocol("a different version of the file"));
        }
    }
    Ok((length, total))
}

/// `bytes S-E/T` or `bytes S-E/*` into `(S, E, T)`. `None` for anything else, including E < S
/// and E at or past T.
fn parse_content_range(value: &str) -> Option<(u64, u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end: u64 = end.trim().parse().ok()?;
    if end < start {
        return None;
    }
    let total = match total.trim() {
        "*" => None,
        length => Some(length.parse::<u64>().ok()?),
    };
    if total.is_some_and(|total| end >= total) {
        return None;
    }
    Some((start, end, total))
}

/// Whether a `416` shows the partial is the whole file.
///
/// It must say the file is exactly as long as the partial (`bytes */N`, N = the partial's
/// length). That proves a length. With a hash to check, the hash proves the rest once the file is
/// verified. Without one, only a strong ETag the server repeats now, equal to the one the partial
/// was saved under, ties these bytes to this file; a weak one (`W/…`) promises less than that.
fn a_refused_range_proves_complete(
    response: &ureq::http::Response<ureq::Body>,
    offset: u64,
    verification: Verification<'_>,
    saved: Option<&ResumeMeta>,
) -> bool {
    let complete_length = header(response, "content-range").and_then(|value| {
        value
            .trim()
            .strip_prefix("bytes */")
            .and_then(|length| length.trim().parse::<u64>().ok())
    });
    if complete_length != Some(offset) {
        return false;
    }
    if verification.sha256.is_some() {
        return true;
    }
    match (
        saved.and_then(|saved| saved.etag.as_deref()),
        header(response, "etag"),
    ) {
        (Some(then), Some(now)) => !then.starts_with("W/") && then == now,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn stream(
    response: &mut ureq::http::Response<ureq::Body>,
    partial: &Path,
    offset: u64,
    span: Option<u64>,
    total: Option<u64>,
    cancel: &Arc<AtomicBool>,
    attempt: u8,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<u64, FetchError> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(offset > 0)
        .truncate(offset == 0)
        .open(partial)?;
    let mut reader = response.body_mut().with_config().limit(BODY_LIMIT).reader();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut done = offset;
    let mut reported = offset;
    let started = Instant::now();

    loop {
        if cancel.load(Ordering::Relaxed) {
            file.flush()?;
            return Err(FetchError::Cancelled);
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        // A range is written up to its last byte and never past it.
        let keep = match span {
            Some(span) => read.min(span.saturating_sub(done - offset) as usize),
            None => read,
        };
        file.write_all(&buffer[..keep])?;
        done += keep as u64;
        if keep < read {
            file.flush()?;
            return Err(FetchError::Protocol(
                "more bytes than the range it announced",
            ));
        }
        if done - reported >= 512 * 1024 {
            reported = done;
            report(done, total, offset, started, attempt, on_progress);
        }
    }

    file.flush()?;
    file.sync_data()?;
    report(done, total, offset, started, attempt, on_progress);
    if span.is_some_and(|span| done - offset < span) {
        // What arrived is good and stays: the next attempt resumes after it.
        return Err(FetchError::Unreachable(
            "the connection ended before the range did".into(),
        ));
    }
    Ok(done)
}

fn report(
    done: u64,
    total: Option<u64>,
    offset: u64,
    started: Instant,
    attempt: u8,
    on_progress: &mut dyn FnMut(Progress),
) {
    let elapsed = started.elapsed().as_secs_f64();
    let speed = (elapsed > 0.2).then(|| ((done - offset) as f64 / elapsed) as u64);
    let eta = speed.and_then(|speed| total.map(|total| total.saturating_sub(done) / speed.max(1)));
    on_progress(Progress {
        done,
        total,
        speed_bytes_per_second: speed,
        eta_seconds: eta,
        attempt,
    });
}

fn verify_file(
    path: &Path,
    min_bytes: u64,
    verification: Verification<'_>,
) -> Result<(), FetchError> {
    let got = path.metadata()?.len();
    if got < min_bytes {
        return Err(FetchError::TooSmall {
            got,
            wanted: min_bytes,
        });
    }
    if let Some(wanted) = verification.expected_bytes {
        if got != wanted {
            return Err(FetchError::WrongSize { got, wanted });
        }
    }
    if let Some(wanted) = verification.sha256 {
        let mut file = File::open(path)?;
        let mut hash = Sha256::new();
        std::io::copy(&mut file, &mut crate::hashing::Hashing(&mut hash))?;
        let got = crate::hashing::hex(hash.finalize());
        if !got.eq_ignore_ascii_case(wanted) {
            return Err(FetchError::Checksum {
                got,
                wanted: wanted.to_string(),
            });
        }
    }
    Ok(())
}

fn publish_file(partial: &Path, target: &Path) -> Result<(), FetchError> {
    // Appended, not `with_extension`, for the reason `partial_path` below does the same: replacing
    // the extension maps `model.onnx` and `model.bin` in one directory onto a single
    // `model.setup-old`, so publishing the second one deletes the first one's rollback copy and a
    // failure then restores the wrong bytes under the wrong name.
    let backup = target.with_file_name(format!(
        "{}.setup-old",
        target.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = fs::remove_file(&backup);
    if target.exists() {
        fs::rename(target, &backup)?;
    }
    if let Err(error) = fs::rename(partial, target) {
        let _ = fs::rename(&backup, target);
        return Err(error.into());
    }
    let _ = fs::remove_file(backup);
    Ok(())
}

fn partial_path(target: &Path) -> PathBuf {
    target.with_file_name(format!(
        "{}.partial",
        target.file_name().unwrap_or_default().to_string_lossy()
    ))
}

fn resume_metadata_path(target: &Path) -> PathBuf {
    target.with_file_name(format!(
        "{}.partial.json",
        target.file_name().unwrap_or_default().to_string_lossy()
    ))
}

fn discard_foreign_resume(url: &str, partial: &Path, metadata: &Path) -> Result<(), FetchError> {
    if partial.exists() && read_resume_metadata(metadata).is_some_and(|saved| saved.url != url) {
        fs::remove_file(partial)?;
        let _ = fs::remove_file(metadata);
    }
    Ok(())
}

fn header(response: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn retry_after_seconds(response: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    let value = header(response, "retry-after")?;
    if let Ok(seconds) = value.trim().parse() {
        return Some(seconds);
    }
    let when = chrono::DateTime::parse_from_rfc2822(value.trim()).ok()?;
    let seconds = when
        .with_timezone(&chrono::Utc)
        .signed_duration_since(chrono::Utc::now())
        .num_seconds();
    Some(seconds.max(0) as u64)
}

fn read_resume_metadata(path: &Path) -> Option<ResumeMeta> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn write_resume_metadata(path: &Path, metadata: &ResumeMeta) -> Result<(), FetchError> {
    let bytes = serde_json::to_vec(metadata).map_err(|error| {
        FetchError::Unreachable(format!("could not record resume state: {error}"))
    })?;
    fs::write(path, bytes)?;
    Ok(())
}

fn retry_delay(attempt: u8) -> Duration {
    #[cfg(test)]
    {
        let _ = attempt;
        Duration::ZERO
    }
    #[cfg(not(test))]
    {
        const SECONDS: [u64; 7] = [2, 5, 15, 30, 60, 120, 120];
        Duration::from_secs(SECONDS[usize::from(attempt.saturating_sub(1)).min(6)])
    }
}

fn wait_with_cancel(duration: Duration, cancel: &Arc<AtomicBool>) -> Result<(), FetchError> {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        if cancel.load(Ordering::Relaxed) {
            return Err(FetchError::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(200).min(duration));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    fn serve(body: Vec<u8>, honor_range: bool) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let clone = stream.try_clone().unwrap();
            let mut range = None;
            for line in BufReader::new(clone).lines().map_while(Result::ok) {
                if line.is_empty() {
                    break;
                }
                if let Some(value) = line.strip_prefix("Range: bytes=") {
                    range = value.trim_end_matches('-').parse::<usize>().ok();
                }
            }
            let start = if honor_range { range.unwrap_or(0) } else { 0 };
            let status = if start > 0 {
                "206 Partial Content"
            } else {
                "200 OK"
            };
            let range = if start > 0 {
                format!(
                    "Content-Range: bytes {start}-{}/{}\r\n",
                    body.len() - 1,
                    body.len()
                )
            } else {
                String::new()
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{range}ETag: \"fixture\"\r\nConnection: close\r\n\r\n",
                body.len() - start
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(&body[start..]).unwrap();
        });
        (format!("http://127.0.0.1:{port}/file"), handle)
    }

    fn scratch(name: &str) -> PathBuf {
        // Keyed on the process id like every other scratch path here: two overlapping
        // `cargo test` runs share `temp_dir()`, and each one's cleanup deletes the
        // other's fixture mid-test. It surfaced as PermissionDenied from a different
        // test each run, which reads like a real fault and is not one.
        let root =
            std::env::temp_dir().join(format!("sakatsu-fetch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn verified_download_is_published_atomically() {
        let root = scratch("verified");
        let body = vec![7; 1024 * 1024];
        let sha = crate::hashing::hex(Sha256::digest(&body));
        let (url, server) = serve(body.clone(), true);
        let target = root.join("model.onnx");
        download_verified(
            &url,
            &target,
            100,
            Verification {
                expected_bytes: Some(body.len() as u64),
                sha256: Some(&sha),
            },
            &Arc::new(AtomicBool::new(false)),
            &mut |_| assert!(!target.exists()),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(target).unwrap(), body);
    }

    #[test]
    fn an_existing_partial_is_resumed() {
        let root = scratch("resume");
        let body = vec![3; 1024 * 1024];
        let target = root.join("model.onnx");
        fs::write(partial_path(&target), &body[..256 * 1024]).unwrap();
        let (url, server) = serve(body.clone(), true);
        write_resume_metadata(
            &resume_metadata_path(&target),
            &ResumeMeta {
                url: url.clone(),
                etag: Some("\"fixture\"".into()),
                last_modified: None,
            },
        )
        .unwrap();
        download(
            &url,
            &target,
            100,
            &Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(target).unwrap(), body);
    }

    #[test]
    fn cancellation_preserves_partial_data() {
        let root = scratch("paused");
        let body = vec![9; 2 * 1024 * 1024];
        let (url, server) = serve(body, true);
        let target = root.join("model.onnx");
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let error = download(&url, &target, 100, &cancel, &mut move |progress| {
            if progress.done > 0 {
                flag.store(true, Ordering::Relaxed);
            }
        })
        .unwrap_err();
        server.join().unwrap();
        assert!(matches!(error, FetchError::Cancelled));
        assert!(partial_path(&target).is_file());
        assert!(!target.exists());
    }

    #[test]
    fn a_server_that_ignores_range_restarts_safely() {
        let root = scratch("ignore-range");
        let body = vec![5; 1024 * 1024];
        let target = root.join("model.onnx");
        fs::write(partial_path(&target), vec![1; 200_000]).unwrap();
        let (url, server) = serve(body.clone(), false);
        write_resume_metadata(
            &resume_metadata_path(&target),
            &ResumeMeta {
                url: url.clone(),
                etag: Some("\"fixture\"".into()),
                last_modified: None,
            },
        )
        .unwrap();
        download(
            &url,
            &target,
            100,
            &Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(target).unwrap(), body);
    }

    /// One scripted answer, given to the next connection whatever it asked for.
    struct Answer {
        status: &'static str,
        headers: Vec<(&'static str, String)>,
        body: Vec<u8>,
        /// Send only this many bytes of the body, then close.
        cut: Option<usize>,
    }

    fn whole(body: &[u8], etag: &str) -> Answer {
        Answer {
            status: "200 OK",
            headers: vec![
                ("Content-Length", body.len().to_string()),
                ("ETag", etag.to_string()),
            ],
            body: body.to_vec(),
            cut: None,
        }
    }

    /// A correct `206` for everything from `from` on.
    fn rest(body: &[u8], from: usize, etag: &str) -> Answer {
        Answer {
            status: "206 Partial Content",
            headers: vec![
                ("Content-Length", (body.len() - from).to_string()),
                (
                    "Content-Range",
                    format!("bytes {from}-{}/{}", body.len() - 1, body.len()),
                ),
                ("ETag", etag.to_string()),
            ],
            body: body[from..].to_vec(),
            cut: None,
        }
    }

    fn status(status: &'static str, headers: Vec<(&'static str, String)>) -> Answer {
        let mut headers = headers;
        headers.push(("Content-Length", "0".into()));
        Answer {
            status,
            headers,
            body: Vec::new(),
            cut: None,
        }
    }

    /// Serve `answers` in order, one per connection, and return each request's header lines.
    ///
    /// Stops accepting after a deadline rather than waiting for a request that never comes, so a
    /// download that gives up early ends the test instead of hanging it.
    fn script(answers: Vec<Answer>) -> (String, std::thread::JoinHandle<Vec<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for answer in answers {
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(_) if Instant::now() < deadline => {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(_) => return requests,
                    }
                };
                stream.set_nonblocking(false).unwrap();
                let mut lines = Vec::new();
                for line in BufReader::new(stream.try_clone().unwrap())
                    .lines()
                    .map_while(Result::ok)
                {
                    if line.is_empty() {
                        break;
                    }
                    lines.push(line);
                }
                requests.push(lines);
                let mut head = format!("HTTP/1.1 {}\r\n", answer.status);
                for (name, value) in &answer.headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str("Connection: close\r\n\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&answer.body[..answer.cut.unwrap_or(answer.body.len())]);
            }
            requests
        });
        (format!("http://127.0.0.1:{port}/file"), handle)
    }

    /// A request header's value, by name in any case: the client sends them lowercase.
    fn header_line(request: &[String], name: &str) -> Option<String> {
        request.iter().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    }

    fn asked_for_a_range(request: &[String]) -> Option<String> {
        header_line(request, "Range")
    }

    fn body(len: usize) -> Vec<u8> {
        (0..len).map(|index| (index % 251) as u8).collect()
    }

    fn sha(bytes: &[u8]) -> String {
        crate::hashing::hex(Sha256::digest(bytes))
    }

    /// A partial already on disk, saved under `etag`, as a previous attempt would have left it.
    fn leave_partial(target: &Path, bytes: &[u8], url: &str, etag: Option<&str>) {
        fs::write(partial_path(target), bytes).unwrap();
        write_resume_metadata(
            &resume_metadata_path(target),
            &ResumeMeta {
                url: url.to_string(),
                etag: etag.map(str::to_string),
                last_modified: None,
            },
        )
        .unwrap();
    }

    fn fetch(
        url: &str,
        target: &Path,
        expected: Option<u64>,
        hash: Option<&str>,
    ) -> Result<u64, FetchError> {
        download_verified(
            url,
            target,
            1,
            Verification {
                expected_bytes: expected,
                sha256: hash,
            },
            &Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        )
    }

    #[test]
    fn a_correct_range_resumes_and_appends_only_what_was_missing() {
        let root = scratch("range-good");
        let target = root.join("model.onnx");
        let data = body(300_000);
        let (url, server) = script(vec![rest(&data, 100_000, "\"v1\"")]);
        leave_partial(&target, &data[..100_000], &url, Some("\"v1\""));
        fetch(&url, &target, Some(data.len() as u64), Some(&sha(&data))).unwrap();
        let requests = server.join().unwrap();
        assert_eq!(
            asked_for_a_range(&requests[0]).as_deref(),
            Some("bytes=100000-")
        );
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn a_range_that_starts_elsewhere_is_never_appended() {
        let root = scratch("range-start");
        let target = root.join("model.onnx");
        let data = body(300_000);
        let mut wrong = rest(&data, 50_000, "\"v1\"");
        wrong.headers.retain(|(name, _)| *name != "Content-Length");
        let (url, server) = script(vec![wrong, whole(&data, "\"v1\"")]);
        leave_partial(&target, &data[..100_000], &url, Some("\"v1\""));
        fetch(&url, &target, Some(data.len() as u64), Some(&sha(&data))).unwrap();
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            asked_for_a_range(&requests[1]),
            None,
            "the restart asks for the whole file"
        );
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn a_range_with_the_wrong_total_restarts() {
        let root = scratch("range-total");
        let target = root.join("model.onnx");
        let data = body(300_000);
        let mut wrong = rest(&data, 100_000, "\"v1\"");
        for header in &mut wrong.headers {
            if header.0 == "Content-Range" {
                header.1 = format!("bytes 100000-299999/{}", data.len() + 10);
            }
        }
        let (url, server) = script(vec![wrong, whole(&data, "\"v1\"")]);
        leave_partial(&target, &data[..100_000], &url, Some("\"v1\""));
        fetch(&url, &target, Some(data.len() as u64), None).unwrap();
        assert_eq!(server.join().unwrap().len(), 2);
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn a_partial_answer_without_its_range_restarts() {
        let root = scratch("range-missing");
        let target = root.join("model.onnx");
        let data = body(300_000);
        let mut bare = rest(&data, 100_000, "\"v1\"");
        bare.headers.retain(|(name, _)| *name != "Content-Range");
        let (url, server) = script(vec![bare, whole(&data, "\"v1\"")]);
        leave_partial(&target, &data[..100_000], &url, Some("\"v1\""));
        fetch(&url, &target, Some(data.len() as u64), Some(&sha(&data))).unwrap();
        assert_eq!(server.join().unwrap().len(), 2);
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn a_length_that_disagrees_with_its_range_restarts() {
        let root = scratch("range-length");
        let target = root.join("model.onnx");
        let data = body(300_000);
        let mut wrong = rest(&data, 100_000, "\"v1\"");
        for header in &mut wrong.headers {
            if header.0 == "Content-Length" {
                header.1 = "5".into();
            }
        }
        let (url, server) = script(vec![wrong, whole(&data, "\"v1\"")]);
        leave_partial(&target, &data[..100_000], &url, Some("\"v1\""));
        fetch(&url, &target, Some(data.len() as u64), Some(&sha(&data))).unwrap();
        assert_eq!(server.join().unwrap().len(), 2);
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn a_short_range_keeps_what_arrived_and_resumes_after_it() {
        let root = scratch("range-short");
        let target = root.join("model.onnx");
        let data = body(300_000);
        let mut short = rest(&data, 100_000, "\"v1\"");
        short.cut = Some(50_000);
        let (url, server) = script(vec![short, rest(&data, 150_000, "\"v1\"")]);
        leave_partial(&target, &data[..100_000], &url, Some("\"v1\""));
        fetch(&url, &target, Some(data.len() as u64), Some(&sha(&data))).unwrap();
        let requests = server.join().unwrap();
        assert_eq!(
            asked_for_a_range(&requests[1]).as_deref(),
            Some("bytes=150000-")
        );
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn a_range_longer_than_announced_is_never_written_past_its_end() {
        let root = scratch("range-long");
        let target = root.join("model.onnx");
        let data = body(300_000);
        let mut long = rest(&data, 100_000, "\"v1\"");
        long.headers.retain(|(name, _)| *name != "Content-Length");
        for header in &mut long.headers {
            if header.0 == "Content-Range" {
                header.1 = "bytes 100000-199999/300000".into();
            }
        }
        let (url, server) = script(vec![long, whole(&data, "\"v1\"")]);
        leave_partial(&target, &data[..100_000], &url, Some("\"v1\""));
        fetch(&url, &target, Some(data.len() as u64), Some(&sha(&data))).unwrap();
        assert_eq!(
            server.join().unwrap().len(),
            2,
            "the overlong answer forced a restart"
        );
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn a_changed_version_restarts_rather_than_mixing_two_files() {
        let root = scratch("etag-changed");
        let target = root.join("model.onnx");
        let old = body(300_000);
        let new: Vec<u8> = old.iter().map(|byte| byte.wrapping_add(1)).collect();
        // A server that ignores If-Range and answers the range from the new version.
        let (url, server) = script(vec![rest(&new, 100_000, "\"v2\""), whole(&new, "\"v2\"")]);
        leave_partial(&target, &old[..100_000], &url, Some("\"v1\""));
        fetch(&url, &target, Some(new.len() as u64), None).unwrap();
        let requests = server.join().unwrap();
        assert_eq!(
            header_line(&requests[0], "If-Range").as_deref(),
            Some("\"v1\"")
        );
        assert_eq!(fs::read(&target).unwrap(), new);
    }

    #[test]
    fn a_refused_range_with_a_hash_is_accepted_and_verified() {
        let root = scratch("416-hash");
        let target = root.join("model.onnx");
        let data = body(200_000);
        let refused = status(
            "416 Range Not Satisfiable",
            vec![("Content-Range", format!("bytes */{}", data.len()))],
        );
        let (url, server) = script(vec![refused]);
        leave_partial(&target, &data, &url, Some("\"v1\""));
        fetch(&url, &target, Some(data.len() as u64), Some(&sha(&data))).unwrap();
        assert_eq!(server.join().unwrap().len(), 1);
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn a_refused_range_without_proof_starts_again() {
        let root = scratch("416-unproven");
        let target = root.join("model.onnx");
        let stale = body(200_000);
        let current: Vec<u8> = stale.iter().map(|byte| byte ^ 0x5a).collect();
        // Same length, no hash, and a weak validator: nothing ties the partial to this file.
        let refused = status(
            "416 Range Not Satisfiable",
            vec![
                ("Content-Range", format!("bytes */{}", stale.len())),
                ("ETag", "W/\"v1\"".into()),
            ],
        );
        let (url, server) = script(vec![refused, whole(&current, "W/\"v1\"")]);
        leave_partial(&target, &stale, &url, Some("W/\"v1\""));
        fetch(&url, &target, None, None).unwrap();
        assert_eq!(server.join().unwrap().len(), 2);
        assert_eq!(
            fs::read(&target).unwrap(),
            current,
            "the stale partial was not published"
        );
    }

    #[test]
    fn a_refused_range_with_a_repeated_strong_etag_is_accepted_without_a_hash() {
        let root = scratch("416-strong");
        let target = root.join("model.onnx");
        let data = body(200_000);
        let refused = status(
            "416 Range Not Satisfiable",
            vec![
                ("Content-Range", format!("bytes */{}", data.len())),
                ("ETag", "\"v1\"".into()),
            ],
        );
        let (url, server) = script(vec![refused]);
        leave_partial(&target, &data, &url, Some("\"v1\""));
        fetch(&url, &target, None, None).unwrap();
        assert_eq!(server.join().unwrap().len(), 1);
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn without_a_hash_or_a_validator_a_partial_is_never_continued() {
        let root = scratch("hashless");
        let target = root.join("archive.zip");
        let data = body(200_000);
        let (url, server) = script(vec![whole(&data, "\"v1\"")]);
        fs::write(partial_path(&target), &data[..50_000]).unwrap();
        fetch(&url, &target, None, None).unwrap();
        let requests = server.join().unwrap();
        assert_eq!(
            asked_for_a_range(&requests[0]),
            None,
            "no Range without something to check it"
        );
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn a_missing_file_fails_at_once() {
        let root = scratch("404");
        let target = root.join("model.onnx");
        let (url, server) = script(vec![status("404 Not Found", vec![])]);
        let error = fetch(&url, &target, None, None).unwrap_err();
        assert!(matches!(error, FetchError::Permanent(_)), "{error}");
        assert_eq!(server.join().unwrap().len(), 1, "a 404 is not retried");
    }

    #[test]
    fn too_many_requests_waits_as_asked_then_succeeds() {
        let root = scratch("429");
        let target = root.join("model.onnx");
        let data = body(10_000);
        let (url, server) = script(vec![
            status("429 Too Many Requests", vec![("Retry-After", "0".into())]),
            whole(&data, "\"v1\""),
        ]);
        fetch(&url, &target, Some(data.len() as u64), None).unwrap();
        assert_eq!(server.join().unwrap().len(), 2);
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn server_errors_are_retried_a_bounded_number_of_times() {
        let root = scratch("503");
        let target = root.join("model.onnx");
        let answers = (0..MAX_ATTEMPTS)
            .map(|_| status("503 Service Unavailable", vec![]))
            .collect();
        let (url, server) = script(answers);
        let error = fetch(&url, &target, None, None).unwrap_err();
        assert!(matches!(error, FetchError::RetryAfter { .. }), "{error}");
        assert_eq!(server.join().unwrap().len(), usize::from(MAX_ATTEMPTS));
    }

    #[test]
    fn a_server_that_keeps_answering_the_wrong_question_gives_up_after_its_restarts() {
        let root = scratch("restarts");
        let target = root.join("model.onnx");
        let data = body(10_000);
        // A partial answer to a whole-file request, every time. Exactly as many as one try and two
        // restarts: a fourth request would be refused and end in a connection error instead.
        let answers = (0..=MAX_RESTARTS)
            .map(|_| rest(&data, 0, "\"v1\""))
            .collect();
        let (url, server) = script(answers);
        let error = fetch(&url, &target, Some(data.len() as u64), None).unwrap_err();
        assert!(matches!(error, FetchError::Protocol(_)), "{error}");
        assert_eq!(
            server.join().unwrap().len(),
            usize::from(MAX_RESTARTS) + 1,
            "one first try and two restarts"
        );
        assert!(!target.exists());
    }

    #[test]
    fn no_length_still_downloads_without_inventing_a_total() {
        let root = scratch("no-length");
        let target = root.join("archive.zip");
        let data = body(50_000);
        let mut no_length = whole(&data, "\"v1\"");
        no_length
            .headers
            .retain(|(name, _)| *name != "Content-Length");
        let (url, server) = script(vec![no_length]);
        let mut totals = Vec::new();
        download_verified(
            &url,
            &target,
            1,
            Verification::default(),
            &Arc::new(AtomicBool::new(false)),
            &mut |progress| totals.push(progress.total),
        )
        .unwrap();
        server.join().unwrap();
        assert!(totals.iter().all(Option::is_none), "no total was made up");
        assert_eq!(fs::read(&target).unwrap(), data);
    }

    #[test]
    fn a_wrong_hash_or_size_is_refused_and_a_working_file_is_kept() {
        let root = scratch("verify");
        let target = root.join("model.onnx");
        fs::write(&target, b"the working install").unwrap();
        let data = body(10_000);
        let (url, server) = script(vec![whole(&data, "\"v1\""), whole(&data, "\"v1\"")]);
        let error = fetch(&url, &target, None, Some(&"0".repeat(64))).unwrap_err();
        assert!(matches!(error, FetchError::Checksum { .. }), "{error}");
        let error = fetch(&url, &target, Some(data.len() as u64 + 1), None).unwrap_err();
        assert!(matches!(error, FetchError::WrongSize { .. }), "{error}");
        server.join().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"the working install");
        assert!(
            !partial_path(&target).exists(),
            "bytes proved wrong are not kept to resume"
        );
    }

    #[test]
    fn a_pause_during_a_retry_wait_is_honoured_at_once() {
        let root = scratch("pause-wait");
        let target = root.join("model.onnx");
        let (url, server) = script(vec![status(
            "503 Service Unavailable",
            vec![("Retry-After", "30".into())],
        )]);
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let pauser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let error = download_verified(
            &url,
            &target,
            1,
            Verification::default(),
            &cancel,
            &mut |_| {},
        )
        .unwrap_err();
        pauser.join().unwrap();
        server.join().unwrap();
        assert!(matches!(error, FetchError::Cancelled), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the thirty-second wait was cut short"
        );
    }

    #[test]
    fn content_ranges_are_read_strictly() {
        assert_eq!(
            parse_content_range("bytes 10-19/20"),
            Some((10, 19, Some(20)))
        );
        assert_eq!(parse_content_range("bytes 10-19/*"), Some((10, 19, None)));
        assert_eq!(parse_content_range("bytes 19-10/20"), None);
        assert_eq!(parse_content_range("bytes 10-20/20"), None);
        assert_eq!(parse_content_range("bytes */20"), None);
        assert_eq!(parse_content_range("items 10-19/20"), None);
    }
}
