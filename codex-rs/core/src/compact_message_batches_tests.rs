use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

fn create_message(role: &str, text: &str) -> ResponseItem {
    serde_json::from_value(json!({
        "type": "message", "role": role,
        "content": [{"type": "input_text", "text": text}],
    }))
    .unwrap()
}

fn create_summary() -> ResponseItem {
    serde_json::from_value(json!({
        "type": "compaction", "encrypted_content": "summary",
    }))
    .unwrap()
}

#[test]
fn batches_cover_all_messages_and_keep_the_entire_compacted_output() {
    let original: Vec<_> = (0..10)
        .map(|index| create_message("assistant", &format!("{index}{}", "x".repeat(2_000))))
        .collect();
    let mut prompt = Prompt {
        input: original.clone(),
        base_instructions: codex_protocol::models::BaseInstructions {
            text: String::new(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut batches = MessageCompactionBatches::prepare(&mut prompt, Some(2_000))
        .unwrap()
        .unwrap();
    let retained = vec![create_message("user", "retained user"), create_summary()];
    let mut consumed = prompt.input;
    while batches.has_remaining() {
        let next = batches.next_input(retained.clone()).unwrap();
        assert_eq!(&next[..retained.len()], &retained);
        assert!(next.iter().map(estimate_item_token_count).sum::<i64>() <= batches.input_budget);
        consumed.extend_from_slice(&next[retained.len()..]);
    }
    assert_eq!(consumed, original);
}

#[test]
fn ordinary_compaction_and_native_tool_histories_are_unchanged() {
    for input in [
        vec![create_message("user", "small")],
        vec![
            serde_json::from_value(json!({
                "type": "function_call", "call_id": "c1", "name": "exec",
                "arguments": "x".repeat(30_000),
            }))
            .unwrap(),
        ],
    ] {
        let mut prompt = Prompt {
            input: input.clone(),
            base_instructions: codex_protocol::models::BaseInstructions {
                text: String::new(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(
            MessageCompactionBatches::prepare(&mut prompt, Some(2_000))
                .unwrap()
                .is_none()
        );
        assert_eq!(prompt.input, input);
    }
}

#[test]
fn retained_output_that_leaves_no_room_fails_without_dropping_items() {
    let item = create_message("user", &"x".repeat(2_000));
    let mut batches = MessageCompactionBatches {
        pending: VecDeque::from([(item.clone(), estimate_item_token_count(&item))]),
        input_budget: 800,
        maximum_input_budget: 800,
        in_flight_prefix_len: 0,
        in_flight_source_costs: Vec::new(),
    };
    let error = batches
        .next_input(vec![item.clone(), create_summary()])
        .unwrap_err();
    assert_eq!(
        batches.pending,
        VecDeque::from([(item.clone(), estimate_item_token_count(&item))])
    );
    insta::assert_snapshot!(error.to_string(), @"Message history cannot fit another bounded compaction request. No context was replaced; all saved messages remain intact.");
}

#[test]
fn empty_or_oversized_compaction_output_is_rejected() {
    let batches = MessageCompactionBatches {
        pending: VecDeque::new(),
        input_budget: 100,
        maximum_input_budget: 100,
        in_flight_prefix_len: 0,
        in_flight_source_costs: Vec::new(),
    };
    assert!(batches.validate_output(&[]).is_err());
    assert!(
        batches
            .validate_output(&[create_summary(), create_message("user", &"x".repeat(2_000))])
            .is_err()
    );
    assert!(
        batches
            .validate_output(&[create_summary(), create_message("user", "keep")])
            .is_ok()
    );
}

#[test]
fn rejected_batch_is_restored_and_halved_without_reordering_messages() {
    let original: Vec<_> = (0..7)
        .map(|index| create_message("assistant", &format!("message {index}")))
        .collect();
    let costs: Vec<_> = original.iter().map(estimate_item_token_count).collect();
    let prefix = vec![create_message("user", "retained"), create_summary()];
    let maximum_input_budget = costs.iter().copied().fold(
        prefix
            .iter()
            .map(estimate_item_token_count)
            .fold(0, i64::saturating_add),
        i64::saturating_add,
    );
    let mut batches = MessageCompactionBatches {
        pending: original
            .iter()
            .cloned()
            .zip(costs.iter().copied())
            .collect(),
        input_budget: maximum_input_budget,
        maximum_input_budget,
        in_flight_prefix_len: 0,
        in_flight_source_costs: Vec::new(),
    };
    let mut input = batches.next_input(prefix.clone()).unwrap();
    assert_eq!(&input[prefix.len()..], original);

    assert!(batches.retry_smaller(&mut input).unwrap());
    assert_eq!(&input[..prefix.len()], prefix);
    assert_eq!(&input[prefix.len()..], &original[..4]);
    assert_eq!(
        batches
            .pending
            .iter()
            .map(|(item, _)| item)
            .collect::<Vec<_>>(),
        original[4..].iter().collect::<Vec<_>>()
    );

    assert!(batches.retry_smaller(&mut input).unwrap());
    assert_eq!(&input[..prefix.len()], prefix);
    assert_eq!(&input[prefix.len()..], &original[..2]);
    assert_eq!(
        batches
            .pending
            .iter()
            .map(|(item, _)| item)
            .collect::<Vec<_>>(),
        original[2..].iter().collect::<Vec<_>>()
    );
}
