//! Safe builders for supplemental kanji and pronunciation data.
//!
//! These archives are fetched only after informed consent, but consent is not trust. Every
//! builder reads into a bounded staging area, accepts only the files and formats it owns, and
//! replaces the last working output only after the complete new set validates.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use encoding_rs::EUC_JP;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug)]
pub enum EnrichmentError {
    Io(std::io::Error),
    Archive(String),
    Malformed(String),
}

impl std::fmt::Display for EnrichmentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Archive(message) => write!(formatter, "archive could not be read: {message}"),
            Self::Malformed(message) => write!(formatter, "unexpected source data: {message}"),
        }
    }
}

impl std::error::Error for EnrichmentError {}

impl From<std::io::Error> for EnrichmentError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<zip::result::ZipError> for EnrichmentError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Archive(error.to_string())
    }
}

const MAX_ARCHIVE_ENTRIES: usize = 30_000;
const MAX_KANJIVG_FILE: u64 = 256 * 1024;
const MAX_KANJIVG_TOTAL: u64 = 256 * 1024 * 1024;
const MAX_AUDIO_FILE: u64 = 5 * 1024 * 1024;
const MAX_AUDIO_TOTAL: u64 = 2 * 1024 * 1024 * 1024;

/// Build the basic KRADFILE table and its reverse component index.
pub fn krad_tables(
    archive: &Path,
    forward: &Path,
    reverse: &Path,
) -> Result<usize, EnrichmentError> {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(File::open(archive)?).read_to_end(&mut bytes)?;
    let (decoded, had_errors) = EUC_JP.decode_without_bom_handling(&bytes);
    if had_errors {
        return Err(EnrichmentError::Malformed(
            "KRADFILE is not valid EUC-JP".into(),
        ));
    }
    let (by_character, by_component) = parse_krad(&decoded)?;
    if by_character.len() < 5_000 {
        return Err(EnrichmentError::Malformed(format!(
            "KRADFILE contains only {} characters",
            by_character.len()
        )));
    }

    let forward_lines: Vec<String> = by_character
        .iter()
        .map(|(character, components)| {
            format!("{character}\t{}", components.iter().collect::<String>())
        })
        .collect();
    let reverse_lines: Vec<String> = by_component
        .iter()
        .map(|(component, characters)| {
            format!("{component}\t{}", characters.iter().collect::<String>())
        })
        .collect();
    replace_pair(forward, &forward_lines, reverse, &reverse_lines)?;
    Ok(by_character.len())
}

type CharacterComponents = BTreeMap<char, Vec<char>>;
type ComponentCharacters = BTreeMap<char, BTreeSet<char>>;

fn parse_krad(text: &str) -> Result<(CharacterComponents, ComponentCharacters), EnrichmentError> {
    let mut by_character: CharacterComponents = BTreeMap::new();
    let mut by_component: ComponentCharacters = BTreeMap::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((literal, parts)) = line.split_once(" : ") else {
            return Err(EnrichmentError::Malformed(format!(
                "KRADFILE line {} has no separator",
                line_number + 1
            )));
        };
        let mut literal_chars = literal.chars();
        let Some(character) = literal_chars.next() else {
            continue;
        };
        if literal_chars.next().is_some() {
            return Err(EnrichmentError::Malformed(format!(
                "KRADFILE line {} has more than one character",
                line_number + 1
            )));
        }
        let mut components = Vec::new();
        for component in parts.split_whitespace().flat_map(str::chars) {
            if !component.is_whitespace() && !components.contains(&component) {
                components.push(component);
            }
        }
        if components.is_empty() {
            continue;
        }
        for component in &components {
            by_component
                .entry(*component)
                .or_default()
                .insert(character);
        }
        by_character.insert(character, components);
    }
    if by_character.is_empty() {
        return Err(EnrichmentError::Malformed(
            "KRADFILE contains no component rows".into(),
        ));
    }
    Ok((by_character, by_component))
}

