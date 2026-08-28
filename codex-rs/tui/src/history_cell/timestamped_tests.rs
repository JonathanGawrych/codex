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
