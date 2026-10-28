//! Everything the app can fetch for itself, and what it costs.
//!
//! # Why this exists as data
//!
//! 屠つ ships about 36 MB and needs roughly a gigabyte it cannot bundle (`project notes` §6).
//! Until now the only way to get that gigabyte was six PowerShell scripts, which is fine
//! for whoever built the app and impossible for anyone they hand it to — the installer
//! does not even carry the scripts, so the setup panel's advice named files that were not
//! there.
//!
//! This is the same information those scripts hold, in a form the app itself can act on.
//!
//! # The scripts are still the development path
//!
//! They are not being replaced, which means the two can disagree — and a URL that has
//! moved in one and not the other is exactly the kind of drift nobody notices until a
//! download 404s. `tests/catalog_matches_scripts.rs` parses the `.ps1` files and asserts
//! every URL here appears in the matching one.
//!
//! Every required artifact is pinned to release metadata and checksummed. KANJIDIC2 uses an
//! immutable mirror release: EDRDG republishes its original URL in place, which otherwise breaks
//! fresh installs even when this release's catalog has not changed.

use std::fmt;

/// How a piece is offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum Tier {
    /// The supported first-run installation. Offered as one informed, user-approved download.
    Essential,
    /// A real capability, plainly worth its size, but not needed to read.
    Optional,
    /// Behind a disclosure: either a choice a beginner has no basis to make, or a
    /// system-level setup such as Ichiran that deserves its own guided action.
    Advanced,
}

/// What a fetch does once the bytes have landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    /// Nothing; the file is used where it lands.
    Nothing,
    /// A Yomitan zip: import it into a sharded index, then build the gloss index.
    ImportDictionary,
    /// KANJIDIC2 plus basic KRADFILE and KanjiVG: build separate, attributed outputs.
    BuildKanjiDetails,
    /// Tofugu's open pronunciation archive: keep exact-keyed MP3 files and its licence.
    BuildAudioPack,
    /// The two WordNet halves: decompress, join, write the category table.
    BuildWordNetTable,
    /// A Yomitan frequency zip: unpack its meta banks where the ranking reads them.
    ///
    /// Not [`After::ImportDictionary`], which builds a sharded index of definitions. A
    /// frequency list has no definitions to index — it answers one question, "how common",
    /// and putting it through the word importer would file it as a dictionary and show its
    /// bare numbers where senses belong.
    UnpackFrequency,
    /// A llama.cpp release zip plus the weights beside it: unpack the binaries, leave the
    /// GGUF files where they landed.
    ///
    /// The only component that installs an executable. It is a plain unzip into a directory
    /// this app owns and starts from — no installer, no service registration, nothing on the
    /// PATH — which is the whole reason this engine stopped requiring Ollama.
    UnpackRuntime,
}

/// The vision reader: a llama.cpp build to run it, and the weights to run.
///
/// **Pinned to a release tag, and it has to be.** llama.cpp tags several builds a day — the
/// tag moved between two API calls while this was being written — so "latest" would mean the
/// app silently changing its own runtime, with the recorded size wrong the moment it did.
/// Moving to a newer build is an edit here, with a new size beside it.
///
/// Vulkan rather than CUDA: one 33 MB build covers NVIDIA, AMD and Intel with no toolkit and
/// no driver install, where CUDA is 238 MB plus a 373 MB runtime and works on one vendor. The
/// zip carries `llama-server.exe`, `mtmd.dll` for the vision projector, and `llama-quantize.exe`
/// for the machines whose Vulkan driver reports no BF16 support.
const PADDLE_VL: &[Artifact] = &[
    Artifact {
        url: "https://github.com/ggml-org/llama.cpp/releases/download/b10644/llama-b10644-bin-win-vulkan-x64.zip",
        file: "llama-vulkan.zip",
        min_bytes: 30 * MB,
        expected_bytes: Some(34_424_351),
        sha256: None,
    },
    Artifact {
        url: "https://huggingface.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF/resolve/d9c36e1e8bbaf35b59d539ad3276c52655b5d3e9/PaddleOCR-VL-For-Manga-BF16.gguf",
        file: "PaddleOCR-VL-For-Manga-BF16.gguf",
        min_bytes: 800 * MB,
        expected_bytes: Some(935_768_736),
        sha256: None,
    },
    Artifact {
        url: "https://huggingface.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF/resolve/d9c36e1e8bbaf35b59d539ad3276c52655b5d3e9/PaddleOCR-VL-For-Manga-mmproj-BF16.gguf",
        file: "PaddleOCR-VL-For-Manga-mmproj-BF16.gguf",
        min_bytes: 750 * MB,
        expected_bytes: Some(881_770_496),
        sha256: None,
    },
];