/// Inconsistently drawn diagrams an import tolerates before it calls the archive broken. The
/// pinned release has one (參, 053c3); a few more in a future release is plausible, dozens is not.
pub const MAX_SKIPPED_DIAGRAMS: usize = 10;

/// Which diagrams an import skipped and why, one `stem<TAB>reason` line each, beside the
/// diagrams. Absent when none were skipped. Not a `.json`, so no lookup can mistake it for one.
pub const SKIPPED_DIAGRAMS: &str = "skipped.tsv";

/// Convert KanjiVG's release archive to one inert typed JSON file per character.
pub fn kanjivg_files(archive: &Path, destination: &Path) -> Result<usize, EnrichmentError> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        return Err(EnrichmentError::Archive("too many entries".into()));
    }
    let pending = staging_dir(destination)?;
    let mut outputs = HashSet::new();
    let mut total = 0u64;
    let mut written = 0usize;
    let mut skipped: Vec<String> = Vec::new();

    let outcome = (|| {
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let enclosed = entry.enclosed_name().ok_or_else(|| {
                EnrichmentError::Archive(format!(
                    "unsafe path: {}",
                    String::from_utf8_lossy(entry.name_raw())
                ))
            })?;
            let extension = enclosed.extension().and_then(|value| value.to_str());
            if extension != Some("svg") {
                return Err(EnrichmentError::Archive(format!(
                    "unexpected file: {}",
                    String::from_utf8_lossy(entry.name_raw())
                )));
            }
            if entry.size() > MAX_KANJIVG_FILE {
                return Err(EnrichmentError::Archive(format!(
                    "diagram is too large: {}",
                    String::from_utf8_lossy(entry.name_raw())
                )));
            }
            total = total.saturating_add(entry.size());
            if total > MAX_KANJIVG_TOTAL {
                return Err(EnrichmentError::Archive(
                    "expanded diagrams are too large".into(),
                ));
            }
            let stem = enclosed
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| EnrichmentError::Archive("diagram has no filename".into()))?;
            if !(5..=6).contains(&stem.len()) || !stem.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(EnrichmentError::Archive(format!(
                    "unexpected diagram name: {}",
                    String::from_utf8_lossy(entry.name_raw())
                )));
            }
            let name = format!("{}.json", stem.to_ascii_lowercase());
            if !outputs.insert(name.clone()) {
                return Err(EnrichmentError::Archive(format!(
                    "duplicate diagram output: {name}"
                )));
            }
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes)?;
            let diagram = match crate::dict::parse_diagram(&bytes) {
                Ok(diagram) => diagram,
                // One inconsistently drawn character costs that character its diagram, not the
                // archive its install; anything unsafe still refuses the archive.
                Err(error) if crate::dict::is_drawing_defect(&error) => {
                    skipped.push(format!("{stem}\t{error}"));
                    continue;
                }
                Err(error) => return Err(EnrichmentError::Malformed(error)),
            };
            let encoded = serde_json::to_vec(&diagram)
                .map_err(|error| EnrichmentError::Malformed(error.to_string()))?;
            fs::write(pending.join(name), encoded)?;
            written += 1;
        }
        if written < 5_000 {
            return Err(EnrichmentError::Malformed(format!(
                "KanjiVG contains only {written} diagrams"
            )));
        }
        // A handful of upstream drawing mistakes is a data defect; many is a broken archive.
        if skipped.len() > MAX_SKIPPED_DIAGRAMS {
            return Err(EnrichmentError::Malformed(format!(
                "{} KanjiVG diagrams are inconsistently drawn",
                skipped.len()
            )));
        }
        if !skipped.is_empty() {
            fs::write(pending.join(SKIPPED_DIAGRAMS), skipped.join("\n") + "\n")?;
        }
        Ok(())
    })();

    if let Err(error) = outcome {
        let _ = fs::remove_dir_all(&pending);
        return Err(error);
    }
    replace_directory(&pending, destination)?;
    Ok(written)
}

