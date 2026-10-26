# SudachiDict for the analyzer comparison

Builds SudachiDict 20260723 the way Lindera's own `lindera-sudachidict` crate does, so the
built-in analyzer can be measured over it beside the embedded UniDic. The results, and why it was
**measured and not adopted**, are in `local documentation`. Nothing here ships.

1. Fetch the source Lindera builds from — `https://lindera.dev/sudachidict-20260723.tar.gz`,
   MD5 `f4cb9708a1d21f895ed49e8b66839283` (the hash `lindera-sudachidict` 6.2.0 pins), about
   200 MB — and unpack it outside the repository. SudachiDict is Apache-2.0 (Works Applications;
   `small_lex.csv` contains part of UniDic); the licence files are in the archive.
2. `cargo run --release --manifest-path scripts/sudachidict/Cargo.toml -- <source> <output>` —
   about 15 seconds; the output is about 566 MB.
3. `python scripts/sudachidict/dictionary_forms.py <source> <output>` — the side table that
   resolves SudachiDict's dictionary-form IDs (37,732 forms).
4. Run any harness that builds its analyzer through `common::built_in()` with
   `SAKATSU_LINDERA_SUDACHI=<output>`: `ud_gold`, `segmentation_gold`, `heteronym_readings`,
   `joyo_readings`, `analyzer_corpus`, `colloquial_comparison`, and the diagnostic
   `analyzer_dictionaries`.
