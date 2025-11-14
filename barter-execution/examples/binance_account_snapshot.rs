use barter_execution::client::binance::{BinanceExecutionClient, BinanceExecutionConfig};
use barter_execution::client::binance::constant::API_V3_BASE_URL;
use barter_instrument::instrument::name::InstrumentNameExchange;
use barter_instrument::asset::name::AssetNameExchange;
use std::env;
use dotenvy::dotenv;

#[tokio::main]
async fn main() {
    dotenv().ok();
    // 从环境变量读取API Key和Secret
    let api_key = env::var("BINANCE_API_KEY").expect("请设置环境变量 BINANCE_API_KEY");
    let api_secret = env::var("BINANCE_API_SECRET").expect("请设置环境变量 BINANCE_API_SECRET");

    // 配置 BinanceExecutionConfig
    let config = BinanceExecutionConfig {
        api_key,
        api_secret,
        base_url: API_V3_BASE_URL.to_string(),
        testnet: false,
    };

    // 创建 BinanceExecutionClient 实例
    let client = BinanceExecutionClient::new(config);

    // 示例，定制资产和交易对
    let assets = vec![
        AssetNameExchange::new("usdt".to_string()),
        AssetNameExchange::new("usdc".to_string()),
        AssetNameExchange::new("btc".to_string()),
        AssetNameExchange::new("eth".to_string()),
    ];
    let instruments = vec![
        InstrumentNameExchange::new("btcusdt".to_string()),
        InstrumentNameExchange::new("ethusdt".to_string()),
    ];

    println!("请求 Binance account_snapshot 接口...");
    match client.public_account_snapshot(&assets, &instruments).await {
        Ok(account_snapshot) => {
            println!("账户持有币种:");
            for balance in &account_snapshot.balances {
                println!(
                    "- {}: free={}, total={}",
                    balance.asset,
                    balance.balance.free,
                    balance.balance.total
                );
            }
            println!("\n当前挂单:");
            for order in &account_snapshot.instruments {
                println!("- {}: orders={}", order.instrument, order.orders.len());
            }
        }
        Err(e) => {
            eprintln!("account_snapshot 接口调用出错: {:?}", e);
        }
    }
}
