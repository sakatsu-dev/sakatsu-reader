#![allow(dead_code)]
//! Fixtures shared by the recovery tests.
//!
//! **One owner for the interrupted-publication shape.** Two test binaries need a root that looks
//! exactly like a process stopped between the publisher's two renames, and a hand-written journal
//! is the kind of fixture that keeps passing after the real format moves. This one is produced by
//! the publisher itself, so it cannot drift from what recovery actually reads.

use std::path::Path;

/// Leave `destination` in the state an interruption between the publisher's two renames produces,
/// **with the journal the publisher itself wrote**.
///
/// `id` is taken from the caller and never derived from the destination's name: `frequency-jpdb`
/// installs into a directory called `jpdb`, so deriving one from the other puts the files where
/// nothing looks for them.
pub fn interrupt_a_real_publication(
    root: &Path,
    id: &str,
    destination: &Path,
    files: &[(&str, &str)],
) {
    let staged = root.join("staged");
    std::fs::create_dir_all(&staged).expect("staging");
    std::fs::write(staged.join("placeholder"), b"new").expect("staged file");
    std::fs::create_dir_all(destination).expect("destination");
    for (name, contents) in files {
        std::fs::write(destination.join(name), contents.as_bytes()).expect("destination file");
    }

    let members = [(id.to_string(), staged, destination.to_path_buf())];
    sakatsu_core::setup::publication::publish_with_commit(root, &members, || {
        Err("stopped".to_string())
    })
    .expect_err("the commit refused, so the journal and a rollback are on disk");

    // The rollback put the files back and cleared the empty backup; recreate what a process stopped
    // between the two renames would have left.
    let backup = root.join("backup");
    std::fs::create_dir_all(&backup).expect("backup");
    std::fs::rename(destination, backup.join(id)).expect("move the only copy aside");
    assert!(
        root.join("publication.json").is_file(),
        "the publisher must have left its journal, or this fixture is not the real shape"
    );
}

/// A scratch directory that is removed however the test ends.
///
/// **A guard rather than a call at the end.** Cleanup written as the last line of a test only runs
/// when the test passes, and a child process that fails an assertion never reaches it — which is
/// how 2,229 of these came to be sitting in the temporary directory, and how the disk filled during
/// this work.
pub struct Scratch(pub std::path::PathBuf);

impl Scratch {
    /// A fresh directory named for this test and this process.
    pub fn new(prefix: &str, name: &str) -> Scratch {
        let path =
            std::env::temp_dir().join(format!("sakatsu-{prefix}-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Scratch(path)
    }
}

impl std::ops::Deref for Scratch {
    type Target = std::path::Path;
    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
