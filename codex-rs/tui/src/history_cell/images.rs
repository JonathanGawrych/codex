use super::HistoryCell;
use super::PlainHistoryCell;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::visible_lines;
use crate::terminal_images::ImagePreview;
use ratatui::text::Line;

#[derive(Debug)]
pub(crate) struct ImageHistoryCell {
    pub(super) text: PlainHistoryCell,
    pub(super) preview: Option<ImagePreview>,
}

impl HistoryCell for ImageHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        visible_lines(self.display_hyperlink_lines(width))
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.text.raw_lines()
    }

    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        let mut lines = self.text.display_hyperlink_lines(width);
        if let Some(preview) = &self.preview {
            lines.extend(preview.lines(width));
        }
        lines
    }
}
