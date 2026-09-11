use super::*;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::Mutex;
use wiremock::Mock;
use wiremock::matchers::path;

#[derive(Clone, Copy)]
enum ImportedCompactionProtocol {
    CompactEndpoint,
    ResponsesV2,
}

async fn create_imported_thread(
    protocol: ImportedCompactionProtocol,
) -> Result<(
    TestCodexHarness,
    Arc<codex_core::CodexThread>,
    Vec<ResponseItem>,
)> {
    let harness = TestCodexHarness::with_auto_env_builder(
        test_codex()
            .with_auth(CodexAuth::create_dummy_chatgpt_auth_for_testing())
            .with_history_mode(ThreadHistoryMode::Paginated)
            .with_config(move |config| {
                config.model = Some("gpt-6-astra".to_string());
                config.model_reasoning_effort = Some(ReasoningEffort::XHigh);
                config.model_context_window = Some(50_000);
                config.model_auto_compact_token_limit = Some(30_000);
                if matches!(protocol, ImportedCompactionProtocol::ResponsesV2) {
                    config
                        .features
                        .enable(Feature::RemoteCompactionV2)
                        .expect("remote compaction v2 should be configurable");
                }
            }),
    )
    .await?;
    let mut original: Vec<ResponseItem> = Vec::new();
    for index in 0..80 {
        let (role, text) = if index % 10 == 0 {
            ("user", format!("original user {index}"))
        } else {
            (
                "assistant",
                format!("imported tool result {index}: {}", "x".repeat(10_000)),
            )
        };
        original.push(serde_json::from_value(json!({
            "type": "message", "role": role,
            "content": [{"type": if role == "user" {"input_text"} else {"output_text"}, "text": text}],
        }))?);
    }
    original.push(serde_json::from_value(json!({
        "type": "message", "role": "user",
        "content": [{"type": "input_text", "text": "saved phone input after migration"}],
    }))?);
    let thread = harness
        .test()
        .thread_manager
        .start_thread(StartThreadOptions {
            initial_history: InitialHistory::Forked(
                original
                    .iter()
                    .cloned()
                    .map(|item| RolloutItem::ResponseItem(item.into()))
                    .collect(),
            ),
            environments: Some(vec![
                harness.test().executor_environment().selection().clone(),
            ]),
            ..StartThreadOptions::new(harness.test().config.clone())
        })
        .await?
        .thread;
    Ok((harness, thread, original))
}

#[derive(Clone)]
struct CompactResponder {
    requests: Arc<Mutex<Vec<Value>>>,
    fail_at: Option<usize>,
    max_assistant_items: Option<usize>,
}

impl wiremock::Respond for CompactResponder {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let bytes = if request
            .headers
            .get("content-encoding")
            .is_some_and(|value| value == "zstd")
        {
            match zstd::stream::decode_all(request.body.as_slice()) {
                Ok(bytes) => bytes,
                Err(error) => panic!("decode compact request: {error}"),
            }
        } else {
            request.body.clone()
        };
        let body: Value = match serde_json::from_slice(&bytes) {
            Ok(body) => body,
            Err(error) => panic!("parse compact request: {error}"),
        };
        let mut requests = self
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        requests.push(body.clone());
        if self.fail_at == Some(requests.len()) {
            return ResponseTemplate::new(400).set_body_json(json!({"error": {
                "type": "invalid_request_error", "code": "fixture_failure", "message": "fixture failure"
            }}));
        }
        let input = match body["input"].as_array() {
            Some(input) => input,
            None => panic!("compact request input is not an array"),
        };
        let assistant_items = input
            .iter()
            .filter(|item| item["type"] == "message" && item["role"] == "assistant")
            .count();
        if self
            .max_assistant_items
            .is_some_and(|maximum| assistant_items > maximum)
        {
            return ResponseTemplate::new(400).set_body_json(json!({"error": {
                "type": "invalid_request_error", "code": "context_length_exceeded", "message": "fixture server-side context overflow"
            }}));
        }
        let mut output: Vec<Value> = input
            .iter()
            .filter(|item| {
                item["type"] == "message" && (item["role"] == "user" || item["role"] == "developer")
            })
            .cloned()
            .collect();
        output.push(json!({"type": "compaction", "encrypted_content": format!("summary-{}", requests.len())}));
        ResponseTemplate::new(200).set_body_json(json!({"output": output}))
    }
}