/// One file to fetch.
#[derive(Debug, Clone)]
pub struct Artifact {
    pub url: &'static str,
    /// What to call it on disk.
    pub file: &'static str,
    /// Refuse anything smaller. Catches a truncated download and an error page served
    /// with a 200, which is what actually goes wrong.
    pub min_bytes: u64,
    /// The exact size, where this release pins one.
    ///
    /// **A field rather than a `match self.file`, and that is the whole change.** These were two
    /// lookup tables sitting a hundred lines away from the artifacts they described, keyed on
    /// filename and disambiguated by `if self.url.contains(…)` when two artifacts shared one. So
    /// an artifact could not be asked what it expected without also knowing which URL it came
    /// from, and the tables could not be enumerated at all — which is how they came to hold two
    /// arms for a manga-ocr export this catalog had stopped shipping. Nothing could reach them
    /// and nothing said so.
    ///
    /// Beside the URL, none of that is possible: adding an artifact means writing what it weighs.
    pub expected_bytes: Option<u64>,
    /// The exact digest, where this release pins one.
    ///
    /// `None` is a real answer and not an oversight — see [`Artifact::verifiable`] and the test
    /// that says which artifacts are allowed to lack one.
    pub sha256: Option<&'static str>,
}

impl Artifact {
    /// The exact size this release pins for the artifact, if it pins one.
    pub fn expected_bytes(&self) -> Option<u64> {
        self.expected_bytes
    }

    /// The exact digest this release pins for the artifact, if it pins one.
    pub fn sha256(&self) -> Option<&'static str> {
        self.sha256
    }

    /// Whether a download of this artifact can be checked against an exact digest.
    ///
    /// **Named, so "no digest" is a stated property rather than a silence.** Eleven artifacts have
    /// no hash: the VLM weights and the llama.cpp build, whose publishers state none, and the
    /// FuguMT and optional-extra files served from a moving `main` where a digest would be a claim
    /// about a file that can change under it. Every one is still size-checked.
    /// `catalog_completeness.rs` lists exactly those eleven, so a twelfth cannot join them by
    /// accident — and none of them is `Tier::Essential`, which is what
    /// [`offline_package_catalog`] relies on.
    pub fn verifiable(&self) -> bool {
        self.sha256.is_some()
    }
}

/// One independently licensed upstream inside a compound setup component.
///
/// Most components have one source and continue using `licence` plus `home`. Kanji details is
/// deliberately one setup action with three separately licensed artifacts, so flattening their
/// attribution into a sentence would make only the first project linkable.
#[derive(Debug, Clone, Copy)]
pub struct Source {
    pub name: &'static str,
    pub licence: &'static str,
    pub home: &'static str,
}

const KANJI_SOURCES: &[Source] = &[
    Source {
        name: "KANJIDIC2",
        licence: "EDRDG, CC BY-SA 4.0",
        home: "https://www.edrdg.org/wiki/index.php/KANJIDIC_Project",
    },
    Source {
        name: "KanjiVG r20260714",
        licence: "Ulrich Apel, CC BY-SA 3.0",
        home: "https://github.com/KanjiVG/kanjivg/releases/tag/r20260714",
    },
    Source {
        name: "basic KRADFILE",
        licence: "EDRDG licence",
        home: "https://www.edrdg.org/krad/kradinf.html",
    },
];

/// One thing the setup screen offers.
#[derive(Debug, Clone)]
pub struct Component {
    /// Stable id the UI passes back to `fetch_component`.
    pub id: &'static str,
    pub label: &'static str,
    /// One line, in terms of what the learner gets — not what the file is.
    pub about: &'static str,
    /// Roughly what it costs to download, for saying so *before* the click.
    pub bytes: u64,
    /// Travels with the data and must be shown (`project notes` §8).
    pub licence: &'static str,
    /// Where the data comes from — the project's own page, not the download URL.
    ///
    /// **A link, because for some of this a link is a licence condition rather than a courtesy.**
    /// `project notes` §8 says so of Japanese WordNet in as many words, and the CC BY-SA rows want
    /// attribution that leads somewhere. The About page renders these; `licence` alone named the
    /// rights holder without saying where to find them.
    pub home: &'static str,
    pub tier: Tier,
    /// Where the files go is **not** here. The shell already knows — `model_dir()`,
    /// `dictionaries_dir()` and friends in `src-tauri/src/lib.rs`, each honouring its own
    /// environment override — and a second copy of that mapping is a second thing to get
    /// wrong, which it promptly was: two of the three model directories written here did
    /// not match the ones the app actually reads.
    pub artifacts: &'static [Artifact],
    pub after: After,
    /// Set for a component installed through a guided system script rather than fetched.
    pub script: Option<&'static str>,
}

impl Component {
    /// Independently attributed sources inside a compound component.
    pub fn sources(&self) -> &'static [Source] {
        match self.id {
            "kanji" => KANJI_SOURCES,
            _ => &[],
        }
    }

    /// Whether the app has to pick this up before it is of any use.
    ///
    /// Derived from what happens after the download rather than listed by hand, so it
    /// cannot drift from the truth. Everything read at startup has to be read again; the
    /// recognition and translation models are loaded on first use, and a frequency list is
    /// swapped in place when the setting that names it changes, so those do not.
    ///
    /// **This used to be `needs_restart`, and the difference is the whole point.** The same
    /// three components, but what they ask for is now a reload rather than a trip through the
    /// app's front door: `commands::adopt` replaces them in place the moment the fetch finishes,
    /// so this says which ones it has work to do for. Asking a reader to restart is asking them
    /// to finish an operation the app could finish itself, and until they did it a successful
    /// download was indistinguishable from one that had achieved nothing.
    ///
    /// The screen also used to say "restart to start using them" after *any* fetch, which sent
    /// people to reopen the app for a model that was already usable — the most irritating
    /// way to be wrong, because the cost lands entirely on someone who did as they were
    /// told.
    pub fn needs_reload(&self) -> bool {
        matches!(
            self.after,
            After::ImportDictionary
                | After::BuildKanjiDetails
                | After::BuildWordNetTable
                | After::BuildAudioPack
        )
    }

    /// Whether the ordinary file downloader can fetch this.
    ///
    /// False only for Ichiran. Its WSL, Docker and PostgreSQL work is launched through
    /// a separate guided action, not disguised as the same kind of Get button used for
    /// an archive the app can download and unpack itself.
    pub fn fetchable(&self) -> bool {
        !self.artifacts.is_empty()
    }
}

