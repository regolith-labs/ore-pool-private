use actix_web::{web, HttpResponse, Responder};
use ore_pool_types::{ChallengeWithTimestamp, ContributePayloadV2, GetChallengePayload};

use crate::{aggregator::Aggregator, operator::Operator};

pub async fn address(operator: web::Data<Operator>) -> impl Responder {
    let operator = operator.as_ref();
    HttpResponse::Ok().json(&operator.proof_address)
}

pub async fn challenge(
    aggregator: web::Data<tokio::sync::RwLock<Aggregator>>,
    clock_tx: web::Data<tokio::sync::broadcast::Sender<i64>>,
    _path: web::Path<GetChallengePayload>,
) -> impl Responder {
    // Read from clock
    let mut clock_rx = clock_tx.subscribe();
    let unix_timestamp = match clock_rx.recv().await {
        Ok(ts) => ts,
        Err(err) => {
            log::error!("{:?}", err);
            return HttpResponse::InternalServerError().body(err.to_string());
        }
    };

    // Acquire read on aggregator for challenge
    let challenge = {
        let aggregator = aggregator.read().await;
        aggregator.current_challenge
    };

    // Build member challenge
    let challenge_with_timestamp = ChallengeWithTimestamp {
        challenge,
        unix_timestamp,
    };
    HttpResponse::Ok().json(&challenge_with_timestamp)
}

/// Accepts solutions from pool members. If their solutions are valid, it
/// aggregates the contributions into a list for publishing and submission.
pub async fn contribute(
    aggregator: web::Data<tokio::sync::RwLock<Aggregator>>,
    payload: web::Json<ContributePayloadV2>,
) -> impl Responder {
    // acquire read on aggregator for challenge
    let r_aggregator = aggregator.read().await;
    let challenge = r_aggregator.current_challenge;
    let best_score = r_aggregator.best_score;
    drop(r_aggregator);

    // decode solution difficulty
    let solution = &payload.solution;
    let difficulty = solution.to_hash().difficulty();
    let score = 2u64.pow(difficulty);

    // error if solution below min difficulty
    if difficulty < (challenge.min_difficulty as u32) {
        log::error!(
            "solution below min difficulity: {:?} received: {:?} required: {:?}",
            payload.authority,
            difficulty,
            challenge.min_difficulty
        );
        return HttpResponse::BadRequest().finish();
    }

    // update best score if new best
    if best_score < score {
        let mut w_aggregator = aggregator.write().await;
        w_aggregator.best_score = score;
        w_aggregator.best_solution = payload.solution;
    }

    HttpResponse::Ok().finish()
}
