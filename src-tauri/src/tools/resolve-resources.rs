//! Print where the app would look for its resources, as JSON — or check they are all inside a root.
//!
//! ```text
//! cargo run -p sakatsu-desktop --bin resolve-resources
//! cargo run -p sakatsu-desktop --bin resolve-resources -- --inside <profile>
//! ```
//!
//! # Why this exists
//!
//! `resource_dir` has real precedence in it: an environment override wins, then the device-data
//! location, then the legacy roaming directory — and which of the last two is live depends on which
//! one currently holds usable files. `scripts/desktop-checkpoint.ps1` needs the answer so it can
//! copy those resources into an isolated profile, and a second copy of that rule written in
//! PowerShell would be a second thing to get wrong. It asks instead.
//!
//! It reads the environment it is given, so running it *before* the checkpoint script redirects
//! anything reports the developer's real paths — which is exactly what needs copying.
//!
//! # `--inside`, and why the check is here rather than in the launcher
//!
//! **Every** resource, not the four this used to report. The launcher could not detect an escaped
//! WordNet, audio or model path because it was never told about them, and each of those has its own
//! environment override that `resource_dir` answers before it looks anywhere else.
//!
//! Containment is answered here because doing it properly is not string work:
//!
//! - **Directory boundaries.** `C:\p-other` starts with `C:\p` and is not inside it. Compared by
//!   path component, never by prefix.
//! - **Junctions and symlinks.** A directory inside the profile can point anywhere. Both sides are
//!   canonicalized first, which resolves them; a path that cannot be canonicalized *and* does not
//!   exist is reported as unresolvable rather than assumed to be fine.
//! - **Case.** Windows paths compare case-insensitively, and a check that did not would pass a
//!   profile spelled with a different case and fail nothing.

use std::path::{Component, Path, PathBuf};

/// Every resource the app resolves, with the override that can send it somewhere else.
fn resources() -> Vec<(&'static str, &'static str, PathBuf)> {
    vec![
        ("store", "", sakatsu_desktop_lib::store_dir()),
        (
            "dictionaries",
            "SAKATSU_DICT_DIR",
            sakatsu_desktop_lib::dictionaries_dir(),
        ),
        (
            "kanji",
            "SAKATSU_KANJI_DIR",
            sakatsu_desktop_lib::kanji_dir(),
        ),
        (
            "wordnet",
            "SAKATSU_WORDNET_DIR",
            sakatsu_desktop_lib::wordnet_dir(),
        ),
        (
            "audio",
            "SAKATSU_AUDIO_DIR",
            sakatsu_desktop_lib::audio_dir(),
        ),
        (
            "model",
            "SAKATSU_MODEL_DIR",
            sakatsu_desktop_lib::model_dir(),
        ),
        (
            "paddle",
            "SAKATSU_PADDLE_DIR",
            sakatsu_desktop_lib::paddle_model_dir(),
        ),
        ("vlm", "SAKATSU_VLM_DIR", sakatsu_desktop_lib::vlm_dir()),
        (
            "translation",
            "SAKATSU_MT_DIR",
            sakatsu_desktop_lib::translation_model_dir(),
        ),
        (
            "device_data",
            "SAKATSU_DEVICE_DATA_DIR",
            sakatsu_desktop_lib::device_data_dir(),
        ),
    ]
}

/// The path as the filesystem sees it, with junctions and symlinks resolved.
///
/// A path that does not exist yet cannot be canonicalized, and that is ordinary here — a resource
/// directory is created on first use. The nearest existing ancestor is canonicalized instead and the
/// remainder appended, so a junction anywhere above the leaf is still followed.
fn settled(path: &Path) -> Result<PathBuf, String> {
    let mut remainder = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        match current.canonicalize() {
            Ok(resolved) => {
                let mut settled = resolved;
                for part in remainder.iter().rev() {
                    settled.push(part);
                }
                return Ok(settled);
            }
            Err(_) => {
                let Some(name) = current.file_name().map(|name| name.to_os_string()) else {
                    return Err(format!("{} cannot be resolved at all", path.display()));
                };
                remainder.push(name);
                if !current.pop() {
                    return Err(format!("{} cannot be resolved at all", path.display()));
                }
            }
        }
    }
}

