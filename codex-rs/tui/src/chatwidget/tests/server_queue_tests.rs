use super::*;
use codex_app_server_protocol::QueuedSubmission;
use pretty_assertions::assert_eq;

fn queued_message(id: &str, text: &str) -> QueuedSubmission {
    QueuedSubmission {
        id: id.to_string(),
        client_user_message_id: id.to_string(),
        input: vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }],
    }
}

#[tokio::test]
async fn reconnect_matches_unconfirmed_submissions_by_id_without_copying_them_locally() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    for id in ["first", "second"] {
        chat.input_queue.pending_steers.push_back(PendingSteer {
            client_id: id.to_string(),
            ..pending_steer("identical text")
        });
        chat.begin_server_queue_submission(id.to_string());
    }
    let input = chat.capture_thread_input_state();
    chat.input_queue.clear();
    chat.restore_reconnected_input(input);
    assert!(chat.input_queue.queued_user_messages.is_empty());
    assert!(chat.input_queue.pending_steers.is_empty());
    assert_chatwidget_snapshot!(
        "unconfirmed_server_queue",
        render_bottom_popup(&chat, /*width*/ 80)
    );
    let queued = queued_message("first", "identical text");
    chat.replace_server_queue(thread_id, vec![queued.clone()]);
    assert_eq!(chat.input_queue.server_queue, vec![queued]);
    assert_eq!(
        chat.input_queue
            .pending_server_submissions
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        vec!["second"]
    );
    chat.handle_thread_item(
        ThreadItem::UserMessage {
            id: "committed".to_string(),
            content: queued_message("second", "identical text").input,
            client_id: Some("second".to_string()),
        },
        "turn".to_string(),
        ThreadItemRenderSource::Replay(ReplayKind::ThreadSnapshot),
    );
    assert!(chat.input_queue.pending_server_submissions.is_empty());
    assert!(chat.input_queue.queued_user_messages.is_empty());
}

#[tokio::test]
async fn dequeued_attachments_and_text_survive_resubmission() {
    let (mut chat, _events, mut operations) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    let input = vec![
        UserInput::Image {
            url: "data:image/png;base64,aW1hZ2U=".to_string(),
            detail: None,
        },
        UserInput::Text {
            text: "original\nmultiline prompt".to_string(),
            text_elements: Vec::new(),
        },
    ];
    chat.restore_server_queue_submission(
        thread_id,
        QueuedSubmission {
            id: "image-message".to_string(),
            client_user_message_id: "image-message".to_string(),
            input: input.clone(),
        },
    );
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    let Op::UserTurn { items, .. } = next_submit_op(&mut operations) else {
        unreachable!()
    };
    assert_eq!(items, input);
}

#[tokio::test]
async fn inactive_thread_receipts_remove_only_the_matching_unconfirmed_submission() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    let unchanged = chat.capture_thread_input_state();
    for id in ["first", "second"] {
        chat.input_queue.pending_steers.push_back(PendingSteer {
            client_id: id.to_string(),
            ..pending_steer("identical text")
        });
        chat.begin_server_queue_submission(id.to_string());
    }
    let mut saved = chat.capture_thread_input_state().unwrap();
    saved.acknowledge_committed_input("older");
    assert_eq!(Some(saved.clone()), chat.capture_thread_input_state());
    saved.acknowledge_committed_input("first");
    assert_eq!(saved.pending_server_submissions.len(), 1);
    saved.acknowledge_committed_input("second");
    assert_eq!(Some(saved), unchanged);
}

