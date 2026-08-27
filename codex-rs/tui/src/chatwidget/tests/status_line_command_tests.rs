use super::*;
use codex_config::types::StatusLineCommandConfig;
use pretty_assertions::assert_eq;

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
        ),
        (
            local_directory
                .path()
                .canonicalize()
                .expect("local directory exists"),
            serde_json::json!(remote_directory),
            serde_json::json!(remote_directory),
        )
    );
}
