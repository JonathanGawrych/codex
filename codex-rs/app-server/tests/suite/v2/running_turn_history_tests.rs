use super::mock_responses_config;
use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItemsListParams;
use codex_app_server_protocol::ThreadItemsListResponse;
use codex_app_server_protocol::ThreadResumeInitialTurnsPageParams;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio::time::timeout;

use super::DEFAULT_READ_TIMEOUT;

#[tokio::test]
async fn running_paginated_resume_preserves_live_and_saved_messages() -> Result<()> {
    assert_running_paginated_resume_preserves_messages("codex-app-server-tests").await
}

#[tokio::test]
async fn android_running_paginated_resume_preserves_live_and_saved_messages() -> Result<()> {
    assert_running_paginated_resume_preserves_messages("codex_chatgpt_android_remote").await
}

async fn assert_running_paginated_resume_preserves_messages(client_name: &str) -> Result<()> {
    let mut first_commentary = responses::ev_assistant_message("msg-first", "First update");
    first_commentary["item"]["phase"] = json!("commentary");
    let mut second_commentary = responses::ev_assistant_message("msg-second", "Second update");
    second_commentary["item"]["phase"] = json!("commentary");
    let mut final_answer = responses::ev_assistant_message("msg-final", "Finished");
    final_answer["item"]["phase"] = json!("final_answer");
    let (release_turn, turn_gate) = oneshot::channel();
    let (server, _response_completions) = start_streaming_sse_server(vec![vec![
        StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![
                responses::ev_response_created("resp-1"),
                first_commentary,
                second_commentary,
            ]),
        },
        StreamingSseChunk {
            gate: Some(turn_gate),
            body: responses::sse(vec![final_answer, responses::ev_completed("resp-1")]),
        },
    ]])
    .await;
    let codex_home = TempDir::new()?;
    mock_responses_config(server.uri()).write(codex_home.path())?;
    let mut client = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    let initialized = client
        .initialize_with_client_info(ClientInfo {
            name: client_name.to_string(),
            title: None,
            version: "dev".to_string(),
        })
        .await?;
    assert!(matches!(initialized, JSONRPCMessage::Response(_)));
    let start_id = client
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("gpt-5.4".to_string()),
            history_mode: Some(ThreadHistoryMode::Paginated),
            ..Default::default()
        })
        .await?;
    let ThreadStartResponse { thread, .. } =
        timeout(DEFAULT_READ_TIMEOUT, client.read_response(start_id)).await??;
    let start_turn_id = client
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            input: vec![UserInput::Text {
                text: "Continue the long task".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let TurnStartResponse { turn } =
        timeout(DEFAULT_READ_TIMEOUT, client.read_response(start_turn_id)).await??;
    let mut live_items = Vec::new();
    loop {
        let notification: ItemCompletedNotification = timeout(
            DEFAULT_READ_TIMEOUT,
            client.read_notification("item/completed"),
        )
        .await??;
        let last_commentary = notification.item.id() == "msg-second";
        live_items.push(notification.item);
        if last_commentary {
            break;
        }
    }

    // Crossing item-page boundaries must preserve the same IDs and text as live delivery.
    let mut saved_items = Vec::new();
    let mut cursor = None;
    loop {
        let list_id = client
            .send_thread_items_list_request(ThreadItemsListParams {
                thread_id: thread.id.clone(),
                turn_id: Some(turn.id.clone()),
                cursor,
                limit: Some(1),
                sort_direction: Some(SortDirection::Asc),
            })
            .await?;
        let page: ThreadItemsListResponse =
            timeout(DEFAULT_READ_TIMEOUT, client.read_response(list_id)).await??;
        saved_items.extend(page.data.into_iter().map(|entry| entry.item));
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(saved_items, live_items);

    for items_view in [TurnItemsView::Full, TurnItemsView::Summary] {
        let list_id = client
            .send_thread_turns_list_request(ThreadTurnsListParams {
                thread_id: thread.id.clone(),
                cursor: None,
                limit: Some(1),
                sort_direction: Some(SortDirection::Desc),
                items_view: Some(items_view),
            })
            .await?;
        let listed: ThreadTurnsListResponse =
            timeout(DEFAULT_READ_TIMEOUT, client.read_response(list_id)).await??;
        let resume_id = client
            .send_thread_resume_request(ThreadResumeParams {
                thread_id: thread.id.clone(),
                exclude_turns: true,
                initial_turns_page: Some(ThreadResumeInitialTurnsPageParams {
                    limit: Some(1),
                    sort_direction: Some(SortDirection::Desc),
                    items_view: Some(items_view),
                }),
                ..Default::default()
            })
            .await?;
        let resumed: ThreadResumeResponse =
            timeout(DEFAULT_READ_TIMEOUT, client.read_response(resume_id)).await??;
        assert_eq!(
            resumed
                .initial_turns_page
                .expect("resume includes the requested turn page")
                .data,
            listed.data
        );
    }
    let resume_id = client
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.id.clone(),
            ..Default::default()
        })
        .await?;
    let resumed: ThreadResumeResponse =
        timeout(DEFAULT_READ_TIMEOUT, client.read_response(resume_id)).await??;
    assert_eq!(resumed.thread.turns[0].items, live_items);

    release_turn.send(()).expect("release the running response");
    let final_notification: ItemCompletedNotification = timeout(
        DEFAULT_READ_TIMEOUT,
        client.read_notification("item/completed"),
    )
    .await??;
    live_items.push(final_notification.item);
    timeout(
        DEFAULT_READ_TIMEOUT,
        client.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let resume_id = client
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.id,
            ..Default::default()
        })
        .await?;
    let resumed: ThreadResumeResponse =
        timeout(DEFAULT_READ_TIMEOUT, client.read_response(resume_id)).await??;
    assert_eq!(resumed.thread.turns[0].items, live_items);
    server.shutdown().await;
    Ok(())
}
