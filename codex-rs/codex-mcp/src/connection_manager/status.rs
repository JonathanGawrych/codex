//! Read-only connection state; observing a server must not start it.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use codex_protocol::mcp::McpServerConnectionStatus;

use super::McpConnectionSet;

impl McpConnectionSet {
    pub(crate) async fn has_closed_ready_connections(&self) -> bool {
        for view in self.servers.values() {
            if !view
                .connection
                .client
                .startup_complete
                .load(Ordering::Acquire)
            {
                continue;
            }
            let Ok(client) = view.connection.client.client().await else {
                continue;
            };
            if client.client.is_closed().await {
                return true;
            }
        }
        false
    }

    pub(crate) async fn connection_statuses(&self) -> HashMap<String, McpServerConnectionStatus> {
        use McpServerConnectionStatus as Status;

        let mut statuses = self
            .disabled_servers
            .iter()
            .map(|name| (name.clone(), Status::Disabled))
            .collect::<HashMap<_, _>>();
        for (name, view) in &self.servers {
            let connection = &view.connection;
            let client = &connection.client;
            let status = if connection.startup_is_dormant() && !client.cancel_token.is_cancelled() {
                Status::NotStarted
            } else {
                client.connection_status().await
            };
            statuses.insert(name.clone(), status);
        }
        statuses
    }
}
