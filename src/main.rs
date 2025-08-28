use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::LocalSet;
use tracing::{error, info, warn};
use tracing_subscriber::fmt::init;

mod api;
mod channel;
mod crypto;
mod p2p;
mod storage;

use api::ApiServer;
use channel::ChannelManager;
use crypto::KeyManager;
use p2p::P2PNode;
use storage::Database;

#[derive(Clone)]
pub struct LightningNode {
    pub node_id: String,
    pub key_manager: Arc<KeyManager>,
    pub channel_manager: Arc<RwLock<ChannelManager>>,
    pub database: Arc<Database>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init();
    info!("Starting Lightning Network Offline Node");

    // Create a LocalSet to run local tasks
    let local = LocalSet::new();

    local
        .run_until(async {
            // Read configuration from environment variables
            let database_url = env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:./data/lightning.db".to_string());

            let api_port = env::var("PORT").unwrap_or_else(|_| "3000".to_string());

            let p2p_port = env::var("P2P_PORT").unwrap_or_else(|_| "4001".to_string());

            info!("Using database: {}", database_url);
            info!("API server will bind to: 127.0.0.1:{}", api_port);

            let database = match Database::new(&database_url).await {
                Ok(db) => Arc::new(db),
                Err(e) => {
                    error!("Failed to initialize database: {}", e);
                    return Err(e);
                }
            };

            if let Err(e) = database.migrate().await {
                error!("Failed to migrate database: {}", e);
                return Err(e);
            }

            // Initialize key manager
            let key_manager = match KeyManager::new() {
                Ok(km) => Arc::new(km),
                Err(e) => {
                    error!("Failed to initialize key manager: {}", e);
                    return Err(e);
                }
            };

            let node_id = key_manager.get_node_id();
            info!("Node ID: {}", node_id);

            // Initialize P2P node wrapped in Arc<RwLock> so it can be shared
            let p2p_node = Arc::new(RwLock::new(P2PNode::new(key_manager.clone()).await?));

            // Initialize channel manager
            let channel_manager =
                match ChannelManager::new(key_manager.clone(), database.clone()).await {
                    Ok(cm) => Arc::new(RwLock::new(cm)),
                    Err(e) => {
                        error!("Failed to initialize channel manager: {}", e);
                        return Err(e);
                    }
                };

            let lightning_node = LightningNode {
                node_id,
                key_manager,
                channel_manager,
                database,
            };

            // Start API server with configured port
            let api_server = ApiServer::new(lightning_node.clone());
            let api_address = format!("127.0.0.1:{}", api_port);
            let api_handle = tokio::task::spawn_local(async move {
                match api_server.start(&api_address).await {
                    Ok(_) => info!("API server stopped gracefully"),
                    Err(e) => error!("API server error: {}", e),
                }
            });

            // Start P2P networking in its own task with cloned reference
            let p2p_for_task = p2p_node.clone();
            let p2p_handle = tokio::task::spawn_local(async move {
                let mut p2p = p2p_for_task.write().await;
                match p2p.start_listening().await {
                    Ok(_) => info!("P2P node stopped gracefully"),
                    Err(e) => error!("P2P node error: {}", e),
                }
            });

            // Wait for all services
            tokio::select! {
                result = api_handle => {
                    match result {
                        Ok(_) => warn!("API server task completed"),
                        Err(e) => error!("API server task failed: {}", e),
                    }
                },
                result = p2p_handle => {
                    match result {
                        Ok(_) => warn!("P2P node task completed"),
                        Err(e) => error!("P2P node task failed: {}", e),
                    }
                },
                _ = tokio::signal::ctrl_c() => info!("Received shutdown signal"),
            }

            info!("Lightning node shutting down");
            Ok(())
        })
        .await
}
