//! Shared fixture loading for the integration tests.

/// The reader-output equivalence harness: explicit outcomes, complete key sets, named baselines and
/// checked resource identities. Written against the pre-change analysis API, so the preserved
/// pre-change source records its own baseline.
#[allow(dead_code)]
pub mod equivalence;
/// Reading UD_Japanese-GSD. Its own file because two harnesses now score against it and a
/// second copy of a CoNLL-U reader is how they would stop agreeing about what gold says.
pub mod gold;
/// The JoyoKanji-Yomi benchmark: fetched pinned, split by kanji with the holdout sealed, aligned
/// only where the target kanji occurs once.
#[allow(dead_code)]
pub mod joyo;
/// Reading KNP-annotated Japanese. Its own file for the same reason `gold` is — and because the
/// original reader answered "unreadable", "malformed" and "empty" with the same empty result.
pub mod knp;
/// What more than one reference harness must agree on — the KWDLC reservation above all.
pub mod reference;
/// Carrying completed cases between invocations. Extracted from `colloquial_comparison.rs`, with
/// the atomic write and the non-destructive refusal it did not have.
pub mod resume;
pub mod scratch;
/// A small deterministic word store, so equivalence covers status targets and known-word ruby.
#[allow(dead_code)]
pub mod store_fixture;
#[allow(unused_imports)]
pub use scratch::Scratch;

use std::collections::{BTreeSet, HashMap};

use sakatsu_core::analyzer::ichiran::{self, raw};
use sakatsu_core::model::Analysis;
use sakatsu_core::ocr::OcrImage;

