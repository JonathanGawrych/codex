use std::io;
use std::io::BufRead;
use std::io::Write;

use crate::startup_draft::StartupCancelled;
use crate::startup_draft::StartupDraft;
use crate::startup_draft::StartupDraftPump;
use crate::tui::Tui;

pub(crate) const TEST_ALLOW_EMBEDDED_APP_SERVER_ENV_VAR: &str =
    "CODEX_TUI_TEST_ALLOW_EMBEDDED_APP_SERVER";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EmbeddedAppServerReason {
    WorkloadIdentity,
    ManagedWorktree,
    OpenSourceProvider,
    ExecServer,
    StrictConfig,
    HookTrustBypass,
    ConfigOverrides,
    ConfigProfile,
    ConfigLoaderOverrides,
    ManagedDaemonUnavailable,
    ManagedDaemonConnectionFailed,
}

pub(crate) fn is_required() -> bool {
    !cfg!(test) && std::env::var_os(TEST_ALLOW_EMBEDDED_APP_SERVER_ENV_VAR).is_none()
}

impl EmbeddedAppServerReason {
    fn description(self) -> &'static str {
        match self {
            Self::WorkloadIdentity => "workload identity is selected for this invocation",
            Self::ManagedWorktree => "`--worktree` requires a local App Server",
            Self::OpenSourceProvider => "`--oss` requires a local App Server",
            Self::ExecServer => "`CODEX_EXEC_SERVER_URL` is set for this invocation",
            Self::StrictConfig => "`--strict-config` cannot be applied to the managed App Server",
            Self::HookTrustBypass => {
                "`--dangerously-bypass-hook-trust` cannot be applied to the managed App Server"
            }
            Self::ConfigOverrides => {
                "command-line config overrides cannot be applied to the managed App Server"
            }
            Self::ConfigProfile => {
                "the selected config profile cannot be applied to the managed App Server"
            }
            Self::ConfigLoaderOverrides => {
                "config loader overrides cannot be applied to the managed App Server"
            }
            Self::ManagedDaemonUnavailable => {
                "the managed App Server is not running or could not be reached"
            }
            Self::ManagedDaemonConnectionFailed => {
                "the managed App Server connection failed during startup"
            }
        }
    }
}

pub(crate) async fn confirm_with_startup_draft(
    startup_draft: &mut StartupDraft,
    reason: EmbeddedAppServerReason,
) -> io::Result<()> {
    if !is_required() {
        return Ok(());
    }
    startup_draft.flush_pending_events().await?;
    let confirmed = startup_draft
        .tui_mut()
        .with_restored(|| async move { confirm(reason) })
        .await?;
    require_confirmation(confirmed)
}

pub(crate) async fn confirm_with_startup_pump(
    startup_draft: &mut StartupDraftPump,
    tui: &mut Tui,
    reason: EmbeddedAppServerReason,
) -> io::Result<()> {
    if !is_required() {
        return Ok(());
    }
    startup_draft.flush_pending_events(tui).await?;
    let confirmed = tui.with_restored(|| async move { confirm(reason) }).await?;
    require_confirmation(confirmed)
}

fn require_confirmation(confirmed: bool) -> io::Result<()> {
    if confirmed {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Interrupted, StartupCancelled))
    }
}

pub(crate) fn confirm(reason: EmbeddedAppServerReason) -> io::Result<bool> {
    crossterm::terminal::disable_raw_mode()?;
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut output = io::stderr();
    confirm_with_io(reason, &mut input, &mut output)
}

fn confirm_with_io(
    reason: EmbeddedAppServerReason,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> io::Result<bool> {
    writeln!(
        output,
        "Codex cannot use the managed App Server because {}.",
        reason.description()
    )?;
    writeln!(output)?;
    writeln!(
        output,
        "An embedded App Server keeps this session in this terminal. Remote Control and another TUI cannot open the session while it is running."
    )?;
    writeln!(output)?;
    write!(output, "Continue with an embedded App Server? [y/N] ")?;
    output.flush()?;

    let mut response = String::new();
    input.read_line(&mut response)?;
    let response = response.trim();
    Ok(response.eq_ignore_ascii_case("y") || response.eq_ignore_ascii_case("yes"))
}

#[cfg(test)]
#[path = "embedded_app_server_confirmation_tests.rs"]
mod tests;
