use anyhow::Error;
use drillx::Solution;
use futures::{SinkExt, StreamExt};
use ore_pool_types::{Challenge, PoolMessage};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug)]
pub struct Pool {
    pub http_client: reqwest::Client,
    pub pool_url: String,
    pub ws_handle: JoinHandle<()>,
    pub solutions_handle: JoinHandle<()>,
}

impl Pool {
    pub async fn new(
        pool_url: &str,
        challenges_tx: tokio::sync::mpsc::UnboundedSender<Challenge>,
        solutions_rx: tokio::sync::mpsc::UnboundedReceiver<Solution>,
    ) -> Result<Self, Error> {
        println!("Establishing websocket connection...");
        let (ws_handle, solutions_handle) = connect(pool_url, challenges_tx, solutions_rx).await?;

        let pool = Self {
            http_client: reqwest::Client::new(),
            pool_url: pool_url.to_string(),
            ws_handle,
            solutions_handle,
        };

        Ok(pool)
    }
}

async fn connect(
    pool_url: &str,
    challenges_tx: tokio::sync::mpsc::UnboundedSender<Challenge>,
    mut solutions_rx: tokio::sync::mpsc::UnboundedReceiver<Solution>,
) -> Result<(JoinHandle<()>, JoinHandle<()>), Error> {
    let ws_url = pool_url.replace("http", "ws");
    let ws_url = format!("{}/connect", ws_url);

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url).await?;
    let (mut write, read) = ws_stream.split();

    // Handle incoming messages
    let ws_handle = tokio::spawn(async move {
        let mut read = read;
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Ping(_)) => {
                    // Noop
                }
                Ok(Message::Text(msg)) => {
                    match serde_json::from_str::<PoolMessage>(&msg) {
                        Err(e) => println!("Failed to parse message: {}", e),
                        Ok(pool_msg) => match pool_msg {
                            PoolMessage::NewChallenge { challenge } => {
                                if let Err(err) = challenges_tx.send(challenge) {
                                    println!("error sending challenge: {:?}", err);
                                }
                            }
                            PoolMessage::Error { message } => {
                                println!("Received error from pool: {}", message);
                                // Handle error
                            }
                            _ => {
                                println!("Unknown message: {:?}", pool_msg);
                            }
                        },
                    }
                }
                Err(e) => {
                    println!("Error receiving message: {}", e);
                    break;
                }
                _ => {
                    println!("Unknown message: {:?}", msg);
                }
            }
        }
    });

    // Start solution submission loop
    let solutions_handle = tokio::spawn({
        async move {
            while let Some(solution) = solutions_rx.recv().await {
                match write
                    .send(Message::Text(
                        serde_json::to_string(&PoolMessage::NewSolution { solution }).unwrap(),
                    ))
                    .await
                {
                    Ok(_) => println!("Successfully sent to pool"),
                    Err(e) => {
                        println!("Failed to send solution to pool: {}", e);
                        // The websocket connection may have dropped, we should probably handle reconnection here
                        break; // Break the loop so the task ends and we can detect the failure
                    }
                }
            }
        }
    });

    Ok((ws_handle, solutions_handle))
}
