//! Question navigation reserves plain Up for option selection and answer history.

use super::*;

impl ChatKeymap {
    pub(crate) fn advances_question(&self, key: KeyEvent) -> bool {
        !key_hint::plain(KeyCode::Up).is_press(key) && self.edit_queued_message.is_pressed(key)
    }
}

impl RuntimeKeymap {
    pub(crate) fn question_navigation_hint(&self) -> Option<ShortcutHint> {
        let hint = self.primary_hint(KeymapContext::Chat, "edit_queued_message");
        if hint == Some(key_hint::plain(KeyCode::Up).into()) {
            self.chat
                .edit_queued_message
                .iter()
                .copied()
                .find(|binding| *binding != key_hint::plain(KeyCode::Up))
                .map(Into::into)
        } else {
            hint
        }
    }
}
