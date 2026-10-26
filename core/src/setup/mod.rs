//! Getting the app from "installed" to "usable", without a terminal.
//!
//! 屠つ ships about 36 MB and needs roughly a gigabyte it cannot bundle (`project notes` §6).
//! That gigabyte used to be reachable only through `scripts/fetch-*.ps1` — fine for
//! whoever built the app, and impossible for anyone handed the installer, which does not
//! even carry the scripts.
//!
//! So the app fetches for itself. What it does **not** do is fetch on its own initiative:
//! every download is behind a click, with its size and licence shown first, which is what
//! `project notes` §8's "nothing large downloads automatically" is actually asking for. An
//! installer that pulled a gigabyte before the app had been opened would satisfy the
//! letter of nothing and the spirit of less.

pub mod backfill;
pub mod build;
pub mod catalog;
pub mod catalog_report;
pub mod claim;
pub mod enrichment;
pub mod fetch;
pub mod install;
pub mod publication;
pub mod resources;
pub mod transport;

pub use build::{kanji_table, wordnet_table, BuildError};
pub use catalog::{human_size, After, Artifact, Component, Tier};
pub use enrichment::{audio_pack, kanjivg_files, krad_tables, EnrichmentError};
pub use fetch::{accept_download, download, download_verified, FetchError, Progress, Verification};
