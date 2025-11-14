[11 tools called]

## 模块结构速览

- `lib.rs`：定义账户事件 `AccountEvent/AccountEventKind`、账户快照 `AccountSnapshot`、统一类型别名（UnindexedAccountEvent 等），同时导出子模块。
- `balance.rs`：资产余额模型 `AssetBalance` 与 `Balance`，记录总额、可用额、更新时间。
- `order/`：
  - `id.rs`：下单标识（策略 ID、客户端订单 ID 等）。
  - `request.rs`：下单/撤单请求 `OrderRequestOpen/Cancel` 与 `RequestOpen/Cancel`。
  - `state.rs`：订单状态机（`OpenInFlight`、`Open`、`Cancelled`、`FullyFilled` 等）。
  - `mod.rs`：订单主体 `Order`、`OrderEvent`，并提供从请求/状态互转的辅助方法。
- `trade.rs`：成交记录 `Trade` 与手续费 `AssetFees`。
- `error.rs`：执行端错误体系（`ClientError`、`ConnectivityError`、`ApiError`、`OrderError` 等）。
- `map.rs`：`ExecutionInstrumentMap`，将 `barter-instrument` 的索引结构映射到交易所原始符号；提供查找、反向查找。
- `indexer.rs`：`AccountEventIndexer`，把执行端返回的非索引化事件/快照映射为引擎内部索引类型；支持 `IndexedStream`。
- `client/`：
  - `mod.rs`：核心 trait `ExecutionClient`，统一定义交易所执行接口（`account_snapshot`、`account_stream`、`open_order`、`cancel_order`、`fetch_balances`、`fetch_trades` 等）。
  - `mock/`：MockExecution 客户端；配合 `exchange/mock` 模拟交易所，支持延迟、手续费、账户更新等。
  - `binance/`：占位符，尚未实现真实客户端。
- `exchange/mock/`：Mock 交易所实现（账户状态 `account.rs`、请求协议 `request.rs`、撮合逻辑 `mod.rs`）。

## 核心能力

1. **统一执行接口**：`ExecutionClient` trait 抽象出对交易所的关键操作；无论实盘还是模拟，都可通过相同 API 被引擎调度。
2. **账户 & 订单模型**：提供标准化的订单结构、状态机、余额、成交记录，贯穿开仓、撤单、成交、余额更新等完整生命周期。
3. **索引映射**：`ExecutionInstrumentMap` + `AccountEventIndexer` 将交易所原生符号、资产与内部索引 (`ExchangeIndex` / `InstrumentIndex` / `AssetIndex`) 对接，保证引擎状态 O(1) 访问。
4. **Mock 执行环境**：内置功能丰富的模拟交易所（延迟、手续费、账户同步），便于回测、纸面交易和自动化测试。
5. **错误体系**：细分连接错误、API 拒绝、订单拒绝等情形，方便上层进行可恢复/不可恢复的区分与重试策略。

## 主逻辑流程

1. **系统构建**：`barter` 引擎通过 `ExecutionBuilder::add_mock` 或 `add_live` 注册执行客户端；内部调用本包生成 `ExecutionInstrumentMap` 和 `AccountEventIndexer`。
2. **请求发送**：当引擎调用 `send_open_requests` 或策略生成订单时，会把 `OrderRequestOpen` 封装为 `ExecutionRequest::Open`，并通过执行通道发送给具体 `ExecutionClient`。
3. **客户端执行**：
   - 实际客户端需实现 `open_order()`、`cancel_order()` 等，将订单发送至交易所。
   - Mock 实现则将请求送入 `MockExchangeRequest` 队列，模拟撮合后返回结果。
4. **回执处理**：`ExecutionManager` 监听客户端响应，利用 `AccountEventIndexer` 转换为索引化 `AccountEvent`（快照、撤单结果、成交、余额更新），再发回引擎，更新 `EngineState`。
5. **账户流 & 快照**：`ExecutionClient::account_stream()` 提供实时账户事件流；`account_snapshot()` 可在启动时拉取完整账户状态。

## 使用方法

- **接入 Mock 执行**（回测/模拟）：
  ```rust
  ExecutionBuilder::new(&indexed_instruments)
      .add_mock(mock_config, clock)?
      .build()
      .init().await?;
  ```
  `mock_config` (`MockExecutionConfig`) 指定模拟交易所、初始账户状态、延迟、手续费等。

- **接入真实交易所**：
  1. 实现 `ExecutionClient` trait（参考 mock 实现）：
     - 处理认证、REST/WebSocket 接口、签名、错误解析。
  2. 在 `ExecutionBuilder::add_live::<YourClient>(config, timeout)` 注册。
  3. 引擎即可通过统一接口对实盘执行、获取账户数据。

- **订单/风控扩展**：
  - 使用 `Order::to_request_cancel()`、`OrderState` 工具在策略或风控中进行状态转换。
  - 通过 `OrderError`、`ApiError`、`ConnectivityError` 定制异常处理。

## 设计亮点与注意事项

- **解耦**：执行层（本包）与引擎层 (`barter`) 通过 `ExecutionRequest`、`AccountEvent` 通道交互，可自由切换实盘/模拟或接入新交易所。
- **索引一致性**：必须使用 `ExecutionInstrumentMap` 生成的映射；若交易所返回未索引的资产/品种会触发 `IndexError`。
- **Mock 功能**：模拟撮合既可测试自动化策略，也可在回测中复用，使回测环境与实盘逻辑尽量一致。
- **扩展性**：添加新交易所只需实现 `ExecutionClient` + 定制错误解析/签名即可；订单模型、事件、错误体系都可复用。

总体来说，`barter-execution` 是整个 Barter 系统的“执行引擎”，负责把引擎统一的订单指令转换成交易所可理解的请求，返回归一化的账户/成交事件，并且提供了完善的模拟环境与错误处理机制，极大地降低了接入实盘和测试环境的复杂度。