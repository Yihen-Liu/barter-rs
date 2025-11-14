use barter::{
    engine::{
        clock::LiveClock,
        state::{
            EngineState,
            global::DefaultGlobalData,
            instrument::{
                data::{DefaultInstrumentMarketData, InstrumentDataState},
                filter::InstrumentFilter,
            },
            trading::TradingState,
        },
    },
    logging::init_logging,
    risk::{RiskApproved, RiskManager, RiskRefused, check::util::calculate_quote_notional},
    statistic::time::Daily,
    strategy::{
        algo::AlgoStrategy,
        close_positions::{ClosePositionsStrategy, close_open_positions_with_market_orders},
        on_disconnect::OnDisconnectStrategy,
        on_trading_disabled::OnTradingDisabled,
    },
    system::{
        builder::{AuditMode, EngineFeedMode, SystemArgs, SystemBuilder},
        config::SystemConfig,
    },
};
use barter_data::{
    event::{DataKind, MarketEvent},
    streams::builder::dynamic::indexed::init_indexed_multi_exchange_market_stream,
    subscription::SubKind,
};
use barter_execution::{
    AccountEvent,
    order::{
        OrderKey, OrderKind, TimeInForce,
        id::{ClientOrderId, StrategyId},
        request::{OrderRequestCancel, OrderRequestOpen, RequestOpen},
    },
};
use barter_instrument::{Side, index::IndexedInstruments};
use derive_more::Constructor;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::BufReader, marker::PhantomData, time::Duration};
use tracing::{debug, info, warn};

const FILE_PATH_SYSTEM_CONFIG: &str = "barter/examples/config/system_config.json";
const RISK_FREE_RETURN: Decimal = dec!(0.05);

// 策略参数
const BUY_THRESHOLD_PERCENT: Decimal = dec!(2.0); // 下跌5%买入
const SELL_THRESHOLD_PERCENT: Decimal = dec!(5.0); // 上涨2%卖出
const ORDER_QUANTITY: Decimal = dec!(0.001); // 每次交易数量

// 风控参数
const MAX_POSITION_PERCENT: Decimal = dec!(50.0); // 最大仓位比例50%

/// 自定义的 InstrumentDataState，用于跟踪基准价格（买入价格）
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Constructor)]
pub struct PriceTrackingData {
    /// 基础市场数据
    pub market_data: DefaultInstrumentMarketData,
    /// 基准价格（买入价格），用于计算涨跌幅
    pub base_price: Option<Decimal>,
}

impl PriceTrackingData {
    /// 获取基准价格
    pub fn get_base_price(&self) -> Option<Decimal> {
        self.base_price
    }

    /// 设置基准价格
    pub fn set_base_price(&mut self, price: Decimal) {
        self.base_price = Some(price);
    }
}

/// 辅助函数：从 InstrumentState 中获取 base_price
/// 由于 EngineState 的类型参数是 PriceTrackingData，data 字段的类型就是 PriceTrackingData
/// 这个函数使用 unsafe 转换，但在类型系统保证的情况下是安全的
fn get_base_price_from_instrument_state<InstrumentData>(
    instrument_state: &barter::engine::state::instrument::InstrumentState<InstrumentData>,
) -> Option<Decimal>
where
    InstrumentData: InstrumentDataState,
{
    // 由于类型系统保证，我们知道 InstrumentData 就是 PriceTrackingData
    // 使用 unsafe 转换来访问 base_price 字段
    unsafe {
        let data_ptr = &instrument_state.data as *const InstrumentData as *const PriceTrackingData;
        (*data_ptr).get_base_price()
    }
}

impl Default for PriceTrackingData {
    fn default() -> Self {
        Self {
            market_data: DefaultInstrumentMarketData::default(),
            base_price: None,
        }
    }
}

impl InstrumentDataState for PriceTrackingData {
    type MarketEventKind = DataKind;

    fn price(&self) -> Option<Decimal> {
        self.market_data.price()
    }
}

