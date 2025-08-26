use crate::LightningNode;
use axum::{
    Router,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::Json,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenChannelRequest {
    peer_node_id: String,
    capacity: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SendPaymentRequest {
    amount: u64,
}

#[derive(Debug, Serialize)]
pub struct NodeInfo {
    node_id: String,
    public_key: String,
    bitcoin_address: String,
    connected_peers: Vec<String>, // This will be empty since we don't have P2P access
}

pub struct ApiServer {
    node: LightningNode,
    // Optional: Add a channel to communicate with P2P node
    p2p_sender: Option<mpsc::UnboundedSender<crate::p2p::P2PMessage>>,
}

impl ApiServer {
    pub fn new(node: LightningNode) -> Self {
        Self {
            node,
            p2p_sender: None,
        }
    }

    // Optional: Method to set P2P sender for communication
    pub fn with_p2p_sender(
        mut self,
        sender: mpsc::UnboundedSender<crate::p2p::P2PMessage>,
    ) -> Self {
        self.p2p_sender = Some(sender);
        self
    }

    pub async fn start(&self, addr: &str) -> anyhow::Result<()> {
        let app = Router::new()
            .route("/api/node/info", get(get_node_info))
            .route("/api/channels", get(get_channels))
            .route("/api/channels", post(open_channel))
            .route("/api/channels/:id/payments", post(send_payment))
            .route("/api/channels/:id/payments", get(get_payments))
            .route("/api/channels/:id/close", post(close_channel))
            .route("/ws", get(websocket_handler))
            .layer(CorsLayer::permissive())
            .with_state(self.node.clone());

        let listener = tokio::net::TcpListener::bind(addr).await?;
        println!("API server listening on {}", addr);
        axum::serve(listener, app).await?;
        Ok(())
    }
}

async fn get_node_info(State(node): State<LightningNode>) -> Json<NodeInfo> {
    // Since we don't have access to P2P node, connected_peers will be empty
    // In a full implementation, you'd need a way to query P2P status
    let connected_peers = Vec::new();

    Json(NodeInfo {
        node_id: node.node_id.clone(),
        public_key: hex::encode(node.key_manager.get_public_key().serialize()),
        bitcoin_address: node.key_manager.get_bitcoin_address(),
        connected_peers,
    })
}

async fn get_channels(
    State(node): State<LightningNode>,
) -> Json<Vec<crate::channel::PaymentChannel>> {
    let channel_manager = node.channel_manager.read().await;
    Json(
        channel_manager
            .get_all_channels()
            .into_iter()
            .cloned()
            .collect(),
    )
}

async fn open_channel(
    State(node): State<LightningNode>,
    Json(req): Json<OpenChannelRequest>,
) -> Result<Json<crate::channel::PaymentChannel>, StatusCode> {
    let mut channel_manager = node.channel_manager.write().await;

    match channel_manager
        .open_channel(req.peer_node_id, req.capacity)
        .await
    {
        Ok(channel) => {
            // Note: P2P broadcasting is removed since we don't have access to P2P node
            // In a full implementation, you'd send a message via a channel to the P2P task
            println!(
                "Channel opened: {} (P2P broadcast not implemented)",
                channel.id
            );

            Ok(Json(channel))
        }
        Err(e) => {
            eprintln!("Failed to open channel: {}", e);
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

async fn send_payment(
    Path(channel_id): Path<String>,
    State(node): State<LightningNode>,
    Json(req): Json<SendPaymentRequest>,
) -> Result<Json<crate::channel::PaymentRecord>, StatusCode> {
    let mut channel_manager = node.channel_manager.write().await;

    match channel_manager.send_payment(&channel_id, req.amount).await {
        Ok(payment) => {
            // Note: P2P broadcasting is removed since we don't have access to P2P node
            // In a full implementation, you'd send a message via a channel to the P2P task
            println!(
                "Payment sent: {} sats on channel {} (P2P broadcast not implemented)",
                req.amount, channel_id
            );

            Ok(Json(payment))
        }
        Err(e) => {
            eprintln!("Failed to send payment: {}", e);
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

async fn get_payments(
    Path(channel_id): Path<String>,
    State(node): State<LightningNode>,
) -> Result<Json<Vec<crate::channel::PaymentRecord>>, StatusCode> {
    let channel_manager = node.channel_manager.read().await;

    match channel_manager.get_channel_payments(&channel_id).await {
        Ok(payments) => Ok(Json(payments)),
        Err(e) => {
            eprintln!("Failed to get payments: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn close_channel(
    Path(channel_id): Path<String>,
    State(node): State<LightningNode>,
) -> Result<StatusCode, StatusCode> {
    let mut channel_manager = node.channel_manager.write().await;

    match channel_manager.close_channel(&channel_id).await {
        Ok(_) => {
            // Note: P2P broadcasting is removed since we don't have access to P2P node
            // In a full implementation, you'd send a message via a channel to the P2P task
            println!(
                "Channel closed: {} (P2P broadcast not implemented)",
                channel_id
            );

            Ok(StatusCode::OK)
        }
        Err(e) => {
            eprintln!("Failed to close channel: {}", e);
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(_node): State<LightningNode>,
) -> axum::response::Response {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    // WebSocket handler for real-time updates
    while let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            match msg {
                Message::Text(text) => {
                    println!("Received WebSocket message: {}", text);
                    // Echo back for now
                    if socket
                        .send(Message::Text(format!("Echo: {}", text)))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        } else {
            break;
        }
    }
}
