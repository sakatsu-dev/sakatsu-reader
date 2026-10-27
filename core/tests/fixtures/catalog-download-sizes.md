# Catalog download sizes

Generated from `core/src/setup/catalog.rs`; do not edit the table by hand.
Regenerate with `cargo run -p sakatsu-core --bin generate-catalog-report`.
These are transfer sizes, not resident memory or peak installation space.
Setup preflight computes remaining disk requirements for the selected location.

| Component | Download bytes | MiB | Basis |
|---|---:|---:|---|
| Dictionaries | 50261459 | 47.9 | pinned artifacts |
| Kanji details | 14186541 | 13.5 | pinned artifacts |
| Particle senses | 11598399 | 11.1 | pinned artifacts |
| manga-ocr | 116703783 | 111.3 | pinned artifacts |
| Translation | 313434776 | 298.9 | pinned artifacts |
| Offline pronunciation | 94371840 | 90.0 | catalog estimate |
| Frequency — JPDB | 6291456 | 6.0 | catalog estimate |
| PP-OCRv6 | 31114838 | 29.7 | pinned artifacts |
| PaddleOCR-VL (manga) | 1851963583 | 1766.2 | pinned artifacts |
| Frequency — BCCWJ | 19922944 | 19.0 | catalog estimate |
| Ichiran analyzer | 5046796288 | 4813.0 | catalog estimate |

Essential components total: **223865020 bytes (213.5 MiB)**. Optional components are separate.
