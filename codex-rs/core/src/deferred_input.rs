//! Admission of durable user input at the next model request.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use codex_protocol::error::Result;
use codex_protocol::turn_input::TurnInputRequest;

use crate::CodexThread;
use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::turn_input::merge_additional_context_input;
use crate::session::turn_input::pending_turn_input;
use crate::state::TaskKind;

/// A persisted input whose owner arbitrates consumption against deletion.
/// Implementations return `None` when another client has already removed it.
/// Only `input` and `additional_context` are consumed. Requests must retain the
/// default thread settings and turn-start options because the turn is already running.
pub trait DeferredTurnInput: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &str;
    fn take(&self) -> Pin<Box<dyn Future<Output = Result<Option<TurnInputRequest>>> + Send + '_>>;
}

impl CodexThread {
    /// Wake a regular turn without consuming the persisted message yet.
    #[expect(
        clippy::await_holding_invalid_type,
        reason = "active turn checks and turn state updates must remain atomic"
    )]
    pub async fn defer_turn_input(&self, input: Arc<dyn DeferredTurnInput>) {
        let active = self.session.active_turn.lock().await;
        let Some(active) = active.as_ref() else {
            return;
        };
        if !active
            .task
            .as_ref()
            .is_some_and(|task| task.kind == TaskKind::Regular)
        {
            return;
        }
        let mut state = active.turn_state.lock().await;
        if state
            .pending_input
            .deferred
            .iter()
            .any(|pending| pending.id() == input.id())
        {
            return;
        }
        state.pending_input.deferred.push(input);
        state.accept_mailbox_delivery_for_current_turn();
        self.session.input_queue.notify_steer();
    }
}

impl Session {
    #[expect(
        clippy::await_holding_invalid_type,
        reason = "active turn checks and turn state updates must remain atomic"
    )]
    pub(crate) async fn take_deferred_input(&self) -> Result<Vec<TurnInput>> {
        let pending = {
            let active = self.active_turn.lock().await;
            let Some(active) = active.as_ref() else {
                return Ok(Vec::new());
            };
            std::mem::take(&mut active.turn_state.lock().await.pending_input.deferred)
        };
        let mut inputs = Vec::new();
        for pending in pending {
            if let Some(request) = pending.take().await? {
                inputs
                    .extend(merge_additional_context_input(self, request.additional_context).await);
                inputs.push(pending_turn_input(self, request.input).await);
            }
        }
        Ok(inputs)
    }
}
