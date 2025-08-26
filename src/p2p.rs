use crate::crypto::KeyManager;
use anyhow::Result;
use futures::StreamExt; // Add this import for select_next_some
use libp2p::{
    Multiaddr, PeerId, Swarm, Transport, gossipsub, mdns, noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(libp2p::swarm::NetworkBehaviour)]
pub struct Behaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
}

// The derive macro will automatically generate a BehaviourEvent enum

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum P2PMessage {
    ChannelOpen {
        channel_id: String,
        funding_txid: String,
        capacity: u64,
        initial_balance: u64,
    },
    ChannelClose {
        channel_id: String,
        final_balance_a: u64,
        final_balance_b: u64,
    },
    Payment {
        channel_id: String,
        amount: u64,
        sequence: u64,
        commitment_tx: String,
        signature: String,
    },
    CommitmentSigned {
        channel_id: String,
        signature: String,
        sequence: u64,
    },
}

pub struct P2PNode {
    swarm: Swarm<Behaviour>,
    key_manager: Arc<KeyManager>,
    message_sender: mpsc::UnboundedSender<P2PMessage>,
    peers: HashMap<PeerId, String>, // peer_id -> node_id mapping
}

impl P2PNode {
    pub async fn new(key_manager: Arc<KeyManager>) -> Result<Self> {
        // Create a random PeerId (in production, derive from node keys)
        let local_key = libp2p::identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());
        println!("Local peer id: {local_peer_id}");

        // Set up transport
        let transport = tcp::tokio::Transport::default()
            .upgrade(libp2p::core::upgrade::Version::V1Lazy)
            .authenticate(noise::Config::new(&local_key)?)
            .multiplex(yamux::Config::default())
            .boxed();

        // Create gossipsub topic for Lightning messages
        let gossipsub_topic = gossipsub::IdentTopic::new("lightning-offline");

        // Set up gossipsub
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(std::time::Duration::from_secs(10))
            .validation_mode(gossipsub::ValidationMode::Strict)
            .build()
            .expect("Valid config");

        let mut gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )
        .map_err(|e| anyhow::anyhow!("Failed to create gossipsub behaviour: {}", e))?;

        gossipsub
            .subscribe(&gossipsub_topic)
            .map_err(|e| anyhow::anyhow!("Failed to subscribe to gossipsub topic: {}", e))?;

        // Set up mDNS for local peer discovery
        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)
            .map_err(|e| anyhow::anyhow!("Failed to create mDNS behaviour: {}", e))?;

        let behaviour = Behaviour { gossipsub, mdns };
        let swarm = Swarm::new(
            transport,
            behaviour,
            local_peer_id,
            libp2p::swarm::Config::with_tokio_executor(),
        );

        let (message_sender, _) = mpsc::unbounded_channel();

        Ok(P2PNode {
            swarm,
            key_manager,
            message_sender,
            peers: HashMap::new(),
        })
    }

    pub async fn start_listening(&mut self) -> Result<()> {
        // Listen on multiple addresses
        let addr: Multiaddr = "/ip4/0.0.0.0/tcp/0"
            .parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse address: {}", e))?;
        self.swarm.listen_on(addr)?;

        loop {
            match self.swarm.select_next_some().await {
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Listening on {address}");
                }
                SwarmEvent::Behaviour(event) => {
                    self.handle_behaviour_event(event).await;
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    println!("Connected to {peer_id}");
                    self.peers.insert(peer_id, peer_id.to_string());
                }
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    println!("Disconnected from {peer_id}");
                    self.peers.remove(&peer_id);
                }
                _ => {}
            }
        }
    }

    async fn handle_behaviour_event(&mut self, event: BehaviourEvent) {
        match event {
            BehaviourEvent::Mdns(mdns::Event::Discovered(list)) => {
                for (peer_id, _multiaddr) in list {
                    println!("mDNS discovered a new peer: {peer_id}");
                    self.swarm
                        .behaviour_mut()
                        .gossipsub
                        .add_explicit_peer(&peer_id);
                    let _ = self.swarm.dial(peer_id);
                }
            }
            BehaviourEvent::Mdns(mdns::Event::Expired(list)) => {
                for (peer_id, _multiaddr) in list {
                    println!("mDNS discover peer has expired: {peer_id}");
                    self.swarm
                        .behaviour_mut()
                        .gossipsub
                        .remove_explicit_peer(&peer_id);
                }
            }
            BehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source,
                message_id: _,
                message,
            }) => {
                if let Ok(p2p_message) = serde_json::from_slice::<P2PMessage>(&message.data) {
                    println!(
                        "Received message from {}: {:?}",
                        propagation_source, p2p_message
                    );
                    let _ = self.message_sender.send(p2p_message);
                }
            }
            _ => {}
        }
    }

    pub async fn broadcast_message(&mut self, message: P2PMessage) -> Result<()> {
        let topic = gossipsub::IdentTopic::new("lightning-offline");
        let serialized = serde_json::to_vec(&message)?;

        if let Err(e) = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .publish(topic, serialized)
        {
            eprintln!("Failed to publish message: {:?}", e);
        }

        Ok(())
    }

    pub fn get_connected_peers(&self) -> Vec<String> {
        self.peers.values().cloned().collect()
    }
}
