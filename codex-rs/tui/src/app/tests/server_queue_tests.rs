use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn replay_confirms_consumed_server_submission_before_restoring_local_input() {
    let (mut app, _events, mut operations) = make_test_app_with_channels().await;
    let thread_id = ThreadId::new();
    let session = test_thread_session(thread_id, app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session.clone());
    app.chat_widget.handle_server_notification(
        turn_started_notification(thread_id, "turn-1"),
        /*replay_kind*/ None,
    );
    app.chat_widget
        .apply_external_edit("accepted before disconnect".to_string());
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let Op::UserTurn {
        items,
        client_user_message_id,
        ..
    } = next_user_turn_op(&mut operations)
    else {
        panic!("expected user input submission");
    };
    app.chat_widget
        .begin_server_queue_submission(client_user_message_id.clone());
    app.chat_widget
        .apply_external_edit("kept draft".to_string());
    let mut input = app
        .chat_widget
        .capture_thread_input_state()
        .expect("input state");
    input.recovered_queue = true;

    app.replay_thread_snapshot(
        ThreadEventSnapshot {
            session: Some(session),
            turns: vec![test_turn(
                "turn-1",
                TurnStatus::Completed,
                vec![ThreadItem::UserMessage {
                    id: "consumed".to_string(),
                    client_id: Some(client_user_message_id),
                    content: items,
                }],
            )],
            events: Vec::new(),
            input_state: Some(input),
        },
        /*resume_restored_queue*/ false,
    );

    assert!(!app.chat_widget.has_queued_follow_up_messages());
    assert_eq!(app.chat_widget.composer_text_with_pending(), "kept draft");
    assert!(app.chat_widget.queued_user_message_texts().is_empty());
}
