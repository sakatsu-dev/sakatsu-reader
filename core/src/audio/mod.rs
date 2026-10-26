//! Word audio: recordings of a word being said (`project notes` §3).
//!
//! ## The source, and its caveat
//!
//! Audio comes from JapanesePod101's dictionary endpoint — the same one Yomichan and
//! Yomitan use. These are real human recordings, which is why it was chosen over
//! synthesised speech.
//!
//! It is worth being clear-eyed about what that is: an **undocumented third-party
//! endpoint with no licence grant to 屠つ**. It can change or disappear without
//! notice, and nothing here caches recordings to disk or redistributes them — audio is
//! fetched on demand, played, and forgotten. Everywhere else the project treats data
//! provenance carefully (`project notes` §8); this is the one place it depends on
//! somebody's server continuing to answer.
//!
//! ## The trap: "no recording" is still a 200
//!
//! Asking for a word with no recording does **not** produce a 404. It returns a fixed
//! placeholder MP3 — 52,288 bytes, md5 `7e2c2f954ef6051373ba916f000168dc` — with
//! `Content-Type: audio/mpeg` and status 200, exactly like a real hit. Without
//! recognising it, every word appears to have audio and the learner hears the same
//! clip forever. The md5 is checked rather than the length, because a length is a much
//! weaker fingerprint.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use md5::{Digest, Md5};
use unicode_normalization::UnicodeNormalization;

/// The endpoint. `kanji` is the written form, `kana` the reading.
const ENDPOINT: &str = "https://assets.languagepod101.com/dictionary/japanese/audiomp3.php";

/// The md5 of the "no recording" placeholder, confirmed by asking for words that
/// cannot exist. Yomitan pins the same value.
const PLACEHOLDER_MD5: &str = "7e2c2f954ef6051373ba916f000168dc";

/// The placeholder's size, for the `HEAD` probe in [`WordAudio::available`].
///
/// **A hint, never a verdict.** A length equal to this only means "possibly the placeholder",
/// and is confirmed by md5; a length different from it is conclusive on its own, because the
/// placeholder is a single fixed file. Used the other way round it would hide a real recording
/// that happened to be this long.
const PLACEHOLDER_BYTES: u64 = 52_288;

/// How many recordings to keep in memory. A few hundred KB at most, and it stops a
/// reader that revisits the same word hammering someone else's server.
const CACHE_LIMIT: usize = 256;
const TIMEOUT: Duration = Duration::from_secs(10);
const BODY_LIMIT: u64 = 5 * 1024 * 1024;

/// Why audio could not be fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// The word had neither a written form nor a reading to ask about.
    NothingToSay,
    /// The endpoint could not be reached.
    Unreachable(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioError::NothingToSay => write!(f, "no word to pronounce"),
            AudioError::Unreachable(message) => {
                write!(f, "could not reach the audio service: {message}")
            }
        }
    }
}

impl std::error::Error for AudioError {}

/// A word as asked about: how it is written, and how it is read.
type WordKey = (String, String);

/// A recording, or the knowledge that there is none.
type Recording = Option<Vec<u8>>;

/// Fetches word recordings, remembering what it has already asked for.
pub struct WordAudio {
    /// `None` records a word that has no recording, so a second look does not ask
    /// again — a miss is as worth caching as a hit.
    cache: Mutex<HashMap<WordKey, Recording>>,
    agent: ureq::Agent,
    pack_dir: Option<PathBuf>,
    local: RwLock<HashMap<WordKey, PathBuf>>,
    // Cache misses for one word are serialised without turning the four independent
    // availability workers into a single-file queue. A small fixed shard set is bounded,
    // needs no lock-map cleanup, and still guarantees identical keys share a gate.
    miss_gates: [Mutex<()>; 16],
}

impl Default for WordAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl WordAudio {
    pub fn new() -> WordAudio {
        Self::with_optional_pack(None)
    }

    pub fn with_pack(dir: PathBuf) -> WordAudio {
        Self::with_optional_pack(Some(dir))
    }

    fn with_optional_pack(pack_dir: Option<PathBuf>) -> WordAudio {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build()
            .into();
        let audio = WordAudio {
            cache: Mutex::new(HashMap::new()),
            agent,
            pack_dir,
            local: RwLock::new(HashMap::new()),
            miss_gates: std::array::from_fn(|_| Mutex::new(())),
        };
        audio.reload();
        audio
    }

    /// Re-read the optional pack after setup finishes, without restarting the app.
    pub fn reload(&self) {
        let fresh = self
            .pack_dir
            .as_deref()
            .map(load_local_index)
            .unwrap_or_default();
        *self
            .local
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = fresh;
    }

