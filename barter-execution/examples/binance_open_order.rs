use barter_execution::client::binance::{BinanceExecutionClient, BinanceExecutionConfig};

use barter_execution::order::{OrderKind, TimeInForce, request::OrderRequestOpen};

use barter_execution::client::binance::constant::API_V3_BASE_URL;
use barter_instrument::{Side, exchange::ExchangeId, instrument::name::InstrumentNameExchange};
use dotenvy::dotenv;
use rust_decimal::Decimal;
use std::env;
use tokio;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let api_key = env::var("BINANCE_API_KEY").expect("set BINANCE_API_KEY environment variable");
    let api_secret =
        env::var("BINANCE_API_SECRET").expect("set BINANCE_API_SECRET environment variable");
    // 替换为你自己的API Key和Secret
    let config = BinanceExecutionConfig {
        api_key,
        api_secret,
        base_url: API_V3_BASE_URL.to_string(),
        testnet: false,
    };

    let client = BinanceExecutionClient::new(config);

    // 构造一个测试订单
    let instrument = InstrumentNameExchange::new("btcusdt");
    let strategy_id = barter_execution::order::id::StrategyId::new("TestStrategy");
    let cid = barter_execution::order::id::ClientOrderId::new("test123");

    let order_request_open = OrderRequestOpen {
        key: barter_execution::order::OrderKey {
            exchange: ExchangeId::BinanceSpot,
            instrument: &instrument,
            strategy: strategy_id,
            cid,
        },
        state: barter_execution::order::request::RequestOpen {
            side: Side::Buy,
            price: Decimal::from_str_radix("140000.0", 10).unwrap(), // 填写对应价格
            quantity: Decimal::from_str_radix("0.00001", 10).unwrap(), // 填写下单数量
            kind: OrderKind::Limit,
            time_in_force: TimeInForce::GoodUntilCancelled { post_only: true },
        },
    };

    println!("发送现货挂单请求: {:?}", order_request_open);

    let response = client.public_open_order(order_request_open).await;

    match response {
        Some(order) => {
            println!("下单结果: {:#?}", order);
        }
        None => {
            println!("下单失败，未收到返回值");
        }
    }
}