impl fmt::Display for Component {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.label, human_size(self.bytes))
    }
}

/// A size a person can judge, which is the only reason it is shown.
pub fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (MB * 1024.0))
    } else {
        format!("{} MB", (bytes as f64 / MB).round() as u64)
    }
}

const MB: u64 = 1024 * 1024;

/// Everything, in the order the setup screen shows it.
pub fn all() -> &'static [Component] {
    &[
        Component {
            id: "dictionaries",
            label: "Dictionaries",
            about: "Definitions, readings and example sentences. Without these no word \
                    can be looked up at all.",
            bytes: 50 * MB,
            licence: "Jitendex and JMnedict — CC BY-SA 4.0, built on JMdict (EDRDG)",
            home: "https://jitendex.org",
            tier: Tier::Essential,
            artifacts: &[
                Artifact {
                    url: "https://github.com/stephenmk/stephenmk.github.io/releases/download/2026.10.03.0/jitendex-yomitan.zip",
                    file: "jitendex-yomitan.zip",
                    min_bytes: 20 * MB,
                    expected_bytes: Some(38_831_822),
                    sha256: Some("e914c76596220dffc6ee0c069cd05992f03adb2988c12c3cebe817a3ac271356"),
                },
                Artifact {
                    url: "https://github.com/yomidevs/jmdict-yomitan/releases/download/2026-10-18/JMnedict.zip",
                    file: "JMnedict.zip",
                    min_bytes: 5 * MB,
                    expected_bytes: Some(11_429_637),
                    sha256: Some("01a02baed0227135a4ef091a2ed058b9f0ba1168a45b59198a919cd6aced50ef"),
                },
            ],
            after: After::ImportDictionary,
            script: Some("fetch-dictionaries.ps1"),
        },
        Component {
            id: "kanji",
            label: "Kanji details",
            about: "Readings, meanings, name readings, visual components and stroke-order \
                    diagrams, with the reading a word actually uses picked out.",
            bytes: 15 * MB,
            licence: "KANJIDIC2 — EDRDG, CC BY-SA 4.0; KanjiVG — Ulrich Apel, CC BY-SA 3.0; basic KRADFILE — EDRDG licence",
            home: "https://www.edrdg.org/wiki/index.php/KANJIDIC_Project",
            tier: Tier::Essential,
            artifacts: &[
                // EDRDG replaces its original URL in place. This immutable mirror retains the
                // exact 2026-10-18 bytes, so existing caches and offline packages remain valid.
                // A newer snapshot needs a separate release and a deliberate catalog refresh.
                Artifact {
                    url: "https://github.com/sakatsu-dev/reader-resources/releases/download/kanjidic2-2026-10-18/kanjidic2-20261018.xml.gz",
                    file: "kanjidic2.xml.gz",
                    min_bytes: MB,
                    expected_bytes: Some(1_488_573),
                    sha256: Some("6182a700e08a8f6a6646529b77f84f0d963ed33cd5c7516ff19e4ee642345446"),
                },
                Artifact {
                    url: "https://github.com/KanjiVG/kanjivg/releases/download/r20260714/kanjivg-20260714-main.zip",
                    file: "kanjivg-20260714-main.zip",
                    min_bytes: 8 * MB,
                    expected_bytes: Some(12_650_667),
                    sha256: Some("b5df6cd2bc249dd49b8041eb99e38ba9f8dc6b9ea57962a80084b4a315d1a7fc"),
                },
                Artifact {
                    // The archive host currently presents a certificate for another name on
                    // Windows. This official HTTP endpoint is therefore paired with an exact
                    // SHA-256 above; altered transport bytes cannot reach the parser.
                    url: "http://ftp.edrdg.org/pub/Nihongo/kradfile.gz",
                    file: "kradfile.gz",
                    min_bytes: 32 * 1024,
                    expected_bytes: Some(47_301),
                    sha256: Some("0c5487c1de77e36ffb5bde652f469f2de9c52efc8320137c115506f3500e9c5f"),
                },
            ],
            after: After::BuildKanjiDetails,
            script: Some("fetch-kanjidic.ps1"),
        },
        Component {
            id: "wordnet",
            label: "Particle senses",
            about: "Sharpens which sense a particle has — whether で marks a place or a \
                    means — by knowing what kind of thing the noun before it is.",
            bytes: 30 * MB,
            licence: "Japanese WordNet — NICT and Francis Bond; Princeton WordNet 3.0. \
                      Both BSD-like, attribution required.",
            home: "https://bond-lab.github.io/wnja/",
            tier: Tier::Essential,
            artifacts: &[
                Artifact {
                    url: "https://github.com/bond-lab/wnja/releases/download/v1.1/wnjpn-ok.tab.gz",
                    file: "wnjpn-ok.tab.gz",
                    min_bytes: MB,
                    expected_bytes: Some(1_079_974),
                    sha256: Some("770a3779425b8565d52db9900b49f2ded6526f71b3b996b8f78fb58b5cf52459"),
                },
                Artifact {
                    url: "https://wordnetcode.princeton.edu/3.0/WNdb-3.0.tar.gz",
                    file: "WNdb-3.0.tar.gz",
                    min_bytes: 5 * MB,
                    expected_bytes: Some(10_518_425),
                    sha256: Some("658b1ba191f5f98c2e9bae3e25c186013158f30ef779f191d2a44e5d25046dc8"),
                },
            ],
            after: After::BuildWordNetTable,
            script: Some("fetch-wordnet.ps1"),
        },
        Component {
            id: "manga-ocr",
            label: "manga-ocr",
            about: "The manga specialist Auto needs for vertical text, furigana and hand \
                    lettering. Also directly selectable for difficult captures.",
            // 111 MiB against the fp32 export's 461, and 27% faster to read with.
            //
            // **Measured before it was chosen.** `core/tests/ocr_corpus.rs` scored both against
            // 141 real captures: fp32 0.1530 mean character error, uint8 0.1561 — 2% relative,
            // and uint8 read one *more* sample perfectly. They differ on 17 samples, uint8
            // better on 6 and worse on 11, so the divergence is noise rather than a regression.
            //
            // The earlier comparison in `quantisation.rs` could only ask whether a variant reads
            // what fp32 reads, because there was no ground truth to ask anything else against.
            bytes: 112 * MB,
            licence: "manga-ocr — Apache-2.0",
            home: "https://github.com/kha-white/manga-ocr",
            tier: Tier::Essential,
            artifacts: &[
                // **The quantised export, and the weights are the only part that changes.**
                //
                // `onnx-community` publishes manga-ocr at eight precisions but ships no
                // `vocab.txt`, so the four small files below still come from `mayocream` — the
                // same mix `scripts/fetch-manga-ocr-variants.ps1` assembles, which is what was
                // measured. Saved under the plain names the loader expects.
                Artifact {
                    url: "https://huggingface.co/onnx-community/manga-ocr-base-ONNX/resolve/f9023406bb2f6b17df67bc4a327c56ecd20611f0/onnx/encoder_model_uint8.onnx",
                    file: "encoder_model.onnx",
                    min_bytes: 70 * MB,
                    expected_bytes: Some(86_967_805),
                    sha256: Some("a73e7a9959f3412f4d0ab60c8cd0f71c29e7e29a2e52a1e184ad6f2be3b892e3"),
                },
                Artifact {
                    url: "https://huggingface.co/onnx-community/manga-ocr-base-ONNX/resolve/f9023406bb2f6b17df67bc4a327c56ecd20611f0/onnx/decoder_model_uint8.onnx",
                    file: "decoder_model.onnx",
                    min_bytes: 20 * MB,
                    expected_bytes: Some(29_627_952),
                    sha256: Some("cc7a42534759864c7b6937aaacc4cc91b37c9207eeae05ee359a04e6d4d222a5"),
                },
                Artifact {
                    url: "https://huggingface.co/mayocream/manga-ocr-onnx/resolve/24b12778d85800835e2ca409236de281b8ab7b9f/vocab.txt",
                    file: "vocab.txt",
                    min_bytes: 1024,
                    expected_bytes: Some(30_216),
                    sha256: Some("5cb5c5586d98a2f331d9f8828e4586479b0611bfba5d8c3b6dadffc84d6a36a3"),
                },
                Artifact {
                    url: "https://huggingface.co/mayocream/manga-ocr-onnx/resolve/24b12778d85800835e2ca409236de281b8ab7b9f/config.json",
                    file: "config.json",
                    min_bytes: 64,
                    expected_bytes: Some(77_161),
                    sha256: Some("fda165bab7a9cd1b4a0ce57ed9a78e516f101fcd23afd9f58c9e779c9b5ed11b"),
                },
                Artifact {
                    url: "https://huggingface.co/mayocream/manga-ocr-onnx/resolve/24b12778d85800835e2ca409236de281b8ab7b9f/generation_config.json",
                    file: "generation_config.json",
                    min_bytes: 16,
                    expected_bytes: Some(273),
                    sha256: Some("012bcb0da9625702d55556b6ed20b1da39de5eba707c5c9ee1575336cfc2a804"),
                },
                Artifact {
                    url: "https://huggingface.co/mayocream/manga-ocr-onnx/resolve/24b12778d85800835e2ca409236de281b8ab7b9f/preprocessor_config.json",
                    file: "preprocessor_config.json",
                    min_bytes: 16,
                    expected_bytes: Some(376),
                    sha256: Some("ee6588147df4bcd5ca13110900697f7b43b64f07da18a4427fa0d88eae857326"),
                },
            ],
            after: After::Nothing,
            script: Some("fetch-ocr-model.ps1"),
        },
        Component {
            id: "translation",
            label: "Translation",
            about: "A rough English gist on request. Never automatic — the point is to \
                    read the Japanese first.",
            bytes: 310 * MB,
            licence: "FuguMT — CC BY-SA 4.0",
            home: "https://huggingface.co/staka/fugumt-ja-en",
            tier: Tier::Optional,
            artifacts: &[
                Artifact {
                    url: "https://huggingface.co/Kadonox/fugumt-ja-en-onnx/resolve/9b09cd7e6263223ac4a5988da1d881ef02b55be8/onnx/encoder_model.onnx",
                    file: "encoder_model.onnx",
                    min_bytes: 100 * MB,
                    expected_bytes: Some(142_366_928),
                    sha256: Some("ce16e5f7cf1fde5dffbae8ed5a9c8d915de8084edc34ce5549a8e7c7271a5d2e"),
                },
                Artifact {
                    url: "https://huggingface.co/Kadonox/fugumt-ja-en-onnx/resolve/9b09cd7e6263223ac4a5988da1d881ef02b55be8/onnx/decoder_model.onnx",
                    file: "decoder_model.onnx",
                    min_bytes: 100 * MB,
                    expected_bytes: Some(167_896_798),
                    sha256: Some("f3fef884dc2bcaba1e3a6f844261ad31be84b433487156d2c11c0e8b297c15ae"),
                },
                Artifact {
                    url: "https://huggingface.co/Kadonox/fugumt-ja-en-onnx/resolve/9b09cd7e6263223ac4a5988da1d881ef02b55be8/tokenizer.json",
                    file: "tokenizer.json",
                    min_bytes: MB,
                    expected_bytes: Some(3_169_716),
                    sha256: Some("e0bb9bef12bb06c118b2bcf7e6ec5975a0dfa3d89191df6cc331767b1643af78"),
                },
                Artifact {
                    url: "https://huggingface.co/Kadonox/fugumt-ja-en-onnx/resolve/9b09cd7e6263223ac4a5988da1d881ef02b55be8/generation_config.json",
                    file: "generation_config.json",
                    min_bytes: 16,
                    expected_bytes: Some(289),
                    sha256: Some("23a8b82900404c37f3f85170507dbc929dc3a6be1ba24fe340d9336d1cf841da"),
                },
                Artifact {
                    url: "https://huggingface.co/Kadonox/fugumt-ja-en-onnx/resolve/9b09cd7e6263223ac4a5988da1d881ef02b55be8/config.json",
                    file: "config.json",
                    min_bytes: 64,
                    expected_bytes: Some(1_045),
                    sha256: Some("24e201e35f20db82adcd87924d59398bf986560a46532d01fb2ae19cf606f17f"),
                },
            ],
            after: After::Nothing,
            script: Some("fetch-translation-model.ps1"),
        },
        Component {
            id: "pronunciation",
            label: "Offline pronunciation",
            about: "Human Japanese vocabulary recordings stored on this device. Exact word \
                    and reading matches play locally; uncovered words keep the current fallback.",
            bytes: 90 * MB,
            licence: "Tofugu and WaniKani Japanese vocabulary pronunciation audio — CC BY-SA 4.0",
            home: "https://github.com/tofugu/japanese-vocabulary-pronunciation-audio",
            tier: Tier::Optional,
            artifacts: &[Artifact {
                url: "https://github.com/tofugu/japanese-vocabulary-pronunciation-audio/archive/refs/heads/master.zip",
                file: "tofugu-pronunciation-master.zip",
                min_bytes: 10 * MB,
                expected_bytes: None,
                sha256: None,
            }],
            after: After::BuildAudioPack,
            script: Some("fetch-pronunciation.ps1"),
        },
        Component {
            id: "frequency-jpdb",
            label: "Frequency — JPDB",
            about: "Orders search results by how common a word is across anime, visual \n                    novels and novels — the closest to what this app is usually pointed \n                    at.",
            bytes: 6 * MB,
            licence: "JPDB v2.2 frequency list — compiled from jpdb.io",
            home: "https://jpdb.io",
            tier: Tier::Optional,
            artifacts: &[Artifact {
                url: "https://github.com/Kuuuube/yomitan-dictionaries/raw/main/dictionaries/JPDB_v2.2_Frequency_Kana_2024-10-13.zip",
                file: "jpdb-frequency.zip",
                min_bytes: MB,
                expected_bytes: None,
                sha256: None,
            }],
            after: After::UnpackFrequency,
            script: Some("fetch-frequency-jpdb.ps1"),
        },
        // **PP-OCRv6 small, since 2026-10-09.** It replaced PP-OCRv5 under the same id, so setup
        // state and the offline package keep one name for "the typeset reader": on the capture
        // corpus v6 small read with less than half v5's character error in a third of the time
        // (`local documentation`). PaddlePaddle publishes v6 in Paddle format only; this is
        // snowfluke's ONNX conversion of the official weights, checked against PaddlePaddle's
        // own inference before it was adopted.
        Component {
            id: "paddle-ocr",
            label: "PP-OCRv6",
            about: "The typeset specialist Auto uses for fast game and menu text, and for \
                    the layout evidence that decides when manga-ocr should also read.",
            bytes: 30 * MB,
            licence: "PP-OCRv6 — Apache-2.0 (PaddlePaddle); ONNX conversion by snowfluke — Apache-2.0",
            home: "https://github.com/PaddlePaddle/PaddleOCR",
            tier: Tier::Essential,
            artifacts: &[
                Artifact {
                    url: "https://huggingface.co/snowfluke/ppu-paddle-ocr-models/resolve/bf1d5edb0335d3262be7caf13f766ba274b4cadd/detection/PP-OCRv6_small_det.onnx",
                    file: "det.onnx",
                    min_bytes: 5 * MB,
                    expected_bytes: Some(9_880_512),
                    sha256: Some(
                        "d73e0058b7a8086bbd57f3d10b8bcd4ff95363f67e06e2762b5e814fe9c9410e",
                    ),
                },
                Artifact {
                    url: "https://huggingface.co/snowfluke/ppu-paddle-ocr-models/resolve/bf1d5edb0335d3262be7caf13f766ba274b4cadd/recognition/PP-OCRv6_small_rec.onnx",
                    file: "rec.onnx",
                    min_bytes: 10 * MB,
                    expected_bytes: Some(21_159_378),
                    sha256: Some("5435fd747c9e0efe15a96d0b378d5bd157e9492ed8fd80edf08f30d02fa24634"),
                },
                Artifact {
                    url: "https://huggingface.co/snowfluke/ppu-paddle-ocr-models/resolve/bf1d5edb0335d3262be7caf13f766ba274b4cadd/recognition/ppocrv6_dict.txt",
                    file: "ppocrv6_dict.txt",
                    min_bytes: 1024,
                    expected_bytes: Some(74_948),
                    sha256: Some("41557512862dfe31970cf22407742b629725461dd84c0d8771bde9c87c2202c8"),
                },
            ],
            after: After::Nothing,
            script: Some("fetch-paddle-ocr.ps1"),
        },
        Component {
            id: "paddle-vl",
            label: "PaddleOCR-VL (manga)",
            about: "An optional vision model for difficult captures, fine-tuned on manga. Guided \
                    setup installs a native local runtime and keeps the endpoint details out of \
                    the ordinary path. Reads best of anything measured here, and wants a GPU.",
            // The official GGUF repository contains a 936 MB model and an 882 MB vision
            // projector. The llama.cpp build is 33 MB beside them; the weights are the cost.
            bytes: 1_770 * MB,
            licence: "PaddleOCR-VL-For-Manga — Apache-2.0; llama.cpp — MIT",
            home: "https://huggingface.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF",
            tier: Tier::Advanced,
            artifacts: PADDLE_VL,
            after: After::UnpackRuntime,
            // **No script.** This used to launch `setup-vlm.ps1`, which installed Ollama
            // system-wide and left a tray application behind. It is an ordinary download now,
            // through the same screen every other component uses.
            script: None,
        },
        // No artifacts, so no Get button — see `Component::fetchable`.
        Component {
            id: "frequency-bccwj",
            label: "Frequency — BCCWJ",
            about: "Orders search results by NINJAL's balanced corpus — books,                     magazines, newspapers, white papers and blogs. The widest coverage, and                     the one to cite.",
            bytes: 19 * MB,
            licence: "BCCWJ frequency list — derived from NINJAL's Balanced Corpus of                       Contemporary Written Japanese",
            home: "https://clrd.ninjal.ac.jp/bccwj/en/",
            tier: Tier::Advanced,
            artifacts: &[Artifact {
                url: "https://github.com/Kuuuube/yomitan-dictionaries/raw/main/dictionaries/BCCWJ_SUW_LUW_combined.zip",
                file: "bccwj-frequency.zip",
                min_bytes: 4 * MB,
                expected_bytes: None,
                sha256: None,
            }],
            after: After::UnpackFrequency,
            script: Some("fetch-frequency-bccwj.ps1"),
        },
        Component {
            id: "ichiran",
            label: "Ichiran analyzer",
            about: "Recommended main analyzer for words, readings and grammar. Lindera assists \
                    in the background and remains available if Ichiran cannot answer. \
                    Guided setup needs WSL, Docker and a local database.",
            // `human_size` reports binary gigabytes. 4.7 × 1024 MiB keeps the UI's
            // deliberately approximate estimate consistent with upstream's 4.7 GB.
            bytes: 4_813 * MB,
            licence: "Ichiran — MIT",
            home: "https://github.com/tshatrov/ichiran",
            tier: Tier::Advanced,
            artifacts: &[],
            after: After::Nothing,
            script: Some("setup-ichiran.ps1"),
        },
    ]
}

