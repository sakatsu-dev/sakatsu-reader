//! Turning what was downloaded into what the app reads.
//!
//! Two of the setup components do not arrive usable. KANJIDIC2 is 15 MB of gzipped XML and
//! the app wants a flat reading table; WordNet arrives as two archives from two projects
//! that have to be joined before either is any use.
//!
//! `scripts/fetch-kanjidic.ps1` and `scripts/fetch-wordnet.ps1` already do this work, and
//! they stay — they are the development path. But a learner handed the installer cannot
//! run them, so the same transformations live here too.
//!
//! **That is a second implementation of a format, which is a real cost.** It is paid
//! deliberately and checked rather than assumed: `tests/setup_build.rs` builds each table
//! both ways and compares them byte for byte, so the two cannot quietly diverge in the way
//! two copies of a parser otherwise would.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;

use flate2::read::GzDecoder;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

/// Why a table could not be built from what was downloaded.
#[derive(Debug)]
pub enum BuildError {
    Io(std::io::Error),
    /// The archive did not hold what it was supposed to.
    Malformed(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Io(error) => write!(f, "{error}"),
            BuildError::Malformed(what) => {
                write!(f, "the download is not what was expected: {what}")
            }
        }
    }
}

impl std::error::Error for BuildError {}

impl From<std::io::Error> for BuildError {
    fn from(error: std::io::Error) -> BuildError {
        BuildError::Io(error)
    }
}

// ---------------------------------------------------------------------------------------
// KANJIDIC2 → char <TAB> on <TAB> kun <TAB> meanings <TAB> strokes <TAB> grade <TAB> nanori
// ---------------------------------------------------------------------------------------

/// Read `kanjidic2.xml.gz` and write the reading table. Returns the character count.
///
/// Name-only readings are retained in their own field for Reference. They never enter the
/// reading matcher: 明 is read あす in names, and letting that candidate into alignment would
/// have 明日 "align" as あす + ひ, which is wrong.
///
/// Still discarded:
/// - **jlpt**, which is the pre-2010 four-level scale while the grammar packs all speak
///   N5–N1. Two scales wearing the same word is worse than one missing field.
/// - pinyin, korean, vietnamese, and every non-English meaning.
///
/// Readings are stored verbatim, because the period in `た.べる` separates stem from
/// okurigana and `dict::kanji::align` needs it.
pub fn kanji_table(archive: &Path, out: &Path) -> Result<usize, BuildError> {
    let mut xml = String::new();
    GzDecoder::new(File::open(archive)?).read_to_string(&mut xml)?;
    if !xml.contains("<kanjidic2") {
        return Err(BuildError::Malformed("not a KANJIDIC2 file".into()));
    }

    let mut reader = Reader::from_str(&xml);
    let mut buffer = Vec::new();
    let mut lines: Vec<String> = Vec::new();

    let mut literal = String::new();
    let mut on: Vec<String> = Vec::new();
    let mut kun: Vec<String> = Vec::new();
    let mut meanings: Vec<String> = Vec::new();
    let mut nanori: Vec<String> = Vec::new();
    let mut strokes = String::new();
    let mut grade = String::new();
    // Later stroke counts are miscounts the file records so a search for them still
    // finds the character. Only the first is the answer.
    let mut have_strokes = false;
    // Which field the text arriving now belongs to, and what has arrived so far.
    let mut want: Option<Field> = None;
    let mut pending = String::new();

    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) => {
                pending.clear();
                match element.name().as_ref() {
                    "character" => {
                        literal.clear();
                        on.clear();
                        kun.clear();
                        meanings.clear();
                        nanori.clear();
                        strokes.clear();
                        grade.clear();
                        have_strokes = false;
                    }
                    "literal" => want = Some(Field::Literal),
                    "stroke_count" => want = Some(Field::Strokes),
                    "grade" => want = Some(Field::Grade),
                    "nanori" => want = Some(Field::Nanori),
                    "reading" => {
                        want = match attribute(&element, "r_type").as_deref() {
                            Some("ja_on") => Some(Field::On),
                            Some("ja_kun") => Some(Field::Kun),
                            _ => None,
                        };
                    }
                    // English is the unmarked default; every other language names itself.
                    "meaning" => {
                        want = match attribute(&element, "m_lang") {
                            None => Some(Field::Meaning),
                            Some(_) => None,
                        };
                    }
                    _ => want = None,
                }
            }
            // Text arrives in pieces, so it is accumulated and committed at the end tag
            // rather than taken per event. `&amp;` is reported as its own `GeneralRef`,
            // which split "coming &amp; going" into two events — and taking each as a
            // finished value turned one meaning into two, writing `coming ; going` where
            // the script had `coming & going`. Twenty-two meanings really do contain one.
            Ok(Event::Text(text)) => {
                if want.is_some() {
                    pending.push_str(&text);
                }
            }
            Ok(Event::CData(text)) => {
                if want.is_some() {
                    pending.push_str(&text);
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if want.is_some() {
                    let name: &str = &reference;
                    match resolve(name) {
                        Some(character) => pending.push(character),
                        // KANJIDIC declares entities in its DTD but uses only the five
                        // predefined ones in content. Anything else is left as written
                        // rather than silently dropped.
                        None => pending.push_str(&format!("&{name};")),
                    }
                }
            }
            Ok(Event::End(element)) => {
                if let Some(field) = want.take() {
                    let value = std::mem::take(&mut pending);
                    match field {
                        Field::Literal => literal = value,
                        Field::On => on.push(value),
                        Field::Kun => kun.push(value),
                        Field::Meaning => meanings.push(value),
                        Field::Strokes => {
                            if !have_strokes {
                                strokes = value;
                                have_strokes = true;
                            }
                        }
                        Field::Grade => grade = value,
                        Field::Nanori => nanori.push(value),
                    }
                }
                if element.name().as_ref() == "character" {
                    // A character with nothing to say about it — JIS X 0212 fillers.
                    if literal.is_empty()
                        || (on.is_empty() && kun.is_empty() && meanings.is_empty())
                    {
                        continue;
                    }
                    lines.push(format!(
                        "{literal}\t{}\t{}\t{}\t{strokes}\t{grade}\t{}",
                        join(&on, ','),
                        join(&kun, ','),
                        join(&meanings, ';'),
                        join(&nanori, ','),
                    ));
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(BuildError::Malformed(error.to_string())),
            _ => {}
        }
    }

    lines.sort();
    write_lines(out, &lines)?;
    Ok(lines.len())
}

