use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn loopback_snapshot_records_the_route_without_contacting_a_server() {
    let address: SocketAddr = "127.0.0.1:443".parse().unwrap();
    assert_eq!(
        read_network_state("127.0.0.1", /*port*/ 443).await,
        Ok(BTreeMap::from([(address, Ok(address.ip()))]))
    );
}

#[tokio::test]
async fn dns_or_source_address_change_wakes_reconnect() {
    let address: SocketAddr = "127.0.0.1:443".parse().unwrap();
    for previous in [
        Err(io::ErrorKind::NotFound),
        Ok(BTreeMap::from([(
            "127.0.0.2:443".parse().unwrap(),
            Ok(address.ip()),
        )])),
        Ok(BTreeMap::from([(
            address,
            Ok("127.0.0.2".parse().unwrap()),
        )])),
    ] {
        let mut network = NetworkChanges {
            host: Some(("127.0.0.1".into(), 443)),
            previous: Some(previous),
        };
        tokio::time::timeout(Duration::from_secs(/*secs*/ 1), network.changed())
            .await
            .unwrap();
        assert_eq!(
            network.previous,
            Some(Ok(BTreeMap::from([(address, Ok(address.ip()))])))
        );
    }
}

#[tokio::test]
async fn unchanged_network_does_not_trigger_retries() {
    let mut network = NetworkChanges::new(&RemoteAppServerEndpoint::WebSocket {
        websocket_url: "ws://127.0.0.1:443".into(),
        auth_token: None,
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(/*millis*/ 50), network.changed())
            .await
            .is_err()
    );
    assert!(network.previous.is_some());
}

#[tokio::test(start_paused = true)]
async fn unix_socket_recovery_uses_the_retry_timer() {
    let mut network = NetworkChanges {
        host: None,
        previous: None,
    };
    assert!(
        tokio::time::timeout(Duration::from_secs(/*secs*/ 30), network.changed())
            .await
            .is_err()
    );
}