#[tokio::test]
async fn question_navigation_recalls_from_the_server_without_replacing_the_main_draft() {
    let (mut chat, mut events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    let queued = queued_message("saved", "queued prompt");
    chat.replace_server_queue(thread_id, vec![queued.clone()]);
    chat.bottom_pane
        .set_composer_text("main draft".into(), Vec::new(), Vec::new());
    chat.add_async_questions(
        "question",
        &[codex_protocol::items::AsyncUserInputQuestion {
            title: "What next?".into(),
            options: None,
        }],
    );
    let forward = KeyEvent::new(KeyCode::Up, KeyModifiers::ALT);
    chat.handle_key_event(forward);
    while events.try_recv().is_ok() {}
    chat.handle_key_event(forward);
    assert!(!chat.bottom_pane.questions.as_ref().unwrap().expanded);
    assert_eq!(chat.bottom_pane.composer_text(), "main draft");
    assert!(
        matches!(events.try_recv(), Ok(AppEvent::RecallThreadQueue { thread_id: actual, queued_submission_id }) if actual == thread_id && queued_submission_id == "saved")
    );
    chat.restore_server_queue_submission(thread_id, queued);
    assert_eq!(
        chat.bottom_pane.composer_text(),
        "queued prompt\nmain draft"
    );
}

#[tokio::test]
async fn up_requests_dequeue_before_prepending_to_the_editor() {
    let (mut chat, mut events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    let first = queued_message("first", "first queued message");
    let second = queued_message("second", "second queued message");
    chat.replace_server_queue(thread_id, vec![first.clone(), second.clone()]);
    chat.bottom_pane
        .set_composer_text("draft".to_string(), Vec::new(), Vec::new());
    while events.try_recv().is_ok() {}
    chat.handle_key_event(KeyEvent::from(KeyCode::Up));
    assert_eq!(chat.bottom_pane.composer_text(), "draft");
    assert_eq!(
        chat.input_queue.server_queue,
        vec![first.clone(), second.clone()]
    );
    assert!(
        matches!(events.try_recv(), Ok(AppEvent::RecallThreadQueue { thread_id: actual_thread, queued_submission_id }) if actual_thread == thread_id && queued_submission_id == "second")
    );
    chat.restore_server_queue_submission(thread_id, second);
    assert_eq!(
        chat.bottom_pane.composer_text(),
        "second queued message\ndraft"
    );
    chat.handle_key_event(KeyEvent::from(KeyCode::Up));
    chat.restore_server_queue_submission(thread_id, first);
    assert_eq!(
        chat.bottom_pane.composer_text(),
        "first queued message\nsecond queued message\ndraft"
    );
    assert_eq!(chat.input_queue.server_queue, vec![]);
}

#[tokio::test]
async fn resumed_server_queue_is_visible_and_scoped_to_its_thread() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    let queued = vec![queued_message(
        "persisted",
        "saved while the TUI was closed",
    )];
    chat.replace_server_queue(thread_id, queued.clone());
    chat.replace_server_queue(
        ThreadId::new(),
        vec![queued_message("other", "another thread")],
    );
    assert_eq!(chat.input_queue.server_queue, queued);
    assert_chatwidget_snapshot!(
        "resumed_server_queue",
        render_bottom_popup(&chat, /*width*/ 80)
    );
}

#[tokio::test]
async fn active_follow_up_is_submitted_before_the_turn_finishes() {
    let (mut chat, _events, mut operations) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    handle_turn_started(&mut chat, "active");
    chat.bottom_pane
        .set_composer_text("consider this now".to_string(), Vec::new(), Vec::new());
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    let Op::UserTurn { items, .. } = next_submit_op(&mut operations) else {
        unreachable!()
    };
    assert_eq!(items, queued_message("id", "consider this now").input);
    assert!(chat.input_queue.queued_user_messages.is_empty());
    assert_eq!(chat.input_queue.pending_steers.len(), 1);
    let client_id = chat
        .input_queue
        .pending_steers
        .front()
        .unwrap()
        .client_id
        .clone();
    let queued = queued_message(&client_id, "consider this now");
    chat.begin_server_queue_submission(queued.client_user_message_id.clone());
    chat.acknowledge_server_queue_submission(queued.clone());
    assert!(chat.input_queue.pending_steers.is_empty());
    assert_eq!(chat.input_queue.server_queue, vec![queued]);
}

#[tokio::test]
async fn escape_starts_persisted_follow_up_after_interrupt() {
    let (mut chat, mut events, mut operations) =
        make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    handle_turn_started(&mut chat, "active");
    chat.replace_server_queue(thread_id, vec![queued_message("saved", "next task")]);
    while events.try_recv().is_ok() {}
    chat.handle_key_event(KeyEvent::from(KeyCode::Esc));
    assert!(matches!(operations.try_recv(), Ok(Op::Interrupt)));
    chat.on_interrupted_turn(TurnAbortReason::Interrupted);
    let mut start_requested = false;
    while let Ok(event) = events.try_recv() {
        start_requested |= matches!(event, AppEvent::StartThreadQueue { thread_id: actual } if actual == thread_id);
    }
    assert!(start_requested);
    assert_eq!(chat.bottom_pane.composer_text(), "");
}