/// One component by id.
pub fn find(id: &str) -> Option<&'static Component> {
    all().iter().find(|component| component.id == id)
}

/// What "Get the essentials" fetches, and what that costs.
pub fn essentials() -> (Vec<&'static Component>, u64) {
    let chosen: Vec<&'static Component> = all()
        .iter()
        .filter(|component| component.tier == Tier::Essential)
        .collect();
    let bytes = chosen.iter().map(|component| component.bytes).sum();
    (chosen, bytes)
}

/// The offline-package catalog, generated from [`all`].
///
/// # Why this is generated rather than maintained
///
/// `scripts/offline-essentials-catalog.json` restates every essential artifact's filename, size,
/// digest and licence. It was written by hand and checked against the catalog by a test — which
/// caught drift, but only after somebody had produced it. Deriving it means the two cannot
/// disagree in the first place, and the test becomes an up-to-date check in the shape of
/// `cargo fmt --check`: it says "run the generator", not "go and reconcile two lists".
///
/// **`package_version` is the one field with no source here**, so it is passed in. It names a
/// build of the offline package rather than anything about the catalog, and inventing a value for
/// it from the catalog's contents would be a version number that changed for the wrong reasons.
///
/// An essential artifact with no pinned size or digest is an error rather than an omission: the
/// package's whole purpose is verified offline installation, and a row that cannot be verified
/// would defeat it silently. `core/tests/catalog_completeness.rs` records which artifacts lack a
/// digest, and none of them is essential.
pub fn offline_package_catalog(package_version: &str) -> Result<String, String> {
    let mut components = Vec::new();
    for component in all().iter().filter(|c| c.tier == Tier::Essential) {
        let mut artifacts = Vec::new();
        for artifact in component.artifacts {
            let bytes = artifact.expected_bytes().ok_or_else(|| {
                format!(
                    "{}/{} is essential but has no pinned size; an offline package cannot verify \
                     it",
                    component.id, artifact.file
                )
            })?;
            let sha256 = artifact.sha256().ok_or_else(|| {
                format!(
                    "{}/{} is essential but has no pinned digest; an offline package cannot \
                     verify it",
                    component.id, artifact.file
                )
            })?;
            artifacts.push(format!(
                "        {{ \"file\": \"{}\", \"bytes\": {bytes}, \"sha256\": \"{sha256}\" }}",
                artifact.file
            ));
        }
        components.push(format!(
            "    {{\n      \"id\": \"{}\",\n      \"licences\": [\"{}\"],\n      \
             \"artifacts\": [\n{}\n      ]\n    }}",
            component.id,
            component.licence.replace('\\', "\\\\").replace('"', "\\\""),
            artifacts.join(",\n")
        ));
    }
    Ok(format!(
        "{{\n  \"package_version\": \"{package_version}\",\n  \"components\": [\n{}\n  ]\n}}\n",
        components.join(",\n")
    ))
}

