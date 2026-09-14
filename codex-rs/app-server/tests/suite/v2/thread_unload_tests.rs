use super::*;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::TurnStatus;
use core_test_support::stdio_server_bin;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use std::process::Stdio;
use std::time::Duration;
use test_case::test_case;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::oneshot;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::connect_async;
use tokio_util::task::AbortOnDropHandle;

#[test_case(ThreadHistoryMode::Legacy; "legacy")]
#[test_case(ThreadHistoryMode::Paginated; "paginated")]
#[tokio::test]
async fn slow_idle_shutdown_finishes_unloading_before_cold_resume(
    history_mode: ThreadHistoryMode,
) -> Result<()> {
    let server = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let executor_home = TempDir::new()?;
    let mut executor = Command::new(codex_utils_cargo_bin::cargo_bin("exec-server")?)
        .env("CODEX_HOME", executor_home.path())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = executor.stdout.take().expect("executor stdout is piped");
    let mut executor_output = BufReader::new(stdout).lines();
    let executor_url = timeout(DEFAULT_READ_TIMEOUT, executor_output.next_line())
        .await??
        .expect("executor emits its WebSocket URL");
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let proxy_url = format!("ws://{}", listener.local_addr()?);
    let (termination_started, wait_for_termination) = oneshot::channel();
    let (finish_termination, termination_released) = oneshot::channel::<()>();
    let _proxy = AbortOnDropHandle::new(tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let downstream = accept_async(stream).await?;
        let (upstream, _) = connect_async(executor_url).await?;
        let (mut upstream_writer, mut upstream_reader) = upstream.split();
        let (mut downstream_writer, mut downstream_reader) = downstream.split();
        let mut first_termination = Some((termination_started, termination_released));
        let requests = async {
            while let Some(message) = downstream_reader.next().await {
                let message = message?;
                if let Ok(text) = message.to_text()
                    && let Ok(request) = serde_json::from_str::<serde_json::Value>(text)
                    && request["method"] == "process/terminate"
                    && let Some((started, released)) = first_termination.take()
                {
                    started.send(()).expect("test waits for MCP termination");
                    // Hold the real executor request past the App Server's shutdown timeout.
                    // Dropping the test's sender also releases the request after a failed assertion.
                    let _ = released.await;
                }
                upstream_writer.send(message).await?;
            }
            anyhow::Ok(())
        };
        let responses = async {
            while let Some(message) = upstream_reader.next().await {
                downstream_writer.send(message?).await?;
            }
            anyhow::Ok(())
        };
        tokio::try_join!(requests, responses)?;
        anyhow::Ok(())
    }));

    std::fs::write(
        codex_home.path().join("environments.toml"),
        format!(
            "default = \"test-executor\"\ninclude_local = false\n\n[[environments]]\nid = \"test-executor\"\nurl = \"{proxy_url}\"\n"
        ),
    )?;
    let command = toml::Value::String(stdio_server_bin()?);
    let executor_cwd = toml::Value::String(executor_home.path().to_string_lossy().into_owned());
    MockResponsesConfig::new(&server.uri())
        .with_root_config("thread_unload_delay_secs = 0")
        .with_extra_config(&format!(
            "[mcp_servers.shutdown_probe]\ncommand = {command}\ncwd = {executor_cwd}\nenvironment_id = \"test-executor\"\nrequired = true\n"
        ))
        .write(codex_home.path())?;
    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        // This test interposes the executor connection, including on foreign-executor runners.
        .without_auto_env()
        .build_initialized()
        .await?;
    let ThreadStartResponse { thread, .. } = app_server
        .request(|request_id| ClientRequest::ThreadStart {
            request_id,
            params: ThreadStartParams {
                history_mode: Some(history_mode),
                ..Default::default()
            },
        })
        .await?;
    let thread_id = thread.id;
    let completed = timeout(
        DEFAULT_READ_TIMEOUT,
        app_server.start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread_id.clone(),
            input: vec![V2UserInput::Text {
                text: "Preserve this first turn".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        }),
    )
    .await??;
    assert_eq!(
        (completed.turn.status, completed.turn.error),
        (TurnStatus::Completed, None)
    );
    let original: ThreadReadResponse = app_server
        .request(|request_id| ClientRequest::ThreadRead {
            request_id,
            params: ThreadReadParams {
                thread_id: thread_id.clone(),
                include_turns: true,
            },
        })
        .await?;

    let _: ThreadUnsubscribeResponse = app_server
        .request(|request_id| ClientRequest::ThreadUnsubscribe {
            request_id,
            params: ThreadUnsubscribeParams {
                thread_id: thread_id.clone(),
            },
        })
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, wait_for_termination).await??;
    tokio::time::sleep(Duration::from_secs(11)).await;
    let resume_request_id = app_server
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread_id.clone(),
            ..Default::default()
        })
        .await?;
    let closing_resume = timeout(
        DEFAULT_READ_TIMEOUT,
        app_server.read_stream_until_error_message(RequestId::Integer(resume_request_id)),
    )
    .await??;
    assert_eq!(
        closing_resume.error.message,
        format!("thread {thread_id} is closing; retry thread/resume after the thread is closed")
    );
    finish_termination
        .send(())
        .expect("executor is waiting for termination");
    let closed: ThreadClosedNotification = timeout(
        Duration::from_secs(30),
        app_server.read_notification("thread/closed"),
    )
    .await??;
    assert_eq!(
        closed,
        ThreadClosedNotification {
            thread_id: thread_id.clone()
        }
    );
    let loaded: ThreadLoadedListResponse = app_server
        .request(|request_id| ClientRequest::ThreadLoadedList {
            request_id,
            params: ThreadLoadedListParams::default(),
        })
        .await?;
    assert_eq!(
        loaded,
        ThreadLoadedListResponse {
            data: Vec::new(),
            next_cursor: None
        }
    );
    let resumed: ThreadResumeResponse = app_server
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: thread_id.clone(),
                ..Default::default()
            },
        })
        .await?;
    assert_eq!(resumed.thread.turns, original.thread.turns);
    assert_eq!(resumed.thread.status, ThreadStatus::Idle);
    let completed = timeout(
        DEFAULT_READ_TIMEOUT,
        app_server.start_turn_and_wait_for_completion(TurnStartParams {
            thread_id,
            input: vec![V2UserInput::Text {
                text: "Continue after the slow shutdown".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        }),
    )
    .await??;
    assert_eq!(
        (completed.turn.status, completed.turn.error),
        (TurnStatus::Completed, None)
    );
    timeout(Duration::from_secs(30), app_server.shutdown_gracefully()).await??;
    Ok(())
}
