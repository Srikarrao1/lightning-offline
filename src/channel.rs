use crate::crypto::KeyManager;
use crate::storage::Database;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentChannel {
    pub id: String,
    pub peer_node_id: String,
    pub funding_txid: String,
    pub capacity: u64, // satoshis
    pub my_balance: u64,
    pub peer_balance: u64,
    pub sequence_number: u64,
    pub is_open: bool,
    pub created_at: DateTime<Utc>,
    pub multisig_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitmentTransaction {
    pub id: String,
    pub channel_id: String,
    pub sequence: u64,
    pub my_balance: u64,
    pub peer_balance: u64,
    pub raw_tx: String,
    pub signature: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRecord {
    pub id: String,
    pub channel_id: String,
    pub amount: u64,
    pub direction: String, // "outgoing" or "incoming"
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub is_offline: bool,
}

pub struct ChannelManager {
    key_manager: Arc<KeyManager>,
    database: Arc<Database>,
    channels: HashMap<String, PaymentChannel>,
    commitment_txs: HashMap<String, Vec<CommitmentTransaction>>,
}

impl ChannelManager {
    pub async fn new(key_manager: Arc<KeyManager>, database: Arc<Database>) -> Result<Self> {
        let mut manager = ChannelManager {
            key_manager,
            database,
            channels: HashMap::new(),
            commitment_txs: HashMap::new(),
        };

        // Load existing channels from database
        manager.load_channels().await?;

        Ok(manager)
    }

    async fn load_channels(&mut self) -> Result<()> {
        let channels = self.database.get_all_channels().await?;
        for channel in channels {
            self.channels.insert(channel.id.clone(), channel);
        }

        // Load commitment transactions
        for channel_id in self.channels.keys() {
            let commitments = self.database.get_channel_commitments(channel_id).await?;
            self.commitment_txs.insert(channel_id.clone(), commitments);
        }

        Ok(())
    }

    pub async fn open_channel(
        &mut self,
        peer_node_id: String,
        capacity: u64,
    ) -> Result<PaymentChannel> {
        let channel_id = Uuid::new_v4().to_string();
        let funding_txid = format!("funding_{}", Uuid::new_v4());

        // Create multisig address (simplified - in reality, coordinate with peer)
        let multisig_address = if peer_node_id.starts_with("12D3") {
            // This is a libp2p peer ID - generate a dummy multisig address for testing
            let uuid_hex = Uuid::new_v4().to_string().replace("-", "");
            // Safe slicing with proper bounds checking
            let addr_hash = if uuid_hex.len() >= 20 {
                &uuid_hex[..20]
            } else {
                &uuid_hex
            };
            format!("bcrt1q{}", addr_hash)
        } else {
            // Assume it's a hex-encoded Bitcoin pubkey
            match hex::decode(&peer_node_id) {
                Ok(decoded) => match bitcoin::secp256k1::PublicKey::from_slice(&decoded) {
                    Ok(peer_pubkey) => self
                        .key_manager
                        .create_multisig_address(&peer_pubkey)?
                        .to_string(),
                    Err(_) => {
                        return Err(anyhow::anyhow!(
                            "Invalid public key format: {}",
                            peer_node_id
                        ));
                    }
                },
                Err(_) => {
                    return Err(anyhow::anyhow!(
                        "Invalid hex encoding in peer node ID: {}",
                        peer_node_id
                    ));
                }
            }
        };

        let channel = PaymentChannel {
            id: channel_id.clone(),
            peer_node_id,
            funding_txid,
            capacity,
            my_balance: capacity / 2, // Split initial funding
            peer_balance: capacity / 2,
            sequence_number: 0,
            is_open: true,
            created_at: Utc::now(),
            multisig_address,
        };

        // Save to database
        self.database.save_channel(&channel).await?;
        self.channels.insert(channel_id.clone(), channel.clone());
        self.commitment_txs.insert(channel_id, Vec::new());

        Ok(channel)
    }

    pub async fn send_payment(&mut self, channel_id: &str, amount: u64) -> Result<PaymentRecord> {
        let channel_snapshot: PaymentChannel;

        {
            // limit scope of mutable borrow
            let channel = self
                .channels
                .get_mut(channel_id)
                .ok_or_else(|| anyhow::anyhow!("Channel not found"))?;

            if !channel.is_open {
                return Err(anyhow::anyhow!("Channel is not open"));
            }

            if channel.my_balance < amount {
                return Err(anyhow::anyhow!("Insufficient balance"));
            }

            // update balances
            channel.my_balance -= amount;
            channel.peer_balance += amount;
            channel.sequence_number += 1;

            // snapshot so we can use it later
            channel_snapshot = channel.clone();

            // mutable borrow of `channel` ends here (scope closes)
        }

        // ✅ safe to borrow `&self` now
        let commitment = self
            .create_commitment_transaction(&channel_snapshot)
            .await?;

        self.database
            .save_commitment_transaction(&commitment)
            .await?;
        self.commitment_txs
            .get_mut(channel_id)
            .unwrap()
            .push(commitment);

        let payment = PaymentRecord {
            id: Uuid::new_v4().to_string(),
            channel_id: channel_id.to_string(),
            amount,
            direction: "outgoing".to_string(),
            sequence: channel_snapshot.sequence_number,
            timestamp: Utc::now(),
            is_offline: true,
        };

        self.database.save_payment(&payment).await?;

        // now we can safely borrow channel mutably again
        if let Some(channel) = self.channels.get_mut(channel_id) {
            self.database.update_channel(channel).await?;
        }

        Ok(payment)
    }

    pub async fn receive_payment(
        &mut self,
        channel_id: &str,
        amount: u64,
        sequence: u64,
    ) -> Result<PaymentRecord> {
        let channel = self
            .channels
            .get_mut(channel_id)
            .ok_or_else(|| anyhow::anyhow!("Channel not found"))?;

        if !channel.is_open {
            return Err(anyhow::anyhow!("Channel is not open"));
        }

        // Update balances (from peer's payment)
        channel.peer_balance -= amount;
        channel.my_balance += amount;
        channel.sequence_number = sequence;

        // Create payment record
        let payment = PaymentRecord {
            id: Uuid::new_v4().to_string(),
            channel_id: channel_id.to_string(),
            amount,
            direction: "incoming".to_string(),
            sequence,
            timestamp: Utc::now(),
            is_offline: true,
        };

        // Save payment record
        self.database.save_payment(&payment).await?;

        // Update channel in database
        self.database.update_channel(channel).await?;

        Ok(payment)
    }

    async fn create_commitment_transaction(
        &self,
        channel: &PaymentChannel,
    ) -> Result<CommitmentTransaction> {
        // Create a simplified commitment transaction
        // In reality, this would be a proper Bitcoin transaction
        let raw_tx = format!(
            "{{\"version\":2,\"inputs\":[{{\"txid\":\"{}\",\"vout\":0}}],\"outputs\":[{{\"amount\":{},\"address\":\"my_address\"}},{{\"amount\":{},\"address\":\"peer_address\"}}],\"sequence\":{}}}",
            channel.funding_txid, channel.my_balance, channel.peer_balance, channel.sequence_number
        );

        let signature = hex::encode(
            self.key_manager
                .sign_message(raw_tx.as_bytes())?
                .serialize_compact(),
        );

        Ok(CommitmentTransaction {
            id: Uuid::new_v4().to_string(),
            channel_id: channel.id.clone(),
            sequence: channel.sequence_number,
            my_balance: channel.my_balance,
            peer_balance: channel.peer_balance,
            raw_tx,
            signature,
            created_at: Utc::now(),
        })
    }

    pub async fn close_channel(&mut self, channel_id: &str) -> Result<()> {
        let channel = self
            .channels
            .get_mut(channel_id)
            .ok_or_else(|| anyhow::anyhow!("Channel not found"))?;

        channel.is_open = false;

        // In reality, broadcast the latest commitment transaction to Bitcoin network
        println!(
            "Settling channel {} - Final balances: Me: {}, Peer: {}",
            channel_id, channel.my_balance, channel.peer_balance
        );

        // Update in database
        self.database.update_channel(channel).await?;

        Ok(())
    }

    pub fn get_channel(&self, channel_id: &str) -> Option<&PaymentChannel> {
        self.channels.get(channel_id)
    }

    pub fn get_all_channels(&self) -> Vec<&PaymentChannel> {
        self.channels.values().collect()
    }

    pub async fn get_channel_payments(&self, channel_id: &str) -> Result<Vec<PaymentRecord>> {
        self.database.get_channel_payments(channel_id).await
    }
}