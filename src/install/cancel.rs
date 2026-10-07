//! Stopping an install that is under way.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::{Error, Result};

/// Stops an install: cancel it, from anywhere, and the install it was given to stops before its next part with
/// `cancelled`, storing nothing half downloaded. Clones share the same state. Dropping the install's future stops it
/// too; this is for a caller that wants the install to end on its own, with its error.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// Not cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels it, for good.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether it has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// `cancelled` once it has been cancelled.
    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::new("cancelled"))
        } else {
            Ok(())
        }
    }
}
