//! On-disk dictionary storage: import once, then read only the bytes a lookup needs.
//!
//! No server, and no native database to build or ship (`project notes` §6). A Yomitan
//! zip is imported into a directory of plain files:
//!
//! ```text
//! <dir>/meta.json          the dictionary's identity and its own attribution
//! <dir>/terms.ndjson       one entry per line, never loaded whole
//! <dir>/tags.json          the dictionary's own tag codes, if it ships any
//! <dir>/spelling/uXXXX.json   headword  → [[offset, length], …]
//! <dir>/reading/uXXXX.json    reading   → [[offset, length], …]
//! <dir>/gloss/ab.json         English word → [[offset, length], …], best first
//! ```
//!
//! ## Why spelling and reading are separate buckets
//!
//! Indexing both together drowns a lookup: が is read the same as 蛾, 我 and 雅, so a
//! single bucket returns a moth and an ego when the reader asked about a particle.
//! They are kept apart, spelling is preferred, and readings are consulted only when
//! nothing is spelled the way the query is (`project notes` §6).
//!
//! Shard files are named by the first character's code point (`u3042.json`) rather
//! than by the character, so the layout does not depend on the filesystem's opinion
//! about non-ASCII names.
//!
//! ## The gloss bucket, and what it can and cannot do
//!
//! `gloss/` is what lets a learner type "eat" and find 食べる. It maps a single English
//! **content word** to the entries whose glosses use it, and it is sharded on a
//! *two-character* prefix rather than one: Latin has 26 first letters against thousands
//! of kanji, and a one-character shard would be some forty times the size that makes the
//! rest of this design fast.
//!
//! **The order within a posting list is the ranking, decided once at build time** — see
//! [`GlossRank`]. A query reads only the first few entries rather than seeking through
//! hundreds, which is what keeps an English search as cheap as a Japanese one.
//!
//! What it cannot do is order true synonyms. Jitendex's `score` saturates: 22,553 entries
//! share the maximum, 食べる and 喫する and 食う among them, and the format carries no
//! frequency list and no priority tags to break that tie. So an English search answers
//! with *several words that mean this*, and the interface says so rather than presenting
//! the first as the answer.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::yomitan::{self, Entry};
use super::DictError;

/// Where an entry lives in `terms.ndjson`.
type Span = (u64, u64);

/// A shard: headword → the entries stored under it.
type Shard = HashMap<String, Vec<Span>>;

/// A dictionary's identity, as its own `index.json` states it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DictionaryMeta {
    pub title: String,
    #[serde(default)]
    pub revision: String,
    #[serde(default, rename = "isUpdatable")]
    pub is_updatable: bool,
    #[serde(default, rename = "indexUrl")]
    pub index_url: Option<String>,
    #[serde(default, rename = "downloadUrl")]
    pub download_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    /// Yomitan's format generation. Older dictionaries call this `version`.
    #[serde(default, alias = "version")]
    pub format: u32,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// The dictionary's own licence and credit line. Captured at import and kept
    /// beside the data, because data travels with its own licence (`project notes` §8) —
    /// dropping it would leave 屠つ unable to say where a definition came from.
    #[serde(default)]
    pub attribution: Option<String>,
    /// Entries actually imported. Added here, not read from `index.json`.
    #[serde(default)]
    pub entry_count: usize,
}

/// How a lookup found an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchKind {
    /// The query is spelled this way.
    Spelling,
    /// The query only matches this entry's reading — a homophone.
    Reading,
    /// The query matches only once katakana is folded to hiragana.
    ///
    /// Kept apart from [`MatchKind::Reading`] because it is a weaker claim: セイア found
    /// せいあ, which is how a name written in katakana reaches JMnedict's hiragana key —
    /// and equally how it reaches an unrelated rare word. A caller that ranks results
    /// needs to know the difference.
    Folded,
    /// The query is a productive する-verb and this entry is the noun it is built on.
    ///
    /// **Not a weaker claim like [`MatchKind::Folded`], and deliberately not treated as one.**
    /// JMdict files a productive する-verb under its bare noun with a `vs` tag: 請求する is not a
    /// term and 請求 is, and 勉強する, 確認する and 19,518 others are the same. The noun really is
    /// the dictionary's word for it, so a hit here is as written and is not caveated.
    SuruStem,
    /// The query is English, and this entry's glosses use it.
    Gloss,
    /// The query is one character, and this entry is written with it.
    Compound,
}

/// Words too common in English glosses to tell one entry from another.
///
/// Every verb gloss begins "to", and "of" appears in 41,061 entries — indexing either
/// would build a single shard holding most of the dictionary while distinguishing
/// nothing. Dropped from both the index and the query, so "to eat" and "eat" ask the
/// same question.
const GLOSS_STOPWORDS: &[&str] = &[
    "to", "a", "an", "the", "of", "in", "on", "for", "and", "or", "be", "is", "it", "with",
];

/// Definition tags that mark a sense as something other than the everyday word.
///
/// Taken from the dictionary's own tag vocabulary rather than invented — these are the
/// register and currency labels JMdict carries. A learner searching "eat" is better met
/// by 食べる than by 召し上がる (honorific) or 食う (vulgar), and this is the only signal
/// in the data that separates them.
///
/// `forms` is deliberately **not** here. It looks like a rarity marker and is not — it
/// sits on 猫, 犬 and 赤, and including it demoted precisely the common native words this
/// list exists to promote.
const MARKED_SENSE: &[&str] = &[
    "archaic",
    "rare",
    "colloquial",
    "historical",
    "slang",
    "honorific",
    "obsolete",
    "dated",
    "derogatory",
    "polite",
    "humble",
    "vulgar",
    "net slang",
    "poetical",
    "jocular",
    "formal",
    "familiar",
    "children's language",
    "abbr.",
    "proverb",
    "idiom",
    "yoji",
    "Kansai",
    "internet",
    "obscure",
];

/// How well an entry answers one English word. **Lower sorts first.**
///
/// Every field was arrived at by running the alternatives over the real 433,885-entry
/// dictionary and reading the answers, not by reasoning about what ought to work:
///
/// - `inexact` — whether the query is the whole gloss or one word inside a longer phrase.
///   "water" *is* 水's gloss; it is one word of 井戸's "water well".
/// - `marked` — see [`MARKED_SENSE`].
/// - `katakana` — a headword written wholly in katakana is usually a transliteration of
///   the learner's own English word. ブック and マウンテン and スクール otherwise beat 本,
///   山 and 学校 on every query, because JMdict's low sequence numbers are the katakana
///   block and the tiebreak was file order. Demoting them only reorders where a native
///   word competes: パン still leads "bread", because nothing neutral competes with it.
/// - `definition` / `gloss` — a word whose *first* sense is this beats one where it is
///   sense twelve. This is what puts 食べる above やる for "eat".
/// - `score` — last, and mostly inert: it saturates at 200 for 22,553 entries.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
struct GlossRank {
    inexact: u8,
    marked: u8,
    katakana: u8,
    definition: u32,
    gloss: u32,
    /// Negated, so that lower-sorts-first means higher-score-first.
    negated_score: f64,
}

/// One result.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LookupHit {
    pub entry: Entry,
    pub matched: MatchKind,
}

/// How a compound sorts under one of its characters.
///
/// The dictionary's own popularity score, negated so that lower-sorts-first means
/// most-common-first — the trick [`GlossRank`] uses for the same reason. `sequence` breaks ties
/// so two equally common entries keep the same order in every build; without it a rebuilt index
/// could reorder a kanji's compounds for no reason a reader could see.
#[derive(PartialEq, PartialOrd)]
struct KanjiRank {
    negated_score: f64,
    sequence: i64,
}

/// Accumulates, for each kanji, the entries whose spelling contains it.
///
/// **Ordered here rather than at query time**, which is the whole point: 行 appears in 1,369
/// entries and a reader wants six of them. Sorting at build time means a query reads the first
/// few postings of one shard and already has the common compounds, the same bargain
/// [`GlossBuilder`] strikes.
///
/// The order comes from `Entry::score`, which the dictionary supplies. That matters because the
/// frequency lists are an optional download: ranking on those would make "common compounds"
/// silently arbitrary on a plain install.
#[derive(Default)]
struct KanjiBuilder {
    postings: HashMap<String, Vec<(KanjiRank, Span)>>,
}

impl KanjiBuilder {
    /// Record this entry under each kanji its *spelling* uses.
    ///
    /// The spelling, never the reading: a compound is a way of writing something, and a kana
    /// reading shares no character with the kanji whose page it would appear on.
    fn add(&mut self, entry: &Entry, span: Span) {
        let mut seen: Vec<char> = Vec::new();
        for character in entry.term.chars().filter(|c| crate::kana::is_kanji(*c)) {
            // Distinct characters only, or 日々 would post twice under 日 and appear twice in
            // its own compound list.
            if seen.contains(&character) {
                continue;
            }
            seen.push(character);
            self.postings
                .entry(character.to_string())
                .or_default()
                .push((
                    KanjiRank {
                        negated_score: -entry.score,
                        sequence: entry.sequence,
                    },
                    span,
                ));
        }
    }

    fn finish(self) -> Shard {
        self.postings
            .into_iter()
            .map(|(character, mut postings)| {
                postings.sort_by(|left, right| {
                    left.0
                        .partial_cmp(&right.0)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                (
                    character,
                    postings.into_iter().map(|(_, span)| span).collect(),
                )
            })
            .collect()
    }
}

/// Accumulates the sequence bucket one entry at a time.
///
/// **What it is for.** JMdict files every spelling of a word as its own entry, tied to its
/// siblings only by a shared sequence number — 走る, 奔る and 疾る are three entries of sequence
/// 1406970. Every other bucket is keyed by something a reader can type, so a lookup of 走る can
/// reach 走る and nothing else, and the page could not say "also written 奔る" however much it
/// wanted to. This is the one bucket keyed by an identity rather than by a search term.
///
/// **The sign is dropped.** Jitendex writes a search-only form as the negation of the entry it
/// belongs to — アレ is `-1000580` against あれ's `1000580` — and those are the same word, which
/// is the rule `HitKey` already applies. Keying on the signed value would file them apart and
/// hide exactly the alternative spellings this exists to find.
///
/// Sequence `0` is skipped: `yomitan.rs` defaults a missing field to it, so it is not an
/// identity but the absence of one, and every such entry would collapse into a single posting
/// list holding the whole dictionary.
#[derive(Default)]
struct SequenceBuilder {
    postings: HashMap<String, Vec<(f64, Span)>>,
}

impl SequenceBuilder {
    fn add(&mut self, entry: &Entry, span: Span) {
        if entry.sequence == 0 {
            return;
        }
        self.postings
            .entry(entry.sequence.abs().to_string())
            .or_default()
            .push((entry.score, span));
    }

    /// Commonest spelling first, which is the order a reader wants them offered in.
    ///
    /// The same rule `one_per_entry` uses to pick which spelling leads a result — ついでに at 200
    /// ahead of 序に at -102 — applied once at build time so no caller has to re-sort.
    fn finish(self) -> Shard {
        self.postings
            .into_iter()
            .map(|(key, mut postings)| {
                postings.sort_by(|left, right| right.0.total_cmp(&left.0));
                (key, postings.into_iter().map(|(_, span)| span).collect())
            })
            .collect()
    }
}

/// Accumulates the gloss bucket one entry at a time.
///
/// Shared by the importer and by [`Dictionary::index_secondary`], so a dictionary indexed
/// during import and one backfilled afterwards cannot come out different — which is the
/// claim that lets an existing install skip re-importing 433,885 entries.
#[derive(Default)]
struct GlossBuilder {
    postings: HashMap<String, Vec<(GlossRank, Span)>>,
}

impl GlossBuilder {
    /// Record every English word this entry can be found by.
    ///
    /// One posting per word, keeping the *best* rank when a word appears in several of
    /// the entry's senses — an entry should be found once, at its strongest.
    fn add(&mut self, entry: &Entry, span: Span) {
        let mut best: HashMap<String, GlossRank> = HashMap::new();
        let katakana = u8::from(crate::kana::is_katakana_word(&entry.term));

        for (definition_index, definition) in entry.definitions.iter().enumerate() {
            let marked = u8::from(
                definition
                    .tags
                    .iter()
                    .any(|tag| MARKED_SENSE.contains(&tag.as_str())),
            );
            for (gloss_index, gloss) in definition.glosses.iter().enumerate() {
                // "X redirected from Y" is Jitendex's cross-reference text, not a
                // definition — 136,668 of them, and indexing them would make "redirected"
                // and "from" two of the commonest words in the dictionary.
                if gloss.contains(" redirected from ") {
                    continue;
                }
                let words = gloss_words(gloss);
                if words.is_empty() {
                    continue;
                }
                for word in &words {
                    let rank = GlossRank {
                        inexact: u8::from(words.len() > 1),
                        marked,
                        katakana,
                        definition: definition_index as u32,
                        gloss: gloss_index as u32,
                        negated_score: -entry.score,
                    };
                    match best.get(word) {
                        Some(kept) if *kept <= rank => {}
                        _ => {
                            best.insert(word.clone(), rank);
                        }
                    }
                }
            }
        }

        for (word, rank) in best {
            self.postings.entry(word).or_default().push((rank, span));
        }
    }

    /// Sort each posting list best-first and drop the ranks.
    ///
    /// The order *is* the ranking from here on: nothing downstream recomputes it, which
    /// is what lets a query read the first few entries instead of all of them.
    fn finish(self) -> Shard {
        self.postings
            .into_iter()
            .map(|(word, mut postings)| {
                postings.sort_by(|left, right| {
                    left.0
                        .partial_cmp(&right.0)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                (word, postings.into_iter().map(|(_, span)| span).collect())
            })
            .collect()
    }
}

/// The English content words a gloss can be found by.
///
/// Parenthetical text is dropped before splitting: "water (esp. cool or cold)" is a gloss
/// of *water* with a usage note, and counting the note's words would stop it being an
/// exact match. The stripping is depth-aware because the notes nest — 犬's gloss is
/// "dog (Canis (lupus) familiaris)", and a non-nesting strip leaves "familiaris" behind.
///
/// One-character words go too: they are "e" and "g" from *e.g.*, 8,376 entries' worth.
pub(super) fn gloss_words(gloss: &str) -> Vec<String> {
    let mut plain = String::with_capacity(gloss.len());
    let mut depth = 0usize;
    for character in gloss.chars() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => plain.push(character),
            _ => {}
        }
    }

    let mut words = Vec::new();
    for raw in plain.split(|c: char| !(c.is_ascii_alphanumeric() || c == '\'')) {
        let word = raw.trim_matches('\'').to_ascii_lowercase();
        if word.chars().count() < 2 || GLOSS_STOPWORDS.contains(&word.as_str()) {
            continue;
        }
        if !words.contains(&word) {
            words.push(word);
        }
    }
    words
}

/// An imported dictionary, ready to look words up in.
#[derive(Debug)]
pub struct Dictionary {
    /// The directory the application names — what a later publish writes into.
    dir: PathBuf,
    /// The generation this reader resolved, once. Immutable for its lifetime.
    root: PathBuf,
    /// Released on drop; reclamation skips any generation still held.
    _lease: GenerationLease,
    meta: DictionaryMeta,
    /// Shards are cached because a passage's words share first characters — looking
    /// up every word in a sentence would otherwise re-read the same shard again and
    /// again. Behind a `Mutex` so a `&self` lookup stays usable from shared state.
    shards: Mutex<HashMap<PathBuf, Arc<Shard>>>,
}

/// Publishing an imported dictionary briefly renames the previous generation aside.
/// Serialising that transition keeps an open or a second import from observing the gap.
static IMPORT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static IMPORT_GENERATION: AtomicU64 = AtomicU64::new(0);

fn import_lock() -> std::sync::MutexGuard<'static, ()> {
    IMPORT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn sibling(dir: &Path, suffix: &str) -> Result<PathBuf, DictError> {
    let parent = dir.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "dictionary destination has no parent directory",
        )
    })?;
    let name = dir.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "dictionary destination has no directory name",
        )
    })?;
    Ok(parent.join(format!(".{}.{suffix}", name.to_string_lossy())))
}

fn unique_sibling(dir: &Path, purpose: &str) -> Result<PathBuf, DictError> {
    loop {
        let generation = IMPORT_GENERATION.fetch_add(1, AtomicOrdering::Relaxed);
        let candidate = sibling(
            dir,
            &format!("{purpose}.{}.{generation}", std::process::id()),
        )?;
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
}

fn backup_path(dir: &Path) -> Result<PathBuf, DictError> {
    sibling(dir, "backup")
}

/// A process can stop in the tiny interval between moving the old generation aside and
/// publishing the new one. The predictable backup name makes the next open repair that
/// interruption instead of reporting that the learner's dictionary disappeared.
fn recover_interrupted_publish(dir: &Path) -> Result<(), DictError> {
    let backup = backup_path(dir)?;
    if !dir.exists() && backup.exists() {
        fs::rename(&backup, dir)?;
    } else if dir.exists() && backup.exists() {
        // The new generation was already published and only cleanup was interrupted.
        if let Err(error) = fs::remove_dir_all(&backup) {
            crate::log_warn!(
                "dictionary",
                "could not remove stale import backup {}: {error}",
                backup.display()
            );
        }
    }
    Ok(())
}

/// Generations a reader is still using, and must not lose underneath it.
///
/// **Retention by count was not ownership, and the difference showed up as a weakened test.** An
/// earlier version kept the newest two generations and accepted that a reader on an older one
/// might find its files gone — its test allowed an error *or* an empty result after reclamation.
/// That is the requirement inverted: a held reader is supposed to keep working until it is
/// released, and a `remove_dir_all` that happens to fail on Windows is not a substitute for
/// knowing whether anyone is reading.
///
/// So a `Dictionary` takes a lease on the generation it resolved, holds it for its whole life, and
/// releases it on drop. Reclamation removes only generations nobody holds.
///
/// Taking a lease and pruning both happen under `IMPORT_LOCK`, so a generation cannot be reclaimed
/// between being resolved and being leased. Releasing is not under the lock, which is harmless: a
/// lease released during a prune leaves the generation for the next one.
static LEASES: Mutex<BTreeMap<PathBuf, usize>> = Mutex::new(BTreeMap::new());

/// A reader's claim on one generation directory, released when the reader is dropped.
#[derive(Debug)]
struct GenerationLease(PathBuf);

impl GenerationLease {
    fn take(root: &Path) -> GenerationLease {
        let mut leases = LEASES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *leases.entry(root.to_path_buf()).or_insert(0) += 1;
        GenerationLease(root.to_path_buf())
    }
}

impl Drop for GenerationLease {
    fn drop(&mut self) {
        let mut leases = LEASES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = leases.get_mut(&self.0) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                leases.remove(&self.0);
            }
        }
    }
}

