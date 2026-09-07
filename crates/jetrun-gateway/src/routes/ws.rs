use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, State, WebSocketUpgrade,
    },
    response::Response,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use uuid::Uuid;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/builds/{id}/logs", get(build_logs_ws))
        .route("/builds/{id}/status", get(build_status_ws))
}

async fn build_logs_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(build_id): Path<Uuid>,
) -> Response {
    ws.on_upgrade(move |socket| handle_log_stream(socket, state, build_id))
}

async fn handle_log_stream(socket: WebSocket, state: AppState, build_id: Uuid) {
    let (mut sender, mut receiver) = socket.split();

    let tx = state.get_or_create_log_channel(build_id);
    let mut rx = tx.subscribe();

    // Forward log messages to the WebSocket client
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Listen for client messages (e.g., unsubscribe)
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                break;
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

async fn build_status_ws(
    ws: WebSocketUpgrade,
    State(_state): State<AppState>,
    Path(_build_id): Path<Uuid>,
) -> Response {
    ws.on_upgrade(|socket| async {
        let (mut _sender, mut receiver) = socket.split();
        // Stub: will stream build status changes
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                break;
            }
        }
    })
}
