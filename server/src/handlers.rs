use actix_web::{web, HttpResponse};
use actix_ws::AggregatedMessage;
use drillx::Solution;
use futures::StreamExt;
use ore_pool_types::PoolMessage;
use rand::Rng;

use crate::{aggregator::Aggregator, Sessions};

/// Websocket handler for streaming pool updates to clients.
pub async fn ws_handler(
    req: actix_web::HttpRequest,
    stream: web::Payload,
    sessions: web::Data<Sessions>,
    aggregator: web::Data<tokio::sync::RwLock<Aggregator>>,
    solutions_tx: web::Data<tokio::sync::mpsc::UnboundedSender<Solution>>,
) -> Result<HttpResponse, actix_web::Error> {
    // Create websocket
    let (response, mut session, stream) = actix_ws::handle(&req, stream)?;
    let session_id = rand::thread_rng().gen::<u64>();

    // Aggregate continuation frames up to 1KiB
    let mut stream = stream
        .aggregate_continuations()
        .max_continuation_size(2_usize.pow(10));

    log::info!("New session: {}", session_id);

    // Store session
    {
        let mut sessions = sessions.lock().await;
        sessions.insert(session_id, session.clone());
    }

    // Dispatch current challenge to new session
    let r_aggregator = aggregator.read().await;
    let _ = r_aggregator.send_challenge(&mut session).await;
    drop(r_aggregator);

    // Spawn websocket handler
    actix_web::rt::spawn(async move {
        let mut interval = actix_web::rt::time::interval(std::time::Duration::from_secs(1));
        loop {
            tokio::select! {
                // Handle incoming messages
                Some(msg) = stream.next() => {
                    match msg {
                        Ok(AggregatedMessage::Close(_)) => {
                            // Remove session on close
                            let mut sessions = sessions.lock().await;
                            sessions.remove(&session_id);
                            break;
                        }
                        Ok(AggregatedMessage::Ping(msg)) => {
                            if session.pong(&msg).await.is_err() {
                                break;
                            }
                        }
                        Ok(AggregatedMessage::Text(text)) => {
                            match serde_json::from_str::<PoolMessage>(&text) {
                                Ok(pool_msg) => match pool_msg {
                                    PoolMessage::NewSolution { solution } => {
                                        log::info!("Received new solution...");
                                        let _ = solutions_tx.send(solution);
                                    }
                                    PoolMessage::Error { message } => {
                                        log::error!("Received error: {}", message);
                                    }
                                    _ => {
                                        log::error!("Unknown message: {:?}", pool_msg);
                                    }
                                },
                                Err(err) => {
                                    log::error!("Failed to parse message: {}", err);
                                }
                            }
                        }
                        Err(_) => break,
                        _ => {}
                    }
                }

                // Handle keepalive
                _ = interval.tick() => {
                    if session.ping(b"ping").await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    Ok(response)
}
