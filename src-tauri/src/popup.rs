use std::sync::atomic::{AtomicU64, Ordering};

/// Cancels stale delayed dismissals when a tray click races a focus-loss event.
///
/// Windows moves focus away from the popup just before it reports some tray
/// clicks. Keeping a generation makes the click authoritative: the pending
/// focus-loss dismissal becomes stale instead of hiding a freshly opened popup.
#[derive(Default)]
pub struct PopupDismissGuard {
    generation: AtomicU64,
}

impl PopupDismissGuard {
    pub fn cancel_pending(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn token(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn is_current(&self, token: u64) -> bool {
        self.token() == token
    }
}

#[cfg(test)]
mod tests {
    use super::PopupDismissGuard;

    #[test]
    fn tray_interaction_invalidates_a_pending_dismissal() {
        let guard = PopupDismissGuard::default();
        let pending = guard.token();

        guard.cancel_pending();

        assert!(!guard.is_current(pending));
    }

    #[test]
    fn unchanged_dismissal_token_remains_current() {
        let guard = PopupDismissGuard::default();
        let pending = guard.token();

        assert!(guard.is_current(pending));
    }
}
