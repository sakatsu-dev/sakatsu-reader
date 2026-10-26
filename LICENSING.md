# Licences

**Data travels with its own licence, and it is attributed.** This document is the authority on
every licence claim in the project: what the code is released under, what the app downloads and
on what terms, what ships inside it, and what is only ever read to measure.

The app's About page credits everything below that a user receives, and
[`src-tauri/THIRD-PARTY-NOTICES.txt`](./src-tauri/THIRD-PARTY-NOTICES.txt) carries the licence
texts of the libraries compiled into it.

## This project's code

- Sakatsu's own code: **GNU General Public License, version 3 only** (`GPL-3.0-only`),
  copyright the Sakatsu contributors. The full terms are in [`LICENSE`](./LICENSE) and travel
  with the app. Adopted 2026-10-22 in place of the previous MIT/Apache-2.0 choice.
- The GPL permits selling copies. If you distribute the application or a modified version,
  you must comply with its source-code and licensing obligations. Private changes need not
  be published. This is not a noncommercial licence.
- This change applies to Sakatsu's own code. Third-party libraries, assets, dictionaries and
  models keep their original licences and notices; it does not revoke permissions already
  granted for earlier copies. Distribution also requires rights to all included contributions.
- Sakatsu remains a clean-room implementation designed against japReader's behaviour
  (GPL-3.0), containing none of its source.

## Rules that apply everywhere

- JMdict / JMnedict — EDRDG, CC BY-SA. Ichiran — MIT. Jitendex — CC BY-SA 4.0.
  FuguMT — CC BY-SA 4.0. Yomitan format — open. Hanabira grammar content — CC.
  Imabi / Tae Kim / Pomax — all rights reserved: **link only, never copy**.
- **Fetched by the app, not by the installer.** The installer contains the executable and
  its small Windows runtime dependencies; dictionaries, data and models are not bundled.
  The required first-run task is behind informed consent with its aggregate size and licences;
  optional resources retain their own explicit actions —
  which is what "nothing large downloads automatically" means. An install-time fetch would
  run elevated, before the app had been seen, with no way to recover a half-failure and no
  way to decline the parts nobody wants. `core/src/setup/` holds the catalog and the
  downloader; the PowerShell scripts remain the development path and tests prove their URLs plus
  the separately published offline-essentials manifest agree with the embedded release catalog.
  Setup state remains in Roaming beside the unchanged store identity; downloaded resources prefer
  LocalAppData or a learner-chosen fixed drive. See `local documentation`.

## At a glance: what the app downloads

Each is fetched only with consent, from a pinned version checked against its size and digest
(`core/src/setup/catalog.rs`), and named with its licence before the download starts.

| Component | Licence | Setup |
|---|---|---|
| Jitendex and JMnedict dictionaries | CC BY-SA 4.0, built on JMdict (EDRDG) | essential |
| KANJIDIC2 | EDRDG, CC BY-SA 4.0 | essential |
| KanjiVG | Ulrich Apel, CC BY-SA 3.0 | essential |
| Basic KRADFILE | EDRDG licence | essential |
| Japanese WordNet; Princeton WordNet 3.0 | BSD-like, attribution required | essential |
| manga-ocr | Apache-2.0 | essential |
| PP-OCRv6 small (ONNX conversion by snowfluke) | Apache-2.0 | essential |
| FuguMT | CC BY-SA 4.0 | optional |
| Tofugu and WaniKani pronunciation audio | CC BY-SA 4.0 | optional |
| JPDB frequency list | compiled from jpdb.io | optional |
| BCCWJ frequency list | derived from NINJAL's BCCWJ | optional |
| PaddleOCR-VL for Manga; llama.cpp | Apache-2.0; MIT | optional |
| Ichiran | MIT | optional |

## Downloaded by the app

- **KANJIDIC2** — EDRDG, **CC BY-SA 4.0**: the same rights holder and licence as the
  JMdict data already shipped, so it takes on no new obligation beyond naming it, which
  the setup panel does. `scripts/fetch-kanjidic.ps1` turns 15 MB of XML into a ~0.6 MB
  `char → readings` table; fetched, never committed, and the app works without it. It is
  what lets the reference page answer *what a character is*, which a word dictionary
  cannot: 館 as a word is やかた, and in 図書館 it is カン.
- **KanjiVG r20260714** — Ulrich Apel, **CC BY-SA 3.0** — supplies stroke geometry and
  numbering. **Basic KRADFILE** — EDRDG licence — supplies visible components, which the UI
  deliberately does not call radicals. The standard **Kanji details** action derives these as
  inert typed JSON, `krad.tsv`, and `krad-reverse.tsv`, separately from `kanjidic.tsv`.
  Downloaded SVG markup is never injected into the interface. Existing KANJIDIC-only installs
  remain usable and are offered Update; name readings stay separate and never enter furigana
  alignment.
