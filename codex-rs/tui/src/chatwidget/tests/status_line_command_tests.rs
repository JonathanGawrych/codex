use super::*;
use codex_config::types::StatusLineCommandConfig;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn status_line_command_uses_client_launch_directory_with_remote_cwd() {
    let local_directory = tempfile::tempdir().expect("local hook directory");
    let remote_directory = local_directory.path().join("remote-only-workspace").abs();
    let mut config = test_config().await;
    config.cwd = remote_directory;
    config.tui_status_line_command = Some(StatusLineCommandConfig {
        command: "pwd".to_string(),
        timeout_ms: 3_000,
        refresh_interval_seconds: 60,
    });
    let resolved_model = get_model_offline_for_tests(config.model.as_deref());
    let session_telemetry = test_session_telemetry(&config, resolved_model.as_str());
    let model_catalog = test_model_catalog(&config);
    let local_settings = crate::local_settings::LocalSettings::from(&config);
    let (app_event_tx, mut app_events) = unbounded_channel::<AppEvent>();
    let (operation_tx, _operations) = unbounded_channel::<Op>();
    let _chat = ChatWidget::new_with_op_target(
        ChatWidgetInit {
            requires_openai_auth: config.model_provider.requires_openai_auth,
            local_settings,
            status_line_command_cwd: local_directory.path().to_path_buf(),
            config,
            frame_requester: FrameRequester::test_dummy(),
            app_event_tx: AppEventSender::new(app_event_tx),
            workspace_command_runner: None,
            initial_user_message: None,
            enhanced_keys_supported: false,
            has_chatgpt_account: false,
            has_codex_backend_auth: false,
            model_catalog,
            feedback: codex_feedback::CodexFeedback::new(),
            is_first_run: true,
            status_account_display: None,
            initial_plan_type: None,
            model: Some(resolved_model),
            startup_tooltip_override: None,
            status_line_invalid_items_warned: Arc::new(AtomicBool::new(false)),
            terminal_title_invalid_items_warned: Arc::new(AtomicBool::new(false)),
            session_telemetry,
        },
        CodexOpTarget::Direct(operation_tx),
    );

    let line = tokio::time::timeout(Duration::from_secs(/*secs*/ 5), async {
        loop {
            if let AppEvent::StatusLineCommandUpdated { result, .. } =
                app_events.recv().await.expect("hook result event")
            {
                break result.expect("hook succeeds").expect("hook output");
            }
        }
    })
    .await
    .expect("hook result timeout")
    .to_string();

    assert_eq!(
        std::fs::canonicalize(line).expect("hook directory exists"),
        local_directory
            .path()
            .canonicalize()
            .expect("local directory exists")
    );
}

#[tokio::test]
async fn status_line_command_keeps_local_directory_after_remote_workspace_update() {
    let (mut chat, mut events, _operations) = make_chatwidget_manual(/*model_override*/ None).await;
    assert_eq!(chat.status_line_command_cwd, chat.config.cwd.to_path_buf());

    let local_directory = tempfile::tempdir().expect("local hook directory");
    chat.status_line_command_cwd = local_directory.path().to_path_buf();
    let remote_directory = local_directory.path().join("remote-only-workspace").abs();
    chat.handle_thread_session(crate::session_state::ThreadSessionState {
        thread_id: ThreadId::new(),
        forked_from_id: None,
        fork_parent_title: None,
        thread_name: None,
        model: "gpt-5.6-sol".to_string(),
        model_provider_id: chat.config.model_provider_id.clone(),
        service_tier: None,
        approval_policy: AskForApproval::Never,
        approvals_reviewer: ApprovalsReviewer::User,
        permission_profile: PermissionProfile::workspace_write(),
        active_permission_profile: None,
        cwd: remote_directory.clone(),
        runtime_workspace_roots: vec![remote_directory.clone()],
        instruction_source_paths: Vec::new(),
        reasoning_effort: None,
        collaboration_mode: None,
        personality: None,
        message_history: None,
        network_proxy: None,
        rollout_path: None,
    });
    chat.local_settings.tui.status_line_command = Some(StatusLineCommandConfig {
        command: "printf '%s|' \"$PWD\"; cat".to_string(),
        timeout_ms: 3_000,
        refresh_interval_seconds: 60,
    });
    chat.status_line_reload_credits = Some(crate::status_line_command::StatusLineReloadCredits {
        available_count: 2,
        credits: Some(vec![crate::status_line_command::StatusLineReloadCredit {
            expires_at: Some(1_801_000_000),
        }]),
    });
    chat.refresh_status_surfaces();

    let line = tokio::time::timeout(Duration::from_secs(/*secs*/ 5), async {
        loop {
            if let AppEvent::StatusLineCommandUpdated { result, .. } =
                events.recv().await.expect("hook result event")
            {
                break result.expect("hook succeeds").expect("hook output");
            }
        }
    })
    .await
    .expect("hook result timeout")
    .to_string();
    let (directory, payload) = line.split_once('|').expect("directory and JSON input");
    let payload: serde_json::Value = serde_json::from_str(payload).expect("hook JSON input");
    assert_eq!(
        (
            std::fs::canonicalize(directory).expect("hook directory exists"),
            payload["cwd"].clone(),
            payload["workspace"]["current_dir"].clone(),
            payload["reloads"].clone(),
        ),
        (
            local_directory
                .path()
                .canonicalize()
                .expect("local directory exists"),
            serde_json::json!(remote_directory),
            serde_json::json!(remote_directory),
            serde_json::json!({"available_count": 2, "credits": [{"expires_at": 1_801_000_000}]}),
        )
    );
}
