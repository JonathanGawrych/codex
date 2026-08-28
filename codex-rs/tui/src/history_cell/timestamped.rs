use super::HistoryCell;
use super::HistoryRenderMode;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::visible_lines;
use chrono::Local;
use chrono::TimeZone;
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use std::sync::Arc;

#[derive(Debug)]
pub(super) struct TimestampedHistoryCell {
    inner: Box<dyn HistoryCell>,
    time: String,
}

#[derive(Debug)]
pub(super) struct ArcTimestampedHistoryCell {
    inner: Arc<dyn HistoryCell>,
    time: String,
}

impl TimestampedHistoryCell {
    pub(super) fn inner(&self) -> &dyn HistoryCell {
        self.inner.as_ref()
    }

    pub(super) fn inner_mut(&mut self) -> &mut dyn HistoryCell {
        self.inner.as_mut()
    }

    fn add_time(&self, lines: Vec<HyperlinkLine>, width: u16) -> Vec<HyperlinkLine> {
        add_time(lines, width, &self.time)
    }
}

impl ArcTimestampedHistoryCell {
    pub(super) fn inner(&self) -> &dyn HistoryCell {
        self.inner.as_ref()
    }

    fn add_time(&self, lines: Vec<HyperlinkLine>, width: u16) -> Vec<HyperlinkLine> {
        add_time(lines, width, &self.time)
    }
}

fn add_time(mut lines: Vec<HyperlinkLine>, width: u16, time: &str) -> Vec<HyperlinkLine> {
    let Some(first) = lines.first_mut() else {
        return lines;
    };
    let width = usize::from(width);
    let time_width = time.len();
    if width < time_width {
        return lines;
    }

    let content_width = first.width();
    if content_width.saturating_add(/*rhs*/ 1 + time_width) <= width {
        first
            .line
            .push_span(" ".repeat(width - content_width - time_width));
        first.line.push_span(time.to_string().dim());
        return lines;
    }

    lines.insert(
        /*index*/ 0,
        Line::from(vec![
            " ".repeat(width - time_width).into(),
            time.to_string().dim(),
        ])
        .into(),
    );
    lines
}

impl HistoryCell for TimestampedHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        visible_lines(self.display_hyperlink_lines(width))
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.inner.raw_lines()
    }

    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.add_time(self.inner.display_hyperlink_lines(width), width)
    }

    fn display_lines_for_mode(&self, width: u16, mode: HistoryRenderMode) -> Vec<Line<'static>> {
        match mode {
            HistoryRenderMode::Rich => self.display_lines(width),
            HistoryRenderMode::Raw => self.inner.raw_lines(),
        }
    }

    fn display_hyperlink_lines_for_mode(
        &self,
        width: u16,
        mode: HistoryRenderMode,
    ) -> Vec<HyperlinkLine> {
        match mode {
            HistoryRenderMode::Rich => self.display_hyperlink_lines(width),
            HistoryRenderMode::Raw => self.inner.display_hyperlink_lines_for_mode(width, mode),
        }
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        visible_lines(self.transcript_hyperlink_lines(width))
    }

    fn transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.add_time(self.inner.transcript_hyperlink_lines(width), width)
    }

    fn has_stable_transcript_height(&self) -> bool {
        self.inner.has_stable_transcript_height()
    }

    fn is_stream_continuation(&self) -> bool {
        self.inner.is_stream_continuation()
    }

    fn transcript_animation_tick(&self) -> Option<u64> {
        self.inner.transcript_animation_tick()
    }
}

impl HistoryCell for ArcTimestampedHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        visible_lines(self.display_hyperlink_lines(width))
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.inner.raw_lines()
    }

    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.add_time(self.inner.display_hyperlink_lines(width), width)
    }

    fn display_lines_for_mode(&self, width: u16, mode: HistoryRenderMode) -> Vec<Line<'static>> {
        match mode {
            HistoryRenderMode::Rich => self.display_lines(width),
            HistoryRenderMode::Raw => self.inner.raw_lines(),
        }
    }

    fn display_hyperlink_lines_for_mode(
        &self,
        width: u16,
        mode: HistoryRenderMode,
    ) -> Vec<HyperlinkLine> {
        match mode {
            HistoryRenderMode::Rich => self.display_hyperlink_lines(width),
            HistoryRenderMode::Raw => self.inner.display_hyperlink_lines_for_mode(width, mode),
        }
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        visible_lines(self.transcript_hyperlink_lines(width))
    }

    fn transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.add_time(self.inner.transcript_hyperlink_lines(width), width)
    }

    fn has_stable_transcript_height(&self) -> bool {
        self.inner.has_stable_transcript_height()
    }

    fn is_stream_continuation(&self) -> bool {
        self.inner.is_stream_continuation()
    }

    fn transcript_animation_tick(&self) -> Option<u64> {
        self.inner.transcript_animation_tick()
    }
}

pub(crate) fn with_created_at(
    cell: Box<dyn HistoryCell>,
    created_at_ms: Option<i64>,
) -> Box<dyn HistoryCell> {
    let Some(time) = format_time(created_at_ms) else {
        return cell;
    };
    Box::new(TimestampedHistoryCell { inner: cell, time })
}

pub(crate) fn with_created_at_arc(
    cell: Arc<dyn HistoryCell>,
    created_at_ms: Option<i64>,
) -> Arc<dyn HistoryCell> {
    let Some(time) = format_time(created_at_ms) else {
        return cell;
    };
    Arc::new(ArcTimestampedHistoryCell { inner: cell, time })
}

fn format_time(created_at_ms: Option<i64>) -> Option<String> {
    created_at_ms
        .filter(|created_at_ms| *created_at_ms > 0)
        .and_then(|created_at_ms| Local.timestamp_millis_opt(created_at_ms).single())
        .map(|created_at| created_at.format("%-I:%M %p").to_string())
}

#[cfg(test)]
#[path = "timestamped_tests.rs"]
mod tests;
