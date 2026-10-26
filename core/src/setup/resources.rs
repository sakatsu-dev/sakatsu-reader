//! Verified resource trees. These operations do not resolve application paths or change
//! which resource set is active; callers supply the exact source and destination.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const RETIRED: &str = "retired-resources.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct RetiredResource {
    source: PathBuf,
    destination: PathBuf,
    fingerprint: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Retirement {
    version: u32,
    resources: Vec<RetiredResource>,
}

/// Copy and verify every tree before publishing or changing the configured location.
/// Old resources remain readable for this process. Removal, if requested, is deferred until
/// the next startup because model sessions and dictionary leases may still use those paths.
///
/// **The caller owns the whole tree before it works out what to move.** A move copies every
/// resource directory and then swaps where the app looks; an install or a gloss backfill running
/// alongside it publishes into a directory this has already read, and the new location silently
/// carries the older bytes. That was reproduced before the claim existed — see
/// `core/tests/resource_coordination.rs`.
///
/// The witness is taken rather than the claim, because the `mappings` are themselves resolved
/// resource locations: claiming in here would leave the caller free to work out where the
/// dictionaries live, wait behind somebody else's move, and then copy from a directory that had
/// been abandoned in the meantime.
pub fn relocate(
    owned: &crate::setup::claim::Claimed,
    target: &Path,
    mappings: &[(PathBuf, PathBuf)],
    remove_old: bool,
    activate: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<String, String> {
    owned.require(crate::setup::claim::Claim::Everything);
    let target = target.canonicalize().map_err(|error| error.to_string())?;
    if target.join(RETIRED).exists() {
        return Err("The previous resource move still has old copies awaiting cleanup. Restart before moving again.".into());
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let stage = target.join(format!(".relocation-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&stage).map_err(|error| error.to_string())?;
    let mut prepared = Vec::new();
    let mut retired = Vec::new();
    for (index, (source, destination)) in mappings.iter().enumerate() {
        if !source.exists() {
            continue;
        }
        let source = source.canonicalize().map_err(|error| error.to_string())?;
        if source == *destination {
            continue;
        }
        if target.starts_with(&source)
            || !destination.starts_with(&target)
            || destination == &target
            || destination
                .strip_prefix(&target)
                .unwrap_or(destination)
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err("Choose a location outside the existing resource folder.".into());
        }
        let copied = stage.join(format!("tree-{index}"));
        copy_tree(&source, &copied)?;
        let fingerprint =
            crate::content::fingerprint(&source).map_err(|error| error.to_string())?;
        if crate::content::fingerprint(&copied).map_err(|error| error.to_string())? != fingerprint {
            return Err(format!("The copy of {} did not verify.", source.display()));
        }
        prepared.push((format!("tree-{index}"), copied, destination.clone()));
        retired.push(RetiredResource {
            source,
            destination: destination.clone(),
            fingerprint,
        });
    }
    let count = retired.len();
    if remove_old && count > 0 {
        let journal = stage.join(RETIRED);
        let bytes = serde_json::to_vec_pretty(&Retirement {
            version: 1,
            resources: retired,
        })
        .map_err(|error| error.to_string())?;
        std::fs::write(&journal, bytes).map_err(|error| error.to_string())?;
        prepared.push((RETIRED.into(), journal, target.join(RETIRED)));
    }
    let outcome = super::publication::publish_with_commit(&stage, &prepared, || activate(&target));

    // **What the staging directory holds decides whether it may go**, and the two cases are not
    // alike. `tree-N` are copies: each was verified equal to its source before anything moved, and
    // the sources are still where they were, so they are preparation and disposable. A `backup/`
    // is not — `publish_with_commit` leaves one only when a restoration **failed**, and it may then
    // hold the only copy of what was at the destination.
    //
    // So the copies are discarded either way, and the directory itself is removed with `remove_dir`,
    // which refuses while anything remains. If it survives, it is holding something somebody may
    // need, and the caller is told where.
    for (name, _, _) in &prepared {
        let member = stage.join(name);
        // A member is a tree for a resource directory and a file for the retirement record, and
        // both are preparation: the record describes a move that did not happen.
        let _ = std::fs::remove_dir_all(&member);
        let _ = std::fs::remove_file(&member);
    }
    // **The same question the offline sweep asks**, rather than a list of filenames to delete: does
    // this root still hold a backup somebody may need? `Interrupted::Nothing` means it does not —
    // the journal beside it then describes nothing outstanding — and the directory can go. Anything
    // else stays, and the caller is told where it is.
    //
    // This also clears the successful path, which had been leaving its journal behind on every
    // move: `remove_dir` refuses a directory with a file in it, and `publish_with_commit` leaves
    // `publication.json` where it wrote it.
    if matches!(
        super::publication::interrupted_publication(&stage),
        super::publication::Interrupted::Nothing
    ) {
        let _ = std::fs::remove_dir_all(&stage);
    }
    if let Err(error) = outcome {
        return Err(if stage.exists() {
            format!(
                "{error} Files needed to recover this are kept at {}.",
                stage.display()
            )
        } else {
            error
        });
    }
    Ok(format!(
        "Verified {count} resource folders at {}.{}",
        target.display(),
        if remove_old && count > 0 {
            " Old copies remain available to this session and will be removed after restart if unchanged."
        } else {
            ""
        }
    ))
}

/// Run after the single-instance guard, with every actually resolved resource path protected.
/// A changed source or target is retained: the move's old consent does not authorize removing
/// files somebody changed afterwards. Errors are returned as notices and the journal survives.
///
/// **It stands aside for any other writer**, which is the caller's job: this runs on a background
/// thread from `.setup()`, so an install begun in the first seconds of a session overlaps it by
/// construction — and it was deleting old copies underneath one. The caller claims before it works
/// out which paths are protected, and reports the refusal as a notice; keeping the journal is the
/// safe direction, because nothing is lost and the next start tries again.
pub fn reclaim_relocated(
    owned: &crate::setup::claim::Claimed,
    target: &Path,
    protected: &[PathBuf],
) -> Vec<String> {
    owned.require(crate::setup::claim::Claim::Everything);
    let journal = target.join(RETIRED);
    let Ok(bytes) = std::fs::read(&journal) else {
        return Vec::new();
    };
    let retirement: Retirement = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => return vec![format!("Cannot read resource retirement record: {error}")],
    };
    if retirement.version != 1 {
        return vec!["Unknown resource retirement format; old copies kept.".into()];
    }
    let mut notices = Vec::new();
    for item in retirement.resources {
        if !item.source.exists() {
            continue;
        }
        let result = (|| {
            let source = item
                .source
                .canonicalize()
                .map_err(|error| error.to_string())?;
            let destination = item
                .destination
                .canonicalize()
                .map_err(|error| error.to_string())?;
            let active_root = target.canonicalize().map_err(|error| error.to_string())?;
            if source != item.source
                || !destination.starts_with(&active_root)
                || active_root.starts_with(&source)
                || source.starts_with(&active_root)
                || protected
                    .iter()
                    .filter_map(|path| path.canonicalize().ok())
                    .any(|path| path.starts_with(&source) || source.starts_with(&path))
            {
                return Err("the old folder is still active or its path changed".to_string());
            }
            for path in [&source, &destination] {
                if crate::content::fingerprint(path).map_err(|error| error.to_string())?
                    != item.fingerprint
                {
                    return Err("the resource files changed since the move".to_string());
                }
            }
            std::fs::remove_dir_all(&source).map_err(|error| error.to_string())
        })();
        if let Err(error) = result {
            notices.push(format!(
                "Old resources kept at {}: {error}",
                item.source.display()
            ));
        }
    }
    if notices.is_empty() {
        if let Err(error) = std::fs::remove_file(journal) {
            notices.push(error.to_string());
        }
    }
    notices
}

pub fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = entry.file_type().map_err(|error| error.to_string())?;
        if metadata.is_symlink() {
            return Err(format!(
                "Refusing to copy symbolic link {}",
                entry.path().display()
            ));
        }
        let target = destination.join(entry.file_name());
        if metadata.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if metadata.is_file() {
            std::fs::copy(entry.path(), target).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub fn verify_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let source_files = tree_files(source)?;
    let destination_files = tree_files(destination)?;
    if source_files != destination_files {
        return Err(format!("The copy of {} is incomplete.", source.display()));
    }
    for (relative, _) in source_files {
        let source_hash = sha256_file(&source.join(&relative))?;
        let destination_hash = sha256_file(&destination.join(&relative))?;
        if source_hash != destination_hash {
            return Err(format!(
                "{} did not verify after copying.",
                relative.display()
            ));
        }
    }
    Ok(())
}

pub fn tree_files(root: &Path) -> Result<Vec<(PathBuf, u64)>, String> {
    fn visit(root: &Path, current: &Path, files: &mut Vec<(PathBuf, u64)>) -> Result<(), String> {
        for entry in std::fs::read_dir(current).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_symlink() {
                return Err(format!("Refusing symbolic link {}", entry.path().display()));
            }
            if kind.is_dir() {
                visit(root, &entry.path(), files)?;
            } else if kind.is_file() {
                files.push((
                    entry
                        .path()
                        .strip_prefix(root)
                        .map_err(|error| error.to_string())?
                        .to_path_buf(),
                    entry.metadata().map_err(|error| error.to_string())?.len(),
                ));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut crate::hashing::Hashing(&mut hash))
        .map_err(|error| error.to_string())?;
    Ok(crate::hashing::hex(hash.finalize()))
}

/// Persist the resource location with the same durable replacement used by the store.
pub fn write_location(file: &Path, target: &Path) -> Result<(), std::io::Error> {
    crate::store::write_atomically(file, target.as_os_str().to_string_lossy().as_bytes())
        .map_err(std::io::Error::other)
}