    pub fn local_len(&self) -> usize {
        self.local
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Whether the optional pack has this exact surface-and-reading pair.
    pub fn available_locally(&self, expression: &str, reading: &str) -> bool {
        self.local_path(expression, reading).is_some()
    }

    /// The recording for a word, or `None` when there is none.
    ///
    /// `expression` is how it is written, `reading` how it is read; either may be
    /// empty, but not both.
    pub fn fetch(&self, expression: &str, reading: &str) -> Result<Recording, AudioError> {
        let expression = expression.trim();
        let reading = reading.trim();
        if expression.is_empty() && reading.is_empty() {
            return Err(AudioError::NothingToSay);
        }

        if let Some(bytes) = self.local_recording(expression, reading) {
            return Ok(Some(bytes));
        }

        let key = (expression.to_string(), reading.to_string());
        if let Some(cached) = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
        {
            return Ok(cached.clone());
        }

        let _miss = self.miss_gates[gate_for(&key)]
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // A sibling availability worker may have answered this exact word while this caller
        // waited. The second check is what turns the mutex into a cache-miss gate.
        if let Some(cached) = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
        {
            return Ok(cached.clone());
        }

        let mut response = self
            .agent
            .get(ENDPOINT)
            .query("kanji", expression)
            .query("kana", reading)
            .call()
            .map_err(|error| AudioError::Unreachable(error.to_string()))?;
        if response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|kind| !kind.starts_with("audio/"))
        {
            return Err(AudioError::Unreachable(
                "the service returned something other than audio".into(),
            ));
        }

        let mut bytes = Vec::new();
        response
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT)
            .reader()
            .read_to_end(&mut bytes)
            .map_err(|error| AudioError::Unreachable(error.to_string()))?;

        let found = if is_placeholder(&bytes) {
            None
        } else {
            Some(bytes)
        };

        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Crude, but a reading session never approaches this and an unbounded cache
        // on a long-running window is worse than an occasional refetch.
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, found.clone());
        Ok(found)
    }

    /// Whether a word has a recording, **without downloading it**.
    ///
    /// This exists so the interface can decide whether to draw a pronunciation button before
    /// anybody presses one. Doing that with [`Self::fetch`] would mean pulling a whole clip per
    /// result — eighteen of them on an ordinary search — which is not a reasonable thing to do
    /// to somebody else's server to answer a question about a button.
    ///
    /// A `HEAD` costs headers and no body, and the endpoint answers one with a `Content-Length`.
    /// That is enough on its own in the common case: the "no recording" reply is one fixed file,
    /// so **any other length is a real recording**. Only a length that matches the placeholder
    /// exactly is ambiguous, and that is settled by fetching once and checking the md5 — the
    /// same test `fetch` uses, because a length is a much weaker fingerprint than a digest and
    /// a real clip of exactly that size must not be hidden.
    ///
    /// So the expensive path runs for words that have no recording, once ever, and the callers
    /// remember the answer.
    pub fn available(&self, expression: &str, reading: &str) -> Result<bool, AudioError> {
        let expression = expression.trim();
        let reading = reading.trim();
        if expression.is_empty() && reading.is_empty() {
            return Err(AudioError::NothingToSay);
        }

        if self.local_path(expression, reading).is_some() {
            return Ok(true);
        }

        // Already fetched this session: the answer is in hand, and asking again would be a
        // request whose reply is sitting in memory.
        if let Some(cached) = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(expression.to_string(), reading.to_string()))
        {
            return Ok(cached.is_some());
        }

        let response = self
            .agent
            .head(ENDPOINT)
            .query("kanji", expression)
            .query("kana", reading)
            .call()
            .map_err(|error| AudioError::Unreachable(error.to_string()))?;

        let length = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok());

        match length {
            // A length that is not the placeholder's cannot be the placeholder.
            Some(bytes) if bytes != PLACEHOLDER_BYTES => Ok(true),
            // Either the placeholder, or a recording that happens to be exactly its size. One
            // fetch settles it, and caches the clip for the press that may follow.
            Some(_) => Ok(self.fetch(expression, reading)?.is_some()),
            // No length to reason about — fall back to the certain answer.
            None => Ok(self.fetch(expression, reading)?.is_some()),
        }
    }

    fn local_recording(&self, expression: &str, reading: &str) -> Option<Vec<u8>> {
        let path = self.local_path(expression, reading)?;
        let bytes = std::fs::read(path).ok()?;
        // A damaged local clip is treated as uncovered, preserving the established remote
        // fallback instead of turning one corrupt file into a broken pronunciation button.
        looks_like_mp3(&bytes).then_some(bytes)
    }

    fn local_path(&self, expression: &str, reading: &str) -> Option<PathBuf> {
        let key = normalise_key(expression, reading)?;
        self.local
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
            .cloned()
    }
}

fn normalise_key(expression: &str, reading: &str) -> Option<WordKey> {
    // No spelling-only matches: homographs can have different recordings and the reading is
    // the disambiguator. The network fallback retains its older either-field behaviour.
    if expression.trim().is_empty() || reading.trim().is_empty() {
        return None;
    }
    let expression: String = expression.trim().nfc().collect();
    let reading: String = reading
        .trim()
        .nfc()
        .map(crate::kana::to_hiragana_char)
        .collect();
    Some((expression, reading))
}

