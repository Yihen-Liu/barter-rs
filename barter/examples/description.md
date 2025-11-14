## WSS被完成触发的流程
在 `engine_sync_with_live_market_data_and_mock_execution_and_audit.rs` 里，WebSocket 连接的建立与订阅调用链如下：

1. **示例入口**  
   `main` 中调用：
   ```rust
   let market_stream = init_indexed_multi_exchange_market_stream(
       &instruments,
       &[SubKind::PublicTrades, SubKind::OrderBooksL1],
   ).await?;
   ```
   (`barter-examples/engine_sync_with_live_market_data_and_mock_execution_and_audit.rs`)

2. **生成订阅并初始化 Stream**  
   `init_indexed_multi_exchange_market_stream`（`barter-data/src/streams/builder/dynamic/indexed.rs`）：
   - 基于 `IndexedInstruments` 和 `SubKind` 生成每个交易所的订阅批次；
   - 调用 `DynamicStreams::init(subscriptions).await?`。

3. **动态流初始化**  
   `DynamicStreams::init`（`barter-data/src/streams/builder/dynamic/mod.rs`）：
   - 对订阅分组（按交易所与子类型）；
   - 对每个 `(exchange, sub_kind)` 调用 `init_market_stream(STREAM_RECONNECTION_POLICY, subs).await?`。

4. **创建可重连的 MarketStream**  
   `init_market_stream`（`barter-data/src/streams/consumer.rs`）：
   - 打印你看到的日志；
   - 调用 `init_reconnecting_stream`，其闭包实际执行 `Exchange::Stream::init::<SnapFetcher>(&subscriptions)`。

5. **真正建立 WebSocket**  
   `MarketStream::init` 的实现见 `barter-data/src/lib.rs`：
   - `Exchange::Subscriber::subscribe(subscriptions).await?`
   - 在 `WebSocketSubscriber::subscribe`（`barter-data/src/subscriber/mod.rs`）中：
     1. 通过 `Exchange::url()` 获取目标地址。对 Binance Spot 来说，`url()` 会调用 `BinanceServerSpot::websocket_url()` 返回常量  
        `wss://stream.binance.com:9443/ws`（`barter-data/src/exchange/binance/spot/mod.rs`）。
     2. 调用 `barter_integration::protocol::websocket::connect(url)`，内部使用 `tokio_tungstenite::connect_async` 发起 WebSocket 握手。
     3. 利用 `SubscriptionMapper` 生成订阅 payload，调用 `websocket.send(...)` 发送订阅请求。
     4. 由 `Exchange::SubValidator::validate` 监听并确认交易所返回的订阅确认，同时收集握手期间缓冲的消息。

6. **后续处理**  
   - 可选：`SnapFetcher::fetch_snapshots` 拉取初始快照（若需要 L2 深度等）。
   - `websocket.split()` 将连接拆成 `Sink/Stream`；分别启动任务发送后续订阅/自定义 Ping。
   - 初始化 `Transformer`（解析器）并处理前面缓存的事件。
   - 返回 `ExchangeWsStream`，供后续 `init_reconnecting_stream` 包装为自动重连流。

**总结**：  
WebSocket 实际是在 `WebSocketSubscriber::subscribe` 中的 `connect(url)` 调用时创建的（依赖 `tokio_tungstenite`），而这一点是由 `init_market_stream`→`Exchange::Stream::init`→`Exchange::Subscriber::subscribe` 这一条调用链触发的。随后订阅请求通过同一 WebSocket 发送，验证成功后流才正式交给引擎消费。