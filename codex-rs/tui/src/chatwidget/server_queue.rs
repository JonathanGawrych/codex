//! Display and recall of server-owned pending user input.

use super::*;
use crate::bottom_pane::LocalImageAttachment;
use codex_app_server_protocol::QueuedSubmission;

impl ThreadInputState {
    pub(crate) fn acknowledge_committed_input(&mut self, client_id: &str) {
        if self
            .pending_steers
            .front()
            .is_some_and(|pending| pending.client_id == client_id)
        {
            self.pending_steers.pop_front();
        }
        self.pending_server_submissions
            .retain(|(id, _)| id != client_id);
    }
}

impl ChatWidget {
    pub(super) fn recall_queued_input(&mut self) -> bool {
        if !self.input_queue.pending_server_submissions.is_empty()
            && self.input_queue.server_queue.is_empty()
        {
            if let Some(thread_id) = self.thread_id {
                self.app_event_tx
                    .send(AppEvent::RefreshThreadQueue { thread_id });
            }
            self.add_warning_message("Delivery is unconfirmed. Checking the server queue before editing; your message remains visible above the editor.".to_string());
            return true;
        }
        if let Some(composer) = self.pop_latest_queued_composer_state() {
            self.prepend_composer_state(composer);
            self.refresh_pending_input_preview();
            self.request_redraw();
            return true;
        }
        if let Some(submission) = self.input_queue.server_queue.last()
            && let Some(thread_id) = self.thread_id
        {
            if submission
                .input
                .iter()
                .any(|item| matches!(item, UserInput::Audio { .. } | UserInput::LocalAudio { .. }))
            {
                self.add_warning_message(
                    "Queued audio cannot be edited in the TUI; the message remains on the server."
                        .to_string(),
                );
                return true;
            }
            self.app_event_tx.send(AppEvent::RecallThreadQueue {
                thread_id,
                queued_submission_id: submission.id.clone(),
            });
            return true;
        }
        false
    }

    pub(crate) fn begin_server_queue_submission(&mut self, client_id: String) {
        if let Some(index) = self
            .input_queue
            .pending_steers
            .iter()
            .position(|pending| pending.client_id == client_id)
        {
            let Some(pending) = self.input_queue.pending_steers.remove(index) else {
                unreachable!("matched pending input index must exist");
            };
            self.input_queue
                .pending_server_submissions
                .push_back((client_id, pending));
        }
    }

    pub(crate) fn restore_pending_server_submissions(&mut self, input: &mut ThreadInputState) {
        for (client_id, pending) in input.pending_server_submissions.drain(..) {
            self.input_queue
                .pending_server_submissions
                .retain(|(id, _)| id != &client_id);
            self.input_queue
                .pending_server_submissions
                .push_back((client_id, pending));
        }
    }

    pub(super) fn acknowledge_server_input(&mut self, client_id: &str) {
        self.input_queue
            .pending_server_submissions
            .retain(|(id, _)| id != client_id);
        self.input_queue
            .server_queue
            .retain(|item| item.client_user_message_id != client_id);
        self.refresh_pending_input_preview();
    }

    pub(crate) fn acknowledge_server_queue_submission(&mut self, submission: QueuedSubmission) {
        self.acknowledge_server_input(&submission.client_user_message_id);
        self.input_queue.server_queue.push(submission);
        self.refresh_pending_input_preview();
    }

    pub(crate) fn replace_server_queue(
        &mut self,
        thread_id: ThreadId,
        queue: Vec<QueuedSubmission>,
    ) {
        if self.thread_id != Some(thread_id) {
            return;
        }
        self.input_queue
            .pending_server_submissions
            .retain(|(id, _)| {
                !queue
                    .iter()
                    .any(|submission| &submission.client_user_message_id == id)
            });
        self.input_queue.server_queue = queue;
        self.refresh_pending_input_preview();
        self.request_redraw();
    }

    pub(crate) fn restore_server_queue_submission(
        &mut self,
        thread_id: ThreadId,
        submission: QueuedSubmission,
    ) {
        if self.thread_id != Some(thread_id) {
            return;
        }
        self.input_queue
            .server_queue
            .retain(|item| item.id != submission.id);
        let display = Self::user_message_display_from_inputs(&submission.input);
        let mention_bindings =
            user_messages::mention_bindings_from_user_inputs(&submission.input, &display.message);
        let message = UserMessage {
            text: display.message,
            text_elements: display.text_elements,
            remote_image_urls: display.remote_image_urls,
            local_images: display
                .local_images
                .into_iter()
                .enumerate()
                .map(|(index, path)| LocalImageAttachment {
                    placeholder: codex_protocol::models::local_image_label_text(index + 1),
                    path,
                })
                .collect(),
            mention_bindings,
        };
        self.prepend_composer_state(Self::composer_state_from_user_message(message, Vec::new()));
        self.refresh_pending_input_preview();
        self.request_redraw();
    }
}
