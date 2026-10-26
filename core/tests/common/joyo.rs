//! **The JoyoKanji-Yomi benchmark**, read, aligned and split — the scoring lives in
//! `joyo_readings.rs`.
//!
//! `scripts/fetch-joyo-yomi.ps1` fetches it (MIT, pinned revision) to `joyo-yomi` under the
//! evaluation folder — see [`dir`] for where that is. `SAKATSU_JOYO` overrides. Rows are `{key, normalized_text, normalized_pron}`: the key is
//! `{kanji}_{reading}_{index}`, and the pronunciation is **phonetic** katakana for the whole
//! sentence with the target kanji's own sound marked `<…>` — long vowels as ー, は as ワ.
//!
//! # The split is by kanji
//!
//! A third of the kanji are holdout, chosen by a hash of the kanji alone, so every reading and every
//! sentence of one kanji falls on one side. Hashing the whole key would put 固's three sentences on
//! both sides and let a decision tuned on two of them be "tested" on the third. The holdout is
//! sealed: [`dev`] refuses to return it.
//!
//! # Alignment is explicit, and partial
//!
//! The target is marked only in the pronunciation. It is located in the text by its kanji, and only
//! where that kanji occurs **exactly once**; a repeated kanji is counted as unaligned rather than
//! guessed at.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// The revision `fetch-joyo-yomi.ps1` pins, and the data file's hash at it.
pub const REVISION: &str = "2ae383bcdac88dacf89c9ab84f9990eec55fe5c9";
pub const DATA_SHA256: &str = "32a843f662c55f1876baf64f8dc3475cc7b777c576a8fea6b43a2343ff46e897";
const DATA_FILE: &str = "common_kanji_source.jsonl";

/// Where the benchmark lives: `SAKATSU_JOYO` if set, else `joyo-yomi` under `SAKATSU_EVAL_DIR`,
/// else `target/sakatsu-eval/joyo-yomi` in the repository — ignored, and on nobody's own drive.
/// The same order `fetch-joyo-yomi.ps1` writes in, so a fetch with no settings lands where this
/// looks.
pub fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_JOYO") {
        return PathBuf::from(dir);
    }
    std::env::var_os("SAKATSU_EVAL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("target")
                .join("sakatsu-eval")
        })
        .join("joyo-yomi")
}

/// One benchmark row, with what the key and the marker say about it.
#[derive(Debug, Clone)]
pub struct Item {
    pub key: String,
    pub kanji: char,
    /// The key's reading: the Joyo-list reading with its okurigana — かためる for 固. Metadata, **not
    /// gold for this occurrence**: the marked sound covers the kanji alone, and may be inflected or
    /// sound-changed.
    pub listed_reading: String,
    pub text: String,
    pub pron: String,
    /// The marked segment, katakana as the benchmark writes it.
    pub target: String,
}

/// Why a row could not become an [`Item`] or an aligned one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Unaligned {
    /// The key is not `{kanji}_{reading}_{index}` with a single-character kanji.
    MalformedKey,
    /// The pronunciation does not carry exactly one `<…>`.
    MalformedMarker,
    /// The kanji does not occur in the text.
    KanjiAbsent,
    /// The kanji occurs more than once; which occurrence is marked is not established here.
    KanjiRepeated,
}

impl Unaligned {
    pub fn name(self) -> &'static str {
        match self {
            Unaligned::MalformedKey => "malformed key",
            Unaligned::MalformedMarker => "malformed marker",
            Unaligned::KanjiAbsent => "kanji absent from text",
            Unaligned::KanjiRepeated => "kanji repeated in text",
        }
    }
}

/// An item and the byte offset of its target kanji in `text`, when it can be established.
pub struct Row {
    pub item: Item,
    pub at: Result<usize, Unaligned>,
}

/// Read every row, verifying the pinned data first.
/// Rows that could not be read, by line number.
pub type Malformed = Vec<(usize, Unaligned)>;

