//! Release status contains display data only. Installers and signing keys stay in the shell.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    NotConfigured,
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Ready,
    Installing,
    Failed,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct UpdateStatus {
    pub generation: u32,
    pub phase: UpdatePhase,
    pub version: Option<String>,
    pub notes: String,
    pub downloaded: f64,
    pub total: Option<f64>,
    pub error: Option<String>,
}

impl UpdateStatus {
    pub fn new(configured: bool) -> Self {
        Self {
            generation: 0,
            phase: if configured {
                UpdatePhase::Idle
            } else {
                UpdatePhase::NotConfigured
            },
            version: None,
            notes: String::new(),
            downloaded: 0.0,
            total: None,
            error: None,
        }
    }
}

pub fn should_check(now: u64, last_attempt: Option<u64>, enabled: bool) -> bool {
    enabled && last_attempt.is_none_or(|last| now.saturating_sub(last) >= 24 * 60 * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_enable_checks_until_the_reader_opts_out() {
        let old: crate::store::Settings = serde_json::from_str("{}").unwrap();
        assert!(old.check_updates_on_startup);
    }

    #[test]
    fn attempts_throttle_offline_restarts_and_clock_rollback() {
        assert!(should_check(1, None, true));
        assert!(!should_check(1, None, false));
        assert!(!should_check(100, Some(99), true));
        assert!(!should_check(100, Some(200), true));
        assert!(should_check(86401, Some(1), true));
    }

    #[test]
    fn absent_configuration_is_not_reported_as_current() {
        assert_eq!(UpdateStatus::new(false).phase, UpdatePhase::NotConfigured);
    }
}
