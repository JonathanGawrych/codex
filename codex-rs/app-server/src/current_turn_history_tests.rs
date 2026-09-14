use super::*;
use codex_protocol::ThreadId;
use codex_protocol::items::ContextCompactionItem;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ItemStartedEvent;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::user_input::UserInput;
use pretty_assertions::assert_eq;

fn start_turn(history: &mut CurrentTurnHistory, turn_id: &str) {
    history.handle_event(&EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: turn_id.to_string(),
        trace_id: None,
        started_at: Some(10),
        model_context_window: None,
        collaboration_mode_kind: Default::default(),
    }));
}

fn complete_item(history: &mut CurrentTurnHistory, turn_id: &str, item: TurnItem) {
    history.handle_event(&EventMsg::ItemCompleted(ItemCompletedEvent {
        thread_id: ThreadId::default(),
        turn_id: turn_id.to_string(),
        item: item.clone(),
        started_at_ms: Some(10_000),
        completed_at_ms: 11_000,
    }));
    for event in item.as_legacy_events(/*show_raw_agent_reasoning*/ false) {
        history.handle_event(&event);
    }
}

#[test]
fn paginated_snapshots_keep_canonical_ids_and_items_across_compaction() {
    let mut history = CurrentTurnHistory::new(ThreadHistoryMode::Paginated);
    start_turn(&mut history, "turn-1");
    let mut user = UserMessageItem {
        id: "user-1".to_string(),
        client_id: Some("client-1".to_string()),
        content: Vec::new(),
    };
    history.handle_event(&EventMsg::ItemStarted(ItemStartedEvent {
        thread_id: ThreadId::default(),
        turn_id: "turn-1".to_string(),
        item: TurnItem::UserMessage(user.clone()),
        started_at_ms: 10_000,
    }));
    user.content.push(UserInput::Text {
        text: "Continue".to_string(),
        text_elements: Vec::new(),
    });
    let user = TurnItem::UserMessage(user);
    complete_item(&mut history, "turn-1", user.clone());
    let compaction = TurnItem::ContextCompaction(ContextCompactionItem {
        id: "compaction-1".to_string(),
    });
    complete_item(&mut history, "turn-1", compaction.clone());
    assert_eq!(
        history.active_turn_snapshot(),
        Some(Turn {
            id: "turn-1".to_string(),
            items: vec![user.into(), compaction.into()],
            items_view: TurnItemsView::Full,
            status: TurnStatus::InProgress,
            error: None,
            started_at: Some(10),
            completed_at: None,
            duration_ms: None,
        })
    );
}

#[test]
fn paginated_snapshots_ignore_late_events_and_clear_finished_turns() {
    let mut history = CurrentTurnHistory::new(ThreadHistoryMode::Paginated);
    start_turn(&mut history, "turn-1");
    start_turn(&mut history, "turn-2");
    let expected = history.active_turn_snapshot();
    complete_item(
        &mut history,
        "turn-1",
        TurnItem::ContextCompaction(ContextCompactionItem {
            id: "late-compaction".to_string(),
        }),
    );
    let mut completion = TurnCompleteEvent {
        turn_id: "turn-1".to_string(),
        last_agent_message: None,
        error: None,
        started_at: Some(10),
        completed_at: Some(11),
        duration_ms: Some(1000),
        time_to_first_token_ms: None,
    };
    history.handle_event(&EventMsg::TurnComplete(completion.clone()));
    assert_eq!(history.active_turn_snapshot(), expected);
    completion.turn_id = "turn-2".to_string();
    history.handle_event(&EventMsg::TurnComplete(completion));
    assert_eq!(history.active_turn_snapshot(), None);
    start_turn(&mut history, "turn-3");
    history.handle_event(&EventMsg::TurnAborted(TurnAbortedEvent {
        turn_id: Some("turn-3".to_string()),
        reason: TurnAbortReason::Interrupted,
        started_at: Some(10),
        completed_at: Some(11),
        duration_ms: Some(1000),
    }));
    assert_eq!(history.active_turn_snapshot(), None);
}
