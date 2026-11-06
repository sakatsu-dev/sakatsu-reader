//! Validation shared by manual imports and publisher updates. A digest identifies bytes,
//! while publisher identity and the reader's consent determine where updates may come from.
use super::DictionaryMeta;
use flate2::read::GzDecoder;
use quick_xml::{events::Event, Reader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

pub const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
pub const MAX_XML: u64 = 128 * 1024 * 1024;
pub const MAX_METADATA: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Words,
    Kanjidic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ImportPreview {
    pub id: String,
    pub kind: ResourceKind,
    pub title: String,
    pub revision: String,
    pub sha256: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanjiSource {
    pub revision: String,
    pub sha256: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(default)]
pub struct DictionaryPreferences {
    pub check_updates: bool,
    pub automatic: bool,
    pub notify_updates: bool,
}

impl Default for DictionaryPreferences {
    fn default() -> Self {
        Self {
            check_updates: true,
            automatic: false,
            notify_updates: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ManagedDictionary {
    pub id: String,
    pub kind: ResourceKind,
    pub title: String,
    pub revision: String,
    pub generation: String,
    pub source: Option<String>,
    pub home: Option<String>,
    pub entries: Option<u32>,
    pub sha256: Option<String>,
    pub consented: bool,
    pub phase: String,
    pub detail: String,
    pub available_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct DictionaryOperation {
    pub id: String,
    pub target: String,
    pub phase: String,
    pub detail: String,
    pub bytes: u32,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct DictionaryManagerSnapshot {
    pub preferences: DictionaryPreferences,
    pub dismissed_updates: std::collections::BTreeMap<String, String>,
    pub dictionaries: Vec<ManagedDictionary>,
    pub operation: Option<DictionaryOperation>,
}

pub fn sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(crate::hashing::hex(hash.finalize()))
}

pub fn inspect_words(path: &Path) -> Result<DictionaryMeta, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > MAX_ARCHIVE {
        return Err("The dictionary archive exceeds 256 MiB.".into());
    }
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Not a supported dictionary ZIP: {e}"))?;
    if archive.len() > 20_000 {
        return Err("The dictionary has too many archive members.".into());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut total = 0u64;
    let mut has_terms = false;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.to_string();
        if entry.enclosed_name().is_none() || !names.insert(name.clone()) {
            return Err("The dictionary has unsafe or duplicate archive members.".into());
        }
        total = total.saturating_add(entry.size());
        if entry.size() > MAX_XML || total > 1024 * 1024 * 1024 {
            return Err("The expanded dictionary exceeds the import limit.".into());
        }
        has_terms |= name.starts_with("term_bank_") && name.ends_with(".json");
        let expected = entry.size();
        let actual = std::io::copy(&mut entry.by_ref().take(expected + 1), &mut std::io::sink())
            .map_err(|e| e.to_string())?;
        if actual != expected {
            return Err("An archive member did not match its declared size.".into());
        }
    }
    if !has_terms {
        return Err("This is not a supported word dictionary: no term banks were found.".into());
    }
    let mut index = archive
        .by_name("index.json")
        .map_err(|_| "The dictionary has no index.json.")?;
    if index.size() > MAX_METADATA {
        return Err("The dictionary metadata is too large.".into());
    }
    let mut json = Vec::new();
    index
        .by_ref()
        .take(MAX_METADATA + 1)
        .read_to_end(&mut json)
        .map_err(|e| e.to_string())?;
    let meta: DictionaryMeta = serde_json::from_slice(&json).map_err(|e| e.to_string())?;
    if meta.title.trim().is_empty()
        || meta.revision.trim().is_empty()
        || !(1..=3).contains(&meta.format)
    {
        return Err("The dictionary needs a title, revision and supported format (1–3).".into());
    }
    Ok(meta)
}

/// The header check can stop early during an update check. Full imports consume and parse
/// the entire bounded document before the existing table builder sees it. No entity resolver
/// is installed; a document cannot make the importer fetch its DTD or any external entity.
pub fn kanji_revision(input: impl Read, header_only: bool) -> Result<String, String> {
    let limit = if header_only { 256 * 1024 } else { MAX_XML };
    let mut xml = String::new();
    if header_only {
        let mut reader = std::io::BufReader::new(input.take(limit + 1));
        let mut bytes = Vec::new();
        use std::io::BufRead;
        loop {
            if reader
                .read_until(b'>', &mut bytes)
                .map_err(|e| e.to_string())?
                == 0
            {
                break;
            }
            if bytes.ends_with(b"</header>") {
                break;
            }
            if bytes.len() as u64 > limit {
                return Err("KANJIDIC2 header exceeds its size limit.".into());
            }
        }
        xml = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    } else {
        input
            .take(limit + 1)
            .read_to_string(&mut xml)
            .map_err(|e| e.to_string())?;
    }
    if xml.len() as u64 > limit {
        return Err("KANJIDIC2 exceeds its size limit.".into());
    }
    let mut reader = Reader::from_str(&xml);
    let mut root = false;
    let mut closed = false;
    let mut field = String::new();
    let mut version = String::new();
    let mut revision = String::new();
    let mut depth = 0usize;
    loop {
        match reader
            .read_event()
            .map_err(|e| format!("Invalid KANJIDIC2 XML: {e}"))?
        {
            Event::Start(e) => {
                if depth == 0 {
                    if root || e.name().as_ref() != "kanjidic2" {
                        return Err("Not a KANJIDIC2 document.".into());
                    }
                    root = true;
                }
                depth += 1;
                if depth > 64 {
                    return Err("KANJIDIC2 XML nesting exceeds its limit.".into());
                }
                field = e.name().as_ref().to_string();
            }
            Event::Text(e) => {
                if field == "file_version" {
                    version.push_str(e.as_ref());
                }
                if field == "date_of_creation" {
                    revision.push_str(e.as_ref());
                }
            }
            Event::End(e) => {
                depth = depth.checked_sub(1).ok_or("Invalid XML nesting.")?;
                if e.name().as_ref() == "kanjidic2" {
                    closed = true;
                }
                if header_only && e.name().as_ref() == "header" {
                    break;
                }
                field.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root
        || (!header_only && (!closed || depth != 0))
        || version.trim() != "4"
        || !is_date(revision.trim())
    {
        return Err(
            "KANJIDIC2 has an unsupported version, missing date or incomplete document.".into(),
        );
    }
    Ok(revision.trim().to_string())
}

pub fn kanji_gzip_header(input: impl Read) -> Result<String, String> {
    kanji_revision(GzDecoder::new(input.take(16 * 1024 * 1024)), true)
}

pub fn inspect_kanji(path: &Path) -> Result<String, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
        return Err("The KANJIDIC2 archive exceeds 16 MiB.".into());
    }
    kanji_revision(
        GzDecoder::new(std::fs::File::open(path).map_err(|e| e.to_string())?),
        false,
    )
}

pub fn is_date(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && ("01"..="12").contains(&&value[5..7])
        && ("01"..="31").contains(&&value[8..10])
}

/// Only the two documented publisher formats have an ordering. Custom revisions remain opaque.
pub fn revision_date(revision: &str) -> Option<String> {
    let value = revision.strip_prefix("JMnedict.").unwrap_or(revision);
    if value != revision && value.len() != 10 {
        return None;
    }
    if value == revision && !is_date(value) {
        let parts = value.split('.').collect::<Vec<_>>();
        if parts.len() != 4
            || !parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
        {
            return None;
        }
    }
    let date: String = value
        .chars()
        .take(10)
        .map(|c| if c == '.' { '-' } else { c })
        .collect();
    is_date(&date).then_some(date)
}

/// `None` is unchanged; `Some(false)` is an opaque or undated change requiring approval.
pub fn revision_change(previous: &str, offered: &str) -> Result<Option<bool>, String> {
    if offered.is_empty() {
        return Err("The publisher supplied no revision.".into());
    }
    if previous == offered {
        return Ok(None);
    }
    match (revision_date(previous), revision_date(offered)) {
        (Some(old), Some(new)) if new < old => {
            Err("The publisher offers an older revision. The installed version was kept.".into())
        }
        (Some(old), Some(new)) => Ok(Some(new > old)),
        _ => Ok(Some(false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn header_and_complete_validation_are_distinct() {
        let header = "<kanjidic2><header><file_version>4</file_version><date_of_creation>2026-10-18</date_of_creation></header>";
        assert_eq!(
            kanji_revision(header.as_bytes(), true).unwrap(),
            "2026-10-18"
        );
        assert!(kanji_revision(header.as_bytes(), false).is_err());
        assert!(kanji_revision(format!("{header}</kanjidic2>").as_bytes(), false).is_ok());
        assert!(kanji_revision(header.replace("kanjidic2", "other").as_bytes(), true).is_err());
        assert!(kanji_revision(header.replace(">4<", ">99<").as_bytes(), true).is_err());
    }
    #[test]
    fn only_known_date_revisions_can_be_ordered() {
        assert_eq!(revision_date("2026.10.03.0"), Some("2026-10-03".into()));
        assert_eq!(
            revision_date("JMnedict.2026-10-18"),
            Some("2026-10-18".into())
        );
        assert_eq!(revision_date("revision-99"), None);
        assert!(!is_date("2026-13-00"));
        assert_eq!(revision_date("2026-10-18-custom"), None);
        assert_eq!(
            revision_change("2026.10.18.0", "2026.10.18.0").unwrap(),
            None
        );
        assert!(revision_change("2026.10.18.0", "2026.10.03.0").is_err());
        assert_eq!(
            revision_change("2026.10.03.0", "2026.10.18.0").unwrap(),
            Some(true)
        );
        assert_eq!(
            revision_change("custom-9", "custom-10").unwrap(),
            Some(false)
        );
        assert_eq!(revision_change("", "2026-10-28").unwrap(), Some(false));
    }
}
