use clap::{Arg, Command};
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json;
use std::process;
use tokio;
use anyhow::{anyhow, Result};


#[derive(Debug, Serialize, Deserialize)]
struct NodeInfo {
    node_id: String,
    public_key: String,
    bitcoin_address: String,
    connected_peers: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PaymentChannel {
    id: String,
    peer_node_id: String,
    funding_txid: String,
    capacity: u64,
    my_balance: u64,
    peer_balance: u64,
    sequence_number: u64,
    is_open: bool,
    created_at: String,
    multisig_address: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PaymentRecord {
    id: String,
    channel_id: String,
    amount: u64,
    direction: String,
    sequence: u64,
    timestamp: String,
    is_offline: bool,
}

#[derive(Debug, Serialize)]
struct OpenChannelRequest {
    peer_node_id: String,
    capacity: u64,
}

#[derive(Debug, Serialize)]
struct SendPaymentRequest {
    amount: u64,
}

struct LightningCli {
    client: reqwest::Client,
    base_url: String,
}

impl LightningCli {
    fn new(base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
        }
    }

    async fn get_node_info(&self) -> Result<NodeInfo, Box<dyn std::error::Error>> {
        let url = format!("{}/api/node/info", self.base_url);
        let response = self.client.get(&url).send().await?;
        let node_info: NodeInfo = response.json().await?;
        Ok(node_info)
    }

    async fn list_channels(&self) -> Result<Vec<PaymentChannel>, Box<dyn std::error::Error>> {
        let url = format!("{}/api/channels", self.base_url);
        let response = self.client.get(&url).send().await?;
        let channels: Vec<PaymentChannel> = response.json().await?;
        Ok(channels)
    }

    async fn open_channel(
        &self,
        peer_node_id: String,
        capacity: u64,
    ) -> Result<PaymentChannel, Box<dyn std::error::Error>> {
        let url = format!("{}/api/channels", self.base_url);
        let request = OpenChannelRequest {
            peer_node_id,
            capacity,
        };

        let response = self.client.post(&url).json(&request).send().await?;

        if response.status().is_success() {
            let channel: PaymentChannel = response.json().await?;
            Ok(channel)
        } else {
            Err(format!("Failed to open channel: {}", response.status()).into())
        }
    }

    async fn send_payment(
        &self,
        channel_id: String,
        amount: u64,
    ) -> Result<PaymentRecord, Box<dyn std::error::Error>> {
        let url = format!("{}/api/channels/{}/payments", self.base_url, channel_id);
        let request = SendPaymentRequest { amount };

        let response = self.client.post(&url).json(&request).send().await?;

        if response.status().is_success() {
            let payment: PaymentRecord = response.json().await?;
            Ok(payment)
        } else {
            Err(format!("Failed to send payment: {}", response.status()).into())
        }
    }

    async fn list_payments(
        &self,
        channel_id: String,
    ) -> Result<Vec<PaymentRecord>, Box<dyn std::error::Error>> {
        let url = format!("{}/api/channels/{}/payments", self.base_url, channel_id);
        let response = self.client.get(&url).send().await?;
        let payments: Vec<PaymentRecord> = response.json().await?;
        Ok(payments)
    }

    async fn close_channel(&self, channel_id: String) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("{}/api/channels/{}/close", self.base_url, channel_id);
        let response = self.client.post(&url).send().await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("Failed to close channel: {}", response.status()).into())
        }
    }
}

fn satoshis_to_btc(satoshis: u64) -> f64 {
    satoshis as f64 / 100_000_000.0
}

