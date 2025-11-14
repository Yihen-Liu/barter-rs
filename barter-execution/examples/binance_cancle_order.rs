use barter_execution::client::binance::{BinanceExecutionClient, BinanceExecutionConfig};
use barter_execution::order::{
    request::OrderRequestCancel, id::{StrategyId, ClientOrderId, OrderId}, OrderKey,
};
use barter_execution::client::binance::constant::API_V3_BASE_URL;
use barter_instrument::{exchange::ExchangeId, instrument::name::InstrumentNameExchange};
use dotenvy::dotenv;
use std::env;
use tokio;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let api_key = env::var("BINANCE_API_KEY").expect("set BINANCE_API_KEY environment variable");
    let api_secret = env::var("BINANCE_API_SECRET").expect("set BINANCE_API_SECRET environment variable");
    
    let config = BinanceExecutionConfig {
        api_key,
        api_secret,
        base_url: API_V3_BASE_URL.to_string(),
        testnet: false,
    };

    let client = BinanceExecutionClient::new(config);

    // 构造撤单所需参数
    let instrument = InstrumentNameExchange::new("btcusdt");
    let strategy_id = StrategyId::new("TestStrategy");
    let cid = ClientOrderId::new("test123");
    // 提供你需要撤销的订单ID
    let order_id = Some(OrderId::new("123456789")); // TODO: 替换为实际订单ID

    let order_request_cancel = OrderRequestCancel {
        key: OrderKey {
            exchange: ExchangeId::BinanceSpot,
            instrument: &instrument,
            strategy: strategy_id,
            cid,
        },
        state: barter_execution::order::request::RequestCancel {
            id: order_id,
        },
    };

    println!("发送现货撤单请求: {:?}", order_request_cancel);

    let response = client.public_cancel_order(order_request_cancel).await;

    match response {
        Some(cancel_result) => {
            println!("撤单结果: {:#?}", cancel_result);
        }
        None => {
            println!("撤单失败，未收到返回值");
        }
    }
}