/// Whether any reader is still using this directory.
fn leased(root: &Path) -> bool {
    LEASES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains_key(root)
}

/// The file naming the live generation. One line, the directory name, nothing else.
const CURRENT: &str = "current";

/// Where a generation lives, and how its name sorts.
///
/// Zero-padded so lexical order is chronological order, which is what makes pruning a matter of
/// sorting rather than of parsing.
fn generation_name(number: u64) -> String {
    format!("g-{number:012}")
}

/// Every published generation in `dir`, oldest first.
fn generations(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("g-"))
        })
        .collect();
    found.sort();
    found
}

/// The generation a reader opened on `dir` should use.
///
/// **A path resolved once, which is the whole point.** `Dictionary` used to hold the directory the
/// *app* names, so a publish that renamed that directory changed what an already-open reader was
/// reading — while its cached byte offsets still described the release it opened on. Resolving to
/// an immutable generation directory here means `root` names bytes that cannot be rewritten.
///
/// A directory with no `current` file is a **legacy generation**: the dictionary is the directory
/// itself, exactly as it was before generations existed. Those installs keep working untouched and
/// are migrated by their next import rather than by a mandatory re-download.
fn resolve_generation(dir: &Path) -> PathBuf {
    let Ok(name) = fs::read_to_string(dir.join(CURRENT)) else {
        return dir.to_path_buf();
    };
    let name = name.trim();
    // A `current` naming something absent is a half-finished publish or a hand-edit. The legacy
    // layout is the safe reading: it is what is actually on disk.
    if name.is_empty() || name.contains(['/', '\\']) {
        return dir.to_path_buf();
    }
    let candidate = dir.join(name);
    if candidate.join("meta.json").exists() {
        candidate
    } else {
        dir.to_path_buf()
    }
}

/// Move a validated staging directory in as the new live generation.
///
/// **The old generation is never moved, so there is no interval where the dictionary is missing.**
/// The previous design renamed the live directory aside and the new one into its place, leaving a
/// window in which a concurrent reader found nothing at all — and needing
/// `recover_interrupted_publish` to repair a crash inside it. Here the new generation is built
/// under a name nothing is using, and the switch is one small atomic write of `current`. A crash
/// before that write leaves an orphan directory and a dictionary that still works.
/// What one publish spent, and how much it reclaimed.
struct Published {
    /// The generation now named by `current`.
    ///
    /// **Reported rather than read back.** `activate` used to re-read the pointer after the switch
    /// to learn this, which put a fallible call *between* switching and recording the recovery
    /// state — so a read failure returned an error for a container that had in fact activated, and
    /// the package publisher then rolled back every dictionary except that one. The name is
    /// already known here; handing it over removes the window entirely.
    name: String,
    /// The rename plus the pointer write. Constant work, whatever the dictionary weighs.
    publish: Duration,
    /// Removing whatever superseded generations nobody was holding.
    reclaim: Duration,
    /// How many generations that actually was. **Zero is the normal case in a running app**,
    /// because a reader holds the one it is using.
    reclaimed: usize,
}

/// Whether a publish reclaims what it supersedes, or leaves that to a later step.
///
/// An ordinary import is the whole operation, so it reclaims as its last leg. A publish inside a
/// package install is not: a later component can still fail, and the bytes this one supersedes are
/// what a roll-back would restore. See [`PreparedGeneration`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prune {
    Now,
    Defer,
}

fn publish_generation(staging: &Path, dir: &Path) -> Result<Published, DictError> {
    publish_generation_into(staging, dir, Prune::Now)
}

/// The number the next published generation will carry.
///
/// Shared with `PreparedGeneration::activate`, which has to name that generation in its recovery
/// record **before** the switch happens. Both run under `import_lock`, so this is the same answer
/// in both places rather than a prediction one of them might get wrong.
fn next_generation_number(dir: &Path) -> u64 {
    generations(dir)
        .last()
        .and_then(|path| path.file_name().and_then(|name| name.to_str()))
        .and_then(|name| name.strip_prefix("g-"))
        .and_then(|digits| digits.parse::<u64>().ok())
        .map_or(1, |number| number + 1)
}

fn publish_generation_into(
    staging: &Path,
    dir: &Path,
    prune: Prune,
) -> Result<Published, DictError> {
    let started = Instant::now();
    fs::create_dir_all(dir)?;
    let previous = generations(dir);
    let next = next_generation_number(dir);

    let published = dir.join(generation_name(next));
    fs::rename(staging, &published)?;

    // The switch. Written to a temporary and renamed, so a reader never sees a half-written name.
    let pointer = dir.join(CURRENT);
    let staged_pointer = dir.join(format!(".{CURRENT}.{}", std::process::id()));
    fs::write(&staged_pointer, generation_name(next).as_bytes())?;
    if let Err(error) = fs::rename(&staged_pointer, &pointer) {
        let _ = fs::remove_file(&staged_pointer);
        // The generation is complete but unreferenced. Leaving it costs disk; removing it and
        // failing to say so would cost the import.
        let _ = fs::remove_dir_all(&published);
        return Err(error.into());
    }

    let publish = started.elapsed();

    let started = Instant::now();
    let reclaimed = match prune {
        Prune::Now => prune_generations(dir, &published, !previous.is_empty()),
        // Deferred, not skipped: `PreparedGeneration::settle` runs the same pass once every
        // component has activated, and a roll-back before then needs these bytes.
        Prune::Defer => 0,
    };
    Ok(Published {
        name: generation_name(next),
        publish,
        reclaim: started.elapsed(),
        reclaimed,
    })
}

/// A complete generation, built and validated inside its live directory, that nothing points at
/// yet.
///
/// # Why this exists
///
/// [`Dictionary::import`] builds, validates, publishes and prunes in one call, which is right for
/// the ordinary case: one dictionary, replaced now. An **offline package** is a different shape —
/// several components that must all succeed or none of them — and the setup path implemented that
/// by copying the whole live dictionary directory, importing into the copy, and swapping the copy
/// in as the new live root.
///
/// That silently broke the guarantee generations exist for. Leases are held against the *original*
/// generation paths, so pruning inside the copy saw no lease on its copied old generation and
/// removed it; swapping the root then took the bytes a held reader was still resolving to. The
/// reader kept answering from its warm shard cache and returned **nothing** for any word it had
/// not already looked up — a silent wrong answer, which is the worst of the available failures.
/// `core/tests/offline_dictionary_publication.rs` is that reproduction.
///
/// So the live container path stays stable and the transaction is expressed here instead:
///
/// | Step | Atomic? |
/// |---|---|
/// | [`Dictionary::prepare`] | no, and it does not need to be — nothing points at the result, and a failure leaves the live generation untouched |
/// | [`PreparedGeneration::activate`] | **yes, per container** — one rename into `g-N` and one atomic write of `current` |
/// | [`PreparedGeneration::roll_back`] | yes, per container — one atomic write restoring the previous `current` |
/// | [`PreparedGeneration::settle`] | not atomic, and does not need to be — it only reclaims disk |
///
/// **There is no atomicity across containers, and this type does not pretend to offer one.** Two
/// dictionaries activated in sequence have an instant between them where one is new and the other
/// is old; both are internally consistent and both answer. What the type does guarantee is that
/// activation is *reversible* until [`settle`](Self::settle) is called, because the superseded
/// generation is still on disk and `current` is one small write away from naming it again.
pub struct PreparedGeneration {
    /// The live container. Never moved, never renamed.
    dir: PathBuf,
    /// The built, validated generation, waiting under a name nothing reads.
    staging: PathBuf,
    /// What to go back to, and the lease that keeps it reachable. `None` before activation.
    previous: Option<Superseded>,
    title: String,
    entry_count: usize,
}

/// The generation an activation displaced, held so a roll-back still has somewhere to go.
///
/// **The lease is the load-bearing part.** Deferring the prune keeps these bytes past the
/// activation, but it does not keep them past *somebody else's* publish: `prune_generations`
/// removes any generation that is not the newest and not leased, and a second publisher running
/// between activation and roll-back would happily reclaim exactly the generation being kept for
/// it. Taking a real [`GenerationLease`] means the existing lease-aware pruning protects the
/// roll-back target, rather than a second retention mechanism having to be invented beside it.
struct Superseded {
    /// What `current` said, or `None` for a container that had no pointer — the legacy layout,
    /// whose roll-back is to remove the pointer again.
    pointed_at: Option<String>,
    /// What this activation put there, so a roll-back can tell whether it is still the current
    /// one. Without it, undoing this transaction would silently overwrite whatever landed after —
    /// a lost update, and a worse outcome than the failure being recovered from.
    published: String,
    /// Which attempt this was, matching the on-disk record so a clear cannot remove somebody
    /// else's.
    transaction: String,
    /// Released by `settle` (so the bytes become reclaimable) or by `Drop`.
    _lease: GenerationLease,
}

impl PreparedGeneration {
    /// The dictionary's own title, available before anything points at it.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// How many entries it holds.
    pub fn entry_count(&self) -> usize {
        self.entry_count
    }

    /// Where the built generation is waiting, before activation moves it in.
    ///
    /// Exposed for tests that need to break a publication at the one point it can fail — there is
    /// no other way to make `activate` fail *without* switching, which is the property worth
    /// asserting.
    pub fn staging_path(&self) -> &Path {
        &self.staging
    }

    /// Point the container at this generation, keeping the superseded one on disk.
    ///
    /// **Nothing is pruned here.** The ordinary import reclaims unleased generations as its last
    /// step, and that is right when the import is the whole operation. Inside a package install it
    /// is not: a later component can still fail, and reclaiming the bytes this one supersedes
    /// would make that failure unrecoverable. [`settle`](Self::settle) does it once everything has
    /// activated.
    pub fn activate(&mut self) -> Result<(), DictError> {
        let _guard = import_lock();
        let pointer = self.dir.join(CURRENT);
        // **A read error is not an absent pointer.** `.ok()` conflated the two, so a container
        // whose `current` could not be read for any reason at all was treated as the legacy layout
        // — and a roll-back would then have *removed* a pointer that was in fact there.
        let pointed_at = match fs::read_to_string(&pointer) {
            Ok(name) => Some(name.trim().to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };

        // **One container, one record — so a second transaction may not start over an unresolved
        // one.** Checking identity when *clearing* cannot protect a record that has already been
        // overwritten: the second activation would replace the only description of what the first
        // did, and with it the protection on the generation the first still needs. Checked here,
        // under the same lock the switch happens under, so there is no window between looking and
        // writing.
        //
        // An unreadable record refuses too. It means a transaction was interrupted and its state
        // cannot be established, which is the case where guessing is least affordable.
        match RecoveryRecord::read(&self.dir) {
            Ok(None) => {}
            Ok(Some(held)) => {
                return Err(DictError::RecoveryOutstanding(format!(
                    "transaction {} left {} unresolved",
                    held.transaction,
                    self.dir.display()
                )))
            }
            Err(error) => {
                return Err(DictError::RecoveryOutstanding(format!(
                    "the recovery record for {} could not be read: {error}",
                    self.dir.display()
                )))
            }
        }

        let superseded = resolve_generation(&self.dir);
        let lease = GenerationLease::take(&superseded);

        // **Everything fallible happens before the switch.** The record is written first, so there
        // is no instant in which the container has moved on and nothing on disk says what it moved
        // on from; and nothing fallible runs between the switch and `self.previous` being set, so
        // an activation cannot report failure for a container that actually activated. That
        // ordering is the whole of the fix — the version that read the pointer back afterwards had
        // exactly that hole, and the package publisher would then have rolled back every
        // dictionary except the one that needed it most.
        let transaction = format!(
            "{}-{}",
            std::process::id(),
            IMPORT_GENERATION.fetch_add(1, AtomicOrdering::Relaxed)
        );
        let published_name = generation_name(next_generation_number(&self.dir));
        RecoveryRecord {
            transaction: transaction.clone(),
            previous: pointed_at.clone(),
            published: published_name.clone(),
            state: RecoveryState::Activating,
        }
        .write(&self.dir)?;

        let outcome = publish_generation_into(&self.staging, &self.dir, Prune::Defer);
        let published = match outcome {
            Ok(published) => published,
            Err(error) => {
                // Nothing switched, so the record describes a transaction that never happened.
                let _ = RecoveryRecord::clear(&self.dir);
                return Err(error);
            }
        };
        debug_assert_eq!(
            published.name, published_name,
            "the record must name the generation that was actually published"
        );

        self.previous = Some(Superseded {
            pointed_at,
            published: published.name,
            transaction,
            _lease: lease,
        });
        Ok(())
    }

    /// Record that a repair has already been tried on this container.
    ///
    /// `RollBackFailed` was declared and never written, which made it a promise the reports could
    /// not keep: a person looking at an outstanding record could not tell a transaction nobody had
    /// touched from one that had already resisted a repair. Best-effort — if the record cannot be
    /// updated the roll-back error is still what the caller gets, and losing the annotation must
    /// not replace it with a different failure.
    fn note_roll_back_failed(&self) {
        let Some(previous) = self.previous.as_ref() else {
            return;
        };
        if let Ok(Some(held)) = RecoveryRecord::read(&self.dir) {
            if held.transaction == previous.transaction {
                let _ = RecoveryRecord {
                    state: RecoveryState::RollBackFailed,
                    ..held
                }
                .write(&self.dir);
            }
        }
    }

    /// Remove this transaction's recovery record — **only if it is still this transaction's.**
    ///
    /// A later attempt overwrites the record with its own id, and clearing on the way out of an
    /// earlier one would then delete a live protection belonging to somebody else. Checked rather
    /// than assumed, because the whole value of the record is that it outlives its writer.
    fn clear_record(&self) -> Result<(), DictError> {
        let Some(previous) = self.previous.as_ref() else {
            return Ok(());
        };
        match RecoveryRecord::read(&self.dir)? {
            Some(held) if held.transaction == previous.transaction => {
                RecoveryRecord::clear(&self.dir)
            }
            // Somebody else's, or already gone.
            _ => Ok(()),
        }
    }

    /// Point the container back at what it named before [`activate`](Self::activate).
    ///
    /// Safe precisely because activation did not prune and holds a lease: the superseded
    /// generation is still on disk and no other publisher may reclaim it. Returns `Ok(())` when
    /// there was nothing to undo.
    ///
    /// **The recovery state survives a failed attempt.** This used to `take()` the saved pointer
    /// before doing any of the fallible work, so a restoration that failed left nothing to retry
    /// with — and the *next* call returned `Ok(())` having done nothing, reporting a roll-back
    /// that had not happened. The state is cleared only once the container actually names the old
    /// generation again, so a caller may retry and a failure stays a failure.
    ///
    /// Under the import lock, like activation: a publish landing between the write and the rename
    /// would otherwise decide what `current` says after this returns.
    /// Point the container back at what it named before [`activate`](Self::activate).
    ///
    /// Safe precisely because activation did not prune and holds a lease: the superseded
    /// generation is still on disk and no other publisher may reclaim it. Returns `Ok(())` when
    /// there was nothing to undo.
    ///
    /// **Any failure is written into the record before it is returned.** Which step failed —
    /// reading the pointer, writing the replacement, clearing the record afterwards — is not
    /// something the next person to look can work out from the container, and a repair that has
    /// already been tried once must not look untouched. Annotating in one place rather than at
    /// each `return` is also why it cannot be forgotten at a new one: the first version annotated
    /// two paths and missed the one the test actually took.
    pub fn roll_back(&mut self) -> Result<(), DictError> {
        let outcome = self.attempt_roll_back();
        if outcome.is_err() {
            self.note_roll_back_failed();
        }
        outcome
    }

    fn attempt_roll_back(&mut self) -> Result<(), DictError> {
        let Some(previous) = self.previous.as_ref() else {
            return Ok(());
        };
        let _guard = import_lock();
        let pointer = self.dir.join(CURRENT);

        // **Refuse rather than clobber.** Retaining the old bytes is not the same as owning the
        // container: another publisher may have landed a newer generation since this transaction
        // activated, and restoring the pointer would then quietly undo *their* work as well as
        // this one's. A lost update is a worse outcome than the failure being recovered from, and
        // it is invisible — the container still names a valid generation and still answers.
        //
        // The caller is told instead. The superseded generation is still on disk and still leased,
        // so nothing has been destroyed and a person can decide what should be live.
        let live = match fs::read_to_string(&pointer) {
            Ok(name) => Some(name.trim().to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        // **Already restored is not a conflict.** Restoration and record cleanup are two steps, and
        // the second can fail on its own — after which a retry arrives to find the pointer already
        // back where it wanted it. Reporting `Superseded` there would be telling the caller
        // somebody else had moved on, when in fact this transaction had succeeded and only its
        // bookkeeping was outstanding.
        if live == previous.pointed_at {
            self.clear_record()?;
            self.previous = None;
            return Ok(());
        }
        if live.as_deref() != Some(previous.published.as_str()) {
            return Err(DictError::Superseded);
        }

        match &previous.pointed_at {
            // The atomic write again, in the other direction.
            Some(name) => {
                let staged = self
                    .dir
                    .join(format!(".{CURRENT}.rollback.{}", std::process::id()));
                fs::write(&staged, name.as_bytes())?;
                if let Err(error) = fs::rename(&staged, &pointer) {
                    let _ = fs::remove_file(&staged);
                    return Err(error.into());
                }
            }
            // There was no pointer: this container was a legacy layout and must go back to being
            // one. The generation this published stays on disk and is reclaimed by a later publish.
            //
            // **The error is reported rather than swallowed.** A pointer that could not be removed
            // leaves the container naming a generation the caller believes it rolled back out of,
            // which is precisely the state a silent `let _ =` would have hidden. Absent is not a
            // failure: it means the removal already happened.
            None => match fs::remove_file(&pointer) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            },
        }
        // Restored, so the recovery point has served its purpose and stops being one. The
        // generation it named is the live one again; what protects it now is that it is current.
        self.clear_record()?;
        // Dropping the lease here is deliberate for the same reason.
        self.previous = None;
        Ok(())
    }

    /// Reclaim whatever this activation superseded, now that the whole operation has succeeded.
    ///
    /// Lease-aware exactly as an ordinary import's last leg is: a generation a reader still holds
    /// is left where it is and reclaimed by a later publish once that reader is dropped. **This is
    /// the point after which a roll-back is no longer possible**, which is why it is a separate
    /// call and not part of `activate`.
    pub fn settle(mut self) -> Result<usize, DictError> {
        let _guard = import_lock();
        // **Released before the prune, not after.** The lease and the recovery marker taken at
        // activation are what stopped another publisher reclaiming the roll-back target; leaving
        // either in place across this call would stop *this* prune reclaiming it too, deferring
        // the reclamation to some later publish that happens to find it unprotected. Dropping the
        // lease takes only the lease table, not this lock.
        // **Reported, not suppressed.** A record left behind by a *successful* transaction is not
        // harmless: it stops every later reclamation on this container and refuses the next
        // install, so a caller that is told the settle succeeded would be told the opposite of
        // what happened. The prune below is skipped, because the record it could not clear is
        // exactly what would protect the generations from it.
        self.clear_record()?;
        self.previous = None;
        let published = resolve_generation(&self.dir);
        let reclaimed = prune_generations(&self.dir, &published, true);
        // Activated, so `Drop` must not remove anything.
        self.staging = PathBuf::new();
        Ok(reclaimed)
    }
}

impl Drop for PreparedGeneration {
    /// A generation nobody activated is a directory nobody will ever read.
    ///
    /// Dropping without activating is the ordinary failure path — a later component failed
    /// validation, and this one never got its pointer — so cleaning up here means the caller does
    /// not have to remember to. After a successful `activate` the staging path no longer exists
    /// (it was renamed into the container), so this is a no-op.
    fn drop(&mut self) {
        if self.staging.as_os_str().is_empty() {
            return;
        }
        if self.staging.exists() {
            let _ = fs::remove_dir_all(&self.staging);
        }
    }
}

/// What a transaction has done to one container, written where a restart can read it.
///
/// # Why a record, and why *outside* the generation
///
/// The first attempt at surviving a dropped owner wrote a marker file **inside** the superseded
/// generation. That was wrong twice over, and the second reason is the one that matters:
///
/// - A generation is immutable. [`Dictionary::content_fingerprint`] hashes every file under it, so
///   a marker made the same pinned dictionary report a different identity while an install was in
///   flight — a resource whose bytes had not changed appearing to have changed.
/// - For a **legacy** layout the dictionary *is* the container directory, so there was nowhere
///   inside it to put anything at all without the same problem, and `activate` skipped writing the
///   marker in exactly that case — leaving legacy data with no persistent protection.
///
/// So the record is a sibling of the container, next to the staging directories, which nothing
/// hashes and nothing reads as dictionary content.
///
/// # What it carries
///
/// Enough to act on after a restart rather than merely to notice: which transaction, what the
/// container pointed at **before** — a generation name, or the legacy layout — what it points at
/// now, and how far the transaction got. A person or a repair routine reading this knows what to
/// restore and what to reclaim; a marker alone said only "something is unfinished".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RecoveryRecord {
    /// Distinguishes one attempt from the next, so a stale record cannot be mistaken for a live
    /// one and two attempts do not blur together in a report.
    pub transaction: String,
    /// What the container pointed at before activation. `None` is the legacy layout: no pointer,
    /// the directory itself was the dictionary.
    pub previous: Option<String>,
    /// The generation this transaction switched to.
    pub published: String,
    /// How far it got.
    pub state: RecoveryState,
}

/// What the record on disk *says*, as distinct from what happened.
///
/// **These are intents, not outcomes, and the difference used to be papered over.** The record is
/// written before the switch — it has to be, or an interruption between the two leaves a container
/// that has moved on with nothing saying what it moved on from — so a state called `Activated` was
/// a claim about the future. An interruption before the switch left a record asserting an
/// activation that never happened.
///
/// So the file says what was *being attempted*, and [`RecoveryRecord::observe`] reconciles it
/// against the pointer to say what actually holds. Nothing has to be written after the switch for
/// that to work, which is what keeps the switch the last fallible step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    /// A switch to `published` was begun. Whether it landed is decided by looking.
    Activating,
    /// A roll-back was attempted and failed. Written when that happens, so a repair that has
    /// already been tried once says so rather than looping silently.
    RollBackFailed,
}

