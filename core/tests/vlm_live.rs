//! `ocr::vlm` against a real server.
//!
//! Ignored by default: it needs something running that speaks OpenAI chat-completions and
//! holds a vision model. Point it at one and run:
//!
//! ```text
//! $env:SAKATSU_VLM_ENDPOINT = "http://127.0.0.1:9999"
//! $env:SAKATSU_VLM_MODEL    = ""            # llama-server ignores it; Ollama needs it
//! cargo test -p sakatsu-core --test vlm_live -- --ignored --nocapture
//! ```
//!
//! # What this can and cannot establish
//!
//! It establishes that **our client** is right: that the crop encodes to a PNG the server
//! accepts, that the request shape is one it understands, that the reply parses, and that
//! an unreachable endpoint fails by naming itself rather than leaking a transport error.
//!
//! It establishes nothing about a *model's* accuracy. Whatever is on the other end is
//! whatever the machine happens to have; `ocr_corpus.rs` is where reading quality is
//! measured, and it needs real material to say anything.

use sakatsu_core::ocr::{Ocr, OcrError, OcrImage, VlmOcr};

fn endpoint() -> Option<String> {
    std::env::var("SAKATSU_VLM_ENDPOINT")
        .ok()
        .filter(|url| !url.is_empty())
}

fn model() -> String {
    std::env::var("SAKATSU_VLM_MODEL").unwrap_or_default()
}

/// Render text as black on white, the same way the other live tests do.
///
/// GDI through PowerShell rather than a font-rasterising dependency: what matters is that
/// real glyphs reach the model.
fn render(text: &str) -> OcrImage {
    let out = std::env::temp_dir().join("sakatsu-vlm-live.png");
    let escaped = text.replace('\'', "''");
    let script = format!(
        r#"Add-Type -AssemblyName System.Drawing
$f = New-Object System.Drawing.Font('Yu Gothic UI', 24)
$probe = New-Object System.Drawing.Bitmap(1,1)
$pg = [System.Drawing.Graphics]::FromImage($probe)
$m = $pg.MeasureString('{escaped}', $f)
$pg.Dispose(); $probe.Dispose()
[int]$w = [math]::Ceiling($m.Width) + 24
[int]$h = [math]::Ceiling($m.Height) + 16
$b = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($b)
$g.Clear([System.Drawing.Color]::White)
$g.TextRenderingHint = 'AntiAliasGridFit'
$g.DrawString('{escaped}', $f, [System.Drawing.Brushes]::Black, 12, 8)
$g.Dispose()
$b.Save('{path}', [System.Drawing.Imaging.ImageFormat]::Png)
$b.Dispose()"#,
        path = out.display().to_string().replace('\\', "\\\\"),
    );

    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .expect("powershell runs");
    assert!(status.success(), "rendering failed");

    let decoded = image::open(&out).expect("the rendered png decodes");
    let rgba = decoded.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    OcrImage::new(width, height, rgba.into_raw()).expect("well-formed")
}

/// The whole path: encode, send, parse. Not an accuracy check — see the module note.
#[test]
#[ignore = "needs a vision model served over an OpenAI-compatible endpoint"]
fn a_served_model_reads_a_rendered_line() {
    let Some(url) = endpoint() else {
        println!("SKIP: set SAKATSU_VLM_ENDPOINT to run this");
        return;
    };

    let engine = VlmOcr::new(&url, &model()).expect("valid VLM endpoint");
    if !engine.is_reachable() {
        println!("SKIP: nothing answering at {url}");
        return;
    }

    let image = render("気持ちが伝わってきます");
    match engine.recognize(&image) {
        Ok(text) => {
            println!("read back: {text:?}");
            assert!(
                !text.trim().is_empty(),
                "a legible rendered line should produce something"
            );
        }
        Err(error) => panic!("the request failed: {error}"),
    }
}

/// A server that is not there has to say which one.
///
/// This is the failure the user will actually hit — the endpoint is a setting, and the
/// server is theirs to start. "Connection refused" with no address in it would leave them
/// checking the wrong thing.
#[test]
fn an_unreachable_endpoint_names_itself() {
    // Port 1 is reserved and nothing listens on it.
    let engine = VlmOcr::new("http://127.0.0.1:1", "").expect("valid VLM endpoint");
    assert!(!engine.is_reachable());

    let image = OcrImage::new(2, 2, vec![255u8; 16]).expect("well-formed");
    match engine.recognize(&image) {
        Err(OcrError::EngineUnavailable(message)) => {
            assert!(
                message.contains("127.0.0.1:1"),
                "the message must name the endpoint, said {message}"
            );
        }
        other => panic!("expected an unavailable-engine error, got {other:?}"),
    }
}
