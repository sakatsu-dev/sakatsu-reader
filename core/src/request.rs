//! Identity carried by work that may outlive the interface state which requested it.
//!
//! The frontend still checks its local generation when a result returns. This identity gives the
//! native side the earlier decision: obsolete work waiting for an expensive engine need never
//! start. Owners are window/session scoped and operations are independent, so clearing OCR cannot
//! cancel an authorised install or a settings write.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum WorkPriority {
    #[default]
    Foreground,
    Speculative,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct RequestIdentity {
    /// One webview lifetime. A second window may use the same operation names safely.
    pub owner: String,
    /// A stable operation name such as `study` or `capture`.
    pub operation: String,
    /// Monotonic within one owner and operation.
    #[cfg_attr(feature = "typescript", ts(as = "u32"))]
    pub generation: u64,
    #[serde(default)]
    pub priority: WorkPriority,
}

impl RequestIdentity {
    pub fn same_operation(&self, other: &Self) -> bool {
        self.owner == other.owner && self.operation == other.operation
    }
}
