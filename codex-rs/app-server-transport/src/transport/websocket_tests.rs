use super::*;
use pretty_assertions::assert_eq;
use tokio::io::duplex;
use tokio::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::client_async;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;

#[tokio::test]
async fn disconnect_sends_service_restart_close_frame() {
    let (server_io, client_io) = duplex(4096);
    let server_handshake = tokio::spawn(accept_async(server_io));
    let (mut client, _) = client_async("ws://localhost/rpc", client_io)
        .await
        .expect("client websocket handshake should succeed");
    let server = server_handshake
        .await
        .expect("server websocket task should join")
        .expect("server websocket handshake should succeed");
    let (server_writer, server_reader) = server.split();
    let (transport_event_tx, mut transport_event_rx) = mpsc::channel(CHANNEL_CAPACITY);
    let connection_task = tokio::spawn(run_websocket_connection(
        server_writer,
        server_reader,
        transport_event_tx,
    ));

    let TransportEvent::ConnectionOpened {
        connection_id,
        disconnect_sender: Some(disconnect_sender),
        ..
    } = transport_event_rx
        .recv()
        .await
        .expect("connection opened event should arrive")
    else {
        panic!("expected websocket connection opened event");
    };
    disconnect_sender.cancel();

    let close_frame = timeout(Duration::from_secs(1), client.next())
        .await
        .expect("service restart close frame should arrive")
        .expect("websocket should produce a frame")
        .expect("close frame should decode");
    assert_eq!(
        close_frame,
        TungsteniteWebSocketMessage::Close(Some(CloseFrame {
            code: CloseCode::Restart,
            reason: "app server restarting".into(),
        }))
    );

    timeout(Duration::from_secs(2), connection_task)
        .await
        .expect("websocket connection task should stop without a peer close reply")
        .expect("websocket connection task should join");
    let TransportEvent::ConnectionClosed {
        connection_id: closed_connection_id,
    } = transport_event_rx
        .recv()
        .await
        .expect("connection closed event should arrive")
    else {
        panic!("expected websocket connection closed event");
    };
    assert_eq!(closed_connection_id, connection_id);
}
