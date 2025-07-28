use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Extension,
    },
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use shared::Packet;
use std::net::SocketAddr;
use std::{sync::Arc, time::Instant};
use tokio::sync::broadcast;
use tokio::{sync::Mutex, time::Duration};
use uuid::Uuid;

async fn ws(
    ws: WebSocketUpgrade,
    Extension(tx): Extension<broadcast::Sender<String>>,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| client(socket, tx))
}

async fn client(socket: WebSocket, tx: broadcast::Sender<String>) {
    let id = Uuid::new_v4().to_string();
    let (mut sender, mut receiver) = socket.split();
    let mut rx = tx.subscribe();

    // inactivity timer
    let last_seen = Arc::new(Mutex::new(Instant::now()));

    // fan-out: broadcast -> this client
    let mut send_task = tokio::spawn(async move {
        while let Ok(txt) = rx.recv().await {
            if sender.send(Message::Text(txt)).await.is_err() {
                break;
            }
        }
    });

    // fan-in: this client -> broadcast
    let tx_clone = tx.clone();
    let last_seen_rx = last_seen.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(txt))) = receiver.next().await {
            *last_seen_rx.lock().await = Instant::now();

            if let Ok(mut pkt) = serde_json::from_str::<Packet>(&txt) {
                pkt.id = id.clone();
                let _ = tx_clone.send(serde_json::to_string(&pkt).unwrap());
            }
        }
    });

    // disconnect participants after 30 s of silence
    let watchdog = tokio::spawn({
        let last_seen = last_seen.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60 * 5)).await;
                if Instant::now().duration_since(*last_seen.lock().await)
                    > Duration::from_secs(60 * 5)
                {
                    break;
                }
            }
        }
    });

    tokio::select! {
      _ = (&mut send_task) => recv_task.abort(),
      _ = (&mut recv_task) => send_task.abort(),
      _ = watchdog => {
        send_task.abort();
        recv_task.abort();
      },
    }
}

#[tokio::main]
async fn main() {
    // Use Fly.io's injected $PORT if available, otherwise default to 3000 for local runs.
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let (tx, _) = broadcast::channel::<String>(1_024);
    let app = Router::new().route("/ws", get(ws)).layer(Extension(tx));

    axum::Server::bind(&SocketAddr::from(([0, 0, 0, 0], port)))
        .serve(app.into_make_service())
        .await
        .unwrap();
}
