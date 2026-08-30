use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use codex_protocol::ThreadId;

use crate::update_action::source_checkout_root;
use crate::update_versions::latest_stable_release;

const SOURCE_UPDATE_MARKER_DIR: &str = "codex-source-update";

pub fn complete_source_update(thread_id: &str) -> anyhow::Result<()> {
    let thread_id = ThreadId::from_string(thread_id).context("invalid source update thread ID")?;
    let checkout_root = source_checkout_root()
        .context("this Codex binary was not built in a recognized source checkout")?;
    complete_source_update_in(checkout_root, thread_id)
}

pub fn install_source_update() -> anyhow::Result<()> {
    let checkout_root = source_checkout_root()
        .context("this Codex binary was not built in a recognized source checkout")?;
    install_source_package(checkout_root)
}

fn complete_source_update_in(checkout_root: &Path, thread_id: ThreadId) -> anyhow::Result<()> {
    validate_source_update(checkout_root)?;
    mark_source_update_complete(checkout_root, thread_id)
}

fn validate_source_update(checkout_root: &Path) -> anyhow::Result<()> {
    run_git(checkout_root, &["symbolic-ref", "--quiet", "HEAD"])
        .context("the source checkout is not on a branch")?;

    let status = run_git(
        checkout_root,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?;
    if !status.trim().is_empty() {
        anyhow::bail!("the source checkout has uncommitted changes:\n{status}");
    }

    let tags = run_git(checkout_root, &["tag", "--list", "rust-v*"])?;
    let latest_release = latest_stable_release(&tags)
        .context("no stable rust-vMAJOR.MINOR.PATCH release tag is available")?;
    let release_commit = format!("{}^{{commit}}", latest_release.tag);
    run_git(checkout_root, &["rev-parse", "--verify", &release_commit])
        .with_context(|| format!("{} is not available", latest_release.tag))?;
    run_git(
        checkout_root,
        &[
            "merge-base",
            "--is-ancestor",
            latest_release.tag.as_str(),
            "HEAD",
        ],
    )
    .with_context(|| {
        format!(
            "the current branch is not rebased onto {}",
            latest_release.tag
        )
    })?;

    Ok(())
}

fn install_source_package(checkout_root: &Path) -> anyhow::Result<()> {
    let installer = checkout_root.join("scripts/install-from-source.sh");
    let status = Command::new(&installer)
        .arg("--skip-build")
        .arg("--after-tui-shutdown")
        .status()
        .with_context(|| format!("failed to run {}", installer.display()))?;
    if !status.success() {
        anyhow::bail!(
            "source package installer {} failed with status {status}",
            installer.display()
        );
    }
    Ok(())
}

fn mark_source_update_complete(checkout_root: &Path, thread_id: ThreadId) -> anyhow::Result<()> {
    let marker_path = source_update_marker_path(checkout_root, thread_id);
    let marker_parent = marker_path
        .parent()
        .context("source update marker path has no parent")?;
    fs::create_dir_all(marker_parent)?;
    fs::write(&marker_path, format!("{thread_id}\n"))?;
    Ok(())
}

pub(crate) fn take_source_update_completion(
    checkout_root: &Path,
    thread_id: ThreadId,
) -> Result<bool, String> {
    let marker_path = source_update_marker_path(checkout_root, thread_id);
    let contents = match fs::read_to_string(&marker_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "failed to read source update marker {}: {error}",
                marker_path.display()
            ));
        }
    };
    if contents.trim() != thread_id.to_string() {
        return Err(format!(
            "source update marker {} does not match thread {thread_id}",
            marker_path.display()
        ));
    }
    fs::remove_file(&marker_path).map_err(|error| {
        format!(
            "failed to remove source update marker {}: {error}",
            marker_path.display()
        )
    })?;
    Ok(true)
}

fn run_git(checkout_root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(checkout_root)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git {} failed with status {}: {}",
            args.join(" "),
            output.status,
            stderr.trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn source_update_marker_path(checkout_root: &Path, thread_id: ThreadId) -> PathBuf {
    checkout_root
        .join("codex-rs/target")
        .join(SOURCE_UPDATE_MARKER_DIR)
        .join(format!("{thread_id}.ready"))
}

#[cfg(test)]
#[path = "source_update_tests.rs"]
mod tests;
