use super::*;
use crate::history_cell::PlainHistoryCell;
use pretty_assertions::assert_eq;
use ratatui::style::Modifier;

#[test]
fn timestamp_is_dimmed_and_right_aligned() {
    let cell = TimestampedHistoryCell {
        inner: Box::new(PlainHistoryCell::new(vec!["hello".into()])),
        time: "3:04 PM".to_string(),
    };

    let lines = cell.display_lines(/*width*/ 20);

    assert_eq!(lines.len(), 1);
    insta::assert_snapshot!(lines[0].to_string(), @"hello        3:04 PM");
    assert!(
        lines[0]
            .spans
            .last()
            .is_some_and(|span| span.style.add_modifier.contains(Modifier::DIM))
    );
}

#[test]
fn timestamp_uses_its_own_top_line_when_content_fills_width() {
    let cell = TimestampedHistoryCell {
        inner: Box::new(PlainHistoryCell::new(vec!["content fills".into()])),
        time: "3:04 PM".to_string(),
    };

    assert_eq!(
        cell.display_lines(/*width*/ 13)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>(),
        vec!["      3:04 PM", "content fills"]
    );
}

#[test]
fn raw_lines_preserve_original_content() {
    let cell = TimestampedHistoryCell {
        inner: Box::new(PlainHistoryCell::new(vec!["hello".into()])),
        time: "3:04 PM".to_string(),
    };

    assert_eq!(cell.raw_lines(), vec![Line::from("hello")]);
}

#[test]
fn timestamp_wrapper_preserves_concrete_cell_downcasts() {
    let cell = with_created_at(
        Box::new(PlainHistoryCell::new(vec!["hello".into()])),
        Some(1_725_000_000_123),
    );

    assert!(cell.as_any().is::<PlainHistoryCell>());
}

#[test]
fn timestamp_wrappers_preserve_warning_details_without_showing_hidden_diagnostics() {
    let warning = super::super::new_warning_event("Retained diagnostic".into());
    let expected = warning.warning_entries();
    let boxed = TimestampedHistoryCell {
        inner: Box::new(warning),
        time: "3:04 PM".into(),
    };
    let shared = ArcTimestampedHistoryCell {
        inner: Arc::new(super::super::new_warning_event(
            "Retained diagnostic".into(),
        )),
        time: "3:04 PM".into(),
    };
    for cell in [&boxed as &dyn HistoryCell, &shared] {
        assert_eq!(cell.warning_entries(), expected);
        assert_eq!(cell.warning_keys(), boxed.inner.warning_keys());
        assert_eq!(
            cell.display_lines_for_mode(/*width*/ 40, HistoryRenderMode::Raw),
            vec![]
        );
        assert_eq!(
            cell.display_hyperlink_lines_for_mode(/*width*/ 40, HistoryRenderMode::Raw),
            vec![]
        );
        assert!(!cell.raw_lines().is_empty());
    }
}

#[test]
fn timestamp_wrappers_retain_compact_plan_and_expansion() {
    let plan = Arc::new(super::super::new_plan_update(
        codex_protocol::plan_tool::UpdatePlanArgs {
            explanation: Some("Inspect the retained details".into()),
            plan: vec![],
        },
    ));
    let boxed = TimestampedHistoryCell {
        inner: Box::new(super::super::new_plan_update(
            codex_protocol::plan_tool::UpdatePlanArgs {
                explanation: Some("Inspect the retained details".into()),
                plan: vec![],
            },
        )),
        time: "3:04 PM".into(),
    };
    let shared = ArcTimestampedHistoryCell {
        inner: plan.clone(),
        time: "3:04 PM".into(),
    };
    for (cell, inner) in [
        (&boxed as &dyn HistoryCell, boxed.inner.as_ref()),
        (&shared, plan.as_ref()),
    ] {
        assert_eq!(cell.activity_ids(), inner.activity_ids());
        assert_eq!(
            cell.activity_disclosure(/*width*/ 48),
            inner.activity_disclosure(/*width*/ 48)
        );
        assert_eq!(
            cell.retained_hyperlink_lines(/*width*/ 48, /*detailed*/ false),
            add_time(
                inner.retained_hyperlink_lines(/*width*/ 48, /*detailed*/ false),
                /*width*/ 48,
                "3:04 PM"
            )
        );
        assert_eq!(
            cell.expanded_hyperlink_lines(/*width*/ 48),
            add_time(
                inner.expanded_hyperlink_lines(/*width*/ 48),
                /*width*/ 48,
                "3:04 PM"
            )
        );
    }
    insta::assert_snapshot!(
        ratatui::text::Text::from(visible_lines(shared.retained_hyperlink_lines(/*width*/ 48, /*detailed*/ false))),
        @"• Updated Plan · 0/0 complete            3:04 PM\n  └ (no steps provided)"
    );
}
