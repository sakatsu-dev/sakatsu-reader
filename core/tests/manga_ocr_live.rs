//! manga-ocr against the real model.
//!
//! Ignored by default: needs `scripts/fetch-ocr-model.ps1` (~440 MB) and CI has no
//! model. Run:
//!
//! ```text
//! cargo test -p sakatsu-core --test manga_ocr_live -- --ignored --nocapture
//! ```
//!
//! The images are rendered here rather than committed, so the test carries no fixture
//! and no licence question. They are deliberately plain — this checks that the
//! pipeline (preprocess → encode → decode → detokenize) produces the right *text*, not
//! that the model copes with hand-lettered manga.

use std::path::PathBuf;

use sakatsu_core::ocr::{MangaOcr, Ocr, OcrImage};

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

/// Render `text` as black on white using a font Windows ships with.
///
/// Uses GDI through PowerShell rather than adding a font-rasterising dependency for a
/// test: what matters is that real glyphs reach the model.
fn render(text: &str) -> OcrImage {
    let out = std::env::temp_dir().join(format!(
        "sakatsu-ocr-{}-{}.png",
        text.chars().count(),
        std::process::id()
    ));
    let script = format!(
        r#"
Add-Type -AssemblyName System.Drawing
$text = [char[]]@({codes}) -join ''
$font = New-Object System.Drawing.Font('Yu Gothic UI', 44)
$bmp = New-Object System.Drawing.Bitmap 1, 1
$g = [System.Drawing.Graphics]::FromImage($bmp)
$size = $g.MeasureString($text, $font)
$g.Dispose(); $bmp.Dispose()
$w = [int]$size.Width + 24; $h = [int]$size.Height + 24
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::White)
$g.TextRenderingHint = 'AntiAliasGridFit'
$g.DrawString($text, $font, [System.Drawing.Brushes]::Black, 12, 12)
$g.Dispose()
$bmp.Save('{path}', [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
"#,
        // Passed as code points so no encoding sits between here and the glyphs.
        codes = text
            .chars()
            .map(|c| format!("0x{:X}", c as u32))
            .collect::<Vec<_>>()
            .join(","),
        path = out.display().to_string().replace('\\', "\\\\"),
    );

    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .expect("run powershell");
    assert!(status.success(), "rendering {text:?} failed");

    let bytes = std::fs::read(&out).expect("read rendered png");
    let decoded = image::load_from_memory(&bytes).expect("decode png");
    let rgba = decoded.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    let _ = std::fs::remove_file(&out);
    OcrImage::new(width, height, rgba.into_raw()).expect("valid image")
}

#[test]
#[ignore = "needs scripts/fetch-ocr-model.ps1"]
fn the_model_reads_rendered_japanese() {
    let dir = model_dir();
    assert!(
        MangaOcr::is_present(&dir),
        "no model at {} — run scripts/fetch-ocr-model.ps1",
        dir.display()
    );

    let started = std::time::Instant::now();
    let engine = MangaOcr::load(&dir).expect("load the model");
    println!("loaded in {:.1}s", started.elapsed().as_secs_f64());
    assert_eq!(engine.engine(), "manga-ocr");

    for expected in ["水を飲む", "気持ち", "先生"] {
        let image = render(expected);
        let started = std::time::Instant::now();
        let text = engine.recognize(&image).expect("recognize");
        println!(
            "{:>6}x{:<4} → {text:?}  (expected {expected:?}, {:.2}s)",
            image.width(),
            image.height(),
            started.elapsed().as_secs_f64()
        );
        assert_eq!(
            text, expected,
            "recognized {text:?} instead of {expected:?}"
        );
    }
}

#[test]
#[ignore = "needs scripts/fetch-ocr-model.ps1"]
fn an_empty_image_is_refused_before_any_inference() {
    let engine = MangaOcr::load(&model_dir()).expect("load the model");
    let empty = OcrImage::new(0, 0, Vec::new()).expect("valid");
    assert!(engine.recognize(&empty).is_err());
}
