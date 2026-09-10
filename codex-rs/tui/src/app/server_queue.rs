//! Persistent pending user messages shared by all clients of a thread.

use super::*;
use codex_app_server_protocol::ThreadQueueAddParams;
use codex_app_server_protocol::ThreadQueueAddResponse;
use codex_app_server_protocol::ThreadQueueListParams;
use codex_app_server_protocol::ThreadQueueListResponse;
use codex_app_server_protocol::ThreadQueueStartParams;
use codex_app_server_protocol::ThreadQueueStartResponse;
use codex_app_server_protocol::ThreadQueueTakeParams;
use codex_app_server_protocol::ThreadQueueTakeResponse;
use codex_app_server_protocol::UserInput;

impl App {
    pub(super) async fn start_thread_queue(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        let request_id = app_server.next_request_id();
        let response: std::result::Result<ThreadQueueStartResponse, _> = app_server
            .request_handle()
            .request_typed(ClientRequest::ThreadQueueStart {
                request_id,
                params: ThreadQueueStartParams {
                    thread_id: thread_id.to_string(),
                    queued_submission_id: None,
                },
            })
            .await;
        if let Err(error) = response {
            self.chat_widget
                .add_error_message(format!("Failed to start queued message: {error}"));
        }
        self.refresh_thread_queue(app_server, thread_id).await;
    }
    pub(super) async fn queue_active_user_input(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        client_user_message_id: String,
        items: Vec<UserInput>,
        prompt_submitted_at: String,
    ) -> Result<bool> {
        let request_id = app_server.next_request_id();
        if self.chat_widget.thread_id() == Some(thread_id) {
            self.chat_widget
                .begin_server_queue_submission(client_user_message_id.clone());
        }
        let response: ThreadQueueAddResponse = app_server
            .request_handle()
            .request_typed(ClientRequest::ThreadQueueAdd {
                request_id,
                params: ThreadQueueAddParams {
                    thread_id: thread_id.to_string(),
                    input: items,
                    client_user_message_id,
                    steer: true,
                    additional_context: Some(
                        crate::prompt_timestamp::prompt_timestamp_additional_context(
                            &prompt_submitted_at,
                        ),
                    ),
                },
            })
            .await?;
        if self.chat_widget.thread_id() == Some(thread_id) {
            self.chat_widget
                .acknowledge_server_queue_submission(response.queued_submission);
        }
        self.refresh_thread_queue(app_server, thread_id).await;
        Ok(true)
    }

    pub(super) async fn refresh_thread_queue(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        let mut queue = Vec::new();
        let mut cursor = None;
        loop {
            let request_id = app_server.next_request_id();
            let response: std::result::Result<ThreadQueueListResponse, _> = app_server
                .request_handle()
                .request_typed(ClientRequest::ThreadQueueList {
                    request_id,
                    params: ThreadQueueListParams {
                        thread_id: thread_id.to_string(),
                        cursor,
                        limit: Some(100),
                    },
                })
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    self.chat_widget
                        .add_error_message(format!("Failed to load queued messages: {error}"));
                    return;
                }
            };
            queue.extend(response.data);
            cursor = response.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        self.chat_widget.replace_server_queue(thread_id, queue);
    }

    pub(super) async fn recall_thread_queue(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        queued_submission_id: String,
    ) {
        let request_id = app_server.next_request_id();
        let response: std::result::Result<ThreadQueueTakeResponse, _> = app_server
            .request_handle()
            .request_typed(ClientRequest::ThreadQueueTake {
                request_id,
                params: ThreadQueueTakeParams {
                    thread_id: thread_id.to_string(),
                    queued_submission_id,
                },
            })
            .await;
        match response {
            Ok(response) => {
                if let Some(submission) = response.queued_submission {
                    self.chat_widget
                        .restore_server_queue_submission(thread_id, submission);
                } else {
                    self.chat_widget.add_warning_message("That message is no longer queued; it was consumed or removed by another client.".to_string());
                }
                self.refresh_thread_queue(app_server, thread_id).await;
            }
            Err(error) => self.chat_widget.add_error_message(format!(
                "Could not confirm dequeue; the editor was left unchanged: {error}"
            )),
        }
    }
}
