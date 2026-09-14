//! Running-turn snapshots for the two persisted history formats.

use codex_app_server_protocol::ThreadHistoryBuilder;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadHistoryMode;
use std::collections::HashMap;

pub(crate) enum CurrentTurnHistory {
    Legacy(ThreadHistoryBuilder),
    Paginated(PaginatedTurnHistory),
}

impl Default for CurrentTurnHistory {
    fn default() -> Self {
        Self::new(ThreadHistoryMode::Legacy)
    }
}

impl CurrentTurnHistory {
    pub(crate) fn new(history_mode: ThreadHistoryMode) -> Self {
        match history_mode {
            ThreadHistoryMode::Legacy => Self::Legacy(ThreadHistoryBuilder::new()),
            ThreadHistoryMode::Paginated => Self::Paginated(PaginatedTurnHistory::default()),
        }
    }

    pub(crate) fn reset(&mut self) {
        match self {
            Self::Legacy(history) => history.reset(),
            Self::Paginated(history) => *history = PaginatedTurnHistory::default(),
        }
    }

    pub(crate) fn active_turn_snapshot(&self) -> Option<Turn> {
        match self {
            Self::Legacy(history) => history.active_turn_snapshot(),
            Self::Paginated(history) => history.turn.clone(),
        }
    }

    pub(crate) fn has_active_turn(&self) -> bool {
        match self {
            Self::Legacy(history) => history.has_active_turn(),
            Self::Paginated(history) => history.turn.is_some(),
        }
    }

    pub(crate) fn handle_event(&mut self, event: &EventMsg) {
        match self {
            Self::Legacy(history) => history.handle_event(event),
            Self::Paginated(history) => history.handle_event(event),
        }
    }
}

#[derive(Default)]
pub(crate) struct PaginatedTurnHistory {
    turn: Option<Turn>,
    item_indexes: HashMap<String, usize>,
}

impl PaginatedTurnHistory {
    fn handle_event(&mut self, event: &EventMsg) {
        match event {
            EventMsg::TurnStarted(event) => {
                self.turn = Some(Turn {
                    id: event.turn_id.clone(),
                    items: Vec::new(),
                    items_view: TurnItemsView::Full,
                    status: TurnStatus::InProgress,
                    error: None,
                    started_at: event.started_at,
                    completed_at: None,
                    duration_ms: None,
                });
                self.item_indexes.clear();
            }
            EventMsg::ItemStarted(event) => {
                self.update_item(&event.turn_id, &event.item);
            }
            EventMsg::ItemCompleted(event) => {
                self.update_item(&event.turn_id, &event.item);
            }
            EventMsg::TurnComplete(event)
                if self
                    .turn
                    .as_ref()
                    .is_some_and(|turn| turn.id == event.turn_id) =>
            {
                *self = Self::default();
            }
            EventMsg::TurnAborted(event)
                if self.turn.as_ref().is_some_and(|turn| {
                    event
                        .turn_id
                        .as_ref()
                        .is_none_or(|turn_id| turn.id == *turn_id)
                }) =>
            {
                *self = Self::default();
            }
            // Core also sends legacy aliases, which lack canonical message IDs. Paginated
            // snapshots use the same item lifecycles as v2 notifications and persisted history.
            _ => {}
        }
    }

    fn update_item(&mut self, turn_id: &str, item: &codex_protocol::items::TurnItem) {
        let Some(turn) = self.turn.as_mut().filter(|turn| turn.id == turn_id) else {
            // A background command can finish after its turn. Its completed snapshot is
            // already persisted in that turn; it does not belong to the running turn.
            return;
        };
        let item = ThreadItem::from(item.clone());
        if let Some(index) = self.item_indexes.get(item.id()) {
            turn.items[*index] = item;
        } else {
            self.item_indexes
                .insert(item.id().to_owned(), turn.items.len());
            turn.items.push(item);
        }
    }
}

#[cfg(test)]
#[path = "current_turn_history_tests.rs"]
mod tests;
