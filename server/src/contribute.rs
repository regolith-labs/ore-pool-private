use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use drillx::Solution;
use serde::{Deserialize, Serialize};
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use tokio::sync::{Mutex, RwLock};

use crate::{Aggregator, Challenge, Contribution};

/// The payload to send to the /contribute endpoint.
#[derive(Debug, Deserialize, Serialize)]
pub struct ContributePayload {
    /// The authority of the member account sending the payload.
    pub authority: Pubkey,

    /// The solution submitted.
    pub solution: Solution,

    /// Must be a valid signature of the solution
    pub signature: Signature,
}

/// Accepts solutions from pool members. If their solutions are valid, it
/// aggregates the contributions into a list for publishing and submission.
pub async fn contribute(
    payload: web::Json<ContributePayload>,
    aggregator: web::Data<Arc<Mutex<Aggregator>>>,
    challenge: web::Data<Arc<RwLock<Challenge>>>,
) -> impl Responder {
    // Authenticate the sender signature
    if !payload
        .signature
        .verify(&payload.authority.to_bytes(), &payload.solution.to_bytes())
    {
        return HttpResponse::BadRequest();
    }

    // TODO Validate sender is an accepted member of the pool

    // Return error if digest is invalid
    let challenge = challenge.read().await;
    if !drillx::is_valid_digest(&challenge, &payload.solution.n, &payload.solution.d) {
        return HttpResponse::BadRequest();
    }
    drop(challenge);

    // Calculate score
    let difficulty = payload.solution.to_hash().difficulty();
    let score = 2u64.pow(difficulty);

    // TODO Reject if score is below min difficulty

    // Update the aggegator
    let mut w_aggregator = aggregator.lock().await;
    w_aggregator.total_score += score;
    w_aggregator.contributions.insert(
        payload.authority,
        Contribution {
            solution: payload.solution,
            score,
        },
    );
    drop(w_aggregator);

    HttpResponse::Ok()
}