/// Render real Japanese glyphs for live OCR pipeline tests without committing a fixture.
///
/// Vertical input treats `lines` as columns in Japanese reading order: the first is drawn
/// on the right, then each following column to its left. Keeping this in one place means
/// the single-engine regression and the cross-engine comparison really see identical pixels.
#[allow(dead_code)]
pub fn render_ocr_lines(lines: &[&str], vertical: bool) -> OcrImage {
    // The pid is here for the same reason it is on every other scratch path: two overlapping
    // test runs would otherwise render into the same file and read each other's pixels.
    let out = std::env::temp_dir().join(format!(
        "sakatsu-ocr-{}-{}-{}.png",
        lines.len(),
        if vertical { "v" } else { "h" },
        std::process::id()
    ));

    let encoded: Vec<String> = lines
        .iter()
        .map(|line| {
            let codes = line
                .chars()
                .map(|c| format!("0x{:X}", c as u32))
                .collect::<Vec<_>>()
                .join(",");
            format!("([char[]]@({codes}) -join '')")
        })
        .collect();

    let script = format!(
        r#"
Add-Type -AssemblyName System.Drawing
$lines = @({lines})
$font = New-Object System.Drawing.Font('Yu Gothic UI', 40)
$vertical = ${vertical}
$bmp = New-Object System.Drawing.Bitmap 1, 1
$g = [System.Drawing.Graphics]::FromImage($bmp)
if ($vertical) {{
  $cell = $g.MeasureString('国', $font)
  $maxLength = 0
  foreach ($l in $lines) {{
    if ($l.Length -gt $maxLength) {{ $maxLength = $l.Length }}
  }}
  $w = [int]($cell.Width * $lines.Length) + 40
  $h = [int]($cell.Height * $maxLength) + 40
}} else {{
  $w = 0; $h = 20
  foreach ($l in $lines) {{
    $s = $g.MeasureString($l, $font)
    if ([int]$s.Width -gt $w) {{ $w = [int]$s.Width }}
    $h += [int]$s.Height
  }}
  $w += 40
}}
$g.Dispose(); $bmp.Dispose()
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::White)
$g.TextRenderingHint = 'AntiAliasGridFit'
if ($vertical) {{
  $cell = $g.MeasureString('国', $font)
  $x = $w - 20 - [int]$cell.Width
  foreach ($l in $lines) {{
    $y = 20
    foreach ($ch in $l.ToCharArray()) {{
      $g.DrawString($ch, $font, [System.Drawing.Brushes]::Black, $x, $y)
      $y += [int]$cell.Height
    }}
    $x -= [int]$cell.Width
  }}
}} else {{
  $y = 10
  foreach ($l in $lines) {{
    $g.DrawString($l, $font, [System.Drawing.Brushes]::Black, 20, $y)
    $y += [int]$g.MeasureString($l, $font).Height
  }}
}}
$g.Dispose()
$bmp.Save('{path}', [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
"#,
        lines = encoded.join(","),
        vertical = vertical,
        path = out.display().to_string().replace('\\', "\\\\"),
    );

    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .expect("run powershell");
    assert!(status.success(), "rendering {lines:?} failed");

    let bytes = std::fs::read(&out).expect("read rendered png");
    let decoded = image::load_from_memory(&bytes).expect("decode png");
    let rgba = decoded.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    let _ = std::fs::remove_file(&out);
    OcrImage::new(width, height, rgba.into_raw()).expect("valid image")
}

/// Parse a captured `ichiran-cli -f` fixture and normalize it, exactly as the
/// analyzer does at runtime — the process invocation is the only thing skipped.
///
/// Regenerate the fixtures with `scripts/capture-ichiran-fixtures.sh`; do not hand-
/// edit them, or the tests stop testing real Ichiran output.
///
/// Not every test binary that shares this module needs every helper — the authoring
/// harness works from the live analyzer and a corpus, never from a fixture.
#[allow(dead_code)]
pub fn analyze(fixture: &str, text: &str) -> Analysis {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ichiran")
        .join(format!("{fixture}.json"));
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let segments = raw::parse(&json).expect("fixture parses as ichiran output");
    // The one function the adapter itself calls, rather than a copy of its steps. This used
    // to repeat them, with a note conceding the copy was a no-op on every committed fixture
    // and would rot unnoticed; it then gained a second step and the copy went stale the same
    // day. Skipping the process invocation is the only difference from a real analysis.
    ichiran::to_analysis(text, &segments)
}

/// Not every test binary that shares this module needs every helper.
#[allow(dead_code)]
pub fn surfaces(analysis: &Analysis) -> Vec<&str> {
    analysis
        .tokens
        .iter()
        .map(|token| token.surface.as_str())
        .collect()
}

/// Every distinct Japanese example sentence in Jitendex — a corpus the grammar pack
/// had no hand in writing.
///
/// Both the authoring harness and the precision gate need this. Authoring uses it to
/// reject a candidate *before* it lands; the gate uses it to catch one that slipped
/// through. Measuring a rule only against the examples it was derived from is
/// circular, and that circularity hid thirteen bad rules until this corpus was used.
///
/// The Japanese half of [`jitendex_examples`], which is where the scan and the cache live.
#[allow(dead_code)]
pub fn jitendex_sentences() -> Vec<String> {
    jitendex_examples()
        .into_iter()
        .map(|(japanese, _)| japanese)
        .collect()
}

/// The same sentences **with their translations**.
///
/// Tatoeba-derived, bundled by Jitendex, CC BY-SA 4.0 and already credited in the app
/// (`project notes` §8). The English is what makes this a *reference* corpus rather than only a
/// source of Japanese: `knowledge_vs_model.rs` scores two machine readings against it, and
/// neither of them wrote it.
///
/// Needs `scripts/fetch-dictionaries.ps1`. Cached, because scanning a 36 MB archive of
/// 400k+ entries takes long enough to be annoying to do twice.
///
/// **Its own cache file**, deliberately: the sentence-only cache is a plain line list that
/// several tests already read, and widening it in place would have every one of them take a
/// tab and a translation as part of a sentence until somebody noticed.
#[allow(dead_code)]
pub fn jitendex_examples() -> Vec<(String, String)> {
    let cache = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/jitendex-examples.tsv");
    if let Ok(text) = std::fs::read_to_string(&cache) {
        let pairs: Vec<(String, String)> = text
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(japanese, english)| (japanese.to_owned(), english.to_owned()))
            .collect();
        if !pairs.is_empty() {
            return pairs;
        }
    }

    let archive_path = dictionaries_dir().join("jitendex-yomitan.zip");
    let file = std::fs::File::open(&archive_path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}\nrun scripts/fetch-dictionaries.ps1 first",
            archive_path.display()
        )
    });
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).expect("open zip");

    let banks: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let file = archive.by_index(i).ok()?;
            let name = file.name().ok()?.into_owned();
            Some(name)
        })
        .filter(|name| name.starts_with("term_bank_") && name.ends_with(".json"))
        .collect();

    let mut seen = std::collections::HashSet::new();
    let mut examples = Vec::new();
    for name in &banks {
        let mut entry = archive.by_name(name).expect("bank present");
        let mut json = String::new();
        std::io::Read::read_to_string(&mut entry, &mut json).expect("read bank");
        for parsed in sakatsu_core::dict::yomitan::parse_term_bank(&json).expect("bank parses") {
            for definition in &parsed.definitions {
                for example in &definition.examples {
                    let text = example.japanese.trim();
                    // Fragments are not sentences and would skew every rate computed
                    // from this.
                    if text.chars().count() >= 6 && seen.insert(text.to_owned()) {
                        // A tab or a newline in either half would corrupt the cache on the way
                        // back in, so they are flattened here rather than trusted.
                        examples.push((flatten(text), flatten(example.english.trim())));
                    }
                }
            }
        }
    }

    let lines: Vec<String> = examples
        .iter()
        .map(|(japanese, english)| format!("{japanese}\t{english}"))
        .collect();
    let _ = std::fs::write(&cache, lines.join("\n"));
    examples
}

