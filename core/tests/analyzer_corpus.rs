//! Ichiran against the built-in analyzer, on real captured Japanese.
//!
//! ```text
//! cargo test -p sakatsu-core --test analyzer_corpus -- --ignored --nocapture
//! ```
//!
//! # What this can and cannot say
//!
//! **There is no ground truth here, and that is the honest difference from `ocr_corpus`.**
//! That corpus has transcriptions somebody read off the image, so an engine can be scored.
//! Nobody has annotated token boundaries or conjugation labels for these passages, so there
//! is nothing to be right or wrong against.
//!
//! What this measures is **agreement**, which is a different and still useful thing. Two
//! analyzers that segment identically on 95% of passages are interchangeable for this app
//! whatever the truth is; two that agree on 60% are not, and the disagreements are then the
//! list worth reading by hand. So this prints a table and a sample of the differences, and
//! the sample is as much the point as the table.
//!
//! It is also worth saying what a disagreement is *not*: evidence against the built-in one.
//! `analyzer/lindera/mod.rs` is explicit that UniDic supplies fewer structural claims than
//! Ichiran's dictionary, so where they differ the sidecar may be more likely to be right —
//! but "more likely" is not "always", and reading them is how you find out which.
//!
//! # The corpus is never committed
//!
//! Same material and same rule as `ocr_corpus`: manga panels and game text belong to whoever
//! made them (`project notes` §8). The passages come from the transcriptions beside the images,
//! and with no corpus this skips rather than failing.

mod common;

use std::collections::BTreeMap;
use std::time::Instant;

use common::corpus_dir;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::{Analysis, Token};
use sakatsu_core::IchiranAnalyzer;

/// Every transcription in the corpus, longest first so the hard ones surface early.
/// The passages to compare over.
///
/// Two sources, because they answer different questions. The corpus transcriptions are this
/// app's own domain — game and manga text, the thing it exists to read — but there are 141 of
/// them and they are gitignored, so they cannot profile anything broadly.
/// `SAKATSU_ANALYZER_SENTENCES` points at a newline-delimited file instead:
/// `target/jitendex-sentences.txt` holds the 26,006 Tatoeba-derived sentences that
/// `scripts/harvest-sentences.py` already extracts, which is volume enough to say *where* the
/// two analyzers disagree rather than only how often.
///
/// **The registers differ, and keeping both is the point.** Tatoeba sentences are short, clean
/// and frequently translated from English; the corpus is dialogue. A disagreement profile from
/// one does not transfer to the other, so the run says which source it read.
fn passages() -> Vec<(String, String)> {
    match std::env::var("SAKATSU_ANALYZER_SENTENCES") {
        Ok(path) if !path.trim().is_empty() => sentences_from(std::path::Path::new(path.trim())),
        _ => corpus_passages(),
    }
}

/// One sentence per line, sampled by stride.
///
/// **Ichiran costs about 0.9 s a call.** Measured here, and the Lisp image loads per invocation
/// so running many inside one WSL process does not help — which puts all 26,006 at roughly six
/// and a half hours. `SAKATSU_ANALYZER_SAMPLE` caps it.
///
/// Stride rather than a seeded shuffle: it is reproducible with no RNG to agree on, and the file
/// arrives in dictionary order, which is unrelated to segmentation — so every Nth line is
/// arbitrary with respect to what is being measured. That would stop being true if the file were
/// ever sorted by length or by difficulty, and this comment is the reason not to.
fn sentences_from(path: &std::path::Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("SKIP: {} could not be read", path.display());
        return Vec::new();
    };
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let limit = std::env::var("SAKATSU_ANALYZER_SAMPLE")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|limit| *limit > 0)
        .unwrap_or(lines.len());
    let stride = lines.len().div_ceil(limit.max(1)).max(1);
    println!(
        "{} sentences in {}, taking every {stride} — {} to compare",
        lines.len(),
        path.display(),
        lines.len().div_ceil(stride),
    );
    lines
        .iter()
        .step_by(stride)
        .enumerate()
        .map(|(index, line)| (format!("line {}", index * stride + 1), (*line).to_string()))
        .collect()
}

