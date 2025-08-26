use crate::channel::{CommitmentTransaction, PaymentChannel, PaymentRecord};
use anyhow::Result;
use sqlx::{Row, Sqlite, sqlite::SqlitePool};

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        std::fs::create_dir_all("./data")?;
        let pool = SqlitePool::connect(database_url).await?;
        Ok(Database { pool })
    }

    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn save_channel(&self, channel: &PaymentChannel) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO channels (id, peer_node_id, funding_txid, capacity, my_balance, peer_balance, sequence_number, is_open, created_at, multisig_address)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#
        )
        .bind(&channel.id)
        .bind(&channel.peer_node_id)
        .bind(&channel.funding_txid)
        .bind(channel.capacity as i64)
        .bind(channel.my_balance as i64)
        .bind(channel.peer_balance as i64)
        .bind(channel.sequence_number as i64)
        .bind(channel.is_open)
        .bind(&channel.created_at)
        .bind(&channel.multisig_address)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_channel(&self, channel: &PaymentChannel) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE channels 
            SET my_balance = ?1, peer_balance = ?2, sequence_number = ?3, is_open = ?4
            WHERE id = ?5
            "#,
        )
        .bind(channel.my_balance as i64)
        .bind(channel.peer_balance as i64)
        .bind(channel.sequence_number as i64)
        .bind(channel.is_open)
        .bind(&channel.id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_all_channels(&self) -> Result<Vec<PaymentChannel>> {
        let rows = sqlx::query(
            "SELECT id, peer_node_id, funding_txid, capacity, my_balance, peer_balance, sequence_number, is_open, created_at, multisig_address FROM channels"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut channels = Vec::new();
        for row in rows {
            channels.push(PaymentChannel {
                id: row.get("id"),
                peer_node_id: row.get("peer_node_id"),
                funding_txid: row.get("funding_txid"),
                capacity: row.get::<i64, _>("capacity") as u64,
                my_balance: row.get::<i64, _>("my_balance") as u64,
                peer_balance: row.get::<i64, _>("peer_balance") as u64,
                sequence_number: row.get::<i64, _>("sequence_number") as u64,
                is_open: row.get("is_open"),
                created_at: row.get("created_at"),
                multisig_address: row.get("multisig_address"),
            });
        }

        Ok(channels)
    }

    pub async fn save_commitment_transaction(
        &self,
        commitment: &CommitmentTransaction,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO commitment_transactions (id, channel_id, sequence, my_balance, peer_balance, raw_tx, signature, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#
        )
        .bind(&commitment.id)
        .bind(&commitment.channel_id)
        .bind(commitment.sequence as i64)
        .bind(commitment.my_balance as i64)
        .bind(commitment.peer_balance as i64)
        .bind(&commitment.raw_tx)
        .bind(&commitment.signature)
        .bind(&commitment.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_channel_commitments(
        &self,
        channel_id: &str,
    ) -> Result<Vec<CommitmentTransaction>> {
        let rows = sqlx::query(
            "SELECT id, channel_id, sequence, my_balance, peer_balance, raw_tx, signature, created_at FROM commitment_transactions WHERE channel_id = ?1 ORDER BY sequence"
        )
        .bind(channel_id)
        .fetch_all(&self.pool)
        .await?;

        let mut commitments = Vec::new();
        for row in rows {
            commitments.push(CommitmentTransaction {
                id: row.get("id"),
                channel_id: row.get("channel_id"),
                sequence: row.get::<i64, _>("sequence") as u64,
                my_balance: row.get::<i64, _>("my_balance") as u64,
                peer_balance: row.get::<i64, _>("peer_balance") as u64,
                raw_tx: row.get("raw_tx"),
                signature: row.get("signature"),
                created_at: row.get("created_at"),
            });
        }

        Ok(commitments)
    }

    pub async fn save_payment(&self, payment: &PaymentRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO payments (id, channel_id, amount, direction, sequence, timestamp, is_offline)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#
        )
        .bind(&payment.id)
        .bind(&payment.channel_id)
        .bind(payment.amount as i64)
        .bind(&payment.direction)
        .bind(payment.sequence as i64)
        .bind(&payment.timestamp)
        .bind(payment.is_offline)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_channel_payments(&self, channel_id: &str) -> Result<Vec<PaymentRecord>> {
        let rows = sqlx::query(
            "SELECT id, channel_id, amount, direction, sequence, timestamp, is_offline FROM payments WHERE channel_id = ?1 ORDER BY timestamp DESC"
        )
        .bind(channel_id)
        .fetch_all(&self.pool)
        .await?;

        let mut payments = Vec::new();
        for row in rows {
            payments.push(PaymentRecord {
                id: row.get("id"),
                channel_id: row.get("channel_id"),
                amount: row.get::<i64, _>("amount") as u64,
                direction: row.get("direction"),
                sequence: row.get::<i64, _>("sequence") as u64,
                timestamp: row.get("timestamp"),
                is_offline: row.get("is_offline"),
            });
        }

        Ok(payments)
    }
}