/// The pooled translation corpus: `want` Jitendex example pairs spread evenly across all of them.
///
/// Only pairs whose English runs past three words, so a one-word gloss is not scored as a
/// sentence. `translation_quality.rs` and `translation_export.rs` both draw from here, which is
/// what lets the metric scripts and the August record speak about the same 300 sentences.
#[allow(dead_code)]
pub fn pooled_corpus(want: usize) -> Vec<(String, String)> {
    let pairs = jitendex_examples();
    let usable: Vec<&(String, String)> = pairs
        .iter()
        .filter(|(japanese, english)| {
            !japanese.is_empty() && english.split_whitespace().count() > 3
        })
        .collect();
    let step = (usable.len() / want.max(1)).max(1);
    usable
        .iter()
        .step_by(step)
        .take(want)
        .map(|(japanese, english)| (japanese.clone(), english.clone()))
        .collect()
}

/// One line, no tabs — what the cache format can survive.
#[allow(dead_code)]
fn flatten(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── Scoring ─────────────────────────────────────────────────────────────────────────────────

/// Words shared by almost any two English sentences, and therefore evidence of nothing.
///
/// The same idea as `translate::compare::EMPTY` and a longer list, because this scores whole
/// sentences rather than the two or three words a comparison marks: `have`, `will` and `not` are
/// noise here and were not there.
const FUNCTION_WORDS: &[&str] = &[
    "the", "a", "an", "of", "to", "in", "on", "at", "by", "for", "with", "from", "as", "is", "are",
    "was", "were", "be", "been", "being", "am", "do", "does", "did", "have", "has", "had", "will",
    "would", "shall", "should", "can", "could", "may", "might", "must", "not", "no", "and", "or",
    "but", "if", "then", "than", "that", "this", "these", "those", "it", "its", "i", "you", "he",
    "she", "we", "they", "me", "him", "her", "us", "them", "my", "your", "his", "our", "their",
    "s", "t", "re", "ll", "ve", "d", "m", "one", "there", "here", "so", "up", "out", "about",
];

/// Lowercase, letters only, and a few suffixes off the end.
///
/// The suffix trimming is what lets *seeks* meet *seek* and *feelings* meet *feeling*, which is
/// most of what separates a rate that means something from one dominated by inflection. It is not
/// a stemmer and will mangle words a stemmer would not — `less` off `unless` — and that is
/// tolerable because it mangles both sides identically.
#[allow(dead_code)]
pub fn normalise(word: &str) -> String {
    let letters: String = word
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    for suffix in ["ingly", "edly", "ing", "ies", "ied", "es", "ed", "s"] {
        // Never trim a word down to nothing or to a stub: `is` is not `i`, `ties` is not `t`.
        if letters.len() > suffix.len() + 2 && letters.ends_with(suffix) {
            let stem = &letters[..letters.len() - suffix.len()];
            return if suffix == "ies" || suffix == "ied" {
                format!("{stem}y")
            } else {
                stem.to_string()
            };
        }
    }
    letters
}

/// The content words of an English text, as a set.
#[allow(dead_code)]
pub fn content(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(normalise)
        .filter(|word| word.len() > 1 && !FUNCTION_WORDS.contains(&word.as_str()))
        .collect()
}

#[derive(Default, Clone, Copy)]
#[allow(dead_code)]
pub struct Score {
    pub hit: usize,
    pub reference: usize,
    pub said: usize,
    pub sentences: usize,
}

// Every test binary compiles this module, and most of them use none of it — so the allow is
// load-bearing rather than tidy-up: without it, adding a helper here fails `clippy -D warnings`
// in a dozen unrelated harnesses that never asked for it.
#[allow(dead_code)]
impl Score {
    pub fn add(&mut self, reference: &BTreeSet<String>, candidate: &BTreeSet<String>) {
        self.hit += reference.intersection(candidate).count();
        self.reference += reference.len();
        self.said += candidate.len();
        self.sentences += 1;
    }
    /// How much of the human translation's meaning is present.
    pub fn recall(&self) -> f64 {
        self.hit as f64 / self.reference.max(1) as f64
    }
    /// How much of what it said was in the human translation.
    pub fn precision(&self) -> f64 {
        self.hit as f64 / self.said.max(1) as f64
    }
    pub fn f1(&self) -> f64 {
        let (r, p) = (self.recall(), self.precision());
        if r + p == 0.0 {
            0.0
        } else {
            2.0 * r * p / (r + p)
        }
    }
}

use sakatsu_core::dict::Dictionary;
use sakatsu_core::grammar::particles::{self, Categories};

// ── What the app knows about each word of a sentence ─────────────────────────────────────────
//
// Shared for the same reason the scoring above is: two harnesses ask it, and one asking the
// dictionary a slightly different question than the other would make their reports impossible to
// compare — which is the whole point of running them against the same corpus.

/// What the app knows about one token, kept apart rather than joined.
///
/// The second measurement needs to ask a question of a *word* — did the model say anything about
/// this one at all — and a joined string cannot answer it.
#[allow(dead_code)]
pub struct TokenKnowledge {
    /// The glosses at the limit asked for. This is what a line on screen would print.
    pub glosses: String,
    /// Every gloss of the leading sense, whatever the limit. Used only to ask whether the model
    /// covered this word, and generous on purpose: a wider net means fewer words wrongly called
    /// dropped, which is the error that would flatter the feature.
    pub every_gloss: String,
    /// The authored layer for this token — the favoured particle sense, and what its inflection
    /// does to the meaning.
    pub authored: Vec<String>,
}

/// Everything the app can say about each word of `analysis`, without asking the model.
///
/// `limit` is how many glosses of the leading sense to take. One is the tightest reading of the
/// word and the noisiest choice of *which* meaning; taking more trades precision for the chance
/// that the right sense is in there somewhere. Which way that trade goes is the experiment.
#[allow(dead_code)]
pub fn per_token(
    analysis: &Analysis,
    dictionary: &Dictionary,
    categories: &Categories,
    limit: usize,
) -> Vec<TokenKnowledge> {
    let mut out = Vec::new();

    for (index, token) in analysis.tokens.iter().enumerate() {
        if !token.is_word() {
            continue;
        }

        // The term the app itself looks a word up by — `commands::lookup` uses the dictionary
        // form and falls back to the surface. Asking a different question here would measure a
        // dictionary the app does not consult.
        let term = if token.dict_form.is_empty() {
            token.surface.as_str()
        } else {
            token.dict_form.as_str()
        };
        let found = dictionary
            .lookup(term)
            .ok()
            .and_then(|hits| hits.into_iter().next());
        let (glosses, every_gloss) =
            match found.as_ref().and_then(|hit| hit.entry.definitions.first()) {
                Some(definition) => (
                    definition
                        .glosses
                        .iter()
                        .take(limit)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" "),
                    definition.glosses.join(" "),
                ),
                None => (String::new(), String::new()),
            };

        let mut authored: Vec<String> = Vec::new();

        // **The ranker's null is respected.** `particles::senses` returns `None` when nothing
        // fired or when the top two tied, and Rust owns that distinction — picking a sense here
        // anyway would measure a rule the app does not apply.
        if let Some(reading) = particles::senses(&analysis.tokens, index, categories) {
            if let Some(at) = reading.favoured {
                if let Some(sense) = reading.senses.get(at) {
                    if sense.score > 0 {
                        authored.push(sense.label.clone());
                    }
                }
            }
        }

        // How the word is inflected, which is meaning the dictionary form's gloss has thrown
        // away: 食べさせられた is not 食べる. `Form::meaning` is the app's own wording for it —
        // written for 屠つ precisely because Imabi and Tae Kim may only be linked — so this
        // scores the sentence the app could actually put on screen, not one composed here.
        // `Unclassified` falls back to the analyzer's label, which is what the panel does too.
        let breakdown = sakatsu_core::grammar::breakdown(token);
        for step in &breakdown.steps {
            let said = step.form.meaning();
            authored.push(if said.is_empty() {
                step.label.clone()
            } else {
                said.to_string()
            });
            if step.neg {
                authored.push("not".to_string());
            }
        }

        out.push(TokenKnowledge {
            glosses,
            every_gloss,
            authored,
        });
    }

    out
}

