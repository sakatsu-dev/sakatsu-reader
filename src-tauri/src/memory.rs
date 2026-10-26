//! What this process has cost, at its worst.
//!
//! # The counter is a process high-water mark, and that shapes everything
//!
//! `GetProcessMemoryInfo` reports `PeakWorkingSetSize`: the largest working set the **whole
//! process** has ever held, from start to now. It is not a running gauge and it never falls, so
//! reading it before and after a call does not isolate that call — it reports the largest the
//! process ever got, including everything that happened first. A "difference" taken that way is
//! zero whenever an earlier stage was heavier, which is exactly when the number would be
//! interesting.
//!
//! So there is no `measure(|| …)` here on purpose. The honest use is: one workload per process,
//! read the peak once at the end, and report it next to the peak of a process that did nothing.
//! `tests/peak_memory.rs` is that harness.
//!
//! # Why this lives in the shell
//!
//! `core` does not link the Windows APIs and should not start: it is the part of the app that
//! builds and runs anywhere. The shell already links `windows` for the mouse hook and the title
//! bar, so this adds a feature group rather than a dependency.

/// The largest working set this process has held, in bytes.
///
/// `None` where the platform does not offer the counter, or the call fails — reported as absent
/// rather than as zero, because zero is a measurement and this is the absence of one.
#[cfg(windows)]
pub fn peak_working_set() -> Option<u64> {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: `counters` is a live, correctly sized `PROCESS_MEMORY_COUNTERS`, and its `cb` says
    // so. `GetCurrentProcess` is a pseudo-handle that needs no closing.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    ok.ok().map(|()| counters.PeakWorkingSetSize as u64)
}

/// Not available off Windows, and said so rather than guessed at.
#[cfg(not(windows))]
pub fn peak_working_set() -> Option<u64> {
    None
}

/// CPU time the calling thread has spent so far, user and kernel together.
///
/// **A running total for one thread**, unlike the peak above, so a difference around a call *is*
/// that call's CPU — on this thread. Work handed to another thread or process is not in it, which
/// is the point for `tests/assistance_latency.rs`: wall time there includes waiting on a lock, and
/// this does not.
///
/// The counter advances in scheduler ticks (about 15.6 ms on a default Windows timer), so one
/// short call reads as zero or one tick; the measurement reports medians over many calls.
#[cfg(windows)]
pub fn thread_cpu_time() -> Option<std::time::Duration> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};

    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: four live `FILETIME`s for the call to fill; `GetCurrentThread` is a pseudo-handle
    // that needs no closing.
    unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    }
    .ok()?;
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    // FILETIME counts 100-nanosecond intervals.
    Some(std::time::Duration::from_nanos(
        (ticks(kernel) + ticks(user)) * 100,
    ))
}

/// Not available off Windows, and said so rather than guessed at.
#[cfg(not(windows))]
pub fn thread_cpu_time() -> Option<std::time::Duration> {
    None
}

/// Bytes as megabytes, for a report a person reads.
pub fn megabytes(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
