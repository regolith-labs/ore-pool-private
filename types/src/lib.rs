use drillx::Solution;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Copy, Clone)]
pub struct Challenge {
    /// The current challenge the pool is accepting solutions for.
    pub challenge: [u8; 32],

    /// Foreign key to the ORE proof account.
    pub lash_hash_at: i64,

    // The current minimum difficulty accepted by the ORE program.
    pub min_score: u64,

    // The cutoff time when the server will stop accepting contributions.
    pub cutoff_time: u64,

    /// The unix timestamp from the onchain clock.
    pub unix_timestamp: i64,
}

impl Default for Challenge {
    fn default() -> Self {
        Challenge {
            challenge: [0; 32],
            lash_hash_at: 0,
            min_score: 0,
            cutoff_time: 0,
            unix_timestamp: 0,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PoolMessage {
    #[serde(rename = "challenge")]
    NewChallenge { challenge: Challenge },
    #[serde(rename = "solution")]
    NewSolution { solution: Solution },
    #[serde(rename = "error")]
    Error { message: String },
}