fn corpus_passages() -> Vec<(String, String)> {
    let dir = corpus_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            // `.txt.mine` is the reviewed-by-nobody draft; `.txt` is promoted ground truth.
            // Either is a real Japanese passage, which is all this needs.
            let name = path.to_string_lossy();
            name.ends_with(".txt.mine") || (name.ends_with(".txt") && !name.ends_with(".mine"))
        })
        // **A transcription is only a sample if it transcribes something.** `corpus/` also
        // holds the review notes and a 31 KB run log, and taking that log as a passage put
        // thirteen thousand English tokens into the tally and made the two analyzers look
        // ten times further apart than they are. The image beside it is what makes it real.
        .filter(|path| {
            let stem = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let stem = stem
                .strip_suffix(".txt.mine")
                .or_else(|| stem.strip_suffix(".txt"))
                .unwrap_or(&stem)
                .to_string();
            ["png", "jpg", "jpeg", "webp", "bmp", "gif"]
                .iter()
                .any(|ext| path.with_file_name(format!("{stem}.{ext}")).is_file())
        })
        .filter_map(|path| {
            let stem = path.file_name()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).ok()?;
            let text = text.trim().to_string();
            (!text.is_empty()).then_some((stem, text))
        })
        .collect();
    // **One passage per capture.** `.txt` and `.txt.mine` are the promoted and draft
    // transcriptions of the *same image*, and every reviewed capture has both — so reading each
    // as its own passage counted 141 captures twice and called it 310. That does not bias one
    // analyzer against the other, since both see the same text, but it silently weights reviewed
    // material double against newly drafted material, which is exactly backwards.
    //
    // The promoted one wins where there is a choice; a draft stands alone until someone promotes
    // it.
    found.sort_by(|a, b| {
        let stem = |name: &str| name.trim_end_matches(".mine").to_string();
        stem(&a.0)
            .cmp(&stem(&b.0))
            .then(a.0.ends_with(".mine").cmp(&b.0.ends_with(".mine")))
    });
    found.dedup_by(|a, b| a.0.trim_end_matches(".mine") == b.0.trim_end_matches(".mine"));
    found.sort_by_key(|entry| std::cmp::Reverse(entry.1.chars().count()));
    found
}

/// Two readings, compared with spacing ignored.
///
/// **Ichiran space-separates the parts of a compound reading.** 失敗した comes back as
/// `しっぱい した` where the built-in analyzer says `しっぱいした` — the same reading written two
/// ways. Counting that as a disagreement put 531 entries in the table over 1,858 sentences, and
/// reading the list showed most of them were this and nothing else: 教えてください, 勉強する,
/// 許してくれた, every ～てください in the corpus.
///
/// Only whitespace is dropped, which `char::is_whitespace` extends to the ideographic space. A
/// different kana, or a long vowel written differently, is a real disagreement and stays one.
fn same_reading(left: &str, right: &str) -> bool {
    let squeeze =
        |reading: &str| -> String { reading.chars().filter(|c| !c.is_whitespace()).collect() };
    squeeze(left) == squeeze(right)
}

/// The surfaces, which is the segmentation itself.
fn surfaces(analysis: &Analysis) -> Vec<&str> {
    analysis
        .tokens
        .iter()
        .map(|token| token.surface.as_str())
        .collect()
}

/// How a token's conjugation reads, for comparison. `None` where the analyzer found none.
/// Just the form chain, without the lemma.
///
/// [`form_of`] deliberately joins the two, because a conjugation that names the wrong lemma is
/// wrong however right its forms are. For *decomposing* a disagreement they have to come apart:
/// the two analyzers differing about 読む vs 読み is a different fault from their differing about
/// Past vs Continuative, and a combined string cannot tell you which happened.
fn forms_of(token: &Token) -> Option<String> {
    let conj = token.conjugation.as_ref()?;
    Some(
        conj.props
            .iter()
            .map(|prop| prop.form.as_str())
            .collect::<Vec<_>>()
            .join("+"),
    )
}

