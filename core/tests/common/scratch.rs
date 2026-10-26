#![allow(dead_code)]

/// A scratch directory removed when its scope ends, including during unwinding.
///
/// Success-only cleanup made failed tests accumulate the largest fixtures. The process id keeps
/// overlapping test runs separate; callers provide a purpose and case name so two users in one
/// integration-test binary do not collide.
pub struct Scratch(pub std::path::PathBuf);

impl Scratch {
    pub fn new(purpose: &str, case: &str) -> Scratch {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sakatsu-{purpose}-{case}-{}-{sequence}",
            std::process::id(),
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Scratch(path)
    }
}

impl AsRef<std::path::Path> for Scratch {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl std::ops::Deref for Scratch {
    type Target = std::path::Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