/// What is actually outstanding on a container, after reconciling the record against the pointer.
///
/// This is what a startup report and a repair routine both need, and neither can get it from the
/// record alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outstanding {
    /// The record describes a switch that never landed: `current` still names what it did before.
    /// Nothing was changed, so nothing needs undoing — only the record needs clearing.
    NeverSwitched { record: RecoveryRecord },
    /// The switch landed and nothing has resolved it. The previous generation is retained.
    Unresolved { record: RecoveryRecord },
    /// The switch landed and something *newer* has since been published over it. Rolling back
    /// would discard that newer work, so this needs a person rather than a routine.
    ConflictingPublication {
        record: RecoveryRecord,
        live: Option<String>,
    },
    /// The container is already back where the record wanted it; only the record itself is left.
    CleanupOnly { record: RecoveryRecord },
}

impl Outstanding {
    /// The record this describes, whatever the shape.
    pub fn record(&self) -> &RecoveryRecord {
        match self {
            Outstanding::NeverSwitched { record }
            | Outstanding::Unresolved { record }
            | Outstanding::ConflictingPublication { record, .. }
            | Outstanding::CleanupOnly { record } => record,
        }
    }

    /// Whether resolving this needs a person to choose, rather than a routine to finish.
    pub fn needs_a_decision(&self) -> bool {
        matches!(self, Outstanding::ConflictingPublication { .. })
    }
}

impl RecoveryRecord {
    fn path(dir: &Path) -> Result<PathBuf, DictError> {
        sibling(dir, "recovery.json")
    }

    /// Read the outstanding record for a container, if any.
    ///
    /// **A malformed record is not "no record".** It means a transaction was interrupted and its
    /// state cannot be read, which is strictly more dangerous than a readable one — so it reports
    /// the error rather than letting a caller reclaim generations on the strength of an absence it
    /// did not really establish.
    pub fn read(dir: &Path) -> Result<Option<RecoveryRecord>, DictError> {
        let path = Self::path(dir)?;
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Write the record, atomically.
    ///
    /// Through a temporary and a rename, for the same reason `current` is: a reader arriving during
    /// the write must see the whole of the old record or the whole of the new one. A half-written
    /// record is an unreadable one, and an unreadable record stops every reclamation on the
    /// container until somebody resolves it — a heavy price for a torn write.
    fn write(&self, dir: &Path) -> Result<(), DictError> {
        let path = Self::path(dir)?;
        let staged = sibling(dir, &format!("recovery.{}.tmp", std::process::id()))?;
        fs::write(&staged, serde_json::to_string_pretty(self)?.as_bytes())?;
        if let Err(error) = fs::rename(&staged, &path) {
            let _ = fs::remove_file(&staged);
            return Err(error.into());
        }
        Ok(())
    }

    /// What is actually outstanding, by reading the record and the pointer together.
    ///
    /// The record says what was attempted; `current` says what happened. Only the pair answers the
    /// question a caller has — finish it, undo it, or ask somebody.
    pub fn observe(dir: &Path) -> Result<Option<Outstanding>, DictError> {
        let Some(record) = Self::read(dir)? else {
            return Ok(None);
        };
        let live = match fs::read_to_string(dir.join(CURRENT)) {
            Ok(name) => Some(name.trim().to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Some(if live == record.previous {
            // Back where it started. Either the switch never landed, or a roll-back restored it and
            // only the record is left; those are the same work — clear it — and the state says
            // which story it was.
            if record.state == RecoveryState::Activating && live.is_some() {
                Outstanding::CleanupOnly { record }
            } else {
                Outstanding::NeverSwitched { record }
            }
        } else if live.as_deref() == Some(record.published.as_str()) {
            Outstanding::Unresolved { record }
        } else {
            Outstanding::ConflictingPublication { record, live }
        }))
    }

    fn clear(dir: &Path) -> Result<(), DictError> {
        let path = Self::path(dir)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// The generation this record protects, as a path under `dir`, if it names one.
    fn protects(&self, dir: &Path) -> Option<PathBuf> {
        self.previous.as_ref().map(|name| dir.join(name))
    }
}

/// Resolve an outstanding transaction by **keeping what is live now**.
///
/// The container already points at the newer generation, so this changes nothing a reader sees; it
/// clears the record and lets the generations it was protecting be reclaimed by the next publish.
///
/// # It resolves a conflict too, and that is the point
///
/// This used to refuse when something newer had been published over the transaction's own
/// generation, on the reasoning that resolving a conflict is a person's decision. That was wrong in
/// the direction that leaves somebody stuck: refusing it left a state with no action that could
/// resolve it at all.
///
/// **What it costs, stated exactly.** The live selection is preserved — the newer installation goes
/// on being served, and nothing a reader sees changes. What is given up is the *protection* on the
/// older generations: once the record is cleared, a later publish may reclaim them. So this is not
/// "nothing is discarded", it is "nothing live is discarded, and the ability to go back is
/// relinquished". Whatever presents this action must say the second half.
///
/// [`restore_previous`] is the one that must refuse there, because it genuinely would discard the
/// newer work.
pub fn keep_current(dir: &Path, expected: &Identity) -> Result<(), DictError> {
    let _guard = import_lock();
    let Some(outstanding) = RecoveryRecord::observe(dir)? else {
        return Err(DictError::StaleRecovery(
            "there is nothing outstanding on this dictionary any more".into(),
        ));
    };
    expected.check(outstanding.record())?;
    // Every shape is resolvable this way, including a conflict: whatever is live stays live.
    RecoveryRecord::clear(dir)
}

/// Which transaction, and which publication, a caller believed it was resolving.
///
/// # Why an action carries this
///
/// A recovery screen is a picture of a moment. Between drawing it and pressing a button, the
/// transaction it described can be resolved by something else, superseded by a newer install, or
/// replaced by a second attempt — and the button would then resolve *that* instead, silently, on
/// the strength of a description the person never saw.
///
/// **Checked under the same lock as the mutation.** Reading the record, comparing it and clearing
/// it are one critical section; a check that released the lock first would be answering about a
/// state that could change before the write it authorises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The transaction the caller was looking at.
    pub transaction: String,
    /// The generation that transaction had published.
    pub published: String,
}

impl Identity {
    fn check(&self, record: &RecoveryRecord) -> Result<(), DictError> {
        if record.transaction != self.transaction {
            return Err(DictError::StaleRecovery(format!(
                "this describes transaction {}, but {} is outstanding now",
                self.transaction, record.transaction
            )));
        }
        if record.published != self.published {
            return Err(DictError::StaleRecovery(format!(
                "transaction {} published {}, not {}",
                record.transaction, record.published, self.published
            )));
        }
        Ok(())
    }
}

/// Resolve an outstanding transaction by **putting the previous generation back**.
///
/// Only from [`Outstanding::Unresolved`]: the switch landed, nothing newer has been published over
/// it, and the generation the record names is still on disk because the record is what kept it
/// there. Restoring is one atomic pointer write, the same one a live roll-back does.
///
/// Refused for a conflict, for the same reason [`keep_current`] is, and unnecessary for the other
/// two states — there is nothing to put back.
pub fn restore_previous(dir: &Path, expected: &Identity) -> Result<(), DictError> {
    let _guard = import_lock();
    let Some(outstanding) = RecoveryRecord::observe(dir)? else {
        return Err(DictError::StaleRecovery(
            "there is nothing outstanding on this dictionary any more".into(),
        ));
    };
    expected.check(outstanding.record())?;
    let record = match &outstanding {
        Outstanding::Unresolved { record } => record.clone(),
        Outstanding::ConflictingPublication { record, live } => {
            return Err(DictError::RecoveryOutstanding(format!(
                "transaction {} published {} but {} is live; restoring would discard it",
                record.transaction,
                record.published,
                live.as_deref().unwrap_or("a legacy layout")
            )))
        }
        // Already where it should be; only the record is left.
        Outstanding::NeverSwitched { .. } | Outstanding::CleanupOnly { .. } => {
            return RecoveryRecord::clear(dir)
        }
    };

    let pointer = dir.join(CURRENT);
    match &record.previous {
        Some(name) => {
            // **Checked before it is named.** The record is what kept these bytes from being
            // reclaimed, so they should be here — but a person may have deleted them by hand
            // between the failure and the repair, and pointing `current` at a generation that is
            // not there would turn a recoverable state into a broken one.
            if !dir.join(name).join("meta.json").is_file() {
                return Err(DictError::RecoveryTargetUnavailable(format!(
                    "the generation {name} this would restore is no longer on disk"
                )));
            }
            let staged = dir.join(format!(".{CURRENT}.restore.{}", std::process::id()));
            fs::write(&staged, name.as_bytes())?;
            if let Err(error) = fs::rename(&staged, &pointer) {
                let _ = fs::remove_file(&staged);
                return Err(error.into());
            }
        }
        // The container was a legacy layout and must go back to being one.
        None => match fs::remove_file(&pointer) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        },
    }
    RecoveryRecord::clear(dir)
}

/// Every container under `dir` with something outstanding, for a startup report.
///
/// **Errors are reported per container rather than failing the sweep.** A malformed record on one
/// dictionary must not stop the app noticing the other three, and it is itself the most important
/// thing to say — so it arrives as an entry with its reason, not as an absence.
pub fn outstanding_recoveries(dir: &Path) -> Vec<(PathBuf, Result<Outstanding, String>)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let container = entry.path();
        if !container.is_dir() {
            continue;
        }
        match RecoveryRecord::observe(&container) {
            Ok(Some(outstanding)) => found.push((container, Ok(outstanding))),
            Ok(None) => {}
            Err(error) => found.push((container, Err(error.to_string()))),
        }
    }
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

/// Remove generations no reader still holds.
///
/// **Ownership decides this, not a retention count.** Anything that is not the generation just
/// published and is not leased has no reader and no future reader — `resolve_generation` will
/// never return it again — so it is safe to remove. Anything leased is left exactly where it is,
/// however old, and reclaimed by a later publish once its reader is dropped.
///
/// A removal can still fail: on Windows a directory cannot be removed while a file inside it is
/// open, and a reader between resolving a shard and reading its bytes has one open. That is logged
/// and retried rather than fought, because the alternative is fighting the very reader this exists
/// to protect. It is a disk cost, never a correctness one — the reader that kept it alive holds a
/// lease, so it was never a deletion candidate in the first place.
fn prune_generations(dir: &Path, published: &Path, had_generations: bool) -> usize {
    let mut reclaimed = 0;
    // Read from disk, because the whole point is that it outlives the process that wrote it. A
    // record that cannot be *parsed* is treated as protecting everything: an unreadable record
    // means a transaction was interrupted and its state is unknown, which is strictly more
    // dangerous than a readable one, so nothing here reclaims on the strength of it.
    let record = RecoveryRecord::read(dir);
    let unreadable = record.is_err();
    let retained = record.ok().flatten().and_then(|held| held.protects(dir));
    if unreadable {
        crate::log_warn!(
            "dictionary",
            "{} has an unreadable recovery record; nothing is reclaimed until it is resolved",
            dir.display()
        );
        return 0;
    }
    for stale in generations(dir) {
        // `retained` outranks everything: it is the only one of the three that survives this
        // process, and it exists precisely for the case where the owner that would have held a
        // lease is gone.
        if stale == published || leased(&stale) || retained.as_deref() == Some(stale.as_path()) {
            continue;
        }
        reclaimed += 1;
        if let Err(error) = fs::remove_dir_all(&stale) {
            crate::log_warn!(
                "dictionary",
                "generation {} is unreferenced but could not be removed: {error}",
                stale.display()
            );
        }
    }

    // **Legacy files are reclaimed only once nothing is reading them.** A directory imported
    // before generations existed *is* its own generation, and a reader opened on it holds a lease
    // on the directory itself. The first publish leaves the files alone regardless; after that
    // they go as soon as the last legacy reader is dropped.
    // **The legacy files are protected by the record as well.** `previous: None` in a record is
    // exactly a legacy layout — the dictionary *is* this directory — and the earlier marker scheme
    // could not express that at all, so abandoning a legacy-to-generation transaction dropped its
    // only protection and the next publish deleted the bytes it was meant to recover from.
    let legacy_retained =
        matches!(RecoveryRecord::read(dir), Ok(Some(held)) if held.previous.is_none());
    if had_generations && !leased(dir) && !legacy_retained {
        for name in ["meta.json", "terms.ndjson", "tags.json"] {
            let _ = fs::remove_file(dir.join(name));
        }
        for bucket in ["spelling", "reading", "gloss", "kanji", "sequence"] {
            let _ = fs::remove_dir_all(dir.join(bucket));
        }
    }
    reclaimed
}

