//! What 屠つ is built on, and under what terms.
//!
//! # Why this exists as data
//!
//! `project notes` §8 is the authority on every licence claim here, and it is a document — nothing
//! reads it, and nothing checks it against what the app actually links. The About page needs the
//! same facts in a form it can render, and the obligations are real: the CC BY-SA data has to be
//! attributed, and WordNet's terms name a link as a condition of use rather than a courtesy.
//!
//! # The software only. The data lives in the catalog.
//!
//! Every dictionary, model and frequency list already carries its licence in
//! [`crate::setup::catalog`], because the setup screen has to show it *before* the download —
//! which is what "nothing large downloads without being asked" means in practice. Restating those
//! strings here would be a second copy of a licence, which is precisely the kind of drift this
//! project keeps a test for. So the About page reads the catalog for the data half and this for
//! the software half, and neither string exists twice.
//!
//! # Almost only the software
//!
//! One group is not packages: [`Group::Consulted`] names the grammar references this app was
//! written against. They ship no code and no text — they are all rights reserved, so they are
//! consulted and linked and never copied (`project notes` §8) — but a reader deserves to know whose
//! description of Japanese the grammar notes were checked against, and the alternative was a
//! citation link on each of forty-four entries, which is the same acknowledgement made forty-four
//! times.
//!
//! Otherwise there is nothing here that is not a package. An Artwork group briefly credited the 屠 seal —
//! CC BY-SA 4.0, traced via CODH's 篆書字体データセット — and was removed by request. That
//! attribution now lives only in `project notes` §8 and in `ui/src/assets/mark/`, **neither of which
//! ships with the installer**, so the built app no longer names it. If it should be credited in the
//! app again, it wants a row here rather than a corner of some other page.
//!
//! # No version numbers, deliberately
//!
//! A manifest holds a *requirement* — `"^2"`, `"0.25.6"`, `"=2.0.0-rc.13"` — and the lockfile
//! holds what that resolved to, which is usually a different number: this tree asks for `image`
//! 0.25.6 and builds 0.25.10. Printing either one on an About page invites the reader to believe
//! the other, and pinning it would mean touching this file on every `cargo update`. A licence and
//! an attribution do not depend on the version; the app's own version is [`self::VERSION`], and
//! that one is read from Cargo rather than typed.
//!
//! # Direct dependencies only
//!
//! Around four hundred crates get compiled into this app. Listing them is a generated wall nobody
//! reads, and reading it is the only thing an About page is for. What is listed is what was
//! *chosen* — and `tests/credits_match_manifests.rs` asserts that everything chosen is listed,
//! so the wall being absent is not the same as something being hidden.

/// 屠つ's own version, from Cargo rather than from a second copy here.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Which part of the app a credit belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// The frame the app is: the shell, the webview, the interface.
    Frame,
    /// Reading Japanese — analysis, recognition, translation.
    Japanese,
    /// The plumbing underneath both.
    Plumbing,
    /// Not code at all: the grammar references the explanations were written against.
    Consulted,
}

impl Group {
    /// What to call this on the page. Here rather than in the UI so the three names cannot drift
    /// from the three variants.
    pub fn label(self) -> &'static str {
        match self {
            Group::Frame => "The app itself",
            Group::Japanese => "Reading Japanese",
            Group::Plumbing => "Underneath",
            Group::Consulted => "Consulted",
        }
    }

    /// Whether the About page shows this group.
    ///
    /// **`Plumbing` is credited but not shown, and the distinction is the point.** Serde, chrono,
    /// zip, a hashing crate and a clipboard binding are what every desktop application in this
    /// language is made of; naming them says nothing about *this* app, and a reader who has got as
    /// far as the About page is looking for what makes it what it is. The two groups that stay are
    /// the ones a reader could not guess: the frame it is built in, and what reads the Japanese.
    ///
    /// **Credited, though — the row stays in [`all`].** That is what `credits_match_manifests.rs`
    /// checks against the manifests, so dropping the data would fail the test that exists to stop
    /// a dependency going unrecorded. Not being *printed* is a decision about the page; not being
    /// *recorded* would be a different thing entirely.
    ///
    /// This is not where any licence obligation is discharged, and it never was. MIT asks for the
    /// copyright and permission notice to travel with the software, and Apache-2.0 §4 asks for the
    /// licence and any NOTICE; a page listing "Serde — MIT OR Apache-2.0" carries neither, for the
    /// groups that stay as much as for the one that goes. That gap is the same size before and
    /// after this, and closing it means shipping the licence texts with the installer, which
    /// nothing in `bundle.resources` does today.
    pub fn on_about(self) -> bool {
        !matches!(self, Group::Plumbing)
    }
}