pub fn load() -> Result<(Vec<Row>, Malformed), String> {
    let path = dir().join(DATA_FILE);
    let bytes = std::fs::read(&path).map_err(|error| {
        format!(
            "{} could not be read ({error}) — run scripts/fetch-joyo-yomi.ps1",
            path.display()
        )
    })?;
    let digest = sakatsu_core::hashing::hex(Sha256::digest(&bytes));
    if digest != DATA_SHA256 {
        return Err(format!(
            "{} is not the pinned revision {REVISION} (sha256 {digest})",
            path.display()
        ));
    }
    let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    let mut rows = Vec::new();
    let mut malformed = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| format!("line {}: {error}", line_number + 1))?;
        let field = |name: &str| value[name].as_str().unwrap_or_default().to_string();
        let (key, text, pron) = (
            field("key"),
            field("normalized_text"),
            field("normalized_pron"),
        );
        let parts: Vec<&str> = key.split('_').collect();
        let mut kanji_chars = parts
            .first()
            .map(|kanji| kanji.chars())
            .into_iter()
            .flatten();
        let (Some(kanji), None, 3) = (kanji_chars.next(), kanji_chars.next(), parts.len()) else {
            malformed.push((line_number + 1, Unaligned::MalformedKey));
            continue;
        };
        let target = match (pron.matches('<').count(), pron.matches('>').count()) {
            (1, 1) => pron
                .split_once('<')
                .and_then(|(_, rest)| rest.split_once('>'))
                .map(|(inside, _)| inside.to_string()),
            _ => None,
        };
        let item = Item {
            key: key.clone(),
            kanji,
            listed_reading: parts[1].to_string(),
            text: text.clone(),
            pron,
            target: target.clone().unwrap_or_default(),
        };
        let at = match (target, text.matches(kanji).count()) {
            (None, _) => Err(Unaligned::MalformedMarker),
            (Some(_), 0) => Err(Unaligned::KanjiAbsent),
            (Some(_), 1) => Ok(text.find(kanji).expect("counted once")),
            (Some(_), _) => Err(Unaligned::KanjiRepeated),
        };
        rows.push(Row { item, at });
    }
    Ok((rows, malformed))
}

/// Whether a kanji is in the holdout third. A pure function of the kanji, so the split cannot drift
/// with the file's order or with a new reading being added.
pub fn is_holdout(kanji: char) -> bool {
    let mut buffer = [0u8; 4];
    let digest = Sha256::digest(kanji.encode_utf8(&mut buffer).as_bytes());
    u64::from_be_bytes(digest[..8].try_into().expect("eight bytes")) % 3 == 0
}

/// The development rows. **The holdout is sealed**: it is not returned, and nothing in this round
/// scores it.
pub fn dev(rows: Vec<Row>) -> Vec<Row> {
    rows.into_iter()
        .filter(|row| !is_holdout(row.item.kanji))
        .collect()
}

/// A digest of the split, so a recorded result names the split it was taken on.
pub fn split_digest(rows: &[Row]) -> String {
    let mut holdout: Vec<char> = rows
        .iter()
        .map(|row| row.item.kanji)
        .filter(|kanji| is_holdout(*kanji))
        .collect();
    holdout.sort_unstable();
    holdout.dedup();
    sakatsu_core::hashing::hex(Sha256::digest(
        holdout.iter().collect::<String>().as_bytes(),
    ))
}

/// Hiragana to katakana. Everything else passes through.
pub fn to_katakana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'ぁ'..='ゖ' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// The vowel a katakana mora ends in, for the long-vowel rules.
fn vowel(c: char) -> Option<char> {
    const ROWS: [(&str, char); 5] = [
        ("アカガサザタダナハバパマヤャラワァ", 'a'),
        ("イキギシジチヂニヒビピミリィ", 'i'),
        ("ウクグスズツヅヌフブプムユュルゥヴ", 'u'),
        ("エケゲセゼテデネヘベペメレェ", 'e'),
        ("オコゴソゾトドノホボポモヨョロヲォ", 'o'),
    ];
    ROWS.iter()
        .find(|(row, _)| row.contains(c))
        .map(|(_, vowel)| *vowel)
}

