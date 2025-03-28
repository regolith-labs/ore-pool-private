mod aggregator;
mod error;
mod handlers;
mod operator;
mod submit;
mod utils;

use actix_web::{get, middleware, web, App, HttpResponse, HttpServer, Responder};
use aggregator::Aggregator;
use operator::Operator;
use utils::create_cors;

// TODO: publish attestation to s3
// write attestation url to db with last-hash-at as foreign key
#[actix_web::main]
async fn main() -> Result<(), error::Error> {
    env_logger::init();

    // clock channel
    let (clock_tx, _) = tokio::sync::broadcast::channel::<i64>(1);
    let clock_tx = web::Data::new(clock_tx);

    // operator and aggregator mutex
    let operator = web::Data::new(Operator::new()?);
    let aggregator = web::Data::new(tokio::sync::RwLock::new(Aggregator::new(&operator).await?));

    // process contributions
    tokio::task::spawn({
        let operator = operator.clone();
        let aggregator = aggregator.clone();
        async move {
            if let Err(err) =
                aggregator::process_contributions(aggregator.as_ref(), operator.as_ref()).await
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
            .app_data(operator.clone())
            .app_data(aggregator.clone())
            .service(web::resource("/address").route(web::get().to(handlers::address)))
            .service(web::resource("/challenge").route(web::get().to(handlers::challenge)))
            .service(web::resource("/contribute").route(web::post().to(handlers::contribute)))
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
