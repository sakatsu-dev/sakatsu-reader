//! Conservative selection between the two conventional local OCR engines.
//!
//! This does not claim to identify manga from pixels. PP-OCRv6 already detects every text
//! line and determines the passage direction, so Auto uses that evidence: clean horizontal
//! output stays with PP-OCRv6; vertical, empty, or partly rejected output is checked against
//! manga-ocr. The paused VLM adapter is never entered from Auto because a generative model
//! can answer fluently when it is wrong.
//!
//! # Re-examined for PP-OCRv6 (2026-10-09), and kept
//!
//! These rules were tuned on PP-OCRv5's evidence, so they were re-examined when v6 replaced it.
//! Five candidates, each a subset of the rules here, were written down before anything was
//! scored. They were compared on two thirds of the development captures, with the last third
//! sealed until one was chosen (`tests/auto_evidence.rs`).
//!
//! **The current rules scored best**: 0.060 mean character error on the tuning third. Dropping
//! the vertical-disagreement rule scored 0.087, and keeping manga-ocr only for an empty read
//! scored 0.101. On the sealed third the rules scored 0.125, against Auto-with-v5's 0.117 on
//! the same captures — two better, two worse. Over all 134 captures it is 0.080 against v5's
//! 0.086, which is no significant difference, at about a third less time per read.
//!
//! What a perfect chooser would still gain (0.028 on the tuning third) sits almost entirely in
//! the vertical disagreements. v6's own line confidence does not separate the ones v6 reads
//! better: the two groups' ranges overlap almost completely. So no threshold was added.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::paddle::{PaddleRead, ReadingDirection};
use super::score::normalize;
use super::{MangaOcr, Ocr, OcrError, OcrImage, PaddleOcr};

pub struct AutoOcr {
    manga_dir: PathBuf,
    paddle_dir: PathBuf,
    models: Arc<Models>,
}

struct Models {
    manga: Mutex<Option<Arc<MangaOcr>>>,
    paddle: Mutex<Option<Arc<PaddleOcr>>>,
    last_use: Mutex<Instant>,
    cleanup_running: AtomicBool,
}

/// Keep both engines warm for a short burst of captures, not for the rest of the process.
/// manga-ocr is the larger fallback; PP-OCRv6 remains because Auto needs its layout evidence
/// at the start of every decision.
const SECOND_ENGINE_IDLE: Duration = Duration::from_secs(2 * 60);

impl AutoOcr {
    pub fn new(manga_dir: PathBuf, paddle_dir: PathBuf) -> AutoOcr {
        AutoOcr {
            manga_dir,
            paddle_dir,
            models: Arc::new(Models {
                manga: Mutex::new(None),
                paddle: Mutex::new(None),
                last_use: Mutex::new(Instant::now()),
                cleanup_running: AtomicBool::new(false),
            }),
        }
    }

    pub fn is_present(manga_dir: &Path, paddle_dir: &Path) -> bool {
        MangaOcr::is_present(manga_dir) || PaddleOcr::is_present(paddle_dir)
    }

    fn manga(&self) -> Result<Option<Arc<MangaOcr>>, OcrError> {
        if !MangaOcr::is_present(&self.manga_dir) {
            return Ok(None);
        }
        let mut loaded = self
            .models
            .manga
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if loaded.is_none() {
            *loaded = Some(Arc::new(MangaOcr::load(&self.manga_dir)?));
        }
        Ok(loaded.as_ref().map(Arc::clone))
    }

    fn paddle(&self) -> Result<Option<Arc<PaddleOcr>>, OcrError> {
        if !PaddleOcr::is_present(&self.paddle_dir) {
            return Ok(None);
        }
        let mut loaded = self
            .models
            .paddle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if loaded.is_none() {
            *loaded = Some(Arc::new(PaddleOcr::open(&self.paddle_dir)?));
        }
        Ok(loaded.as_ref().map(Arc::clone))
    }

    fn manga_fallback(
        &self,
        image: &OcrImage,
        paddle_error: Option<OcrError>,
    ) -> Result<String, OcrError> {
        match self.manga() {
            Ok(Some(manga)) => match manga.recognize(image) {
                Ok(text) => {
                    crate::log_info!("ocr-auto", "manga-ocr used as the available fallback");
                    Ok(text)
                }
                Err(manga_error) => Err(combine_errors(paddle_error, manga_error)),
            },
            Ok(None) => Err(paddle_error.unwrap_or_else(no_models)),
            Err(manga_error) => Err(combine_errors(paddle_error, manga_error)),
        }
    }

