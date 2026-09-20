use std::io::Cursor;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use image::DynamicImage;
use image::ImageFormat;
use image::ImageReader;
use image::Limits;

const MAX_SOURCE_BASE64_BYTES: usize = 48 * 1024 * 1024;
const MAX_PREVIEW_BASE64_BYTES: usize = 256 * 1024;
const MAX_PREVIEW_DIMENSION: u32 = 192;
const MAX_SOURCE_DIMENSION: u32 = 8192;
const MAX_DECODED_ALLOC_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn generated_image_preview(result: &str) -> String {
    if result.len() > MAX_SOURCE_BASE64_BYTES {
        return String::new();
    }

    let Ok(bytes) = BASE64_STANDARD.decode(result) else {
        return if result.len() <= MAX_PREVIEW_BASE64_BYTES {
            result.to_string()
        } else {
            String::new()
        };
    };
    let mut reader = ImageReader::new(Cursor::new(bytes));
    reader.set_format(ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_DIMENSION);
    limits.max_image_height = Some(MAX_SOURCE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_ALLOC_BYTES);
    reader.limits(limits);
    let Ok(image) = reader.decode() else {
        return if result.len() <= MAX_PREVIEW_BASE64_BYTES {
            result.to_string()
        } else {
            String::new()
        };
    };

    if image.width() <= MAX_PREVIEW_DIMENSION
        && image.height() <= MAX_PREVIEW_DIMENSION
        && result.len() <= MAX_PREVIEW_BASE64_BYTES
    {
        return result.to_string();
    }

    let rgba = image.to_rgba8();
    let preview = image::imageops::thumbnail(&rgba, MAX_PREVIEW_DIMENSION, MAX_PREVIEW_DIMENSION);
    let mut encoded = Cursor::new(Vec::new());
    if DynamicImage::ImageRgba8(preview)
        .write_to(&mut encoded, ImageFormat::Png)
        .is_err()
    {
        return String::new();
    }
    let preview = BASE64_STANDARD.encode(encoded.into_inner());
    if preview.len() > MAX_PREVIEW_BASE64_BYTES {
        return String::new();
    }
    preview
}

#[cfg(test)]
#[path = "mobile_image_preview_tests.rs"]
mod tests;
