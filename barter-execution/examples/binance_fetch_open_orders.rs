use barter_execution::client::binance::{BinanceExecutionClient, BinanceExecutionConfig};
use barter_execution::client::binance::constant::API_V3_BASE_URL;
use barter_instrument::instrument::name::InstrumentNameExchange;
use std::env;
use dotenvy::dotenv;

#[tokio::main]
async fn main() {
    dotenv().ok();
    // read api key and secret from environment variables
    let api_key = env::var("BINANCE_API_KEY").expect("set BINANCE_API_KEY environment variable");
    let api_secret = env::var("BINANCE_API_SECRET").expect("set BINANCE_API_SECRET environment variable");

    // initialize BinanceExecutionConfig
    let config = BinanceExecutionConfig {
        api_key,
        api_secret,
        base_url: API_V3_BASE_URL.to_string(),
        testnet: false,
    };

    // create BinanceExecutionClient
    let client = BinanceExecutionClient::new(config);

    // set test instrument
    let symbol = InstrumentNameExchange::new("btcusdt".to_string());

    println!("request Binance open orders...");
    match client.public_fetch_open_orders(&[symbol]).await {
        Ok(orders) => println!("get open orders count: {}", orders.len()),
        Err(e) => eprintln!("get open orders error: {:?}", e),
    }
}