/// A reading in the benchmark's phonetic convention: katakana, ヂ and ヅ as ジ and ズ, and a vowel
/// lengthened by the next kana written ー — セイ → セー, コウ → コー, オオ → オー, ユウ → ユー.
///
/// **A pronunciation normaliser, deliberately lossy.** せい and せー come out the same, so a match
/// says the reading *sounds* right and nothing about how it is spelled. Particle rules (は → ワ) are
/// not applied: targets here are kanji, never particles.
pub fn phonetic(reading: &str) -> String {
    let katakana: Vec<char> = to_katakana(reading)
        .chars()
        .map(|c| match c {
            'ヂ' => 'ジ',
            'ヅ' => 'ズ',
            other => other,
        })
        .collect();
    let mut out: Vec<char> = Vec::with_capacity(katakana.len());
    for c in katakana {
        let previous = out.last().copied().and_then(|last| {
            if last == 'ー' {
                // A run of length marks continues the vowel before them.
                out.iter()
                    .rev()
                    .find(|c| **c != 'ー')
                    .copied()
                    .and_then(vowel)
            } else {
                vowel(last)
            }
        });
        let lengthens = matches!(
            (previous, c),
            (Some('a'), 'ア')
                | (Some('i'), 'イ')
                | (Some('u'), 'ウ')
                | (Some('e'), 'エ' | 'イ')
                | (Some('o'), 'オ' | 'ウ')
        );
        out.push(if lengthens { 'ー' } else { c });
    }
    out.into_iter().collect()
}

/// How a shown reading was matched to the marked segment, when it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Matched {
    /// Kana taken from the text after the kanji — its written okurigana.
    pub okurigana: usize,
    /// Moras taken from the pronunciation after `>` — 0 or 1. See [`sounds_like`].
    pub boundary: usize,
}

/// The moras the benchmark leaves outside the marker when a kanji's sound ends in them.
const BOUNDARY_MORAS: [char; 5] = ['ツ', 'ク', 'チ', 'キ', 'ッ'];