fn gate_for(key: &WordKey) -> usize {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() as usize % 16
}

fn load_local_index(dir: &Path) -> HashMap<WordKey, PathBuf> {
    let Ok(index) = std::fs::read_to_string(dir.join("index.tsv")) else {
        return HashMap::new();
    };
    let mut found = HashMap::new();
    for line in index.lines() {
        let mut fields = line.split('\t');
        let (Some(expression), Some(reading), Some(file)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if fields.next().is_some() {
            continue;
        }
        let Some(key) = normalise_key(expression, reading) else {
            continue;
        };
        // The pack builder only emits flat numeric names. Enforce that again while loading so
        // a damaged or hand-edited index cannot make local playback read outside the pack.
        let file_path = Path::new(file);
        let safe_generated_name = file
            .strip_suffix(".mp3")
            .is_some_and(|stem| stem.len() == 6 && stem.bytes().all(|byte| byte.is_ascii_digit()));
        if file_path.components().count() != 1 || !safe_generated_name {
            continue;
        }
        let path = dir.join("mp3").join(file_path);
        if path.is_file() {
            found.entry(key).or_insert(path);
        }
    }
    found
}

fn looks_like_mp3(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3")
        || bytes
            .windows(2)
            .take(4096)
            .any(|pair| pair[0] == 0xff && pair[1] & 0xe0 == 0xe0)
}

/// Whether these bytes are the "no recording" clip rather than a word.
pub fn is_placeholder(bytes: &[u8]) -> bool {
    let digest = Md5::digest(bytes);
    crate::hashing::hex(digest) == PLACEHOLDER_MD5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_with_neither_spelling_nor_reading_is_refused_before_any_request() {
        let audio = WordAudio::new();
        assert_eq!(audio.fetch("   ", ""), Err(AudioError::NothingToSay));
    }

    #[test]
    fn the_placeholder_is_recognised_by_its_hash() {
        // The real placeholder is 52,288 zero-ish bytes of MP3; what matters here is
        // that the check is a hash of the content, not its length.
        assert!(!is_placeholder(b"a real recording"));
        assert!(!is_placeholder(&[]));
    }

    #[test]
    fn the_hash_used_is_md5_in_lowercase_hex() {
        // Pins the format of the constant, so a hashing change cannot silently make
        // every word look like it has audio.
        let digest = Md5::digest(b"");
        assert_eq!(
            crate::hashing::hex(digest),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(PLACEHOLDER_MD5.len(), 32);
        assert!(PLACEHOLDER_MD5.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn local_audio_requires_the_exact_surface_and_normalised_reading() {
        let dir = std::env::temp_dir().join("sakatsu-local-audio");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("mp3")).unwrap();
        std::fs::write(dir.join("mp3/000000.mp3"), b"ID3\x04\0\0recording").unwrap();
        std::fs::write(dir.join("index.tsv"), "上手\tじょうず\t000000.mp3\n").unwrap();
        let audio = WordAudio::with_pack(dir);

        assert_eq!(audio.local_len(), 1);
        assert!(audio.local_recording("上手", "ジョウズ").is_some());
        assert!(audio.local_recording("上手", "うわて").is_none());
        assert!(audio.local_recording("", "じょうず").is_none());
    }

    #[test]
    fn a_new_pack_is_adopted_without_reconstructing_audio_state() {
        let dir = std::env::temp_dir().join("sakatsu-local-audio-reload");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("mp3")).unwrap();
        let audio = WordAudio::with_pack(dir.clone());
        assert_eq!(audio.local_len(), 0);

        std::fs::write(dir.join("mp3/000000.mp3"), b"ID3\x04\0\0recording").unwrap();
        std::fs::write(dir.join("index.tsv"), "水\tみず\t000000.mp3\n").unwrap();
        audio.reload();
        assert_eq!(audio.local_len(), 1);
    }

    /// Hits the real endpoint. Ignored: it needs the network, and it depends on
    /// somebody else's server still being there.
    ///
    /// ```text
    /// cargo test -p sakatsu-core --lib audio -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs the network"]
    fn real_words_have_recordings_and_invented_ones_do_not() {
        let audio = WordAudio::new();

        let water = audio
            .fetch("水", "みず")
            .expect("fetch")
            .expect("水 has audio");
        println!("水 → {} bytes", water.len());
        assert!(water.len() > 1000, "suspiciously small: {}", water.len());
        assert!(!is_placeholder(&water));

        // A word that cannot exist: the endpoint answers 200 with the placeholder.
        let invented = audio.fetch("ヷヷヷヷ", "ヷヷヷヷ").expect("fetch");
        assert_eq!(
            invented, None,
            "the placeholder must not count as a recording"
        );

        // The second ask is served from memory rather than the network.
        let again = audio.fetch("水", "みず").expect("fetch").expect("cached");
        assert_eq!(again, water);
    }
}
