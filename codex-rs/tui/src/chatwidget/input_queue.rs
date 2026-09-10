//! Queued user input and pending-steer state for `ChatWidget`.
//!
//! This module keeps the mutable input queues together so `ChatWidget` can
//! apply UI/protocol effects around a focused reducer-style state bag.

use std::collections::VecDeque;

use super::PendingSteer;
use super::QueuedUserMessage;
use super::UserMessage;
use super::UserMessageHistoryRecord;
use super::user_message_preview_text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SessionExitAfterTurn {
    Archive,
    Delete,
}

impl SessionExitAfterTurn {
    pub(super) fn command(self) -> &'static str {
        match self {
            Self::Archive => "/archive",
            Self::Delete => "/delete",
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct PendingInputPreview {
    pub(super) queued_messages: Vec<String>,
    pub(super) pending_steers: Vec<String>,
    pub(super) rejected_steers: Vec<String>,
}

#[derive(Debug, Default)]
pub(super) struct InputQueueState {
    /// Requests whose server acceptance has not yet been confirmed.
    pub(super) pending_server_submissions: VecDeque<(String, PendingSteer)>,
    pub(super) server_queue: Vec<codex_app_server_protocol::QueuedSubmission>,
    /// User inputs queued while a turn is in progress.
    pub(super) queued_user_messages: VecDeque<QueuedUserMessage>,
    /// History records for queued user messages. Slash commands such as `/goal`
    /// can render history that differs from the text submitted to core, so this
    /// stays in lockstep with `queued_user_messages`, with missing entries
    /// treated as user-message text.
    pub(super) queued_user_message_history_records: VecDeque<UserMessageHistoryRecord>,
    /// A user turn has been submitted to core, but `TurnStarted` has not arrived yet.
    pub(super) user_turn_pending_start: bool,
    /// User messages that tried to steer a non-regular turn and must be retried first.
    pub(super) rejected_steers_queue: VecDeque<UserMessage>,
    /// History records for rejected steers. Slash commands such as `/goal` can
    /// render history that differs from the text submitted to core, so this stays
    /// in lockstep with `rejected_steers_queue`, with missing entries treated as
    /// user-message text.
    pub(super) rejected_steer_history_records: VecDeque<UserMessageHistoryRecord>,
    /// Steers already submitted to core but not yet committed into history.
    pub(super) pending_steers: VecDeque<PendingSteer>,
    /// When set, the next interrupt should submit a pending steer or queued
    /// follow-up instead of restoring it into the composer.
    pub(super) submit_follow_up_after_interrupt: bool,
    /// Session action to run after the active live turn completes successfully.
    pub(super) session_exit_after_turn: Option<SessionExitAfterTurn>,
    pub(super) suppress_queue_autosend: bool,
    /// Hold submissions while a usage failure or backend-directed model fallback is resolved.
    pub(super) rate_limit_recovery_pending: bool,
    pub(super) recovered_queue: bool,
}

impl InputQueueState {
    pub(super) fn has_queued_follow_up_messages(&self) -> bool {
        !self.rejected_steers_queue.is_empty() || !self.queued_user_messages.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.pending_server_submissions.clear();
        self.server_queue.clear();
        self.recovered_queue = false;
        self.queued_user_messages.clear();
        self.queued_user_message_history_records.clear();
        self.user_turn_pending_start = false;
        self.rejected_steers_queue.clear();
        self.rejected_steer_history_records.clear();
        self.pending_steers.clear();
        self.submit_follow_up_after_interrupt = false;
        self.session_exit_after_turn = None;
        self.rate_limit_recovery_pending = false;
    }

    pub(super) fn preview(&self) -> PendingInputPreview {
        let mut queued_messages: Vec<String> = self
            .server_queue
            .iter()
            .map(|item| super::ChatWidget::user_message_display_from_inputs(&item.input).message)
            .collect();
        queued_messages.extend(self.pending_server_submissions.iter().map(|(_, pending)| {
            let text =
                user_message_preview_text(&pending.user_message, Some(&pending.history_record));
            format!("Delivery unconfirmed: {text}")
        }));
        queued_messages.extend(self.queued_user_messages.iter().enumerate().map(
            |(idx, message)| {
                user_message_preview_text(
                    message,
                    self.queued_user_message_history_records.get(idx),
                )
            },
        ));
        let pending_steers = self
            .pending_steers
            .iter()
            .map(|steer| {
                user_message_preview_text(&steer.user_message, Some(&steer.history_record))
            })
            .collect();
        let rejected_steers = self
            .rejected_steers_queue
            .iter()
            .enumerate()
            .map(|(idx, message)| {
                user_message_preview_text(message, self.rejected_steer_history_records.get(idx))
            })
            .collect();

        PendingInputPreview {
            queued_messages,
            pending_steers,
            rejected_steers,
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn preview_keeps_queue_categories_separate() {
        let mut state = InputQueueState::default();
        state
            .queued_user_messages
            .push_back(UserMessage::from("queued").into());
        state
            .rejected_steers_queue
            .push_back(UserMessage::from("rejected"));
        state.pending_steers.push_back(PendingSteer {
            client_id: "test-submission".to_string(),
            user_message: UserMessage::from("pending"),
            history_record: UserMessageHistoryRecord::UserMessageText,
            compare_key: crate::chatwidget::user_messages::PendingSteerCompareKey {
                message: "pending".to_string(),
                image_count: 0,
            },
        });

        assert_eq!(
            state.preview(),
            PendingInputPreview {
                queued_messages: vec!["queued".to_string()],
                pending_steers: vec!["pending".to_string()],
                rejected_steers: vec!["rejected".to_string()],
            }
        );
    }

    #[test]
    fn clear_resets_all_input_queues() {
        let mut state = InputQueueState::default();
        state
            .queued_user_messages
            .push_back(UserMessage::from("queued").into());
        state
            .rejected_steers_queue
            .push_back(UserMessage::from("rejected"));
        state.user_turn_pending_start = true;
        state.submit_follow_up_after_interrupt = true;
        state.session_exit_after_turn = Some(SessionExitAfterTurn::Archive);

        state.clear();

        assert!(state.queued_user_messages.is_empty());
        assert!(state.queued_user_message_history_records.is_empty());
        assert!(!state.user_turn_pending_start);
        assert!(state.rejected_steers_queue.is_empty());
        assert!(state.rejected_steer_history_records.is_empty());
        assert!(state.pending_steers.is_empty());
        assert!(!state.submit_follow_up_after_interrupt);
        assert_eq!(state.session_exit_after_turn, None);
    }
}