    fn choose_after_paddle(
        &self,
        image: &OcrImage,
        paddle: PaddleRead,
    ) -> Result<(String, AutoReason), OcrError> {
        if !should_check_manga(&paddle) {
            crate::log_info!("ocr-auto", "pp-ocrv6 kept for a confident horizontal read");
            return Ok((paddle.text, AutoReason::ConfidentHorizontal));
        }

        let manga = match self.manga() {
            Ok(Some(manga)) => manga,
            Ok(None) => return Ok((paddle.text, AutoReason::MangaUnavailable)),
            Err(error) if !paddle.text.trim().is_empty() => {
                crate::log_error!(
                    "ocr-auto",
                    "manga-ocr could not load; keeping PP-OCRv6: {error}"
                );
                return Ok((paddle.text, AutoReason::MangaUnavailable));
            }
            Err(error) => return Err(error),
        };
        let manga_text = match manga.recognize(image) {
            Ok(text) => text,
            Err(error) if !paddle.text.trim().is_empty() => {
                crate::log_error!("ocr-auto", "manga-ocr failed; keeping PP-OCRv6: {error}");
                return Ok((paddle.text, AutoReason::MangaUnavailable));
            }
            Err(error) => return Err(error),
        };

        // The reason travels with the text now. Same rules, same outcome — what is new is that
        // the caller can be told which one fired.
        let (choice, reason) = pick(&paddle, &manga_text);
        match choice {
            Pick::Paddle => {
                crate::log_info!(
                    "ocr-auto",
                    "pp-ocrv6 kept after the manga-ocr check ({:?}, {})",
                    paddle.direction,
                    reason.label()
                );
                Ok((paddle.text, reason))
            }
            Pick::Manga => {
                crate::log_info!(
                    "ocr-auto",
                    "manga-ocr chosen after the PP-OCRv6 check ({:?}, {})",
                    paddle.direction,
                    reason.label()
                );
                Ok((manga_text, reason))
            }
        }
    }
}

impl AutoOcr {
    /// Read, and say which rule decided it.
    ///
    /// [`Ocr::recognize`] answers the question the pipeline asks — what does this crop say. This
    /// answers the one a measurement asks, and the one the interface will eventually want: *why
    /// that engine*. The routing is identical; only the reporting is added.
    pub fn recognize_explained(&self, image: &OcrImage) -> Result<(String, AutoReason), OcrError> {
        if image.is_empty() {
            return Err(OcrError::EmptyImage);
        }

        let outcome = match self.paddle() {
            Ok(Some(paddle)) => match paddle.recognize_detailed(image) {
                Ok(read) => self.choose_after_paddle(image, read),
                Err(error) => self
                    .manga_fallback(image, Some(error))
                    .map(|text| (text, AutoReason::PaddleIncomplete)),
            },
            Ok(None) => self
                .manga_fallback(image, None)
                .map(|text| (text, AutoReason::PaddleIncomplete)),
            Err(error) => self
                .manga_fallback(image, Some(error))
                .map(|text| (text, AutoReason::PaddleIncomplete)),
        };
        Models::defer_trim(&self.models);
        outcome
    }
}

impl Ocr for AutoOcr {
    fn warm(&self) -> Result<(), OcrError> {
        let paddle = self.paddle();
        let manga = self.manga();
        // Startup must retain the same idle eviction policy as a burst of captures.
        Models::defer_trim(&self.models);
        match (paddle, manga) {
            (Err(first), Err(second)) => Err(combine_errors(Some(first), second)),
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(None), Ok(None)) => Err(no_models()),
            _ => Ok(()),
        }
    }
    fn recognize(&self, image: &OcrImage) -> Result<String, OcrError> {
        self.recognize_explained(image).map(|(text, _)| text)
    }

    fn engine(&self) -> &str {
        "auto"
    }

    /// The model Auto handed this image to, not "auto".
    fn recognize_attributed(&self, image: &OcrImage) -> Result<(String, String), OcrError> {
        let (text, reason) = self.recognize_explained(image)?;
        let produced_by = if reason.chose_manga() {
            "manga-ocr"
        } else {
            crate::ocr::paddle::ENGINE
        };
        Ok((text, produced_by.to_string()))
    }
}

#[cfg(test)]
mod startup_warm_tests {
    use super::*;

    #[test]
    fn absent_models_do_not_count_as_a_successful_warmup() {
        let absent =
            std::env::temp_dir().join(format!("sakatsu-no-ocr-models-{}", std::process::id()));
        let auto = AutoOcr::new(absent.join("manga-ocr"), absent.join("pp-ocrv6"));
        assert!(auto.warm().is_err());
    }

