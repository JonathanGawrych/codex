use super::*;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use pretty_assertions::assert_eq;

#[test]
fn preview_truncation_preserves_utf8_and_explains_full_detail_access() {
    let mut text = "abcédef".to_string();
    truncate_preview(&mut text, 4);
    assert_eq!(
        text,
        "abc\n[Mobile preview truncated. Open this thread in the TUI or desktop for full details.]"
    );
    let mut text = "éé".to_string();
    truncate_preview(&mut text, 4);
    assert_eq!(text, "éé");
}

#[test]
fn file_preview_budget_is_shared_across_changes_without_removing_paths() {
    let changes = vec!["first", "second"]
        .into_iter()
        .map(|path| FileUpdateChange {
            path: path.into(),
            kind: PatchChangeKind::Add,
            diff: "x".repeat(PREVIEW_BYTES),
        })
        .collect();
    let mut item = ThreadItem::FileChange {
        id: "patch".into(),
        changes,
        status: PatchApplyStatus::Completed,
    };
    redact_thread_history_item(&mut item);
    assert_eq!(
        item,
        ThreadItem::FileChange {
            id: "patch".into(),
            status: PatchApplyStatus::Completed,
            changes: vec![
                FileUpdateChange {
                    path: "first".into(),
                    kind: PatchChangeKind::Add,
                    diff: "x".repeat(PREVIEW_BYTES)
                },
                FileUpdateChange {
                    path: "second".into(),
                    kind: PatchChangeKind::Add,
                    diff: PREVIEW_NOTICE.into()
                },
            ],
        }
    );
}

#[test]
fn paginated_generated_image_keeps_its_id_with_an_empty_inline_result() {
    let mut item: ThreadItem = serde_json::from_value(serde_json::json!({
        "type": "imageGeneration", "id": "image", "status": "completed",
        "result": "x".repeat(1_000_000), "revisedPrompt": null, "failure": null,
    }))
    .unwrap();
    let mut expected = serde_json::to_value(&item).unwrap();
    expected["result"] = serde_json::json!("");
    redact_thread_history_item(&mut item);
    assert_eq!(item.id(), "image");
    assert_eq!(serde_json::to_value(item).unwrap(), expected);
}

#[test]
fn command_output_preview_preserves_the_command_and_execution_metadata() {
    let mut item: ThreadItem = serde_json::from_value(serde_json::json!({
        "type": "commandExecution", "id": "command", "command": "cat report.txt",
        "cwd": "/workspace", "status": "completed", "commandActions": [],
        "aggregatedOutput": "x".repeat(PREVIEW_BYTES + 1), "exitCode": 0, "durationMs": 42,
    }))
    .unwrap();
    let mut expected = serde_json::to_value(&item).unwrap();
    expected["aggregatedOutput"] = serde_json::json!("x".repeat(PREVIEW_BYTES) + PREVIEW_NOTICE);
    redact_thread_history_item(&mut item);
    assert_eq!(serde_json::to_value(item).unwrap(), expected);
}