#[tokio::test]
async fn oversized_import_completes_compaction_and_turn_without_rewriting_history() -> Result<()> {
    let (harness, thread, original) =
        create_imported_thread(ImportedCompactionProtocol::CompactEndpoint).await?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    Mock::given(path("/v1/responses/compact"))
        .respond_with(CompactResponder {
            requests: requests.clone(),
            fail_at: None,
            max_assistant_items: None,
        })
        .mount(harness.server())
        .await;
    let normal = responses::mount_sse_sequence(
        harness.server(),
        vec![
            responses::sse_failed(
                "overflow",
                "context_length_exceeded",
                "fixture context overflow",
            ),
            sse(vec![
                responses::ev_assistant_message("reply", "completed after bounded compaction"),
                responses::ev_completed("response"),
            ]),
        ],
    )
    .await;
    thread
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "phone input that exceeds context".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_event(&thread, |event| match event {
        EventMsg::Error(_) => true,
        EventMsg::TurnComplete(_) => panic!("expected an initial context-window error"),
        _ => false,
    })
    .await;
    wait_for_turn_complete(&thread).await;
    thread
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "phone retry".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_event(&thread, |event| match event {
        EventMsg::ItemCompleted(item) if matches!(item.item, TurnItem::ContextCompaction(_)) => {
            true
        }
        EventMsg::Error(error) => panic!("compaction failed: {}", error.message),
        EventMsg::TurnComplete(_) => panic!(
            "turn completed without compaction; compact calls: {}",
            requests.lock().unwrap().len()
        ),
        _ => false,
    })
    .await;
    wait_for_turn_complete(&thread).await;
    let requests = requests.lock().unwrap().clone();
    assert!(requests.len() > 2);
    let mut sent_assistant_messages = Vec::new();
    for (index, request) in requests.iter().enumerate() {
        assert_eq!(request["model"], json!("gpt-6-astra"));
        assert_eq!(request["reasoning"]["effort"], json!("xhigh"));
        assert!(request.to_string().len() / 4 < 50_000);
        let ids: Vec<_> = request["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["id"].as_str())
            .collect();
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            ids.len()
        );
        if index > 0 {
            assert!(
                request["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["encrypted_content"] == format!("summary-{index}"))
            );
        }
        sent_assistant_messages.extend(
            request["input"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["role"] == "assistant")
                .cloned(),
        );
    }
    let expected: Vec<_> = original
        .iter()
        .filter(|item| matches!(item, ResponseItem::Message { role, .. } if role == "assistant"))
        .map(|item| serde_json::to_value(item).unwrap())
        .collect();
    assert_eq!(
        responses::strip_response_item_ids_from_json(responses::strip_metadata_from_json(json!(
            sent_assistant_messages
        ))),
        responses::strip_response_item_ids_from_json(responses::strip_metadata_from_json(json!(
            expected
        )))
    );
    let normal_requests = normal.requests();
    assert_eq!(normal_requests.len(), 2);
    let followup = &normal_requests[1];
    assert!(followup.body_contains_text("saved phone input after migration"));
    assert!(followup.body_contains_text("phone input that exceeds context"));
    assert!(followup.body_contains_text("phone retry"));
    thread.submit(Op::Shutdown).await?;
    wait_for_event(&thread, |event| matches!(event, EventMsg::ShutdownComplete)).await;
    let stored: Vec<_> = fs::read_to_string(thread.rollout_path().unwrap())?
        .lines()
        .map(codex_rollout::parse_rollout_line)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let original_stored: Vec<_> = stored
        .iter()
        .filter_map(|record| match &record.item {
            RolloutItem::ResponseItem(item) => Some(responses::strip_metadata(item.item.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        responses::strip_response_item_ids(&original_stored[..original.len()]),
        original
    );
    assert_eq!(
        stored
            .iter()
            .filter(|record| matches!(record.item, RolloutItem::Compacted(_)))
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn interrupted_compaction_does_not_install_an_intermediate_checkpoint() -> Result<()> {
    let (harness, thread, original) =
        create_imported_thread(ImportedCompactionProtocol::CompactEndpoint).await?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    Mock::given(path("/v1/responses/compact"))
        .respond_with(CompactResponder {
            requests: requests.clone(),
            fail_at: None,
            max_assistant_items: None,
        })
        .mount(harness.server())
        .await;
    let second_request = Arc::new(tokio::sync::Notify::new());
    let started = second_request.clone();
    Mock::given(path("/v1/responses/compact"))
        .and(move |_: &wiremock::Request| requests.lock().unwrap().len() == 1)
        .respond_with(move |_: &wiremock::Request| {
            started.notify_one();
            ResponseTemplate::new(200)
                .set_body_json(json!({"output": compacted_summary_only_output("too late")}))
                .set_delay(Duration::from_secs(30))
        })
        .with_priority(1)
        .mount(harness.server())
        .await;
    thread.submit(Op::Compact).await?;
    tokio::time::timeout(Duration::from_secs(10), second_request.notified()).await?;
    thread.submit(Op::Interrupt).await?;
    wait_for_event(&thread, |event| matches!(event, EventMsg::TurnAborted(_))).await;
    thread.submit(Op::Shutdown).await?;
    wait_for_event(&thread, |event| matches!(event, EventMsg::ShutdownComplete)).await;
    let stored: Vec<_> = fs::read_to_string(thread.rollout_path().unwrap())?
        .lines()
        .map(codex_rollout::parse_rollout_line)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert!(
        stored
            .iter()
            .all(|record| !matches!(record.item, RolloutItem::Compacted(_)))
    );
    let original_stored: Vec<_> = stored
        .iter()
        .filter_map(|record| match &record.item {
            RolloutItem::ResponseItem(item) => Some(responses::strip_metadata(item.item.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        responses::strip_response_item_ids(&original_stored[..original.len()]),
        original
    );
    Ok(())
}

#[tokio::test]
async fn failed_intermediate_compaction_keeps_original_context_and_saved_phone_input() -> Result<()>
{
    let (harness, thread, original) =
        create_imported_thread(ImportedCompactionProtocol::CompactEndpoint).await?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    Mock::given(path("/v1/responses/compact"))
        .respond_with(CompactResponder {
            requests: requests.clone(),
            fail_at: Some(2),
            max_assistant_items: None,
        })
        .mount(harness.server())
        .await;
    thread.submit(Op::Compact).await?;
    wait_for_event(&thread, |event| matches!(event, EventMsg::Error(_))).await;
    wait_for_turn_complete(&thread).await;
    assert_eq!(requests.lock().unwrap().len(), 2);
    thread.submit(Op::Shutdown).await?;
    wait_for_event(&thread, |event| matches!(event, EventMsg::ShutdownComplete)).await;
    let stored: Vec<_> = fs::read_to_string(thread.rollout_path().unwrap())?
        .lines()
        .map(codex_rollout::parse_rollout_line)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert!(
        stored
            .iter()
            .all(|record| !matches!(record.item, RolloutItem::Compacted(_)))
    );
    let original_stored: Vec<_> = stored
        .iter()
        .filter_map(|record| match &record.item {
            RolloutItem::ResponseItem(item) => Some(responses::strip_metadata(item.item.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        responses::strip_response_item_ids(&original_stored[..original.len()]),
        original
    );
    Ok(())
}

#[tokio::test]
async fn server_context_rejection_retries_smaller_slices_without_rewriting_history() -> Result<()> {
    let (harness, thread, original) =
        create_imported_thread(ImportedCompactionProtocol::CompactEndpoint).await?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    Mock::given(path("/v1/responses/compact"))
        .respond_with(CompactResponder {
            requests: requests.clone(),
            fail_at: None,
            max_assistant_items: Some(4),
        })
        .mount(harness.server())
        .await;

    thread.submit(Op::Compact).await?;
    wait_for_event(&thread, |event| match event {
        EventMsg::ItemCompleted(item) if matches!(item.item, TurnItem::ContextCompaction(_)) => {
            true
        }
        EventMsg::Error(error) => panic!("compaction failed: {}", error.message),
        EventMsg::TurnComplete(_) => panic!("turn completed without compaction"),
        _ => false,
    })
    .await;
    wait_for_turn_complete(&thread).await;

    let requests = requests.lock().unwrap().clone();
    let assistant_counts: Vec<_> = requests
        .iter()
        .map(|request| {
            request["input"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["type"] == "message" && item["role"] == "assistant")
                .count()
        })
        .collect();
    assert!(assistant_counts.first().is_some_and(|count| *count > 4));
    assert!(assistant_counts.iter().skip(1).any(|count| *count <= 4));

    thread.submit(Op::Shutdown).await?;
    wait_for_event(&thread, |event| matches!(event, EventMsg::ShutdownComplete)).await;
    let stored: Vec<_> = fs::read_to_string(thread.rollout_path().unwrap())?
        .lines()
        .map(codex_rollout::parse_rollout_line)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let original_stored: Vec<_> = stored
        .iter()
        .filter_map(|record| match &record.item {
            RolloutItem::ResponseItem(item) => Some(responses::strip_metadata(item.item.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        responses::strip_response_item_ids(&original_stored[..original.len()]),
        original
    );
    assert_eq!(
        stored
            .iter()
            .filter(|record| matches!(record.item, RolloutItem::Compacted(_)))
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn responses_v2_compacts_oversized_import_in_bounded_requests() -> Result<()> {
    let (harness, thread, original) =
        create_imported_thread(ImportedCompactionProtocol::ResponsesV2).await?;
    let mut responses = vec![responses::sse_failed(
        "rejected",
        "context_length_exceeded",
        "fixture server-side context overflow",
    )];
    responses.extend((1..=8).map(|index| {
        sse(vec![
            json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "compaction",
                    "encrypted_content": format!("summary-{index}"),
                },
            }),
            responses::ev_completed(&format!("response-{index}")),
        ])
    }));
    let response_mock = responses::mount_sse_sequence(harness.server(), responses).await;

    thread.submit(Op::Compact).await?;
    wait_for_event(&thread, |event| match event {
        EventMsg::ItemCompleted(item) if matches!(item.item, TurnItem::ContextCompaction(_)) => {
            true
        }
        EventMsg::Error(error) => panic!("compaction failed: {}", error.message),
        EventMsg::TurnComplete(_) => panic!("turn completed without compaction"),
        _ => false,
    })
    .await;
    wait_for_turn_complete(&thread).await;

    let requests = response_mock.requests();
    assert!(requests.len() > 2);
    assert!(requests.iter().all(|request| {
        request.inputs_of_type("compaction_trigger").len() == 1
            && estimate_compact_input_tokens(request) < 50_000
    }));
    assert!(
        requests[1].input().len() < requests[0].input().len(),
        "the context-window rejection should retry a smaller first batch"
    );
    let sent_assistant_messages = requests
        .iter()
        .skip(1)
        .flat_map(responses::ResponsesRequest::input)
        .filter(|item| item["type"] == "message" && item["role"] == "assistant")
        .collect::<Vec<_>>();
    let expected = original
        .iter()
        .filter(|item| matches!(item, ResponseItem::Message { role, .. } if role == "assistant"))
        .map(|item| serde_json::to_value(item).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        responses::strip_response_item_ids_from_json(responses::strip_metadata_from_json(json!(
            sent_assistant_messages
        ))),
        responses::strip_response_item_ids_from_json(responses::strip_metadata_from_json(json!(
            expected
        )))
    );

    thread.submit(Op::Shutdown).await?;
    wait_for_event(&thread, |event| matches!(event, EventMsg::ShutdownComplete)).await;
    let stored = fs::read_to_string(thread.rollout_path().unwrap())?
        .lines()
        .map(codex_rollout::parse_rollout_line)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let original_stored = stored
        .iter()
        .filter_map(|record| match &record.item {
            RolloutItem::ResponseItem(item) => Some(responses::strip_metadata(item.item.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        responses::strip_response_item_ids(&original_stored[..original.len()]),
        original
    );
    assert_eq!(
        stored
            .iter()
            .filter(|record| matches!(record.item, RolloutItem::Compacted(_)))
            .count(),
        1
    );
    Ok(())
}