    #[test]
    #[ignore = "needs both installed OCR engines in SAKATSU_EVAL_MODEL_DIR"]
    fn startup_loads_both_real_models_without_running_recognition() {
        let root = PathBuf::from(
            std::env::var_os("SAKATSU_EVAL_MODEL_DIR").expect("set the isolated model root"),
        );
        let manga = root.join("manga-ocr");
        let paddle = root.join("ppocrv6");
        assert!(MangaOcr::is_present(&manga) && PaddleOcr::is_present(&paddle));
        let auto = AutoOcr::new(manga, paddle);
        assert!(auto.models.manga.lock().unwrap().is_none());
        assert!(auto.models.paddle.lock().unwrap().is_none());
        auto.warm().expect("both actual models initialize");
        assert!(auto.models.manga.lock().unwrap().is_some());
        assert!(auto.models.paddle.lock().unwrap().is_some());
    }
}

impl Models {
    fn defer_trim(models: &Arc<Self>) {
        *models
            .last_use
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
        if models
            .cleanup_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let weak = Arc::downgrade(models);
        std::thread::spawn(move || {
            loop {
                let Some(models) = weak.upgrade() else {
                    return;
                };
                let mut last_use = models
                    .last_use
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let elapsed = last_use.elapsed();
                if elapsed < SECOND_ENGINE_IDLE {
                    let wait = SECOND_ENGINE_IDLE - elapsed;
                    drop(last_use);
                    drop(models);
                    std::thread::sleep(wait);
                    continue;
                }

                let paddle_loaded = models
                    .paddle
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .is_some();
                if paddle_loaded {
                    // Clearing the cache is safe even if an inference owns an Arc clone.
                    // In that case the model lives until that inference, and only it, ends.
                    *models
                        .manga
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
                }
                models.cleanup_running.store(false, Ordering::Release);
                // Keep the timestamp lock until the flag is clear. A capture finishing now
                // then sees `false` and starts the next cleanup instead of losing the wakeup.
                *last_use = Instant::now();
                return;
            }
        });
    }
}

fn no_models() -> OcrError {
    OcrError::EngineUnavailable(
        "Screen capture setup is incomplete — resume setup from Settings > Recognition".into(),
    )
}

fn combine_errors(paddle: Option<OcrError>, manga: OcrError) -> OcrError {
    match paddle {
        Some(paddle) => OcrError::Failed(format!(
            "Auto OCR could not use PP-OCRv6 ({paddle}) or manga-ocr ({manga})"
        )),
        None => manga,
    }
}

fn should_check_manga(read: &PaddleRead) -> bool {
    read.text.trim().is_empty()
        || read.accepted_lines < read.detected_lines
        || read.direction == ReadingDirection::Vertical
}

/// Which rule decided the read, and therefore which engine's text came back.
///
/// **The policy was only ever visible in the log.** Scoring the router against a corpus
/// showed it losing to a perfect chooser on 13 of 131 distinct samples, 18 of 19 of them in
/// the same direction — handed to manga-ocr where PP-OCRv5 had read better. Knowing *that*
/// is not enough to fix it; the rule that fired is what says whether the fault is the
/// rejected-line trigger or the vertical one.
///
/// Public because the interface wants it too: `local documentation` records that a
/// disagreement between the two engines is evidence the read is doubtful and is worth
/// surfacing quietly. This is that evidence, named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoReason {
    /// PP-OCRv6 read cleanly and horizontally. manga-ocr was never loaded.
    ConfidentHorizontal,
    /// manga-ocr was consulted and had nothing to say, so PP-OCRv6 stood.
    MangaSilent,
    /// manga-ocr could not be loaded, or failed, and PP-OCRv6 had text.
    MangaUnavailable,
    /// PP-OCRv6 returned nothing, or rejected a line it had detected.
    PaddleIncomplete,
    /// Vertical, and the two engines disagreed about what it said.
    VerticalDisagreement,
    /// Consulted, and PP-OCRv6 was kept anyway.
    PaddleKept,
}

