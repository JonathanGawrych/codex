//! Restore thread-owned environment selections before connecting to executors.

use crate::config::Config;
use codex_exec_server::EnvironmentManager;
use codex_history::InitialHistory;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_rollout::RolloutItem;
use codex_utils_path_uri::PathUri;

pub(crate) fn restore_thread_environments(
    history: &InitialHistory,
    config: &Config,
    environment_manager: &EnvironmentManager,
) -> Result<Option<Vec<TurnEnvironmentSelection>>> {
    let InitialHistory::Resumed(resumed) = history else {
        return Ok(None);
    };
    // Forked/referenced parent snapshots cannot change this thread's workspace.
    let snapshot = resumed.history.iter().rev().find_map(|item| match item {
        RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(event))
            if event.thread_id == Some(resumed.conversation_id) =>
        {
            Some(&event.thread_settings)
        }
        _ => None,
    });
    let Some(snapshot) = snapshot else {
        return Ok(None);
    };
    let Some(settings) = &snapshot.environments else {
        return Ok(None);
    };
    for environment in settings {
        if environment_manager
            .get_environment(&environment.environment_id)
            .is_none()
        {
            return Err(CodexErr::InvalidRequest(format!(
                "saved thread environment `{}` is not configured in environments.toml",
                environment.environment_id
            )));
        }
    }
    let mut environments = settings
        .iter()
        .cloned()
        .map(TryInto::try_into)
        .collect::<std::result::Result<Vec<TurnEnvironmentSelection>, _>>()
        .map_err(|error| {
            CodexErr::InvalidRequest(format!("invalid persisted environment path: {error}"))
        })?;
    if let Some(primary) = environments.first_mut() {
        // Legacy resume overrides describe only the primary workspace. Preserve all
        // secondary paths, including selections whose executor is currently offline.
        if config.cwd != snapshot.cwd {
            let previous_cwd = primary.cwd.clone();
            primary.cwd = PathUri::from_abs_path(&config.cwd);
            let mut roots = Vec::new();
            for root in &primary.workspace_roots {
                let root = if *root == previous_cwd {
                    &primary.cwd
                } else {
                    root
                };
                if !roots.contains(root) {
                    roots.push(root.clone());
                }
            }
            primary.workspace_roots = roots;
        }
        if config.workspace_roots_explicit {
            primary.workspace_roots = config
                .workspace_roots
                .iter()
                .map(PathUri::from_abs_path)
                .collect();
        }
    }
    Ok(Some(environments))
}

#[cfg(test)]
#[path = "environment_resume_tests.rs"]
mod tests;
