//! The one ONNX Runtime environment every in-process model shares.
//!
//! Since ort 2.0.0-rc.14 a session is built *from* an environment instead of from one the crate
//! created implicitly. manga-ocr, PP-OCR and FuguMT each load their own sessions, possibly on
//! different threads, so they ask here: the first caller builds the environment, everyone after
//! shares it. It is built with ort's defaults, which are what rc.13's implicit environment used, so
//! the upgrade changes no runtime setting.
//!
//! A failure to build it is returned, not panicked on, and is not remembered: each loader reports
//! it as its model being unavailable, and the next load tries again.

use std::sync::{Mutex, PoisonError};

use ort::environment::Environment;
use ort::session::builder::SessionBuilder;
use ort::session::Session;

/// The process's environment, built on first use.
pub fn environment() -> ort::Result<Environment> {
    static SHARED: Mutex<Option<Environment>> = Mutex::new(None);
    let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(environment) = shared.as_ref() {
        return Ok(environment.clone());
    }
    let environment = ort::init().build()?;
    *shared = Some(environment.clone());
    Ok(environment)
}

/// The execution providers every in-process session is built with, in the order ONNX Runtime
/// tries them.
///
/// **Empty, so ONNX Runtime uses its default: the processor.** This is the one place a provider
/// would be added, [`session_builder`] applies exactly this list, and [`acceleration`] reports
/// from it — so what the status display says and what the sessions were built with cannot drift
/// apart.
fn execution_providers() -> Vec<ort::ep::ExecutionProviderDispatch> {
    Vec::new()
}

/// A session builder under the shared environment, with [`execution_providers`] applied.
pub fn session_builder() -> ort::Result<SessionBuilder> {
    let builder = Session::builder(&environment()?)?;
    let providers = execution_providers();
    if providers.is_empty() {
        return Ok(builder);
    }
    Ok(builder.with_execution_providers(providers)?)
}

/// What the in-process models compute on, from the providers they are built with.
///
/// With none configured, the processor. With one configured this says only `Unknown`: ONNX
/// Runtime falls back to the processor without telling the caller when a provider cannot start,
/// so a configured provider is not evidence that it is in use.
pub fn acceleration() -> crate::runtime_status::Acceleration {
    if execution_providers().is_empty() {
        crate::runtime_status::Acceleration::Cpu
    } else {
        crate::runtime_status::Acceleration::Unknown
    }
}

#[cfg(test)]
mod tests {
    use ort::AsPointer;

    use super::*;

    #[test]
    fn every_caller_gets_the_same_environment() {
        let first = environment().expect("the environment builds");
        let second = environment().expect("and is shared after");
        // Both handles name one ONNX Runtime environment, not two.
        assert_eq!(first.ptr(), second.ptr());
    }
}