#[cfg(test)]
mod tests {

    /// The three read at startup have to be read again; nothing else does.
    ///
    /// Named one by one rather than re-deriving from `After`, which would only assert that
    /// the function is itself — the point is that *these* are the components the app has to
    /// take up after a fetch, and that adding a fourth is a decision somebody makes here
    /// rather than a consequence they discover.
    ///
    /// **`commands::adopt` matches this list arm for arm**, and that is what the pairing is
    /// for: a component that answers true here and is missing there would report itself ready
    /// while every lookup in it still found nothing.
    #[test]
    fn only_what_is_read_at_startup_asks_to_be_reloaded() {
        let wants: Vec<&str> = all()
            .iter()
            .filter(|component| component.needs_reload())
            .map(|component| component.id)
            .collect();
        assert_eq!(wants, ["dictionaries", "kanji", "wordnet", "pronunciation"]);
    }

    /// A fetch the app has to take up says so, and one it does not must not — there is no
    /// work to schedule for a model that is read on first use anyway.
    #[test]
    fn the_lazily_loaded_things_do_not() {
        for id in [
            "manga-ocr",
            "translation",
            "paddle-ocr",
            "paddle-vl",
            "frequency-jpdb",
        ] {
            let component = all()
                .iter()
                .find(|component| component.id == id)
                .unwrap_or_else(|| panic!("{id} is in the catalog"));
            assert!(
                !component.needs_reload(),
                "{id} is loaded on demand, so there is nothing to reload"
            );
        }
    }