- **Offline pronunciation** is optional. It uses only Tofugu's explicitly open Japanese
  vocabulary pronunciation archive — Tofugu and WaniKani, **CC BY-SA 4.0** — retaining its
  licence and MP3 files while discarding redundant OGG files. Playback requires an exact
  `(surface, normalized reading)` match, so homographs cannot borrow one another's recording;
  uncovered words retain the existing network fallback. The archive is fetched, never bundled.
- **Japanese WordNet** — NICT and Francis Bond, BSD-like. **Attribution and a link to
  <https://bond-lab.github.io/wnja/> are conditions of use**, so both appear in the
  app's setup panel, not only here. **Princeton WordNet 3.0** — WordNet License,
  BSD-like, attribution likewise required. `scripts/fetch-wordnet.ps1` joins them into
  the noun→category table that sharpens particle senses; fetched, never committed, and
  the app works without it (`core/src/grammar/particles.rs`).
- **PP-OCRv6 (small)** — PaddlePaddle, **Apache-2.0**, through snowfluke's ONNX conversion of the
  official weights (also Apache-2.0); it replaced PP-OCRv5 on 2026-10-09. The conversion was
  checked against PaddlePaddle's own inference before it was adopted, and the catalog names both
  before the download. The engines tried beside it and dropped — PP-OCRv6 medium, Baberu OCR
  (Apache-2.0) and PaddleOCR-VL 1.6 general (Apache-2.0) — are recorded with their measurements in
  `local documentation`; none is fetched any more. The 1.6-based manga fine-tune
  (`gigass03/PaddleOCR-VL-1.6-Manga-Finetuned`) answered 401 and remains to be tried.

## Shipped inside the app

- **Tatoeba — CC BY 2.0 FR, shipped.** The curated grammar entries added on 2026-10-17 use
  Tatoeba sentences as examples, taken from the Tatoeba exports directly rather than from
  Jitendex, whose bundled copies are CC BY-SA and would put a share-alike term on the pack. Each
  sentence's number is recorded in `local documentation`, and the About page credits Tatoeba
  (`credits.rs`), which is the attribution the licence requires.
- **The 屠 seal mark** — traced from 印篆貫珠 (国立国会図書館) via CODH's 篆書字体データセット,
  **CC BY-SA 4.0**, doi:10.20676/00000390. Path data rather than a font because *no
  open-licence font carries 屠 in seal script* — the two that exist hold 133 and 1,374 CJK
  characters and skip it, and the one commercial face that has it forbids embedding in
  software. The letterform itself is public domain; the scan is not, so it is attributed.
  Provenance and the trace in `ui/src/assets/mark/`.
- No bundled text face. The app draws its interface in whatever the system already has
  (`--font` in `ui/src/theme.css`), which now names a Japanese fallback so mixed
  English-and-Japanese strings do not resolve to a Chinese-preferred face. A subset of
  Yuji Syuku was committed for the stacked 屠つ wordmark and removed on 2026-08-03 when
  the seal replaced it: two glyphs nothing drew with, carrying an OFL that then had to
  travel with them. Git has it if the wordmark ever comes back.

## Evaluation and tests only — never shipped