fn form_of(token: &Token) -> Option<String> {
    let conj = token.conjugation.as_ref()?;
    let forms: Vec<&str> = conj.props.iter().map(|prop| prop.form.as_str()).collect();
    Some(format!("{}[{}]", conj.lemma, forms.join("+")))
}

#[derive(Default)]
struct Tally {
    passages: usize,
    same_segmentation: usize,
    tokens_ichiran: usize,
    tokens_lindera: usize,
    /// Tokens at the same surface position with the same surface.
    aligned: usize,
    /// Ichiran tokens whose span the other analyzer tiles exactly, with one token or several.
    spans_checked: usize,
    spans_identical: usize,
    spans_finer: usize,
    spans_conflicting: usize,
    /// Of the finer splits, how many contain Ichiran's dictionary form somewhere.
    finer_kept_lemma: usize,
    /// Of the finer splits, how many does the shipped dictionary hold as a single term?
    ///
    /// **An upper bound, and mostly an unusable one — read the sample before trusting it.**
    /// It measures 48.3%, which looks like most of the sidecar's longer units could be
    /// recovered by `dict::merge_names`'s existing shape: the dictionary confirming a run, as
    /// it already does for names.
    ///
    /// The sample says otherwise. Alongside real words — あの子, ご主人様, について — it holds
    /// それは (それ | は), そこから, だから, なので and ああああ. JMdict has entries for
    /// lexicalised particle sequences, so "the dictionary knows this string" is a far weaker
    /// claim for ordinary words than for names, where a name dictionary saying so is nearly
    /// conclusive. Joining それは would collapse a pronoun and a topic particle into one token
    /// and take the grammar layer's whole subject with it.
    ///
    /// Kept as a measurement because the number is tempting and the sample is the refutation.
    finer_known_to_the_dictionary: usize,
    same_dict_form: usize,
    same_reading: usize,
    /// The same, after the dictionary has had its say. Zero when none is installed.
    same_reading_corrected: usize,
    readings_corrected: usize,
    /// Ichiran offered several readings and the built-in one picked a listed alternative.
    /// **Not a disagreement**: Ichiran had no opinion to disagree with.
    reading_was_an_ichiran_alternative: usize,
    both_conjugated: usize,
    same_form: usize,
    /// Agreement once the two known differences of *convention* are set aside.
    ///
    /// Neither is an error and both were measured: the built-in analyzer names a dictionary
    /// form `Non-past` where Ichiran reports no conjugation at all (70.7% of the gap), and the
    /// two spell the same lemma differently — 成る against なる — in 301 of 393 disagreements.
    /// Counting either as a fault makes the headline read far worse than the analyzer behaves,
    /// which is how 52% came to look like a 48% error rate.
    conj_agreed_by_convention: usize,
    conj_lemma_spelling_only: usize,
    conj_bare_dictionary_form: usize,
    ichiran_only_conj: usize,
    lindera_only_conj: usize,
    ichiran_seconds: f64,
    lindera_seconds: f64,
}

