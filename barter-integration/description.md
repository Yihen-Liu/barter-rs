## 模块总览

- `lib.rs`：定义 `Transformer`、`StreamParser`、`Validator` 等核心 trait，并导出 `error`、`protocol`、`channel`、`stream`、`metric`、`subscription`、`collection`、`snapshot` 等模块。
- `error.rs`：统一的 `SocketError`，覆盖 WebSocket/HTTP 请求、序列化、订阅、API 错误，便于上层统一处理。
- `protocol/`：抽象通信协议。`websocket.rs` 给出基于 `tokio-tungstenite` 的默认实现；`http/` 下提供 `RestClient`、签名器、请求构建策略等。
- `stream/`：组合协议与转换逻辑的总线。`ExchangeStream` 接收协议消息，调用用户实现的 `Transformer` 输出业务数据；`indexed.rs` 给出对接 `barter-instrument` 的索引包装；`merge.rs` 用于合并账户/行情等多个流。
- `channel.rs`：封装 `tokio::mpsc`，抽象 `Tx` trait，并提供 `ChannelTxDroppable` 等工具，方便控制流向和关闭通知。
- `collection/`：提供 `OneOrMany`、`NoneOneOrMany` 等辅助集合类型，以及 `FnvIndexMap/Set` 别名。
- `subscription.rs`：`SubscriptionId` 新类型，统一订阅标识格式。
- `metric.rs`：`Metric/Tag/Field/Value`，用于测量 HTTP/WS 延迟等指标。
- `snapshot.rs`：构建带快照+增量更新的数据流（`SnapUpdates`），常用于账户/行情同步。
- `examples/simple_websocket_integration.rs`：示范如何用 `WebSocket` + 自定义 `Transformer` 来消费 Binance 行情。

## 核心流程

1. 建立底层连接：WebSocket (`protocol::websocket::WebSocket`) 或 HTTP (`protocol::http::RestClient`)。
2. 为流绑定 `Transformer`：解析协议消息，自定义业务逻辑（示例中累加成交量）。
3. 使用 `Stream` 工具 (`ExchangeStream`) 包装连接，自动解析、转换，再通过 `StreamExt` 消费。
4. 渠道与组合：利用 `channel::Channel` 提供的 `Tx/Rx` 在任务间传递消息；`stream::merge`、`SnapUpdates` 支持合并多个数据流或恢复快照。
5. 执行与监控：通过 `RestClient` 构建/签名请求，执行并返回 `(响应, Metric)`，便于记录 API 延迟等。

## 模块能力与用法

- **protocol::http**：`RestClient::new(base_url, strategy, parser)` 组合请求构建与解析；`Strategy` 负责签名/加头；`Parser` 负责解析响应/错误。执行时 `execute(request)` 返回响应与延迟指标。
- **protocol::websocket**：`WebSocket` 类型、`WsMessage` 枚举、`WebSocketSerdeParser` 等，直接基于 `tokio-tungstenite`；`ExchangeStream<WebSocketSerdeParser, WebSocket, Transformer>` 即可处理推送。
- **stream::ExchangeStream**：通过 `stream` 和 `transformer` 把协议消息转为业务消息；流结束自动转发错误或中止。
- **channel::mpsc_unbounded**：快速建管道；配合 `ChannelTxDroppable` 在接收方掉线时自动关闭发送。
- **stream::indexed::IndexedStream**：传入 `Indexer`，自动把原始事件映射成索引化结构（上层通常用 `AccountEventIndexer` 等实现）。
- **snapshot::SnapUpdates**：构建快照+增量数据源（常由 `ExecutionManager` 使用，向引擎推送账户状态）。
- **collection::OneOrMany**：订阅或配置可写单个或数组，框架内部会统一展开。
- **metric.rs**：利用 `Metric` 记录 HTTP 请求耗时等关键指标，方便监控。

## 示例（WebSocket）

```rust
type WsStream = ExchangeStream<WebSocketSerdeParser, WebSocket, Transformer>;

let (ws_stream, _) = connect_async(url).await?;
let transformer = StatefulTransformer { sum_of_volume: 0.0 };

let mut stream = WsStream::new(ws_stream, transformer, VecDeque::new());

while let Some(result) = stream.next().await {
    match result {
        Ok(value) => println!("Volume sum: {value}"),
        Err(err) => eprintln!("Error: {err}"),
    }
}
```

## 使用建议

- 将 `RestClient`、`ExchangeStream` 视为“协议层（Web/HTTP）→ 统一数据层”的适配器，上层只需提供解析/转换逻辑。
- 合理利用 `Channel`、`merge` 等工具，将市场数据、账户反馈、命令执行等拆分为模块化异步任务。
- 如果需要索引化、快照恢复，与 `barter-instrument`、`barter-execution` 一起使用；若只做简单推送消费，可直接用 `websocket` + `Transformer`。
- 错误处理遵循 `SocketError`：可区分可恢复/不可恢复场景，并配合 `Unrecoverable` trait 做重连或停止策略。

总体而言，`barter-integration` 是 Barter 生态的底层通信框架，负责对接 WebSocket、HTTP 等协议，把原始消息转换为统一的业务数据，为上层行情、执行、风控提供可靠基础设施。

## 核心能力总结

- 统一通信抽象：以 `ExchangeStream` 和 `RestClient` 把 WebSocket、HTTP 等协议转换成可直接消费的数据流或 API 调用，屏蔽底层细节。
- 灵活协议适配：`StreamParser`、`Transformer`、`BuildStrategy`、`HttpParser` 等 trait 组合，可自定义解析/转换/签名逻辑，满足各交易所差异。
- 流处理工具链：内置 `IndexedStream`、`merge`、`SnapUpdates` 等，支持索引化、快照+增量同步、多流融合，便于构建稳定的行情/账户处理流水线。
- 通道与集合工具：封装 `mpsc` 管道、`OneOrMany` 等集合类型，帮助统一管理订阅和消息分发。
- 统一错误与指标：`SocketError` 描述全链路错误，`Metric` 捕获 HTTP/WS 延迟等性能数据，便于监控与故障处理。