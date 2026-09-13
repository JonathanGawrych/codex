use std::io::Write;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use codex_protocol::ThreadId;
use codex_utils_absolute_path::AbsolutePathBuf;
use sha2::Digest;
use sha2::Sha256;

const MAX_GENERATED_IMAGE_BASE64_BYTES: usize = 48 * 1024 * 1024;
const MAX_GENERATED_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub(super) fn local_generated_image_path(
    codex_home: &AbsolutePathBuf,
    thread_id: Option<ThreadId>,
    call_id: &str,
    result: &str,
    saved_path: Option<AbsolutePathBuf>,
) -> Result<Option<AbsolutePathBuf>> {
    if saved_path
        .as_ref()
        .is_some_and(|path| path.as_path().is_file())
    {
        return Ok(saved_path);
    }
    if result.is_empty() {
        return Ok(saved_path);
    }
    if result.len() > MAX_GENERATED_IMAGE_BASE64_BYTES {
        bail!("generated image exceeds the local transfer limit");
    }

    let bytes = BASE64_STANDARD
        .decode(result)
        .context("generated image is not valid base64")?;
    if bytes.len() > MAX_GENERATED_IMAGE_BYTES {
        bail!("generated image exceeds the local file limit");
    }
    if !bytes.starts_with(PNG_SIGNATURE) {
        bail!("generated image is not a PNG file");
    }

    let thread_id = thread_id.map_or_else(|| "unknown-thread".to_string(), |id| id.to_string());
    let directory = codex_home.join("generated_images").join(thread_id);
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("create {}", directory.display()))?;

    let digest = format!("{:x}", Sha256::digest(&bytes));
    let filename = format!("{}-{}.png", sanitize_path_segment(call_id), &digest[..16]);
    let path = directory.join(filename);
    if path.as_path().is_file() {
        return Ok(Some(path));
    }

    let mut temporary = tempfile::NamedTempFile::new_in(&directory)
        .with_context(|| format!("create temporary file in {}", directory.display()))?;
    temporary
        .write_all(&bytes)
        .with_context(|| format!("write generated image to {}", directory.display()))?;
    match temporary.persist_noclobber(&path) {
        Ok(_) => Ok(Some(path)),
        Err(_error) if path.as_path().is_file() => Ok(Some(path)),
        Err(error) => {
            Err(error.error).with_context(|| format!("save generated image to {}", path.display()))
        }
    }
}

fn sanitize_path_segment(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "generated-image".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
#[path = "generated_images_tests.rs"]
mod tests;
