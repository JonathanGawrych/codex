use super::*;
use crate::app::remote_user_input::prepare_remote_user_turn;
use crate::app::tests::session_lifecycle_requests::recorded_params;
use crate::app::tests::session_lifecycle_requests::start_recording_app_server;
use crate::app::tests::session_lifecycle_requests::start_recording_remote_app_server;
use codex_app_server_protocol::UserInput;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use std::path::PathBuf;
use tempfile::tempdir;

const TINY_PNG_BYTES: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 96, 0, 2, 0, 0, 5, 0, 1,
    122, 94, 171, 63, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
const TINY_PNG_DATA_URL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=";

fn user_turn_with_image(path: PathBuf) -> AppCommand {
    AppCommand::user_turn(
        "client-message".to_string(),
        vec![
            UserInput::LocalImage { path, detail: None },
            UserInput::Text {
                text: "inspect this screenshot".to_string(),
                text_elements: Vec::new(),
            },
        ],
        "2026-09-12T08:16:00-06:00".to_string(),
        test_path_buf("/tmp/project"),
        AskForApproval::OnRequest,
        None,
        "gpt-test".to_string(),
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

#[tokio::test]
async fn remote_user_turn_snapshots_local_image_before_submission() -> Result<()> {
    let mut app = make_test_app().await;
    let (mut remote_app_server, requests, proxy) =
        start_recording_remote_app_server(&app.config).await?;
    let started = remote_app_server.start_thread(&app.config).await?;
    let thread_id = started.session.thread_id;
    app.enqueue_primary_thread_session(started.session, started.turns)
        .await?;
    requests.lock().expect("request recorder lock").clear();
    let temp_dir = tempdir()?;
    let image_path = temp_dir.path().join("clipboard.png");
    std::fs::write(&image_path, TINY_PNG_BYTES)?;
    let operation = user_turn_with_image(image_path.clone());

    app.submit_thread_op(&mut remote_app_server, thread_id, operation)
        .await?;
    std::fs::remove_file(&image_path)?;

    assert_eq!(
        recorded_params(&requests, "turn/start")[0]["input"],
        serde_json::json!([
            UserInput::Image {
                url: TINY_PNG_DATA_URL.to_string(),
                detail: None,
            },
            UserInput::Text {
                text: "inspect this screenshot".to_string(),
                text_elements: Vec::new(),
            },
        ])
    );
    remote_app_server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn embedded_user_turn_keeps_local_image_path() -> Result<()> {
    let app = make_test_app().await;
    let (embedded_app_server, _requests, proxy) = start_recording_app_server(
        &app.config,
        /*blocked_thread_list*/ None,
        /*failed_thread_name*/ None,
    )
    .await?;
    let image_path = test_path_buf("/tmp/clipboard.png");
    let mut operation = user_turn_with_image(image_path.clone());

    prepare_remote_user_turn(&embedded_app_server, &mut operation).await?;

    let AppCommand::UserTurn { items, .. } = operation else {
        panic!("expected user turn");
    };
    assert_eq!(
        items.first(),
        Some(&UserInput::LocalImage {
            path: image_path,
            detail: None,
        })
    );
    embedded_app_server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn failed_remote_image_snapshot_keeps_original_user_turn() -> Result<()> {
    let app = make_test_app().await;
    let (remote_app_server, _requests, proxy) =
        start_recording_remote_app_server(&app.config).await?;
    let temp_dir = tempdir()?;
    let image_path = temp_dir.path().join("invalid.png");
    std::fs::write(&image_path, b"not an image")?;
    let mut operation = user_turn_with_image(image_path);
    let original_operation = operation.clone();

    prepare_remote_user_turn(&remote_app_server, &mut operation)
        .await
        .expect_err("invalid image must not be submitted");

    assert_eq!(operation, original_operation);
    remote_app_server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn failed_remote_image_snapshot_surfaces_error_without_exiting_tui() -> Result<()> {
    let (mut app, mut events, mut operations) = make_test_app_with_channels().await;
    let (mut remote_app_server, _requests, proxy) =
        start_recording_remote_app_server(&app.config).await?;
    let started = remote_app_server.start_thread(&app.config).await?;
    app.enqueue_primary_thread_session(started.session, started.turns)
        .await?;
    let temp_dir = tempdir()?;
    let image_path = temp_dir.path().join("invalid.png");
    std::fs::write(&image_path, b"not an image")?;
    app.chat_widget.attach_image(image_path);
    app.chat_widget.insert_str("inspect this screenshot");
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let operation = next_user_turn_op(&mut operations);
    while events.try_recv().is_ok() {}
    let mut tui = crate::tui::test_support::make_test_tui()?;

    let control = app
        .handle_event(
            &mut tui,
            &mut remote_app_server,
            AppEvent::CodexOp(operation),
        )
        .await?;

    assert!(matches!(control, AppRunControl::Continue));
    insta::assert_snapshot!(
        next_history_message(&mut events),
        @"■ Failed to start turn: could not prepare a local attachment for the remote App Server: unsupported image `image/png`: unsupported image `image/png`"
    );
    remote_app_server.shutdown().await?;
    proxy.await??;
    Ok(())
}