- **Aozora Bunko ruby** — test-only reading exemplars. The
  [official handling rules](https://www.aozora.gr.jp/guide/kijyunn.html) distinguish public-domain
  works from works distributed with permission; the catalog itself is **CC BY 4.0**, credited to
  Aozora Bunko. `scripts/harvest-aozora-readings.py --download` selects only works whose work and
  every credited person's copyright flags are `なし`, with author roles only, modern kana, and
  Japanese literature classifications (NDC 913/914). Translations are excluded. It retains the
  source archives, full catalog records, URLs, hashes and transformation notes beside a bounded
  ruby-derived TSV under gitignored `scratch/context-plan/aozora/`. Nothing enters an app pack or
  committed fixture. Literary ruby is experimental training/retrieval evidence, not manga/game reading gold. The
  sentence-context feasibility test failed its gates; no new embedding model is shipped or
  downloaded by the app. That first test used the already installed Ollama model only.
- **Tohoku Japanese BERT v3** — **Apache-2.0**, test-only target-word classification in
  local reading experiments. `scripts/fetch-reading-encoder.py --download` explicitly
  fetches pinned, hash-verified safetensors and tokenizer metadata into ignored scratch storage.
  The model card describes CC-100 and Japanese Wikipedia pretraining; its model licence does
  not grant permission to redistribute arbitrary source corpora. Aozora work-split classifiers
  use only the permitted local ruby bank above; KWDLC and captures never train them. No app
  loader, model catalog entry or shipped weights were added.
- **KWDLC** — Kyoto University Web Document Leads Corpus, **no licence declared**. There is no
  LICENSE file and GitHub reports none; the README says only that it is "released for the purpose
  of contributing to the research of natural language processing", which is intent rather than a
  grant. It therefore has the standing this section gives Imabi and Tae Kim: **consult, never
  import.** `scripts/fetch-kwdlc.ps1` puts it beside the other corpora in the user's data directory,
  outside the repository entirely, and only the
  annotation is taken — not the `org/` web text. It is read to *measure* and nothing derived from
  it may reach a pack, a fixture, a dictionary or anything that ships. It earns that latitude
  because it is annotated in the **JUMAN** lineage rather than UniDic, which makes it the only
  source here able to judge between the built-in analyzer and Ichiran without sharing a dictionary
  with one of them — and the only one carrying readings that can settle 私 わたし/わたくし, which
  UD_Japanese-GSD cannot, being UniDic's own answer. Ask the authors before going further than
  measurement.
- **UD_Japanese-GSD** — Universal Dependencies, **CC BY-SA 4.0**: the licence already
  carried for Jitendex and KANJIDIC2, so naming it is the whole obligation.
  `scripts/fetch-ud-japanese.ps1` takes the 3 MB test and dev splits; the 40 MB train split is
  opt-in and only earns its place once something is being fitted rather than measured. It is
  the **only ground truth this project has for segmentation** — every other cross-analyzer
  number measures agreement, which cannot say who is right — and its `LUWBILabel` layer is what
  decides whether a finer split should be rejoined, a judgement dictionary membership cannot
  make. Its readings come from UniDic, so it cannot adjudicate a reading *against* UniDic. News
  and blog register: it validates the mechanism, not this app's domain. Fetched, never
  committed, and everything works without it.
- **JESC** — Japanese-English Subtitle Corpus, Stanford / Google Brain / Rakuten,
  **CC BY-SA 4.0**: again the licence already carried, so naming it is the whole obligation.
  `scripts/fetch-jesc.ps1` keeps only the Japanese column of the 2.8M pairs. It is here for
  **register and nothing else** — the two corpora above are news, blogs and encyclopedic prose,
  which is where the built-in analyzer already agrees with Ichiran at 96%, and the gap is casual
  speech. Subtitle dialogue is the closest freely-licensed thing to the manga and game text this
  app is for. It is emphatically **not gold**: no boundary or reading annotation, so it measures
  agreement only, and its crawled Japanese is of uneven quality — `必要のない子 耐えるにしている。`
  is in it. It is also standard casual rather than dialect, because subtitles for foreign film are
  rendered in standard Japanese; 〜だべ and 〜けんども stay the business of the hand-collected
  corpus, which no licence covers because it is the user's own reading. Fetched, never committed.
- **SudachiDict 20260723** — Works Applications, **Apache-2.0** (its `small_lex.csv` contains part
  of UniDic, under UniDic's own BSD/GPL/LGPL terms). Evaluation only: built outside the repository
  by `scripts/sudachidict/` from the archive Lindera's own crate pins, and measured against the
  embedded UniDic (`local documentation`). Not shipped, fetched or offered by the app.
- **Evaluation-only data** — read to measure, never shipped: the **JoyoKanji-Yomi** benchmark
  (SB Intuitions, **MIT**), fetched by `scripts/fetch-joyo-yomi.ps1` outside the repository with
  its licence beside it; and PaddleOCR's own Python pipeline with PaddlePaddle's Paddle-format
  weights (both Apache-2.0), installed in a scratch environment as the parity reference for
  PP-OCRv6 (`scripts/ocr-parity/`). Neither reaches the app or a committed fixture.
- **Public JLPT grammar lists, evaluation only** — 絵でわかる日本語 (edewakaru.com) and the JLPT
  Grammar List (jlptgrammarlist.neocities.org), **no licence declared**, so consult, never import.
  `scripts/fetch-grammar-lists.py` keeps only names, levels, glosses and links, outside the
  repository; `core/tests/grammar_lists.rs` measures the packs against them and writes its review
  sheets under `target/`. No explanation, example sentence or audio from either is read or kept.
- **Translation metrics and candidates, evaluation only** (`scripts/mt-metrics/`,
  `local documentation`), all fetched outside the repository at pinned revisions:
  **COMET-22** (`Unbabel/wmt22-comet-da`, **Apache-2.0**) with its XLM-R encoder config (MIT);
  **MetricX-24** hybrid large (`google/metricx-24-hybrid-large-v2p6-bfloat16`, **Apache-2.0**),
  its mT5 tokenizer (Apache-2.0) and the reference `google-research/metricx` code (Apache-2.0,
  read as the parity reference, not copied); **Tatoeba** sentence exports (**CC BY 2.0 FR**) for
  the two-human-translations probe; and **LFM2-350M-ENJP-MT** (Liquid AI, **LFM Open License
  v1.0**: Apache-2.0-like, but commercial use is licensed only to organisations under US$10M
  annual revenue). LFM2 was run by hand on a local `llama-server`; the app neither fetches nor
  offers it. If it is ever offered, the licence text ships beside the weights, credits name it, and
  the revenue condition is stated where the download is offered.