/// One thing to credit.
#[derive(Debug, Clone)]
pub struct Credit {
    /// What its authors call it.
    pub name: &'static str,
    /// An SPDX expression where there is one, prose where the terms are not a standard licence.
    pub licence: &'static str,
    /// Where to read about it — and for the licences that require attribution, the link that
    /// discharges it.
    pub url: &'static str,
    /// **One line on why it is here, and this is the field that makes this a page rather than a
    /// wall.** A list of names and licences is a legal notice; a list that says what each thing
    /// does is something a reader gets anything from.
    pub why: &'static str,
    pub group: Group,
    /// The manifest entries this credit stands for.
    ///
    /// A family gets one row — Tauri is four crates and an npm package, and five rows saying
    /// "Tauri" would be worse than one. Empty for a credit that is not a package at all.
    ///
    /// This is what `tests/credits_match_manifests.rs` matches against, so a dependency added to
    /// `core/Cargo.toml`, `src-tauri/Cargo.toml` or `ui/package.json` and not named here turns
    /// that test red.
    pub packages: &'static [&'static str],
}

/// Everything, in the order the About page shows it.
pub fn all() -> &'static [Credit] {
    ALL
}

static ALL: &[Credit] = &[
    // --- the frame ---
    Credit {
        name: "Minisign",
        licence: "MIT",
        url: "https://github.com/jedisct1/rust-minisign",
        why: "Signs and verifies release artifacts.",
        group: Group::Plumbing,
        packages: &["minisign"],
    },
    Credit {
        name: "Tauri",
        licence: "Apache-2.0 OR MIT",
        url: "https://tauri.app",
        why: "The desktop shell: a Rust process, the system's own webview, and the bridge \
              between them. Chosen over Electron so the *app* is tens of megabytes rather \
              than hundreds. The installer is larger only because it carries the webview \n              runtime for machines without one, and installs nothing extra on machines \n              that already have it.",
        group: Group::Frame,
        packages: &[
            "tauri",
            "tauri-build",
            "tauri-plugin-global-shortcut",
            "tauri-plugin-single-instance",
            "tauri-plugin-dialog",
            "tauri-plugin-updater",
            "@tauri-apps/api",
            "@tauri-apps/plugin-dialog",
        ],
    },
    Credit {
        name: "Svelte",
        licence: "MIT",
        url: "https://svelte.dev",
        why: "Draws the interface. Its compiler turns components into plain DOM updates, so \
              there is no framework runtime shipped alongside them.",
        group: Group::Frame,
        packages: &[
            "svelte",
            "svelte-check",
            "@sveltejs/vite-plugin-svelte",
            "@tsconfig/svelte",
        ],
    },
    Credit {
        name: "Vite",
        licence: "MIT",
        url: "https://vite.dev",
        why: "Builds the interface, and serves it while it is being worked on.",
        group: Group::Frame,
        packages: &["vite"],
    },
    Credit {
        name: "Playwright",
        licence: "Apache-2.0",
        url: "https://playwright.dev",
        why: "Checks the built Analysis layout in the system Edge browser at the window sizes \
              and display scales the desktop app has to survive.",
        group: Group::Frame,
        packages: &["@playwright/test"],
    },
    Credit {
        name: "Prettier",
        licence: "MIT",
        url: "https://prettier.io",
        why: "Keeps the interface's source in one formatting, as a check CI runs. A development               tool: it ships in nothing.",
        group: Group::Frame,
        packages: &["prettier", "prettier-plugin-svelte"],
    },
    Credit {
        name: "TypeScript",
        licence: "Apache-2.0",
        url: "https://www.typescriptlang.org",
        why: "Types the interface, and — through `model.ts` — is where the shape of everything \
              crossing the boundary from Rust is written down.",
        group: Group::Frame,
        packages: &["typescript"],
    },
    Credit {
        name: "ts-rs",
        licence: "MIT",
        url: "https://github.com/Aleph-Alpha/ts-rs",
        why: "Writes the TypeScript declarations the interface programs against straight from the \
              Rust types, so an enum variant renamed in Rust cannot quietly go on working in the \
              reader. A build-time tool: it sits behind an optional feature and ships in nothing.",
        group: Group::Frame,
        packages: &["ts-rs"],
    },
    // --- reading Japanese ---
    Credit {
        name: "Lindera",
        licence: "MIT",
        url: "https://github.com/lindera/lindera",
        why: "Breaks a passage into words when Ichiran is not installed. It embeds UniDic, so \
              the app can analyse Japanese out of the box with nothing downloaded.",
        group: Group::Japanese,
        packages: &["lindera"],
    },
    Credit {
        name: "ONNX Runtime, via ort",
        licence: "MIT OR Apache-2.0",
        url: "https://ort.pyke.io",
        why: "Runs conventional recognition and translation models in-process. The optional \
              PaddleOCR-VL reader uses an app-managed llama.cpp process instead.",
        group: Group::Japanese,
        packages: &["ort"],
    },
    Credit {
        name: "Hugging Face tokenizers",
        licence: "Apache-2.0",
        url: "https://github.com/huggingface/tokenizers",
        why: "Turns a sentence into what the translation model expects. It reads FuguMT's \
              `tokenizer.json` directly, which is why no SentencePiece build is needed.",
        group: Group::Japanese,
        packages: &["tokenizers"],
    },
    Credit {
        name: "image and imageproc",
        licence: "MIT OR Apache-2.0, and MIT",
        url: "https://github.com/image-rs/image",
        why: "Prepares a capture for recognition — greyscale, resizing, and the contour tracing \
              PP-OCR's detector needs. Between them they are why this does not need OpenCV.",
        group: Group::Japanese,
        packages: &["image", "imageproc"],
    },
    Credit {
        name: "quick-xml",
        licence: "MIT",
        url: "https://github.com/tafia/quick-xml",
        why: "Reads KANJIDIC2's 15 MB of XML into the small reading table the reference page \
              answers character questions from.",
        group: Group::Japanese,
        packages: &["quick-xml"],
    },
    Credit {
        name: "encoding_rs",
        licence: "MIT OR Apache-2.0",
        url: "https://github.com/hsivonen/encoding_rs",
        why: "Decodes the basic KRADFILE's published EUC-JP text without asking the user's \
              system locale to interpret it.",
        group: Group::Japanese,
        packages: &["encoding_rs"],
    },
    Credit {
        name: "unicode-normalization",
        licence: "MIT OR Apache-2.0",
        url: "https://github.com/unicode-rs/unicode-normalization",
        why: "Normalises a spelling and reading before matching it to an exact recording in the \
              optional offline pronunciation pack.",
        group: Group::Japanese,
        packages: &["unicode-normalization"],
    },
    // --- text that ships, and is not a package ---
    //
    // `packages: &[]` for the reason the consulted group below gives. Unlike that group this
    // *is* copied: CC BY 2.0 FR requires the attribution, and this row is it.
    Credit {
        name: "Tatoeba",
        licence: "CC BY 2.0 FR",
        url: "https://tatoeba.org/",
        why: "Native example sentences, with their English, for the grammar notes added in \
              October 2026 — each one chosen by reading its translation, and recorded by \
              sentence number in the project's grammar-accuracy notes.",
        group: Group::Japanese,
        packages: &[],
    },
    // --- consulted, and only consulted ---
    //
    // `packages: &[]` on all three: they are not dependencies, and
    // `credits_match_manifests.rs` matches on that field in both directions, so an empty one
    // keeps them out of a check that has nothing to say about them.
    Credit {
        name: "Imabi",
        licence: "All rights reserved — linked, never copied",
        url: "https://imabi.org/",
        why: "A long, careful grammar reference, and the one most of the curated grammar notes               were checked against. Its lessons are the authority where this app and a textbook               disagree.",
        group: Group::Consulted,
        packages: &[],
    },
    Credit {
        name: "An Introduction to Japanese — Michiel \"Pomax\" Kamermans",
        licence: "All rights reserved — linked, never copied",
        url: "https://pomax.github.io/nrGrammar/",
        why: "A whole-language description rather than a lesson series, and the better second               opinion when a point turns on how a form is built.",
        group: Group::Consulted,
        packages: &[],
    },
    Credit {
        name: "Tae Kim's Guide to Japanese",
        licence: "All rights reserved — linked, never copied",
        url: "https://guidetojapanese.org/learn/",
        why: "The plain-spoken account of how the language is actually used, and a check on               explanations that had drifted into being technically true and useless.",
        group: Group::Consulted,
        packages: &[],
    },
    // --- underneath ---
    Credit {
        // **Shipped, not merely used.** The installer embeds Microsoft's Evergreen runtime so a
        // machine with no internet can still install - see `project notes` §8 on things travelling
        // with their licence. The `webview2-com` crates in the notices file are the Rust
        // bindings; this is the runtime itself, which is not a cargo dependency and appears in
        // no manifest. Hence `packages: &[]`.
        name: "Microsoft Edge WebView2 Runtime",
        licence: "Microsoft Edge WebView2 Runtime distribution terms - redistribution permitted",
        url: "https://developer.microsoft.com/microsoft-edge/webview2/",
        why: "The engine this interface is drawn with. It is embedded in the installer rather               than fetched during setup, because an installer that needs the network is an               installer that fails without one. Where Windows already provides it nothing               further is installed, and Microsoft keeps it updated.",
        group: Group::Frame,
        packages: &[],
    },
    Credit {
        name: "Serde",
        licence: "MIT OR Apache-2.0",
        url: "https://serde.rs",
        why: "Every structure that crosses the boundary between Rust and the interface, and \
              every settings file that survives a restart.",
        group: Group::Plumbing,
        packages: &["serde", "serde_json"],
    },
    Credit {
        name: "ureq",
        licence: "MIT OR Apache-2.0",
        url: "https://github.com/algesten/ureq",
        why: "The only thing here that opens a network connection: fetching what the setup \
              screen offers, talking to AnkiConnect on loopback, and asking for a word's audio.",
        group: Group::Plumbing,
        packages: &["ureq"],
    },
    Credit {
        name: "xcap",
        licence: "Apache-2.0",
        url: "https://github.com/nashaofu/xcap",
        why: "Takes the screenshot the region overlay is drawn over.",
        group: Group::Plumbing,
        packages: &["xcap"],
    },
    Credit {
        name: "arboard",
        licence: "MIT OR Apache-2.0",
        url: "https://github.com/1Password/arboard",
        why: "Reads the clipboard, so a copied image or a line of text can go straight into the \
              reader.",
        group: Group::Plumbing,
        packages: &["arboard"],
    },
    Credit {
        name: "windows",
        licence: "MIT OR Apache-2.0",
        url: "https://github.com/microsoft/windows-rs",
        why: "The Win32 calls this app makes for itself: the mouse binding's low-level hook, and \
              the title bar it draws in place of the system's.",
        group: Group::Plumbing,
        packages: &["windows"],
    },
    Credit {
        name: "zip, tar and flate2",
        licence: "MIT, and MIT OR Apache-2.0",
        url: "https://github.com/zip-rs/zip2",
        why: "Unpack what the setup screen fetches — Yomitan dictionaries are zips, WordNet is a \
              tarball, KANJIDIC2 arrives gzipped. Pure Rust, so no C toolchain is needed to \
              build this app.",
        group: Group::Plumbing,
        packages: &["zip", "tar", "flate2"],
    },
    Credit {
        name: "chrono",
        licence: "MIT OR Apache-2.0",
        url: "https://github.com/chronotope/chrono",
        why: "Timestamps in the optional log file, and nothing else.",
        group: Group::Plumbing,
        packages: &["chrono"],
    },
    Credit {
        name: "RustCrypto hashes",
        licence: "MIT OR Apache-2.0",
        url: "https://github.com/RustCrypto/hashes",
        why: "MD5 names a word's remote audio file the way that service expects; SHA-256 proves \
              a required setup artifact is exactly the immutable release Sakatsu catalogued.",
        group: Group::Plumbing,
        packages: &["md-5", "sha2"],
    },
    Credit {
        name: "fs2",
        licence: "MIT OR Apache-2.0",
        url: "https://github.com/danburkert/fs2-rs",
        why: "Asks the target drive how much space is available before setup begins, so a large \
              local install can choose another location before a disk-full failure.",
        group: Group::Plumbing,
        packages: &["fs2"],
    },
    Credit {
        name: "base64",
        licence: "MIT OR Apache-2.0",
        url: "https://github.com/marshallpierce/rust-base64",
        why: "Decodes Sakatsu's tiny embedded Japanese OCR health fixture before a newly \
              installed recognition engine is marked ready.",
        group: Group::Plumbing,
        packages: &["base64"],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every field on the page is a field that has to be there.
    ///
    /// A blank `why` renders as a name with nothing under it, and a blank `url` renders as a link
    /// to nowhere — which for the CC BY-SA row would be an attribution that does not attribute.
    #[test]
    fn every_credit_is_complete() {
        for credit in all() {
            assert!(!credit.name.is_empty());
            assert!(!credit.licence.is_empty(), "{}", credit.name);
            assert!(!credit.why.is_empty(), "{}", credit.name);
            assert!(
                credit.url.starts_with("http"),
                "{} needs a link, not {:?}",
                credit.name,
                credit.url
            );
        }
    }

    /// No package appears under two credits.
    ///
    /// The drift test only asks whether a manifest entry is named *somewhere*, so a crate listed
    /// twice would pass it while the page showed the same dependency under two headings.
    #[test]
    fn no_package_is_credited_twice() {
        let mut seen: Vec<&str> = Vec::new();
        for credit in all() {
            for package in credit.packages {
                assert!(
                    !seen.contains(package),
                    "{package} is credited twice, the second time under {}",
                    credit.name
                );
                seen.push(package);
            }
        }
    }
}