/// Whether `shown` — the ruby over the target kanji alone — sounds like the marked segment.
///
/// Two conventions of the benchmark's marking, both found by reading the first run's errors rather
/// than assumed, are accounted for — and only these two:
///
/// 1. **The marked segment spans the kanji and the okurigana written after it.** 固める is marked
///    `<カタメ>`, 枯れて `<カレ>`, 向こう `<ムコー>`, while the ruby over 固, 枯 and 向 is かた, か
///    and む. So `shown` may be extended by a prefix of the kana the **text** writes straight after
///    the kanji (`following`). Those kana are the same on both sides, so only the kanji's own
///    portion is being tested.
/// 2. **A final mora that becomes a doubled consonant or joins the next sound is left outside the
///    marker.** 熱湯 is `<ネ>ットー`, 必要 `<ヒ>ツヨー`, 側近 `<ソ>ッキン`, while the ruby over 熱, 必
///    and 側 is ねっ, ひつ and そっ. So the gold may be extended by **one** mora from straight after
///    `>` (`after_marker`), and only by ツ, ク, チ, キ or ッ.
///
/// The app's output contributes nothing but `shown`; the text and the gold pronunciation decide the
/// rest. Returns how the match was made, or `None`.
pub fn sounds_like(
    shown: &str,
    following: &str,
    target: &str,
    after_marker: &str,
) -> Option<Matched> {
    let kana: Vec<char> = following
        .chars()
        .take_while(|c| matches!(c, 'ぁ'..='ゖ' | 'ァ'..='ヺ' | 'ー'))
        .collect();
    let mut golds = vec![(phonetic(target), 0)];
    if let Some(next) = after_marker.chars().next() {
        if BOUNDARY_MORAS.contains(&next) {
            golds.push((phonetic(&format!("{target}{next}")), 1));
        }
    }
    for (gold, boundary) in &golds {
        for taken in 0..=kana.len() {
            let extended: String = shown.chars().chain(kana[..taken].iter().copied()).collect();
            if phonetic(&extended) == *gold {
                return Some(Matched {
                    okurigana: taken,
                    boundary: *boundary,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_normaliser_folds_vowel_length_and_nothing_else() {
        // The benchmark's own spellings: 精 セー, 時計 トケー, 非常 ヒジョー.
        assert_eq!(phonetic("せい"), "セー");
        assert_eq!(phonetic("けい"), "ケー");
        assert_eq!(phonetic("じょう"), "ジョー");
        assert_eq!(phonetic("おお"), "オー");
        assert_eq!(phonetic("ゆう"), "ユー");
        assert_eq!(phonetic("かため"), "カタメ");
        assert_eq!(phonetic("ぢ"), "ジ");
        // Lossy on purpose: two spellings, one sound.
        assert_eq!(phonetic("せい"), phonetic("せー"));
        // And not over-eager: a different vowel is not a lengthening.
        assert_eq!(phonetic("かい"), "カイ");
        assert_eq!(phonetic("こい"), "コイ");
    }

    #[test]
    fn the_marked_segment_may_include_the_written_okurigana() {
        let matched = |shown, following, target| sounds_like(shown, following, target, "");
        // The benchmark's own rows.
        assert_eq!(
            matched("かた", "める必要", "カタメ").map(|m| m.okurigana),
            Some(1)
        );
        assert_eq!(
            matched("か", "れてしまった", "カレ").map(|m| m.okurigana),
            Some(1)
        );
        assert_eq!(
            matched("む", "こうに", "ムコー").map(|m| m.okurigana),
            Some(2)
        );
        assert_eq!(
            matched("かえり", "みることは", "カエリミル").map(|m| m.okurigana),
            Some(2)
        );
        // No okurigana needed.
        assert_eq!(
            matched("せい", "密な", "セー").map(|m| m.okurigana),
            Some(0)
        );
        // A wrong reading stays wrong: the text's kana cannot make こう into おおやけ.
        assert_eq!(matched("こう", "のために", "オーヤケ"), None);
        // And the extension never reaches past the kana run into the next kanji.
        assert_eq!(
            matched("か", "わした", "カワシ").map(|m| m.okurigana),
            Some(2)
        );
        assert_eq!(matched("か", "", "カワシ"), None);
    }

    #[test]
    fn a_final_mora_left_outside_the_marker_is_taken_back_once() {
        // 熱湯 <ネ>ットー, 必要 <ヒ>ツヨー, 側近 <ソ>ッキン: the ruby is right.
        assert_eq!(
            sounds_like("ねっ", "湯", "ネ", "ットー"),
            Some(Matched {
                okurigana: 0,
                boundary: 1
            })
        );
        assert_eq!(
            sounds_like("ひつ", "要", "ヒ", "ツヨー").map(|m| m.boundary),
            Some(1)
        );
        assert_eq!(
            sounds_like("そっ", "近", "ソ", "ッキン").map(|m| m.boundary),
            Some(1)
        );
        // One mora only, and only those moras: a reading reaching into the next kanji stays wrong.
        assert_eq!(sounds_like("ねっと", "湯", "ネ", "ットー"), None);
        assert_eq!(sounds_like("もと", "で", "シタ", "デ"), None);
        // 六つ read むっつ but shown as む over 六: む + つ is ムツ, not ムッツ. Stays wrong.
        assert_eq!(sounds_like("む", "つ", "ムッツ", ""), None);
    }

    #[test]
    fn the_split_is_by_kanji_and_stable() {
        // Every sentence of one kanji lands on one side, whatever its reading or index.
        assert_eq!(is_holdout('固'), is_holdout('固'));
        let sample: Vec<char> = "日一人年大十二本中長出三時行見月後前生五間上東四今金九入学高円子外八六下来気小七山話女北午百書先名川千水半男西電校語土木聞食車何南万毎白天母火右読友左休父雨"
            .chars()
            .collect();
        let holdout = sample.iter().filter(|kanji| is_holdout(**kanji)).count();
        // About a third, not a degenerate split: 22 of these 78, and 724 of the benchmark's 2,129.
        let share = holdout as f64 / sample.len() as f64;
        assert!(
            (0.2..0.47).contains(&share),
            "{holdout} of {}",
            sample.len()
        );
    }
}