// ── Scoring a translation the way a translation should be scored ─────────────────────────────
//
// Shared by `translation_quality.rs` and `fugumt_levers.rs`. Every one of these was validated
// before it was used to rank anything — see `translation_quality.rs` for the three-point check
// and for why content-word overlap was discarded after failing it.

/// Where a local model server is. Ollama's default; llama-server and LM Studio speak the same
/// shape. The embedder lives here too.
#[allow(dead_code)]
pub fn endpoint() -> String {
    std::env::var("SAKATSU_LLM_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".into())
}

#[allow(dead_code)]
pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// Small, local, already installed — a stand-in for COMET that costs nothing to obtain.
#[allow(dead_code)]
pub const EMBEDDER: &str = "qwen3-embedding:0.6b";

// ── chrF2 ───────────────────────────────────────────────────────────────────────────────────

/// Character n-grams of one length, counted.
#[allow(dead_code)]
pub fn char_grams(text: &str, n: usize) -> HashMap<String, usize> {
    // Whitespace is collapsed rather than kept: chrF is about the letters, and a candidate that
    // differs only in spacing is not a different translation.
    let chars: Vec<char> = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .collect();
    let mut counts = HashMap::new();
    if chars.len() < n {
        return counts;
    }
    for window in chars.windows(n) {
        *counts.entry(window.iter().collect::<String>()).or_insert(0) += 1;
    }
    counts
}

