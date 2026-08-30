//! Configurable command-backed status-line rendering.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use codex_ansi_escape::ansi_escape_line;
use codex_config::types::StatusLineCommandConfig;
use ratatui::text::Line;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Serialize)]
pub(crate) struct StatusLineCommandPayload {
    pub(crate) schema_version: u32,
    pub(crate) agent: StatusLineAgent,
    pub(crate) update: Option<StatusLineUpdate>,
    pub(crate) session_id: Option<String>,
    pub(crate) cwd: String,
    pub(crate) workspace: StatusLineWorkspace,
    pub(crate) model: StatusLineModel,
    pub(crate) service_tier: Option<String>,
    pub(crate) profile: Option<String>,
    pub(crate) personality: Option<StatusLinePersonality>,
    pub(crate) context_window: StatusLineContextWindow,
    pub(crate) rate_limits: StatusLineRateLimits,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusLineAgent {
    pub(crate) name: String,
    pub(crate) version: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusLineUpdate {
    pub(crate) latest_version: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusLineWorkspace {
    pub(crate) current_dir: String,
    pub(crate) project_dir: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusLineModel {
    pub(crate) id: String,
    pub(crate) display_name: String,
    pub(crate) reasoning_effort: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusLinePersonality {
    pub(crate) id: String,
    pub(crate) display_name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusLineContextWindow {
    pub(crate) used_percentage: Option<i64>,
    pub(crate) remaining_percentage: Option<i64>,
    pub(crate) size: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusLineRateLimits {
    pub(crate) five_hour: Option<StatusLineRateLimitWindow>,
    pub(crate) seven_day: Option<StatusLineRateLimitWindow>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusLineRateLimitWindow {
    pub(crate) used_percentage: f64,
    pub(crate) resets_at: Option<i64>,
    pub(crate) window_minutes: Option<i64>,
}

pub(crate) fn serialize_status_line_payload(
    payload: &StatusLineCommandPayload,
) -> Result<String, String> {
    serde_json::to_string(payload)
        .map_err(|error| format!("failed to serialize status-line command input: {error}"))
}

pub(crate) async fn run_status_line_command(
    config: StatusLineCommandConfig,
    payload: String,
    cwd: &Path,
) -> Result<Option<Line<'static>>, String> {
    let timeout_duration = Duration::from_millis(config.timeout_ms.max(1));
    let mut command = status_line_shell_command(&config.command);
    command
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let command_run = async move {
        let mut child = command
            .spawn()
            .map_err(|error| format!("failed to start status-line command: {error}"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "status-line command stdin was not available".to_string())?;
        stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|error| format!("failed to write status-line command input: {error}"))?;
        stdin
            .shutdown()
            .await
            .map_err(|error| format!("failed to close status-line command input: {error}"))?;
        drop(stdin);

        child
            .wait_with_output()
            .await
            .map_err(|error| format!("failed to read status-line command output: {error}"))
    };

    let output = tokio::time::timeout(timeout_duration, command_run)
        .await
        .map_err(|_| {
            format!(
                "status-line command timed out after {} ms",
                config.timeout_ms.max(1)
            )
        })??;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            return Err(format!(
                "status-line command exited with status {}",
                output.status
            ));
        }
        let stderr = stderr.chars().take(2_000).collect::<String>();
        return Err(format!(
            "status-line command exited with status {}: {stderr}",
            output.status
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or_default();
    if first_line.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(ansi_escape_line(first_line)))
}

#[cfg(unix)]
fn status_line_shell_command(command: &str) -> Command {
    let mut shell = Command::new("/bin/sh");
    shell.arg("-c").arg(command);
    shell
}

#[cfg(windows)]
fn status_line_shell_command(command: &str) -> Command {
    let mut shell = Command::new("cmd.exe");
    shell.arg("/C").arg(command);
    shell
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(command: &str) -> StatusLineCommandConfig {
        StatusLineCommandConfig {
            command: command.to_string(),
            timeout_ms: 1_000,
            refresh_interval_seconds: 60,
        }
    }

    #[test]
    fn payload_matches_claude_status_line_fields() {
        let payload = StatusLineCommandPayload {
            schema_version: 1,
            agent: StatusLineAgent {
                name: "codex".to_string(),
                version: "0.149.1".to_string(),
            },
            update: Some(StatusLineUpdate {
                latest_version: "0.150.1".to_string(),
            }),
            session_id: Some("019c-test".to_string()),
            cwd: "/Volumes/code/work/project".to_string(),
            workspace: StatusLineWorkspace {
                current_dir: "/Volumes/code/work/project".to_string(),
                project_dir: Some("/Volumes/code/work/project".to_string()),
            },
            model: StatusLineModel {
                id: "gpt-5.6-sol".to_string(),
                display_name: "gpt-5.6-sol".to_string(),
                reasoning_effort: Some("xhigh".to_string()),
            },
            service_tier: Some("fast".to_string()),
            profile: Some("tibbit".to_string()),
            personality: Some(StatusLinePersonality {
                id: "pragmatic".to_string(),
                display_name: "Pragmatic".to_string(),
            }),
            context_window: StatusLineContextWindow {
                used_percentage: Some(12),
                remaining_percentage: Some(88),
                size: Some(272_000),
            },
            rate_limits: StatusLineRateLimits {
                five_hour: Some(StatusLineRateLimitWindow {
                    used_percentage: 23.0,
                    resets_at: Some(1_800_000_000),
                    window_minutes: Some(300),
                }),
                seven_day: Some(StatusLineRateLimitWindow {
                    used_percentage: 2.0,
                    resets_at: Some(1_800_500_000),
                    window_minutes: Some(10_080),
                }),
            },
        };

        let json = serde_json::to_string_pretty(&payload).expect("payload should serialize");
        insta::assert_snapshot!(json, @r#"
        {
          "schema_version": 1,
          "agent": {
            "name": "codex",
            "version": "0.149.1"
          },
          "update": {
            "latest_version": "0.150.1"
          },
          "session_id": "019c-test",
          "cwd": "/Volumes/code/work/project",
          "workspace": {
            "current_dir": "/Volumes/code/work/project",
            "project_dir": "/Volumes/code/work/project"
          },
          "model": {
            "id": "gpt-5.6-sol",
            "display_name": "gpt-5.6-sol",
            "reasoning_effort": "xhigh"
          },
          "service_tier": "fast",
          "profile": "tibbit",
          "personality": {
            "id": "pragmatic",
            "display_name": "Pragmatic"
          },
          "context_window": {
            "used_percentage": 12,
            "remaining_percentage": 88,
            "size": 272000
          },
          "rate_limits": {
            "five_hour": {
              "used_percentage": 23.0,
              "resets_at": 1800000000,
              "window_minutes": 300
            },
            "seven_day": {
              "used_percentage": 2.0,
              "resets_at": 1800500000,
              "window_minutes": 10080
            }
          }
        }
        "#);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_receives_json_and_preserves_ansi_styles() {
        let config =
            test_config("grep -q 'gpt-test' && printf '\\033[38;2;48;132;255mCodex\\033[0m\\n'");
        let line = run_status_line_command(
            config,
            r#"{"model":{"display_name":"gpt-test"}}"#.to_string(),
            Path::new("/"),
        )
        .await
        .expect("command should succeed")
        .expect("command should return one line");

        insta::assert_debug_snapshot!(line, @r#"Line::from(Span::from("Codex").fg(Color::Rgb(48, 132, 255)))"#);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_timeout_covers_the_process_lifetime() {
        let mut config = test_config("while :; do :; done");
        config.timeout_ms = 10;

        let error = run_status_line_command(config, "{}".to_string(), Path::new("/"))
            .await
            .expect_err("command should time out");

        assert_eq!(error, "status-line command timed out after 10 ms");
    }
}
