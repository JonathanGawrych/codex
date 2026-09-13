//! Persisted environment paths and configuration ownership, without executor credentials.

use crate::protocol::EnvironmentConfigState;
use crate::protocol::TurnEnvironmentSelection;
use codex_utils_path_uri::LegacyAppPathString;
use codex_utils_path_uri::LegacyAppPathStringError;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ThreadEnvironmentSettings {
    pub environment_id: String,
    // Persist native path strings so another host can restore foreign paths while offline.
    pub cwd: LegacyAppPathString,
    pub workspace_roots: Vec<LegacyAppPathString>,
    pub config_source: ThreadEnvironmentConfigSource,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ThreadEnvironmentConfigSource {
    Thread,
    Owner,
}

impl From<&TurnEnvironmentSelection> for ThreadEnvironmentSettings {
    fn from(selection: &TurnEnvironmentSelection) -> Self {
        Self {
            environment_id: selection.environment_id.clone(),
            cwd: selection.cwd.clone().into(),
            workspace_roots: selection
                .workspace_roots
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
            config_source: match selection.config {
                EnvironmentConfigState::FromThread => ThreadEnvironmentConfigSource::Thread,
                EnvironmentConfigState::Pending
                | EnvironmentConfigState::Ready(_)
                | EnvironmentConfigState::Failed(_) => ThreadEnvironmentConfigSource::Owner,
            },
        }
    }
}

impl TryFrom<ThreadEnvironmentSettings> for TurnEnvironmentSelection {
    type Error = LegacyAppPathStringError;

    fn try_from(settings: ThreadEnvironmentSettings) -> Result<Self, Self::Error> {
        Ok(Self {
            environment_id: settings.environment_id,
            cwd: settings.cwd.try_into()?,
            workspace_roots: settings
                .workspace_roots
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
            config: match settings.config_source {
                ThreadEnvironmentConfigSource::Thread => EnvironmentConfigState::FromThread,
                // The attachment owner must supply fresh permissions before tools can run.
                ThreadEnvironmentConfigSource::Owner => EnvironmentConfigState::Pending,
            },
        })
    }
}

#[cfg(test)]
#[path = "environment_settings_tests.rs"]
mod tests;