/// chrF2 — character n-gram F-score, recall weighted twice as heavily as precision.
///
/// Averaged over n = 1..=6, which is the standard configuration. Orders where the reference has
/// no n-grams at all are skipped rather than scored zero: a three-word reference has no 6-grams,
/// and counting that as a failure would punish short sentences for being short.
#[allow(dead_code)]
pub fn chrf2(reference: &str, candidate: &str) -> f64 {
    let (mut precisions, mut recalls) = (Vec::new(), Vec::new());
    for n in 1..=6 {
        let (r, c) = (char_grams(reference, n), char_grams(candidate, n));
        let total_r: usize = r.values().sum();
        let total_c: usize = c.values().sum();
        if total_r == 0 || total_c == 0 {
            continue;
        }
        let matched: usize = c
            .iter()
            .map(|(gram, count)| *count.min(r.get(gram).unwrap_or(&0)))
            .sum();
        precisions.push(matched as f64 / total_c as f64);
        recalls.push(matched as f64 / total_r as f64);
    }
    if precisions.is_empty() {
        return 0.0;
    }
    let p = precisions.iter().sum::<f64>() / precisions.len() as f64;
    let r = recalls.iter().sum::<f64>() / recalls.len() as f64;
    if p + r == 0.0 {
        return 0.0;
    }
    let beta2 = 4.0;
    (1.0 + beta2) * p * r / (beta2 * p + r)
}

// ── BLEU ────────────────────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

#[allow(dead_code)]
pub fn word_grams(words: &[String], n: usize) -> HashMap<Vec<String>, usize> {
    let mut counts = HashMap::new();
    if words.len() < n {
        return counts;
    }
    for window in words.windows(n) {
        *counts.entry(window.to_vec()).or_insert(0) += 1;
    }
    counts
}

/// Corpus BLEU accumulates across sentences before dividing — the standard definition, and not
/// the same as averaging per-sentence BLEU, which is dominated by short sentences scoring zero on
/// 4-grams.
#[derive(Default)]
#[allow(dead_code)]
pub struct Bleu {
    matched: [usize; 4],
    total: [usize; 4],
    candidate_length: usize,
    reference_length: usize,
}

