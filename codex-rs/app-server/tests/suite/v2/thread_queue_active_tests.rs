use super::*;
use codex_app_server_protocol::ThreadQueueTakeParams;
use codex_app_server_protocol::ThreadQueueTakeResponse;
use core_test_support::responses;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn durable_steer_wakes_agent_wait_and_reaches_same_turn() -> Result<()> {
    let server = create_mock_responses_server_sequence(vec![
        responses::sse(vec![
            responses::ev_response_created("waiting"),
            responses::ev_function_call_with_namespace(
                "waiting-call",
                "collaboration",
                "wait_agent",
                r#"{"timeout_ms":60000}"#,
            ),
            responses::ev_completed("waiting"),
        ]),
        create_final_assistant_message_sse_response("received follow-up")?,
    ])
    .await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(home.path())?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .without_managed_config()
        .with_args(&[
            "-c",
            "features.multi_agent_v2.enabled=true",
            "-c",
            "model=\"gpt-5.4\"",
        ])
        .build_initialized()
        .await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let started: TurnStartResponse = app
        .request(|request_id| ClientRequest::TurnStart {
            request_id,
            params: TurnStartParams {
                thread_id: thread.id.clone(),
                input: vec![text("wait for an agent")],
                ..Default::default()
            },
        })
        .await?;
    timeout(READ_TIMEOUT, async {
        loop {
            let notification: ItemStartedNotification =
                app.read_notification("item/started").await?;
            if matches!(notification.item, ThreadItem::CollabAgentToolCall { .. }) {
                break;
            }
        }
        anyhow::Ok(())
    })
    .await??;

    let queued = queue_item(
        &mut app,
        ThreadQueueAddParams {
            steer: true,
            additional_context: Some(std::collections::HashMap::from([(
                "prompt_timestamp".to_string(),
                codex_app_server_protocol::AdditionalContextEntry {
                    value: "2026-09-10T10:00:00-06:00".to_string(),
                    kind: codex_app_server_protocol::AdditionalContextKind::Application,
                },
            )])),
            ..submission(&thread.id, "stop waiting and consider this now")
        },
    )
    .await?;
    let completed: TurnCompletedNotification =
        timeout(READ_TIMEOUT, app.read_notification("turn/completed")).await??;
    assert_eq!(
        (completed.turn.id, completed.turn.status),
        (started.turn.id, TurnStatus::Completed)
    );
    assert_eq!(list_queue(&mut app, &thread.id).await?.data, vec![]);
    let requests = server
        .received_requests()
        .await
        .context("request capture unavailable")?;
    let bodies = requests
        .iter()
        .filter(|request| request.url.path().ends_with("/responses"))
        .map(wiremock::Request::body_json::<Value>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(bodies.len(), 2);
    let input = bodies[1]["input"].to_string();
    assert!(input.contains("stop waiting and consider this now"));
    assert!(input.contains("Wait interrupted by new input."));
    assert!(input.contains("2026-09-10T10:00:00-06:00"));
    let taken: ThreadQueueTakeResponse = app
        .request(|request_id| ClientRequest::ThreadQueueTake {
            request_id,
            params: ThreadQueueTakeParams {
                thread_id: thread.id,
                queued_submission_id: queued.id,
            },
        })
        .await?;
    assert_eq!(taken.queued_submission, None);
    Ok(())
}

#[tokio::test]
async fn dequeue_returns_latest_input_and_prevents_model_delivery() -> Result<()> {
    let (mut app, _home, server) = queue_app(vec![
        blocked_turn_response()?,
        create_final_assistant_message_sse_response("finished")?,
    ])
    .await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let (_, approval) = start_blocked_turn(&mut app, &thread.id).await?;
    let queued = queue_item(
        &mut app,
        ThreadQueueAddParams {
            steer: true,
            ..submission(&thread.id, "original queued input")
        },
    )
    .await?;
    let updated: ThreadQueueUpdateResponse = app
        .request(|request_id| ClientRequest::ThreadQueueUpdate {
            request_id,
            params: ThreadQueueUpdateParams {
                thread_id: thread.id.clone(),
                queued_submission_id: queued.id.clone(),
                input: vec![text("edited queued input")],
            },
        })
        .await?;
    let taken: ThreadQueueTakeResponse = app
        .request(|request_id| ClientRequest::ThreadQueueTake {
            request_id,
            params: ThreadQueueTakeParams {
                thread_id: thread.id.clone(),
                queued_submission_id: queued.id.clone(),
            },
        })
        .await?;
    assert_eq!(taken.queued_submission, Some(updated.queued_submission));
    let taken_again: ThreadQueueTakeResponse = app
        .request(|request_id| ClientRequest::ThreadQueueTake {
            request_id,
            params: ThreadQueueTakeParams {
                thread_id: thread.id.clone(),
                queued_submission_id: queued.id,
            },
        })
        .await?;
    assert_eq!(taken_again.queued_submission, None);
    decline_approval(&mut app, approval).await?;
    let completed: TurnCompletedNotification =
        timeout(READ_TIMEOUT, app.read_notification("turn/completed")).await??;
    assert_eq!(completed.turn.status, TurnStatus::Completed);
    for request in server
        .received_requests()
        .await
        .context("request capture unavailable")?
    {
        assert!(!String::from_utf8_lossy(&request.body).contains("queued input"));
    }
    Ok(())
}

#[tokio::test]
async fn pending_steer_survives_interrupt_and_server_restart() -> Result<()> {
    let (mut app, home, _server) = queue_app(vec![blocked_turn_response()?]).await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    let (turn_id, _) = start_blocked_turn(&mut app, &thread.id).await?;
    let queued = queue_item(
        &mut app,
        ThreadQueueAddParams {
            steer: true,
            ..submission(&thread.id, "keep this across restart")
        },
    )
    .await?;
    let _: TurnInterruptResponse = app
        .request(|request_id| ClientRequest::TurnInterrupt {
            request_id,
            params: TurnInterruptParams {
                thread_id: thread.id.clone(),
                turn_id,
            },
        })
        .await?;
    let _: TurnCompletedNotification =
        timeout(READ_TIMEOUT, app.read_notification("turn/completed")).await??;
    assert_eq!(
        list_queue(&mut app, &thread.id).await?.data,
        vec![queued.clone()]
    );
    timeout(READ_TIMEOUT, app.shutdown_gracefully()).await??;
    let mut resumed = TestAppServer::builder()
        .with_codex_home(home.path())
        .without_managed_config()
        .build_initialized()
        .await?;
    assert_eq!(
        list_queue(&mut resumed, &thread.id).await?.data,
        vec![queued.clone()]
    );
    let taken: ThreadQueueTakeResponse = resumed
        .request(|request_id| ClientRequest::ThreadQueueTake {
            request_id,
            params: ThreadQueueTakeParams {
                thread_id: thread.id,
                queued_submission_id: queued.id.clone(),
            },
        })
        .await?;
    assert_eq!(taken.queued_submission, Some(queued));
    Ok(())
}
