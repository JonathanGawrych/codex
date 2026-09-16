//! Collect image placements during synchronous layout and discard rows covered by later widgets.
//! The terminal writes only changed rows, after text, so graphics cannot cover a popup or composer.

use super::ImageRow;
use super::TerminalImageWriter;
use super::delete_kitty_images_in_rows;
use crate::terminal_hyperlinks::HyperlinkLine;
use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType;
use ratatui::buffer::Buffer;
use ratatui::buffer::Cell;
use ratatui::buffer::CellDiffOption;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io::Write;

thread_local! {
    static CAPTURE: RefCell<Option<Vec<PlacedImage>>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PlacedImage {
    area: Rect,
    image: ImageRow,
    cells: Vec<Cell>,
}

pub(crate) struct FrameImageCapture;

impl FrameImageCapture {
    pub(crate) fn begin() -> Self {
        CAPTURE.with_borrow_mut(|capture| {
            assert!(capture.is_none(), "terminal frame rendering cannot nest");
            *capture = Some(Vec::new());
        });
        Self
    }

    #[expect(
        clippy::expect_used,
        reason = "begin initializes the capture and finish consumes its guard before Drop clears it."
    )]
    pub(crate) fn finish(self, buffer: &Buffer) -> Vec<PlacedImage> {
        CAPTURE.with_borrow_mut(|capture| {
            capture
                .take()
                .expect("frame capture is active")
                .into_iter()
                .filter(|placement| {
                    placement.area.intersection(buffer.area) == placement.area
                        && placement.area.columns().zip(&placement.cells).all(
                            |(column, expected)| &buffer[(column.x, placement.area.y)] == expected,
                        )
                })
                .collect()
        })
    }
}

impl Drop for FrameImageCapture {
    fn drop(&mut self) {
        CAPTURE.with_borrow_mut(|capture| {
            capture.take();
        });
    }
}

pub(crate) fn capture_images(
    lines: &[HyperlinkLine],
    area: Rect,
    buffer: &Buffer,
    scroll_rows: u16,
) {
    CAPTURE.with_borrow_mut(|capture| {
        let Some(placements) = capture else {
            return;
        };
        if area.is_empty() || lines.iter().all(|line| line.image.is_none()) {
            return;
        }
        let mut row = 0;
        for line in lines {
            let height = Paragraph::new(line.line.clone())
                .wrap(Wrap { trim: false })
                .line_count(area.width);
            if let Some(image) = &line.image
                && height == 1
                && row >= usize::from(scroll_rows)
                && row - usize::from(scroll_rows) < usize::from(area.height)
                && image.column.saturating_add(image.columns) <= area.width
            {
                let image_area = Rect::new(
                    area.x + image.column,
                    area.y + (row - usize::from(scroll_rows)) as u16,
                    image.columns,
                    /*height*/ 1,
                );
                let mut image = image.clone();
                image.column = 0;
                placements.push(PlacedImage {
                    area: image_area,
                    image,
                    cells: image_area
                        .columns()
                        .map(|column| buffer[(column.x, image_area.y)].clone())
                        .collect(),
                });
            }
            row += height;
        }
    });
}

/// Dirty rows include text changes beside images: terminal erase commands can clear graphics too.
pub(crate) fn prepare_rows(
    previous: &[PlacedImage],
    next: &[PlacedImage],
    old: &mut Buffer,
    new: &Buffer,
) -> BTreeSet<u16> {
    let mut rows = BTreeSet::new();
    for placement in previous.iter().chain(next) {
        let y = placement.area.y;
        if y < new.area.top() || y >= new.area.bottom() {
            continue;
        }
        let changed = previous
            .iter()
            .filter(|image| image.area.y == y)
            .ne(next.iter().filter(|image| image.area.y == y))
            || (new.area.left()..new.area.right())
                .any(|x| old.cell((x, y)).is_none_or(|cell| cell != &new[(x, y)]));
        if changed {
            rows.insert(y);
        }
    }
    for &y in &rows {
        for x in new.area.left()..new.area.right() {
            if let Some(cell) = old.cell_mut((x, y)) {
                cell.reset();
                cell.set_diff_option(CellDiffOption::AlwaysUpdate);
            }
        }
    }
    rows
}

pub(crate) fn clear_rows(
    writer: &mut impl Write,
    protocol: &TerminalImageWriter,
    rows: &BTreeSet<u16>,
    left: u16,
) -> std::io::Result<()> {
    for &row in rows {
        if matches!(protocol, TerminalImageWriter::KittyPhysical) {
            delete_kitty_images_in_rows(writer, row..row.saturating_add(/*rhs*/ 1))?;
        }
        queue!(writer, MoveTo(left, row), Clear(ClearType::UntilNewLine))?;
    }
    Ok(())
}

pub(crate) fn write_rows(
    writer: &mut impl Write,
    protocol: &mut TerminalImageWriter,
    images: &[PlacedImage],
    rows: &BTreeSet<u16>,
) -> std::io::Result<()> {
    for placement in images {
        if rows.contains(&placement.area.y) {
            queue!(writer, MoveTo(placement.area.x, placement.area.y))?;
            protocol.write_image_row(writer, &placement.image)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;
