mod aggregator;
mod error;
mod handlers;
mod operator;
mod submit;
mod utils;

use std::{collections::HashMap, sync::Arc};

use actix_web::{get, middleware, web, App, HttpResponse, HttpServer, Responder};
use aggregator::Aggregator;
use drillx::Solution;
use operator::Operator;
use tokio::sync::Mutex;
use utils::create_cors;

// Shared state to track all active websocket sessions
pub type Sessions = Arc<Mutex<HashMap<u64, actix_ws::Session>>>;

#[actix_web::main]
async fn main() -> Result<(), error::Error> {
    env_logger::init();

    // clock channel
    let (clock_tx, _) = tokio::sync::broadcast::channel::<i64>(1);
    let clock_tx = web::Data::new(clock_tx);

    // solution channel
    let (solutions_tx, mut solutions_rx) = tokio::sync::mpsc::unbounded_channel::<Solution>();
    let solutions_tx = web::Data::new(solutions_tx);

    // websocket sessions
    let sessions = web::Data::new(Arc::new(Mutex::new(HashMap::new())));

    // operator and aggregator
    let operator = web::Data::new(Operator::new()?);
    let aggregator = web::Data::new(tokio::sync::RwLock::new(Aggregator::new(&operator).await?));

    // start mining loop
    tokio::task::spawn({
        let operator = operator.clone();
        let aggregator = aggregator.clone();
        let sessions = sessions.clone();
        async move {
            if let Err(err) =
                aggregator::mining_loop(aggregator.as_ref(), operator.as_ref(), sessions.as_ref())
                    .await
            {
                log::error!("{:?}", err);
            }
        }
    });

    // process solutions
    tokio::task::spawn({
        let aggregator = aggregator.clone();
        async move {
            if let Err(err) =
                aggregator::process_solutions(aggregator.as_ref(), &mut solutions_rx).await
            {
                log::error!("{:?}", err);
            }
        }
    });

    // clock
    tokio::task::spawn({
        let operator = operator.clone();
        let clock_tx = clock_tx.clone();
        async move {
            // every 10 seconds fetch the rpc clock
            loop {
                let mut ticks = 0;
                let mut unix_timestamp = match operator.get_clock().await {
                    Ok(clock) => clock.unix_timestamp,
                    Err(err) => {
                        log::error!("{:?}", err);
                        continue;
                    }
                };

                // every 1 seconds simulate a tick
                loop {
                    // reset every 10 ticks
                    ticks += 1;
                    if ticks.eq(&10) {
                        break;
                    }
                    // send tick
                    let _ = clock_tx.send(unix_timestamp);
                    // simulate tick of rpc clock
                    unix_timestamp += 1;
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    });

    // launch server
    HttpServer::new(move || {
        log::info!("starting pool server");
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(create_cors())
            .app_data(clock_tx.clone())
            .app_data(solutions_tx.clone())
            .app_data(operator.clone())
            .app_data(aggregator.clone())
            .app_data(sessions.clone())
            // .service(web::resource("/challenge").route(web::get().to(handlers::challenge)))
            // .service(web::resource("/contribute").route(web::post().to(handlers::contribute)))
            .service(web::resource("/connect").route(web::get().to(handlers::ws_handler)))
            .service(health)
    })
    .bind("0.0.0.0:3000")?
    .run()
    .await
    .map_err(From::from)
}

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().body("ok")
}