#[test]
#[ignore = "needs Ichiran installed and a corpus"]
fn compare_the_two_analyzers_over_the_corpus() {
    let passages = passages();
    if passages.is_empty() {
        eprintln!(
            "SKIP: nothing to compare. Without SAKATSU_ANALYZER_SENTENCES this reads the corpus transcriptions in {}, the same gitignored directory `ocr_corpus` needs; point that variable at a newline-delimited sentence file to profile against volume instead.",
            corpus_dir().display()
        );
        return;
    }

    // Opened once. The point of the comparison is the readings, and the dictionary is what
    // this app already ships to settle them — leaving it out would measure a configuration
    // nobody runs.
    let jitendex = common::dictionaries_dir().join("jitendex");
    let dictionaries: Vec<Dictionary> = Dictionary::open(&jitendex).into_iter().collect();
    if dictionaries.is_empty() {
        eprintln!("note: no imported Jitendex — the corrected column will be empty");
    }

    let lindera = common::built_in().expect("lindera is compiled in");
    let ichiran = IchiranAnalyzer::new();
    // One call to find out, rather than discovering it 141 times.
    if ichiran.analyze("test").is_err() {
        eprintln!("SKIP: Ichiran did not answer — it needs WSL and its PostgreSQL database.");
        return;
    }

    let mut tally = Tally::default();
    // Disagreements worth reading, keyed so the output is stable between runs.
    let mut differences: BTreeMap<String, (String, String)> = BTreeMap::new();
    // Reading disagreements, counted by how often each one recurs. Which words they are
    // decides the fix: a missing reading is a coverage problem, a different one is a
    // disambiguation problem, and they want opposite remedies.
    let mut readings: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    // **The conjugation gap, decomposed.** 52% agreement sounds like a 48% error rate and is
    // not one: the two analyzers do not mean the same thing by "conjugated". These two maps are
    // what say so — the first names the form the built-in analyzer assigned where Ichiran
    // reported nothing at all, the second what the two called a token they both inflected.
    let mut lindera_only_forms: BTreeMap<String, usize> = BTreeMap::new();
    let mut conj_differences: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    let mut corrections: Vec<(String, String, String, String)> = Vec::new();
    // **Counted, not collected, and the count is the whole point at any real sample size.**
    //
    // This was a `BTreeSet` printed alphabetically and capped at thirty. Over a hundred passages
    // that was harmless because there were about thirty. Over two thousand there are several
    // times that, and an alphabetical cap shows あのね through ブレーキランプ and silently drops
    // the rest — which is not merely less information, it is a confident ranking that has nothing
    // to do with which candidates matter.
    //
    // Deciding a user dictionary is a frequency question: a compound split wrongly in one line
    // per thousand is noise, and one split wrongly in one line per ten is worth an entry. Only a
    // count can tell those apart.
    let mut rejoinable: BTreeMap<String, usize> = BTreeMap::new();

    for (name, text) in &passages {
        let started = Instant::now();
        let Ok(ichi) = ichiran.analyze(text) else {
            eprintln!("  {name}: Ichiran failed, skipping");
            continue;
        };
        tally.ichiran_seconds += started.elapsed().as_secs_f64();

        let started = Instant::now();
        let Ok(mut lind) = lindera.analyze(text) else {
            eprintln!("  {name}: Lindera failed, skipping");
            continue;
        };
        tally.lindera_seconds += started.elapsed().as_secs_f64();

        // Two copies: one as the analyzer left it, one after the dictionary has corrected it.
        // Both are scored, because the question is what the correction is worth, and that is
        // only answerable against the uncorrected number.
        let raw = lind.clone();
        let counts = sakatsu_core::dict::readings::correct(&mut lind, &dictionaries);
        tally.readings_corrected += counts.corrected;

        // Every correction, with what all three parties said. Agreement with Ichiran is a
        // proxy for truth, not truth — a correction that moves away from Ichiran may be
        // moving towards right — so these have to be read rather than counted.
        for after in &lind.tokens {
            let Some(before) = raw
                .tokens
                .iter()
                .find(|token| token.start == after.start && token.surface == after.surface)
            else {
                continue;
            };
            if before.reading == after.reading {
                continue;
            }
            let ichiran = ichi
                .tokens
                .iter()
                .find(|token| token.surface == after.surface)
                .map(|token| token.reading.clone())
                .unwrap_or_default();
            corrections.push((
                after.surface.clone(),
                before.reading.clone(),
                after.reading.clone(),
                ichiran,
            ));
        }

        tally.passages += 1;
        tally.tokens_ichiran += ichi.tokens.len();
        tally.tokens_lindera += lind.tokens.len();

        let (a, b) = (surfaces(&ichi), surfaces(&lind));
        if a == b {
            tally.same_segmentation += 1;
        } else if differences.len() < 25 {
            differences.insert(name.clone(), (a.join(" | "), b.join(" | ")));
        }

        // **Boundaries, not token identity.** Comparing surfaces counts `狙ってた` against
        // `狙っ | てた` as a disagreement, when it is the same reading at a finer grain — and
        // that one distinction was most of an apparent 86% disagreement. What matters for this
        // app is whether the two ever cut a word in a *different place*, which is what a
        // conflicting span means.
        let spans: Vec<(usize, usize)> = lind
            .tokens
            .iter()
            .filter_map(|token| Some((token.start?, token.end?)))
            .collect();
        for token in &ichi.tokens {
            let (Some(start), Some(end)) = (token.start, token.end) else {
                continue;
            };
            let inside: Vec<&(usize, usize)> = spans
                .iter()
                .filter(|(s, e)| *s < end && *e > start)
                .collect();
            if inside.is_empty() {
                continue;
            }
            tally.spans_checked += 1;

            let tiles = inside.first().is_some_and(|(s, _)| *s == start)
                && inside.last().is_some_and(|(_, e)| *e == end);
            if !tiles {
                tally.spans_conflicting += 1;
            } else if inside.len() == 1 {
                tally.spans_identical += 1;
            } else {
                tally.spans_finer += 1;
                if dictionaries.iter().any(|dictionary| {
                    dictionary
                        .lookup(&token.surface)
                        .map(|hits| hits.iter().any(|hit| hit.entry.term == token.surface))
                        .unwrap_or(false)
                }) {
                    tally.finer_known_to_the_dictionary += 1;
                    let pieces: Vec<&str> = inside
                        .iter()
                        .filter_map(|(s, e)| {
                            lind.tokens
                                .iter()
                                .find(|t| t.start == Some(*s) && t.end == Some(*e))
                                .map(|t| t.surface.as_str())
                        })
                        .collect();
                    *rejoinable
                        .entry(format!("{}  ({})", token.surface, pieces.join(" | ")))
                        .or_default() += 1;
                }
                // A finer split is only harmless if the word is still findable in it.
                if !token.dict_form.is_empty()
                    && lind.tokens.iter().any(|other| {
                        other.start.is_some_and(|s| s >= start)
                            && other.end.is_some_and(|e| e <= end)
                            && other.dict_form == token.dict_form
                    })
                {
                    tally.finer_kept_lemma += 1;
                }
            }
        }

        // Compared by surface position rather than by index: a single extra token early on
        // would otherwise misalign everything after it and report near-total disagreement.
        let mut by_start: BTreeMap<usize, &Token> = BTreeMap::new();
        for token in &lind.tokens {
            if let Some(start) = token.start {
                by_start.insert(start, token);
            }
        }
        for token in &ichi.tokens {
            let Some(start) = token.start else { continue };
            let Some(other) = by_start.get(&start) else {
                continue;
            };
            if other.surface != token.surface {
                continue;
            }
            tally.aligned += 1;
            if other.dict_form == token.dict_form {
                tally.same_dict_form += 1;
            }
            if raw.tokens.iter().any(|before| {
                before.start == other.start && same_reading(&before.reading, &token.reading)
            }) {
                tally.same_reading += 1;
            }
            if same_reading(&other.reading, &token.reading) {
                tally.same_reading_corrected += 1;
            } else if token
                .alternatives
                .iter()
                .any(|alt| !alt.reading.is_empty() && same_reading(&alt.reading, &other.reading))
            {
                // **Ichiran listed this reading itself and did not choose it.** 晴々 comes back
                // as せいせい and はればれ at an identical score of 65; the adapter takes the
                // first, and in 晴々として…良い気分 the first is the wrong one. Counting that as
                // a disagreement scores the built-in analyzer down for resolving a tie the
                // sidecar declined to.
                tally.reading_was_an_ichiran_alternative += 1;
            } else {
                *readings
                    .entry((
                        token.surface.clone(),
                        token.reading.clone(),
                        other.reading.clone(),
                    ))
                    .or_default() += 1;
            }
            match (form_of(token), form_of(other)) {
                (Some(one), Some(two)) => {
                    tally.both_conjugated += 1;
                    if one == two {
                        tally.same_form += 1;
                    } else {
                        // **Same word, spelled two ways.** Ichiran gives the kanji headword and
                        // the built-in analyzer the kana, so compare the lemma *readings*: if
                        // those agree and the form chains agree, the two said the same thing.
                        let reading_of = |token: &Token| {
                            token
                                .conjugation
                                .as_ref()
                                .map(|conj| conj.reading.clone())
                                .unwrap_or_default()
                        };
                        let same_reading = {
                            let (a, b) = (reading_of(token), reading_of(other));
                            !a.is_empty() && a == b
                        };
                        if same_reading && forms_of(token) == forms_of(other) {
                            tally.conj_agreed_by_convention += 1;
                            tally.conj_lemma_spelling_only += 1;
                        }
                        // Split by *what* differs. The lemma and the forms fail for different
                        // reasons and want different fixes.
                        let key = match (
                            token.conjugation.as_ref().map(|c| c.lemma.clone()),
                            other.conjugation.as_ref().map(|c| c.lemma.clone()),
                        ) {
                            (Some(a), Some(b)) if a != b => ("lemma".to_string(), a, b),
                            _ => (
                                "forms".to_string(),
                                forms_of(token).unwrap_or_default(),
                                forms_of(other).unwrap_or_default(),
                            ),
                        };
                        *conj_differences.entry(key).or_default() += 1;
                    }
                }
                (Some(_), None) => tally.ichiran_only_conj += 1,
                (None, Some(_)) => {
                    tally.lindera_only_conj += 1;
                    // Ichiran's `conj` records the derivation that produced a surface, so a word
                    // already in its dictionary form has none. Naming that form Non-past is a
                    // different bookkeeping, not a disagreement about the Japanese.
                    if forms_of(other).as_deref() == Some("Non-past") {
                        tally.conj_agreed_by_convention += 1;
                        tally.conj_bare_dictionary_form += 1;
                    }
                    *lindera_only_forms
                        .entry(forms_of(other).unwrap_or_default())
                        .or_default() += 1;
                }
                (None, None) => {}
            }
        }
    }

    report(&tally, &differences);
    report_conjugation(&lindera_only_forms, &conj_differences);
    report_readings(&readings);

    // Printed with the count, because the count on its own reads as an opportunity and these
    // are what it is actually made of.
    //
    // **Most frequent first, and expect the top of this list to be the worst candidates.** The
    // commonest strings the dictionary "knows" are lexicalised particle sequences — それは, には,
    // のに — so ranking by frequency puts the ones that must *not* be rejoined at the top rather
    // than hiding them in an alphabetical tail. That is the useful failure: the list now shows
    // what is common, and what is common turns out mostly to be what the header warns about.
    let mut ranked: Vec<(&String, &usize)> = rejoinable.iter().collect();
    // Count descending, then alphabetically, so ties do not reshuffle between runs.
    ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let shown = ranked.len().min(40);
    println!(
        "
=== words the dictionary would rejoin — note how many must not be ===
  {} distinct, {} occurrences in all; showing the {shown} most frequent",
        ranked.len(),
        ranked.iter().map(|(_, count)| **count).sum::<usize>(),
    );
    for (word, count) in ranked.iter().take(shown) {
        println!("  {count:5}x  {word}");
    }

    if !corrections.is_empty() {
        corrections.sort();
        corrections.dedup();
        println!(
            "
=== every reading the dictionary changed ==="
        );
        println!("  surface: unidic -> dictionary   (ichiran said)");
        for (surface, before, after, ichiran) in &corrections {
            let verdict = if after == ichiran {
                "now agrees"
            } else if before == ichiran {
                "MOVED AWAY from ichiran"
            } else {
                "all three differ"
            };
            println!(
                "  {surface}: {before} -> {after}   ({}) [{verdict}]",
                if ichiran.is_empty() { "—" } else { ichiran }
            );
        }
    }
}