/// Extract the Tofugu open archive as an exact `(surface, normalised reading)` MP3 index.
pub fn audio_pack(archive: &Path, destination: &Path) -> Result<usize, EnrichmentError> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        return Err(EnrichmentError::Archive("too many entries".into()));
    }
    let pending = staging_dir(destination)?;
    let audio_dir = pending.join("mp3");
    fs::create_dir_all(&audio_dir)?;
    let mut index_lines = Vec::new();
    let mut keys = HashSet::new();
    let mut outputs = HashSet::new();
    let mut total = 0u64;
    let mut licence: Option<Vec<u8>> = None;

    let outcome = (|| {
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let enclosed = entry.enclosed_name().ok_or_else(|| {
                EnrichmentError::Archive(format!(
                    "unsafe path: {}",
                    String::from_utf8_lossy(entry.name_raw())
                ))
            })?;
            let normalised_path = enclosed.to_string_lossy().replace('\\', "/");
            let lower = normalised_path.to_ascii_lowercase();

            if lower.ends_with("/license") || lower.ends_with("/license.md") {
                if entry.size() > 128 * 1024 {
                    return Err(EnrichmentError::Archive("licence file is too large".into()));
                }
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes)?;
                licence = Some(bytes);
                continue;
            }
            if !lower.contains("/lib/mp3/") || !lower.ends_with(".mp3") {
                continue;
            }
            if entry.size() == 0 || entry.size() > MAX_AUDIO_FILE {
                return Err(EnrichmentError::Archive(format!(
                    "recording has an invalid size: {}",
                    String::from_utf8_lossy(entry.name_raw())
                )));
            }
            total = total.saturating_add(entry.size());
            if total > MAX_AUDIO_TOTAL {
                return Err(EnrichmentError::Archive(
                    "expanded audio pack is too large".into(),
                ));
            }

            let filename = enclosed
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    EnrichmentError::Archive("recording has no UTF-8 filename".into())
                })?;
            let (surface, reading) = audio_filename(filename)?;
            let key = (surface.clone(), reading.clone());
            if !keys.insert(key) {
                return Err(EnrichmentError::Archive(format!(
                    "duplicate pronunciation key: {surface} / {reading}"
                )));
            }
            let output = format!("{:06}.mp3", index_lines.len());
            if !outputs.insert(output.clone()) {
                return Err(EnrichmentError::Archive(format!(
                    "duplicate recording output: {output}"
                )));
            }
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes)?;
            if !looks_like_mp3(&bytes) {
                return Err(EnrichmentError::Malformed(format!(
                    "recording is not MP3 audio: {filename}"
                )));
            }
            fs::write(audio_dir.join(&output), bytes)?;
            index_lines.push(format!("{surface}\t{reading}\t{output}"));
        }
        if index_lines.is_empty() {
            return Err(EnrichmentError::Malformed(
                "pronunciation archive contains no MP3 recordings".into(),
            ));
        }
        let licence = licence.ok_or_else(|| {
            EnrichmentError::Malformed("pronunciation archive contains no licence".into())
        })?;
        index_lines.sort();
        write_lines(&pending.join("index.tsv"), &index_lines)?;
        fs::write(pending.join("LICENSE"), licence)?;
        fs::write(
            pending.join("meta.json"),
            br#"{"title":"Tofugu Japanese Vocabulary Pronunciation Audio","licence":"CC BY-SA 4.0","source":"https://github.com/tofugu/japanese-vocabulary-pronunciation-audio"}"#,
        )?;
        Ok(())
    })();

    if let Err(error) = outcome {
        let _ = fs::remove_dir_all(&pending);
        return Err(error);
    }
    let count = index_lines.len();
    // Windows will not rename a directory while an archive inside it is still held open.
    // Today the download staging area is separate, but dropping explicitly keeps publication
    // correct if those paths are ever co-located and releases the large central directory early.
    drop(zip);
    replace_directory(&pending, destination)?;
    Ok(count)
}

