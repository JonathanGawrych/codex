//! Bounded chat previews. Graphics are emitted only by the scrollback writer; ordinary
//! ratatui buffers use colored half blocks, and raw history retains the original text.

use std::fmt;
use std::fs::File;
use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use anyhow::ensure;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use codex_terminal_detection::TerminalInfo;
use codex_terminal_detection::TerminalName;
use image::ImageFormat;
use image::ImageReader;
use image::RgbaImage;
use image::imageops::FilterType;
use ratatui::style::Color;
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::terminal_hyperlinks::HyperlinkLine;

const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_COLUMNS: u16 = 64;
const MAX_ROWS: u16 = 20;
const PIXELS_PER_COLUMN: u32 = 10;
const PIXELS_PER_ROW: u32 = 20;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ImagePreview(Arc<RgbaImage>);

impl fmt::Debug for ImagePreview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ImagePreview")
            .field(&self.0.dimensions())
            .finish()
    }
}

/// Each strip occupies exactly one terminal row. Partial history replay and scroll-region
/// insertion can therefore clip or scroll a preview without moving the composer cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageRow {
    preview: ImagePreview,
    row: u16,
    rows: u16,
    columns: u16,
    pub(crate) column: u16,
}

impl ImagePreview {
    pub(crate) fn from_path(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        ensure!(file.metadata()?.is_file(), "image is not a regular file");
        let mut bytes = Vec::new();
        file.take((MAX_IMAGE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        Self::from_bytes(&bytes)
    }

    pub(crate) fn from_base64(data: &str) -> Result<Self> {
        let data = if let Some(data_url) = data.strip_prefix("data:") {
            data_url
                .split_once(',')
                .ok_or_else(|| anyhow::anyhow!("invalid image data URL"))?
                .1
        } else {
            data
        };
        ensure!(
            data.len() <= MAX_IMAGE_BYTES.div_ceil(/*rhs*/ 3) * 4,
            "image exceeds 32 MiB"
        );
        Self::from_bytes(&STANDARD.decode(data)?)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_IMAGE_BYTES, "image exceeds 32 MiB");
        let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode()?;
        let max_width = u32::from(MAX_COLUMNS) * PIXELS_PER_COLUMN;
        let max_height = u32::from(MAX_ROWS) * PIXELS_PER_ROW;
        let image = if image.width() > max_width || image.height() > max_height {
            image.thumbnail(max_width, max_height)
        } else {
            image
        };
        Ok(Self(Arc::new(image.to_rgba8())))
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "Image previews retain source pixel colors rather than theme colors."
    )]
    pub(crate) fn lines(&self, width: u16) -> Vec<HyperlinkLine> {
        if width == 0 {
            return Vec::new();
        }
        let columns = width
            .min(MAX_COLUMNS)
            .min(self.0.width().div_ceil(PIXELS_PER_COLUMN) as u16)
            .max(/*other*/ 1);
        let rows = (u32::from(columns) * self.0.height())
            .div_ceil(self.0.width() * 2)
            .clamp(/*min*/ 1, u32::from(MAX_ROWS)) as u16;
        let fallback = image::imageops::resize(
            &*self.0,
            u32::from(columns),
            u32::from(rows) * 2,
            FilterType::Triangle,
        );
        (0..rows)
            .map(|row| {
                let spans = (0..columns)
                    .map(|column| {
                        let top = fallback.get_pixel(u32::from(column), u32::from(row) * 2);
                        let bottom = fallback.get_pixel(u32::from(column), u32::from(row) * 2 + 1);
                        let color = |pixel: &image::Rgba<u8>| {
                            if pixel[3] == 0 {
                                Color::Reset
                            } else {
                                Color::Rgb(pixel[0], pixel[1], pixel[2])
                            }
                        };
                        Span::from("▀").fg(color(top)).bg(color(bottom))
                    })
                    .collect::<Vec<_>>();
                let mut line = HyperlinkLine::new(Line::from(spans));
                line.image = Some(ImageRow {
                    preview: self.clone(),
                    row,
                    rows,
                    columns,
                    column: 0,
                });
                line
            })
            .collect()
    }
}

impl ImageRow {
    pub(crate) fn write_kitty(&self, writer: &mut impl Write) -> std::io::Result<()> {
        let image = &self.preview.0;
        let top = u32::from(self.row) * image.height() / u32::from(self.rows);
        let bottom = (u32::from(self.row) + 1) * image.height() / u32::from(self.rows);
        let strip = image::imageops::crop_imm(
            &**image,
            /*x*/ 0,
            top,
            image.width(),
            (bottom - top).max(/*other*/ 1),
        )
        .to_image();
        let mut png = Cursor::new(Vec::new());
        strip
            .write_to(&mut png, ImageFormat::Png)
            .map_err(std::io::Error::other)?;
        let payload = STANDARD.encode(png.into_inner());
        let chunks = payload.as_bytes().chunks(/*chunk_size*/ 4096);
        let count = chunks.len();
        writer.write_all(" ".repeat(usize::from(self.column)).as_bytes())?;
        for (index, chunk) in chunks.enumerate() {
            let more = u8::from(index + 1 < count);
            if index == 0 {
                write!(
                    writer,
                    "\x1b_Ga=T,t=d,f=100,c={},r=1,C=1,q=2,m={more};",
                    self.columns
                )?;
            } else {
                write!(writer, "\x1b_Gq=2,m={more};")?;
            }
            writer.write_all(chunk)?;
            writer.write_all(b"\x1b\\")?;
        }
        Ok(())
    }
}

pub(crate) fn supports_kitty(info: &TerminalInfo) -> bool {
    if info.multiplexer.is_some() {
        return false;
    }
    match info.name {
        TerminalName::Kitty | TerminalName::Ghostty | TerminalName::WezTerm => true,
        TerminalName::Iterm2 => info
            .version
            .as_deref()
            .and_then(|version| {
                let mut parts = version.split('.');
                Some((
                    parts.next()?.parse::<u32>().ok()?,
                    parts.next()?.parse::<u32>().ok()?,
                ))
            })
            .is_some_and(|version| version >= (3, 6)),
        TerminalName::AppleTerminal
        | TerminalName::WarpTerminal
        | TerminalName::VsCode
        | TerminalName::Alacritty
        | TerminalName::Konsole
        | TerminalName::GnomeTerminal
        | TerminalName::Vte
        | TerminalName::WindowsTerminal
        | TerminalName::Dumb
        | TerminalName::Unknown => false,
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
