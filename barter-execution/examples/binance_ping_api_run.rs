use barter_execution::client::binance::{
    BinanceExecutionClient, BinanceExecutionConfig, 
    constant::API_V3_BASE_URL,
};
use tokio;

#[tokio::main]
async fn main() {
    let config = BinanceExecutionConfig {
        api_key: "".to_string(), // dont need api key for ping
        api_secret: "".to_string(),
        base_url: API_V3_BASE_URL.to_string(),
        testnet: false,
    };

    let client = BinanceExecutionClient::new(config);

    println!("Testing Binance /api/v3/ping ...");
    match client.ping().await {
        Ok(_) => println!("Binance ping API: Success!"),
        Err(e) => eprintln!("Binance ping API: Failed! Error: {:?}", e),
    }
}
