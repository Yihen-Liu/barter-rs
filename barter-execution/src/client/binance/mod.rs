use crate::order::OrderKey;
use crate::{
    UnindexedAccountEvent, UnindexedAccountSnapshot,
    balance::AssetBalance,
    client::ExecutionClient,
    error::{ApiError, ConnectivityError, UnindexedClientError, UnindexedOrderError},
    order::{
        Order,
        id::{ClientOrderId, OrderId},
        request::{OrderRequestCancel, OrderRequestOpen, UnindexedOrderResponseCancel},
        state::{Cancelled, Open},
    },
    trade::{AssetFees, Trade, TradeId},
};
use barter_instrument::{
    Side,
    asset::{QuoteAsset, name::AssetNameExchange},
    exchange::ExchangeId,
    instrument::name::InstrumentNameExchange,
};
use barter_integration::error::SocketError;
use chrono::{DateTime, Utc};
use derive_more::Constructor;
use futures::stream::BoxStream;
use hmac::{Hmac, Mac};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{error, info, warn};

type HmacSha256 = Hmac<Sha256>;
pub mod wire;
use crate::client::binance::wire::{
    BinanceAccountResponse, BinanceOpenOrder, BinanceOrderResponse, BinanceTradeResponse,
    UserDataStreamMessage,
};
pub mod constant;
/// Binance Spot execution client configuration.
#[derive(
    Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Deserialize, Serialize, Constructor,
)]
pub struct BinanceExecutionConfig {
    /// Binance API key
    pub api_key: String,
    /// Binance API secret
    pub api_secret: String,
    /// Base URL for Binance API (default: "https://api.binance.com")
    pub base_url: String,
    /// Whether to use testnet (default: false)
    pub testnet: bool,
}

impl Default for BinanceExecutionConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_secret: String::new(),
            base_url: constant::API_V3_BASE_URL.to_string(),
            testnet: false,
        }
    }
}

/// Binance Spot execution client.
#[derive(Debug, Clone)]
pub struct BinanceExecutionClient {
    config: BinanceExecutionConfig,
    http_client: reqwest::Client,
    listen_key: Arc<RwLock<Option<String>>>,
}

