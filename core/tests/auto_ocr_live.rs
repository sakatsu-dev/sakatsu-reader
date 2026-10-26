//! Auto OCR against both installed conventional models.
//!
//! The policy itself is unit-tested without models. This ignored test proves that the
//! direction evidence reaches it from the real PP-OCRv6 detector and that the selected
//! output remains correct for both horizontal and vertical fixtures.
//!
//! ```text
//! cargo test -p sakatsu-core --test auto_ocr_live -- --ignored --nocapture
//! ```

mod common;

use std::path::PathBuf;
use std::time::Instant;

use common::render_ocr_lines;
use sakatsu_core::ocr::score::normalize;
use sakatsu_core::ocr::{AutoOcr, MangaOcr, Ocr, PaddleOcr};

fn models(name: &str) -> PathBuf {
    let appdata = std::env::var_os("APPDATA").expect("APPDATA");
    PathBuf::from(appdata)
        .join("com.sakatsu.app")
        .join("models")
        .join(name)
}

#[test]
#[ignore = "needs manga-ocr and PP-OCRv6"]
fn the_installed_models_route_horizontal_and_vertical_text() {
    let manga_dir = models("manga-ocr");
    let paddle_dir = models("ppocrv6");
    if !MangaOcr::is_present(&manga_dir) || !PaddleOcr::is_present(&paddle_dir) {
        println!("SKIP: install manga-ocr and PP-OCRv6 before testing Auto OCR");
        return;
    }

    let auto = AutoOcr::new(manga_dir, paddle_dir);
    for (label, lines, vertical) in [
        ("horizontal", ["設定を開く", "名前を保存"], false),
        ("vertical", ["水を飲む", "本を読む"], true),
    ] {
        let image = render_ocr_lines(&lines, vertical);
        let expected = lines.join("\n");
        let started = Instant::now();
        let got = auto.recognize(&image).expect("Auto OCR reads the fixture");
        println!(
            "Auto {label:<10} {:>6.2}s  {got:?}",
            started.elapsed().as_secs_f64()
        );
        assert_eq!(normalize(&got), normalize(&expected), "{label} read");
    }
}
