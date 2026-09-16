use super::*;
use crate::terminal_hyperlinks::HyperlinkParagraph;
use crate::terminal_images::ImagePreview;
use image::Rgba;
use image::RgbaImage;
use pretty_assertions::assert_eq;
use ratatui::style::Style;
use ratatui::widgets::Widget;
use std::sync::Arc;

fn preview() -> ImagePreview {
    ImagePreview(Arc::new(RgbaImage::from_fn(
        /*width*/ 40,
        /*height*/ 40,
        |x, y| Rgba([x as u8, y as u8, 180, 255]),
    )))
}

#[test]
fn layout_captures_only_visible_uncovered_image_rows() {
    let area = Rect::new(
        /*x*/ 3, /*y*/ 2, /*width*/ 8, /*height*/ 2,
    );
    let mut buffer = Buffer::empty(area);
    let lines = preview().lines(/*width*/ 4);
    let capture = FrameImageCapture::begin();
    HyperlinkParagraph::new(&lines, Style::default())
        .scroll(/*rows*/ 1)
        .render(area, &mut buffer);
    let images = capture.finish(&buffer);
    assert_eq!(images.len(), 1);
    assert_eq!(
        images[0].area,
        Rect::new(
            /*x*/ 3, /*y*/ 2, /*width*/ 4, /*height*/ 1
        )
    );
    assert_eq!(images[0].image.row, 1);

    let capture = FrameImageCapture::begin();
    HyperlinkParagraph::new(&lines, Style::default()).render(area, &mut buffer);
    Paragraph::new("popup").render(
        Rect::new(
            /*x*/ 5, /*y*/ 2, /*width*/ 3, /*height*/ 1,
        ),
        &mut buffer,
    );
    let images = capture.finish(&buffer);
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].area.y, 3);
    insta::assert_snapshot!(buffer.content.iter().map(Cell::symbol).collect::<String>(), @"▀▀pop   ▀▀▀▀    ");
}

#[test]
fn unchanged_frames_do_not_retransmit_images_and_removed_rows_are_erased() {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 8, /*height*/ 3,
    );
    let mut buffer = Buffer::empty(area);
    let lines = preview().lines(/*width*/ 4);
    let capture = FrameImageCapture::begin();
    HyperlinkParagraph::new(&lines, Style::default()).render(area, &mut buffer);
    let images = capture.finish(&buffer);
    let mut old = Buffer::empty(area);
    let rows = prepare_rows(&[], &images, &mut old, &buffer);
    assert_eq!(rows, BTreeSet::from([0, 1]));
    let mut output = Vec::new();
    write_rows(
        &mut output,
        &mut TerminalImageWriter::ItermInline,
        &images,
        &rows,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(output)
            .unwrap()
            .matches("File=inline=1")
            .count(),
        2
    );

    let rows = prepare_rows(&images, &images, &mut buffer.clone(), &buffer);
    assert_eq!(rows, BTreeSet::new());
    let mut next = buffer.clone();
    Paragraph::new("reply").render(area, &mut next);
    let rows = prepare_rows(&images, &[], &mut buffer, &next);
    assert_eq!(rows, BTreeSet::from([0, 1]));
    let mut output = Vec::new();
    clear_rows(
        &mut output,
        &TerminalImageWriter::KittyPhysical,
        &rows,
        /*left*/ 0,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("a=d,d=Y,y=1"));
    assert!(output.contains("a=d,d=Y,y=2"));
    assert!(!output.contains("a=T"));
}

#[test]
fn capture_is_cleared_when_rendering_returns_early() {
    {
        let _capture = FrameImageCapture::begin();
    }
    let capture = FrameImageCapture::begin();
    assert_eq!(capture.finish(&Buffer::empty(Rect::ZERO)), vec![]);
}
