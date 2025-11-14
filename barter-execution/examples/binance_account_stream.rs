use barter_execution::client::binance::{BinanceExecutionClient, BinanceExecutionConfig};
use barter_execution::client::binance::constant::API_V3_BASE_URL;
use barter_instrument::instrument::name::InstrumentNameExchange;
use barter_instrument::asset::name::AssetNameExchange;
use dotenvy::dotenv;
use std::env;
//use futures::StreamExt;

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

    println!("连接 Binance account_stream...");

    match client.public_account_stream(&assets, &instruments).await {
        Ok(mut stream) => {
            println!("已连接到 account_stream，等待事件...");

            // 仅展示前10条事件，防止示例阻塞过久
            // let mut count = 0;
            // while let Some(event) = stream.next().await {
            //     println!("收到账户事件: {:?}", event);
            //     count += 1;
            //     if count >= 10 {
            //         println!("已接收10条事件，退出！");
            //         break;
            //     }
            // }
        }
        Err(e) => {
            eprintln!("account_stream 连接异常: {:?}", e);
        }
    }
}
