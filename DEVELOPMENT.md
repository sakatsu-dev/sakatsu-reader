# Developing ?? (Sakatsu)

How to build and test the app, and how each part works and why. The [README](./README.md)
is the overview. Shared interface styles live in `ui/src/theme.css`.

## Stack

- **Tauri v2** — Rust core (`sakatsu-core`) + web UI, with a path to iOS/Android.
- **Analyzer** — Lindera + UniDic built in; Ichiran (sidecar) preferred when present. Both behind one `Analyzer` trait.
- **ML** — OCR (manga-ocr) and translation (FuguMT) via ONNX Runtime (`ort`).

## Layout

See `project notes` §2. `core/` is platform-independent and holds the owned logic;
`src-tauri/` is the desktop shell; `ui/` is the frontend.

## Development

Needs Rust 1.92+ (the floor `ort` sets), Node 20.19+ or 22.12+ (Vite 8's floor), and the Tauri CLI (`cargo install tauri-cli --version "^2"`).

**On Windows, stage the Microsoft C++ runtime once before the first build.** The Windows
configuration bundles the four redistributable DLLs beside the executable, and they are
generated rather than committed, so `cargo build`, `cargo test` and `cargo tauri dev` all stop
with "resource path `windows-runtime\msvcp140.dll` doesn't exist" until they are there:

```powershell
node scripts/stage-vc-runtime.mjs   # copies them from Visual Studio's x64 redistributable
```

It needs the Visual Studio C++ build tools, which Rust's MSVC toolchain needs anyway. CI runs
the same step before its release build.

**Ichiran is optional** — see below. Building and testing never need it: the analyzer's
tests work from committed fixtures. On Windows it lives in WSL, so the default command
is `wsl -e ichiran-cli -f`; override the whole command line with
`SAKATSU_ICHIRAN_CMD` if yours is elsewhere. Refresh the fixtures with
`wsl -e bash scripts/capture-ichiran-fixtures.sh`, and exercise the real sidecar with
`cargo test -p sakatsu-core --lib -- --ignored`.

```powershell
cargo test --workspace                              # model, Analyzer trait, IPC, wire-format golden
cargo test -p sakatsu-core --features typescript --test typescript_contract   # generated TS declarations
cargo clippy --workspace --all-targets -- -D warnings
npm --prefix ui ci                                  # first time only
npm --prefix ui run check                           # svelte-check, incl. the TS↔Rust contract
npm --prefix ui run check:format                    # Prettier; ui/.prettierignore is a ratchet
npm --prefix ui run build                           # production bundle; unused CSS is fatal
npm --prefix ui run check:layout                    # system Edge layout/scale matrix (Windows)
cargo tauri dev                                     # run the app; first build takes a few minutes
```

CI (`.github/workflows/ci.yml`) runs these checks. On Linux, the core suite runs one Cargo
test target at a time through `scripts/ci-core-tests.py`, including library/binary harnesses,
integration tests and doctests. It discovers targets from Cargo metadata, reports feature-gated
targets, and removes only completed test executables inside its explicitly owned target directory.
Dependencies and normal binaries remain available. The job records target accounting and disk
use, disables incremental/debug output, and caches downloads rather than compiled targets.
The TypeScript contract and both clippy feature configurations remain separate checks.
Both Rust jobs first fetch locked dependency sources because offline licence checks inspect
the desktop and optional-feature graphs too, including crates not compiled by the core job.
The notices generator and its checker share `scripts/notices-target.txt`, so both inspect the
shipped Windows target rather than silently changing their scope with the machine running them.

PowerShell scripts must be ASCII or UTF-8 with BOM so Windows PowerShell 5.1 can read them on
any ANSI codepage. App-launched scripts retain the stricter ASCII rule. The desktop suite checks
these bytes and also asks the real PowerShell 5.1 parser to read every script.

Both Rust jobs verify/cache the pinned Jitendex archive outside the checkout. An existing
particle-distribution test needs its licensed example corpus; no private report or personal
profile is used. The archive's size/hash are checked even on cache hits, and a hermetic test
keeps the CI pin aligned with the catalog. No model is downloaded for these jobs.

The committed catalog-size fixture is verified without local reports. Regenerate it intentionally
after changing artifact metadata:

```powershell
cargo run -p sakatsu-core --bin generate-catalog-report
# An additional report can be written with: -- --output PATH
```

Layout tests pin Japanese text roles and the Japanese fallback in mixed prose to Yu Gothic UI,
retaining Segoe UI for Latin, and verify the rendered platform font
before each worker begins. Production typography is unchanged. Font diagnostics include the
runner image, Edge version and the previous serif choice's actual fallback. Review affected
screenshots when changing these test fonts; do not raise tolerances to hide font differences.
The layout runner builds an isolated QA bundle and serves it with Vite preview. This avoids
opening hundreds of development-module requests per page on Windows. The normal production
build separately verifies that QA fixtures are absent; the layout build verifies they are present.

Note `cargo tauri dev`, not `npm run tauri` — the npm CLI is deliberately not a
dependency of `ui/`. A running `cargo tauri dev` holds a lock on
`target/debug/sakatsu-desktop.exe`, so stop the app before running the tests.

Word status and settings live in `%APPDATA%\com.sakatsu.app\store` as plain JSON. If
that store cannot be opened the app still runs and says so in a banner, rather than
letting you mark words that silently go nowhere.

**The `typescript` line is not optional.** The `TS` derives are behind an off-by-default feature,
so `cargo test --workspace` compiles none of the generation machinery — a green ordinary run says
nothing about whether `ui/src/bindings.ts` still matches the Rust types. Regenerating is a separate
command that verification never runs:

```powershell
cargo run -p sakatsu-core --features typescript --bin generate-bindings
```

**`cargo test --workspace` is not everything.** Around thirty tests carry `#[ignore]`
because they need a model, the dictionaries, the Ichiran sidecar or a served vision model,
and none of them run in the ordinary suite. Worse, most of the ones that *do* run check for
what they need and return early — and cargo reports that as `ok`, so a machine with nothing
installed shows a wall of green while testing almost nothing.

```powershell
pwsh -File scripts/test-all.ps1                 # both passes, per suite
pwsh -File scripts/test-all.ps1 -IgnoredOnly    # just the ignored half
```

It separates **passed** from **skipped** and prints why each one skipped. Two tests had
been broken for months before this existed, both invisible behind the green tick.

**Window lifetime needs a real window**, so it has a script of its own rather than a place in
either pass:

```powershell
cargo build -p sakatsu-desktop --release
pwsh -File scripts/test-window-lifetime.ps1     # closing the reader must end the session
```

It opens the capture frame, posts `WM_CLOSE` to the reader the way Alt+F4 does, and fails if the
process is still alive twenty seconds later. That was a real state: the frame has no close button,
no taskbar entry and sits above everything, and no binding brings the reader back — so an instance
could outlive its own interface with Task Manager as the only way out. `cargo test` cannot see it,
because the mock runtime only raises `CloseRequested` inside a run loop the unit tests never pump.

**`target/` needs occasional weeding.** Cargo never reclaims old artifacts: a rebuild
whose inputs hash differently leaves the previous one behind for good, and deleting a
source file leaves its output behind for good. Left alone this reached 64.5 GB here, of
which 40 GB was unreachable — 21 copies of `sakatsu_core`, five 328 MB copies of
`libort_sys`, and a 95 MB binary from a test deleted that afternoon.

```powershell
pwsh -File scripts/trim-target.ps1              # report only
pwsh -File scripts/trim-target.ps1 -Confirm     # actually delete
```

It removes only what nothing can reach — artifacts whose source is gone, and older copies
of things that have a newer one. It does not touch debug information or anything that
changes what a build produces. `-Incremental` additionally clears the incremental cache,
which is the one part with a real cost: a slower next rebuild.

When the drive is tight and keeping any compiled cache is less important than reclaiming
all of it, use the full clean instead:

```powershell
pwsh -File scripts/clear-target.ps1              # show the path and size, then ask
pwsh -File scripts/clear-target.ps1 -WhatIf      # preview only; never asks or deletes
pwsh -File scripts/clear-target.ps1 -Force       # unattended, with safety checks intact
```

It resolves the target through Cargo, refuses an unexpected or linked directory by
default, checks for a running app/build, calls `cargo clean`, and verifies the directory
is empty. Source, Git data, downloaded models, dictionaries and app data live elsewhere
and are not touched. The cost is a full Rust rebuild next time. If this workspace
deliberately uses a custom `target-dir`, inspect the printed path and opt in with
`-AllowCustomTarget`.

### Capture

**Ctrl+Shift+Q**, or the Capture button. The screen is frozen first, then a
transparent overlay opens over the monitor under the cursor; drag a region, or click
or press Escape to cancel. The overlay also gives up if it loses focus — a fullscreen
always-on-top layer that outlives its usefulness dims the screen and swallows every
click.

Text can also come from the **clipboard**. Clipboard *text* is used as-is rather than
being rendered and recognized — copying Japanese from a web page should not go through
OCR to come back slightly wrong. Clipboard images use the OCR model selected under
**Settings → Recognition**.

Recognition defaults to **Auto**. It keeps PP-OCRv6's clean horizontal reads, checks
manga-ocr when PP-OCRv6 detects vertical text or rejects a line, and requires both
complementary engines as part of the supported setup. If one is later damaged or removed,
Auto temporarily falls back to the survivor while the compact setup notice asks for the missing
engine to be repaired. Recognition settings can still force **manga-ocr** or **PP-OCRv6**; the
Analysis action row deliberately has no model selector.

**PaddleOCR-VL (manga)** is offered beside them and reads more accurately than any of them —
0.0297 mean character error over the 141-capture corpus against Auto's 0.0975, and below the
0.0652 no router over the other two can reach. It is optional because its catalog-listed transfer
is large and it needs a graphics card: the same weights measure hundredths of a second on a 4070 Ti SUPER and tens of
seconds on a processor. Setup times a read on your machine and only offers to make it your
reader when that came back fast. Auto never routes into it.

**Nothing is installed outside the app.** It fetches its Vulkan llama.cpp build into its own
folder, starts it when a capture needs it and
stops it when the reader closes. There is no service, no tray icon and no startup entry.

Local models are fetched for packaging or development:

```powershell
pwsh -File scripts/fetch-ocr-model.ps1    # manga-ocr, Apache-2.0
pwsh -File scripts/fetch-paddle-ocr.ps1   # PP-OCRv6 small, Apache-2.0
```

Current transfer sizes are shown in setup. Preflight separately computes the temporary
disk requirement at the chosen location.

Local models load on first capture rather than at startup, so an app that is only used
for pasted text never pays for them. If the selected model is unavailable, anything
needing OCR says so plainly (clipboard *text* still works, since it needs no
recognition).

Decoding is greedy plus the model's own `no_repeat_ngram_size: 3` guard. Its config
also specifies `num_beams: 4`; beam search is a refinement here rather than
load-bearing as it is for translation (§7), so output can differ slightly from the
reference implementation on ambiguous crops.

Check it against the real model — the images are rendered by the test, so there is no
fixture to commit:

```powershell
cargo test -p sakatsu-core --test onnx_probe     -- --ignored --nocapture  # graph signatures
cargo test -p sakatsu-core --test manga_ocr_live -- --ignored --nocapture  # reads Japanese
```

The overlay's coordinates are CSS pixels and a screenshot's are physical pixels — on
a 150% display a 100-pixel drag covers 150 image pixels. `Rect::scaled` does that
conversion and `core/src/ocr` unit-tests it; the live tests print the real factor:

```powershell
cargo test -p sakatsu-desktop --test capture_live -- --ignored --nocapture
cargo test -p sakatsu-desktop --test capture_ipc  -- --ignored --nocapture
```

### The model is a contract

`ui/src/model.ts` mirrors `sakatsu_core::model` by hand, and cannot silently drift
from it: a test in `src-tauri/tests/ipc.rs` pins the serialized shape into
`ui/src/model.golden.json`, and `ui/src/model.contract.ts` type-checks the mirror
against that golden — catching both a field renamed away and a field newly added.
After changing the model:

1. `$env:UPDATE_GOLDEN=1; cargo test -p sakatsu-desktop; Remove-Item env:UPDATE_GOLDEN`
2. `npm --prefix ui run check`, then fix `ui/src/model.ts` where it points.

Tests for `src-tauri` go in `tests/`, never in `#[cfg(test)]` modules — see
`src-tauri/build.rs` for the Windows linker reason.

### Packaging and first run

```powershell
cargo tauri build --bundles nsis   # → target/release/bundle/nsis
```

Produces one NSIS installer — the same target CI releases. It is **about 226 MB, and
most of that is not this app**: it carries Microsoft's WebView2 runtime, which is the
engine the interface is drawn with. It installs per-user, into `%LOCALAPPDATA%`, so it
needs no administrator prompt.

That is a deliberate trade against the principle two sections below. The installer used to
embed Microsoft's *online* bootstrapper instead, which downloaded the runtime mid-install
— so a machine without internet got `Error 1722` and nothing installed, measured at exit
1603 for the MSI and 2 for NSIS. An installer that spends a connection it was never
promised, and dies without one, is the failure that section exists to prevent; carrying the
runtime is what honours it. On a machine that already has WebView2, nothing extra is
installed and the payload is skipped.

Verified rather than assumed: `.installer-qa\run-clean-machine-qa.ps1 -NoNetwork` installs
into a disposable Windows Sandbox with **networking switched off** and asserts the whole
chain — exit code, the runtime arriving, the Add/Remove entry, the executable, and a window
that is the app rather than an error dialog.

The single installer also includes the pinned essentials package. Ichiran and optional resources retain separate downloads.
ONNX Runtime is statically linked into the executable, but its Windows build uses the
Microsoft C++ runtime, so the Windows build stages the four licensed x64 redistributable
DLLs beside the executable — a clean machine launches without a second installer, an
internet connection or an administrator prompt. Those generated DLLs are deliberately
gitignored.

A fresh install explains one informed required setup containing dictionaries, kanji details,
particle data, and both complementary OCR engines; nothing large downloads without approval.
The transfer persists across restarts through Windows BITS, with a resumable verified HTTP
fallback. An Analyzer step then recommends Ichiran with Lindera assistance, discloses its separate
download and WSL/Docker prerequisites, and retains an explicit **Use Lindera for now** option.
That pending step survives a restart; existing completed installations are not forced through it.
Translation, frequency data and pronunciation remain optional. A failed or
paused setup can enter pasted-text mode with the built-in analyzer while a compact Analysis notice
keeps **Resume setup** available.

Downloaded dictionaries can be **imported from that panel**. Import runs on a
background thread and reports back by event, because a large dictionary takes over a
minute and doing it on the command thread would freeze the window into something
indistinguishable from a crash. A newly imported dictionary is adopted by the running app before
the completion event is shown, so the next lookup uses it without a restart.

### Dictionaries are fetched, never committed

```powershell
pwsh -File scripts/fetch-dictionaries.ps1     # ~48 MB into %APPDATA%\com.sakatsu.app\dictionaries
```

Jitendex (36.8 MB) and JMnedict (10.9 MB), both CC BY-SA 4.0. Publisher checks are automatic by default; dictionary downloads require approval unless explicitly enabled. The importer captures each dictionary's own `attribution` from its
`index.json` and keeps it in `meta.json` beside the data — that licence requires
attribution, so don't strip it.

A zip is imported once into plain files: `terms.ndjson` plus a sharded offset index,
with **separate spelling and reading buckets** so looking up が doesn't return 蛾, 我
and 雅. Lookups read only the bytes they need; nothing loads the dictionary whole.
Jitendex imports to 433,885 entries / 143 MB, of which a lookup touches one shard
(≤468 KB) plus one line.

After changing the Yomitan parser, run the real-dictionary tests — hand-built
fixtures only ever agree with whatever their author believed:

```powershell
cargo test -p sakatsu-core --test jitendex_live -- --ignored --nocapture
```

### Versioned resource snapshots

KANJIDIC2 is downloaded from a dated, immutable release in
[sakatsu-dev/reader-resources](https://github.com/sakatsu-dev/reader-resources).
The publisher's original URL changes in place, so it cannot preserve an older catalog pin.
The application and `fetch-kanjidic.ps1` use the same snapshot, exact size and SHA-256.
Its installed filename and offline-package identity stay unchanged.

For a resource refresh, prepare and verify a new attributed snapshot, upload all assets to a
draft release, then publish with immutable releases enabled. Keep previous releases available.
Update the catalog, fetch script and offline manifest together if the bytes change. Do not use
`latest`, replace a published asset, or remove checksum verification to recover setup.

Verify the real endpoint with:

```powershell
cargo test -p sakatsu-core --test setup_build the_catalog_urls_still_answer -- --ignored --exact --nocapture
```

That explicit test downloads 1.4 MiB, verifies the catalog size and SHA-256, builds the kanji
table and checks a known reading. It is ignored in the offline suite.

### Setting it up without a terminal

The installer carries the app plus its small Windows runtime dependencies. Everything large is
fetched, and **the app fetches it itself**: open it with nothing installed and one guided essentials
task installs dictionaries, kanji details, particle senses, manga-ocr, and PP-OCRv6. The current
download sizes are shown in setup; preflight computes their actual remaining and peak disk cost.

Nothing downloads on its own initiative. Each size and licence is shown before its button,
which is what [`LICENSING.md`](./LICENSING.md) asks for — not that setup be hard, but that the app never
spend someone's connection without being asked. An installer that pulled a gigabyte before
the app had been opened would satisfy the letter of that and none of the point.

The `scripts/fetch-*.ps1` are still here and still the development path; the app holds the
same pinned URLs, sizes, and hashes in `core/src/setup/catalog.rs`, and tests prevent those
sources and the offline essentials manifest from drifting. Ichiran uses a separate **Set up** action: it opens the bundled guided
PowerShell flow, checks WSL and Docker before building anything, offers the missing Windows
prerequisite with consent, preserves working installs, and offers **Use Ichiran now** when ready.

```powershell
cargo test -p sakatsu-core --test catalog_matches_scripts   # the URLs still agree
cargo test -p sakatsu-core --test offline_essentials_catalog # release package still agrees
cargo test -p sakatsu-core --test setup_build -- --ignored --nocapture   # + the real URLs answer
```

### Looking things up on purpose

The book in the left rail opens a reference page: one box, answering with **words and
grammar together**. A learner typing なければ does not know which of the two they are
asking about, and the imported JLPT pack's 828 grammar points carry no match rules — the
engine never spots them in a sentence, so this page is the only way to reach them.

Japanese goes through the analyzer first, so a conjugated form is fine: 食べた finds
食べる, 行かない finds 行く.

**English works too.** It needs a one-time index of the definitions already installed —
the page offers to build it, it takes about ten seconds, and **nothing is downloaded and
nothing is re-imported**: it reads `terms.ndjson` where the dictionary already sits. About
16 MB for Jitendex.

What English search cannot do is rank true synonyms. Jitendex's score saturates — 22,553
entries share the maximum, 食べる and 喫する and 食う among them — and the format carries no
frequency list. So it answers with *several words that mean this*, in the dictionary's own
order, and says so rather than presenting the first as the answer. The ranking it does
have was built by running the alternatives over all 433,885 entries and reading the
results; see `GlossRank` in [core/src/dict/index.rs](core/src/dict/index.rs).

```powershell
cargo test -p sakatsu-core --test jitendex_live english_search -- --ignored --nocapture
```

Reference keeps those different answer types together without turning them into one long wall.
At a normal window width the same balanced previews can be shown as Cards or Rows; a narrow window
uses Rows so every result remains readable. Choosing one unfolds its complete detail in place, and
only that result loads pronunciation, tag explanations, diagrams, and deeper examples. **Words,
Grammar, Kanji, and Terms** are post-search facets over the same stable answer, not different
search modes. An ambiguous English or homographic search waits for a choice rather than dressing
the first item as authoritative. Only a uniquely direct grammar, particle, glossary, or
single-kanji answer opens automatically.

For a multi-token Japanese phrase, Reference may reuse Analysis's conservative contextual sense
ranking and label a unique supported meaning **Likely in this phrase**. A single word or English
query has no sentence evidence, so dictionary order remains untouched. Search tabs remember their
facet, expanded result, and collection position for the session; only their queries survive an
application restart.

The Glossary uses the same adaptive Cards/Rows collection and single inline expansion for browsing
the grammatical vocabulary used throughout Sakatsu. Grouped browsing is the default, A–Z is a
quiet alternative, and search matches labels, aliases, summaries, and longer details. Authored
**Builds on**, **Contrasts with**, **Related**, and **Used in these patterns** links connect
concepts without inventing a graph from similar English wording. Reference and Glossary remember
their wide-layout choices independently.

### Kanji details, on the reference page

```powershell
pwsh -File scripts/fetch-kanjidic.ps1
```

The dictionaries are *word* dictionaries. Kun'yomi and on'yomi belong to *characters*, so
without this 勉 and 聞 cannot be looked up at all, and 館 answers やかた "mansion" — which
is not what it does in 図書館.

The in-app **Kanji details** action fetches KANJIDIC2, stable KanjiVG r20260714, and basic
KRADFILE. It stores readings/name readings, inert typed stroke paths, and visual-component
indexes separately; downloaded SVG markup never reaches the DOM. An older KANJIDIC-only install
keeps working and is offered an explicit Update rather than receiving a surprise download.

Search a word on the reference page and its characters appear with **the reading that word
actually uses** picked out: 図書館 marks ト, ショ, カン. Name readings appear separately and
never enter this alignment. The alignment is worked out by
matching the word's reading against each character's candidates, allowing the two regular
sound changes — rendaku (人々 ひと·**び**と) and gemination (学校 が**っ**こう) — and the
okurigana a kun reading keeps inside a compound (焼豚 や**き**ぶた).

**Where it does not come out, nothing is marked and the page says so.** 明日 あした is not
built from 明 and 日 at all, and inventing a reading there would teach something false.
Across the whole installed dictionary that is 90.7% aligned and 9.3% abstained, the
abstentions being ateji and 熟字訓 — 玉蜀黍 とうもろこし, 基督教 キリストきょう.

```powershell
cargo test -p sakatsu-core --test jitendex_live reading_alignment -- --ignored --nocapture
```

Deliberately not in the reader: the analyzer already supplies the contextual reading and
furigana already renders it.

### Particle senses are sharper with WordNet

```powershell
pwsh -File scripts/fetch-wordnet.ps1   # ~1 MB written, BSD-like
```

Click a **で** or a **に** and the grammar panel lists everything it can mean here, with native
examples, and puts one in front **only when the surrounding words support it** — with
the reason shown, so it can be checked. When nothing supports one, it says so rather
than picking; being confidently wrong about grammar is worse than being silent.

**Two particles, not five.** を, が and は are authored and still listed in full on the Reference
page, but they are not ranked here: their ordinary readings — plain object, plain subject, plain
topic — carry no cues, so the panel could only ever promote a *marked* reading or shrug. Measured
over 5,886 occurrences of real sentences it shrugged 84–99% of the time for those three, which is
what a ranking that cannot reach the common answer looks like. で and に have a cue on every sense
and are ranked; authoring cues for the others brings them back automatically
(`ParticleEntry::rankable`). See `local documentation`.

This table is what lets it say "公園 is a place" instead of shrugging. It is optional:
without it the structural cues still work and the panel abstains more often. On the
held-out fixture that is 7 sentences read correctly instead of 13, out of the 14 that
are ranked — and **zero misread either way**, which is the number that matters
(`cargo test -p sakatsu-core --test particle_senses -- --nocapture`).

Japanese WordNet is NICT and Francis Bond's (<https://bond-lab.github.io/wnja/>), joined
here with Princeton WordNet 3.0. Both are BSD-like and both require attribution, which
is why the setup panel names them.

### Translation is opt-in

```powershell
pwsh -File scripts/fetch-translation-model.ps1   # ~300 MB, CC BY-SA 4.0
```

FuguMT Japanese→English, in-process through ONNX Runtime. **Nothing is translated —
and the model is not even loaded — until you ask.** The point of the app is to read the
Japanese; a translation sitting on screen does the reading for you.

Asking means clicking **Translation** beside **Analyze**, or Ctrl+clicking a word in the passage.
English appears in its own card above the selected-word details, so the Japanese passage keeps the
full reader width. Multi-sentence captures open as one passage for context; **Read line by line**
switches to the cached sentence-aligned rendering. Ctrl+click strikes the selected word and
transforms only its sentence in that aligned flow, while the surrounding translations remain in
place. English is not open on launch.

Decoding is **greedy, and that is load-bearing** (§7): the model's own config asks for
12 beams, and beam search on it degenerates into word salad. Do not "optimise" it into
beam search. The pad token doubles as the decoder's start token and is banned from
being generated, or greedy output can be empty.

What comes back is labelled a rough gist, and it earns that label — 情けない ("pathetic")
comes back as "merciful". Check it against the real model with:

```powershell
cargo test -p sakatsu-core --test fugumt_live -- --ignored --nocapture
```

### Anki export

Needs Anki running with the AnkiConnect add-on; 屠つ talks to `http://127.0.0.1:8765`.
The **+ Anki** button in the vocabulary panel sends the word *and the sentence it was
met in* — a card without its sentence teaches a definition rather than a language.

One note type (`屠つ (Sakatsu)`), six fields: Expression, Reading, Meaning, Sentence,
Source, PartOfSpeech. One card, recognition — the skill a reader needs, and one review
per word rather than two.

`Sentence` holds Anki's own furigana markup (`気持[きも]ち`) with the word the card is
about wrapped in `<b>`. It is **the sentence**, not the passage: the tokens travel to
Rust and `anki::sentence` picks out the one the word was met in.

**The front hides the reading and the back gives it**, which is the whole shape of the
card. The question renders through `{{kanji:Sentence}}` — the sentence exactly as it was
on the page, no ruby — so the reading is something you recall rather than something you
are shown. The answer renders the same field through `{{furigana:Sentence}}`. Both
filters are built into Anki; only *generating* the markup needs an add-on, and 屠つ does
that itself, so the card works on a stock install.

The answer is written out rather than reusing `{{FrontSide}}`, for two reasons: reusing
it would print the sentence twice, once bare and once with ruby, and writing it out lets
the answer lead with the word the card asked about and put the sentence below as
evidence. Order: word, then reading, part of speech and meaning, then the sentence, then
the dictionary.

A card carries at most **three senses**. Every inline sense used to reach it, and a
common word arrived as eight glosses of near-synonyms — which cannot be graded honestly.
The reference page still shows all of them.

It is created if absent and **brought into step if it already exists** — missing fields
added, template and styling rewritten. That reverses the old rule of never touching an
existing note type, and the cost is real: a hand-edited template is overwritten. The
cost of the old rule was worse, because every improvement to the card reached only
people who had never used the feature. Fields are only ever *added*, and field contents
are never touched, so no note loses anything it holds. Duplicates are left to Anki's own
check.

Only the inline senses reach the card; the "also in …" sources stay a reading-time
disclosure. The client is covered by a stub server in `core/tests/anki_stub.rs`, which
checks what is actually *sent* — a typo in an action name would pass every unit test
and fail against real Anki.

### Word audio

**Offline pronunciation** is an optional setup component, not an installer payload:

```powershell
pwsh -File scripts/fetch-pronunciation.ps1
```

It retains only MP3 files from Tofugu's open Japanese vocabulary pronunciation archive
(CC BY-SA 4.0) and indexes exact `(written form, normalized reading)` pairs. A newly installed
pack is adopted immediately. Homographs never match by spelling alone, and uncovered words fall
back to the existing on-demand source below.

The **▶** button next to a headword plays a recording from JapanesePod101's
dictionary endpoint — the same source Yomichan and Yomitan use, and real human
recordings rather than synthesised speech.

Be aware what that is: an **undocumented third-party endpoint with no licence grant**.
It can change or vanish, and nothing is cached to disk or redistributed — audio is
fetched on demand, played, and forgotten. This is the one place the project leans on
somebody else's server.

The trap it hides: asking for a word with no recording returns **HTTP 200 with a
placeholder MP3**, not a 404. `core/src/audio` detects it by md5
(`7e2c2f954ef6051373ba916f000168dc`, 52,288 bytes) and reports "no recording" — without
that check every word appears to have audio and you hear the same clip forever.

### Two analyzers: one built in, one optional

**屠つ analyses Japanese out of the box.** Lindera with a UniDic dictionary is
compiled into the binary, so a fresh install tokenizes, reads and looks up words with
no setup whatsoever.

**Ichiran is the recommended main analyzer, with Lindera assistance and fallback.** It is used
automatically when it answers:

| | built in (Lindera + UniDic) | Ichiran |
|---|---|---|
| tokenizing, base forms | yes | yes |
| readings | **95.9% agreement, measured** | from a JMdict database |
| dictionary senses | from the dictionaries | from the analyzer |
| conjugation | common attached forms rebuilt into a chain | a full derivation chain |
| compounds | structurally marked helper verbs rejoined | one word, with its parts |
| setup | none | guided WSL + Docker/PostgreSQL; see the generated catalog report |

The senses gap closes itself — the merge policy promotes a dictionary when the analyzer
has none, and names it — so with Jitendex installed the vocabulary panel looks much the
same. Lindera reconstructs UniDic's attached auxiliaries and non-independent helper
verbs, so 食べさせられていた has a learner-facing derivation and 伝わってきます has the
same two-part shape. Ichiran remains richer where the dictionary does not encode enough
context to resolve an ambiguous ending safely.

**The readings row is measured, not claimed.** Over the 141-capture corpus the two agree on
95.9% of aligned tokens — up from 83.3% when this was IPADIC, which read 者 as しゃ inside a
word said もの. The dictionary is 338 MB inside the binary against IPADIC's 68 MB, and that
size is what the accuracy cost.

Selection is automatic (Ichiran if it answers, built-in otherwise) and can be forced
with `SAKATSU_ANALYZER=ichiran|lindera`. The check runs a real analysis rather than
looking for the binary, because a sidecar that can't reach its database fails exactly
like a missing one.

Embedding the dictionary costs about 10 MB of installer — a fair price for an app that
works when you open it.

#### Why Ichiran can't simply be bundled

Measured on a working native install: `ichiran-cli` is 22.5 MB of **Linux** ELF and its
sources are 25 MB. Upstream's Docker instructions at the time of that experiment produced about
**4.7 GB of installed PostgreSQL data**. That dated installed footprint is distinct from the
current transfer estimate in the generated catalog report. Ichiran doesn't run natively
on Windows, needs a database *server* rather than an embedded one, and is seventy times
the dictionary set §6 already judged too big to bundle. Shipping it means shipping a
Linux distribution.

`scripts/setup-ichiran.ps1` is exposed as **Set up Ichiran** in onboarding's Analyzer step,
Installation Status and Analyzer settings. It reports by default and installs with
`-Install`, following Ichiran's own Docker route. It refuses to replace a working native
install, never deletes a database, and installs a bounded wrapper that restarts stopped
Compose services before retrying. The launcher is recorded in Sakatsu Reader's data as
well as the user environment. **Use Ichiran now** reloads and checks it without restarting
Sakatsu or signing out of Windows. The build compiles Common Lisp and restores the database, so it remains visible
in a real console rather than looking frozen behind the app.

### Grammar text lives in the pack

`core/src/grammar/packs/core.json` holds every title, explanation and example, so
the vocabulary breakdown and the grammar view quote one source. Code decides
*whether* a pattern applies; the pack decides what is *said* about it. Unknown JSON
keys are rejected, so a typo in a pack is a test failure rather than a rule that
silently never fires.

Match rules are written against **observed** Ichiran output only. Ichiran merges a
lot on its own — `には`, `ことができる` and `はずです` each arrive as one token — so
confirm a surface with `scripts/capture-ichiran-fixtures.sh` before adding a rule
for it.

There are two packs. `core.json` is curated and small. `hanabira.json` is 816 JLPT grammar
points rebuilt from [hanabira.org's CC0 content](https://github.com/tristcoil/hanabira.org-japanese-content)
by `scripts/import-grammar-pack.ps1`. It began as a reference shelf with **no match rules** — a
rule auto-derived from a title like `A が いちばん～` misfires — and 374 have since been authored
and verified against held-out sentences by `pack_authoring.rs` and `pack_precision.rs`. The rest
remain reachable from "Look up a grammar point" in the Grammar panel, searchable by kana, by
romaji, or by meaning.

**What the pack says is checked too, not just whether it matches.** `grammar_accuracy.rs` gates
one invariant in the ordinary test run — no two entries may teach one point at different JLPT
levels — and reports on wording and formatting behind `--ignored`. It found twelve contradictory
duplicates and eight entries sharing four explanations.

Imabi, Tae Kim and Pomax stay off limits for import: all are all-rights-reserved and may only be
linked. They are the references the grammar notes were written against, and they are credited once
on the About page under **Consulted** (`credits::Group::Consulted`) rather than linked from each
entry — the same acknowledgement, made once instead of forty-four times.

### Prepare the included essentials

Before any installer build, run:

```powershell
python scripts/prepare-essentials.py --fetch-missing
cargo tauri build --bundles nsis
```

The producer checks the compiled catalog against the committed snapshot, verifies every input,
then publishes `src-tauri/resources/essentials/essentials.zip`. `--source-root PATH` reuses
verified artifacts laid out as `component-id/filename`; `--output PATH` supports other local
outputs. Without `--fetch-missing`, missing inputs fail without contacting the network.
The existing PowerShell package-builder entry point delegates to the same producer.
The build hook refuses a missing or stale package. Data and the download cache remain ignored.

First-run setup prefers the included package and fills missing resources. Working installed
resources are preserved, including newer dictionaries. A damaged package offers an explicit
Download repair rather than silently switching to the network. The older offline-package
chooser remains a recovery tool. Ichiran still needs its separate guided setup.

Settings > Search > Dictionaries accepts Yomitan word ZIPs and raw KANJIDIC2 `.xml.gz` files.
An import is previewed before the user chooses Add or an explicit replacement target.
Updates use publisher metadata and stable installation IDs, not dated titles. Imported
publishers need consent before background access. Daily checks notify by default; automatic
installation is opt-in. Unknown revision schemes and installations without a recorded version
require manual approval. Models, WordNet, KanjiVG and KRADFILE remain release-pinned.

Resource-backed accuracy checks continue using their frozen evaluation dictionaries. Updating
an installed dictionary does not authorize refreshing a gold fixture or acceptance floor.

Publisher orchestration tests replace only HTTP transport and use the production IPC handler.
Run them explicitly with `cargo test -p sakatsu-desktop --features dictionary-publisher-tests --test dictionary_publishers`.
The fixture feature is disabled by default and has no effect in release builds.
