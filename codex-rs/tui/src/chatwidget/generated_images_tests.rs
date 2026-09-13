use super::*;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;

const TINY_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=";

#[test]
fn copies_remote_generated_image_into_local_codex_home() {
    let codex_home = tempfile::tempdir().expect("temporary Codex home");
    let thread_id = ThreadId::new();

    let path = local_generated_image_path(
        &codex_home.path().abs(),
        Some(thread_id),
        "call/1",
        TINY_PNG_BASE64,
        Some(AbsolutePathBuf::from_absolute_path("/remote/image.png").unwrap()),
    )
    .expect("copy generated image")
    .expect("local generated image path");

    assert_eq!(
        path.as_path().parent(),
        Some(
            codex_home
                .path()
                .join("generated_images")
                .join(thread_id.to_string())
                .as_path()
        )
    );
    assert!(path.as_path().file_name().is_some_and(|name| {
        name.to_string_lossy().starts_with("call_1-") && name.to_string_lossy().ends_with(".png")
    }));
    assert_eq!(
        std::fs::read(path).expect("read copied image"),
        BASE64_STANDARD.decode(TINY_PNG_BASE64).unwrap()
    );
}

#[test]
fn preserves_accessible_generated_image_path() {
    let codex_home = tempfile::tempdir().expect("temporary Codex home");
    let image = tempfile::NamedTempFile::new().expect("temporary image");
    let image_path = AbsolutePathBuf::from_absolute_path(image.path()).unwrap();

    assert_eq!(
        local_generated_image_path(
            &codex_home.path().abs(),
            Some(ThreadId::new()),
            "call-1",
            "invalid base64 is not inspected",
            Some(image_path.clone()),
        )
        .unwrap(),
        Some(image_path)
    );
}

#[test]
fn rejects_non_png_remote_generated_image() {
    let codex_home = tempfile::tempdir().expect("temporary Codex home");
    let result = BASE64_STANDARD.encode(b"not a png");

    let error = local_generated_image_path(
        &codex_home.path().abs(),
        Some(ThreadId::new()),
        "call-1",
        &result,
        /*saved_path*/ None,
    )
    .unwrap_err();

    assert_eq!(error.to_string(), "generated image is not a PNG file");
}
