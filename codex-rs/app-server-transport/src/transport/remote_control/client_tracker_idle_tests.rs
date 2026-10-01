use super::*;
use crate::outgoing_message::OutgoingMessage;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerNotificationEnvelope;
use codex_app_server_protocol::ThreadArchivedNotification;
use pretty_assertions::assert_eq;
use serde_json::json;

struct ConnectedClient {
    tracker: ClientTracker,
    connection_id: ConnectionId,
    writer: mpsc::Sender<QueuedOutgoingMessage>,
    transport_events: mpsc::Receiver<TransportEvent>,
    server_events: mpsc::Receiver<QueuedServerEnvelope>,
}

impl ConnectedClient {
    async fn connect() -> Self {
        let (server_event_tx, server_events) = mpsc::channel(CHANNEL_CAPACITY);
        let (transport_event_tx, mut transport_events) = mpsc::channel(CHANNEL_CAPACITY);
        let mut tracker = ClientTracker::new(
            server_event_tx,
            transport_event_tx,
            &CancellationToken::new(),
        );
        tracker
            .handle_message(create_envelope(ClientEvent::ClientMessage {
                message: JSONRPCMessage::Request(JSONRPCRequest {
                    id: RequestId::Integer(1),
                    method: "initialize".to_string(),
                    params: None,
                    trace: None,
                }),
            }))
            .await
            .expect("initialize opens the client");
        let (connection_id, writer) = match transport_events.recv().await {
            Some(TransportEvent::ConnectionOpened {
                connection_id,
                writer,
                ..
            }) => (connection_id, writer),
            event => panic!("expected connection opened, got {event:?}"),
        };
        assert!(matches!(
            transport_events.recv().await,
            Some(TransportEvent::IncomingMessage { .. })
        ));
        Self {
            tracker,
            connection_id,
            writer,
            transport_events,
            server_events,
        }
    }
}

fn create_envelope(event: ClientEvent) -> ClientEnvelope {
    ClientEnvelope {
        event,
        client_id: ClientId("client".to_string()),
        stream_id: Some(StreamId("stream".to_string())),
        seq_id: None,
        cursor: None,
    }
}

#[tokio::test(start_paused = true)]
async fn idle_client_receives_notifications_until_twenty_four_hours() {
    let mut client = ConnectedClient::connect().await;
    tokio::time::advance(Duration::from_secs(24 * 60 * 60 - 1)).await;
    assert_eq!(
        client.tracker.close_expired_clients().await.unwrap(),
        vec![]
    );

    let message = OutgoingMessage::AppServerNotification(ServerNotificationEnvelope {
        notification: ServerNotification::ThreadArchived(ThreadArchivedNotification {
            thread_id: "thread".to_string(),
        }),
        emitted_at_ms: Some(1_234),
    });
    let expected = json!({ "type": "server_message", "message": &message });
    client
        .writer
        .send(QueuedOutgoingMessage::new(message))
        .await
        .unwrap();
    let delivered = client.server_events.recv().await.unwrap();
    assert_eq!(serde_json::to_value(delivered.event).unwrap(), expected);

    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(
        client.tracker.close_expired_clients().await.unwrap(),
        vec![(
            ClientId("client".to_string()),
            StreamId("stream".to_string())
        )]
    );
    assert!(matches!(
        client.transport_events.recv().await,
        Some(TransportEvent::ConnectionClosed { connection_id }) if connection_id == client.connection_id
    ));
}

#[tokio::test(start_paused = true)]
async fn inbound_activity_restarts_the_twenty_four_hour_idle_period() {
    for event in [
        ClientEvent::Ping,
        ClientEvent::ClientMessage {
            message: JSONRPCMessage::Notification(JSONRPCNotification {
                method: "initialized".to_string(),
                params: None,
            }),
        },
    ] {
        let mut client = ConnectedClient::connect().await;
        tokio::time::advance(Duration::from_secs(23 * 60 * 60)).await;
        client
            .tracker
            .handle_message(create_envelope(event))
            .await
            .unwrap();
        tokio::time::advance(Duration::from_secs(60 * 60)).await;
        assert_eq!(
            client.tracker.close_expired_clients().await.unwrap(),
            vec![]
        );

        tokio::time::advance(Duration::from_secs(23 * 60 * 60)).await;
        assert_eq!(
            client.tracker.close_expired_clients().await.unwrap(),
            vec![(
                ClientId("client".to_string()),
                StreamId("stream".to_string())
            )]
        );
        client.tracker.shutdown().await;
    }
}

#[tokio::test(start_paused = true)]
async fn explicit_disconnect_closes_client_before_idle_expiration() {
    let mut client = ConnectedClient::connect().await;
    client
        .tracker
        .handle_message(create_envelope(ClientEvent::ClientClosed))
        .await
        .unwrap();
    assert!(matches!(
        client.transport_events.recv().await,
        Some(TransportEvent::ConnectionClosed { connection_id }) if connection_id == client.connection_id
    ));
    client.writer.closed().await;
    assert_eq!(
        client.tracker.close_expired_clients().await.unwrap(),
        vec![]
    );
}
