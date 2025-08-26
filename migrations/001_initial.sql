CREATE TABLE IF NOT EXISTS channels (
    id TEXT PRIMARY KEY,
    peer_node_id TEXT NOT NULL,
    funding_txid TEXT NOT NULL,
    capacity INTEGER NOT NULL,
    my_balance INTEGER NOT NULL,
    peer_balance INTEGER NOT NULL,
    sequence_number INTEGER NOT NULL DEFAULT 0,
    is_open BOOLEAN NOT NULL DEFAULT TRUE,
    created_at DATETIME NOT NULL,
    multisig_address TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS commitment_transactions (
    id TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    my_balance INTEGER NOT NULL,
    peer_balance INTEGER NOT NULL,
    raw_tx TEXT NOT NULL,
    signature TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    FOREIGN KEY (channel_id) REFERENCES channels (id)
);

CREATE TABLE IF NOT EXISTS payments (
    id TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL,
    amount INTEGER NOT NULL,
    direction TEXT NOT NULL CHECK (direction IN ('incoming', 'outgoing')),
    sequence INTEGER NOT NULL,
    timestamp DATETIME NOT NULL,
    is_offline BOOLEAN NOT NULL DEFAULT FALSE,
    FOREIGN KEY (channel_id) REFERENCES channels (id)
);

CREATE INDEX IF NOT EXISTS idx_channels_peer_node_id ON channels(peer_node_id);
CREATE INDEX IF NOT EXISTS idx_channels_is_open ON channels(is_open);
CREATE INDEX IF NOT EXISTS idx_commitment_transactions_channel_id ON commitment_transactions(channel_id);
CREATE INDEX IF NOT EXISTS idx_commitment_transactions_sequence ON commitment_transactions(sequence);
CREATE INDEX IF NOT EXISTS idx_payments_channel_id ON payments(channel_id);
CREATE INDEX IF NOT EXISTS idx_payments_timestamp ON payments(timestamp);