/// XML's five predefined entities. KANJIDIC's content uses only `&amp;`.
fn resolve(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => return None,
    })
}

#[derive(Clone, Copy)]
enum Field {
    Literal,
    On,
    Kun,
    Meaning,
    Strokes,
    Grade,
    Nanori,
}

fn attribute(element: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    element.attributes().flatten().find_map(|attribute| {
        (attribute.key.as_ref() == name).then(|| attribute.value.into_owned())
    })
}

/// Characters a value may not contain, because the file separates fields with them.
///
/// All three regardless of which one is joining here, matching the script exactly — a
/// value carrying any of them would be read back as two values, and losing one reading is
/// better than inventing one.
const SEPARATORS: [char; 3] = ['\t', ',', ';'];

fn join(values: &[String], separator: char) -> String {
    values
        .iter()
        .filter(|value| !value.contains(SEPARATORS))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(&separator.to_string())
}

// ---------------------------------------------------------------------------------------
// WordNet → word <TAB> category
// ---------------------------------------------------------------------------------------

/// The classes a particle sense can turn on. Everything else collapses away — this table
/// is for disambiguating particles, not for representing WordNet.
const KEEP: &[&str] = &[
    "noun.location",
    "noun.artifact",
    "noun.substance",
    "noun.time",
    "noun.quantity",
    "noun.event",
    "noun.state",
    "noun.phenomenon",
    "noun.person",
    "noun.group",
    "noun.food",
    "noun.animal",
    "noun.plant",
    "noun.body",
];

/// WordNet's own `lex_filenum` → name. Only the noun files; the numbers are stable
/// across releases.
const LEXNAMES: &[(u32, &str)] = &[
    (3, "noun.Tops"),
    (4, "noun.act"),
    (5, "noun.animal"),
    (6, "noun.artifact"),
    (7, "noun.attribute"),
    (8, "noun.body"),
    (9, "noun.cognition"),
    (10, "noun.communication"),
    (11, "noun.event"),
    (12, "noun.feeling"),
    (13, "noun.food"),
    (14, "noun.group"),
    (15, "noun.location"),
    (16, "noun.motive"),
    (17, "noun.object"),
    (18, "noun.person"),
    (19, "noun.phenomenon"),
    (20, "noun.plant"),
    (21, "noun.possession"),
    (22, "noun.process"),
    (23, "noun.quantity"),
    (24, "noun.relation"),
    (25, "noun.shape"),
    (26, "noun.state"),
    (27, "noun.substance"),
    (28, "noun.time"),
];