/// What one import spent, leg by leg.
///
/// The three are unrelated quantities and are reported unmixed: **build** is proportional to the
/// archive, **publish** is a rename plus a small pointer write whatever the dictionary weighs, and
/// **reclaim** is a `remove_dir_all` over however many superseded generations nobody holds — which
/// is usually none, because a running app has a reader on one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportLegs {
    /// Reading the archive and writing the index.
    pub build: Duration,
    /// Opening the staged generation to prove it answers before anything points at it.
    pub validate: Duration,
    /// The rename and the pointer write.
    pub publish: Duration,
    /// Removing superseded generations, if any were unleased.
    pub reclaim: Duration,
    /// How many were removed. Zero is the normal case while a reader is open.
    pub reclaimed: usize,
}

impl Dictionary {
    /// Import a Yomitan zip into `dir`, replacing anything already there only after the
    /// complete new generation can be opened.
    pub fn import(archive_path: &Path, dir: &Path) -> Result<Dictionary, DictError> {
        Self::import_timed(archive_path, dir).map(|(dictionary, _)| dictionary)
    }

    /// Build and validate a new generation for `dir` **without publishing it**.
    ///
    /// The same work [`import`](Self::import) does, stopping one step short: the generation is
    /// complete, opens, and carries its secondary indexes, but `current` still names the old one
    /// and no reader can reach the new bytes. [`PreparedGeneration::activate`] is the switch.
    ///
    /// **`dir` is the live container**, not a copy of it. That is the whole point — see
    /// [`PreparedGeneration`] for what copying it cost.
    ///
    /// The English gloss and kanji buckets are built here rather than after publication. The
    /// ordinary path imports, publishes, and then backfills through a second publish, which is two
    /// generations for one install; inside a package that would also mean a window where the new
    /// dictionary is live and cannot answer an English query. Building both into the generation
    /// that gets activated costs one extra staging directory of hard links.
    pub fn prepare(archive_path: &Path, dir: &Path) -> Result<PreparedGeneration, DictError> {
        let _guard = import_lock();
        recover_interrupted_publish(dir)?;

        let parent = dir.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "dictionary destination has no parent directory",
            )
        })?;
        fs::create_dir_all(parent)?;

        let built = unique_sibling(dir, "prepare")?;
        let outcome = (|| {
            Self::import_into(archive_path, &built)?;
            // Opening it *is* the validation, exactly as it is for an import: a generation that
            // cannot be opened never gets a name anything points at.
            let opened = Self::open_unlocked(&built)?;
            let title = opened.meta().title.clone();
            let entry_count = opened.meta().entry_count;

            let staging = if opened.needs_secondary_index() {
                let indexed = unique_sibling(dir, "prepare")?;
                let outcome = opened
                    .build_secondary_into(&indexed)
                    .and_then(|_| Self::open_unlocked(&indexed).map(|_| ()));
                if let Err(error) = outcome {
                    let _ = fs::remove_dir_all(&indexed);
                    return Err(error);
                }
                indexed
            } else {
                built.clone()
            };
            Ok(PreparedGeneration {
                dir: dir.to_path_buf(),
                staging,
                previous: None,
                title,
                entry_count,
            })
        })();

        match outcome {
            Ok(prepared) => {
                // The first staging directory has served its purpose whenever the indexed one
                // replaced it. Its bytes are hard-linked into the survivor, so this drops a link
                // rather than a dictionary.
                if prepared.staging != built {
                    let _ = fs::remove_dir_all(&built);
                }
                Ok(prepared)
            }
            Err(error) => {
                let _ = fs::remove_dir_all(&built);
                Err(error)
            }
        }
    }

    /// The same import, reporting what each leg cost.
    ///
    /// **The legs have to be timed, not inferred.** A benchmark once reported a reclamation cost
    /// by subtracting one whole import from another — which measured two full index builds plus
    /// however many generations the second publish happened to find unleased, and called the
    /// difference "reclamation". Building, publishing and reclaiming are three unrelated
    /// quantities: one is proportional to the archive, one is a rename and a small write, and one
    /// is a `remove_dir_all` over however many superseded generations nobody is holding.
    ///
    /// `reclaimed` is part of the report for that last reason. A publish that removed two
    /// generations and one that removed none are not comparable, and the count is the only thing
    /// that says which happened.
    pub fn import_timed(
        archive_path: &Path,
        dir: &Path,
    ) -> Result<(Dictionary, ImportLegs), DictError> {
        let _guard = import_lock();
        recover_interrupted_publish(dir)?;

        let parent = dir.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "dictionary destination has no parent directory",
            )
        })?;
        fs::create_dir_all(parent)?;
        let staging = unique_sibling(dir, "staging")?;

        // Built, validated, then published under a name nothing is using. `open_unlocked` on the
        // staging directory is the validation: a generation that cannot be opened is never named
        // by `current`, so a failed import leaves the previous one live and untouched.
        let outcome = (|| {
            let started = Instant::now();
            Self::import_into(archive_path, &staging)?;
            let build = started.elapsed();

            let started = Instant::now();
            Self::open_unlocked(&staging)?;
            let validate = started.elapsed();

            let published = publish_generation(&staging, dir)?;
            let dictionary = Self::open_unlocked(dir)?;
            Ok((
                dictionary,
                ImportLegs {
                    build,
                    validate,
                    publish: published.publish,
                    reclaim: published.reclaim,
                    reclaimed: published.reclaimed,
                },
            ))
        })();
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        outcome
    }

    /// Build one generation in an otherwise private directory. It must never point at
    /// the live directory: many files are mutually dependent through byte offsets.
    fn import_into(archive_path: &Path, dir: &Path) -> Result<(), DictError> {
        let file = File::open(archive_path)?;
        let mut archive = zip::ZipArchive::new(BufReader::new(file))?;

        let mut meta: DictionaryMeta = {
            let mut index = archive
                .by_name("index.json")
                .map_err(|_| DictError::NotADictionary)?;
            let mut json = String::new();
            index.read_to_string(&mut json)?;
            serde_json::from_str(&json)?
        };

        meta.source_sha256 =
            Some(super::managed::sha256(archive_path).map_err(DictError::BadFormat)?);

        // Sort the banks by their number so an import is reproducible: entry order
        // decides the order of equal-scoring results.
        let mut banks: Vec<(u32, String)> = archive
            .file_names()
            // zip 9 decodes names fallibly; one that is neither UTF-8 nor CP437 cannot be an
            // ASCII bank name, so skipping it is what matching it always did.
            .filter_map(Result::ok)
            .filter_map(|name| {
                bank_number(&name, "term_bank_").map(|number| (number, name.into_owned()))
            })
            .collect();
        banks.sort_unstable();

        fs::create_dir_all(dir)?;
        let mut spelling: Shard = HashMap::new();
        let mut reading: Shard = HashMap::new();
        let mut glosses = GlossBuilder::default();
        let mut sequences = SequenceBuilder::default();
        let mut kanji = KanjiBuilder::default();
        let mut count = 0usize;

        {
            let terms_path = dir.join("terms.ndjson");
            let mut terms = BufWriter::new(File::create(&terms_path)?);
            let mut offset = 0u64;

            for (_, name) in &banks {
                let json = read_member(&mut archive, name)?;
                for entry in yomitan::parse_term_bank(&json)? {
                    let line = serde_json::to_string(&entry)?;
                    let length = line.len() as u64;
                    terms.write_all(line.as_bytes())?;
                    terms.write_all(b"\n")?;

                    spelling
                        .entry(entry.term.clone())
                        .or_default()
                        .push((offset, length));
                    // Only index a reading separately when it differs, so an
                    // all-kana word is not returned twice for one query.
                    if entry.reading != entry.term {
                        reading
                            .entry(entry.reading.clone())
                            .or_default()
                            .push((offset, length));
                    }
                    glosses.add(&entry, (offset, length));
                    kanji.add(&entry, (offset, length));
                    sequences.add(&entry, (offset, length));

                    offset += length + 1; // the newline
                    count += 1;
                }
            }
            terms.flush()?;
        }

        write_shards(&dir.join("spelling"), &spelling, shard_name)?;
        write_shards(&dir.join("reading"), &reading, shard_name)?;
        write_shards(&dir.join("gloss"), &glosses.finish(), gloss_shard_name)?;
        write_shards(&dir.join("kanji"), &kanji.finish(), kanji_shard_name)?;
        write_shards(
            &dir.join("sequence"),
            &sequences.finish(),
            sequence_shard_name,
        )?;
        write_tags(&mut archive, dir)?;

        meta.entry_count = count;
        fs::write(dir.join("meta.json"), serde_json::to_vec_pretty(&meta)?)?;

        Ok(())
    }

    /// Whether `dir` holds a dictionary, in either layout.
    ///
    /// **Callers must not look for `meta.json` themselves.** Five places did, and every one of
    /// them silently stopped seeing a dictionary the moment it was published as a generation:
    /// discovery skipped it, the setup panel called the component uninstalled, and the backfill
    /// enumerated nothing. Where the files live is this type's business, and this is how to ask.
    pub fn exists_in(dir: &Path) -> bool {
        resolve_generation(dir).join("meta.json").is_file()
    }

    /// The dictionary directories under `dir`, in the order the application opens them.
    ///
    /// **Selection is part of the pipeline, not a detail beside it.** Sharing the processing code
    /// between the app and a measurement harness proves nothing if the two are handed different
    /// dictionaries, and they were: a harness transcribed this policy from a comment, filtered on
    /// a root-level `meta.json` — the pre-generation layout, so it omitted every published
    /// install — and scored a smaller corpus without saying so.
    ///
    /// The order matters because it decides which dictionary answers first: an ordinary dictionary
    /// before a name dictionary, then everything else by path, which is stable across runs.
    ///
    /// This returns paths rather than open readers **on purpose**. The app skips a broken index so
    /// the rest still start; a measurement run must fail instead of quietly scoring less. That is
    /// a real difference of policy, and it belongs at each call site where it can be read.
    pub fn discover(dir: &Path) -> Vec<PathBuf> {
        const PREFERRED: &[&str] = &["jitendex", "jmnedict"];

        let mut found: Vec<(usize, PathBuf)> = Vec::new();
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !Dictionary::exists_in(&path) {
                continue;
            }
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let rank = PREFERRED
                .iter()
                .position(|preferred| name.contains(preferred))
                .unwrap_or(PREFERRED.len());
            found.push((rank, path));
        }
        found.sort();
        found.into_iter().map(|(_, path)| path).collect()
    }

    /// The generation directory this reader resolved. Its files, not the directory the
    /// application names.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The name this dictionary is installed under, and the only stable identity it has.
    ///
    /// **A title is not one.** Two installed dictionaries may carry the same `index.json` title —
    /// two revisions of the same source, or two builds of it — and anything keyed on the title
    /// then merges them or picks whichever came first. `root` is no better: it names the
    /// *generation*, so it changes every time an index is published. The install directory is
    /// what the application chose, is unique within a store because it is a directory, and
    /// survives republication.
    pub fn installed_as(&self) -> String {
        self.dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.dir.display().to_string())
    }

    /// Open an already-imported dictionary.
    pub fn open(dir: &Path) -> Result<Dictionary, DictError> {
        let _guard = import_lock();
        recover_interrupted_publish(dir)?;
        Self::open_unlocked(dir)
    }

    fn open_unlocked(dir: &Path) -> Result<Dictionary, DictError> {
        // Resolved once, here, and never consulted again. Everything this reader answers with —
        // metadata, shard spans and the entry bytes those spans index — comes from this one
        // directory, which publication never rewrites.
        let root = resolve_generation(dir);
        let meta = fs::read_to_string(root.join("meta.json"))?;
        // Taken before the reader is handed out, under `IMPORT_LOCK`, so a generation cannot be
        // reclaimed between being resolved and being claimed.
        let lease = GenerationLease::take(&root);
        Ok(Dictionary {
            dir: dir.to_path_buf(),
            root,
            _lease: lease,
            meta: serde_json::from_str(&meta)?,
            shards: Mutex::new(HashMap::new()),
        })
    }

    pub fn meta(&self) -> &DictionaryMeta {
        &self.meta
    }

    /// Look a word up.
    ///
    /// Spelling wins: readings are consulted only when nothing is spelled the way
    /// `query` is, so が does not come back as 蛾. Results are ordered by Yomitan's
    /// score, most frequent first.
    pub fn lookup(&self, query: &str) -> Result<Vec<LookupHit>, DictError> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let spelled = self.hits(query, "spelling", MatchKind::Spelling)?;
        if !spelled.is_empty() {
            return Ok(spelled);
        }
        let read = self.hits(query, "reading", MatchKind::Reading)?;
        if !read.is_empty() {
            return Ok(read);
        }

        // **する is morphology, not spelling.** The analyzer reports a suru-verb's dictionary
        // form as `<noun>する` — `analyzer::lindera::compose_component` builds it that way — and
        // JMdict files those under the bare noun. So 請求させていただきます asked for 請求する,
        // found nothing, and showed a reader no definitions at all; 勉強します and 確認した did
        // the same. 19,520 entries are `vs`-tagged, 4.5% of the dictionary.
        //
        // Additive and last-resort like the fold below, so it cannot displace anything: a する
        // verb that is genuinely its own term — 愛する, 察する — matched exactly above and never
        // arrives here. It sits *before* the fold because it is the stronger claim of the two.
        if let Some(stem) = query.strip_suffix("する").filter(|stem| !stem.is_empty()) {
            let spelled = self.hits(stem, "spelling", MatchKind::SuruStem)?;
            if !spelled.is_empty() {
                return Ok(spelled);
            }
        }

        // Last resort, and only ever additive: both exact attempts found nothing, so a
        // folded hit can never displace one. This is what lets a name written in katakana
        // reach a dictionary that files it under hiragana — セイア is stored by JMnedict as
        // せいあ, and without folding the name the reader wants is unreachable while a rare
        // homophone the analyzer folded its own way is not.
        let folded = crate::kana::to_hiragana(query);
        if folded == query {
            return Ok(Vec::new());
        }
        let spelled = self.hits(&folded, "spelling", MatchKind::Folded)?;
        if !spelled.is_empty() {
            return Ok(spelled);
        }
        self.hits(&folded, "reading", MatchKind::Folded)
    }

    /// Every record **read** `reading`, whatever it is spelled — the reading index, asked directly.
    ///
    /// [`Dictionary::lookup`] is spelling-first on purpose: a reader who selects が should not be
    /// shown 蛾. That same rule makes it the wrong instrument for *enumerating candidates*: a kana
    /// query that matches one spelling never reaches the homophones behind it, so a caller asking
    /// "which words could this be" would see one and conclude there is only one. This asks the
    /// reading bucket and nothing else, so the caller that needs the competition can find it. The
    /// query is folded to hiragana, which is how readings are filed.
    pub fn lookup_reading(&self, reading: &str) -> Result<Vec<LookupHit>, DictError> {
        let reading = crate::kana::to_hiragana(reading.trim());
        if reading.is_empty() {
            return Ok(Vec::new());
        }
        self.hits(&reading, "reading", MatchKind::Reading)
    }

    fn hits(
        &self,
        query: &str,
        bucket: &str,
        matched: MatchKind,
    ) -> Result<Vec<LookupHit>, DictError> {
        let Some(path) = self.shard_path(query, bucket) else {
            return Ok(Vec::new());
        };
        let shard = self.shard(&path)?;
        let Some(spans) = shard.get(query) else {
            return Ok(Vec::new());
        };

        let mut terms = File::open(self.root.join("terms.ndjson"))?;
        let mut hits = Vec::with_capacity(spans.len());
        for &(offset, length) in spans {
            let mut buffer = vec![0u8; length as usize];
            terms.seek(SeekFrom::Start(offset))?;
            terms.read_exact(&mut buffer)?;
            let entry: Entry = serde_json::from_slice(&buffer)?;
            hits.push(LookupHit { entry, matched });
        }

        // Higher score first; ties keep import order, which is the banks' order.
        hits.sort_by(|left, right| {
            right
                .entry
                .score
                .partial_cmp(&left.entry.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(hits)
    }

    /// Identity of the bytes this reader is pinned to, independent of their install location.
    /// Evaluation calls this once per run, outside timed workloads; it reads the entire generation.
    pub fn content_fingerprint(&self) -> Result<String, DictError> {
        crate::content::fingerprint(&self.root).map_err(DictError::Io)
    }

    /// Whether English search is available.
    ///
    /// False for a dictionary imported before the gloss bucket existed. That is a
    /// supported state and not an error — but the caller must **say so**, because a
    /// search that silently returns nothing is indistinguishable from a word that is
    /// genuinely absent.
    pub fn has_glosses(&self) -> bool {
        self.root.join("gloss").is_dir()
    }

    /// Whether a character's compounds can be listed.
    ///
    /// Separate from [`Dictionary::has_glosses`] because the two buckets arrived in different
    /// versions: a dictionary imported after glosses but before this one has the first and not
    /// the second, and a caller that assumed they travel together would list no compounds while
    /// reporting that it could.
    pub fn has_kanji(&self) -> bool {
        self.root.join("kanji").is_dir()
    }

    /// Whether any secondary bucket is missing, and so whether a backfill is worth its wait.
    ///
    /// **One question, because the answer is one pass.** Asking `has_glosses` alone is how a
    /// dictionary that predates a later bucket gets skipped forever: it has the bucket the gate
    /// names, so nothing ever rebuilds the ones it lacks, and the feature reports itself
    /// available while returning nothing.
    pub fn needs_secondary_index(&self) -> bool {
        !self.has_glosses() || !self.has_kanji() || !self.has_sequence()
    }

    /// Whether a word's other spellings can be found.
    ///
    /// A third bucket with a third arrival version, and separate from the two above for the same
    /// reason they are separate from each other — see [`Dictionary::has_kanji`].
    pub fn has_sequence(&self) -> bool {
        self.root.join("sequence").is_dir()
    }

    /// Every entry sharing a dictionary sequence number: the same word, written its other ways.
    ///
    /// Ordered commonest-spelling-first by the builder, so `limit` takes the ones worth showing
    /// rather than an arbitrary few. Returns nothing — rather than failing — for a dictionary
    /// indexed before this bucket existed, which is what [`Dictionary::has_sequence`] is for.
    pub fn same_entry(&self, sequence: i64, limit: usize) -> Result<Vec<Entry>, DictError> {
        if sequence == 0 {
            return Ok(Vec::new());
        }
        let key = sequence.abs().to_string();
        let path = self.root.join("sequence").join(sequence_shard_name(&key));
        if !path.exists() {
            return Ok(Vec::new());
        }
        let shard = self.shard(&path)?;
        let Some(spans) = shard.get(&key) else {
            return Ok(Vec::new());
        };

        let mut terms = File::open(self.root.join("terms.ndjson"))?;
        let mut entries = Vec::new();
        for &(offset, length) in spans.iter().take(limit) {
            let mut buffer = vec![0u8; length as usize];
            terms.seek(SeekFrom::Start(offset))?;
            terms.read_exact(&mut buffer)?;
            entries.push(serde_json::from_slice(&buffer)?);
        }
        Ok(entries)
    }

    /// Build the secondary buckets for a dictionary imported without them, and publish the result
    /// as a complete generation.
    ///
    /// **The whole index is published at once, or not at all.** Writing the buckets into the live
    /// generation left two states a reader could see: `has_glosses` turns true the moment the
    /// directory exists, which is before its shards have all arrived, and a backfill that failed
    /// part-way left that partial index behind for good. Publishing a generation makes the switch
    /// one atomic write of `current` after every shard is on disk and the result has been opened.
    ///
    /// **Callers must refresh their readers.** This returns a new generation; a `Dictionary` held
    /// across it keeps its own, by design. `commands::build_gloss_index` re-runs
    /// `discover_dictionaries` for exactly this reason.
    ///
    /// Reads `terms.ndjson` rather than the original zip, so **nobody re-imports**: the
    /// spans are recoverable by re-scanning with the same offset arithmetic the importer
    /// used, and the entries are all there. Takes about as long as reading the file once.
    ///
    /// **Every secondary bucket in one pass**, and named for none of them. They are built
    /// together by the importer, so building them apart here is how a backfilled dictionary comes
    /// to differ from a freshly imported one — the hazard `GlossBuilder`'s own note already warns
    /// about. It also means one wait and one prompt: a reader asked to build an index three
    /// times, for features they did not ask about separately, is being asked to care about our
    /// storage layout.
    ///
    /// **Whoever calls this must gate on [`Dictionary::needs_secondary_index`], not on one
    /// bucket.** The gate used to read `has_glosses`, which meant a dictionary imported before
    /// the kanji bucket existed — having glosses already — was skipped forever and silently
    /// listed no compounds.
    pub fn index_secondary(&self) -> Result<usize, DictError> {
        let _guard = import_lock();
        // **Holding the lock is not the same as being current.** This reader resolved its
        // generation when it was opened, and an import may have landed since: the lock serialises
        // publication, it does not make a pinned root fresh. Building from a superseded generation
        // and publishing the result would reinstate the older dictionary as the newest one — the
        // sequence being open A, import B, then backfill through the A reader.
        //
        // Rejected rather than silently retargeted: the caller opened this reader and is entitled
        // to know its request described a dictionary that has since been replaced.
        // `commands::build_gloss_index` opens fresh readers immediately before asking, so this is
        // the narrow race rather than the ordinary path.
        if resolve_generation(&self.dir) != self.root {
            return Err(DictError::Superseded);
        }
        // Built beside the dictionary, never inside the generation being read. See the note above
        // about publishing the whole index at once.
        let staging = unique_sibling(&self.dir, "backfill")?;
        let outcome = self.build_secondary_into(&staging);
        let published = outcome.and_then(|count| {
            Self::open_unlocked(&staging)?;
            publish_generation(&staging, &self.dir)?;
            Ok(count)
        });
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        published
    }

    /// Assemble a complete generation in `staging`: the parts that do not change, then the
    /// secondary buckets built fresh.
    fn build_secondary_into(&self, staging: &Path) -> Result<usize, DictError> {
        fs::create_dir_all(staging)?;
        // The immutable half. Linked where the filesystem allows it — `terms.ndjson` is about
        // 143 MB for Jitendex and copying it to add three index buckets would be absurd — and
        // copied where it does not. A hard link is safe precisely because a generation is never
        // rewritten: reclaiming one directory drops a link, and the other keeps the bytes.
        for name in ["meta.json", "terms.ndjson", "tags.json"] {
            let from = self.root.join(name);
            if from.exists() {
                link_or_copy(&from, &staging.join(name))?;
            }
        }
        for bucket in ["spelling", "reading"] {
            let from = self.root.join(bucket);
            if from.exists() {
                link_or_copy_tree(&from, &staging.join(bucket))?;
            }
        }

        let mut reader = BufReader::new(File::open(self.root.join("terms.ndjson"))?);
        let mut kanji = KanjiBuilder::default();
        let mut glosses = GlossBuilder::default();
        let mut sequences = SequenceBuilder::default();
        let mut offset = 0u64;
        let mut line = String::new();
        let mut count = 0usize;

        loop {
            line.clear();
            let read = reader.read_line(&mut line)?;
            if read == 0 {
                break;
            }
            // `read` counts the newline the importer wrote; the span does not.
            let json = line.trim_end_matches('\n');
            if !json.trim().is_empty() {
                let entry: Entry = serde_json::from_str(json)?;
                glosses.add(&entry, (offset, json.len() as u64));
                kanji.add(&entry, (offset, json.len() as u64));
                sequences.add(&entry, (offset, json.len() as u64));
                count += 1;
            }
            offset += read as u64;
        }

        write_shards(&staging.join("gloss"), &glosses.finish(), gloss_shard_name)?;
        write_shards(&staging.join("kanji"), &kanji.finish(), kanji_shard_name)?;
        write_shards(
            &staging.join("sequence"),
            &sequences.finish(),
            sequence_shard_name,
        )?;
        Ok(count)
    }

    /// Entries whose glosses use every word of an English query, best first.
    ///
    /// Only the first `limit` entries of the most selective posting list are read from
    /// disk. Their stored order is the ranking (see [`GlossRank`]), so this does no
    /// scoring of its own — which is the point, since the alternative is seeking through
    /// hundreds of entries to rank them at query time.
    pub fn search_glosses(&self, query: &str, limit: usize) -> Result<Vec<LookupHit>, DictError> {
        let mut words = gloss_words(query);
        if words.is_empty() {
            // Every word was a stopword. Asking about "the" is a fair question to answer
            // badly, and a worse one to answer with silence.
            words = query
                .split_whitespace()
                .map(|word| word.to_ascii_lowercase())
                .collect();
        }
        if words.is_empty() {
            return Ok(Vec::new());
        }

        let mut lists = Vec::with_capacity(words.len());
        for word in &words {
            let Some(path) = self.gloss_shard_path(word) else {
                return Ok(Vec::new());
            };
            match self.shard(&path)?.get(word) {
                // One word with no entries means no entry has them all.
                None => return Ok(Vec::new()),
                Some(spans) => lists.push(spans.clone()),
            }
        }

        // Drive from the rarest word: it is both the most selective and the cheapest to
        // walk, and its order is the one most specific to what was asked.
        lists.sort_by_key(Vec::len);
        let (driver, rest) = lists.split_first().expect("at least one word");
        let others: Vec<std::collections::HashSet<Span>> = rest
            .iter()
            .map(|spans| spans.iter().copied().collect())
            .collect();

        let mut terms = File::open(self.root.join("terms.ndjson"))?;
        let mut hits = Vec::new();
        for &(offset, length) in driver {
            if hits.len() >= limit {
                break;
            }
            if !others.iter().all(|set| set.contains(&(offset, length))) {
                continue;
            }
            let mut buffer = vec![0u8; length as usize];
            terms.seek(SeekFrom::Start(offset))?;
            terms.read_exact(&mut buffer)?;
            hits.push(LookupHit {
                entry: serde_json::from_slice(&buffer)?,
                matched: MatchKind::Gloss,
            });
        }
        Ok(hits)
    }

    /// The commonest entries written with `character`, and how many there are in total.
    ///
    /// The count is the honest one — the whole posting list — while only `limit` entries are
    /// read from disk. That pairing is the point: a reader can be told "6 of 1369" without the
    /// page paying for 1369, because the ordering was settled when the index was built.
    ///
    /// Returns an empty list and a count of zero for a dictionary indexed before this bucket
    /// existed; [`Dictionary::has_kanji`] is how a caller tells that apart from a character
    /// nothing uses, exactly as `has_glosses` does for English.
    pub fn compounds(
        &self,
        character: &str,
        limit: usize,
    ) -> Result<(Vec<LookupHit>, usize), DictError> {
        let Some(path) = self.kanji_shard_path(character) else {
            return Ok((Vec::new(), 0));
        };
        let shard = self.shard(&path)?;
        let Some(spans) = shard.get(character) else {
            return Ok((Vec::new(), 0));
        };

        let total = spans.len();
        let mut terms = File::open(self.root.join("terms.ndjson"))?;
        let mut hits = Vec::new();
        for &(offset, length) in spans.iter().take(limit) {
            let mut buffer = vec![0u8; length as usize];
            terms.seek(SeekFrom::Start(offset))?;
            terms.read_exact(&mut buffer)?;
            hits.push(LookupHit {
                entry: serde_json::from_slice(&buffer)?,
                matched: MatchKind::Compound,
            });
        }
        Ok((hits, total))
    }

    fn kanji_shard_path(&self, character: &str) -> Option<PathBuf> {
        if character.is_empty() {
            return None;
        }
        Some(self.root.join("kanji").join(kanji_shard_name(character)))
    }

    fn shard_path(&self, query: &str, bucket: &str) -> Option<PathBuf> {
        if query.is_empty() {
            return None;
        }
        Some(self.root.join(bucket).join(shard_name(query)))
    }

    fn gloss_shard_path(&self, word: &str) -> Option<PathBuf> {
        if word.is_empty() {
            return None;
        }
        Some(self.root.join("gloss").join(gloss_shard_name(word)))
    }

    fn shard(&self, path: &Path) -> Result<Arc<Shard>, DictError> {
        {
            let cache = self
                .shards
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(shard) = cache.get(path) {
                return Ok(Arc::clone(shard));
            }
        }

        // Do not hold the dictionary-wide cache lock across disk I/O and JSON parsing. Intent
        // warmup may be reading one shard while the learner asks for another; an unrelated
        // speculative miss must never make the visible lookup queue behind it. Two simultaneous
        // misses for the same shard may parse it twice, then the second check below keeps one
        // canonical Arc. The higher-level exact-request gate prevents that in the ordinary path.
        // A missing shard is a miss, not an error: it just means no headword in this
        // dictionary starts with that character.
        //
        // **An absence is answered but never cached, and that distinction is the whole of a bug
        // this used to have.** A shard file that does not exist yet is not the same fact as a
        // shard file that exists and is empty: the second cannot change, and the first changes the
        // moment `index_secondary` backfills it. Caching the absence made the change unobservable
        // for the life of the reader, so a gloss search made before the backfill kept answering
        // empty while `has_glosses` — which stats the directory fresh — reported the index
        // available. `commands.rs` asserted the opposite in a comment; the reader really did not
        // see its new files. `a_backfilled_index_is_visible_to_a_reader_that_asked_before_it_existed`
        // is that case.
        //
        // The cost is one filesystem read *attempt* per query on the absent path — not a `stat`;
        // this asks for the contents and takes `NotFound` as the answer. Every shard that exists
        // is still cached on first read, so the common path is unchanged.
        //
        // **This fixes absence, and absence only.** A shard file that already exists can still be
        // rewritten underneath a reader: `index_secondary` rebuilds its buckets with
        // `remove_dir_all` followed by fresh writes, in the live directory, with no staging. A
        // reader holding cached spans from before that rebuild keeps them. Nothing here makes an
        // existing file immutable — that is what the generation work establishes, and until it
        // lands `a_reader_is_pinned_to_the_generation_it_was_opened_on` is the failing case that
        // says so.
        let json = match fs::read_to_string(path) {
            Ok(json) => json,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Arc::new(HashMap::new()))
            }
            Err(error) => return Err(error.into()),
        };
        let shard = Arc::new(serde_json::from_str::<Shard>(&json)?);
        let mut cache = self
            .shards
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(Arc::clone(
            cache
                .entry(path.to_path_buf())
                .or_insert_with(|| Arc::clone(&shard)),
        ))
    }

    /// Every entry, in import order. For maintenance and tests — the reader never
    /// needs this, which is the point of the sharded index.
    pub fn entries(&self) -> Result<Vec<Entry>, DictError> {
        let file = BufReader::new(File::open(self.root.join("terms.ndjson"))?);
        let mut entries = Vec::new();
        for line in file.lines() {
            let line = line?;
            if !line.trim().is_empty() {
                entries.push(serde_json::from_str(&line)?);
            }
        }
        Ok(entries)
    }
}

