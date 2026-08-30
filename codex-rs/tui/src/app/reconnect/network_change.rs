//! Detect changes to the endpoint's DNS answers and locally selected source addresses.
//! Poll only while reconnect is waiting. UDP connect selects a local route without sending
//! application data or requiring the app-server to offer a UDP service.

use std::collections::BTreeMap;
use std::io;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::time::Duration;

use codex_app_server_client::RemoteAppServerEndpoint;
use tokio::net::UdpSocket;
use url::Url;

type NetworkState = Result<BTreeMap<SocketAddr, Result<IpAddr, io::ErrorKind>>, io::ErrorKind>;

pub(super) struct NetworkChanges {
    host: Option<(String, u16)>,
    previous: Option<NetworkState>,
}

impl NetworkChanges {
    pub(super) fn new(endpoint: &RemoteAppServerEndpoint) -> Self {
        let host = match endpoint {
            RemoteAppServerEndpoint::WebSocket { websocket_url, .. } => {
                let url = Url::parse(websocket_url)
                    .unwrap_or_else(|_| unreachable!("validated app-server URL"));
                let host = match url
                    .host()
                    .unwrap_or_else(|| unreachable!("app-server URL has a host"))
                {
                    url::Host::Domain(domain) => domain.to_owned(),
                    url::Host::Ipv4(address) => address.to_string(),
                    url::Host::Ipv6(address) => address.to_string(),
                };
                Some((
                    host,
                    url.port_or_known_default()
                        .unwrap_or_else(|| unreachable!("WebSocket port")),
                ))
            }
            RemoteAppServerEndpoint::UnixSocket { .. } => None,
        };
        Self {
            host,
            previous: None,
        }
    }

    pub(super) async fn changed(&mut self) {
        let Some((host, port)) = &self.host else {
            std::future::pending::<()>().await;
            return;
        };
        loop {
            // DNS and routing failures are network states too. A later success wakes the
            // reconnect wait. Keep only error kinds, never credential-bearing endpoint text.
            let current = tokio::time::timeout(
                Duration::from_secs(/*secs*/ 2),
                read_network_state(host, *port),
            )
            .await
            .unwrap_or(Err(io::ErrorKind::TimedOut));
            let changed = self.previous.as_ref().is_some_and(|old| old != &current);
            self.previous = Some(current);
            if changed {
                return;
            }
            tokio::time::sleep(Duration::from_secs(/*secs*/ 2)).await;
        }
    }
}

async fn read_network_state(host: &str, port: u16) -> NetworkState {
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| error.kind())?;
    let mut routes = BTreeMap::new();
    for address in addresses {
        let source = async {
            let bind_address = match address {
                SocketAddr::V4(_) => "0.0.0.0:0",
                SocketAddr::V6(_) => "[::]:0",
            };
            let socket = UdpSocket::bind(bind_address).await?;
            socket.connect(address).await?;
            socket.local_addr().map(|source| source.ip())
        }
        .await;
        routes.insert(address, source.map_err(|error: io::Error| error.kind()));
    }
    Ok(routes)
}

#[cfg(test)]
#[path = "network_change_tests.rs"]
mod tests;
