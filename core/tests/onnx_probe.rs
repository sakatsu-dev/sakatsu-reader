//! Prints the real ONNX graph signatures of the manga-ocr export.
//!
//! Ignored by default: it needs `scripts/fetch-ocr-model.ps1` to have run. Run it
//! before touching the inference loop:
//!
//! ```text
//! cargo test -p sakatsu-core --test onnx_probe -- --ignored --nocapture
//! ```
//!
//! This exists because guessing tensor names is how this project has been bitten
//! twice already — Ichiran's `alternative` wrapper and Jitendex's `sense-group` were
//! both "the documented shape was not the real shape". Input and output names are the
//! API of a model file, so they get read, not assumed.

use std::path::PathBuf;

fn model_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let appdata = std::env::var_os("APPDATA").expect("APPDATA (or set SAKATSU_MODEL_DIR)");
    PathBuf::from(appdata)
        .join("com.sakatsu.app")
        .join("models")
        .join("manga-ocr")
}

fn describe(label: &str, path: PathBuf) {
    println!("\n=== {label}: {}", path.display());
    if !path.exists() {
        println!("  MISSING — run scripts/fetch-ocr-model.ps1");
        return;
    }

    let session = sakatsu_core::onnx::session_builder()
        .expect("session builder")
        .commit_from_file(&path)
        .unwrap_or_else(|error| panic!("load {}: {error}", path.display()));

    println!("  inputs:");
    for input in session.inputs() {
        println!("    {:<28} {:?}", input.name(), input.dtype());
    }
    println!("  outputs:");
    for output in session.outputs() {
        println!("    {:<28} {:?}", output.name(), output.dtype());
    }
}

/// FuguMT's graphs, which differ from manga-ocr's in ways the loop has to respect —
/// notably whether the decoder wants an encoder attention mask.
#[test]
#[ignore = "needs scripts/fetch-translation-model.ps1"]
fn show_the_translation_model_signatures() {
    let dir = model_dir()
        .parent()
        .expect("models dir")
        .join("fugumt-ja-en");
    describe("mt encoder", dir.join("encoder_model.onnx"));
    describe("mt decoder", dir.join("decoder_model.onnx"));

    // The tokenizer decides what the ids mean, so its vocabulary size has to match
    // the decoder's logits.
    //
    // Through the app's own loader, not `Tokenizer::from_file`. This export sets
    // `precompiled_charsmap` to null and the `tokenizers` crate **panics** on it rather
    // than returning an error — so the version of this that called `from_file` and matched
    // on the result died inside a dependency and the `Err` arm below was unreachable.
    // `fugumt::load_tokenizer` is the workaround the app has always used.
    let tokenizer = dir.join("tokenizer.json");
    match sakatsu_core::translate::fugumt::load_tokenizer(&tokenizer) {
        Ok(tokenizer) => {
            println!(
                "\n=== tokenizer.json: {} tokens",
                tokenizer.get_vocab_size(true)
            );
            let encoded = tokenizer.encode("水を飲む", true).expect("encode");
            println!("  '水を飲む' → {:?}", encoded.get_ids());
            println!("  tokens     → {:?}", encoded.get_tokens());
        }
        Err(error) => println!("\n=== tokenizer.json unreadable: {error}"),
    }
}

/// PP-OCRv6's graphs, and the one number that silently ruins everything.
///
/// The recogniser is CTC-decoded against a character dictionary: its last output
/// dimension is the number of classes, which must be the dictionary's length plus the
/// blank (and, in PaddleOCR's dictionaries, a trailing space entry). Get that off by one
/// and every character after the mismatch decodes to its neighbour — plausible-looking
/// Japanese that is quietly wrong, which is far worse than an error.
#[test]
#[ignore = "needs the PP-OCRv6 models"]
fn show_the_paddle_signatures() {
    let dir = model_dir().parent().expect("models dir").join("ppocrv6");
    describe("paddle det", dir.join("det.onnx"));
    describe("paddle rec", dir.join("rec.onnx"));

    let keys = dir.join("ppocrv6_dict.txt");
    match std::fs::read_to_string(&keys) {
        Ok(text) => {
            // `lines()` drops a trailing newline, which matters here: PaddleOCR's
            // dictionaries often end with a blank line standing for the space character.
            let lines: Vec<&str> = text.split('\n').collect();
            let entries: Vec<&str> = text.lines().collect();
            println!("\n=== ppocrv6_dict.txt");
            println!(
                "  lines(): {}  split('\\n'): {}",
                entries.len(),
                lines.len()
            );
            println!("  first 8: {:?}", &entries[..entries.len().min(8)]);
            println!(
                "  last 4:  {:?}",
                &entries[entries.len().saturating_sub(4)..]
            );
            let japanese = entries
                .iter()
                .filter(|c| c.chars().any(|c| ('\u{3040}'..='\u{30FF}').contains(&c)))
                .count();
            println!("  kana entries: {japanese}");
        }
        Err(error) => println!("\n=== ppocrv6_dict.txt unreadable: {error}"),
    }
}

#[test]
#[ignore = "needs scripts/fetch-ocr-model.ps1"]
fn show_the_model_signatures() {
    let dir = model_dir();
    describe("encoder", dir.join("encoder_model.onnx"));
    describe("decoder", dir.join("decoder_model.onnx"));

    // The vocabulary decides how token ids become text, so its size and its first few
    // entries (the special tokens) matter as much as the graphs.
    let vocab = dir.join("vocab.txt");
    if let Ok(text) = std::fs::read_to_string(&vocab) {
        let lines: Vec<&str> = text.lines().collect();
        println!("\n=== vocab.txt: {} tokens", lines.len());
        println!("  first 8: {:?}", &lines[..lines.len().min(8)]);
        // generation_config says decoder_start=2, eos=3, pad=0 — check what those are.
        for id in [0usize, 1, 2, 3] {
            println!("  id {id} = {:?}", lines.get(id));
        }
    } else {
        println!("\n=== vocab.txt MISSING");
    }
}
