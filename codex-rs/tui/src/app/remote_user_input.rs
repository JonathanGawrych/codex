//! Converts client-local media into portable input before remote App Server submission.

use crate::app_command::AppCommand;
use crate::app_server_session::AppServerSession;
use codex_app_server_protocol::UserInput;
use codex_protocol::models::snapshot_local_user_input;
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("could not prepare a local attachment for the remote App Server: {source}")]
pub(super) struct RemoteUserInputPreparationError {
    #[source]
    source: io::Error,
}

pub(super) async fn prepare_remote_user_turn(
    app_server: &AppServerSession,
    operation: &mut AppCommand,
) -> Result<(), RemoteUserInputPreparationError> {
    if !app_server.uses_remote_workspace() {
        return Ok(());
    }
    let AppCommand::UserTurn { items, .. } = operation else {
        return Ok(());
    };
    if !items.iter().any(|item| {
        matches!(
            item,
            UserInput::LocalImage { .. } | UserInput::LocalAudio { .. }
        )
    }) {
        return Ok(());
    }

    let items_to_prepare = items.clone();
    let prepared_items = tokio::task::spawn_blocking(move || {
        items_to_prepare
            .into_iter()
            .map(|item| {
                let mut item = item.into_core();
                snapshot_local_user_input(&mut item)?;
                Ok(UserInput::from(item))
            })
            .collect::<io::Result<Vec<_>>>()
    })
    .await
    .map_err(|error| RemoteUserInputPreparationError {
        source: io::Error::other(error),
    })?
    .map_err(|source| RemoteUserInputPreparationError { source })?;
    *items = prepared_items;
    Ok(())
}
