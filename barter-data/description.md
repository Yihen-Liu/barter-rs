## 模块结构

- **核心入口 `lib.rs`**：汇总导出 `streams`、`exchange`、`subscriber`、`subscription`、`transformer`、`books` 等子模块，定义 `MarketStream`、`SnapshotFetcher` 等统一接口，并给出示例用法。`ExchangeWsStream` 泛化了 `WebSocket + Transformer` 的组合。[`35:94:barter-rs/barter-data/src/lib.rs`]
- **`streams/`**：`StreamBuilder` 负责批量订阅、去重、验证并初始化重连流，`Streams` 提供 `select/select_all` 便捷消费接口，`consumer` 驱动异步循环，`reconnect` 封装自动重连策略。[`1:140:barter-rs/barter-data/src/streams/builder/mod.rs`]
- **`exchange/`**：每个交易所实现 `Connector`、`StreamSelector`、`SubscriptionValidator`、`Transformer` 等，具体拆分 channel、market、subscription、trade/orderbook 逻辑，保证输出统一的 `MarketEvent`。
- **`subscriber/`**：`WebSocketSubscriber` 负责真正发起连接、发送订阅、校验回应，并返回 `Subscribed`（包含 `WebSocket`、映射表、缓冲事件）。[`53:108:barter-rs/barter-data/src/subscriber/mod.rs`]
- **`subscription/`**：定义 `Subscription`、`SubscriptionKind`（如 `PublicTrades`、`OrderBooksL1` 等），与 `InstrumentData` 结合描述要监听的市场。
- **`transformer/`**：`ExchangeTransformer`／`StatelessTransformer` 将交易所原始消息映射为归一化事件；复杂场景（如 L2 快照）可自定义。
- **`books/`**：提供本地订单簿管理器、映射器，支持在客户端维护 L1/L2 深度。
- **`event`、`instrument`、`error`** 等提供数据类型、事件结构和错误封装。

## 核心能力

- **多交易所、多品种实时推送**：统一 `StreamBuilder` 接口一次性初始化多个交易所、多个订阅，自动分配连接与通道。[`35:94:barter-rs/barter-data/src/lib.rs`]
- **归一化数据模型**：所有交易所输出同一 `MarketEvent` 结构，策略侧无需关注差异。
- **自动重连与订阅校验**：`ReconnectingStream` 按策略自动处理断线，`SubscriptionValidator` 确认订阅成功，保证长时间稳定运行。[`1:140:barter-rs/barter-data/src/streams/builder/mod.rs`]
- **可组合的 Transformer**：默认 `StatelessTransformer` 满足大部分需求，也允许注入自定义逻辑（如合并快照、增量更新）。
- **订单簿工具链**：`books` 模块帮助快速维护本地 L2/L1 深度，配合 `ExchangeTransformer` 获取初始快照。

## 主流程

1. **构建订阅**：用户准备 `Subscription`（三元组：交易所、品种、数据类型），交给 `StreamBuilder::subscribe`；内部保留 Future，等待 `init`。[`76:140:barter-rs/barter-data/src/streams/builder/mod.rs`]
2. **校验并发送订阅**：`WebSocketSubscriber::subscribe` 连接交易所、发送订阅消息、调用各交易所的 Validator 校验并收集初始缓冲事件。[`53:108:barter-rs/barter-data/src/subscriber/mod.rs`]
3. **构建 `ReconnectingStream`**：`init_market_stream` 将订阅封装成可重连的 `ExchangeStream`，内部使用 `barter-integration` 的 `ExchangeStream` + `Transformer` 将原始 `WsMessage` 转换为 `MarketEvent`。
4. **分发事件**：每个订阅创建一个任务，将 `MarketEvent` 推入对应 `Channel`；`StreamBuilder::init` 汇总为 `Streams`，用户可按交易所或 select_all 消费事件流。[`1:52:barter-rs/barter-data/src/streams/mod.rs`]

## 使用方法示例

```rust
let streams = Streams::<PublicTrades>::builder()
    .subscribe([
        (BinanceSpot::default(), "btc", "usdt", MarketDataInstrumentKind::Spot, PublicTrades),
        (Coinbase, "btc", "usd", MarketDataInstrumentKind::Spot, PublicTrades),
    ])
    .init()
    .await
    .unwrap();

let mut joined = streams.select_all();
while let Some(event) = joined.next().await {
    println!("{event:?}");
}
```
[`35:94:barter-rs/barter-data/src/lib.rs`]

### 实践建议
- **订阅管理**：每次 `subscribe` 会开一个独立 WebSocket 连接，按需求分组订阅控制并发量。
- **错误处理**：`select_all().with_error_handler()` 可集中处理 `DataError`；遇到 `SocketError::Terminated` 时会自动重连。
- **扩展交易所**：实现 `Connector`、`StreamSelector`、`SubscriptionMapper/Validator` 与 `Transformer` 即可接入新交易所。
- **订单簿场景**：结合 `books::manager` 和相应的 `OrderBooksL2` Transformer，在客户端维护高精度深度。

总体来说，`barter-data` 提供从“订阅定义 → WebSocket 建连 → 自动重连 → 归一化事件分发”的全链路能力，适合作为高频行情采集和策略前端的数据管道。