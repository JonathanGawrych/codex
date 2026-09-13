use super::*;
use crate::history_cell::HistoryCell;
use crate::history_cell::HistoryRenderMode;
use crate::history_cell::new_image_generation_call;
use crate::history_cell::with_created_at;
use crate::insert_history::HistoryLineWrapPolicy;
use crate::insert_history::wrap_history_hyperlink_lines;
use crate::terminal_hyperlinks::prefix_hyperlink_lines;
use codex_terminal_detection::Multiplexer;
use codex_utils_absolute_path::AbsolutePathBuf;
use image::Rgba;
use pretty_assertions::assert_eq;

fn create_png(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_fn(width, height, |x, y| {
        Rgba([((x * 31) % 256) as u8, ((y * 47) % 256) as u8, 180, 255])
    });
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[test]
fn preview_is_bounded_and_reflows_to_the_available_width() {
    let preview = ImagePreview::from_bytes(&create_png(/*width*/ 1280, /*height*/ 800)).unwrap();
    assert_eq!(preview.0.dimensions(), (640, 400));
    let dimensions = [0, 1, 20, 64, 200].map(|width| {
        let lines = preview.lines(width);
        (lines.first().map(HyperlinkLine::width), lines.len())
    });
    assert_eq!(
        dimensions,
        [
            (None, 0),
            (Some(1), 1),
            (Some(20), 7),
            (Some(64), 20),
            (Some(64), 20)
        ]
    );
}

#[test]
fn text_preview_snapshot_preserves_colors_and_omits_pixel_bytes_from_debug() {
    let preview = ImagePreview::from_bytes(&create_png(/*width*/ 40, /*height*/ 40)).unwrap();
    insta::assert_debug_snapshot!(preview.lines(/*width*/ 80));
}

#[test]
fn invalid_oversized_and_incomplete_images_are_rejected() {
    assert!(ImagePreview::from_base64("not-base64").is_err());
    assert!(ImagePreview::from_bytes(&create_png(/*width*/ 4, /*height*/ 4)[..33]).is_err());
    assert!(ImagePreview::from_bytes(&vec![0; MAX_IMAGE_BYTES + 1]).is_err());
    assert!(ImagePreview::from_bytes(&create_png(/*width*/ 8193, /*height*/ 1)).is_err());
    let bytes = create_png(/*width*/ 40, /*height*/ 40);
    let base64 = STANDARD.encode(&bytes);
    assert_eq!(
        ImagePreview::from_base64(&format!("data:image/png;base64,{base64}")).unwrap(),
        ImagePreview::from_bytes(&bytes).unwrap()
    );
}

#[test]
fn row_transmission_is_quiet_cursor_neutral_and_contains_only_a_png_strip() {
    let mut state = 123u32;
    let image = RgbaImage::from_fn(/*width*/ 640, /*height*/ 400, |_x, _y| {
        state = state
            .wrapping_mul(/*rhs*/ 1_664_525)
            .wrapping_add(/*rhs*/ 1_013_904_223);
        let [red, green, blue, _] = state.to_le_bytes();
        Rgba([red, green, blue, 255])
    });
    let preview = ImagePreview(Arc::new(image));
    let lines = preview.lines(/*width*/ 64);
    let row = lines[10].image.as_ref().unwrap();
    let mut output = Vec::new();
    row.write_kitty(&mut output).unwrap();
    let output = String::from_utf8(output).unwrap();
    let sequences = output
        .split("\x1b\\")
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    assert!(sequences.len() > 1, "exercise chunked transmission");
    assert!(sequences[0].starts_with("\x1b_Ga=T,t=d,f=100,c=64,r=1,C=1,q=2,m=1;"));
    let mut payload = String::new();
    for (index, sequence) in sequences.iter().enumerate() {
        let (header, chunk) = sequence.split_once(';').unwrap();
        assert!(header.contains("q=2"));
        assert!(chunk.len() <= 4096);
        assert!(header.ends_with(if index + 1 == sequences.len() {
            "m=0"
        } else {
            "m=1"
        }));
        payload.push_str(chunk);
    }
    let strip = image::load_from_memory(&STANDARD.decode(payload).unwrap())
        .unwrap()
        .to_rgba8();
    assert_eq!(
        strip,
        image::imageops::crop_imm(
            &*preview.0,
            /*x*/ 0,
            /*y*/ 200,
            /*width*/ 640,
            /*height*/ 20
        )
        .to_image()
    );
    assert!(!output.contains('\n'));
}

#[test]
fn history_wrapping_and_tail_clipping_keep_independent_image_rows() {
    let preview = ImagePreview::from_bytes(&create_png(/*width*/ 160, /*height*/ 160)).unwrap();
    let lines = prefix_hyperlink_lines(preview.lines(/*width*/ 20), "  ".into(), "  ".into());
    for policy in [
        HistoryLineWrapPolicy::PreWrap,
        HistoryLineWrapPolicy::Terminal,
    ] {
        let (wrapped, rows) = wrap_history_hyperlink_lines(&lines, /*wrap_width*/ 20, policy);
        assert_eq!((&wrapped, rows), (&lines, lines.len()));
        let mut output = Vec::new();
        wrapped
            .last()
            .unwrap()
            .image
            .as_ref()
            .unwrap()
            .write_kitty(&mut output)
            .unwrap();
        assert!(output.starts_with(b"  \x1b_G"));
    }
    let (narrow, _) = wrap_history_hyperlink_lines(
        &lines,
        /*wrap_width*/ 5,
        HistoryLineWrapPolicy::PreWrap,
    );
    assert!(narrow.iter().all(|line| line.image.is_none()));
}

#[test]
fn images_taller_than_the_screen_preserve_following_text_and_composer_position() {
    use crate::insert_history::InsertHistoryMode;
    use crate::insert_history::insert_history_hyperlink_lines_with_mode_and_wrap_policy;
    use crate::test_backend::VT100Backend;
    use ratatui::layout::Position;
    use ratatui::layout::Rect;
    use ratatui::layout::Size;

    let preview = ImagePreview::from_bytes(&create_png(/*width*/ 640, /*height*/ 400)).unwrap();
    for mode in [InsertHistoryMode::Standard, InsertHistoryMode::FullScreen] {
        let mut terminal = crate::custom_terminal::Terminal::with_options(VT100Backend::new(
            /*width*/ 80, /*height*/ 12,
        ))
        .unwrap();
        terminal.set_viewport_area(Rect::new(
            /*x*/ 0, /*y*/ 10, /*width*/ 80, /*height*/ 2,
        ));
        terminal
            .set_cursor_position(Position::new(/*x*/ 3, /*y*/ 10))
            .unwrap();
        let mut lines = preview.lines(/*width*/ 80);
        lines.push("After the image".into());
        insert_history_hyperlink_lines_with_mode_and_wrap_policy(
            &mut terminal,
            &lines,
            mode,
            HistoryLineWrapPolicy::PreWrap,
            Size::new(/*width*/ 80, /*height*/ 12),
        )
        .unwrap();
        let screen = terminal.backend().vt100().screen();
        assert_eq!(screen.cursor_position(), (10, 3));
        assert_eq!(
            screen.rows(/*start*/ 0, /*width*/ 80).nth(/*n*/ 9).unwrap(),
            "After the image"
        );
    }
}

#[test]
fn generated_image_live_and_replayed_cells_match_and_raw_mode_keeps_only_text() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("generated.png");
    std::fs::write(&path, create_png(/*width*/ 40, /*height*/ 40)).unwrap();
    let create = || {
        new_image_generation_call(
            "image-call".into(),
            "completed",
            Some("Blue pattern".into()),
            Some(AbsolutePathBuf::try_from(path.clone()).unwrap()),
        )
    };
    let live = create();
    let replayed = create();
    assert_eq!(
        live.display_hyperlink_lines(/*width*/ 80),
        replayed.display_hyperlink_lines(/*width*/ 80)
    );
    assert_eq!(live.raw_lines().len(), 3);
    assert!(
        live.display_hyperlink_lines_for_mode(/*width*/ 80, HistoryRenderMode::Raw)
            .iter()
            .all(|line| line.image.is_none())
    );
    let stamped = with_created_at(Box::new(live), Some(1_700_000_000));
    assert_eq!(
        stamped
            .display_hyperlink_lines(/*width*/ 80)
            .iter()
            .filter(|line| line.image.is_some())
            .count(),
        2
    );
}

#[test]
fn graphics_require_a_supported_terminal_outside_a_multiplexer() {
    let mut info = TerminalInfo {
        name: TerminalName::Iterm2,
        term_program: Some("iTerm.app".into()),
        version: Some("3.7.1".into()),
        term: None,
        multiplexer: None,
    };
    assert!(supports_kitty(&info));
    info.version = Some("3.5.0".into());
    assert!(!supports_kitty(&info));
    info.name = TerminalName::Kitty;
    assert!(supports_kitty(&info));
    info.multiplexer = Some(Multiplexer::Tmux { version: None });
    assert!(!supports_kitty(&info));
    info.multiplexer = None;
    info.name = TerminalName::Unknown;
    assert!(!supports_kitty(&info));
}