/// The reading disagreements, most frequent first, split by what kind they are.
///
/// **Empty on one side is a different problem from different on both.** UniDic not knowing a
/// word at all is a coverage gap that a bigger dictionary fixes; UniDic choosing 行った as
/// いった where Ichiran chose おこなった is a disambiguation gap that a bigger dictionary does
/// not. Counting them together would average two unrelated things.
fn report_readings(readings: &BTreeMap<(String, String, String), usize>) {
    let mut rows: Vec<_> = readings.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1));

    let missing: usize = rows
        .iter()
        .filter(|((_, _, lind), _)| lind.is_empty())
        .map(|(_, count)| **count)
        .sum();
    let extra: usize = rows
        .iter()
        .filter(|((_, ichi, lind), _)| ichi.is_empty() && !lind.is_empty())
        .map(|(_, count)| **count)
        .sum();
    let differing: usize = rows
        .iter()
        .filter(|((_, ichi, lind), _)| !ichi.is_empty() && !lind.is_empty())
        .map(|(_, count)| **count)
        .sum();

    println!(
        "
=== reading disagreements ==="
    );
    println!("  lindera has none, ichiran does   {missing:>5}   (coverage)");
    println!("  ichiran has none, lindera does   {extra:>5}");
    println!("  both have one, they differ       {differing:>5}   (disambiguation)");
    println!(
        "
  most frequent, surface: ichiran / lindera"
    );
    for ((surface, ichi, lind), count) in rows.iter().take(30) {
        println!(
            "  {count:>3}x  {surface}: {} / {}",
            if ichi.is_empty() { "—" } else { ichi },
            if lind.is_empty() { "—" } else { lind }
        );
    }
}

