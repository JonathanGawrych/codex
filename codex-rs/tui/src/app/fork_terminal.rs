//! Open user-created session forks in a separate terminal window.

use crate::AppServerTarget;
use crate::RemoteAppServerEndpoint;
use crate::bottom_pane::LocalImageAttachment;
use crate::bottom_pane::MentionBinding;
use crate::chatwidget::UserMessage;
use codex_protocol::ThreadId;
use codex_protocol::user_input::TextElement;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::eyre;
use serde::Deserialize;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

const FORK_DRAFT_ENV: &str = "CODEX_INTERNAL_FORK_DRAFT";
pub(crate) const FORK_DAEMON_SOCKET_ENV: &str = "CODEX_INTERNAL_FORK_DAEMON_SOCKET";
const FORK_REMOTE_AUTH_ENV: &str = "CODEX_INTERNAL_FORK_REMOTE_AUTH";

#[derive(Debug, Deserialize, Serialize)]
struct ForkDraft {
    text: String,
    local_images: Vec<ForkDraftLocalImage>,
    remote_image_urls: Vec<String>,
    text_elements: Vec<TextElement>,
    mention_bindings: Vec<ForkDraftMentionBinding>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ForkDraftLocalImage {
    placeholder: String,
    path: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
struct ForkDraftMentionBinding {
    sigil: char,
    mention: String,
    path: String,
}

impl From<UserMessage> for ForkDraft {
    fn from(message: UserMessage) -> Self {
        Self {
            text: message.text,
            local_images: message
                .local_images
                .into_iter()
                .map(|image| ForkDraftLocalImage {
                    placeholder: image.placeholder,
                    path: image.path,
                })
                .collect(),
            remote_image_urls: message.remote_image_urls,
            text_elements: message.text_elements,
            mention_bindings: message
                .mention_bindings
                .into_iter()
                .map(|binding| ForkDraftMentionBinding {
                    sigil: binding.sigil,
                    mention: binding.mention,
                    path: binding.path,
                })
                .collect(),
        }
    }
}

impl From<ForkDraft> for UserMessage {
    fn from(draft: ForkDraft) -> Self {
        Self {
            text: draft.text,
            local_images: draft
                .local_images
                .into_iter()
                .map(|image| LocalImageAttachment {
                    placeholder: image.placeholder,
                    path: image.path,
                })
                .collect(),
            remote_image_urls: draft.remote_image_urls,
            text_elements: draft.text_elements,
            mention_bindings: draft
                .mention_bindings
                .into_iter()
                .map(|binding| MentionBinding {
                    sigil: binding.sigil,
                    mention: binding.mention,
                    path: binding.path,
                })
                .collect(),
        }
    }
}

pub(super) fn forked_thread_name(
    parent_name: Option<&str>,
    explicit_name: Option<&str>,
    detail: Option<&str>,
) -> String {
    if let Some(name) = explicit_name.map(str::trim).filter(|name| !name.is_empty()) {
        return name.to_string();
    }

    let parent_name = parent_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Session");
    match detail.map(str::trim).filter(|detail| !detail.is_empty()) {
        Some(detail) => format!("{parent_name} ({detail})"),
        None => format!("{parent_name} (fork)"),
    }
}

pub(super) fn take_startup_fork_draft() -> Result<Option<UserMessage>> {
    let Some(path) = std::env::var_os(FORK_DRAFT_ENV).map(PathBuf::from) else {
        return Ok(None);
    };
    read_and_remove_fork_draft(&path).map(Some)
}

pub(super) async fn open_forked_session(
    thread_id: ThreadId,
    profile: Option<&str>,
    app_server_target: &AppServerTarget,
    draft: Option<UserMessage>,
) -> Result<()> {
    #[cfg(test)]
    {
        let _ = (thread_id, profile, app_server_target, draft);
        Ok(())
    }

    #[cfg(not(test))]
    {
        let draft_path = draft.map(write_fork_draft).transpose()?;
        let result = open_forked_session_in_terminal(
            thread_id,
            profile,
            app_server_target,
            draft_path.as_deref(),
        )
        .await;
        if result.is_err()
            && let Some(path) = draft_path
        {
            let _ = fs::remove_file(path);
        }
        result
    }
}

fn write_fork_draft(message: UserMessage) -> Result<PathBuf> {
    let mut file = tempfile::Builder::new()
        .prefix("codex-fork-draft-")
        .suffix(".json")
        .tempfile()
        .wrap_err("failed to create the fork prompt draft file")?;
    serde_json::to_writer(file.as_file_mut(), &ForkDraft::from(message))
        .wrap_err("failed to write the fork prompt draft")?;
    file.as_file_mut()
        .flush()
        .wrap_err("failed to flush the fork prompt draft")?;
    let (_, path) = file
        .keep()
        .map_err(|error| error.error)
        .wrap_err("failed to preserve the fork prompt draft")?;
    Ok(path)
}

fn read_and_remove_fork_draft(path: &Path) -> Result<UserMessage> {
    let contents = fs::read(path)
        .wrap_err_with(|| format!("failed to read fork prompt draft `{}`", path.display()))?;
    fs::remove_file(path)
        .wrap_err_with(|| format!("failed to remove fork prompt draft `{}`", path.display()))?;
    let draft: ForkDraft = serde_json::from_slice(&contents)
        .wrap_err_with(|| format!("failed to parse fork prompt draft `{}`", path.display()))?;
    Ok(draft.into())
}

struct ForkResumeCommand {
    arguments: Vec<String>,
    environment: Vec<(String, String)>,
}

fn fork_resume_command(
    executable: &Path,
    thread_id: ThreadId,
    profile: Option<&str>,
    app_server_target: &AppServerTarget,
    draft_path: Option<&Path>,
) -> Result<ForkResumeCommand> {
    let executable = executable
        .to_str()
        .ok_or_else(|| eyre!("the Codex executable path is not valid UTF-8"))?;
    let mut arguments = vec![executable.to_string()];
    let mut environment = Vec::new();
    if let Some(profile) = profile {
        arguments.extend(["--profile".to_string(), profile.to_string()]);
    }
    match app_server_target {
        AppServerTarget::Embedded => {}
        AppServerTarget::LocalDaemon {
            endpoint: RemoteAppServerEndpoint::UnixSocket { socket_path },
        } => environment.push((
            FORK_DAEMON_SOCKET_ENV.to_string(),
            socket_path.display().to_string(),
        )),
        AppServerTarget::LocalDaemon {
            endpoint: RemoteAppServerEndpoint::WebSocket { websocket_url, .. },
        }
        | AppServerTarget::Remote {
            endpoint: RemoteAppServerEndpoint::WebSocket { websocket_url, .. },
        } => arguments.extend(["--remote".to_string(), websocket_url.clone()]),
        AppServerTarget::Remote {
            endpoint: RemoteAppServerEndpoint::UnixSocket { socket_path },
        } => arguments.extend([
            "--remote".to_string(),
            format!("unix://{}", socket_path.display()),
        ]),
    }
    let remote_auth_token = match app_server_target {
        AppServerTarget::LocalDaemon {
            endpoint: RemoteAppServerEndpoint::WebSocket { auth_token, .. },
        }
        | AppServerTarget::Remote {
            endpoint: RemoteAppServerEndpoint::WebSocket { auth_token, .. },
        } => auth_token.as_ref(),
        _ => None,
    };
    if let Some(auth_token) = remote_auth_token {
        environment.push((FORK_REMOTE_AUTH_ENV.to_string(), auth_token.clone()));
        arguments.extend([
            "--remote-auth-token-env".to_string(),
            FORK_REMOTE_AUTH_ENV.to_string(),
        ]);
    }
    arguments.extend(["resume".to_string(), thread_id.to_string()]);
    if let Some(path) = draft_path {
        environment.push((FORK_DRAFT_ENV.to_string(), path.display().to_string()));
    }
    Ok(ForkResumeCommand {
        arguments,
        environment,
    })
}

#[cfg(all(not(test), target_os = "macos"))]
async fn open_forked_session_in_terminal(
    thread_id: ThreadId,
    profile: Option<&str>,
    app_server_target: &AppServerTarget,
    draft_path: Option<&Path>,
) -> Result<()> {
    let executable = std::env::current_exe().wrap_err("failed to locate the Codex executable")?;
    let launch = fork_resume_command(
        &executable,
        thread_id,
        profile,
        app_server_target,
        draft_path,
    )?;
    let mut arguments = Vec::with_capacity(launch.environment.len() + launch.arguments.len() + 1);
    if !launch.environment.is_empty() {
        arguments.push("env".to_string());
        arguments.extend(
            launch
                .environment
                .into_iter()
                .map(|(name, value)| format!("{name}={value}")),
        );
    }
    arguments.extend(launch.arguments);
    let command = shlex::try_join(arguments.iter().map(String::as_str))
        .map_err(|error| eyre!("failed to quote the fork resume command: {error}"))?;

    let use_apple_terminal = std::env::var("TERM_PROGRAM").as_deref() == Ok("Apple_Terminal");
    let (application, create_window) = if use_apple_terminal {
        ("Terminal", "do script (item 1 of argv)")
    } else {
        (
            "iTerm2",
            "create window with default profile command (item 1 of argv)",
        )
    };
    let status = tokio::process::Command::new("/usr/bin/osascript")
        .args(["-e", "on run argv"])
        .args(["-e", &format!("tell application \"{application}\"")])
        .args(["-e", create_window])
        .args(["-e", "activate"])
        .args(["-e", "end tell"])
        .args(["-e", "end run"])
        .arg(command)
        .status()
        .await
        .wrap_err_with(|| format!("failed to start {application}"))?;
    if !status.success() {
        return Err(eyre!("{application} exited with status {status}"));
    }
    Ok(())
}

#[cfg(all(not(test), target_os = "linux"))]
async fn open_forked_session_in_terminal(
    thread_id: ThreadId,
    profile: Option<&str>,
    app_server_target: &AppServerTarget,
    draft_path: Option<&Path>,
) -> Result<()> {
    let executable = std::env::current_exe().wrap_err("failed to locate the Codex executable")?;
    let launch = fork_resume_command(
        &executable,
        thread_id,
        profile,
        app_server_target,
        draft_path,
    )?;
    let configured_terminal = std::env::var("TERMINAL").ok();
    let mut terminal = configured_terminal
        .as_deref()
        .and_then(shlex::split)
        .filter(|parts| !parts.is_empty())
        .unwrap_or_else(|| vec!["x-terminal-emulator".to_string()]);
    let program = terminal.remove(0);
    let mut command = tokio::process::Command::new(program);
    command.args(terminal).arg("-e");
    command.args(launch.arguments).envs(launch.environment);
    command
        .spawn()
        .wrap_err("failed to start a new terminal window")?;
    Ok(())
}

#[cfg(all(not(test), target_os = "windows"))]
async fn open_forked_session_in_terminal(
    thread_id: ThreadId,
    profile: Option<&str>,
    app_server_target: &AppServerTarget,
    draft_path: Option<&Path>,
) -> Result<()> {
    let executable = std::env::current_exe().wrap_err("failed to locate the Codex executable")?;
    let launch = fork_resume_command(
        &executable,
        thread_id,
        profile,
        app_server_target,
        draft_path,
    )?;
    let mut command = tokio::process::Command::new("cmd.exe");
    command.args(["/D", "/C", "start", ""]);
    command.args(launch.arguments).envs(launch.environment);
    command
        .spawn()
        .wrap_err("failed to start a new terminal window")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::user_input::ByteRange;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;

    #[test]
    fn derived_fork_names_append_context() {
        assert_eq!(
            forked_thread_name(Some("Status line"), None, None),
            "Status line (fork)"
        );
        assert_eq!(
            forked_thread_name(Some("Status line"), None, Some("fork for prompt edit")),
            "Status line (fork for prompt edit)"
        );
        assert_eq!(
            forked_thread_name(Some("Status line"), Some("Try another color"), None),
            "Try another color"
        );
        assert_eq!(forked_thread_name(None, None, None), "Session (fork)");
    }

    #[test]
    fn fork_draft_round_trip_preserves_structured_input() -> Result<()> {
        let message = UserMessage {
            text: "compare @app and [Image #1]".to_string(),
            local_images: vec![LocalImageAttachment {
                placeholder: "[Image #1]".to_string(),
                path: PathBuf::from("/tmp/image.png"),
            }],
            remote_image_urls: vec!["data:image/png;base64,abc".to_string()],
            text_elements: vec![TextElement::new(ByteRange { start: 8, end: 12 }, None)],
            mention_bindings: vec![MentionBinding {
                sigil: '@',
                mention: "app".to_string(),
                path: "app://example".to_string(),
            }],
        };
        let path = write_fork_draft(message.clone())?;

        let restored = read_and_remove_fork_draft(&path)?;

        assert_eq!(restored, message);
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn fork_resume_command_preserves_profile_daemon_and_draft() -> Result<()> {
        let thread_id = ThreadId::from_string("019c2d47-4935-7423-a190-05691f566092")?;
        let executable = Path::new("/tmp/codex");
        let draft = Path::new("/tmp/fork-draft.json");
        let app_server_target = AppServerTarget::LocalDaemon {
            endpoint: RemoteAppServerEndpoint::UnixSocket {
                socket_path: AbsolutePathBuf::from_absolute_path("/tmp/codex.sock")?,
            },
        };

        let command = fork_resume_command(
            executable,
            thread_id,
            Some("tibbit"),
            &app_server_target,
            Some(draft),
        )?;

        assert_eq!(
            command.arguments,
            vec![
                "/tmp/codex",
                "--profile",
                "tibbit",
                "resume",
                "019c2d47-4935-7423-a190-05691f566092",
            ]
        );
        assert_eq!(
            command.environment,
            vec![
                (
                    FORK_DAEMON_SOCKET_ENV.to_string(),
                    "/tmp/codex.sock".to_string(),
                ),
                (
                    FORK_DRAFT_ENV.to_string(),
                    "/tmp/fork-draft.json".to_string(),
                ),
            ]
        );
        Ok(())
    }
}