/// Split `【】` into the two, for either lenticular bracket.
///
/// **The black pair `【】` (U+3010/U+3011) is what the archive uses** - all 6,355 recordings in
/// it, with none of any other kind. This looked only for the white pair `〖〗` (U+3016/U+3017),
/// which no filename has ever carried, so the first entry read failed and the whole pack was
/// rejected as malformed: offline pronunciation had never once worked, by this route or by
/// `scripts/fetch-pronunciation.ps1`, which had the same character in its regex.
///
/// The white pair is still accepted. The two are near-indistinguishable at a glance - which is
/// how the wrong one survived being written down twice and tested once - and there is no
/// ambiguity in accepting both, because no filename contains either bracket in its word.
fn split_reading(stem: &str) -> Option<(&str, &str)> {
    [('【', '】'), ('〖', '〗')]
        .into_iter()
        .find_map(|(open, close)| {
            let (surface, reading) = stem.rsplit_once(open)?;
            Some((surface, reading.strip_suffix(close)?))
        })
}

fn audio_filename(filename: &str) -> Result<(String, String), EnrichmentError> {
    let stem = filename
        .strip_suffix(".mp3")
        .ok_or_else(|| EnrichmentError::Malformed("recording is not an MP3".into()))?;
    let (surface, reading) = split_reading(stem).ok_or_else(|| {
        EnrichmentError::Malformed(format!("recording filename has no reading: {filename}"))
    })?;
    let surface: String = surface.trim().nfc().collect();
    let reading: String = reading
        .trim()
        .nfc()
        .map(crate::kana::to_hiragana_char)
        .collect();
    if surface.is_empty()
        || reading.is_empty()
        || surface
            .chars()
            .any(|character| matches!(character, '\t' | '\n'))
        || reading
            .chars()
            .any(|character| matches!(character, '\t' | '\n'))
    {
        return Err(EnrichmentError::Malformed(format!(
            "recording filename has an empty or unsafe key: {filename}"
        )));
    }
    Ok((surface, reading))
}

fn looks_like_mp3(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3")
        || bytes
            .windows(2)
            .take(4096)
            .any(|pair| pair[0] == 0xff && pair[1] & 0xe0 == 0xe0)
}

