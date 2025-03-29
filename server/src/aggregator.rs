use drillx::Solution;
use ore_api::{
    consts::{BUS_ADDRESSES, BUS_COUNT},
    state::Bus,
};
use ore_pool_types::Challenge;
use rand::Rng;
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use steel::AccountDeserialize;

use crate::{error::Error, operator::Operator, submit};

/// Aggregates contributions from the pool members.
pub struct Aggregator {
    /// The current challenge.
    pub current_challenge: Challenge,

    /// The best solution for the current challenge.
    pub best_solution: Solution,

    /// The score of the best solution.
    pub best_score: u64,
}

pub async fn process_contributions(
    aggregator: &tokio::sync::RwLock<Aggregator>,
    operator: &Operator,
) -> Result<(), Error> {
    // outer loop for new challenges
    loop {
        let timer = tokio::time::Instant::now();
        let cutoff_time = {
            let proof = match operator.get_proof().await {
                Ok(proof) => proof,
                Err(err) => {
                    log::error!("{:?}", err);
                    continue;
                }
            };
            match operator.get_cutoff(&proof).await {
                Ok(cutoff_time) => cutoff_time,
                Err(err) => {
                    log::error!("{:?}", err);
                    continue;
                }
            }
        };
        let remaining_time = cutoff_time.saturating_sub(timer.elapsed().as_secs());
        tokio::time::sleep(tokio::time::Duration::from_secs(remaining_time)).await;

        // at this point, the cutoff time has been reached
        let best_score = {
            let r_aggregator = aggregator.read().await;
            r_aggregator.best_score
        };
        if best_score > 0 {
            // submit if contributions exist
            let mut w_aggregator = aggregator.write().await;
            if let Err(err) = w_aggregator.submit_and_reset(operator).await {
                log::error!("{:?}", err);
            }
        } else {
            // no contributions yet, wait for the first one to submit
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }
}

impl Aggregator {
    pub async fn new(operator: &Operator) -> Result<Self, Error> {
        // fetch accounts
        let proof = operator.get_proof().await?;
        let cutoff_time = operator.get_cutoff(&proof).await?;
        let min_difficulty = operator.min_difficulty().await?;
        let challenge = Challenge {
            challenge: proof.challenge,
            lash_hash_at: proof.last_hash_at,
            min_difficulty,
            cutoff_time,
            unix_timestamp: 0,
        };

        // build self
        let aggregator = Aggregator {
            current_challenge: challenge,
            best_solution: Solution::new([0; 16], [0; 8]),
            best_score: 0,
        };
        Ok(aggregator)
    }

    async fn submit_and_reset(&mut self, operator: &Operator) -> Result<(), Error> {
        // check if reset is needed
        // this may happen if a solution is landed on chain
        // but a subsequent application error is thrown before resetting
        if self.check_for_reset(operator).await? {
            self.reset(operator).await?;
            // there was a reset
            // so restart contribution loop against new challenge
            return Ok(());
        };

        // prepare best solution and attestation of hash-power
        let best_solution = self.best_solution;

        // derive accounts for instructions
        let bus = self.find_bus(operator).await?;

        // Get boost accounts
        let rpc_client = &operator.rpc_client;
        let boost_config_address = ore_boost_api::state::config_pda().0;
        let boost_config_account = rpc_client.get_account_data(&boost_config_address).await?;
        let boost_config = ore_boost_api::state::Config::try_from_bytes(&boost_config_account)?;

        // build instructions
        let auth_ix = ore_api::sdk::auth(operator.proof_address);
        let submit_ix = ore_api::sdk::mine(
            operator.keypair.pubkey(),
            operator.authority,
            bus,
            best_solution,
            boost_config.current,
            boost_config_address,
        );
        let rotate_ix = ore_boost_api::sdk::rotate(operator.keypair.pubkey());
        let sig = submit::submit_instructions(
            &operator.keypair,
            &operator.rpc_client,
            &operator.jito_client,
            &[auth_ix, submit_ix, rotate_ix],
            550_000,
            2_000,
        )
        .await?;
        log::info!("{:?}", sig);

        // reset
        self.reset(operator).await?;
        Ok(())
    }

    /// fetch the bus with the largest balance
    async fn find_bus(&self, operator: &Operator) -> Result<Pubkey, Error> {
        let rpc_client = &operator.rpc_client;
        let accounts = rpc_client.get_multiple_accounts(&BUS_ADDRESSES).await?;
        let mut top_bus_balance: u64 = 0;
        let bus_index = rand::thread_rng().gen_range(0..BUS_COUNT);
        let mut top_bus = BUS_ADDRESSES[bus_index];
        for account in accounts.into_iter().flatten() {
            if let Ok(bus) = Bus::try_from_bytes(&account.data) {
                if bus.rewards.gt(&top_bus_balance) {
                    top_bus_balance = bus.rewards;
                    top_bus = BUS_ADDRESSES[bus.id as usize];
                }
            }
        }
        Ok(top_bus)
    }

    async fn reset(&mut self, operator: &Operator) -> Result<(), Error> {
        log::info!("resetting");
        self.update_challenge(operator).await?;
        Ok(())
    }

    async fn check_for_reset(&self, operator: &Operator) -> Result<bool, Error> {
        let last_hash_at = self.current_challenge.lash_hash_at;
        let proof = operator.get_proof().await?;
        let needs_reset = proof.last_hash_at != last_hash_at;
        Ok(needs_reset)
    }

    async fn update_challenge(&mut self, operator: &Operator) -> Result<(), Error> {
        let max_retries = 10;
        let mut retries = 0;
        let last_hash_at = self.current_challenge.lash_hash_at;
        loop {
            let proof = operator.get_proof().await?;
            if proof.last_hash_at != last_hash_at {
                let cutoff_time = operator.get_cutoff(&proof).await?;
                let min_difficulty = operator.min_difficulty().await?;
                self.current_challenge.challenge = proof.challenge;
                self.current_challenge.lash_hash_at = proof.last_hash_at;
                self.current_challenge.min_difficulty = min_difficulty;
                self.current_challenge.cutoff_time = cutoff_time;
                return Ok(());
            } else {
                retries += 1;
                if retries == max_retries {
                    return Err(Error::Internal("failed to fetch new challenge".to_string()));
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }
}