impl<InstrumentKey> barter::engine::Processor<&MarketEvent<InstrumentKey, DataKind>>
    for PriceTrackingData
{
    type Audit = ();

    fn process(&mut self, event: &MarketEvent<InstrumentKey, DataKind>) -> Self::Audit {
        // 先更新基础市场数据
        self.market_data.process(event);

        // 如果还没有基准价格，使用当前价格作为基准
        if self.base_price.is_none() {
            if let Some(current_price) = self.price() {
                self.base_price = Some(current_price);
                info!(
                    base_price = %current_price,
                    "设置基准价格"
                );
            }
        }
    }
}

impl<ExchangeKey, AssetKey, InstrumentKey>
    barter::engine::Processor<&AccountEvent<ExchangeKey, AssetKey, InstrumentKey>>
    for PriceTrackingData
{
    type Audit = ();

    fn process(
        &mut self,
        event: &AccountEvent<ExchangeKey, AssetKey, InstrumentKey>,
    ) -> Self::Audit {
        // 当有新的成交时，更新基准价格为买入价格
        if let barter_execution::AccountEventKind::Trade(trade) = &event.kind {
            if trade.side == Side::Buy {
                // 买入后，更新基准价格为买入价格
                self.base_price = Some(trade.price);
                info!(
                    base_price = %trade.price,
                    "更新基准价格为买入价格"
                );
            } else if trade.side == Side::Sell {
                // 卖出后，重置基准价格，等待下次买入
                self.base_price = None;
                info!("卖出后重置基准价格");
            }
        }
    }
}

impl<ExchangeKey, InstrumentKey>
    barter::engine::state::order::in_flight_recorder::InFlightRequestRecorder<
        ExchangeKey,
        InstrumentKey,
    > for PriceTrackingData
{
    fn record_in_flight_cancel(&mut self, _: &OrderRequestCancel<ExchangeKey, InstrumentKey>) {}

    fn record_in_flight_open(&mut self, _: &OrderRequestOpen<ExchangeKey, InstrumentKey>) {}
}

/// 价格变化策略：
/// 1. 当价格下跌2%时，买入
/// 2. 当价格相对于买入价格上涨5%时，卖出
/// 3. 买入后，将当前价格设置为买入价，如果继续下跌2%，则继续买入（加仓）
#[derive(Debug, Clone)]
pub struct PriceChangeStrategy {
    pub id: StrategyId,
}

impl Default for PriceChangeStrategy {
    fn default() -> Self {
        Self {
            id: StrategyId::new("PriceChangeStrategy"),
        }
    }
}

impl AlgoStrategy for PriceChangeStrategy {
    type State = EngineState<DefaultGlobalData, PriceTrackingData>;

