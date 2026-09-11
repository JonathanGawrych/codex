use super::*;
use codex_protocol::items::FileChangeItem;
use codex_protocol::items::McpToolCallItem;
use codex_protocol::items::McpToolCallStatus;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::PatchApplyStatus;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn mobile_paginated_history_bounds_payloads_and_preserves_full_detail() -> Result<()> {
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new("http://127.0.0.1:1").write(codex_home.path())?;
    let filename_ts = "2025-01-05T12-00-00";
    let id = create_fake_paginated_rollout(
        codex_home.path(),
        filename_ts,
        "2025-01-05T12:00:00Z",
        "original user",
        Some("mock_provider"),
        /*git_info*/ None,
    )?;
    let thread_id = codex_protocol::ThreadId::from_string(&id)?;
    let path = rollout_path(codex_home.path(), filename_ts, &id);
    let mut records: Vec<Value> = std::fs::read_to_string(&path)?
        .lines()
        .map(serde_json::from_str)
        .collect::<serde_json::Result<_>>()?;
    let mut items = vec![paginated_turn_started("large-turn")];
    for index in 0..3 {
        items.push(paginated_completed_item(
            thread_id,
            "large-turn",
            CoreTurnItem::FileChange(FileChangeItem {
                id: format!("patch-{index}"),
                changes: [(
                    format!("file-{index}.txt").into(),
                    FileChange::Add {
                        content: "+ é Unicode diff contents\n".repeat(34_000),
                    },
                )]
                .into_iter()
                .collect(),
                status: Some(PatchApplyStatus::Completed),
                auto_approved: Some(true),
                stdout: None,
                stderr: None,
            }),
        ));
    }
    let mcp: McpToolCallItem = serde_json::from_value(json!({
        "id": "mcp-image", "server": "image-fixture", "tool": "read", "arguments": {},
        "status": "completed", "result": {"content": [
            {"type": "image", "mimeType": "image/png", "data": "a".repeat(8_000_000)}
        ]}, "duration": {"secs": 0, "nanos": 1000000},
    }))?;
    assert_eq!(mcp.status, McpToolCallStatus::Completed);
    items.push(paginated_completed_item(
        thread_id,
        "large-turn",
        CoreTurnItem::McpToolCall(mcp),
    ));
    items.push(paginated_turn_completed("large-turn"));
    for item in items {
        let mut record = serde_json::to_value(item)?;
        record["timestamp"] = json!("2025-01-05T12:00:01Z");
        record["ordinal"] = json!(records.len());
        records.push(record);
    }
    let original = records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, &original)?;

    let mut desktop = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let resume = desktop
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: id.clone(),
            exclude_turns: true,
            ..Default::default()
        })
        .await?;
    let _: ThreadResumeResponse =
        timeout(DEFAULT_READ_TIMEOUT, desktop.read_response(resume)).await??;
    let full = read_items_page(
        &mut desktop,
        thread_id,
        Some("large-turn"),
        /*cursor*/ None,
        Some(100),
        SortDirection::Asc,
    )
    .await?;
    assert_eq!(full.data.len(), 4);
    assert!(serde_json::to_vec(&full)?.len() > 10_000_000);
    desktop.shutdown_gracefully().await?;

    for client_name in ["codex_chatgpt_android_remote", "codex_chatgpt_ios_remote"] {
        let mut mobile = TestAppServer::builder()
            .with_codex_home(codex_home.path())
            .build()
            .await?;
        mobile
            .initialize_with_client_info(ClientInfo {
                name: client_name.into(),
                title: None,
                version: "test".into(),
            })
            .await?;
        let resume = mobile
            .send_thread_resume_request(ThreadResumeParams {
                thread_id: id.clone(),
                exclude_turns: true,
                ..Default::default()
            })
            .await?;
        let _: ThreadResumeResponse =
            timeout(DEFAULT_READ_TIMEOUT, mobile.read_response(resume)).await??;
        let summaries = read_turns_page(
            &mut mobile,
            thread_id,
            /*cursor*/ None,
            Some(5),
            SortDirection::Desc,
            Some(TurnItemsView::Summary),
        )
        .await?;
        assert_eq!(summaries.data[0].id, "large-turn");
        let turns = read_turns_page(
            &mut mobile,
            thread_id,
            /*cursor*/ None,
            Some(5),
            SortDirection::Desc,
            Some(TurnItemsView::Full),
        )
        .await?;
        assert!(serde_json::to_vec(&turns)?.len() < 60_000);
        let mut preview = full.clone();
        preview.data.clear();
        let mut cursor = None;
        loop {
            let page = read_items_page(
                &mut mobile,
                thread_id,
                Some("large-turn"),
                cursor,
                Some(2),
                SortDirection::Asc,
            )
            .await?;
            assert!(serde_json::to_vec(&page)?.len() < 40_000);
            assert!(!page.data.is_empty());
            preview.data.extend(page.data);
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(preview.data.len(), full.data.len());
        for (entry, original) in preview.data.iter().zip(&full.data) {
            assert_eq!(
                (&entry.turn_id, entry.created_at_ms, entry.item.id()),
                (
                    &original.turn_id,
                    original.created_at_ms,
                    original.item.id()
                )
            );
        }
        let diffs: Vec<_> = preview
            .data
            .iter()
            .filter_map(|entry| match &entry.item {
                ThreadItem::FileChange { changes, .. } => Some(changes[0].diff.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(diffs.len(), 3);
        assert!(diffs.iter().all(|diff| diff.ends_with(
            "[Mobile preview truncated. Open this thread in the TUI or desktop for full details.]"
        )));
        assert_eq!(&preview.data[3].item, &turns.data[0].items[3]);
        mobile.shutdown_gracefully().await?;
    }
    let mut desktop = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let reread = read_items_page(
        &mut desktop,
        thread_id,
        Some("large-turn"),
        /*cursor*/ None,
        Some(100),
        SortDirection::Asc,
    )
    .await?;
    assert_eq!(reread, full);
    assert!(std::fs::read_to_string(path)?.starts_with(&original));
    desktop.shutdown_gracefully().await?;
    Ok(())
}
