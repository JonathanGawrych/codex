//! Source-checkout update lifecycle for `ChatWidget`.

use super::*;

impl ChatWidget {
    pub(super) fn maybe_restart_after_source_update(&mut self) -> bool {
        let Some(checkout_root) = crate::update_action::source_checkout_root() else {
            return false;
        };
        self.maybe_restart_after_source_update_in(checkout_root)
    }

    pub(super) fn maybe_restart_after_source_update_in(&mut self, checkout_root: &Path) -> bool {
        let Some(thread_id) = self.thread_id else {
            return false;
        };
        match crate::source_update::take_source_update_completion(checkout_root, thread_id) {
            Ok(false) => return false,
            Ok(true) => {}
            Err(error) => {
                self.add_error_message(error);
                return false;
            }
        }
        self.app_event_tx.send(AppEvent::RestartAfterUpdate);
        true
    }
}