impl AutoReason {
    /// A short label, for a table.
    pub fn label(self) -> &'static str {
        match self {
            AutoReason::ConfidentHorizontal => "confident-horizontal",
            AutoReason::MangaSilent => "manga-silent",
            AutoReason::MangaUnavailable => "manga-unavailable",
            AutoReason::PaddleIncomplete => "paddle-incomplete",
            AutoReason::VerticalDisagreement => "vertical-disagreement",
            AutoReason::PaddleKept => "paddle-kept",
        }
    }

    /// Whether this rule handed the read to manga-ocr.
    pub fn chose_manga(self) -> bool {
        matches!(
            self,
            AutoReason::PaddleIncomplete | AutoReason::VerticalDisagreement
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Paddle,
    Manga,
}

/// Prefer PP-OCRv6 when both engines agree because it preserves detected line breaks. On
/// disagreement, the evidence that caused the second read decides: manga-ocr owns vertical
/// text and PP-OCRv6's own rejected lines own the confidence fallback.
fn pick(paddle: &PaddleRead, manga: &str) -> (Pick, AutoReason) {
    if manga.trim().is_empty() {
        return (Pick::Paddle, AutoReason::MangaSilent);
    }
    if paddle.text.trim().is_empty() || paddle.accepted_lines < paddle.detected_lines {
        return (Pick::Manga, AutoReason::PaddleIncomplete);
    }
    if paddle.direction == ReadingDirection::Vertical && normalize(&paddle.text) != normalize(manga)
    {
        return (Pick::Manga, AutoReason::VerticalDisagreement);
    }
    (Pick::Paddle, AutoReason::PaddleKept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(
        text: &str,
        direction: ReadingDirection,
        detected_lines: usize,
        accepted_lines: usize,
    ) -> PaddleRead {
        PaddleRead {
            text: text.into(),
            direction,
            detected_lines,
            accepted_lines,
        }
    }

    #[test]
    fn a_clean_horizontal_read_never_loads_the_second_engine() {
        let paddle = read("設定を開く", ReadingDirection::Horizontal, 1, 1);
        assert!(!should_check_manga(&paddle));
    }

    #[test]
    fn vertical_or_rejected_output_asks_manga_ocr() {
        let vertical = read("縦書き", ReadingDirection::Vertical, 1, 1);
        let rejected = read("一行", ReadingDirection::Horizontal, 2, 1);
        assert!(should_check_manga(&vertical));
        assert!(should_check_manga(&rejected));
    }

    #[test]
    fn agreement_keeps_paddles_line_layout() {
        let paddle = read("水を飲む\n本を読む", ReadingDirection::Vertical, 2, 2);
        // The reason is asserted too: agreement is not the same rule as manga falling silent,
        // and a change that swapped them would otherwise pass.
        assert_eq!(
            pick(&paddle, "水を飲む本を読む"),
            (Pick::Paddle, AutoReason::PaddleKept)
        );
    }

    #[test]
    fn manga_owns_a_vertical_disagreement() {
        let paddle = read("本水をを読飲むむ", ReadingDirection::Vertical, 2, 2);
        assert_eq!(
            pick(&paddle, "水を飲む本を読む"),
            (Pick::Manga, AutoReason::VerticalDisagreement)
        );
    }

    #[test]
    fn a_rejected_horizontal_line_falls_back_but_blank_manga_does_not_replace_text() {
        let paddle = read("読めた行", ReadingDirection::Horizontal, 2, 1);
        assert_eq!(
            pick(&paddle, "二つとも読めた"),
            (Pick::Manga, AutoReason::PaddleIncomplete)
        );
        assert_eq!(pick(&paddle, ""), (Pick::Paddle, AutoReason::MangaSilent));
    }

    /// Named after the real capture classes rather than generic branches so a future heuristic
    /// change has to show which learner workflow it improves or regresses. Pixel-level accuracy
    /// stays in the opt-in corpus suite; this hermetic matrix pins routing policy.
    #[test]
    fn auto_fixture_matrix_keeps_the_supported_routing_policy() {
        let fixtures = [
            (
                "horizontal dialogue",
                read("次へ進む", ReadingDirection::Horizontal, 1, 1),
                false,
            ),
            (
                "game menu",
                read("設定\n終了", ReadingDirection::Horizontal, 2, 2),
                false,
            ),
            (
                "vertical manga",
                read("縦書き", ReadingDirection::Vertical, 1, 1),
                true,
            ),
            (
                "furigana rejection",
                read("本", ReadingDirection::Horizontal, 2, 1),
                true,
            ),
            (
                "mixed direction",
                read("HP 縦書き", ReadingDirection::Vertical, 2, 2),
                true,
            ),
            (
                "noisy capture",
                read("読めた", ReadingDirection::Horizontal, 4, 1),
                true,
            ),
            (
                "blank Paddle",
                read("", ReadingDirection::Horizontal, 0, 0),
                true,
            ),
        ];
        for (name, evidence, checks_manga) in fixtures {
            assert_eq!(should_check_manga(&evidence), checks_manga, "{name}");
        }
    }
}