/// Join the Japanese and Princeton halves into the noun → category table.
///
/// The lexicographer file each synset belongs to *is* the coarse category wanted, so no
/// hypernym walk is needed — which matters, because the hierarchy ships only in
/// WordNet-ja's SQLite build and `project notes` §6 ruled a native database out.
///
/// A word in several synsets takes the commonest category, and **a tie is dropped rather
/// than guessed**: a wrong category produces a confidently wrong particle reading, which
/// is worse than the abstention it would replace.
pub fn wordnet_table(japanese: &Path, princeton: &Path, out: &Path) -> Result<usize, BuildError> {
    let by_number: HashMap<u32, &str> = LEXNAMES.iter().copied().collect();

    // synset offset → lexicographer file, from the Princeton archive's data.noun.
    let mut lexnames: HashMap<String, u32> = HashMap::new();
    let mut archive = tar::Archive::new(GzDecoder::new(File::open(princeton)?));
    let mut found = false;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().into_owned();
        if !path.ends_with("data.noun") {
            continue;
        }
        found = true;
        for line in BufReader::new(&mut entry).lines() {
            let line = line?;
            // The licence header's lines begin with two spaces.
            if line.starts_with("  ") {
                continue;
            }
            let mut parts = line.split(' ');
            let (Some(offset), Some(number)) = (parts.next(), parts.next()) else {
                continue;
            };
            if let Ok(number) = number.parse::<u32>() {
                lexnames.insert(offset.to_string(), number);
            }
        }
        break;
    }
    if !found {
        return Err(BuildError::Malformed(
            "no data.noun in the WordNet archive".into(),
        ));
    }

    // Japanese word → category, voting across the synsets it belongs to.
    let mut votes: HashMap<String, HashMap<&str, usize>> = HashMap::new();
    let reader = BufReader::new(GzDecoder::new(File::open(japanese)?));
    for line in reader.lines() {
        let line = line?;
        let mut parts = line.split('\t');
        let (Some(synset), Some(word)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(offset) = synset.strip_suffix("-n") else {
            continue; // nouns only
        };
        let Some(number) = lexnames.get(offset) else {
            continue;
        };
        let Some(name) = by_number.get(number) else {
            continue;
        };
        if !KEEP.contains(name) {
            continue;
        }
        *votes
            .entry(word.to_string())
            .or_default()
            .entry(name)
            .or_insert(0) += 1;
    }

    if votes.is_empty() {
        // `kanji_table` checks its input looks like KANJIDIC2; this had no equivalent, so
        // an unusable Japanese half produced `Ok(0)`, truncated `categories.tsv` to
        // nothing, and was reported to the learner as "0 words" with ok: true. A working
        // table was destroyed to get there.
        return Err(BuildError::Malformed(
            "no Japanese words matched a synset — the download is not the WordNet table".into(),
        ));
    }

    let mut lines: Vec<String> = Vec::new();
    for (word, counts) in votes {
        let mut ranked: Vec<(&str, usize)> = counts.into_iter().collect();
        ranked.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
        if ranked.len() > 1 && ranked[0].1 == ranked[1].1 {
            continue; // tied, and a coin-toss category makes a confidently wrong reading
        }
        lines.push(format!("{word}\t{}", ranked[0].0));
    }
    lines.sort();
    write_lines(out, &lines)?;
    Ok(lines.len())
}