fn staging_dir(destination: &Path) -> Result<PathBuf, EnrichmentError> {
    let parent = destination
        .parent()
        .ok_or_else(|| EnrichmentError::Malformed("output has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("data");
    let pending = parent.join(format!(".{name}.pending-{}", std::process::id()));
    if pending.exists() {
        fs::remove_dir_all(&pending)?;
    }
    fs::create_dir_all(&pending)?;
    Ok(pending)
}

fn replace_directory(pending: &Path, destination: &Path) -> Result<(), EnrichmentError> {
    let backup = destination.with_extension(format!("old-{}", std::process::id()));
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    if destination.exists() {
        fs::rename(destination, &backup)?;
    }
    if let Err(error) = fs::rename(pending, destination) {
        if backup.exists() {
            let _ = fs::rename(&backup, destination);
        }
        return Err(error.into());
    }
    if backup.exists() {
        fs::remove_dir_all(backup)?;
    }
    Ok(())
}

fn replace_pair(
    first: &Path,
    first_lines: &[String],
    second: &Path,
    second_lines: &[String],
) -> Result<(), EnrichmentError> {
    let suffix = std::process::id();
    let first_pending = first.with_extension(format!("pending-{suffix}"));
    let second_pending = second.with_extension(format!("pending-{suffix}"));
    let first_backup = first.with_extension(format!("old-{suffix}"));
    let second_backup = second.with_extension(format!("old-{suffix}"));
    for stale in [
        &first_pending,
        &second_pending,
        &first_backup,
        &second_backup,
    ] {
        if stale.exists() {
            fs::remove_file(stale)?;
        }
    }
    write_lines(&first_pending, first_lines)?;
    if let Err(error) = write_lines(&second_pending, second_lines) {
        let _ = fs::remove_file(&first_pending);
        return Err(error);
    }

    if first.exists() {
        fs::rename(first, &first_backup)?;
    }
    if second.exists() {
        if let Err(error) = fs::rename(second, &second_backup) {
            let _ = restore_file(&first_backup, first);
            let _ = fs::remove_file(&first_pending);
            let _ = fs::remove_file(&second_pending);
            return Err(error.into());
        }
    }

    let publish = (|| {
        fs::rename(&first_pending, first)?;
        fs::rename(&second_pending, second)?;
        Ok::<(), std::io::Error>(())
    })();
    if let Err(error) = publish {
        let _ = fs::remove_file(first);
        let _ = fs::remove_file(second);
        let _ = restore_file(&first_backup, first);
        let _ = restore_file(&second_backup, second);
        let _ = fs::remove_file(&first_pending);
        let _ = fs::remove_file(&second_pending);
        return Err(error.into());
    }
    if first_backup.exists() {
        fs::remove_file(first_backup)?;
    }
    if second_backup.exists() {
        fs::remove_file(second_backup)?;
    }
    Ok(())
}

fn restore_file(backup: &Path, destination: &Path) -> std::io::Result<()> {
    if backup.exists() {
        fs::rename(backup, destination)?;
    }
    Ok(())
}

fn write_lines(path: &Path, lines: &[String]) -> Result<(), EnrichmentError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = std::io::BufWriter::new(File::create(path)?);
    for line in lines {
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SCRATCH_NUMBER: AtomicUsize = AtomicUsize::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let number = SCRATCH_NUMBER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sakatsu-enrichment-{label}-{}-{number}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create scratch directory");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn zip_with(path: &Path, members: &[(&str, &[u8])]) {
        let file = File::create(path).expect("create zip");
        let mut writer = zip::ZipWriter::new(file);
        for (name, bytes) in members {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("start member");
            writer.write_all(bytes).expect("write member");
        }
        writer.finish().expect("finish zip");
    }

    #[test]
    fn krad_deduplicates_components_and_builds_the_reverse_index() {
        let (forward, reverse) = parse_krad("# header\n明 : 日 月 日\n朋 : 月 月\n").unwrap();
        assert_eq!(forward[&'明'].to_vec(), ['日', '月']);
        assert_eq!(
            reverse[&'月'].iter().copied().collect::<Vec<_>>(),
            ['明', '朋']
        );
    }

    /// **Taken from the archive, not written by hand.** The fixture this replaces used the white
    /// lenticular bracket and a katakana reading; the real archive uses the black bracket and is
    /// hiragana throughout, so the test passed against data that does not exist while every real
    /// recording failed. Names below were read out of
    /// `tofugu-pronunciation-master.zip`: `上手` (skilled), `水` (water), and `〜丁目` (the block
    /// counter) - which is the entry the reported error named, and the first one the importer
    /// ever reached.
    #[test]
    fn audio_filenames_come_from_the_archive_as_it_is_actually_named() {
        assert_eq!(
            audio_filename("上手【じょうず】.mp3").unwrap(),
            ("上手".into(), "じょうず".into())
        );
        assert_eq!(
            audio_filename("水【みず】.mp3").unwrap(),
            ("水".into(), "みず".into())
        );
        // A leading `~` is part of the word for a suffix entry, and must survive as written.
        assert_eq!(
            audio_filename("〜丁目【ちょうめ】.mp3").unwrap(),
            ("〜丁目".into(), "ちょうめ".into())
        );

        // The white bracket stays readable, so a rename upstream cannot cost the feature twice.
        assert_eq!(
            audio_filename("上手〖じょうず〗.mp3").unwrap(),
            ("上手".into(), "じょうず".into())
        );
        // Katakana readings do not appear in today's archive; the folding is kept for the day
        // one does, and asserted here so it cannot rot unnoticed.
        assert_eq!(
            audio_filename("上手【ジョウズ】.mp3").unwrap(),
            ("上手".into(), "じょうず".into())
        );

        assert!(audio_filename("上手.mp3").is_err());
    }

    #[test]
    fn mp3_validation_rejects_arbitrary_bytes() {
        assert!(looks_like_mp3(b"ID3\x04\0\0"));
        assert!(looks_like_mp3(&[0xff, 0xfb, 0x90, 0x64]));
        assert!(!looks_like_mp3(b"<html>not audio</html>"));
    }

    #[test]
    fn kanjivg_rejects_archive_traversal_without_touching_the_installed_generation() {
        let scratch = Scratch::new("kanjivg-traversal");
        let archive = scratch.0.join("source.zip");
        let destination = scratch.0.join("installed");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("old.json"), b"still here").unwrap();
        zip_with(&archive, &[("../04e00.svg", b"not allowed")]);

        assert!(kanjivg_files(&archive, &destination).is_err());
        assert_eq!(
            fs::read(destination.join("old.json")).unwrap(),
            b"still here"
        );
        assert!(!scratch.0.join("04e00.svg").exists());
    }

    #[test]
    fn kanjivg_rejects_duplicate_outputs_and_active_markup() {
        let scratch = Scratch::new("kanjivg-duplicates");
        let archive = scratch.0.join("duplicate.zip");
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><g><path d="M1 2L3 4"/><text x="1" y="2">1</text></g></svg>"#;
        zip_with(
            &archive,
            &[("kanji/04e00.svg", svg), ("other/04E00.svg", svg)],
        );
        assert!(kanjivg_files(&archive, &scratch.0.join("out")).is_err());

        let active = scratch.0.join("active.zip");
        zip_with(
            &active,
            &[(
                "kanji/04e00.svg",
                br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#,
            )],
        );
        assert!(kanjivg_files(&active, &scratch.0.join("out")).is_err());
    }

    #[test]
    fn pronunciation_pack_rejects_corrupt_and_duplicate_recordings_losslessly() {
        let scratch = Scratch::new("audio-validation");
        let destination = scratch.0.join("installed");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("index.tsv"), b"old generation").unwrap();

        let corrupt = scratch.0.join("corrupt.zip");
        zip_with(
            &corrupt,
            &[
                ("archive/LICENSE", b"CC BY-SA 4.0"),
                ("archive/lib/mp3/上手【じょうず】.mp3", b"not an mp3"),
            ],
        );
        assert!(audio_pack(&corrupt, &destination).is_err());
        assert_eq!(
            fs::read(destination.join("index.tsv")).unwrap(),
            b"old generation"
        );

        let duplicate = scratch.0.join("duplicate.zip");
        zip_with(
            &duplicate,
            &[
                ("archive/LICENSE", b"CC BY-SA 4.0"),
                ("archive/lib/mp3/上手【じょうず】.mp3", b"ID3\x04\0\0one"),
                ("archive/lib/mp3/上手【ジョウズ】.mp3", b"ID3\x04\0\0two"),
            ],
        );
        assert!(audio_pack(&duplicate, &destination).is_err());
        assert_eq!(
            fs::read(destination.join("index.tsv")).unwrap(),
            b"old generation"
        );
    }

    #[test]
    fn pronunciation_pack_publishes_only_indexed_mp3_and_licence_metadata() {
        let scratch = Scratch::new("audio-success");
        let archive = scratch.0.join("source.zip");
        let destination = scratch.0.join("installed");
        zip_with(
            &archive,
            &[
                ("archive/LICENSE", b"CC BY-SA 4.0"),
                (
                    "archive/lib/mp3/上手【じょうず】.mp3",
                    b"ID3\x04\0\0recording",
                ),
                ("archive/lib/ogg/上手【じょうず】.ogg", b"redundant"),
                ("archive/README.md", b"not retained"),
            ],
        );

        assert_eq!(audio_pack(&archive, &destination).unwrap(), 1);
        assert_eq!(
            fs::read_to_string(destination.join("index.tsv")).unwrap(),
            "上手\tじょうず\t000000.mp3\n"
        );
        assert!(destination.join("mp3/000000.mp3").is_file());
        assert!(destination.join("LICENSE").is_file());
        assert!(destination.join("meta.json").is_file());
        assert!(!destination.join("lib/ogg").exists());
        assert!(!destination.join("README.md").exists());
    }
}
