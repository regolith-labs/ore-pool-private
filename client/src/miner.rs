use std::sync::Arc;

use anyhow::Error;
use drillx::{equix, Solution};
use ore_pool_types::Challenge;
use tokio::{
    sync::RwLock,
    task::{JoinError, JoinSet},
};

use super::pool::Pool;

#[derive(Debug)]
pub struct Miner {
    pub pool: Pool,
    pub cores: u64,
    pub nonce_indices: Vec<u64>,
    pub solutions_tx: tokio::sync::mpsc::UnboundedSender<Solution>,
    pub tasks: JoinSet<Result<(), JoinError>>,
}

impl Miner {
    pub async fn launch(pool_url: &str) -> Result<Self, Error> {
        // Split nonce-device space for muliple cores
        println!("Splitting nonce-device space for multiple cores...");
        let cores = 12; // num_cpus::get() as u64; // TODO: make this configurable
        let mut nonce_indices = Vec::with_capacity(cores as usize);
        for _ in 0..(cores) {
            nonce_indices.push(rand::random::<u64>());
        }

        // Init challenge channel for continuous processing of new challenges
        println!("Initializing challenge channel...");
        let (challenges_tx, challenges_rx) = tokio::sync::mpsc::unbounded_channel::<Challenge>();

        // Init solutions channel for continuous submission of best solutions
        println!("Initializing solutions channel...");
        let (solutions_tx, solutions_rx) = tokio::sync::mpsc::unbounded_channel::<Solution>();

        // Connect to pool
        println!("Connecting to pool...");
        let pool = Pool::new(pool_url, challenges_tx, solutions_rx).await?;

        // Spawn a task to mine
        let tasks = Miner::mine(
            challenges_rx,
            cores,
            nonce_indices.clone(),
            solutions_tx.clone(),
        )
        .await
        .unwrap();

        Ok(Self {
            pool,
            cores,
            nonce_indices,
            solutions_tx,
            tasks,
        })
    }

    async fn mine(
        mut challenge_rx: tokio::sync::mpsc::UnboundedReceiver<Challenge>,
        cores: u64,
        nonce_indices: Vec<u64>,
        solutions_tx: tokio::sync::mpsc::UnboundedSender<Solution>,
    ) -> Result<JoinSet<Result<(), JoinError>>, Error> {
        // println!("Challenge: {:?}", challenge);

        // Dispatch job to each thread
        let global_best_score = Arc::new(RwLock::new(0u64));
        let current_challenge = Arc::new(RwLock::new(Challenge::default()));
        tokio::spawn({
            let current_challenge = current_challenge.clone();
            let global_best_score = global_best_score.clone();
            async move {
                while let Some(challenge) = challenge_rx.recv().await {
                    println!("New challenge: {:?}", challenge);
                    *current_challenge.write().await = challenge;
                    *global_best_score.write().await = 0;
                }
            }
        });

        println!("Mining on {} cores", cores);
        let core_ids = core_affinity::get_core_ids().expect("Failed to fetch core count");
        let core_ids = core_ids.into_iter().filter(|id| id.id < (cores as usize));
        let handles = core_ids
            .map(|i| {
                let global_best_score = global_best_score.clone();
                let current_challenge = current_challenge.clone();
                tokio::spawn({
                    let starting_nonce = nonce_indices[i.id];
                    let mut memory = equix::SolverMemory::new();
                    let ch = solutions_tx.clone();
                    async move {
                        // Start hashing
                        let mut nonce = starting_nonce;
                        loop {
                            // Get current challenge
                            // TODO: Reading this every time adds latency, find a better way
                            let challenge = *current_challenge.read().await;
                            let local_best_score = *global_best_score.read().await;

                            // Get hashes
                            let hxs = drillx::hashes_with_memory(
                                &mut memory,
                                &challenge.challenge,
                                &nonce.to_le_bytes(),
                            );

                            // Look for best difficulty score in all hashes
                            for hx in hxs {
                                let score = hx.difficulty() as u64;
                                if score >= challenge.min_score && score > local_best_score {
                                    // Update global best difficulty if local best is higher
                                    *global_best_score.write().await = score;

                                    // Continuously upload best solution to pool
                                    let digest = hx.d;
                                    let nonce = nonce.to_le_bytes();
                                    let solution = Solution {
                                        d: digest,
                                        n: nonce,
                                    };

                                    println!("Sending solution: {}", score);
                                    if let Err(err) = ch.send(solution) {
                                        println!("{} {:?}", "ERROR", err);
                                    }

                                    // Yield control to allow other tasks to run
                                    tokio::task::yield_now().await;
                                }
                            }

                            // Log best difficulty every 100 nonces
                            if i.id == 0 {
                                if nonce % 100 == 0 {
                                    println!(
                                        "Mining... Best score: {} {}",
                                        local_best_score, nonce
                                    );
                                }
                            }

                            // Increment nonce
                            nonce = nonce.wrapping_add(1);
                        }
                    }
                })
            })
            .collect::<Vec<_>>();

        // Create a new JoinSet to manage the tasks
        let mut set = tokio::task::JoinSet::new();
        for handle in handles {
            set.spawn(handle);
        }
        Ok(set)
    }
}