/// Why the conjugation numbers are what they are.
///
/// **52% agreement is not a 48% error rate**, and this is the section that says so. The two
/// analyzers do not mean the same thing by the word: Ichiran's `conj` records the derivation that
/// produced the surface, so a word already in its dictionary form has an empty one; the built-in
/// analyzer names the form the token is *in*, and for a dictionary form that name is Non-past.
/// 読む is then a conjugation to one and not to the other, with neither being wrong.
///
/// So the first table is the important one. If Non-past dominates it, the gap is a difference of
/// convention and closing it means agreeing what to count, not fixing a reconstruction. Whatever
/// is left under other forms is the part that might be a real fault, and it is much smaller than
/// the headline implies.
fn report_conjugation(
    lindera_only: &BTreeMap<String, usize>,
    differences: &BTreeMap<(String, String, String), usize>,
) {
    let total: usize = lindera_only.values().sum();
    println!(
        "
=== conjugation, decomposed ==="
    );
    println!("  the built-in analyzer found one where Ichiran found none: {total}");
    let mut rows: Vec<_> = lindera_only.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (form, count) in rows.iter().take(12) {
        println!(
            "  {:>6}  ({:>4.1}%)  {}",
            count,
            percent(**count, total),
            if form.is_empty() {
                "(no form named)"
            } else {
                form
            }
        );
    }

    println!(
        "
  where both found one and they differed"
    );
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for ((kind, _, _), count) in differences {
        *by_kind.entry(kind.as_str()).or_default() += count;
    }
    for (kind, count) in &by_kind {
        println!("  {count:>6}  {kind}");
    }
    let mut rows: Vec<_> = differences.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!(
        "
  most frequent, ichiran / lindera"
    );
    for ((kind, one, two), count) in rows.iter().take(15) {
        println!("  {count:>5}x  [{kind}] {one} / {two}");
    }
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    100.0 * part as f64 / whole as f64
}