#[allow(dead_code)]
impl Bleu {
    pub fn add(&mut self, reference: &str, candidate: &str) {
        let (r, c) = (words(reference), words(candidate));
        self.candidate_length += c.len();
        self.reference_length += r.len();
        for n in 1..=4 {
            let (rg, cg) = (word_grams(&r, n), word_grams(&c, n));
            self.total[n - 1] += cg.values().sum::<usize>();
            self.matched[n - 1] += cg
                .iter()
                .map(|(gram, count)| *count.min(rg.get(gram).unwrap_or(&0)))
                .sum::<usize>();
        }
    }
    pub fn score(&self) -> f64 {
        let mut log_sum = 0.0;
        for n in 0..4 {
            if self.total[n] == 0 {
                return 0.0;
            }
            // Add-one smoothing on the numerator only, so a corpus that happens to match no
            // 4-grams scores low rather than collapsing the whole product to zero.
            let p = (self.matched[n] as f64 + 1.0) / (self.total[n] as f64 + 1.0);
            log_sum += p.ln() / 4.0;
        }
        let brevity = if self.candidate_length > self.reference_length {
            1.0
        } else if self.candidate_length == 0 {
            0.0
        } else {
            (1.0 - self.reference_length as f64 / self.candidate_length as f64).exp()
        };
        100.0 * brevity * log_sum.exp()
    }
}

// ── Embedding cosine ────────────────────────────────────────────────────────────────────────

/// Embed a whole column, in batches, retrying once.
///
/// **Batched because a single 300-item request silently failed.** The first pooled run came back
/// with `NaN` for two models out of six and no paired test at all, because the `if let` guarding
/// it found no vectors. A partial answer that looks like a missing column is the worst kind: it
/// is easy to read past. Fifty at a time, and a failed batch is retried before the column is
/// given up on.
#[allow(dead_code)]
pub fn embed(texts: &[String]) -> Option<Vec<Vec<f32>>> {
    let mut all = Vec::with_capacity(texts.len());
    for chunk in texts.chunks(50) {
        let got = embed_batch(chunk).or_else(|| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            embed_batch(chunk)
        })?;
        if got.len() != chunk.len() {
            return None;
        }
        all.extend(got);
    }
    Some(all)
}