/// Whether `path` is `root` or beneath it, **by path component**.
///
/// Not `starts_with` on the string: `C:\profile-other` starts with `C:\profile` and is not inside
/// it. Case-insensitively, because Windows paths are.
fn inside(root: &Path, path: &Path) -> bool {
    let key = |component: &Component| component.as_os_str().to_string_lossy().to_lowercase();
    let mut wanted = root.components().map(|part| key(&part));
    let mut found = path.components().map(|part| key(&part));
    loop {
        match (wanted.next(), found.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(left), Some(right)) if left == right => continue,
            _ => return false,
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--inside") {
        let root = args.next().unwrap_or_else(|| {
            eprintln!("usage: resolve-resources --inside <root>");
            std::process::exit(2);
        });
        std::process::exit(check(Path::new(&root)));
    }

    let quote = |path: &Path| {
        // Backslashes and quotes, so the output is JSON a shell can parse rather than nearly-JSON.
        path.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    };
    let all = resources();
    println!("{{");
    for (index, (name, _, path)) in all.iter().enumerate() {
        let comma = if index + 1 == all.len() { "" } else { "," };
        println!("  \"{name}\": \"{}\"{comma}", quote(path));
    }
    println!("}}");
}

/// Report every resource that resolves outside `root`, and answer with the process exit code.
fn check(root: &Path) -> i32 {
    let Ok(root) = settled(root) else {
        eprintln!("the root itself cannot be resolved: {}", root.display());
        return 2;
    };
    let mut escaped = Vec::new();
    for (name, override_name, path) in resources() {
        match settled(&path) {
            Ok(resolved) if inside(&root, &resolved) => {}
            Ok(resolved) => escaped.push(format!(
                "  {name} resolves to {}{}",
                resolved.display(),
                if override_name.is_empty() {
                    String::new()
                } else if std::env::var_os(override_name).is_some() {
                    format!(" (from {override_name})")
                } else {
                    String::new()
                }
            )),
            Err(why) => escaped.push(format!("  {name}: {why}")),
        }
    }
    if escaped.is_empty() {
        println!(
            "every resource resolves inside {} ({} checked)",
            root.display(),
            resources().len()
        );
        return 0;
    }
    eprintln!("resources outside {}:", root.display());
    for line in &escaped {
        eprintln!("{line}");
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The check this replaced was `StartsWith`, and this is the case it got wrong.
    #[test]
    fn a_sibling_with_a_shared_prefix_is_not_inside() {
        let root = Path::new(r"C:\temp\profile");
        assert!(inside(root, Path::new(r"C:\temp\profile\kanji")));
        assert!(inside(root, Path::new(r"C:\temp\profile")));
        assert!(
            !inside(root, Path::new(r"C:\temp\profile-other\kanji")),
            "a sibling sharing a prefix is a different directory"
        );
        assert!(!inside(root, Path::new(r"C:\temp")));
        assert!(!inside(root, Path::new(r"C:\elsewhere\profile\kanji")));
    }

    #[test]
    fn containment_ignores_case_because_windows_does() {
        assert!(inside(
            Path::new(r"C:\Temp\Profile"),
            Path::new(r"c:\temp\profile\kanji")
        ));
    }

    /// A directory that does not exist yet still resolves through its existing ancestors.
    #[test]
    fn a_path_that_does_not_exist_yet_resolves_through_what_does() {
        let root = std::env::temp_dir();
        let deep = root.join("sakatsu-resolve-check").join("not").join("here");
        let resolved = settled(&deep).expect("resolvable through its ancestors");
        let resolved_root = settled(&root).expect("the temp directory exists");
        assert!(
            inside(&resolved_root, &resolved),
            "{} should be inside {}",
            resolved.display(),
            resolved_root.display()
        );
        assert!(
            resolved.ends_with("here"),
            "and the part that does not exist yet is kept: {}",
            resolved.display()
        );
    }
}
