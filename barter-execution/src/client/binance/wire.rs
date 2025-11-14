use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct BinanceAccountResponse {
    pub balances: Vec<BinanceBalance>,
}

#[derive(Deserialize, Debug)]
pub struct BinanceBalance {
    pub asset: String,
    pub free: String,
    pub locked: String,
}

#[derive(Deserialize, Debug)]
pub struct BinanceOrderResponse {
    pub order_id: i64,
    pub symbol: String,
    pub status: String,
    #[serde(rename = "type")]
    pub order_type: String,
    pub side: String,
    pub price: String,
    pub executed_qty: String,
    pub orig_qty: String,
    pub time: i64,
}

#[derive(Deserialize, Debug)]
pub struct BinanceTradeResponse {
    pub id: i64,
    pub order_id: i64,
    pub symbol: String,
    pub price: String,
    pub qty: String,
    pub quote_qty: String,
    pub commission: String,
    pub commission_asset: String,
    pub time: i64,
    pub is_buyer: bool,
}

#[derive(Deserialize, Debug)]
pub struct BinanceOpenOrder {
    pub order_id: i64,
    pub symbol: String,
    pub price: String,
    pub orig_qty: String,
    pub executed_qty: String,
    pub status: String,
    pub side: String,
    #[serde(rename = "type")]
    pub order_type: String,
    pub time: i64,
}
#[derive(Deserialize,Debug)]
pub struct UserDataStreamMessage {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: Option<String>,
    #[serde(rename = "i")]
    pub order_id: Option<i64>,
    #[serde(rename = "c")]
    pub client_order_id: Option<String>,
    #[serde(rename = "S")]
    pub side: Option<String>,
    #[serde(rename = "o")]
    pub order_type: Option<String>,
    #[serde(rename = "f")]
    pub time_in_force: Option<String>,
    #[serde(rename = "q")]
    pub quantity: Option<String>,
    #[serde(rename = "p")]
    pub price: Option<String>,
    #[serde(rename = "x")]
    pub execution_type: Option<String>,
    #[serde(rename = "X")]
    pub order_status: Option<String>,
    #[serde(rename = "z")]
    pub cumulative_filled_quantity: Option<String>,
    #[serde(rename = "L")]
    pub last_executed_price: Option<String>,
    #[serde(rename = "l")]
    pub last_executed_quantity: Option<String>,
    #[serde(rename = "n")]
    pub commission: Option<String>,
    #[serde(rename = "N")]
    pub commission_asset: Option<String>,
    #[serde(rename = "T")]
    pub trade_time: Option<i64>,
}
