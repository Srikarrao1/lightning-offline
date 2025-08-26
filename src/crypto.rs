use anyhow::Result;
use bitcoin::key::CompressedPublicKey;
use bitcoin::secp256k1::{
    Keypair, Message, PublicKey as SecpPublicKey, Secp256k1, SecretKey, ecdsa::Signature,
};
use bitcoin::{Address, Network, PrivateKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

pub struct KeyManager {
    secp: Secp256k1<bitcoin::secp256k1::All>,
    private_key: SecretKey,
    public_key: SecpPublicKey,
    node_id: String,
    bitcoin_address: Address,
}

impl KeyManager {
    pub fn new() -> Result<Self> {
        let secp = Secp256k1::new();
        let mut rng = OsRng;
        let mut sk_bytes = [0u8; 32];
        use rand::RngCore;
        rng.fill_bytes(&mut sk_bytes);
        let secret_key = SecretKey::from_slice(&sk_bytes).expect("32 bytes, within curve order");

        // ✅ Generate keypair using secp256k1 v0.29 API
        let keypair = Keypair::from_secret_key(&secp, &secret_key);

        let private_key = keypair.secret_key();
        let public_key = keypair.public_key();

        println!("Secret: {:?}", private_key.display_secret());
        println!("Public: {:?}", public_key);

        let compressed = CompressedPublicKey::from_slice(&public_key.serialize())?;

        // Generate node ID from raw secp256k1 pubkey
        let mut hasher = Sha256::new();
        hasher.update(public_key.serialize());
        let node_id = hex::encode(hasher.finalize());

        // Wrap as CompressedPublicKey for Bitcoin types
        let compressed = CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();

        // Generate Bitcoin address (bech32 P2WPKH on regtest)
        let bitcoin_private_key = PrivateKey::new(private_key, Network::Regtest);
        let bitcoin_address = Address::p2wpkh(&compressed, Network::Regtest);

        Ok(KeyManager {
            secp,
            private_key,
            public_key,
            node_id,
            bitcoin_address,
        })
    }

    pub fn get_node_id(&self) -> String {
        self.node_id.clone()
    }

    pub fn get_public_key(&self) -> SecpPublicKey {
        self.public_key
    }

    pub fn get_bitcoin_address(&self) -> String {
        self.bitcoin_address.to_string()
    }

    pub fn sign_message(&self, message: &[u8]) -> Result<Signature> {
        let mut hasher = Sha256::new();
        hasher.update(message);
        let hash = hasher.finalize();
        let message = Message::from_digest_slice(&hash)?;
        Ok(self.secp.sign_ecdsa(&message, &self.private_key))
    }

    pub fn verify_signature(
        &self,
        message: &[u8],
        signature: &Signature,
        pubkey: &SecpPublicKey,
    ) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(message);
        let hash = hasher.finalize();

        if let Ok(message) = Message::from_digest_slice(&hash) {
            self.secp.verify_ecdsa(&message, signature, pubkey).is_ok()
        } else {
            false
        }
    }

    pub fn create_multisig_address(&self, other_pubkey: &SecpPublicKey) -> Result<Address> {
        use bitcoin::opcodes::all::{OP_CHECKMULTISIG, OP_PUSHNUM_2};
        use bitcoin::script::Builder;

        let self_pubkey = bitcoin::PublicKey {
            compressed: true,
            inner: self.public_key,
        };

        let other_pubkey = bitcoin::PublicKey {
            compressed: true,
            inner: *other_pubkey,
        };

        let script = Builder::new()
            .push_opcode(OP_PUSHNUM_2)
            .push_key(&self_pubkey)
            .push_key(&other_pubkey)
            .push_opcode(OP_PUSHNUM_2)
            .push_opcode(OP_CHECKMULTISIG)
            .into_script();

        Ok(Address::p2wsh(&script, Network::Regtest))
    }
}