impl BinanceExecutionClient {
    /// Create a new Binance execution client.
    pub fn new(config: BinanceExecutionConfig) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            listen_key: Arc::new(RwLock::new(None)),
            config,
        }
    }

    /// Ping the Binance API to check if the connection is alive.
    pub async fn ping(&self) -> Result<(), UnindexedClientError> {
        let url = format!("{}{}", self.config.base_url, constant::API_V3_PING);
        let response = self.http_client.get(&url).send().await.map_err(|e| {
            UnindexedClientError::Connectivity(ConnectivityError::Socket(
                SocketError::from(e).to_string(),
            ))
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|e| {
            UnindexedClientError::Connectivity(ConnectivityError::Socket(
                SocketError::from(e).to_string(),
            ))
        })?;
        if !status.is_success() {
            return Err(UnindexedClientError::Api(ApiError::OrderRejected(format!(
                "HTTP {}: {}",
                status, body
            ))));
        }
        Ok(())
    }

    /// Generate HMAC-SHA256 signature for Binance API requests.
    fn sign(&self, query_string: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.config.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(query_string.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Build signed query string with timestamp and signature.
    fn build_signed_query(&self, mut params: HashMap<String, String>) -> String {
        params.insert(
            "timestamp".to_string(),
            Utc::now().timestamp_millis().to_string(),
        );
        params.insert("recvWindow".to_string(), "5000".to_string());

        let mut query_parts: Vec<String> =
            params.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        query_parts.sort();

        let query_string = query_parts.join("&");
        let signature = self.sign(&query_string);
        format!("{}&signature={}", query_string, signature)
    }

    /// Make a signed GET request to Binance API.
    async fn get_signed(
        &self,
        endpoint: &str,
        params: HashMap<String, String>,
    ) -> Result<String, UnindexedClientError> {
        let query_string = self.build_signed_query(params);
        let url = format!("{}{}?{}", self.config.base_url, endpoint, query_string);

        let response = self
            .http_client
            .get(&url)
            .header("X-MBX-APIKEY", &self.config.api_key)
            .send()
            .await
            .map_err(|e| {
                UnindexedClientError::Connectivity(ConnectivityError::Socket(
                    SocketError::from(e).to_string(),
                ))
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            UnindexedClientError::Connectivity(ConnectivityError::Socket(
                SocketError::from(e).to_string(),
            ))
        })?;

        if !status.is_success() {
            return Err(UnindexedClientError::Api(ApiError::OrderRejected(format!(
                "HTTP {}: {}",
                status, body
            ))));
        }

        Ok(body)
    }

    /// Make a signed POST request to Binance API.
    async fn post_signed(
        &self,
        endpoint: &str,
        params: HashMap<String, String>,
    ) -> Result<String, UnindexedClientError> {
        let query_string = self.build_signed_query(params.clone());
        let url = format!("{}{}?{}", self.config.base_url, endpoint, query_string);

        let response = self
            .http_client
            .post(&url)
            .header("X-MBX-APIKEY", &self.config.api_key)
            .send()
            .await
            .map_err(|e| {
                UnindexedClientError::Connectivity(ConnectivityError::Socket(
                    SocketError::from(e).to_string(),
                ))
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            UnindexedClientError::Connectivity(ConnectivityError::Socket(
                SocketError::from(e).to_string(),
            ))
        })?;

        if !status.is_success() {
            return Err(UnindexedClientError::Api(ApiError::OrderRejected(format!(
                "HTTP {}: {}",
                status, body
            ))));
        }

        Ok(body)
    }

    /// Make a signed DELETE request to Binance API.
    async fn delete_signed(
        &self,
        endpoint: &str,
        params: HashMap<String, String>,
    ) -> Result<String, UnindexedClientError> {
        let query_string = self.build_signed_query(params.clone());
        let url = format!("{}{}?{}", self.config.base_url, endpoint, query_string);

        let response = self
            .http_client
            .delete(&url)
            .header("X-MBX-APIKEY", &self.config.api_key)
            .send()
            .await
            .map_err(|e| {
                UnindexedClientError::Connectivity(ConnectivityError::Socket(
                    SocketError::from(e).to_string(),
                ))
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            UnindexedClientError::Connectivity(ConnectivityError::Socket(
                SocketError::from(e).to_string(),
            ))
        })?;

        if !status.is_success() {
            return Err(UnindexedClientError::Api(ApiError::OrderRejected(format!(
                "HTTP {}: {}",
                status, body
            ))));
        }

        Ok(body)
    }

    /// Create or refresh user data stream listen key.
    async fn get_listen_key(&self) -> Result<String, UnindexedClientError> {
        let url = format!(
            "{}{}",
            self.config.base_url,
            constant::API_V3_USER_DATA_STREAM
        );

        let response = self
            .http_client
            .post(&url)
            .header("X-MBX-APIKEY", &self.config.api_key)
            .send()
            .await
            .map_err(|e| {
                UnindexedClientError::Connectivity(ConnectivityError::Socket(
                    SocketError::from(e).to_string(),
                ))
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            UnindexedClientError::Connectivity(ConnectivityError::Socket(
                SocketError::from(e).to_string(),
            ))
        })?;

        if !status.is_success() {
            return Err(UnindexedClientError::Api(ApiError::OrderRejected(format!(
                "HTTP {}: {}",
                status, body
            ))));
        }

        #[derive(Deserialize)]
        struct ListenKeyResponse {
            listen_key: String,
        }

        let response: ListenKeyResponse = serde_json::from_str(&body).map_err(|e| {
            UnindexedClientError::Api(ApiError::OrderRejected(format!(
                "Failed to parse listen key response: {}",
                e
            )))
        })?;

        Ok(response.listen_key)
    }

    /// Parse Binance account response.
    fn parse_account_response(
        &self,
        body: &str,
        instruments: &[InstrumentNameExchange],
    ) -> Result<UnindexedAccountSnapshot, UnindexedClientError> {
        let account: BinanceAccountResponse = serde_json::from_str(body).map_err(|e| {
            UnindexedClientError::AccountSnapshot(format!("Failed to parse account: {}", e))
        })?;

        let now = Utc::now();
        let balances: Vec<AssetBalance<AssetNameExchange>> = account
            .balances
            .into_iter()
            .filter_map(|b| {
                let free: Decimal = b.free.parse().ok()?;
                let locked: Decimal = b.locked.parse().ok()?;
                let total = free + locked;

                if total > Decimal::ZERO {
                    Some(AssetBalance {
                        asset: AssetNameExchange::new(b.asset.to_lowercase()),
                        balance: crate::balance::Balance::new(total, free),
                        time_exchange: now,
                    })
                } else {
                    None
                }
            })
            .collect();

        let instrument_snapshots = instruments
            .iter()
            .map(|instrument| crate::InstrumentAccountSnapshot {
                instrument: instrument.clone(),
                orders: vec![],
            })
            .collect();

        Ok(UnindexedAccountSnapshot {
            exchange: ExchangeId::BinanceSpot,
            balances,
            instruments: instrument_snapshots,
        })
    }

    /// Parse Binance order response.
    fn parse_order_response(
        &self,
        body: &str,
        request: OrderRequestOpen<ExchangeId, InstrumentNameExchange>,
    ) -> Result<
        Order<ExchangeId, InstrumentNameExchange, Result<Open, UnindexedOrderError>>,
        UnindexedClientError,
    > {
        let order: BinanceOrderResponse = serde_json::from_str(body).map_err(|e| {
            UnindexedClientError::Api(ApiError::OrderRejected(format!(
                "Failed to parse order response: {}",
                e
            )))
        })?;

        let order_id = OrderId::new(order.order_id.to_string());
        let filled_quantity: Decimal = order.executed_qty.parse().unwrap_or_default();
        let time_exchange = DateTime::from_timestamp_millis(order.time).unwrap_or_else(Utc::now);

        let state = if order.status == "FILLED" {
            Ok(Open {
                id: order_id,
                time_exchange,
                filled_quantity,
            })
        } else if order.status == "NEW" || order.status == "PARTIALLY_FILLED" {
            Ok(Open {
                id: order_id,
                time_exchange,
                filled_quantity,
            })
        } else {
            Err(UnindexedOrderError::Rejected(ApiError::OrderRejected(
                format!("Order status: {}", order.status),
            )))
        };

        Ok(Order {
            key: request.key,
            side: request.state.side,
            price: request.state.price,
            quantity: request.state.quantity,
            kind: request.state.kind,
            time_in_force: request.state.time_in_force,
            state,
        })
    }

    /// Parse Binance trade response.
    fn parse_trades_response(
        &self,
        body: &str,
        time_since: DateTime<Utc>,
    ) -> Result<Vec<Trade<QuoteAsset, InstrumentNameExchange>>, UnindexedClientError> {
        let trades: Vec<BinanceTradeResponse> = serde_json::from_str(body).map_err(|e| {
            UnindexedClientError::Api(ApiError::OrderRejected(format!(
                "Failed to parse trades response: {}",
                e
            )))
        })?;

        let mut result = Vec::new();
        for trade in trades {
            let time_exchange =
                DateTime::from_timestamp_millis(trade.time).unwrap_or_else(Utc::now);
            if time_exchange < time_since {
                continue;
            }

            let price: Decimal = trade.price.parse().unwrap_or_default();
            let quantity: Decimal = trade.qty.parse().unwrap_or_default();
            let commission: Decimal = trade.commission.parse().unwrap_or_default();

            result.push(Trade {
                id: TradeId::new(trade.id.to_string()),
                order_id: OrderId::new(trade.order_id.to_string()),
                instrument: InstrumentNameExchange::new(trade.symbol),
                strategy: crate::order::id::StrategyId::new("BinanceExecution"),
                time_exchange,
                side: if trade.is_buyer {
                    Side::Buy
                } else {
                    Side::Sell
                },
                price,
                quantity: if trade.is_buyer { quantity } else { -quantity },
                fees: AssetFees {
                    asset: QuoteAsset,
                    fees: commission,
                },
            });
        }

        Ok(result)
    }
}

impl ExecutionClient for BinanceExecutionClient {
    const EXCHANGE: ExchangeId = ExchangeId::BinanceSpot;
    type Config = BinanceExecutionConfig;
    type AccountStream = BoxStream<'static, UnindexedAccountEvent>;

    fn new(config: Self::Config) -> Self {
        Self::new(config)
    }

    async fn account_snapshot(
        &self,
        _assets: &[AssetNameExchange],
        instruments: &[InstrumentNameExchange],
    ) -> Result<UnindexedAccountSnapshot, UnindexedClientError> {
        let params = HashMap::new();
        let body = self.get_signed(constant::API_V3_ACCOUNT, params).await?;
        self.parse_account_response(&body, instruments)
    }

    async fn account_stream(
        &self,
        _assets: &[AssetNameExchange],
        _instruments: &[InstrumentNameExchange],
    ) -> Result<Self::AccountStream, UnindexedClientError> {
        let listen_key = self.get_listen_key().await?;
        *self.listen_key.write().await = Some(listen_key.clone());

        let ws_url = format!("{}/{}", constant::WSS_STREAM_BASE_URL, listen_key);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        // Spawn WebSocket task
        let client_clone = self.clone();
        tokio::spawn(async move {
            loop {
                match tokio_tungstenite::connect_async(&ws_url).await {
                    Ok((ws_stream, _)) => {
                        use futures::StreamExt;
                        let (mut _write, mut read) = ws_stream.split();
                        info!("Binance account stream connected");

                        while let Some(msg) = read.next().await {
                            match msg {
                                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                                    // Parse user data stream message
                                    // Binance user data stream format:
                                    // {
                                    //   "e": "executionReport",
                                    //   "E": 123456789,
                                    //   "s": "BTCUSDT",
                                    //   ...
                                    // }
                                    if let Err(e) =
                                        client_clone.parse_user_data_stream(&text, &tx).await
                                    {
                                        warn!("Failed to parse user data stream: {}", e);
                                    }
                                }
                                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                                    warn!("Binance account stream closed");
                                    break;
                                }
                                Err(e) => {
                                    error!("Binance account stream error: {}", e);
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to connect to Binance account stream: {}", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok(Box::pin(UnboundedReceiverStream::new(rx)))
    }

    async fn cancel_order(
        &self,
        request: OrderRequestCancel<ExchangeId, &InstrumentNameExchange>,
    ) -> Option<UnindexedOrderResponseCancel> {
        let mut params = HashMap::new();
        params.insert(
            "symbol".to_string(),
            request.key.instrument.to_string().to_uppercase(),
        );

        if let Some(order_id) = &request.state.id {
            params.insert("orderId".to_string(), order_id.to_string());
        } else {
            warn!("Cancel order request missing order ID");
            return Some(UnindexedOrderResponseCancel {
                key: OrderKey {
                    exchange: request.key.exchange,
                    instrument: request.key.instrument.clone(),
                    strategy: request.key.strategy.clone(),
                    cid: request.key.cid.clone(),
                },
                state: Err(UnindexedOrderError::Rejected(ApiError::OrderRejected(
                    "Missing order ID".to_string(),
                ))),
            });
        }

        match self.delete_signed(constant::API_V3_ORDER, params).await {
            Ok(_) => Some(UnindexedOrderResponseCancel {
                key: OrderKey {
                    exchange: request.key.exchange,
                    instrument: request.key.instrument.clone(),
                    strategy: request.key.strategy.clone(),
                    cid: request.key.cid.clone(),
                },
                state: Ok(Cancelled {
                    id: request.state.id.clone().unwrap(),
                    time_exchange: Utc::now(),
                }),
            }),
            Err(e) => Some(UnindexedOrderResponseCancel {
                key: OrderKey {
                    exchange: request.key.exchange,
                    instrument: request.key.instrument.clone(),
                    strategy: request.key.strategy.clone(),
                    cid: request.key.cid.clone(),
                },
                state: Err(match e {
                    UnindexedClientError::Connectivity(ce) => UnindexedOrderError::Connectivity(ce),
                    UnindexedClientError::Api(ae) => UnindexedOrderError::Rejected(ae),
                    _ => UnindexedOrderError::Rejected(ApiError::OrderRejected(e.to_string())),
                }),
            }),
        }
    }

    async fn open_order(
        &self,
        request: OrderRequestOpen<ExchangeId, &InstrumentNameExchange>,
    ) -> Option<Order<ExchangeId, InstrumentNameExchange, Result<Open, UnindexedOrderError>>> {
        let mut params = HashMap::new();
        params.insert(
            "symbol".to_string(),
            request.key.instrument.to_string().to_uppercase(),
        );
        params.insert(
            "side".to_string(),
            match request.state.side {
                Side::Buy => "BUY".to_string(),
                Side::Sell => "SELL".to_string(),
            },
        );
        params.insert(
            "type".to_string(),
            match request.state.kind {
                crate::order::OrderKind::Market => "MARKET".to_string(),
                crate::order::OrderKind::Limit => "LIMIT".to_string(),
            },
        );
        params.insert("quantity".to_string(), request.state.quantity.to_string());

        if matches!(request.state.kind, crate::order::OrderKind::Limit) {
            params.insert("price".to_string(), request.state.price.to_string());
            params.insert(
                "timeInForce".to_string(),
                match request.state.time_in_force {
                    crate::order::TimeInForce::GoodUntilCancelled { .. } => "GTC".to_string(),
                    crate::order::TimeInForce::ImmediateOrCancel => "IOC".to_string(),
                    crate::order::TimeInForce::FillOrKill => "FOK".to_string(),
                    crate::order::TimeInForce::GoodUntilEndOfDay => "GTD".to_string(),
                },
            );
        }

        let owned_request = OrderRequestOpen {
            key: OrderKey {
                exchange: request.key.exchange,
                instrument: request.key.instrument.clone(),
                strategy: request.key.strategy.clone(),
                cid: request.key.cid.clone(),
            },
            state: request.state.clone(),
        };

        match self.post_signed(constant::API_V3_ORDER, params).await {
            Ok(body) => match self.parse_order_response(&body, owned_request.clone()) {
                Ok(order) => Some(order),
                Err(e) => Some(Order {
                    key: owned_request.key,
                    side: owned_request.state.side,
                    price: owned_request.state.price,
                    quantity: owned_request.state.quantity,
                    kind: owned_request.state.kind,
                    time_in_force: owned_request.state.time_in_force,
                    state: Err(match e {
                        UnindexedClientError::Connectivity(ce) => {
                            UnindexedOrderError::Connectivity(ce)
                        }
                        UnindexedClientError::Api(ae) => UnindexedOrderError::Rejected(ae),
                        _ => UnindexedOrderError::Rejected(ApiError::OrderRejected(e.to_string())),
                    }),
                }),
            },
            Err(e) => Some(Order {
                key: owned_request.key,
                side: owned_request.state.side,
                price: owned_request.state.price,
                quantity: owned_request.state.quantity,
                kind: owned_request.state.kind,
                time_in_force: owned_request.state.time_in_force,
                state: Err(match e {
                    UnindexedClientError::Connectivity(ce) => UnindexedOrderError::Connectivity(ce),
                    UnindexedClientError::Api(ae) => UnindexedOrderError::Rejected(ae),
                    _ => UnindexedOrderError::Rejected(ApiError::OrderRejected(e.to_string())),
                }),
            }),
        }
    }

    async fn fetch_balances(
        &self,
        _assets: &[AssetNameExchange],
    ) -> Result<Vec<AssetBalance<AssetNameExchange>>, UnindexedClientError> {
        let snapshot = self.account_snapshot(_assets, &[]).await?;
        Ok(snapshot.balances)
    }

    async fn fetch_open_orders(
        &self,
        instruments: &[InstrumentNameExchange],
    ) -> Result<Vec<Order<ExchangeId, InstrumentNameExchange, Open>>, UnindexedClientError> {
        let mut all_orders = Vec::new();

        for instrument in instruments {
            let mut params = HashMap::new();
            params.insert("symbol".to_string(), instrument.to_string().to_uppercase());

            let body = self
                .get_signed(constant::API_V3_OPEN_ORDERS, params)
                .await?;

            let orders: Vec<BinanceOpenOrder> = serde_json::from_str(&body).map_err(|e| {
                UnindexedClientError::Api(ApiError::OrderRejected(format!(
                    "Failed to parse open orders: {}",
                    e
                )))
            })?;

            for order in orders {
                if order.status == "NEW" || order.status == "PARTIALLY_FILLED" {
                    let price: Decimal = order.price.parse().unwrap_or_default();
                    let quantity: Decimal = order.orig_qty.parse().unwrap_or_default();
                    let filled_quantity: Decimal = order.executed_qty.parse().unwrap_or_default();
                    let time_exchange =
                        DateTime::from_timestamp_millis(order.time).unwrap_or_else(Utc::now);

                    all_orders.push(Order {
                        key: OrderKey {
                            exchange: ExchangeId::BinanceSpot,
                            instrument: instrument.clone(),
                            strategy: crate::order::id::StrategyId::new("BinanceExecution"),
                            cid: ClientOrderId::new(order.order_id.to_string()),
                        },
                        side: if order.side == "BUY" {
                            Side::Buy
                        } else {
                            Side::Sell
                        },
                        price,
                        quantity,
                        kind: if order.order_type == "MARKET" {
                            crate::order::OrderKind::Market
                        } else {
                            crate::order::OrderKind::Limit
                        },
                        time_in_force: crate::order::TimeInForce::GoodUntilCancelled {
                            post_only: false,
                        },
                        state: Open {
                            id: OrderId::new(order.order_id.to_string()),
                            time_exchange,
                            filled_quantity,
                        },
                    });
                }
            }
        }

        Ok(all_orders)
    }

    async fn fetch_trades(
        &self,
        time_since: DateTime<Utc>,
    ) -> Result<Vec<Trade<QuoteAsset, InstrumentNameExchange>>, UnindexedClientError> {
        // Note: Binance API requires a symbol parameter, so we fetch all symbols
        // This is a simplified implementation - in practice, you'd want to track which symbols
        // you're interested in
        let mut params = HashMap::new();
        params.insert("limit".to_string(), "1000".to_string());
        params.insert(
            "startTime".to_string(),
            time_since.timestamp_millis().to_string(),
        );

        // This endpoint requires a symbol, so we can't fetch all trades without knowing symbols
        // For now, return empty - in practice, you'd need to call this per symbol
        Ok(vec![])
    }
}

impl BinanceExecutionClient {
    /// Parse Binance user data stream message and convert to AccountEvent.
    async fn parse_user_data_stream(
        &self,
        text: &str,
        tx: &tokio::sync::mpsc::UnboundedSender<UnindexedAccountEvent>,
    ) -> Result<(), UnindexedClientError> {
        let msg: UserDataStreamMessage = serde_json::from_str(text).map_err(|e| {
            UnindexedClientError::AccountStream(format!("Failed to parse user data stream: {}", e))
        })?;

        match msg.event_type.as_str() {
            "executionReport" => {
                if let Some(status) = &msg.order_status {
                    match status.as_str() {
                        "FILLED" | "PARTIALLY_FILLED" => {
                            if let (
                                Some(symbol),
                                Some(price_str),
                                Some(qty_str),
                                Some(trade_time),
                            ) = (
                                &msg.symbol,
                                &msg.last_executed_price,
                                &msg.last_executed_quantity,
                                msg.trade_time,
                            ) {
                                let price: Decimal = price_str.parse().unwrap_or_default();
                                let quantity: Decimal = qty_str.parse().unwrap_or_default();
                                let side = if msg.side.as_deref() == Some("BUY") {
                                    Side::Buy
                                } else {
                                    Side::Sell
                                };

                                let trade = Trade {
                                    id: TradeId::new(format!("{}", trade_time)),
                                    order_id: OrderId::new(msg.order_id.unwrap_or(0).to_string()),
                                    instrument: InstrumentNameExchange::new(symbol.clone()),
                                    strategy: crate::order::id::StrategyId::new("BinanceExecution"),
                                    time_exchange: DateTime::from_timestamp_millis(trade_time)
                                        .unwrap_or_else(Utc::now),
                                    side,
                                    price,
                                    quantity: if side == Side::Buy {
                                        quantity
                                    } else {
                                        -quantity
                                    },
                                    fees: AssetFees {
                                        asset: QuoteAsset,
                                        fees: msg
                                            .commission
                                            .as_ref()
                                            .and_then(|c| c.parse().ok())
                                            .unwrap_or_default(),
                                    },
                                };

                                let _ = tx.send(UnindexedAccountEvent::new(
                                    ExchangeId::BinanceSpot,
                                    trade,
                                ));
                            }
                        }
                        "CANCELED" => {
                            // Handle order cancellation
                            // Implementation depends on your needs
                        }
                        _ => {}
                    }
                }
            }
            "outboundAccountPosition" => {
                // Handle balance updates
                // Implementation depends on your needs
            }
            _ => {}
        }

        Ok(())
    }
}

impl BinanceExecutionClient {
    pub async fn public_account_snapshot(
        &self,
        assets: &[AssetNameExchange],
        instruments: &[InstrumentNameExchange],
    ) -> Result<UnindexedAccountSnapshot, UnindexedClientError> {
        return self.account_snapshot(assets, instruments).await;
    }

    pub async fn public_account_stream(
        &self,
        assets: &[AssetNameExchange],
        instruments: &[InstrumentNameExchange],
    ) -> Result<BoxStream<'static, UnindexedAccountEvent>, UnindexedClientError> {
        return self.account_stream(assets, instruments).await;
    }

    pub async fn public_fetch_open_orders(
        &self,
        instruments: &[InstrumentNameExchange],
    ) -> Result<Vec<Order<ExchangeId, InstrumentNameExchange, Open>>, UnindexedClientError> {
        return self.fetch_open_orders(instruments).await;
    }

    pub async fn public_fetch_trades(
        &self,
        time_since: DateTime<Utc>,
    ) -> Result<Vec<Trade<QuoteAsset, InstrumentNameExchange>>, UnindexedClientError> {
        return self.fetch_trades(time_since).await;
    }

    pub async fn public_fetch_account(
        &self,
        assets: &[AssetNameExchange],
        instruments: &[InstrumentNameExchange],
    ) -> Result<UnindexedAccountSnapshot, UnindexedClientError> {
        return self.account_snapshot(assets, instruments).await;
    }

    pub async fn public_fetch_balances(
        &self,
        assets: &[AssetNameExchange],
    ) -> Result<Vec<AssetBalance<AssetNameExchange>>, UnindexedClientError> {
        return self.fetch_balances(assets).await;
    }

    pub async fn public_fetch_orders_open(
        &self,
        instruments: &[InstrumentNameExchange],
    ) -> Result<Vec<Order<ExchangeId, InstrumentNameExchange, Open>>, UnindexedClientError> {
        return self.fetch_open_orders(instruments).await;
    }

    pub async fn public_open_order(
        &self,
        request: OrderRequestOpen<ExchangeId, &InstrumentNameExchange>,
    ) -> Option<Order<ExchangeId, InstrumentNameExchange, Result<Open, UnindexedOrderError>>> {
        return self.open_order(request).await;
    }

    pub async fn public_cancel_order(
        &self,
        request: OrderRequestCancel<ExchangeId, &InstrumentNameExchange>,
    ) -> Option<UnindexedOrderResponseCancel> {
        return self.cancel_order(request).await;
    }
}
