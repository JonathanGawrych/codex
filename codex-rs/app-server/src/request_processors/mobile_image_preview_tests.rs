use super::*;
use image::ImageBuffer;
use image::Rgba;
use pretty_assertions::assert_eq;

#[test]
fn preserves_small_generated_image_result() {
    assert_eq!(generated_image_preview("cG5n"), "cG5n");
}

#[test]
fn resizes_large_generated_image_result() {
    let mut state = 0x1234_5678_u32;
    let image = ImageBuffer::from_fn(1024, 1024, |_x, _y| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        Rgba([
            state as u8,
            (state >> 8) as u8,
            (state >> 16) as u8,
            u8::MAX,
        ])
    });
    let mut source = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut source, ImageFormat::Png)
        .expect("encode source image");
    let source = BASE64_STANDARD.encode(source.into_inner());
    assert!(source.len() > MAX_PREVIEW_BASE64_BYTES);

    let preview = generated_image_preview(&source);

    assert!(!preview.is_empty());
    assert!(preview.len() <= MAX_PREVIEW_BASE64_BYTES);
    let preview = BASE64_STANDARD.decode(preview).expect("decode preview");
    let preview = image::load_from_memory_with_format(&preview, ImageFormat::Png)
        .expect("decode preview image");
    assert_eq!((preview.width(), preview.height()), (512, 512));
}

#[test]
fn omits_invalid_large_generated_image_result() {
    assert_eq!(
        generated_image_preview(&"!".repeat(MAX_PREVIEW_BASE64_BYTES + 1)),
        ""
    );
}
