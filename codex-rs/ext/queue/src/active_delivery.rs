//! Durable queue entries offered to a running turn, without consuming them early.

use std::sync::Arc;

use codex_core::DeferredTurnInput;
use codex_core::TurnInputRequest;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;

use crate::QueuedItem;
use crate::QueuedItemService;

struct DeferredQueueEntry {
    service: QueuedItemService,
    thread_id: ThreadId,
    id: String,
}

impl std::fmt::Debug for DeferredQueueEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeferredQueueEntry")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl DeferredTurnInput for DeferredQueueEntry {
    fn id(&self) -> &str {
        &self.id
    }

    fn take(
        &self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = codex_protocol::error::Result<Option<TurnInputRequest>>,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            self.service
                .take(self.thread_id, self.id.clone())
                .await
                .map(|item| item.map(QueuedItem::into_request))
                .map_err(|error| CodexErr::InvalidRequest(error.to_string()))
        })
    }
}

impl QueuedItem {
    pub(crate) fn into_request(self) -> TurnInputRequest {
        let mut request = TurnInputRequest::new(self.input);
        request.additional_context = self.additional_context;
        request
    }
}

impl QueuedItemService {
    pub(crate) async fn offer_active_input(
        &self,
        thread_id: ThreadId,
    ) -> Result<(), crate::QueueServiceError> {
        let Some(manager) = self.thread_manager.upgrade() else {
            return Ok(());
        };
        let Ok(thread) = manager.get_thread(thread_id).await else {
            return Ok(());
        };
        for item in self.list(thread_id).await? {
            if item.steer {
                thread
                    .defer_turn_input(Arc::new(DeferredQueueEntry {
                        service: self.clone(),
                        thread_id,
                        id: item.id,
                    }))
                    .await;
            }
        }
        Ok(())
    }
}