fn report(tally: &Tally, differences: &BTreeMap<String, (String, String)>) {
    let n = tally.passages;
    println!("\n=== {n} passages ===\n");

    println!("Segmentation");
    println!(
        "  identical token sequence      {:>5} / {n}  ({:.1}%)",
        tally.same_segmentation,
        percent(tally.same_segmentation, n)
    );
    println!(
        "  tokens                        ichiran {}, lindera {}",
        tally.tokens_ichiran, tally.tokens_lindera
    );
    println!(
        "  tokens aligned by position    {:>5}  ({:.1}% of ichiran's)",
        tally.aligned,
        percent(tally.aligned, tally.tokens_ichiran)
    );

    println!(
        "
Boundaries — does the built-in one ever cut a word differently?"
    );
    println!(
        "  same span                     {:>5} / {}  ({:.1}%)",
        tally.spans_identical,
        tally.spans_checked,
        percent(tally.spans_identical, tally.spans_checked)
    );
    println!(
        "  split finer, same edges       {:>5}  ({:.1}%), of which {} keep the lemma",
        tally.spans_finer,
        percent(tally.spans_finer, tally.spans_checked),
        tally.finer_kept_lemma
    );
    println!(
        "    of those, the dictionary knows the whole word  {:>4}  ({:.1}%)",
        tally.finer_known_to_the_dictionary,
        percent(tally.finer_known_to_the_dictionary, tally.spans_finer)
    );
    println!(
        "  cut in a different place      {:>5}  ({:.1}%)  <- the only real disagreement",
        tally.spans_conflicting,
        percent(tally.spans_conflicting, tally.spans_checked)
    );

    println!("\nWhere they align, do they agree?");
    println!(
        "  dictionary form               {:>5} / {}  ({:.1}%)",
        tally.same_dict_form,
        tally.aligned,
        percent(tally.same_dict_form, tally.aligned)
    );
    println!(
        "  reading, as the analyzer left it {:>4} / {}  ({:.1}%)",
        tally.same_reading,
        tally.aligned,
        percent(tally.same_reading, tally.aligned)
    );
    println!(
        "  reading, dictionary-corrected {:>5} / {}  ({:.1}%)   [{} corrected]",
        tally.same_reading_corrected,
        tally.aligned,
        percent(tally.same_reading_corrected, tally.aligned),
        tally.readings_corrected
    );
    println!(
        "  + ichiran listed it as an alternative {:>3}          ({:.1}% counting those)",
        tally.reading_was_an_ichiran_alternative,
        percent(
            tally.same_reading_corrected + tally.reading_was_an_ichiran_alternative,
            tally.aligned
        )
    );

    println!("\nConjugation — the part UniDic has to rebuild");
    println!(
        "  both found one                {:>5}, agreeing on {} ({:.1}%)",
        tally.both_conjugated,
        tally.same_form,
        percent(tally.same_form, tally.both_conjugated)
    );
    println!(
        "  ichiran only                  {:>5}",
        tally.ichiran_only_conj
    );
    println!(
        "  lindera only                  {:>5}",
        tally.lindera_only_conj
    );

    // **The honest figure, beside the strict one.** Both belong here: the strict number is what
    // a naive comparison reports and the reason this looked like the project's worst accuracy
    // gap, and the adjusted one is what is left after the two differences of bookkeeping that
    // were measured rather than assumed. Printing only the second would be flattering; printing
    // only the first was misleading, which is how it read for three runs.
    let comparable = tally.both_conjugated + tally.ichiran_only_conj + tally.lindera_only_conj;
    println!(
        "  once the two conventions are set aside: {} of {} ({:.1}%)",
        tally.same_form + tally.conj_agreed_by_convention,
        comparable,
        percent(
            tally.same_form + tally.conj_agreed_by_convention,
            comparable
        )
    );
    println!(
        "    the same lemma spelled two ways     {:>5}",
        tally.conj_lemma_spelling_only
    );
    println!(
        "    a dictionary form Ichiran leaves bare {:>3}",
        tally.conj_bare_dictionary_form
    );

    println!("\nCost");
    println!(
        "  ichiran                       {:>7.2}s total, {:.3}s per passage",
        tally.ichiran_seconds,
        tally.ichiran_seconds / n.max(1) as f64
    );
    println!(
        "  lindera                       {:>7.4}s total, {:.5}s per passage",
        tally.lindera_seconds,
        tally.lindera_seconds / n.max(1) as f64
    );

    if !differences.is_empty() {
        println!("\n=== segmentation differences, for reading by hand ===");
        for (name, (ichi, lind)) in differences {
            println!("\n{name}");
            println!("  ichiran: {ichi}");
            println!("  lindera: {lind}");
        }
    }
}