fn btc_to_satoshis(btc: f64) -> u64 {
    (btc * 100_000_000.0) as u64
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let matches = Command::new("lightning-cli")
        .version("1.0")
        .author("Lightning Network Team")
        .about("CLI client for Lightning Network offline payments")
        .arg(
            Arg::new("server")
                .long("server")
                .value_name("URL")
                .help("Lightning node server URL")
                .default_value("http://localhost:3000"),
        )
        .subcommand(Command::new("info").about("Display node information"))
        .subcommand(
            Command::new("channels")
                .about("List all payment channels")
                .subcommand(Command::new("list").about("List all channels"))
                .subcommand(
                    Command::new("open")
                        .about("Open a new payment channel")
                        .arg(
                            Arg::new("peer")
                                .long("peer")
                                .value_name("NODE_ID")
                                .help("Peer node ID")
                                .required(true),
                        )
                        .arg(
                            Arg::new("capacity")
                                .long("capacity")
                                .value_name("BTC")
                                .help("Channel capacity in BTC")
                                .required(true),
                        ),
                )
                .subcommand(
                    Command::new("close").about("Close a payment channel").arg(
                        Arg::new("channel_id")
                            .long("channel-id")
                            .value_name("ID")
                            .help("Channel ID to close")
                            .required(true),
                    ),
                ),
        )
        .subcommand(
            Command::new("pay")
                .about("Send a Lightning payment")
                .arg(
                    Arg::new("channel_id")
                        .long("channel-id")
                        .value_name("ID")
                        .help("Channel ID to send payment through")
                        .required(true),
                )
                .arg(
                    Arg::new("amount")
                        .long("amount")
                        .value_name("BTC")
                        .help("Payment amount in BTC")
                        .required(true),
                ),
        )
        .subcommand(
            Command::new("payments")
                .about("List payments for a channel")
                .arg(
                    Arg::new("channel_id")
                        .long("channel-id")
                        .value_name("ID")
                        .help("Channel ID to list payments for")
                        .required(true),
                ),
        )
        .get_matches();

    let server_url = matches.get_one::<String>("server").unwrap().clone();
    let cli = LightningCli::new(server_url);

    let result = match matches.subcommand() {
        Some(("info", _)) => match cli.get_node_info().await {
            Ok(info) => {
                println!("🏠 Lightning Node Information");
                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                println!("Node ID:         {}", info.node_id);
                println!("Public Key:      {}", info.public_key);
                println!("Bitcoin Address: {}", info.bitcoin_address);
                println!(
                    "Connected Peers: {}",
                    if info.connected_peers.is_empty() {
                        "None".to_string()
                    } else {
                        info.connected_peers.join(", ")
                    }
                );
                Ok(())
            }
            Err(e) => Err(e),
        },

        Some(("channels", sub_matches)) => {
            match sub_matches.subcommand() {
                Some(("list", _)) | None => match cli.list_channels().await {
                    Ok(channels) => {
                        if channels.is_empty() {
                            println!("No channels found.");
                        } else {
                            println!("⚡ Payment Channels");
                            println!("━━━━━━━━━━━━━━━━━━━");
                            for channel in channels {
                                println!();
                                println!("Channel ID:    {}", channel.id);
                                println!("Peer:          {}...", &channel.peer_node_id[..32]);
                                println!(
                                    "Status:        {}",
                                    if channel.is_open { "Open" } else { "Closed" }
                                );
                                println!(
                                    "Capacity:      {:.8} BTC",
                                    satoshis_to_btc(channel.capacity)
                                );
                                println!(
                                    "My Balance:    {:.8} BTC",
                                    satoshis_to_btc(channel.my_balance)
                                );
                                println!(
                                    "Peer Balance:  {:.8} BTC",
                                    satoshis_to_btc(channel.peer_balance)
                                );
                                println!("Sequence:      {}", channel.sequence_number);
                                println!("Created:       {}", channel.created_at);
                            }
                        }
                        Ok(())
                    }
                    Err(e) => Err(e),
                },

                Some(("open", open_matches)) => {
                    let peer = open_matches.get_one::<String>("peer").unwrap().clone();

                    let capacity_btc: f64 = open_matches
                        .get_one::<String>("capacity")
                        .unwrap()
                        .parse::<f64>()
                        .map_err(|_| anyhow!("Invalid capacity amount"))?; 

                    println!(
                        "Opening channel with peer {} and capacity {} BTC",
                        peer, capacity_btc
                    );

                    let capacity_sats = btc_to_satoshis(capacity_btc);

                    println!("Opening channel with peer: {}...", &peer[..16]);
                    println!(
                        "Capacity: {:.8} BTC ({} satoshis)",
                        capacity_btc, capacity_sats
                    );

                    match cli.open_channel(peer, capacity_sats).await {
                        Ok(channel) => {
                            println!("✅ Channel opened successfully!");
                            println!("Channel ID: {}", channel.id);
                            println!("Funding TX: {}", channel.funding_txid);
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }

                Some(("close", close_matches)) => {
                    let channel_id = close_matches
                        .get_one::<String>("channel_id")
                        .unwrap()
                        .clone();

                    println!("Closing channel: {}", channel_id);

                    match cli.close_channel(channel_id).await {
                        Ok(_) => {
                            println!("✅ Channel closed successfully!");
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }

                _ => {
                    eprintln!("Unknown channels subcommand. Use 'lightning-cli channels --help' for usage.");
                    process::exit(1);
                }
            }
        }

        Some(("pay", pay_matches)) => {
            let channel_id = pay_matches.get_one::<String>("channel_id").unwrap().clone();

            let amount_btc: f64 = pay_matches
                .get_one::<String>("amount")
                .unwrap()
                .parse::<f64>()
                .map_err(|_| anyhow!("Invalid payment amount"))?;

            println!("Paying {} BTC on channel {}", amount_btc, channel_id);

            let amount_sats = btc_to_satoshis(amount_btc);

            println!("Sending payment through channel: {}", channel_id);
            println!("Amount: {:.8} BTC ({} satoshis)", amount_btc, amount_sats);

            match cli.send_payment(channel_id, amount_sats).await {
                Ok(payment) => {
                    println!("✅ Payment sent successfully!");
                    println!("Payment ID: {}", payment.id);
                    println!("Sequence: {}", payment.sequence);
                    println!("Timestamp: {}", payment.timestamp);
                    if payment.is_offline {
                        println!("📱 Sent offline - will be synced when online");
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        Some(("payments", payments_matches)) => {
            let channel_id = payments_matches
                .get_one::<String>("channel_id")
                .unwrap()
                .clone();

            match cli.list_payments(channel_id).await {
                Ok(payments) => {
                    if payments.is_empty() {
                        println!("No payments found for this channel.");
                    } else {
                        println!("💸 Payment History");
                        println!("━━━━━━━━━━━━━━━━━━");
                        for payment in payments {
                            let direction_symbol = if payment.direction == "outgoing" {
                                "→"
                            } else {
                                "←"
                            };
                            let offline_marker = if payment.is_offline { " 📱" } else { "" };

                            println!(
                                "{} {:.8} BTC (seq: {}){}",
                                direction_symbol,
                                satoshis_to_btc(payment.amount),
                                payment.sequence,
                                offline_marker
                            );
                            println!("   {} - {}", payment.timestamp, payment.id);
                        }
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        _ => {
            eprintln!("No subcommand provided. Use '--help' for usage information.");
            process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("❌ Error: {}", e);
        process::exit(1);
    }
    Ok(())
}
