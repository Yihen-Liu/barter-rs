## 模块概览

 **核心入口 (`lib.rs`)**：统一导出 `exchange`、`asset`、`instrument`、`index` 等模块，并定义通用结构 `Keyed`、`Underlying`、`Side` 等，为其他 crate 提供公共类型。  

```rust
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct Underlying<AssetKey> {
    pub base: AssetKey,
    pub quote: AssetKey,
}

```

**交易所模块 `exchange`**：定义 `ExchangeId` 与 `ExchangeIndex`。`ExchangeId` 枚举涵盖常见现货、期货、期权及模拟交易所，并支持序列化/反序列化，方便网络交互或配置文件使用。  

```rust
#[serde(rename = "execution", rename_all = "snake_case")]
pub enum ExchangeId {
    Other,
    Simulated,
    Mock,
    BinanceFuturesCoin,
    ...
    Poloniex,
}
```

**资产模块 `asset`**：提供资产标识 (`AssetId`, `AssetIndex`)、资产实体 (`Asset`)、资产种类 (`AssetKind`) 以及交易所资产包装 (`ExchangeAsset`)。强调“内部名/交易所名”双轨制，确保在序列化或索引时既保持统一命名，又保留原始交易所标识。  

 ```rust
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct Asset {
    pub name_internal: AssetNameInternal,
    pub name_exchange: AssetNameExchange,
}
```

**品种模块 `instrument`**：项目核心，描述交易品种的结构、命名、合约规格、行情订阅信息等。  
`Instrument` 结构体保存交易所标识、内部/交易所名称、标的资产 (`Underlying`)、报价资产 (`InstrumentQuoteAsset`)、品种类型 (`InstrumentKind`) 及可选规格 (`InstrumentSpec`)。  
```rust
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
pub struct Instrument<ExchangeKey, AssetKey> {
    pub exchange: ExchangeKey,
    pub name_internal: InstrumentNameInternal,
    pub name_exchange: InstrumentNameExchange,
    pub underlying: Underlying<AssetKey>,
    pub quote: InstrumentQuoteAsset,
    pub kind: InstrumentKind<AssetKey>,
    pub spec: Option<InstrumentSpec<AssetKey>>,
}
```
  - `kind/`：区分 Spot、Perpetual、Future、Option，附带合约规模、结算资产、到期日、行权方式等信息。
  - `name/`：定义内部/交易所命名规则，采用 `SmolStr` 节省内存，提升比较效率。
  - `spec/`：描述报价精度、下单最小单位、名义价值、数量单位（按合约、资产或计价货币）等。
  - `market_data/`：封装订阅行情所需的最小字段，便于与 `barter-data` 共享。

**索引模块 `index`**：提供 `IndexedInstruments` 及其 Builder，用于把“原始的 Instrument 列表”做去重、建立 `ExchangeIndex`/`AssetIndex`/`InstrumentIndex` 映射，实现 O(1) 查找，支撑引擎状态存储。  
```rust
pub struct IndexedInstruments {
    exchanges: Vec<Keyed<ExchangeIndex, ExchangeId>>,
    assets: Vec<Keyed<AssetIndex, ExchangeAsset<Asset>>>,
    instruments:
        Vec<Keyed<InstrumentIndex, Instrument<Keyed<ExchangeIndex, ExchangeId>, AssetIndex>>>,
}
```
  Builder 会处理去重、索引分配，并在构建品种时自动把资产引用转换成 `AssetIndex`，保证一致性。  
  ```rust
let instruments = self
    .instruments
    .into_iter()
    .enumerate()
    .map(|(index, instrument)| {
        let instrument = instrument.map_exchange_key(Keyed::new(exchange_key, exchange_id));
        let instrument = instrument
            .map_asset_key_with_lookup(|asset: &Asset| {
                find_asset_by_exchange_and_name_internal(
                    &assets,
                    exchange_id,
                    &asset.name_internal,
                )
            })
            .expect("every asset related to every instrument has been added");
        Keyed::new(InstrumentIndex::new(index), instrument)
    })
    .collect();
```

- **辅助类型**
  - `Keyed<Key, Value>`：任何键值对的轻量包装，常用于索引表。
  - `Side`：买卖方向枚举，含多种序列化别名。

## 主要功能

1. **统一的数据模型**：对交易所、资产、品种等核心概念做强类型封装，避免字符串散落；便于序列化、反序列化与跨模块传递。
2. **索引化状态管理**：通过 `IndexedInstruments` 把所有实体映射成连续索引，支撑 `barter` 引擎的高速 O(1) 状态访问。
3. **多品种支持**：一套 `InstrumentKind` 同时适配现货/期货/期权/永续合约，并在同一结构中共享公共字段。
4. **精细规格描述**：`InstrumentSpec` 允许记录 tick size、最小下单量、合约面值、数量单位等，为风控、下单、回测提供数据。
5. **行情/执行桥梁**：`market_data` 子模块与 `barter-data` 对接，`quote` 和 `Underlying` 支持执行模块计算保证金、盈亏。

## 使用方式

### 1. 构造资产与品种

```rust
use barter_instrument::{
    asset::Asset,
    exchange::ExchangeId,
    instrument::{
        Instrument,
        kind::InstrumentKind,
        name::{InstrumentNameExchange, InstrumentNameInternal},
        quote::InstrumentQuoteAsset,
    },
    Underlying,
};

let base = Asset::new("btc", "BTC");
let quote = Asset::new("usdt", "USDT");

let instrument = Instrument::new(
    ExchangeId::BinanceSpot,
    InstrumentNameInternal::new_from_exchange(ExchangeId::BinanceSpot, "btc_usdt"),
    InstrumentNameExchange::from("BTC_USDT"),
    Underlying::new(base.clone(), quote.clone()),
    InstrumentQuoteAsset::UnderlyingQuote,
    InstrumentKind::Spot,
    None,
);
```

### 2. 构建索引集合

```rust
use barter_instrument::index::IndexedInstruments;

let indexed = IndexedInstruments::new([instrument]);
// 之后可按 ExchangeId / 内部名 查找 index
let spot_idx = indexed.find_exchange_index(ExchangeId::BinanceSpot)?;
let btc_idx = indexed.find_asset_index(ExchangeId::BinanceSpot, &"btc".into())?;
```

### 3. 与 `IndexedInstrumentsBuilder` 协作

当需要逐步追加或从配置文件生成时，使用 Builder 自动去重、映射：

```rust
use barter_instrument::{index::IndexedInstrumentsBuilder, test_utils::instrument, exchange::ExchangeId};

let indexed = IndexedInstrumentsBuilder::default()
    .add_instrument(instrument(ExchangeId::BinanceSpot, "btc", "usdt"))
    .add_instrument(instrument(ExchangeId::Coinbase, "btc", "usd"))
    .build();
```

Builder 会确保所有涉及的资产、交易所自动索引，避免手动管理。

## 适用场景与注意事项

- **适用场景**：当你需要在不同模块之间共享统一的交易所/资产/品种定义；或为引擎、风控、回测提供一致的索引数据源。
- **注意事项**：
  - `IndexedInstruments` 构建后不可修改（保持索引稳定）。如需增量更新，重新使用 Builder。
  - `InstrumentSpec` 是可选的，但如果执行/风控逻辑依赖精度或最小下单量，务必提供。
  - 内外命名转换依赖 `AssetNameInternal`/`InstrumentNameInternal` 约定，解析配置时需保持格式一致。

总体而言，`barter-instrument` 负责把各类原始配置/字符串整理成强类型、索引化、可序列化的数据结构，是整个 Barter 生态的“字典”和“索引”基础。