#[allow(dead_code)]
pub fn embed_batch(texts: &[String]) -> Option<Vec<Vec<f32>>> {
    let url = format!("{}/api/embed", endpoint());
    // An empty string embeds to nothing useful and some servers reject it outright, which would
    // fail the whole batch around it.
    let cleaned: Vec<String> = texts
        .iter()
        .map(|t| {
            if t.trim().is_empty() {
                ".".to_string()
            } else {
                t.clone()
            }
        })
        .collect();
    let body = serde_json::json!({ "model": EMBEDDER, "input": cleaned });
    let mut response = ureq::post(&url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .send_json(&body)
        .ok()?;
    let parsed: serde_json::Value =
        serde_json::from_str(&response.body_mut().read_to_string().ok()?).ok()?;
    parsed["embeddings"].as_array().map(|rows| {
        rows.iter()
            .map(|row| {
                row.as_array()
                    .map(|v| {
                        v.iter()
                            .filter_map(|x| x.as_f64())
                            .map(|x| x as f32)
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect()
    })
}

#[allow(dead_code)]
pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    f64::from(dot / (na * nb))
}

/// Whether a path is an image the OCR corpus should consider.
///
/// **One definition, shared.** Two harnesses walk the same directory for different reasons —
/// `ocr_corpus` scores what has ground truth, `corpus_bootstrap` suggests truth for what has
/// none — and a corpus where one of them silently ignores `.webp` would be one where samples
/// vanish between the two runs without saying so.
#[allow(dead_code)]
pub fn is_corpus_image(path: &std::path::Path) -> bool {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp")
}

/// Every corpus image in `dir`, sorted, whether or not it has ground truth.
#[allow(dead_code)]
pub fn corpus_images(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut images: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_corpus_image(path))
        .collect();
    images.sort();
    images
}

pub mod captures;

/// The built-in analyzer a harness measures: the embedded UniDic, or — when
/// `SAKATSU_LINDERA_SUDACHI` names a compiled SudachiDict directory — that dictionary, so the same
/// harness, scoring and inputs can be run once for each and compared.
///
/// A named directory that will not load fails loudly: a run asked to measure SudachiDict must
/// never quietly measure UniDic instead.
#[allow(dead_code)]
pub fn built_in(
) -> Result<sakatsu_core::analyzer::LinderaAnalyzer, sakatsu_core::analyzer::AnalyzeError> {
    match std::env::var_os("SAKATSU_LINDERA_SUDACHI") {
        Some(dir) => {
            let analyzer =
                sakatsu_core::analyzer::LinderaAnalyzer::sudachi(std::path::Path::new(&dir));
            assert!(
                analyzer.is_ok(),
                "SAKATSU_LINDERA_SUDACHI names {} but it does not load",
                std::path::Path::new(&dir).display()
            );
            println!(
                "built-in analyzer: SudachiDict from {}",
                std::path::Path::new(&dir).display()
            );
            analyzer
        }
        None => sakatsu_core::analyzer::LinderaAnalyzer::new(),
    }
}

/// Where the OCR corpus lives. `SAKATSU_OCR_CORPUS` overrides.
///
/// The material is copyrighted, so the default is a gitignored directory at the repository
/// root and nothing here ever writes outside it.
#[allow(dead_code)]
pub fn corpus_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_OCR_CORPUS") {
        return std::path::PathBuf::from(dir);
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("corpus")
}

/// Where the fetch script puts the zips. `SAKATSU_DICT_DIR` overrides.
#[allow(dead_code)]
pub fn dictionaries_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_DICT_DIR") {
        return std::path::PathBuf::from(dir);
    }
    let appdata = std::env::var_os("APPDATA").expect("APPDATA (or set SAKATSU_DICT_DIR)");
    std::path::PathBuf::from(appdata)
        .join("com.sakatsu.app")
        .join("dictionaries")
}

/// The dictionaries the application would open, in the order it would open them.
///
/// **Selection is shared with production, not transcribed from it.** A harness here used to
/// reimplement this from a comment: it filtered on a root-level `meta.json`, which is the
/// pre-generation layout, so it silently omitted every dictionary published as a generation — and
/// it ended in `.ok()`, so one that failed to open became one that was never there. Both make a
/// run score a smaller corpus and report a rate for it.
///
/// So the ranking comes from `Dictionary::discover`, and **a dictionary that will not open fails
/// the run**. That is the one place a measurement harness must differ from the app, which skips a
/// broken index so the rest still start.
#[allow(dead_code)]
pub fn installed_dictionaries() -> Vec<sakatsu_core::dict::index::Dictionary> {
    sakatsu_core::dict::index::Dictionary::discover(&dictionaries_dir())
        .into_iter()
        .map(|path| {
            sakatsu_core::dict::index::Dictionary::open(&path).unwrap_or_else(|error| {
                panic!(
                    "a required dictionary at {} would not open: {error}. A measurement must fail \
                     here rather than score what is left.",
                    path.display()
                )
            })
        })
        .collect()
}

/// Joins only: the segmentation the app runs, without the reading pass.
///
/// **One copy, because this encodes an *order* and not just two calls.** `commands::study` runs
/// `merge_compounds` before `merge_names` for a stated reason — a compound reading is the stronger
/// claim, so it never competes with a name for the same run — and a harness that gets that order
/// wrong measures a pipeline nobody ships. It was written twice before it was written once, and
/// now it is not written here at all: `pipeline::Pipeline` is the production code, and this
/// forwards to it.
///
/// Skipping these understated long-unit F1 by 2.7 points against gold, which is the size of most
/// of a day's work.
///
/// Deliberately still joins-only. Joining a word changes the denominator of a reading measurement,
/// so the segmentation harnesses want exactly this and `Pipeline::display` is what the reader sees.
#[allow(dead_code)]
pub fn through_pipeline(
    tokens: &[sakatsu_core::model::Token],
    dictionaries: &[sakatsu_core::dict::index::Dictionary],
) -> Vec<sakatsu_core::model::Token> {
    sakatsu_core::pipeline::Pipeline::new(dictionaries)
        .segmentation(tokens)
        .0
}
