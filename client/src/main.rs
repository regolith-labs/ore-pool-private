mod miner;
mod pool;

use anyhow::Error;
pub use miner::Miner;

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Launch miner
    let pool_url = "http://localhost:3000";
    let _miner = Miner::launch(pool_url).await?;

    // Keep main thread alive
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
