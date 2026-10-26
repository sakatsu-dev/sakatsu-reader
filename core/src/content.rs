//! Content identity independent of installation path and modification timestamps.
use sha2::{Digest, Sha256};
use std::path::Path;

/// Hash names, node kinds and file bytes in deterministic order. Empty directories count:
/// a present empty gloss index behaves differently from an absent index.
pub fn fingerprint(root: &Path) -> Result<String, std::io::Error> {
    fn walk(root: &Path, directory: &Path, hash: &mut Sha256) -> Result<(), std::io::Error> {
        let mut entries = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(std::io::Error::other("cannot fingerprint a symbolic link"));
            }
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(std::io::Error::other)?;
            let name = relative
                .to_str()
                .ok_or_else(|| std::io::Error::other("non-UTF-8 resource path"))?
                .replace('\\', "/");
            hash.update((name.len() as u64).to_le_bytes());
            hash.update(name.as_bytes());
            if kind.is_dir() {
                hash.update(b"D");
                walk(root, &path, hash)?;
            } else if kind.is_file() {
                hash.update(b"F");
                let mut file = std::fs::File::open(&path)?;
                let expected = file.metadata()?.len();
                hash.update(expected.to_le_bytes());
                if std::io::copy(&mut file, &mut crate::hashing::Hashing(hash))? != expected {
                    return Err(std::io::Error::other(
                        "resource changed while fingerprinting",
                    ));
                }
            } else {
                return Err(std::io::Error::other("unsupported resource node"));
            }
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    hash.update(b"sakatsu-resource-tree-v1\0");
    walk(root, root, &mut hash)?;
    Ok(crate::hashing::hex(hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_titles_and_lengths_cannot_hide_different_bytes_or_missing_indexes() {
        let root = std::env::temp_dir().join(format!("sakatsu-content-{}", std::process::id()));
        // **Start clean, and leave nothing.** Process ids are reused, and this test ends by adding a
        // `gloss` index to `left`; a folder left by an earlier run with the same id made the two
        // trees differ before the test had changed anything, and failed it at random.
        let _ = std::fs::remove_dir_all(&root);
        let left = root.join("left");
        let right = root.join("right");
        for path in [&left, &right] {
            std::fs::create_dir_all(path).unwrap();
            std::fs::write(path.join("meta.json"), "same release").unwrap();
            std::fs::write(path.join("terms.ndjson"), "first").unwrap();
        }
        let before = fingerprint(&left).unwrap();
        assert_eq!(
            before,
            fingerprint(&right).unwrap(),
            "location is not content"
        );
        std::fs::write(right.join("terms.ndjson"), "other").unwrap();
        assert_ne!(
            before,
            fingerprint(&right).unwrap(),
            "same-length replacement counts"
        );
        std::fs::create_dir_all(left.join("gloss")).unwrap();
        assert_ne!(before, fingerprint(&left).unwrap(), "empty indexes count");
        let _ = std::fs::remove_dir_all(&root);
    }
}