    /// The first screen someone sees lists the optional tier in catalog order, so the
    /// order is a decision rather than an accident of what was added last.
    #[test]
    fn the_optional_tier_leads_with_what_the_app_is_for() {
        let optional: Vec<&str> = all()
            .iter()
            .filter(|component| component.tier == Tier::Optional)
            .map(|component| component.id)
            .collect();
        assert_eq!(optional, ["translation", "pronunciation", "frequency-jpdb"]);
    }
    use super::*;

    #[test]
    fn every_component_says_what_it_is_and_what_it_costs() {
        for component in all() {
            assert!(!component.id.is_empty());
            assert!(!component.label.is_empty(), "{}", component.id);
            // Shown before the click, so it has to be worth reading.
            assert!(
                component.about.len() > 40,
                "{} needs a real description",
                component.id
            );
            // Attribution is a licence condition for most of these, not a nicety — and for
            // Japanese WordNet the link is named in the terms, so it is checked like the rest of
            // the sentence rather than left to whoever adds the next component.
            assert!(!component.licence.is_empty(), "{}", component.id);
            assert!(
                component.home.starts_with("http"),
                "{} needs a link to where the data comes from, not {:?}",
                component.id,
                component.home
            );
            assert!(component.bytes > 0, "{}", component.id);
            for source in component.sources() {
                assert!(!source.name.is_empty(), "{}", component.id);
                assert!(!source.licence.is_empty(), "{}", source.name);
                assert!(source.home.starts_with("https://"), "{}", source.home);
            }
            if component.fetchable() {
                for artifact in component.artifacts {
                    assert!(
                        artifact.url.starts_with("https://")
                            || (artifact.file == "kradfile.gz"
                                && artifact.url.starts_with("http://ftp.edrdg.org/")),
                        "{}",
                        artifact.url
                    );
                    assert!(!artifact.file.is_empty());
                }
            }
        }

        let mut ids: Vec<&str> = all().iter().map(|c| c.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate component id");
    }

    /// Ichiran is the last component that needs a guided system action.
    ///
    /// **`paddle-vl` used to be the other one**, because it installed Ollama — a system-wide
    /// runtime with a tray application and a startup entry, which no download screen can
    /// honestly represent as a progress bar. It fetches a llama.cpp build into a directory the
    /// app owns instead, so it is an ordinary download like everything else here, and the
    /// guided list is down to one.
    #[test]
    fn ichiran_is_the_only_component_that_still_needs_a_guided_system_action() {
        let ichiran = find("ichiran").expect("guided component");
        assert!(!ichiran.fetchable());
        assert!(ichiran.script.is_some());
        assert_eq!(ichiran.tier, Tier::Advanced);

        // Every other component, `paddle-vl` included, is a direct app-managed download.
        for component in all().iter().filter(|c| c.id != "ichiran") {
            assert!(
                component.fetchable(),
                "{} should be fetchable",
                component.id
            );
        }

        // `script` is not the marker for this. Most components name a `fetch-*.ps1` as the
        // manual alternative to the download screen, and having one says nothing about whether
        // the app can install them itself — `fetchable` is what does.
        assert!(find("paddle-vl").expect("paddle-vl").script.is_none());
    }

    /// The vision reader ships a runtime as well as weights, which nothing else here does.
    ///
    /// Asserted because the executable is the part that makes this component different, and a
    /// silent drop of it would leave the weights on disk with nothing able to read them.
    #[test]
    fn the_vision_reader_fetches_a_runtime_alongside_its_weights() {
        let component = find("paddle-vl").expect("paddle-vl");
        assert_eq!(component.after, After::UnpackRuntime);

        let files: Vec<&str> = component.artifacts.iter().map(|a| a.file).collect();
        assert_eq!(
            files,
            [
                "llama-vulkan.zip",
                "PaddleOCR-VL-For-Manga-BF16.gguf",
                "PaddleOCR-VL-For-Manga-mmproj-BF16.gguf"
            ]
        );

        // Pinned, both of them: a floating llama.cpp tag would change the app's own runtime
        // without an edit here, and the tag moves several times a day.
        assert!(
            component.artifacts[0].url.contains("/download/b10644/"),
            "the runtime must name a release tag"
        );
        for artifact in component.artifacts.iter().skip(1) {
            assert!(
                artifact.url.contains("/resolve/d9c36e1e"),
                "the weights must name a commit"
            );
        }
    }

    #[test]
    fn the_essentials_include_lookup_and_both_complementary_ocr_engines() {
        let (chosen, bytes) = essentials();
        let ids: Vec<&str> = chosen.iter().map(|c| c.id).collect();
        assert_eq!(
            ids,
            [
                "dictionaries",
                "kanji",
                "wordnet",
                "manga-ocr",
                "paddle-ocr"
            ]
        );
        // 721 MiB before the quantised manga-ocr replaced the fp32 export — a 349 MiB cut
        // to what a first run must fetch, for 2% relative character error. See the entry.
        // 372 MiB until PP-OCRv6 small (30 MB) replaced PP-OCRv5's server tier (165 MB).
        assert_eq!(bytes, 237 * MB, "unexpected required download total");
    }

    #[test]
    fn every_required_artifact_is_exactly_pinned_for_resume_and_offline_import() {
        for component in all()
            .iter()
            .filter(|component| component.tier == Tier::Essential)
        {
            for artifact in component.artifacts {
                assert!(
                    artifact.expected_bytes().is_some(),
                    "{}:{} has no exact size",
                    component.id,
                    artifact.file
                );
                assert!(
                    artifact.sha256().is_some(),
                    "{}:{} has no SHA-256",
                    component.id,
                    artifact.file
                );
                assert!(!artifact.url.contains("/latest/"), "{}", artifact.url);
                assert!(!artifact.url.contains("/resolve/main/"), "{}", artifact.url);
            }
        }
    }

    #[test]
    fn sizes_read_the_way_a_person_would_say_them() {
        assert_eq!(human_size(50 * MB), "50 MB");
        assert_eq!(human_size(3_800 * MB), "3.7 GB");
        assert_eq!(
            human_size(find("ichiran").expect("catalog entry").bytes),
            "4.7 GB"
        );
    }
}