/// One tag as a dictionary's own `tag_bank_N.json` defines it: a 5-field positional
/// array `[name, category, order, notes, score]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TagInfo {
    #[serde(default)]
    pub category: String,
    /// What the code means in prose — the only part a learner reads.
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub score: f64,
}

/// Capture the dictionary's own tag codes. JMnedict explains `place` and `surname`
/// this way; the JMdict part-of-speech codes are *not* here, which is why a separate
/// abbreviation table is needed (`project notes` §6).
fn write_tags<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    dir: &Path,
) -> Result<(), DictError> {
    let mut banks: Vec<(u32, String)> = archive
        .file_names()
        // As for the term banks: an undecodable name is never a bank.
        .filter_map(Result::ok)
        .filter_map(|name| {
            bank_number(&name, "tag_bank_").map(|number| (number, name.into_owned()))
        })
        .collect();
    banks.sort_unstable();

    let mut tags: HashMap<String, TagInfo> = HashMap::new();
    for (_, name) in &banks {
        let json = read_member(archive, name)?;
        let rows: Vec<Vec<serde_json::Value>> = serde_json::from_str(&json)?;
        for row in rows {
            let Some(name) = row.first().and_then(|value| value.as_str()) else {
                continue;
            };
            tags.insert(
                name.to_string(),
                TagInfo {
                    category: string_at(&row, 1),
                    order: row.get(2).and_then(serde_json::Value::as_i64).unwrap_or(0),
                    notes: string_at(&row, 3),
                    score: row
                        .get(4)
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0),
                },
            );
        }
    }

    if !tags.is_empty() {
        fs::write(dir.join("tags.json"), serde_json::to_vec_pretty(&tags)?)?;
    }
    Ok(())
}

fn string_at(row: &[serde_json::Value], at: usize) -> String {
    row.get(at)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn read_member<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<String, DictError> {
    let mut member = archive.by_name(name)?;
    let mut json = String::new();
    member.read_to_string(&mut json)?;
    Ok(json)
}

/// `term_bank_3.json` → `Some(3)`, ignoring any directory prefix so a zip that wraps
/// its contents in a folder still imports.
fn bank_number(name: &str, prefix: &str) -> Option<u32> {
    let file = name.rsplit('/').next()?;
    file.strip_prefix(prefix)?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

/// Shard by the first character's code point, e.g. `u3042.json` for あ.
fn shard_name(key: &str) -> String {
    match key.chars().next() {
        Some(first) => format!("u{:04x}.json", first as u32),
        None => "u0000.json".to_string(),
    }
}

/// Shard an English word on its first **two** characters, e.g. `ea.json` for "eat".
///
/// One character would not do. Japanese headwords spread over thousands of first
/// characters and give shards of a few kilobytes; twenty-six Latin letters would give
/// shards some forty times larger, and reading one to answer a query is the whole cost
/// model of this index. A one-letter word shards under itself.
/// One file per character.
///
/// Not the two-letter prefix `gloss_shard_name` uses: there are a few thousand kanji against tens
/// of thousands of English words, and a character is already the whole key — so a shard holds
/// exactly the postings a query wants and nothing else. Hex-encoded because a file named after
/// the character itself would depend on the filesystem's own normalisation.
fn kanji_shard_name(character: &str) -> String {
    let mut name = String::new();
    for point in character.chars() {
        name.push_str(&format!("{:x}", point as u32));
    }
    if name.is_empty() {
        "other.json".to_string()
    } else {
        format!("{name}.json")
    }
}

/// Shard a sequence number on its **last** two digits, e.g. `70.json` for 1406970.
///
/// The last two rather than the first: JMdict sequences share long prefixes — most of Jitendex
/// is 1xxxxxx — so sharding from the front would put nearly the whole dictionary in one file.
/// The tail is evenly distributed and gives a hundred shards of roughly equal size.
fn sequence_shard_name(key: &str) -> String {
    let digits: String = key.chars().filter(char::is_ascii_digit).collect();
    match digits.len() {
        0 => "other.json".to_string(),
        n => format!("{}.json", &digits[n.saturating_sub(2)..]),
    }
}

fn gloss_shard_name(word: &str) -> String {
    let prefix: String = word
        .chars()
        .take(2)
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if prefix.is_empty() {
        "other.json".to_string()
    } else {
        format!("{prefix}.json")
    }
}

/// Write a bucket out, `name` deciding which shard each key lands in.
/// Hard-link a file, falling back to a copy where the filesystem will not link.
///
/// Linking is safe because generations are immutable: two directories naming the same bytes can
/// never disagree, and reclaiming one drops a link rather than the content.
fn link_or_copy(from: &Path, to: &Path) -> Result<(), DictError> {
    match fs::hard_link(from, to) {
        Ok(()) => Ok(()),
        // Cross-volume, an unsupported filesystem, or a link count at its limit. None of those is
        // a reason to fail an import.
        Err(_) => {
            fs::copy(from, to)?;
            Ok(())
        }
    }
}

fn link_or_copy_tree(from: &Path, to: &Path) -> Result<(), DictError> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            link_or_copy_tree(&entry.path(), &target)?;
        } else {
            link_or_copy(&entry.path(), &target)?;
        }
    }
    Ok(())
}

