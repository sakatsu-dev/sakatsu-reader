//! Warmup must survive the same wrappers as recognition, without submitting an image.
use sakatsu_core::ocr::{
    preprocess::{Preprocess, PreprocessOptions},
    Ocr, OcrError, OcrImage,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct WarmOnly {
    calls: Arc<AtomicUsize>,
    fails: bool,
}
impl Ocr for WarmOnly {
    fn recognize(&self, _: &OcrImage) -> Result<String, OcrError> {
        panic!("warmup ran recognition")
    }
    fn engine(&self) -> &str {
        "warm-test"
    }
    fn warm(&self) -> Result<(), OcrError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fails {
            Err(OcrError::Failed("warm fixture".into()))
        } else {
            Ok(())
        }
    }
}

#[test]
fn warmup_reaches_an_engine_through_arc_and_preparation_without_recognition() {
    let calls = Arc::new(AtomicUsize::new(0));
    let inner: Arc<dyn Ocr> = Arc::new(WarmOnly {
        calls: calls.clone(),
        fails: false,
    });
    let outer: Arc<dyn Ocr> = Arc::new(Preprocess::new(inner, PreprocessOptions::default()));
    outer.warm().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_wrapped_warm_failure_is_preserved() {
    let calls = Arc::new(AtomicUsize::new(0));
    let inner: Arc<dyn Ocr> = Arc::new(WarmOnly {
        calls: calls.clone(),
        fails: true,
    });
    let outer = Preprocess::new(inner, PreprocessOptions::default());
    assert_eq!(outer.warm(), Err(OcrError::Failed("warm fixture".into())));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
