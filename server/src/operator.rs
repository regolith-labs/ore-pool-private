use std::str::FromStr;

use ore_api::state::{Config, Proof};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    clock::Clock, commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Keypair,
    signer::EncodableKey, sysvar,
};
use steel::AccountDeserialize;

use crate::error::Error;

pub const BUFFER_OPERATOR: u64 = 5;
const MIN_DIFFICULTY: Option<u64> = Some(7);

pub struct Operator {
    /// The miner authority keypair.
    pub keypair: Keypair,

    /// The authority address.
    pub authority: Pubkey,

    /// The proof address.
    pub proof_address: Pubkey,

    /// Solana RPC client.
    pub rpc_client: RpcClient,

    /// JITO RPC client.
    pub jito_client: RpcClient,
}

impl Operator {
    pub fn new() -> Result<Operator, Error> {
        let keypair = Self::keypair()?;
        let authority = Self::authority()?;
        let proof_address = Self::proof_address()?;
        let rpc_client = Self::rpc_client()?;
        let jito_client = Self::jito_client();
        Ok(Operator {
            keypair,
            authority,
            proof_address,
            rpc_client,
            jito_client,
        })
    }
}

impl Operator {
    pub async fn get_proof(&self) -> Result<Proof, Error> {
        let proof_address = self.proof_address;
        let rpc_client = &self.rpc_client;
        let data = rpc_client.get_account_data(&proof_address).await?;
        let proof = Proof::try_from_bytes(data.as_slice())?;
        Ok(*proof)
    }

    pub async fn get_cutoff(&self, proof: &Proof) -> Result<u64, Error> {
        let clock = self.get_clock().await?;
        Ok(proof
            .last_hash_at
            .saturating_add(60)
            .saturating_sub(BUFFER_OPERATOR as i64)
            .saturating_sub(clock.unix_timestamp)
            .max(0) as u64)
    }

    pub async fn min_score(&self) -> Result<u64, Error> {
        let config = self.get_config().await?;
        let program_min = config.min_difficulty;
        match MIN_DIFFICULTY {
            Some(operator_min) => {
                let max = program_min.max(operator_min);
                Ok(max)
            }
            None => Ok(program_min),
        }
    }

    async fn get_config(&self) -> Result<Config, Error> {
        let config_pda = ore_api::consts::CONFIG_ADDRESS;
        let rpc_client = &self.rpc_client;
        let data = rpc_client.get_account_data(&config_pda).await?;
        let config = Config::try_from_bytes(data.as_slice())?;
        Ok(*config)
    }

    pub async fn get_clock(&self) -> Result<Clock, Error> {
        let rpc_client = &self.rpc_client;
        let data = rpc_client.get_account_data(&sysvar::clock::id()).await?;
        bincode::deserialize(&data).map_err(From::from)
    }
}

impl Operator {
    fn keypair() -> Result<Keypair, Error> {
        let keypair_path = Operator::keypair_path()?;
        let keypair = Keypair::read_from_file(keypair_path)
            .map_err(|err| Error::Internal(err.to_string()))?;
        Ok(keypair)
    }

    fn keypair_path() -> Result<String, Error> {
        std::env::var("KEYPAIR_PATH").map_err(From::from)
    }

    fn authority() -> Result<Pubkey, Error> {
        let authority = std::env::var("AUTHORITY")?;
        let authority = Pubkey::from_str(&authority)?;
        Ok(authority)
    }

    fn proof_address() -> Result<Pubkey, Error> {
        let authority = Self::authority()?;
        let (proof_address, _) = ore_api::state::proof_pda(authority);
        Ok(proof_address)
    }

    fn rpc_client() -> Result<RpcClient, Error> {
        let rpc_url = Operator::rpc_url()?;
        Ok(RpcClient::new_with_commitment(
            rpc_url,
            CommitmentConfig::confirmed(),
        ))
    }

    fn jito_client() -> RpcClient {
        let rpc_url = "https://mainnet.block-engine.jito.wtf/api/v1/transactions";
        RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed())
    }

    fn rpc_url() -> Result<String, Error> {
        std::env::var("RPC_URL").map_err(From::from)
    }
}
