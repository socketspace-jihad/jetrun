//! WebSocket live log streaming — tails per-step log files on disk.
//! Zero NATS overhead: reads directly from the worker's log directory.
//! For multi-machine scaling, swap disk tail for NATS subscription (same WS API).

use std::path::PathBuf;

use axum::{
    extract::{Path, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::Response,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

use super::AppState;

/// WebSocket endpoint: streams live log lines for a build step.
/// Client connects, receives all existing lines, then new lines as they're written.
/// Closes automatically when the build finishes and no more lines arrive.
pub async fn ws_step_logs(
    State(state): State<AppState>,
    Path((build_id, step_id)): Path<(Uuid, Uuid)>,
    ws: WebSocketUpgrade,
) -> Response {
    let log_dir = state.log_dir.clone();
    let store = state.store.clone();

    ws.on_upgrade(move |socket| handle_step_logs(socket, log_dir, store, build_id, step_id))
}

async fn handle_step_logs(
    mut socket: WebSocket,
    log_dir: PathBuf,
    store: std::sync::Arc<dyn jetrun_store::traits::Store>,
    build_id: Uuid,
    step_id: Uuid,
) {
    let log_path = log_dir
        .join("live")
        .join(build_id.to_string())
        .join(format!("{}.log", step_id));

    // Wait for log file to appear (build may not have started this step yet)
    let file = loop {
        match tokio::fs::File::open(&log_path).await {
            Ok(f) => break f,
            Err(_) => {
                // Check if build is still active — if finished, no file will appear
                if let Ok(Some(b)) = store.find_build_by_id(build_id).await {
                    use jetrun_common::models::BuildStatus;
                    match b.status {
                        BuildStatus::Success | BuildStatus::Failed | BuildStatus::Cancelled => {
                            let _ = socket.send(Message::Close(None)).await;
                            return;
                        }
                        _ => {}
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    };

    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut idle_ticks: u32 = 0;

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                // EOF — no new data yet. Check if build is still running.
                idle_ticks += 1;

                // Check build status every ~1s (5 ticks × 200ms)
                if idle_ticks >= 5 {
                    idle_ticks = 0;
                    if let Ok(Some(b)) = store.find_build_by_id(build_id).await {
                        use jetrun_common::models::BuildStatus;
                        match b.status {
                            BuildStatus::Success | BuildStatus::Failed | BuildStatus::Cancelled => {
                                // Drain remaining lines then close
                                loop {
                                    line.clear();
                                    match reader.read_line(&mut line).await {
                                        Ok(0) => break,
                                        Ok(_) => {
                                            let trimmed = line.trim_end();
                                            if !trimmed.is_empty() {
                                                if socket.send(Message::Text(trimmed.to_string().into())).await.is_err() {
                                                    return;
                                                }
                                            }
                                        }
                                        Err(_) => break,
                                    }
                                }
                                let _ = socket.send(Message::Close(None)).await;
                                return;
                            }
                            _ => {}
                        }
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            Ok(_) => {
                idle_ticks = 0;
                let trimmed = line.trim_end();
                if !trimmed.is_empty() {
                    if socket.send(Message::Text(trimmed.to_string().into())).await.is_err() {
                        return; // Client disconnected
                    }
                }
            }
            Err(_) => {
                let _ = socket.send(Message::Close(None)).await;
                return;
            }
        }
    }
}
