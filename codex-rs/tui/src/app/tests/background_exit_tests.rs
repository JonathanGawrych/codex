use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn external_writer_view_quits_with_escape_ctrl_c_or_q() -> Result<()> {
    let (mut app, mut events, _operations) = make_test_app_with_channels().await;
    app.chat_widget.show_external_writer_thread();
    let mut app_server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    while events.try_recv().is_ok() {}

    app.keymap.app.open_external_editor = vec![crate::key_hint::ctrl(KeyCode::Char('g'))];
    app.handle_key_event(
        &mut tui,
        &mut app_server,
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
    )
    .await;
    assert_eq!(
        app.chat_widget.external_editor_state(),
        ExternalEditorState::Closed
    );
    assert!(
        !events
            .try_recv()
            .is_ok_and(|event| matches!(event, AppEvent::LaunchExternalEditor))
    );

    for key in [
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
    ] {
        app.handle_key_event(&mut tui, &mut app_server, key).await;
        assert!(matches!(
            events.try_recv(),
            Ok(AppEvent::Exit(ExitMode::Immediate))
        ));
    }
    Ok(())
}

#[tokio::test]
async fn external_writer_view_preserves_draft_from_keys_and_paste() -> Result<()> {
    let (mut app, _events, _operations) = make_test_app_with_channels().await;
    let thread_id = ThreadId::new();
    app.enqueue_primary_thread_session(
        test_thread_session(thread_id, test_path_buf("/tmp/project")),
        Vec::new(),
    )
    .await?;
    app.app_server_target = AppServerTarget::Remote {
        endpoint: crate::RemoteAppServerEndpoint::WebSocket {
            websocket_url: "wss://example.com/".to_string(),
            auth_token: None,
        },
    };
    app.ensure_thread_channel(thread_id).mark_external_writer();
    app.chat_widget.insert_str("Retained draft");
    app.chat_widget.show_external_writer_thread();
    let mut app_server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;

    for offline in [false, true] {
        app.reconnect.offline = offline;
        app.handle_tui_event(
            &mut tui,
            &mut app_server,
            TuiEvent::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        )
        .await?;
        app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Paste("pasted".into()))
            .await?;
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "Retained draft"
        );
    }

    app.reconnect.offline = false;
    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
    )
    .await?;
    assert!(app.overlay.is_some());
    Ok(())
}

#[tokio::test]
async fn daemon_disconnect_exit_summary_includes_reconnect_and_stop_instructions() -> Result<()> {
    let (mut app, _, _) = make_test_app_with_channels().await;
    let thread_id = prepare_running_local_daemon(&mut app)?;
    app.keymap.agents.stop = vec![crate::key_hint::plain(KeyCode::F(10))];
    let mut exit_info = app.exit_info(ExitReason::UserRequested);
    exit_info.token_usage = TokenUsage {
        input_tokens: 10,
        output_tokens: 2,
        total_tokens: 12,
        ..Default::default()
    };
    let output = exit_info
        .format_exit_messages(/*color_enabled*/ false)
        .join("\n")
        .replace(&thread_id.to_string(), "THREAD_ID");
    assert_snapshot!("daemon_disconnect_exit", output);
    Ok(())
}

#[tokio::test]
async fn remote_disconnect_exit_summary_does_not_require_a_local_rollout_or_print_credentials() {
    let (mut app, _, _) = make_test_app_with_channels().await;
    app.app_server_target = AppServerTarget::Remote {
        endpoint: crate::RemoteAppServerEndpoint::WebSocket {
            websocket_url: "wss://user:secret@example.com:443/?token=private#secret".to_string(),
            auth_token: Some("secret-token".to_string()),
        },
    };
    let thread_id = ThreadId::from_string("123e4567-e89b-12d3-a456-426614174000").unwrap();
    app.active_thread_id = Some(thread_id);
    app.chat_widget.handle_thread_session(test_thread_session(
        thread_id,
        test_path_buf("/tmp/project"),
    ));
    let exit_info = app.exit_info(ExitReason::Fatal("connection lost".to_string()));
    let lines = exit_info.format_exit_messages(/*color_enabled*/ false);
    let command = shlex::split(lines[1].strip_prefix("Reconnect: ").unwrap()).unwrap();
    assert_eq!(
        crate::resolve_remote_addr(&command[2]).unwrap(),
        crate::RemoteAppServerEndpoint::WebSocket {
            websocket_url: "wss://example.com/".to_string(),
            auth_token: None,
        }
    );
    assert_snapshot!("remote_disconnect_exit", lines.join("\n"));
}

#[tokio::test]
async fn embedded_exit_keeps_the_session_summary() {
    let (mut app, _, _) = make_test_app_with_channels().await;
    let thread_id = prepare_local_daemon_thread(&mut app).unwrap();
    app.app_server_target = AppServerTarget::Embedded;
    let mut exit_info = app.exit_info(ExitReason::UserRequested);
    exit_info.token_usage = TokenUsage {
        output_tokens: 2,
        total_tokens: 2,
        ..Default::default()
    };
    exit_info.resume_hint = Some(ResumableThread {
        thread_id,
        thread_name: None,
    });
    let output = exit_info
        .format_exit_messages(/*color_enabled*/ false)
        .join("\n")
        .replace(&thread_id.to_string(), "THREAD_ID");
    assert_snapshot!(output, @"
    Token usage: total=2 input=0 output=2
    To continue this session, run:
      codex resume THREAD_ID
    ");
}

fn prepare_local_daemon_thread(app: &mut App) -> Result<ThreadId> {
    app.app_server_target = AppServerTarget::LocalDaemon {
        endpoint: crate::RemoteAppServerEndpoint::UnixSocket {
            socket_path: AbsolutePathBuf::relative_to_current_dir("codex.sock")?,
        },
    };
    let thread_id = ThreadId::new();
    app.active_thread_id = Some(thread_id);
    app.chat_widget.handle_thread_session(test_thread_session(
        thread_id,
        test_path_buf("/tmp/project"),
    ));
    Ok(thread_id)
}

fn prepare_running_local_daemon(app: &mut App) -> Result<ThreadId> {
    let thread_id = prepare_local_daemon_thread(app)?;
    app.chat_widget.handle_server_notification(
        turn_started_notification(thread_id, "turn-1"),
        /*replay_kind*/ None,
    );
    Ok(thread_id)
}

async fn assert_running_task_key_interrupts(key_event: KeyEvent) -> Result<()> {
    let (mut app, mut app_event_rx, mut op_rx) = make_test_app_with_channels().await;
    prepare_running_local_daemon(&mut app)?;
    while app_event_rx.try_recv().is_ok() {}
    while op_rx.try_recv().is_ok() {}
    let mut app_server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;

    app.handle_key_event(&mut tui, &mut app_server, key_event)
        .await;

    assert!(app.chat_widget.no_modal_or_popup_active());
    assert_matches!(op_rx.try_recv(), Ok(Op::Interrupt));
    assert!(app_event_rx.try_recv().is_err());
    Ok(())
}

#[tokio::test]
async fn daemon_ctrl_c_interrupts_without_prompting_or_exiting() -> Result<()> {
    assert_running_task_key_interrupts(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .await
}