    fn generate_algo_orders(
        &self,
        state: &Self::State,
    ) -> (
        impl IntoIterator<Item = OrderRequestCancel>,
        impl IntoIterator<Item = OrderRequestOpen>,
    ) {
        let mut opens = Vec::new();

        // 遍历所有标的
        for instrument_state in state.instruments.instruments(&InstrumentFilter::None) {
            let current_price = match instrument_state.data.price() {
                Some(price) => price,
                None => {
                    debug!(
                        instrument = %instrument_state.instrument.name_internal,
                        "无法获取当前价格，跳过"
                    );
                    continue;
                }
            };

            // 由于 EngineState 的类型参数是 PriceTrackingData，data 字段就是 PriceTrackingData 类型
            // 使用辅助函数获取 base_price（最新买入价格）
            let base_price = get_base_price_from_instrument_state(&instrument_state);
            let has_position = instrument_state.position.current.is_some();

            // 如果没有持仓且没有基准价格，等待设置基准价格
            if !has_position && base_price.is_none() {
                // 这种情况会在下次 process MarketEvent 时设置基准价格
                continue;
            }

            if !has_position {
                // 没有持仓：检查是否下跌2%（相对于基准价格）
                if let Some(base) = base_price {
                    let price_change_pct = ((current_price - base) / base) * dec!(100.0);

                    if price_change_pct <= -BUY_THRESHOLD_PERCENT {
                        info!(
                            instrument = %instrument_state.instrument.name_internal,
                            current_price = %current_price,
                            base_price = %base,
                            price_change_pct = %price_change_pct,
                            "价格下跌超过2%，生成买入订单"
                        );

                        opens.push(OrderRequestOpen {
                            key: OrderKey {
                                exchange: instrument_state.instrument.exchange,
                                instrument: instrument_state.key,
                                strategy: self.id.clone(),
                                cid: ClientOrderId::random(),
                            },
                            state: RequestOpen {
                                side: Side::Buy,
                                price: current_price,
                                quantity: ORDER_QUANTITY,
                                kind: OrderKind::Market,
                                time_in_force: TimeInForce::ImmediateOrCancel,
                            },
                        });
                    }
                }
            } else {
                // 有持仓：优先检查卖出条件，然后检查加仓条件
                if let Some(position) = &instrument_state.position.current {
                    // 1. 优先检查是否可以卖出：如果价格相对于持仓平均买入价上涨5%
                    let entry_price = position.price_entry_average;
                    let price_change_from_entry =
                        ((current_price - entry_price) / entry_price) * dec!(100.0);

                    if price_change_from_entry >= SELL_THRESHOLD_PERCENT {
                        info!(
                            instrument = %instrument_state.instrument.name_internal,
                            current_price = %current_price,
                            entry_price = %entry_price,
                            price_change_pct = %price_change_from_entry,
                            position_quantity = %position.quantity_abs,
                            "价格相对于平均买入价上涨超过5%，生成卖出订单"
                        );

                        opens.push(OrderRequestOpen {
                            key: OrderKey {
                                exchange: instrument_state.instrument.exchange,
                                instrument: instrument_state.key,
                                strategy: self.id.clone(),
                                cid: ClientOrderId::random(),
                            },
                            state: RequestOpen {
                                side: Side::Sell,
                                price: current_price,
                                quantity: position.quantity_abs,
                                kind: OrderKind::Market,
                                time_in_force: TimeInForce::ImmediateOrCancel,
                            },
                        });
                    } else {
                        // 2. 只有在不满足卖出条件时，才检查是否可以加仓
                        // 如果价格相对于最新买入价（base_price）继续下跌2%，继续买入
                        if let Some(base) = base_price {
                            let price_change_from_last_buy =
                                ((current_price - base) / base) * dec!(100.0);

                            if price_change_from_last_buy <= -BUY_THRESHOLD_PERCENT {
                                info!(
                                    instrument = %instrument_state.instrument.name_internal,
                                    current_price = %current_price,
                                    last_buy_price = %base,
                                    price_change_pct = %price_change_from_last_buy,
                                    current_position = %position.quantity_abs,
                                    average_entry_price = %entry_price,
                                    "价格相对于最新买入价继续下跌2%，生成加仓买入订单"
                                );

                                opens.push(OrderRequestOpen {
                                    key: OrderKey {
                                        exchange: instrument_state.instrument.exchange,
                                        instrument: instrument_state.key,
                                        strategy: self.id.clone(),
                                        cid: ClientOrderId::random(),
                                    },
                                    state: RequestOpen {
                                        side: Side::Buy,
                                        price: current_price,
                                        quantity: ORDER_QUANTITY,
                                        kind: OrderKind::Market,
                                        time_in_force: TimeInForce::ImmediateOrCancel,
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }

        (std::iter::empty(), opens)
    }
}

impl ClosePositionsStrategy for PriceChangeStrategy {
    type State = EngineState<DefaultGlobalData, PriceTrackingData>;

    fn close_positions_requests<'a>(
        &'a self,
        state: &'a Self::State,
        filter: &'a InstrumentFilter,
    ) -> (
        impl IntoIterator<Item = OrderRequestCancel> + 'a,
        impl IntoIterator<Item = OrderRequestOpen> + 'a,
    )
    where
        barter_instrument::exchange::ExchangeIndex: 'a,
        barter_instrument::asset::AssetIndex: 'a,
        barter_instrument::instrument::InstrumentIndex: 'a,
    {
        close_open_positions_with_market_orders(&self.id, state, filter, |_| {
            ClientOrderId::random()
        })
    }
}

impl<Clock, State, ExecutionTxs, Risk> OnDisconnectStrategy<Clock, State, ExecutionTxs, Risk>
    for PriceChangeStrategy
{
    type OnDisconnect = ();

    fn on_disconnect(
        _: &mut barter::engine::Engine<Clock, State, ExecutionTxs, Self, Risk>,
        _: barter_instrument::exchange::ExchangeId,
    ) -> Self::OnDisconnect {
    }
}

impl<Clock, State, ExecutionTxs, Risk> OnTradingDisabled<Clock, State, ExecutionTxs, Risk>
    for PriceChangeStrategy
{
    type OnTradingDisabled = ();

    fn on_trading_disabled(
        _: &mut barter::engine::Engine<Clock, State, ExecutionTxs, Self, Risk>,
    ) -> Self::OnTradingDisabled {
    }
}

/// 自定义风控管理器：当仓位超过50%时，停止买入
#[derive(Debug, Clone)]
pub struct PositionLimitRiskManager {
    pub max_position_percent: Decimal,
    phantom: PhantomData<EngineState<DefaultGlobalData, PriceTrackingData>>,
}

impl Default for PositionLimitRiskManager {
    fn default() -> Self {
        Self {
            max_position_percent: MAX_POSITION_PERCENT,
            phantom: PhantomData,
        }
    }
}

impl RiskManager for PositionLimitRiskManager {
    type State = EngineState<DefaultGlobalData, PriceTrackingData>;

    fn check(
        &self,
        state: &Self::State,
        cancels: impl IntoIterator<Item = OrderRequestCancel>,
        opens: impl IntoIterator<Item = OrderRequestOpen>,
    ) -> (
        impl IntoIterator<Item = RiskApproved<OrderRequestCancel>>,
        impl IntoIterator<Item = RiskApproved<OrderRequestOpen>>,
        impl IntoIterator<Item = RiskRefused<OrderRequestCancel>>,
        impl IntoIterator<Item = RiskRefused<OrderRequestOpen>>,
    ) {
        // 总是批准取消请求
        let approved_cancels = cancels
            .into_iter()
            .map(RiskApproved::new)
            .collect::<Vec<_>>();

        // 计算当前总仓位价值和总资产价值
        let (total_position_value, total_asset_value) = calculate_position_and_asset_values(state);

        // 计算当前仓位比例
        let current_position_percent = if total_asset_value > Decimal::ZERO {
            (total_position_value / total_asset_value) * dec!(100.0)
        } else {
            Decimal::ZERO
        };

        // 处理买入订单请求
        let (approved_opens, refused_opens): (Vec<_>, Vec<_>) = opens.into_iter().fold(
            (Vec::new(), Vec::new()),
            |(mut approved, mut refused), request_open| {
                // 只对买入订单进行仓位检查
                if request_open.state.side == Side::Buy {
                    // 计算如果执行这个订单后的仓位价值
                    let instrument_state = state
                        .instruments
                        .instrument_index(&request_open.key.instrument);

                    let order_notional = calculate_quote_notional(
                        request_open.state.quantity,
                        request_open.state.price,
                        instrument_state.instrument.kind.contract_size(),
                    );

                    if let Some(order_value) = order_notional {
                        let new_position_value = total_position_value + order_value;
                        let new_position_percent = if total_asset_value > Decimal::ZERO {
                            (new_position_value / total_asset_value) * dec!(100.0)
                        } else {
                            Decimal::ZERO
                        };

                        // 如果新仓位比例超过限制，拒绝买入订单
                        if new_position_percent > self.max_position_percent {
                            warn!(
                                instrument = %instrument_state.instrument.name_internal,
                                current_position_percent = %current_position_percent,
                                new_position_percent = %new_position_percent,
                                max_position_percent = %self.max_position_percent,
                                total_position_value = %total_position_value,
                                total_asset_value = %total_asset_value,
                                order_value = %order_value,
                                "风控拒绝：仓位超过限制，停止买入"
                            );
                            refused.push(RiskRefused::new(
                                request_open,
                                format!(
                                    "仓位比例 {}% 超过限制 {}%",
                                    new_position_percent, self.max_position_percent
                                ),
                            ));
                            return (approved, refused);
                        }
                    }
                }

                // 卖出订单或通过检查的买入订单，批准
                approved.push(RiskApproved::new(request_open));
                (approved, refused)
            },
        );

        (
            approved_cancels,
            approved_opens,
            std::iter::empty(),
            refused_opens,
        )
    }
}

/// 计算总仓位价值和总资产价值
fn calculate_position_and_asset_values(
    state: &EngineState<DefaultGlobalData, PriceTrackingData>,
) -> (Decimal, Decimal) {
    // 计算总仓位价值（所有持仓的市场价值）
    let mut total_position_value = Decimal::ZERO;

    for instrument_state in state.instruments.instruments(&InstrumentFilter::None) {
        if let Some(position) = &instrument_state.position.current {
            // 获取当前市场价格
            if let Some(current_price) = instrument_state.data.price() {
                // 计算持仓的市场价值
                let position_notional = calculate_quote_notional(
                    position.quantity_abs,
                    current_price,
                    instrument_state.instrument.kind.contract_size(),
                );

                if let Some(value) = position_notional {
                    total_position_value += value;
                }
            }
        }
    }

    // 计算总资产价值（所有资产余额的总价值）
    // 注意：这里简化处理，假设所有资产都以报价资产计价
    // 实际应用中可能需要根据资产类型和汇率进行转换
    let mut total_asset_value = Decimal::ZERO;

    for asset_state in state.assets.assets() {
        if let Some(balance) = &asset_state.balance {
            // 累加所有资产的总余额
            total_asset_value += balance.value.total;
        }
    }

    (total_position_value, total_asset_value)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    init_logging();

    // 加载配置
    let SystemConfig {
        instruments,
        executions,
    } = load_config()?;

    // 构建 IndexedInstruments
    let instruments = IndexedInstruments::new(instruments);

    // 初始化市场数据流
    let market_stream = init_indexed_multi_exchange_market_stream(
        &instruments,
        &[SubKind::PublicTrades, SubKind::OrderBooksL1],
    )
    .await?;

    // 构建系统参数
    let args = SystemArgs::new(
        &instruments,
        executions,
        LiveClock,
        PriceChangeStrategy::default(),
        PositionLimitRiskManager::default(), // 使用自定义风控管理器
        market_stream,
        DefaultGlobalData::default(),
        |_| PriceTrackingData::default(),
    );

    // 构建并运行系统
    let system = SystemBuilder::new(args)
        .engine_feed_mode(EngineFeedMode::Iterator)
        .audit_mode(AuditMode::Enabled)
        .trading_state(TradingState::Disabled)
        .build::<barter::EngineEvent, _>()?
        .init_with_runtime(tokio::runtime::Handle::current())
        .await?;

    // 启用交易
    info!("启用交易");
    system.trading_state(TradingState::Enabled);
    
    // 运行60秒
    info!("运行60秒...");
    tokio::time::sleep(Duration::from_secs(60)).await;

    // 关闭前清理
    info!("关闭前清理订单和持仓");
    system.cancel_orders(InstrumentFilter::None);
    system.close_positions(InstrumentFilter::None);

    // 优雅关闭
    let (engine, _shutdown_audit) = system.shutdown().await?;

    // 生成交易总结
    let trading_summary = engine
        .trading_summary_generator(RISK_FREE_RETURN)
        .generate(Daily);

    // 打印交易总结
    trading_summary.print_summary();

    Ok(())
}

fn load_config() -> Result<SystemConfig, Box<dyn std::error::Error>> {
    let file = File::open(FILE_PATH_SYSTEM_CONFIG)?;
    let reader = BufReader::new(file);
    let config = serde_json::from_reader(reader)?;
    Ok(config)
}