fn write_lines(out: &Path, lines: &[String]) -> Result<(), BuildError> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::io::BufWriter::new(File::create(out)?);
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

    fn scratch(name: &str) -> std::path::PathBuf {
        // Keyed on the process id like every other scratch path here: two overlapping
        // `cargo test` runs share `temp_dir()`, and each one's cleanup deletes the
        // other's fixture mid-test. It surfaced as PermissionDenied from a different
        // test each run, which reads like a real fault and is not one.
        let dir = std::env::temp_dir().join(format!("sakatsu-build-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(bytes).expect("compress");
        encoder.finish().expect("finish")
    }

    /// A wrong download must not take a working table with it.
    #[test]
    fn a_wordnet_join_that_matches_nothing_is_refused_and_changes_nothing() {
        let dir = scratch("wordnet-empty");
        let out = dir.join("categories.tsv");
        std::fs::write(&out, "水\tnoun.substance\n").expect("seed a good table");

        // Well-formed gzip, right shape, but no synset the Princeton half knows — which
        // is what a moved or re-versioned upstream file looks like.
        let japanese = dir.join("wnjpn-ok.tab.gz");
        std::fs::write(&japanese, gzip(b"99999999-n\t\xe6\xb0\xb4\thand\n")).expect("write");

        let princeton = dir.join("WNdb-3.0.tar.gz");
        let mut tar = tar::Builder::new(Vec::new());
        let data = b"06294441 15 n 01 water 0 000\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, "dict/data.noun", &data[..])
            .expect("tar");
        std::fs::write(&princeton, gzip(&tar.into_inner().expect("finish"))).expect("write");

        let error = wordnet_table(&japanese, &princeton, &out).expect_err("should refuse");
        assert!(matches!(error, BuildError::Malformed(_)), "{error:?}");
        assert_eq!(
            std::fs::read_to_string(&out).expect("still there"),
            "水\tnoun.substance\n",
            "the table that was already there must survive a failed rebuild"
        );
    }

    #[test]
    fn a_download_that_is_not_kanjidic_is_refused() {
        let dir = scratch("kanji-wrong");
        let archive = dir.join("kanjidic2.xml.gz");
        std::fs::write(&archive, gzip(b"<html>404</html>")).expect("write");
        let error = kanji_table(&archive, &dir.join("kanjidic.tsv")).expect_err("should refuse");
        assert!(matches!(error, BuildError::Malformed(_)), "{error:?}");
        assert!(!dir.join("kanjidic.tsv").exists(), "nothing was written");
    }

    /// `&amp;` arrives as its own event; one meaning must not become two.
    #[test]
    fn an_escaped_ampersand_stays_inside_one_meaning() {
        let dir = scratch("kanji-amp");
        let xml = b"<?xml version=\"1.0\"?><kanjidic2><character><literal>\xe4\xba\xa4</literal>\
<misc><stroke_count>6</stroke_count><grade>2</grade></misc><reading_meaning><rmgroup>\
<reading r_type=\"ja_on\">\xe3\x82\xb3\xe3\x82\xa6</reading>\
<meaning>coming &amp; going</meaning><meaning>mingle</meaning>\
</rmgroup></reading_meaning></character></kanjidic2>";
        let archive = dir.join("k.gz");
        std::fs::write(&archive, gzip(xml)).expect("write");
        let table = dir.join("kanjidic.tsv");
        assert_eq!(kanji_table(&archive, &table).expect("builds"), 1);

        let line = std::fs::read_to_string(&table).expect("read");
        let meanings = line
            .trim_end()
            .split('\t')
            .nth(3)
            .expect("a meanings field");
        assert_eq!(
            meanings, "coming & going;mingle",
            "the entity is resolved in place, not split into another meaning"
        );
    }

    #[test]
    fn kanjidic_keeps_nanori_in_a_separate_final_field() {
        let dir = scratch("kanji-nanori");
        let xml = b"<?xml version=\"1.0\"?><kanjidic2><character><literal>\xe6\x98\x8e</literal>\
<misc><stroke_count>8</stroke_count><grade>2</grade></misc><reading_meaning><rmgroup>\
<reading r_type=\"ja_on\">\xe3\x83\xa1\xe3\x82\xa4</reading><meaning>bright</meaning>\
</rmgroup><nanori>\xe3\x81\x82\xe3\x81\x99</nanori></reading_meaning></character></kanjidic2>";
        let archive = dir.join("k.gz");
        std::fs::write(&archive, gzip(xml)).expect("write");
        let table = dir.join("kanjidic.tsv");
        kanji_table(&archive, &table).expect("builds");
        let fields: Vec<String> = std::fs::read_to_string(table)
            .unwrap()
            .trim_end()
            .split('\t')
            .map(str::to_string)
            .collect();
        assert_eq!(fields.len(), 7);
        assert_eq!(fields[6], "あす");
    }
}