fn write_shards(
    dir: &Path,
    entries: &Shard,
    name: impl Fn(&str) -> String,
) -> Result<(), DictError> {
    // **The bucket is no longer cleared, because it can no longer be stale.** Clearing it was
    // right when a re-import rewrote `terms.ndjson` underneath the same directory: a leftover
    // shard whose key had gone still held offsets, and they then pointed into the middle of a
    // different entry. A generation is written once and never rewritten, so within one the set of
    // headwords cannot change, and there is nothing to leave behind.
    //
    // Removing the clear is what makes a rebuild safe for a reader. `index_secondary` runs against
    // the live generation — `build_gloss_index` backfills one while readers are open on it — and
    // `remove_dir_all` followed by fresh writes gave that reader a window in which the bucket was
    // absent, then partial. Each shard is now published by rename instead, so a reader sees either
    // the previous file or the complete new one.
    fs::create_dir_all(dir)?;
    let mut sharded: HashMap<String, Shard> = HashMap::new();
    for (headword, spans) in entries {
        if headword.is_empty() {
            continue;
        }
        sharded
            .entry(name(headword))
            .or_default()
            .insert(headword.clone(), spans.clone());
    }
    for (name, shard) in sharded {
        let target = dir.join(&name);
        // Same shape as `store::write_atomically` and `setup::fetch::publish_file`: the reader
        // never observes a partially written shard, only the old one or the new one.
        let staged = dir.join(format!(".{name}.{}", std::process::id()));
        fs::write(&staged, serde_json::to_vec(&shard)?)?;
        if let Err(error) = fs::rename(&staged, &target) {
            let _ = fs::remove_file(&staged);
            return Err(error.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a Yomitan zip in memory and write it to `path`.
    ///
    /// `index.json` is stored uncompressed and the banks are deflated, so the reader
    /// is exercised against both — real dictionaries are deflated.
    /// Where the live generation actually is.
    ///
    /// Tests that reach into the layout — removing a bucket to simulate an older import,
    /// or reading shard files back — have to name the generation, not the directory the
    /// application names. `Dictionary` resolves this once at open; these do it explicitly.
    fn live(dir: &Path) -> PathBuf {
        resolve_generation(dir)
    }

    fn write_dictionary(path: &Path, members: &[(&str, String)]) {
        let file = File::create(path).expect("create zip");
        let mut writer = zip::ZipWriter::new(file);
        for (name, contents) in members {
            let method = if name.ends_with("index.json") {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            };
            writer
                .start_file(
                    *name,
                    zip::write::SimpleFileOptions::default().compression_method(method),
                )
                .expect("start member");
            writer.write_all(contents.as_bytes()).expect("write member");
        }
        writer.finish().expect("finish zip");
    }

    fn sample_dictionary(dir: &Path) -> PathBuf {
        let index = json!({
            "title": "Test Dict",
            "format": 3,
            "revision": "2026-07-29",
            "author": "nobody",
            "attribution": "JMdict — EDRDG, CC BY-SA 4.0"
        });

        // Two banks, out of order in the zip, to prove import order follows the
        // bank number rather than the archive's own ordering.
        let bank2 = json!([["蛾", "が", "n", "", 5, ["moth"], 3, ""]]);
        let bank1 = json!([
            ["が", "", "prt", "", 40, ["indicates the subject"], 1, ""],
            ["我", "が", "n", "", 20, ["ego; self"], 2, ""],
            [
                "食べる",
                "たべる",
                "v1 vt",
                "v1",
                132,
                ["to eat"],
                4,
                "ichi1"
            ],
            [
                "食べる",
                "たべる",
                "v1 vt",
                "v1",
                8,
                ["to live on (a salary)"],
                5,
                ""
            ]
        ]);
        let tags = json!([
            ["prt", "partOfSpeech", 0, "particle", 0],
            ["n", "partOfSpeech", 0, "noun", 0]
        ]);

        let path = dir.join("test-dict.zip");
        write_dictionary(
            &path,
            &[
                ("index.json", index.to_string()),
                ("term_bank_2.json", bank2.to_string()),
                ("term_bank_1.json", bank1.to_string()),
                ("tag_bank_1.json", tags.to_string()),
            ],
        );
        path
    }

    /// A scratch directory that cleans itself up.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            // **The process id is load-bearing, not decoration.** Two `cargo test` runs
            // overlapping — a workspace run started while a package run is still going, which is
            // ordinary during development — shared these paths exactly, and each one's
            // `remove_dir_all` deleted the other's fixture mid-import. It surfaced as
            // `PermissionDenied` from a different test every time, which reads like a real fault
            // and is not one. `log.rs` and `corpus.rs` already keyed on the pid; these did not.
            let path = std::env::temp_dir()
                .join(format!("sakatsu-dict-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create scratch");
            Scratch(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn importing_captures_the_dictionarys_identity_and_its_attribution() {
        let scratch = Scratch::new("meta");
        let zip = sample_dictionary(scratch.path());
        let dictionary = Dictionary::import(&zip, &scratch.path().join("out")).expect("import");

        let meta = dictionary.meta();
        assert_eq!(meta.title, "Test Dict");
        assert_eq!(meta.format, 3);
        assert_eq!(meta.entry_count, 5);
        // Losing this would leave 屠つ unable to credit the source (project notes §8).
        assert_eq!(
            meta.attribution.as_deref(),
            Some("JMdict — EDRDG, CC BY-SA 4.0")
        );
    }

    #[test]
    fn an_entry_round_trips_through_the_shard_index() {
        let scratch = Scratch::new("roundtrip");
        let zip = sample_dictionary(scratch.path());
        let out = scratch.path().join("out");
        let dictionary = Dictionary::import(&zip, &out).expect("import");

        let hits = dictionary.lookup("食べる").expect("lookup");
        assert_eq!(hits.len(), 2, "both senses of 食べる are stored");
        assert_eq!(hits[0].matched, MatchKind::Spelling);
        assert_eq!(hits[0].entry.term, "食べる");
        assert_eq!(hits[0].entry.reading, "たべる");
        assert_eq!(hits[0].entry.definitions[0].glosses, ["to eat"]);
        // Ordered by score: the common sense first.
        assert!(hits[0].entry.score > hits[1].entry.score);

        // Reopening reads the same bytes — nothing is held only in memory.
        let reopened = Dictionary::open(&out).expect("open");
        assert_eq!(reopened.lookup("食べる").expect("lookup").len(), 2);
        assert_eq!(reopened.meta().entry_count, 5);
    }

    #[test]
    fn looking_up_a_particle_does_not_drown_in_its_homophones() {
        // The case from project notes §6: が is read the same as 蛾, 我 and 雅. A single
        // bucket would answer a question about a particle with a moth.
        let scratch = Scratch::new("homophone");
        let zip = sample_dictionary(scratch.path());
        let dictionary = Dictionary::import(&zip, &scratch.path().join("out")).expect("import");

        let hits = dictionary.lookup("が").expect("lookup");
        assert_eq!(hits.len(), 1, "only the word spelled が");
        assert_eq!(hits[0].matched, MatchKind::Spelling);
        assert_eq!(
            hits[0].entry.definitions[0].glosses,
            ["indicates the subject"]
        );

        // The homophones are still reachable — by their own spelling.
        let moth = dictionary.lookup("蛾").expect("lookup");
        assert_eq!(moth[0].entry.definitions[0].glosses, ["moth"]);
        assert_eq!(moth[0].matched, MatchKind::Spelling);
    }

    #[test]
    fn a_reading_only_query_falls_back_to_the_reading_bucket() {
        let scratch = Scratch::new("reading");
        let zip = sample_dictionary(scratch.path());
        let dictionary = Dictionary::import(&zip, &scratch.path().join("out")).expect("import");

        // Nothing is spelled たべる, so the reading bucket answers.
        let hits = dictionary.lookup("たべる").expect("lookup");
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|hit| hit.matched == MatchKind::Reading));
        assert!(hits.iter().all(|hit| hit.entry.term == "食べる"));
    }

    #[test]
    fn a_katakana_query_reaches_a_hiragana_entry_only_when_nothing_exact_matches() {
        // The セイア case: JMnedict files the name Seia under せいあ, so a katakana surface
        // finds nothing in either exact bucket. Folding is what makes the name reachable.
        let scratch = Scratch::new("folded");
        let zip = sample_dictionary(scratch.path());
        let dictionary = Dictionary::import(&zip, &scratch.path().join("out")).expect("import");

        let hits = dictionary.lookup("タベル").expect("lookup");
        assert_eq!(hits.len(), 2, "タベル folds to たべる");
        assert!(hits.iter().all(|hit| hit.matched == MatchKind::Folded));
        assert!(hits.iter().all(|hit| hit.entry.term == "食べる"));

        // And it is genuinely last: an exact hit is never displaced by a folded one, so
        // the fold can only ever add answers where there were none.
        let exact = dictionary.lookup("たべる").expect("lookup");
        assert!(exact.iter().all(|hit| hit.matched == MatchKind::Reading));

        // A query with no katakana in it does no extra work and finds nothing new.
        assert!(dictionary.lookup("およぐ").expect("lookup").is_empty());
    }

    #[test]
    fn import_order_follows_the_bank_number_not_the_archive_order() {
        let scratch = Scratch::new("order");
        let zip = sample_dictionary(scratch.path());
        let dictionary = Dictionary::import(&zip, &scratch.path().join("out")).expect("import");

        // term_bank_1 comes second in the zip but first in the index.
        let entries = dictionary.entries().expect("entries");
        assert_eq!(entries.first().map(|e| e.term.as_str()), Some("が"));
        assert_eq!(entries.last().map(|e| e.term.as_str()), Some("蛾"));
    }

    #[test]
    fn the_dictionarys_own_tag_codes_are_captured() {
        let scratch = Scratch::new("tags");
        let zip = sample_dictionary(scratch.path());
        let out = scratch.path().join("out");
        Dictionary::import(&zip, &out).expect("import");

        let tags: HashMap<String, TagInfo> = serde_json::from_str(
            &fs::read_to_string(live(&out).join("tags.json")).expect("tags.json"),
        )
        .expect("parse tags");
        assert_eq!(tags["prt"].notes, "particle");
        assert_eq!(tags["n"].category, "partOfSpeech");
    }

    #[test]
    fn a_miss_is_empty_rather_than_an_error() {
        let scratch = Scratch::new("miss");
        let zip = sample_dictionary(scratch.path());
        let dictionary = Dictionary::import(&zip, &scratch.path().join("out")).expect("import");

        // No headword starts with these, so the shard file does not even exist.
        assert!(dictionary.lookup("存在しない").expect("lookup").is_empty());
        assert!(dictionary.lookup("zzz").expect("lookup").is_empty());
        assert!(dictionary.lookup("   ").expect("lookup").is_empty());
    }

    #[test]
    fn an_english_word_finds_the_entries_glossed_with_it() {
        let scratch = Scratch::new("gloss");
        let zip = sample_dictionary(scratch.path());
        let dictionary = Dictionary::import(&zip, &scratch.path().join("out")).expect("import");

        assert!(dictionary.has_glosses());

        let hits = dictionary.search_glosses("eat", 10).expect("search");
        assert_eq!(hits[0].entry.term, "食べる");
        assert_eq!(hits[0].matched, MatchKind::Gloss);

        // "to" is a stopword, so this asks the same question as "eat" — which is the
        // point of dropping it from the query as well as from the index.
        let spelled_out = dictionary.search_glosses("to eat", 10).expect("search");
        assert_eq!(spelled_out[0].entry.term, "食べる");

        // A word inside a longer gloss is still found.
        let moth = dictionary.search_glosses("moth", 10).expect("search");
        assert_eq!(moth[0].entry.term, "蛾");
    }

    /// Every query word must match, or "moth" and "ego" would answer each other.
    #[test]
    fn a_multi_word_query_needs_every_word() {
        let scratch = Scratch::new("gloss-multi");
        let zip = sample_dictionary(scratch.path());
        let dictionary = Dictionary::import(&zip, &scratch.path().join("out")).expect("import");

        assert!(!dictionary
            .search_glosses("ego self", 10)
            .expect("s")
            .is_empty());
        // 我 is "ego; self" and 蛾 is "moth". Neither has both words.
        assert!(dictionary
            .search_glosses("ego moth", 10)
            .expect("s")
            .is_empty());
        assert!(dictionary
            .search_glosses("nothinghere", 10)
            .expect("s")
            .is_empty());
    }

    /// Parenthetical notes are not part of the gloss, however deeply they nest.
    #[test]
    fn a_usage_note_does_not_stop_a_gloss_being_an_exact_match() {
        // The real case: 犬's gloss is "dog (Canis (lupus) familiaris)", and a strip that
        // does not track depth leaves "familiaris" behind and calls the match inexact.
        assert_eq!(gloss_words("dog (Canis (lupus) familiaris)"), ["dog"]);
        assert_eq!(gloss_words("water (esp. cool or cold)"), ["water"]);
        // Stopwords and single letters go; "e" and "g" here are from "e.g.".
        assert_eq!(gloss_words("to live on (e.g. a salary)"), ["live"]);
        assert!(gloss_words("of the").is_empty());
    }

    /// Two characters, not one — the reason is in `gloss_shard_name`'s doc comment.
    #[test]
    fn gloss_shards_are_named_by_a_two_character_prefix() {
        assert_eq!(gloss_shard_name("eat"), "ea.json");
        assert_eq!(gloss_shard_name("e"), "e.json");
        assert_eq!(gloss_shard_name("one's"), "on.json");
    }

    /// **The claim the backfill rests on.**
    ///
    /// An existing install must not have to re-import 433,885 entries to gain English
    /// search, which is only true if indexing from `terms.ndjson` afterwards produces
    /// exactly what indexing during import would have.
    #[test]
    fn indexing_glosses_afterwards_matches_indexing_them_during_import() {
        let scratch = Scratch::new("gloss-backfill");
        let zip = sample_dictionary(scratch.path());
        let out = scratch.path().join("out");
        Dictionary::import(&zip, &out).expect("import");

        let fresh = read_bucket(&live(&out).join("gloss"));
        assert!(!fresh.is_empty(), "import wrote a gloss bucket");

        // Throw it away and rebuild from the entries alone — no zip in sight.
        fs::remove_dir_all(live(&out).join("gloss")).expect("remove");
        let reopened = Dictionary::open(&out).expect("open");
        assert!(!reopened.has_glosses(), "and it is gone");

        let count = reopened.index_secondary().expect("backfill");
        assert_eq!(count, 5, "every entry was read");
        // The backfill publishes a generation rather than writing into the one being read, so the
        // reader that asked for it keeps its own — and the refreshed one is where the index is.
        assert!(
            !reopened.has_glosses(),
            "the requesting reader stays on the generation it resolved"
        );
        let refreshed = Dictionary::open(&out).expect("open after the backfill");
        assert_eq!(
            read_bucket(&live(&out).join("gloss")),
            fresh,
            "byte-for-byte the same"
        );
        assert_eq!(
            refreshed.search_glosses("eat", 10).expect("search")[0]
                .entry
                .term,
            "食べる"
        );
    }

    /// A dictionary imported before the gloss bucket existed still opens and still works.
    #[test]
    fn a_dictionary_without_a_gloss_bucket_says_so_rather_than_failing() {
        let scratch = Scratch::new("gloss-absent");
        let zip = sample_dictionary(scratch.path());
        let out = scratch.path().join("out");
        Dictionary::import(&zip, &out).expect("import");
        fs::remove_dir_all(live(&out).join("gloss")).expect("remove");

        let dictionary = Dictionary::open(&out).expect("open");
        assert!(!dictionary.has_glosses());
        // Empty, not an error — but the caller has `has_glosses` to tell the two apart,
        // because a silent nothing reads exactly like a word that is not in the dictionary.
        assert!(dictionary
            .search_glosses("eat", 10)
            .expect("search")
            .is_empty());
        // Japanese lookup is untouched.
        assert_eq!(dictionary.lookup("食べる").expect("lookup").len(), 2);
    }

    /// Every shard file in a bucket, for comparing two builds of it.
    fn read_bucket(dir: &Path) -> std::collections::BTreeMap<String, Shard> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Default::default();
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| {
                let json = fs::read_to_string(entry.path()).expect("read shard");
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    serde_json::from_str(&json).expect("parse shard"),
                )
            })
            .collect()
    }

    /// **A re-import must not leave the old index behind.**
    ///
    /// `terms.ndjson` is recreated from scratch, so every offset moves. A shard file whose
    /// key no longer exists used to survive with spans pointing into the middle of a
    /// different entry — a lookup then returns the wrong headword, or fails to parse at
    /// all. Fetching a dictionary is one click now, so re-importing is something people
    /// will actually do.
    /// A generation that cannot be opened is never published, and the live one is untouched.
    ///
    /// **The order is the guarantee.** The staging directory is built, then opened, and only a
    /// generation that opened is named by `current`. A corrupt archive therefore costs the import
    /// and nothing else — where the previous design had already renamed the live directory aside
    /// by the time anything went wrong, and depended on a restore path to put it back.
    #[test]
    fn a_failed_import_leaves_the_live_generation_alone() {
        let scratch = Scratch::new("failed-publish");
        let out = scratch.path().join("out");
        let archive = sample_dictionary(scratch.path());
        Dictionary::import(&archive, &out).expect("first import");
        let before = resolve_generation(&out);
        let held = Dictionary::open(&out).expect("open");

        // Not a dictionary at all.
        let broken = scratch.path().join("broken.zip");
        write_dictionary(&broken, &[("nothing.json", "{}".to_string())]);
        assert!(
            Dictionary::import(&broken, &out).is_err(),
            "an archive with no index.json is not a dictionary"
        );

        assert_eq!(
            resolve_generation(&out),
            before,
            "a failed import must not move the pointer"
        );
        assert!(
            !held.lookup("が").expect("lookup").is_empty(),
            "the reader open across the failure still answers"
        );
        assert!(
            !Dictionary::open(&out)
                .expect("open")
                .lookup("が")
                .expect("lookup")
                .is_empty(),
            "and so does a new one"
        );
    }

    /// A crash between writing a generation and naming it leaves a working dictionary.
    ///
    /// **This is the interval the old design could not survive.** It renamed the live directory
    /// aside and the new one into place; a stop in between left nothing at the path the app reads,
    /// and `recover_interrupted_publish` existed to repair it. Publication is now one small write
    /// of `current` **after** the new generation is complete, so the same stop leaves an orphan
    /// directory and a dictionary that still works. Simulated by writing the generation and not
    /// the pointer.
    #[test]
    fn a_generation_written_but_never_named_is_ignored() {
        let scratch = Scratch::new("orphan-generation");
        let out = scratch.path().join("out");
        let archive = sample_dictionary(scratch.path());
        Dictionary::import(&archive, &out).expect("import");
        let live_before = resolve_generation(&out);

        // A complete generation directory that `current` does not name.
        let orphan = out.join("g-000000000099");
        copy_tree(&live_before, &orphan);
        assert!(orphan.join("meta.json").exists(), "the orphan is complete");

        assert_eq!(
            resolve_generation(&out),
            live_before,
            "an unnamed generation is not live, however complete it looks"
        );
        assert!(
            !Dictionary::open(&out)
                .expect("open")
                .lookup("が")
                .expect("lookup")
                .is_empty(),
            "the dictionary opens and answers across the interrupted publish"
        );

        // The next publish supersedes it and reclaims what it can.
        Dictionary::import(&archive, &out).expect("second import");
        assert_ne!(resolve_generation(&out), live_before, "the pointer moved");
    }

    /// A backfill requested through a superseded reader is refused, not published.
    ///
    /// **The lock serialises publication; it does not make a pinned root fresh.** A reader
    /// resolves its generation when it is opened and keeps it — which is the whole point of
    /// leases — so a backfill asked for through a reader that has since been overtaken would build
    /// from the *old* generation and publish it as the newest one. The sequence is ordinary: open
    /// A, an install lands B, a queued gloss-index job runs on the A reader it captured earlier.
    /// The result would be B silently replaced by A.
    ///
    /// Refused rather than quietly retargeted at the current generation: the caller asked about a
    /// dictionary that has been replaced, and is entitled to know rather than to receive an answer
    /// about something else.
    #[test]
    fn a_backfill_from_a_superseded_reader_is_refused() {
        let scratch = Scratch::new("stale-backfill");
        let out = scratch.path().join("out");

        let write = |name: &str, entries: serde_json::Value| -> PathBuf {
            let path = scratch.path().join(name);
            write_dictionary(
                &path,
                &[
                    (
                        "index.json",
                        json!({ "title": "T", "format": 3 }).to_string(),
                    ),
                    ("term_bank_1.json", entries.to_string()),
                ],
            );
            path
        };
        let first = write(
            "a.zip",
            json!([["猫", "ねこ", "n", "", 10, ["cat"], 1, ""]]),
        );
        let second = write(
            "b.zip",
            json!([["犬", "いぬ", "n", "", 10, ["dog"], 1, ""]]),
        );

        Dictionary::import(&first, &out).expect("import A");
        let stale = Dictionary::open(&out).expect("open A");
        assert!(!stale.lookup("猫").expect("lookup").is_empty(), "A is live");

        Dictionary::import(&second, &out).expect("import B");
        let current = resolve_generation(&out);

        assert!(
            matches!(stale.index_secondary(), Err(DictError::Superseded)),
            "a backfill through the A reader must be refused once B is live"
        );
        assert_eq!(
            resolve_generation(&out),
            current,
            "and must not move the pointer"
        );

        // B is still what the application gets, with A nowhere in it.
        let live = Dictionary::open(&out).expect("open");
        assert!(
            !live.lookup("犬").expect("lookup").is_empty(),
            "B is still the live dictionary"
        );
        assert!(
            live.lookup("猫").expect("lookup").is_empty(),
            "A was not reinstated by the refused backfill"
        );

        // The reader that is current can still do the work.
        assert!(
            live.index_secondary().is_ok(),
            "a current reader is not refused"
        );
    }

    /// A backfill that fails part-way leaves no partial index and no moved pointer.
    ///
    /// **The partial state used to be reachable and permanent.** Writing the buckets into the live
    /// generation meant `has_glosses` — which only asks whether the directory exists — turned true
    /// as soon as the first shard landed, and a failure after that left the half-built bucket
    /// behind for good. Building a whole generation and switching to it with one write of
    /// `current` removes the state entirely: either every shard is there and the pointer moved, or
    /// nothing changed.
    ///
    /// The failure is induced by appending a line `terms.ndjson` cannot parse. The backfill reads
    /// that file end to end, so it fails after several entries have already been indexed — which
    /// is precisely part-way.
    #[test]
    fn a_backfill_that_fails_part_way_changes_nothing() {
        let scratch = Scratch::new("backfill-fails");
        let out = scratch.path().join("out");
        let archive = sample_dictionary(scratch.path());
        Dictionary::import(&archive, &out).expect("import");
        fs::remove_dir_all(live(&out).join("gloss")).expect("drop the gloss bucket");

        let before = resolve_generation(&out);
        let entries = before.join("terms.ndjson");
        let mut corrupted = fs::read_to_string(&entries).expect("read");
        corrupted.push_str(
            "{ this is not an entry }
",
        );
        fs::write(&entries, corrupted).expect("write");

        let reader = Dictionary::open(&out).expect("open");
        assert!(!reader.has_glosses(), "no index to begin with");
        assert!(
            reader.index_secondary().is_err(),
            "an unparseable entry must fail the backfill"
        );

        assert_eq!(
            resolve_generation(&out),
            before,
            "a failed backfill must not move the pointer"
        );
        assert!(
            !Dictionary::open(&out).expect("open").has_glosses(),
            "and must not leave a partial index where a reader would trust it"
        );
        // The dictionary still works for everything that did not depend on the backfill.
        assert!(
            !Dictionary::open(&out)
                .expect("open")
                .lookup("が")
                .expect("lookup")
                .is_empty(),
            "a failed backfill costs the index, not the dictionary"
        );
    }

    /// A held reader keeps its generation across any number of publishes, and loses it only after
    /// it is dropped.
    ///
    /// **An earlier version of this test accepted an error or an empty result after reclamation.**
    /// That was the requirement inverted: retention by count is not ownership, and a
    /// `remove_dir_all` that happens to fail on Windows is not a substitute for knowing whether
    /// anyone is reading. Every lookup below must **succeed with the right contents** — no error is
    /// tolerated, and neither is a false miss.
    ///
    /// Both paths are checked. 犬 is looked up before the publishes, so it answers from a warmed
    /// shard cache; 鳥 is looked up only afterwards, so it goes to disk for a shard and then for
    /// the bytes. A lease that only kept the cache alive would pass the first and fail the second.
    #[test]
    fn a_held_reader_keeps_its_generation_until_it_is_released() {
        let scratch = Scratch::new("lease-held");
        let out = scratch.path().join("out");

        let first = json!([
            ["犬", "いぬ", "n", "", 10, ["dog"], 1, ""],
            ["猫", "ねこ", "n", "", 10, ["cat"], 2, ""],
            ["鳥", "とり", "n", "", 10, ["bird"], 3, ""]
        ]);
        let path = scratch.path().join("first.zip");
        write_dictionary(
            &path,
            &[
                (
                    "index.json",
                    json!({ "title": "T", "format": 3 }).to_string(),
                ),
                ("term_bank_1.json", first.to_string()),
            ],
        );
        let held = Dictionary::import(&path, &out).expect("first import");
        let generation = resolve_generation(&out);

        // Warm the cache for 犬 and leave 鳥 cold.
        assert_eq!(held.lookup("犬").expect("lookup").len(), 1);

        // Several publishes, each dropping 犬 so a reader that followed them would lose it.
        let later = json!([["猫", "ねこ", "n", "", 10, ["cat"], 2, ""]]);
        let path = scratch.path().join("later.zip");
        write_dictionary(
            &path,
            &[
                (
                    "index.json",
                    json!({ "title": "T", "format": 3 }).to_string(),
                ),
                ("term_bank_1.json", later.to_string()),
            ],
        );
        for _ in 0..3 {
            Dictionary::import(&path, &out).expect("later import");
        }

        assert!(
            generation.exists(),
            "a leased generation must survive every publish while it is held"
        );

        // Cached path: 犬 was looked up before the publishes.
        let dog = held.lookup("犬").expect("a held reader must not fail");
        assert_eq!(
            dog.len(),
            1,
            "and must not miss what its generation contains"
        );
        assert_eq!(dog[0].entry.term, "犬");

        // Uncached path: 鳥 was not. This is the one a cache-only lease would fail.
        let bird = held.lookup("鳥").expect("a held reader must not fail");
        assert_eq!(bird.len(), 1, "an uncached lookup must reach its own files");
        assert_eq!(bird[0].entry.term, "鳥");
        assert_eq!(
            bird[0].entry.definitions[0].glosses,
            vec!["bird".to_string()]
        );

        // Released, then reclaimed by the next publish.
        drop(held);
        Dictionary::import(&path, &out).expect("import after release");
        assert!(
            !generation.exists(),
            "an unreferenced generation must be reclaimed once its reader is gone"
        );
    }

    /// A reader opened on a pre-generation directory keeps its files through the migration.
    ///
    /// **The installed-dictionary gate runs do not cover this.** They open a legacy layout and read
    /// it, which shows compatibility; they never import, so they never perform the transition. This
    /// is that transition: a legacy reader is held while two publishes migrate the directory to
    /// generations underneath it.
    #[test]
    fn a_legacy_reader_keeps_its_files_through_the_migration() {
        let scratch = Scratch::new("lease-legacy");
        let out = scratch.path().join("out");
        let archive = sample_dictionary(scratch.path());
        Dictionary::import(&archive, &out).expect("import");
        flatten_to_legacy(&out);

        // A reader of the pre-generation layout: its generation is the directory itself.
        let legacy = Dictionary::open(&out).expect("open");
        assert_eq!(
            resolve_generation(&out),
            out,
            "the fixture really is the legacy layout"
        );
        assert!(!legacy.lookup("が").expect("lookup").is_empty());

        // Two publishes: the first migrates, the second is when legacy files become reclaimable.
        Dictionary::import(&archive, &out).expect("migrating import");
        Dictionary::import(&archive, &out).expect("second import");

        assert!(
            out.join("terms.ndjson").exists(),
            "legacy files must survive while a legacy reader still holds them"
        );
        let hits = legacy.lookup("が").expect("a legacy reader must not fail");
        assert!(
            !hits.is_empty(),
            "and must not miss what it could find before"
        );
        assert!(
            !legacy.lookup("食べる").expect("lookup").is_empty(),
            "including a word it had not looked up before the migration"
        );

        drop(legacy);
        Dictionary::import(&archive, &out).expect("import after release");
        assert!(
            !out.join("terms.ndjson").exists(),
            "and are reclaimed once nothing is reading them"
        );
    }

    /// Move a published generation back up into the directory, as a pre-generation install.
    fn flatten_to_legacy(dir: &Path) {
        let generation = resolve_generation(dir);
        assert_ne!(&generation, dir, "there is a generation to flatten");
        for entry in fs::read_dir(&generation).expect("read") {
            let entry = entry.expect("entry");
            fs::rename(entry.path(), dir.join(entry.file_name())).expect("move up");
        }
        fs::remove_dir_all(&generation).expect("drop the generation");
        fs::remove_file(dir.join(CURRENT)).expect("drop the pointer");
    }

    /// Copy a directory tree, for simulating a publish that stopped before its pointer write.
    fn copy_tree(from: &Path, to: &Path) {
        fs::create_dir_all(to).expect("create");
        for entry in fs::read_dir(from).expect("read") {
            let entry = entry.expect("entry");
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), &target).expect("copy");
            }
        }
    }

    /// A reader is pinned to the generation it was opened on, and never mixes two.
    ///
    /// **The sibling test below reopens before asserting, and that is what hides this.** Its helper
    /// says so: "Reopened from disk, so the shard cache cannot hide a stale file." Reopening is
    /// correct for what that test is about — that the *files* are consistent after a re-import. It
    /// is not what the application does. `DictionaryState` holds owned `Dictionary` values built
    /// once at startup, and a thread that called `.now()` before an install keeps using them.
    ///
    /// `Dictionary` identifies its data by `root: PathBuf` — a path, not a handle — caches shard
    /// spans that are never invalidated, and reopens `terms.ndjson` by that path on every query.
    /// `publish_staged` renames the whole directory. So the offsets and the bytes can come from
    /// different generations. 犬 is placed first so that dropping it moves 猫.
    ///
    /// **What this asserts is a snapshot, not freshness.** A held reader must keep answering from
    /// the release it was opened on: the same entries, with the same contents. It must not see the
    /// new one, and it must not fail. Freshness belongs to a reader acquired after the swap, which
    /// is the second half. An earlier version of this test demanded the held reader see the *new*
    /// release, which would have rejected the very design it exists to justify.
    #[test]
    fn a_reader_is_pinned_to_the_generation_it_was_opened_on() {
        let scratch = Scratch::new("held-reimport");
        let out = scratch.path().join("out");

        // 犬 first, so removing it later moves 猫. 鳥 is never looked up before the swap: it is
        // how the pin is checked on a path the shard cache cannot have warmed.
        let first = json!([
            ["犬", "いぬ", "n", "", 10, ["dog"], 1, ""],
            ["猫", "ねこ", "n", "", 10, ["cat"], 2, ""],
            ["鳥", "とり", "n", "", 10, ["bird"], 3, ""]
        ]);
        let path = scratch.path().join("first.zip");
        write_dictionary(
            &path,
            &[
                (
                    "index.json",
                    json!({ "title": "T", "format": 3 }).to_string(),
                ),
                ("term_bank_1.json", first.to_string()),
            ],
        );
        let held = Dictionary::import(&path, &out).expect("first import");

        // Load-bearing: this is what puts generation-1 spans into the shard cache. 鳥 is
        // deliberately left alone.
        assert_eq!(held.lookup("猫").expect("lookup").len(), 1);
        assert_eq!(held.lookup("犬").expect("lookup").len(), 1);

        // The upstream release drops 犬 and lengthens 猫, so every offset moves.
        let second = json!([
            [
                "猫",
                "ねこ",
                "n",
                "",
                10,
                [
                    "cat",
                    "feline",
                    "a much longer gloss list that shifts every byte offset"
                ],
                2,
                ""
            ],
            ["鳥", "とり", "n", "", 10, ["bird"], 3, ""]
        ]);
        let path = scratch.path().join("second.zip");
        write_dictionary(
            &path,
            &[
                (
                    "index.json",
                    json!({ "title": "T", "format": 3 }).to_string(),
                ),
                ("term_bank_1.json", second.to_string()),
            ],
        );
        Dictionary::import(&path, &out).expect("second import");

        // A reader acquired after the swap sees the new release. This half already works.
        let fresh = dictionary_of(&out);
        assert!(
            fresh.lookup("犬").expect("lookup").is_empty(),
            "a newly acquired reader must not still find the dropped word"
        );
        let fresh_cat = fresh.lookup("猫").expect("lookup");
        assert_eq!(fresh_cat.len(), 1);
        // Across every definition: the importer gives a multi-gloss entry one definition per
        // gloss rather than one definition holding them all.
        let glosses = |hit: &LookupHit| -> Vec<String> {
            hit.entry
                .definitions
                .iter()
                .flat_map(|definition| definition.glosses.iter().cloned())
                .collect()
        };
        assert!(
            glosses(&fresh_cat[0]).iter().any(|gloss| gloss == "feline"),
            "a newly acquired reader must see the lengthened entry, got {:?}",
            glosses(&fresh_cat[0])
        );

        // The held reader is pinned. Contents are checked, not just counts: a hit whose bytes came
        // from the other generation would still be one hit.
        let dog = held
            .lookup("犬")
            .expect("the held reader must still answer");
        assert_eq!(
            dog.len(),
            1,
            "the held reader kept the release it was opened on"
        );
        assert_eq!(dog[0].entry.term, "犬");
        assert_eq!(dog[0].entry.definitions[0].glosses, vec!["dog".to_string()]);

        let cat = held
            .lookup("猫")
            .expect("the held reader must still answer");
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].entry.term, "猫");
        assert_eq!(
            glosses(&cat[0]),
            vec!["cat".to_string()],
            "the held reader must see its own generation of 猫, not the lengthened one"
        );

        // And a word it never asked for before the swap, so the pin is not merely the cache
        // answering from memory.
        let bird = held
            .lookup("鳥")
            .expect("an uncached lookup must also be pinned");
        assert_eq!(bird.len(), 1);
        assert_eq!(bird[0].entry.term, "鳥");
        assert_eq!(
            bird[0].entry.definitions[0].glosses,
            vec!["bird".to_string()]
        );
    }

    /// A published index is visible to readers acquired after it, and no reader contradicts itself.
    ///
    /// **The bug was self-contradiction, not staleness.** A missing shard file was cached as an
    /// empty map and never re-checked, so a gloss search made before `index_secondary` backfilled
    /// the bucket kept answering empty — while `has_glosses`, which stats the directory fresh,
    /// reported the index available. `commands.rs` states the assumption this disproves: *"The
    /// dictionaries themselves stay open and see their new files immediately, so invalidate only
    /// the session result cache before the UI retries."*
    ///
    /// **The assertions are deliberately architecture-independent.** Whether a held reader sees a
    /// later backfill depends on whether it is pinned to a generation — that is a design choice,
    /// and this test does not prejudge it. What no design may do is let one reader advertise an
    /// index it cannot read. So: a reader acquired after publication sees it, and every reader
    /// agrees with itself.
    #[test]
    fn a_published_index_is_visible_and_no_reader_contradicts_itself() {
        let scratch = Scratch::new("backfill-visible");
        let out = scratch.path().join("out");
        let archive = sample_dictionary(scratch.path());
        Dictionary::import(&archive, &out).expect("import");

        // The state a dictionary imported before the gloss index existed is in.
        fs::remove_dir_all(live(&out).join("gloss")).expect("drop the gloss bucket");

        let held = Dictionary::open(&out).expect("open");
        assert!(!held.has_glosses(), "the bucket was just removed");
        // Load-bearing: the query that used to cache the absent shard as an empty map.
        assert!(
            held.search_glosses("moth", 10).expect("search").is_empty(),
            "no index yet, so no results yet"
        );

        // The backfill, exactly as `build_gloss_index` performs it: on a separate handle.
        Dictionary::open(&out)
            .expect("open")
            .index_secondary()
            .expect("backfill");

        // A reader acquired after publication sees it, without the application restarting.
        let fresh = Dictionary::open(&out).expect("open");
        assert!(fresh.has_glosses(), "the published index is there");
        assert!(
            !fresh.search_glosses("moth", 10).expect("search").is_empty(),
            "a newly acquired reader must find what the published index contains"
        );

        // **The invariant that holds whatever the design.** A reader may lag a publication, but it
        // may never claim an index it will then answer nothing from: that is the shape the UI
        // reports as "English search available" over a reader that returns empty until restart.
        assert_eq!(
            held.has_glosses(),
            !held.search_glosses("moth", 10).expect("search").is_empty(),
            "the held reader advertises an index it cannot read"
        );
    }

    /// **A re-import must not leave the old index behind.**
    ///
    /// `terms.ndjson` is recreated from scratch, so every offset moves. A shard file whose
    /// key no longer exists used to survive with spans pointing into the middle of a
    /// different entry — a lookup then returns the wrong headword, or fails to parse at
    /// all. Fetching a dictionary is one click now, so re-importing is something people
    /// will actually do.
    ///
    /// This reopens before asserting, which is what it is for: the *files* are consistent.
    /// [`a_reader_is_pinned_to_the_generation_it_was_opened_on`] is the other half — what a
    /// reader that was already open sees.
    #[test]
    fn re_importing_does_not_leave_shards_pointing_at_the_old_file() {
        let scratch = Scratch::new("reimport");
        let out = scratch.path().join("out");

        // First: a dictionary with two words.
        let first = json!([
            ["猫", "ねこ", "n", "", 10, ["cat"], 1, ""],
            ["犬", "いぬ", "n", "", 10, ["dog"], 2, ""]
        ]);
        let path = scratch.path().join("first.zip");
        write_dictionary(
            &path,
            &[
                (
                    "index.json",
                    json!({ "title": "T", "format": 3 }).to_string(),
                ),
                ("term_bank_1.json", first.to_string()),
            ],
        );
        Dictionary::import(&path, &out).expect("first import");
        assert_eq!(dictionary_of(&out).lookup("犬").expect("lookup").len(), 1);

        // Then the upstream release drops 犬 and adds a longer entry, moving every offset.
        let second = json!([[
            "猫",
            "ねこ",
            "n",
            "",
            10,
            ["cat", "feline", "a much longer gloss list"],
            1,
            ""
        ]]);
        let path = scratch.path().join("second.zip");
        write_dictionary(
            &path,
            &[
                (
                    "index.json",
                    json!({ "title": "T", "format": 3 }).to_string(),
                ),
                ("term_bank_1.json", second.to_string()),
            ],
        );
        Dictionary::import(&path, &out).expect("second import");

        let dictionary = dictionary_of(&out);
        assert!(
            dictionary.lookup("犬").expect("lookup").is_empty(),
            "犬 is gone from the source and must be gone from the index"
        );
        // And what remains is itself, read from the right bytes rather than from wherever
        // the old span happened to point.
        let cat = dictionary.lookup("猫").expect("lookup");
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].entry.term, "猫");
        // Each element of the source's field 5 is its own definition, so the new, longer
        // entry is visible as the extra ones.
        let glosses: Vec<&str> = cat[0]
            .entry
            .definitions
            .iter()
            .flat_map(|definition| definition.glosses.iter())
            .map(String::as_str)
            .collect();
        assert!(glosses.contains(&"feline"), "got {glosses:?}");
    }

    #[test]
    fn a_failed_re_import_leaves_the_previous_dictionary_intact() {
        let scratch = Scratch::new("failed-reimport");
        let out = scratch.path().join("out");
        let first = sample_dictionary(scratch.path());
        Dictionary::import(&first, &out).expect("first import");

        let broken = scratch.path().join("broken.zip");
        write_dictionary(
            &broken,
            &[
                (
                    "index.json",
                    json!({ "title": "Broken", "format": 3 }).to_string(),
                ),
                ("term_bank_1.json", "this is not json".into()),
            ],
        );
        Dictionary::import(&broken, &out).expect_err("broken replacement must fail");

        let reopened = Dictionary::open(&out).expect("old generation still opens");
        assert_eq!(reopened.meta().title, "Test Dict");
        assert_eq!(reopened.lookup("食べる").expect("lookup").len(), 2);
    }

    #[test]
    fn open_recovers_a_previous_generation_left_in_the_publish_backup() {
        let scratch = Scratch::new("recover-publish");
        let out = scratch.path().join("out");
        let zip = sample_dictionary(scratch.path());
        Dictionary::import(&zip, &out).expect("import");

        let backup = backup_path(&out).expect("backup path");
        fs::rename(&out, &backup).expect("simulate interruption");
        let reopened = Dictionary::open(&out).expect("recover");

        assert!(out.exists());
        assert!(!backup.exists());
        assert_eq!(reopened.lookup("食べる").expect("lookup").len(), 2);
    }

    /// Reopened from disk, so the shard cache cannot hide a stale file.
    fn dictionary_of(dir: &Path) -> Dictionary {
        Dictionary::open(dir).expect("open")
    }

    #[test]
    fn a_zip_without_an_index_is_not_a_dictionary() {
        let scratch = Scratch::new("notadict");
        let path = scratch.path().join("random.zip");
        write_dictionary(&path, &[("readme.txt", "not a dictionary".into())]);

        let error =
            Dictionary::import(&path, &scratch.path().join("out")).expect_err("should fail");
        assert!(matches!(error, DictError::NotADictionary), "got {error:?}");
        assert!(error.to_string().contains("index.json"));
    }

    /// A dictionary whose entries share characters, with popularity deliberately out of order.
    fn compound_dictionary(dir: &Path) -> PathBuf {
        let index = json!({
            "title": "Compounds",
            "format": 3,
            "revision": "2026-07-29",
            "author": "nobody",
            "attribution": "JMdict — EDRDG, CC BY-SA 4.0"
        });
        // 行 is in all four. The bank order is *not* the popularity order, so a test that
        // passes by accident of insertion order cannot.
        let bank = json!([
            ["徳行", "とくこう", "n", "", 3, ["virtuous conduct"], 1, ""],
            ["銀行", "ぎんこう", "n", "", 90, ["bank"], 2, ""],
            ["行く", "いく", "v5k-s", "v5k-s", 140, ["to go"], 3, ""],
            ["旅行", "りょこう", "n", "", 50, ["travel; trip"], 4, ""]
        ]);
        let path = dir.join("compounds.zip");
        write_dictionary(
            &path,
            &[
                ("index.json", index.to_string()),
                ("term_bank_1.json", bank.to_string()),
            ],
        );
        path
    }

    /// One word written four ways, plus an unrelated one, plus an entry with no sequence at all.
    ///
    /// Field 6 is the sequence. 走る/奔る/疾る share 1406970; アレ is filed as **negative** 1000580
    /// against あれ's positive one, which is how Jitendex marks a search-only form of the same
    /// word. 道 has its own sequence and must never be swept in; the last entry has sequence 0,
    /// which is the absence of an identity rather than one.
    fn spelling_dictionary(dir: &Path) -> PathBuf {
        let index = json!({
            "title": "Spellings",
            "format": 3,
            "revision": "2026-07-29",
            "author": "nobody",
            "attribution": "JMdict — EDRDG, CC BY-SA 4.0"
        });
        // Deliberately not in popularity order, so an accident of insertion cannot pass.
        let bank = json!([
            ["奔る", "はしる", "v5r", "", -101, ["to run"], 1406970, ""],
            ["走る", "はしる", "v5r", "", 200, ["to run"], 1406970, ""],
            ["疾る", "はしる", "v5r", "", -101, ["to run"], 1406970, ""],
            ["あれ", "あれ", "pn", "", 150, ["that"], 1000580, ""],
            ["アレ", "あれ", "pn", "", 5, ["that"], -1000580, ""],
            ["道", "みち", "n", "", 120, ["road"], 1454080, ""],
            ["むかし", "むかし", "n", "", 10, ["long ago"], 0, ""]
        ]);
        let path = dir.join("spellings.zip");
        write_dictionary(
            &path,
            &[
                ("index.json", index.to_string()),
                ("term_bank_1.json", bank.to_string()),
            ],
        );
        path
    }

    /// **A word's other spellings, commonest first.**
    ///
    /// Every other bucket is keyed by something a reader can type, so a lookup of 走る could only
    /// ever reach 走る. This one is keyed by the entry's identity, which is the only way the page
    /// can say "also written 奔る" — and the order is settled at build time so a caller takes the
    /// spellings worth showing rather than an arbitrary few.
    #[test]
    fn a_words_other_spellings_come_back_commonest_first() {
        let scratch = Scratch::new("spellings");
        let out = scratch.path().join("out");
        Dictionary::import(&spelling_dictionary(scratch.path()), &out).expect("import");
        let dictionary = dictionary_of(&out);

        assert!(dictionary.has_sequence(), "the importer builds the bucket");
        let spellings: Vec<String> = dictionary
            .same_entry(1406970, 10)
            .expect("same entry")
            .into_iter()
            .map(|entry| entry.term)
            .collect();
        assert_eq!(
            spellings,
            vec!["走る", "奔る", "疾る"],
            "the ordinary spelling leads and the rare ones follow, whatever order they were banked in"
        );
    }

    /// **The sign is dropped, and dropping it is the whole point.**
    ///
    /// Jitendex files a search-only form as the negation of the entry it belongs to. Keyed on the
    /// signed value, アレ and あれ would land in different posting lists — so a lookup of one
    /// would report that the other does not exist, which is the exact case this bucket is for.
    #[test]
    fn a_negatively_filed_form_is_the_same_word() {
        let scratch = Scratch::new("negative-sequence");
        let out = scratch.path().join("out");
        Dictionary::import(&spelling_dictionary(scratch.path()), &out).expect("import");
        let dictionary = dictionary_of(&out);

        for asked in [1000580, -1000580] {
            let spellings: Vec<String> = dictionary
                .same_entry(asked, 10)
                .expect("same entry")
                .into_iter()
                .map(|entry| entry.term)
                .collect();
            assert_eq!(
                spellings,
                vec!["あれ", "アレ"],
                "asked as {asked}, both signs name one word"
            );
        }
    }

    /// Sequence `0` is the absence of an identity, not one that everything shares.
    ///
    /// **Asserted against what the builder wrote, not what the reader returns.** `same_entry`
    /// refuses `0` on its own account, so a test that only asked it would pass while the bucket
    /// held a posting list naming every unnumbered entry in the dictionary — dead weight on disk
    /// that nothing could ever read.
    #[test]
    fn entries_without_a_sequence_are_not_all_the_same_word() {
        let scratch = Scratch::new("no-sequence");
        let out = scratch.path().join("out");
        Dictionary::import(&spelling_dictionary(scratch.path()), &out).expect("import");
        let dictionary = dictionary_of(&out);

        let zero = live(&out).join("sequence").join(sequence_shard_name("0"));
        if zero.exists() {
            let shard: Shard =
                serde_json::from_str(&fs::read_to_string(&zero).expect("read")).expect("parse");
            assert!(
                !shard.contains_key("0"),
                "or every entry the source left unnumbered would be one enormous word"
            );
        }
        assert!(
            dictionary.same_entry(0, 10).expect("same entry").is_empty(),
            "and the reader refuses it too"
        );

        let unrelated: Vec<String> = dictionary
            .same_entry(1454080, 10)
            .expect("same entry")
            .into_iter()
            .map(|entry| entry.term)
            .collect();
        assert_eq!(unrelated, vec!["道"], "a word with no siblings has none");
    }

    /// The backfill and the importer must agree, or an existing install gets a different index.
    #[test]
    fn a_backfill_builds_the_same_spellings_the_importer_would() {
        let scratch = Scratch::new("spellings-backfill");
        let out = scratch.path().join("out");
        Dictionary::import(&spelling_dictionary(scratch.path()), &out).expect("import");

        // Stand in for a dictionary imported before the bucket existed.
        fs::remove_dir_all(live(&out).join("sequence")).expect("remove the bucket");
        let dictionary = dictionary_of(&out);
        assert!(!dictionary.has_sequence());
        assert!(
            dictionary.needs_secondary_index(),
            "a missing bucket has to be worth a backfill even though the others are present"
        );

        dictionary.index_secondary().expect("backfill");
        let rebuilt = dictionary_of(&out);
        let spellings: Vec<String> = rebuilt
            .same_entry(1406970, 10)
            .expect("same entry")
            .into_iter()
            .map(|entry| entry.term)
            .collect();
        assert_eq!(spellings, vec!["走る", "奔る", "疾る"]);
    }

    /// **A character's compounds are its commonest, and the count is the whole list.**
    ///
    /// Ordered when the index is built, from the dictionary's own popularity score, so a query
    /// reads a few postings rather than all 1,369 of 行's and still leads with 行く and 銀行.
    /// Ranking at query time would mean reading everything; ranking on the frequency lists would
    /// mean "common" depended on an optional download.
    #[test]
    fn a_characters_compounds_lead_with_the_commonest() {
        let scratch = Scratch::new("compounds");
        let zip = compound_dictionary(scratch.path());
        let dictionary = Dictionary::import(&zip, &scratch.path().join("out")).expect("import");
        assert!(dictionary.has_kanji());

        let (hits, total) = dictionary.compounds("行", 3).expect("compounds");
        assert_eq!(
            total, 4,
            "the count is every entry written with it, not the page"
        );
        assert_eq!(
            hits.iter()
                .map(|hit| hit.entry.term.as_str())
                .collect::<Vec<_>>(),
            ["行く", "銀行", "旅行"],
            "commonest first, and the fourth is not read",
        );
        assert!(hits.iter().all(|hit| hit.matched == MatchKind::Compound));

        // Every kanji of a term, not just its first: 旅行 is under 旅 as well as 行.
        let (travel, _) = dictionary.compounds("旅", 8).expect("compounds");
        assert_eq!(
            travel
                .iter()
                .map(|hit| hit.entry.term.as_str())
                .collect::<Vec<_>>(),
            ["旅行"],
        );

        // A character nothing uses is empty rather than an error, as a missing shard always is.
        let (none, total) = dictionary.compounds("犬", 8).expect("compounds");
        assert!(none.is_empty());
        assert_eq!(total, 0);
    }

    /// A dictionary imported before this bucket existed backfills it with the glosses, in one
    /// pass — building them apart is how a backfilled index comes to differ from an imported one.
    #[test]
    fn the_backfill_builds_both_buckets_together() {
        let scratch = Scratch::new("compound-backfill");
        let zip = compound_dictionary(scratch.path());
        let out = scratch.path().join("out");
        let dictionary = Dictionary::import(&zip, &out).expect("import");
        drop(dictionary);

        fs::remove_dir_all(live(&out).join("kanji")).expect("drop the kanji bucket");
        fs::remove_dir_all(live(&out).join("gloss")).expect("drop the gloss bucket");
        let reopened = Dictionary::open(&out).expect("reopen");
        assert!(
            !reopened.has_kanji() && !reopened.has_glosses(),
            "both are gone"
        );

        reopened.index_secondary().expect("backfill");
        assert!(
            !reopened.has_kanji() && !reopened.has_glosses(),
            "the requesting reader stays on the generation it resolved"
        );
        let refreshed = Dictionary::open(&out).expect("open after the backfill");
        assert!(
            refreshed.has_kanji() && refreshed.has_glosses(),
            "one pass restored both"
        );
        let (hits, total) = refreshed.compounds("行", 2).expect("compounds");
        assert_eq!(total, 4);
        assert_eq!(
            hits[0].entry.term, "行く",
            "and in the same order as an import"
        );
    }
}
