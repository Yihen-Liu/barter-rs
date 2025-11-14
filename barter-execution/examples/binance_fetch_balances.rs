use barter_execution::client::binance::{
    BinanceExecutionClient, BinanceExecutionConfig, constant::API_V3_BASE_URL,
};
use barter_instrument::asset::name::AssetNameExchange;
use std::env;
use dotenvy::dotenv;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let api_key = env::var("BINANCE_API_KEY").expect("set BINANCE_API_KEY environment variable");
    let api_secret =
        env::var("BINANCE_API_SECRET").expect("set BINANCE_API_SECRET environment variable");

    let config = BinanceExecutionConfig {
        api_key,
        api_secret,
        base_url: API_V3_BASE_URL.to_string(),
        testnet: false,
    };

    let client = BinanceExecutionClient::new(config);

    // you can manually specify the assets to query
    let assets = [
        AssetNameExchange::new("USDT"),
        AssetNameExchange::new("USDC"),
        AssetNameExchange::new("BTC"),
        AssetNameExchange::new("ETH"),
    ];

    println!("Fetching balances for assets: USDT, USDC, BTC, ETH");
    match client.public_fetch_balances(&assets).await {
        Ok(balances) => {
            println!("Balances fetch successful!");
            for balance in balances {
                println!(
                    "{}: free={}, locked={}",
                    balance.asset.to_string(),
                    balance.balance.free,
                    balance.balance.total
                );
            }
        }
        Err(e) => {
            eprintln!("Failed to fetch balances: {:?}", e);
        }
    }
}
