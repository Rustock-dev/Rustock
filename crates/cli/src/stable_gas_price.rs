//! The miner's stable minimum gas price, rskj's `miner.stableGasPrice`: its
//! settings, read as rskj reads them (`StableMinGasPriceSystemConfig`), its
//! two sources, and the provider rskj's `MinGasPriceProviderFactory` picks.
//!
//! A web API answers a bitcoin's price as JSON, read at a JSON pointer
//! (`HttpGetMinGasPriceProvider`); or a contract on this chain answers it to
//! an `eth_call` on this node (`EthCallMinGasPriceProvider`). Each answer is
//! read as rskj reads it, the web's as Jackson reads JSON (`java`). The
//! provider that keeps the price, and falls back on the fixed one, is
//! `rustock_execution::mining::StableMinGasPriceProvider`.

mod java;

use crate::config_file::{DurationValue, StableGasPriceParams, StableGasPriceSection};
use crate::https;
use anyhow::{anyhow, bail, ensure, Context, Result};
use java::JsonPointer;
use rustock_execution::mining::{
    BtcExchangeRateSource, FixedMinGasPriceProvider, MinGasPriceProvider, MinGasPriceProviderType,
    StableMinGasPriceProvider,
};
use serde_json::{json, Value};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

/// rskj `StableMinGasPriceSystemConfig`.
#[derive(Debug, Clone, PartialEq)]
pub struct StableGasPriceConfig {
    pub enabled: bool,
    /// `refreshRate`, in milliseconds.
    pub refresh_rate_millis: i64,
    pub min_stable_gas_price: i64,
    pub method: MinGasPriceProviderType,
    pub min_valid_price: i64,
    pub max_valid_price: i64,
    params: StableGasPriceParams,
}

/// rskj `HttpGetStableMinGasSystemConfig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpGetConfig {
    pub url: String,
    pub json_path: String,
    pub timeout_millis: i64,
}

/// rskj `EthCallMinGasPriceSystemConfig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EthCallConfig {
    /// The contract called, rskj's `getAddress`.
    pub to: String,
    pub from: String,
    pub data: String,
}

impl StableGasPriceConfig {
    /// As rskj's constructor reads `miner.stableGasPrice`: off unless
    /// `enabled`, and off, no refresh rate, no stable price and the method
    /// `FIXED`. On, `source.method` must name one of rskj's three, or rskj
    /// doesn't start. The valid range is read either way: 0 to a `long`'s
    /// largest unless set.
    pub fn read(section: &StableGasPriceSection) -> Result<Self> {
        let enabled = section.enabled.unwrap_or(false);
        let refresh_rate_millis = match &section.refresh_rate {
            Some(rate) if enabled => {
                duration_millis(rate).context("[mining.stable_gas_price] refresh_rate")?
            }
            _ => 0,
        };
        let min_stable_gas_price = if enabled {
            section.min_stable_gas_price.unwrap_or(0)
        } else {
            0
        };
        let source = section.source.clone().unwrap_or_default();
        let method = if enabled {
            let method = source.method.as_deref().ok_or_else(|| {
                anyhow!("[mining.stable_gas_price] is enabled and names no source.method")
            })?;
            method_named(method)?
        } else {
            MinGasPriceProviderType::Fixed
        };
        Ok(Self {
            enabled,
            refresh_rate_millis,
            min_stable_gas_price,
            method,
            min_valid_price: section.min_valid_price.unwrap_or(0),
            max_valid_price: section.max_valid_price.unwrap_or(i64::MAX),
            params: source.params.unwrap_or_default(),
        })
    }

    /// rskj `getHttpGetConfig`: `url`, `json_path` and `timeout`, each
    /// required.
    pub fn http_get(&self) -> Result<HttpGetConfig> {
        let missing = |key: &str| {
            anyhow!("[mining.stable_gas_price.source.params] names no {key}, which HTTP_GET needs")
        };
        let params = &self.params;
        let url = params.url.clone().ok_or_else(|| missing("url"))?;
        let json_path = params
            .json_path
            .clone()
            .ok_or_else(|| missing("json_path"))?;
        let timeout = params.timeout.as_ref().ok_or_else(|| missing("timeout"))?;
        Ok(HttpGetConfig {
            url,
            json_path,
            timeout_millis: duration_millis(timeout)
                .context("[mining.stable_gas_price.source.params] timeout")?,
        })
    }

    /// rskj `getEthCallConfig`: `to`, `from` and `data`, each required.
    pub fn eth_call(&self) -> Result<EthCallConfig> {
        let missing = |key: &str| {
            anyhow!("[mining.stable_gas_price.source.params] names no {key}, which ETH_CALL needs")
        };
        let params = &self.params;
        Ok(EthCallConfig {
            to: params.to.clone().ok_or_else(|| missing("to"))?,
            from: params.from.clone().ok_or_else(|| missing("from"))?,
            data: params.data.clone().ok_or_else(|| missing("data"))?,
        })
    }
}

/// rskj's `getEnum(MinGasPriceProviderType.class, "source.method")`: a
/// constant's name, exactly.
fn method_named(name: &str) -> Result<MinGasPriceProviderType> {
    match name {
        "FIXED" => Ok(MinGasPriceProviderType::Fixed),
        "HTTP_GET" => Ok(MinGasPriceProviderType::HttpGet),
        "ETH_CALL" => Ok(MinGasPriceProviderType::EthCall),
        other => bail!(
            "[mining.stable_gas_price] source.method {other:?} is none of FIXED, HTTP_GET and ETH_CALL"
        ),
    }
}

/// Whether the settings have the miner's price from an `eth_call` on this
/// node, which needs the node's state open to calls.
pub fn calls_this_node(section: Option<&StableGasPriceSection>) -> bool {
    section.is_some_and(|section| {
        section.enabled == Some(true)
            && section.source.as_ref().and_then(|s| s.method.as_deref()) == Some("ETH_CALL")
    })
}

/// A duration as rskj's HOCON reads it (`SimpleConfig.parseDuration`), in the
/// milliseconds `Duration.toMillis` gives: a number alone is milliseconds;
/// otherwise a number and one of HOCON's units.
pub fn duration_millis(value: &DurationValue) -> Result<i64> {
    let nanos = match value {
        DurationValue::Millis(millis) => millis.saturating_mul(1_000_000),
        DurationValue::FractionalMillis(millis) => (millis * 1_000_000.0) as i64,
        DurationValue::Text(text) => hocon_duration_nanos(text)?,
    };
    Ok(nanos.div_euclid(1_000_000))
}

/// `SimpleConfig.parseDuration`: the letters at the end are the unit, a
/// singular one longer than two letters read as its plural, and the rest the
/// number, a `long` where it is all digits and a Java double otherwise.
fn hocon_duration_nanos(input: &str) -> Result<i64> {
    let s = input.trim();
    let unit_at = s
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphabetic())
        .last()
        .map_or(s.len(), |(at, _)| at);
    let (number, unit) = (s[..unit_at].trim(), &s[unit_at..]);
    ensure!(!number.is_empty(), "no number in the duration {input:?}");
    let unit = if unit.len() > 2 && !unit.ends_with('s') {
        format!("{unit}s")
    } else {
        unit.to_string()
    };
    let nanos_per_unit: i64 = match unit.as_str() {
        "" | "ms" | "millis" | "milliseconds" => 1_000_000,
        "us" | "micros" | "microseconds" => 1_000,
        "ns" | "nanos" | "nanoseconds" => 1,
        "d" | "days" => 86_400_000_000_000,
        "h" | "hours" => 3_600_000_000_000,
        "s" | "seconds" => 1_000_000_000,
        "m" | "minutes" => 60_000_000_000,
        other => bail!("the duration {input:?} has a unit HOCON doesn't read: {other:?}"),
    };
    let unsigned = number.strip_prefix(['+', '-']).unwrap_or(number);
    if !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()) {
        let count: i64 = number
            .parse()
            .with_context(|| format!("the duration {input:?} is past a long"))?;
        return Ok(count.saturating_mul(nanos_per_unit));
    }
    let count = java::parse_double(number)
        .ok_or_else(|| anyhow!("the duration {input:?} has no number HOCON reads"))?;
    Ok((count * nanos_per_unit as f64) as i64)
}

/// rskj `MinGasPriceProviderFactory.create`: the fixed price, or a stable one
/// over it where `miner.stableGasPrice` is on. `eth_call` runs a call on
/// this node for the `ETH_CALL` source.
pub fn create(
    fixed_min_gas_price: i64,
    section: Option<&StableGasPriceSection>,
    chain_id: u8,
    eth_call: EthCaller,
) -> Result<Arc<dyn MinGasPriceProvider>> {
    create_with(
        fixed_min_gas_price,
        section,
        chain_id,
        eth_call,
        Arc::new(simple_http_get),
    )
}

/// [`create`], with what the web source fetches through.
fn create_with(
    fixed_min_gas_price: i64,
    section: Option<&StableGasPriceSection>,
    chain_id: u8,
    eth_call: EthCaller,
    http_get: HttpGet,
) -> Result<Arc<dyn MinGasPriceProvider>> {
    let fixed: Arc<dyn MinGasPriceProvider> =
        Arc::new(FixedMinGasPriceProvider::new(fixed_min_gas_price));
    // rskj's `reference.conf` has the section, off, where a node sets none.
    let config = StableGasPriceConfig::read(section.unwrap_or(&StableGasPriceSection::default()))?;
    if !config.enabled {
        info!(
            target: "rustock::mining",
            "Stable min gas price is disabled. Falling back to FIXED with fixedMinGasPrice: {fixed_min_gas_price}"
        );
        return Ok(fixed);
    }
    let source: Arc<dyn BtcExchangeRateSource> = match config.method {
        MinGasPriceProviderType::HttpGet => {
            info!(target: "rustock::mining", "Creating 'Http-Get' stable min gas price provider");
            Arc::new(HttpGetSource::with_get(config.http_get()?, http_get)?)
        }
        MinGasPriceProviderType::EthCall => {
            info!(target: "rustock::mining", "Creating 'Eth-Call' stable min gas price provider");
            Arc::new(EthCallSource::new(config.eth_call()?, chain_id, eth_call))
        }
        MinGasPriceProviderType::Fixed => {
            info!(
                target: "rustock::mining",
                "Creating 'Fixed' min gas price provider with fixedMinGasPrice: {fixed_min_gas_price}"
            );
            return Ok(fixed);
        }
    };
    Ok(Arc::new(StableMinGasPriceProvider::new(
        source,
        fixed,
        config.min_stable_gas_price,
        config.refresh_rate_millis,
        config.min_valid_price,
        config.max_valid_price,
    )))
}

/// What fetches a URL's body, with a timeout in milliseconds, as rskj's
/// `SimpleHttpClient.doGet` does.
pub type HttpGet = Arc<dyn Fn(&str, i64) -> Result<String> + Send + Sync>;

/// rskj `HttpGetMinGasPriceProvider`'s source: a bitcoin's price from a web
/// API, at a JSON pointer.
pub struct HttpGetSource {
    config: HttpGetConfig,
    pointer: JsonPointer,
    get: HttpGet,
}

impl fmt::Debug for HttpGetSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpGetSource")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl HttpGetSource {
    /// rskj's constructor reads `jsonPath` as a JSON pointer
    /// (`JsonPointer.valueOf`), which refuses one that isn't empty and doesn't
    /// start with `/`, and its client refuses a timeout past a Java `int` of
    /// milliseconds: rskj doesn't start on either.
    pub fn with_get(config: HttpGetConfig, get: HttpGet) -> Result<Self> {
        let pointer = JsonPointer::compile(&config.json_path)
            .map_err(|e| anyhow!("[mining.stable_gas_price.source.params] json_path: {e}"))?;
        ensure!(
            config.timeout_millis <= i64::from(i32::MAX),
            "[mining.stable_gas_price.source.params] timeout of {} ms is more than rskj's client takes",
            config.timeout_millis
        );
        Ok(Self {
            config,
            pointer,
            get,
        })
    }
}

impl BtcExchangeRateSource for HttpGetSource {
    /// rskj `getBtcExchangeRate`: the answer, unless blank, read at the
    /// pointer.
    fn btc_exchange_rate(&self) -> Option<i64> {
        let response = match (self.get)(&self.config.url, self.config.timeout_millis) {
            Ok(response) => response,
            Err(e) => {
                error!(target: "rustock::mining", "Error getting min gas price from web provider: {e:#}");
                return None;
            }
        };
        if is_blank(&response) {
            return None;
        }
        parse_price(&response, &self.pointer)
    }

    fn provider_type(&self) -> MinGasPriceProviderType {
        MinGasPriceProviderType::HttpGet
    }
}

/// The most of an answer read. rskj reads all of one; no price API answers
/// with more than a few hundred bytes, and a server that sent without end
/// would otherwise have the node hold all it sent.
const MAX_ANSWER: usize = 1 << 20;

/// rskj `SimpleHttpClient.doGet`: the body of a `2xx` answer, as UTF-8, with
/// the lines Java's `readLine` loop reads run together.
fn simple_http_get(url: &str, timeout_millis: i64) -> Result<String> {
    // `HttpURLConnection.setConnectTimeout` refuses one below zero.
    let timeout =
        u64::try_from(timeout_millis).map_err(|_| anyhow!("timeouts can't be negative"))?;
    let fetch = https::Fetch::java(Duration::from_millis(timeout));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("a runtime for the price's fetch")?;
    let mut answer = Capped::default();
    runtime.block_on(https::get_with(url, &fetch, &mut answer))?;
    Ok(joined_lines(&answer.0))
}

/// A body as Java's `readLine` loop gives it: UTF-8, every line ending gone.
fn joined_lines(body: &[u8]) -> String {
    String::from_utf8_lossy(body)
        .chars()
        .filter(|c| !matches!(c, '\n' | '\r'))
        .collect()
}

/// An answer kept whole, up to [`MAX_ANSWER`].
#[derive(Default)]
struct Capped(Vec<u8>);

impl https::Sink for Capped {
    fn write(&mut self, data: &[u8]) -> Result<()> {
        ensure!(
            self.0.len() + data.len() <= MAX_ANSWER,
            "the answer runs past {MAX_ANSWER} bytes"
        );
        self.0.extend_from_slice(data);
        Ok(())
    }
}

/// Apache's `StringUtils.isBlank`, which rskj asks of an answer: nothing in
/// it but what Java's `Character.isWhitespace` takes for whitespace.
fn is_blank(text: &str) -> bool {
    text.chars().all(|c| {
        matches!(
            c,
            '\t'..='\r'
                | '\u{1c}'..=' '
                | '\u{1680}'
                | '\u{2000}'..='\u{2006}'
                | '\u{2008}'..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{205f}'
                | '\u{3000}'
        )
    })
}

/// rskj `HttpGetMinGasPriceProvider.parsePrice`: the answer as Jackson's
/// tree, the node at the pointer, and Jackson's `asLong` of it; nothing where
/// the answer isn't JSON or the pointer finds no node.
fn parse_price(response: &str, pointer: &JsonPointer) -> Option<i64> {
    match java::read_tree(response) {
        Ok(tree) => tree
            .as_ref()
            .and_then(|root| root.at(pointer))
            .map(java::Node::as_long),
        Err(e) => {
            error!(target: "rustock::mining", "Error parsing min gas price from web provider: {e}");
            None
        }
    }
}

/// What runs an `eth_call` on this node at its best block, as rskj's
/// `EthModule.call` does for the provider: the call's answer, a hex string.
pub type EthCaller = Arc<dyn Fn(&Value) -> Result<String> + Send + Sync>;

/// rskj `EthCallMinGasPriceProvider`'s source: a bitcoin's price from a
/// contract on this chain, as this node runs the call.
pub struct EthCallSource {
    config: EthCallConfig,
    chain_id: String,
    call: EthCaller,
}

impl fmt::Debug for EthCallSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EthCallSource")
            .field("config", &self.config)
            .field("chain_id", &self.chain_id)
            .finish_non_exhaustive()
    }
}

impl EthCallSource {
    /// `chain_id` is the node's, which rskj's call names as
    /// `ethModule.chainId()` gives it.
    pub fn new(config: EthCallConfig, chain_id: u8, call: EthCaller) -> Self {
        Self {
            config,
            chain_id: format!("{chain_id:#x}"),
            call,
        }
    }
}

impl BtcExchangeRateSource for EthCallSource {
    /// rskj `getBtcExchangeRate`: the call from `from` to `to` with `data`, at
    /// the best block, its answer read as `HexUtils.jsonHexToLong` reads it;
    /// nothing where the call fails or answers no such number.
    fn btc_exchange_rate(&self) -> Option<i64> {
        let arguments = json!({
            "from": self.config.from,
            "to": self.config.to,
            "chainId": self.chain_id,
            "data": self.config.data,
        });
        match (self.call)(&json!([arguments, "latest"]))
            .and_then(|output| json_hex_to_long(&output))
        {
            Ok(price) => Some(price),
            Err(e) => {
                error!(target: "rustock::mining", "Error calling eth module: {e:#}");
                None
            }
        }
    }

    fn provider_type(&self) -> MinGasPriceProviderType {
        MinGasPriceProviderType::EthCall
    }
}

/// rskj `HexUtils.jsonHexToLong`: `0x`, then hex digits read as a Java
/// `long`.
fn json_hex_to_long(x: &str) -> Result<i64> {
    let digits = x
        .strip_prefix("0x")
        .ok_or_else(|| anyhow!("{x:?} is not hex: it has no 0x"))?;
    i64::from_str_radix(digits, 16).with_context(|| format!("{x:?} is not a hex long"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    const FALLBACK_PRICE: i64 = 10;

    /// A fallback of 10 that counts how often it is asked, as the tests'
    /// mocks do.
    #[derive(Debug, Default)]
    struct CountedFallback {
        asked: AtomicUsize,
    }

    impl MinGasPriceProvider for CountedFallback {
        fn min_gas_price(&self) -> i64 {
            self.asked.fetch_add(1, Ordering::SeqCst);
            FALLBACK_PRICE
        }

        fn provider_type(&self) -> MinGasPriceProviderType {
            MinGasPriceProviderType::Fixed
        }
    }

    fn section(toml_text: &str) -> StableGasPriceSection {
        toml::from_str(toml_text).expect("the section parses")
    }

    // ── rskj HttpGetMinGasPriceProviderTest ─────────────────────────────

    /// The test's `createWebStableSystemConfig`.
    fn web_config() -> HttpGetConfig {
        HttpGetConfig {
            url: "https://rsk.co/price?ids=bitcoin&vs_currencies=usd".into(),
            json_path: "/bitcoin/usd".into(),
            timeout_millis: 3000,
        }
    }

    /// A web that answers `answer` to every request, counting them.
    fn web(answer: std::result::Result<&'static str, &'static str>) -> (HttpGet, Arc<AtomicUsize>) {
        let asked = Arc::new(AtomicUsize::new(0));
        let counted = asked.clone();
        let get: HttpGet = Arc::new(move |_url: &str, _timeout: i64| {
            counted.fetch_add(1, Ordering::SeqCst);
            answer.map(str::to_string).map_err(|e| anyhow!(e))
        });
        (get, asked)
    }

    /// The test's provider over `get`: rskj's `createStableMinGasPriceSystemConfig`,
    /// a stable price of 4,265,280,000,000, kept 30 s, for a bitcoin priced
    /// 1,000 to 100,000.
    fn web_provider(get: HttpGet, fallback: Arc<CountedFallback>) -> StableMinGasPriceProvider {
        let source = HttpGetSource::with_get(web_config(), get).unwrap();
        StableMinGasPriceProvider::new(
            Arc::new(source),
            fallback,
            4_265_280_000_000,
            30_000,
            1_000,
            100_000,
        )
    }

    /// rskj `returnsMappedPriceFromWebClient`.
    #[test]
    fn returns_mapped_price_from_web_client() {
        let (get, _) = web(Ok(r#"{"bitcoin":{"usd":10000}}"#));
        let source = HttpGetSource::with_get(web_config(), get).unwrap();
        assert_eq!(source.btc_exchange_rate(), Some(10_000));
    }

    /// rskj `whenRequestingTheValueTwiceCachedValueIsUsed`.
    #[test]
    fn when_requesting_the_value_twice_cached_value_is_used() {
        let (get, asked) = web(Ok(r#"{"bitcoin":{"usd":10000}}"#));
        let provider = web_provider(get, Arc::default());
        assert_eq!(provider.get_min_gas_price(true), 426_528_000);
        assert_eq!(provider.min_gas_price(), 426_528_000);
        assert_eq!(asked.load(Ordering::SeqCst), 1);
    }

    /// rskj `whenEmptyResponseReturnsFallbackProvider`.
    #[test]
    fn when_empty_response_returns_fallback_provider() {
        let fallback = Arc::new(CountedFallback::default());
        let (get, _) = web(Ok(""));
        assert_eq!(
            web_provider(get, fallback.clone()).get_min_gas_price(true),
            FALLBACK_PRICE
        );
        assert_eq!(fallback.asked.load(Ordering::SeqCst), 1);
    }

    /// rskj `whenErrorReturnsFallbackProvider`.
    #[test]
    fn when_error_returns_fallback_provider() {
        let fallback = Arc::new(CountedFallback::default());
        let (get, _) = web(Err("Error"));
        assert_eq!(
            web_provider(get, fallback.clone()).get_min_gas_price(true),
            FALLBACK_PRICE
        );
        assert_eq!(fallback.asked.load(Ordering::SeqCst), 1);
    }

    /// rskj `whenPathIsWrongReturnsFallBack`.
    #[test]
    fn when_path_is_wrong_returns_fall_back() {
        let fallback = Arc::new(CountedFallback::default());
        let (get, _) = web(Ok(r#"{"btc":{"usd":10000}}"#));
        assert_eq!(
            web_provider(get, fallback.clone()).get_min_gas_price(true),
            FALLBACK_PRICE
        );
        assert_eq!(fallback.asked.load(Ordering::SeqCst), 1);
    }

    // ── rskj EthCallMinGasPriceProviderTest ─────────────────────────────

    const ORACLE: &str = "0xbffBD993FF1d229B0FfE55668F2009d20d4F7C5f";

    /// A node whose `eth_call` answers `answer`, keeping what it was asked.
    fn node(
        answer: std::result::Result<&'static str, &'static str>,
    ) -> (EthCaller, Arc<Mutex<Vec<Value>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let kept = calls.clone();
        let call: EthCaller = Arc::new(move |params: &Value| {
            kept.lock().unwrap().push(params.clone());
            answer.map(str::to_string).map_err(|e| anyhow!(e))
        });
        (call, calls)
    }

    fn oracle_source(data: &str, call: EthCaller) -> EthCallSource {
        let config = EthCallConfig {
            to: ORACLE.into(),
            from: ORACLE.into(),
            data: data.into(),
        };
        EthCallSource::new(config, 33, call)
    }

    /// rskj `constructorSetsFieldsCorrectly`, for the two values the settings
    /// can give; they can't give none (rskj's `getString` refuses that too).
    #[test]
    fn constructor_sets_fields_correctly() {
        for data in ["0x123", "0xabc"] {
            let config = EthCallConfig {
                to: "0xaddress".into(),
                from: "0xfrom".into(),
                data: data.into(),
            };
            let source = EthCallSource::new(config, 33, node(Ok("0x0")).0);
            // rskj's getters `getToAddress`, `getFromAddress` and `getData`.
            assert_eq!(source.config.to, "0xaddress");
            assert_eq!(source.config.from, "0xfrom");
            assert_eq!(source.config.data, data);
        }
    }

    /// rskj `constructorSetsFieldsToNullWhenConfigReturnsNull` has a mocked
    /// configuration give none. rustock's settings can't: an `ETH_CALL`
    /// source without `to`, `from` or `data` is refused, as rskj's
    /// `EthCallMinGasPriceSystemConfig` refuses it.
    #[test]
    fn the_settings_refuse_an_eth_call_without_to_from_or_data() {
        let full = r#"
            enabled = true
            [source]
            method = "ETH_CALL"
            [source.params]
            to = "0x77045E71a7A2c50903d88e564cD72fab11e82051"
            from = "0xCD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826"
            data = "0x98d5fdca"
        "#;
        assert!(StableGasPriceConfig::read(&section(full))
            .unwrap()
            .eth_call()
            .is_ok());
        for key in ["to", "from", "data"] {
            let without: String = full
                .lines()
                .filter(|line| !line.trim_start().starts_with(&format!("{key} =")))
                .collect::<Vec<_>>()
                .join("\n");
            let config = StableGasPriceConfig::read(&section(&without)).unwrap();
            assert!(config.eth_call().is_err(), "without {key}");
        }
    }

    /// rskj `getStableMinGasPrice_callsEthModulesCallMethod`: the call is made
    /// from and to the configured addresses with the data and the chain's id,
    /// at the best block, and `0x21` is 33.
    #[test]
    fn get_stable_min_gas_price_calls_eth_modules_call_method() {
        let (call, calls) = node(Ok("0x21"));
        let source = oracle_source("0x", call);
        assert_eq!(source.btc_exchange_rate(), Some(33));
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls[0],
            json!([
                {"from": ORACLE, "to": ORACLE, "chainId": "0x21", "data": "0x"},
                "latest"
            ])
        );
    }

    /// rskj `getStableMinGasPrice_callsFallback_whenNoData`: a call that
    /// answers nothing gives no price, whatever data it was given.
    #[test]
    fn get_stable_min_gas_price_calls_fallback_when_no_data() {
        for data in ["", "0x"] {
            let source = oracle_source(data, node(Err("no answer")).0);
            assert_eq!(source.btc_exchange_rate(), None, "data {data:?}");
        }
    }

    /// rskj `getStableMinGasPrice_callsFallback_whenEthModuleIsNull`: a node
    /// that can't run the call yet gives no price.
    #[test]
    fn get_stable_min_gas_price_calls_fallback_when_eth_module_is_null() {
        let source = oracle_source(
            "0x",
            node(Err("the node's state is not open to calls yet")).0,
        );
        assert_eq!(source.btc_exchange_rate(), None);
    }

    /// rskj `getType_returnsOnChain`.
    #[test]
    fn get_type_returns_on_chain() {
        let source = oracle_source("0x", node(Ok("0x0")).0);
        assert_eq!(source.provider_type(), MinGasPriceProviderType::EthCall);
    }

    /// `HexUtils.jsonHexToLong`: `0x` first, a Java long's hex digits after.
    #[test]
    fn a_call_answer_is_read_as_a_hex_long() {
        assert_eq!(json_hex_to_long("0x21").unwrap(), 33);
        assert_eq!(
            json_hex_to_long(&format!("0x{:064x}", 60_000)).unwrap(),
            60_000,
            "a word with its leading zeros"
        );
        assert!(json_hex_to_long("0x").is_err(), "no digits");
        assert!(json_hex_to_long("21").is_err(), "no 0x");
        assert!(
            json_hex_to_long(&format!("0x{}", "f".repeat(64))).is_err(),
            "past a long"
        );
    }

    // ── rskj MinGasPriceProviderFactoryTest ──────────────────────────────

    fn no_node() -> EthCaller {
        node(Err("no node")).0
    }

    /// rskj `createFixedMinGasPriceProvider`.
    #[test]
    fn create_fixed_min_gas_price_provider() {
        let config = section(
            r#"
            enabled = true
            refresh_rate = 10
            min_stable_gas_price = 100
            source = { method = "FIXED" }
            "#,
        );
        let provider = create(100, Some(&config), 33, no_node()).unwrap();
        assert_eq!(provider.min_gas_price(), 100);
        assert_eq!(provider.provider_type(), MinGasPriceProviderType::Fixed);
    }

    /// rskj `createWithNullConfig`.
    #[test]
    fn create_with_null_config() {
        let provider = create(100, None, 33, no_node()).unwrap();
        assert_eq!(provider.min_gas_price(), 100);
        assert_eq!(provider.provider_type(), MinGasPriceProviderType::Fixed);
    }

    /// rskj `createWebProviderMethod`: a web source on a URL that can't be
    /// fetched, whose price is the fixed one, through rustock's own client.
    #[test]
    fn create_web_provider_method() {
        let config = section(
            r#"
            enabled = true
            refresh_rate = 10
            min_stable_gas_price = 100
            [source]
            method = "HTTP_GET"
            [source.params]
            url = "url"
            timeout = 1000
            json_path = "/price"
            "#,
        );
        let provider = create(100, Some(&config), 33, no_node()).unwrap();
        assert_eq!(provider.min_gas_price(), 100);
        assert_eq!(provider.provider_type(), MinGasPriceProviderType::HttpGet);
    }

    /// rskj `createWithDisabledConfigReturnFixed`: off, the source isn't
    /// read at all, its pointer that isn't one included.
    #[test]
    fn create_with_disabled_config_return_fixed() {
        let config = section(
            r#"
            enabled = false
            refresh_rate = 10
            min_stable_gas_price = 100
            [source]
            method = "HTTP_GET"
            [source.params]
            url = "url"
            timeout = 1000
            json_path = "price"
            "#,
        );
        let provider = create(100, Some(&config), 33, no_node()).unwrap();
        assert_eq!(provider.min_gas_price(), 100);
        assert_eq!(provider.provider_type(), MinGasPriceProviderType::Fixed);
    }

    /// On, a web source whose pointer isn't one, or whose timeout is past a
    /// Java `int` of milliseconds, stops the node, as it stops rskj.
    #[test]
    fn a_web_source_rskj_would_not_start_on_is_refused() {
        let web_section = |json_path: &str, timeout: &str| {
            section(&format!(
                "enabled = true\n[source]\nmethod = \"HTTP_GET\"\n[source.params]\n\
                 url = \"https://price.example/\"\njson_path = \"{json_path}\"\ntimeout = {timeout}\n"
            ))
        };
        let (get, _) = web(Ok("1"));
        let made = |s: StableGasPriceSection| create_with(0, Some(&s), 33, no_node(), get.clone());
        assert!(made(web_section("/price", "1000")).is_ok());
        assert!(
            made(web_section("", "1000")).is_ok(),
            "the empty pointer is the whole answer"
        );
        assert!(made(web_section("price", "1000")).is_err());
        assert!(made(web_section("/price", "2147483648")).is_err());
    }

    // ── rskj EthCallMinGasPriceSystemConfigTest ─────────────────────────

    fn eth_call_config() -> EthCallConfig {
        let config = section(
            r#"
            enabled = true
            [source]
            method = "ETH_CALL"
            [source.params]
            to = "0x77045E71a7A2c50903d88e564cD72fab11e82051"
            from = "0xCD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826"
            data = "0x98d5fdca"
            "#,
        );
        StableGasPriceConfig::read(&config)
            .unwrap()
            .eth_call()
            .unwrap()
    }

    /// rskj `testAddress`.
    #[test]
    fn test_address() {
        assert_eq!(
            eth_call_config().to,
            "0x77045E71a7A2c50903d88e564cD72fab11e82051"
        );
    }

    /// rskj `testFrom`.
    #[test]
    fn test_from() {
        assert_eq!(
            eth_call_config().from,
            "0xCD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826"
        );
    }

    /// rskj `testData`.
    #[test]
    fn test_data() {
        assert_eq!(eth_call_config().data, "0x98d5fdca");
    }

    // ── rskj HttpGetStableMinGasSystemConfigTest ─────────────────────────

    fn http_get_config() -> HttpGetConfig {
        let config = section(
            r#"
            enabled = true
            [source]
            method = "HTTP_GET"
            [source.params]
            url = "http://test.url"
            json_path = "testPath"
            timeout = "1000 milliseconds"
            "#,
        );
        StableGasPriceConfig::read(&config)
            .unwrap()
            .http_get()
            .unwrap()
    }

    /// rskj `testGetUrl`.
    #[test]
    fn test_get_url() {
        assert_eq!(http_get_config().url, "http://test.url");
    }

    /// rskj `testRequestPath`: the settings keep the path as written; only
    /// the source reads it as a pointer.
    #[test]
    fn test_request_path() {
        assert_eq!(http_get_config().json_path, "testPath");
    }

    /// rskj `testGetTimeout`.
    #[test]
    fn test_get_timeout() {
        assert_eq!(http_get_config().timeout_millis, 1000);
    }

    // ── rskj StableMinGasPriceSystemConfigTest ──────────────────────────

    fn stable_config() -> StableGasPriceConfig {
        StableGasPriceConfig::read(&section(
            r#"
            enabled = true
            refresh_rate = "10 hours"
            min_stable_gas_price = 100
            source = { method = "FIXED" }
            "#,
        ))
        .unwrap()
    }

    /// rskj `testGetRefreshRate`.
    #[test]
    fn test_get_refresh_rate() {
        assert_eq!(stable_config().refresh_rate_millis, 10 * 3_600_000);
    }

    /// rskj `testGetMinStableGasPrice`.
    #[test]
    fn test_get_min_stable_gas_price() {
        assert_eq!(stable_config().min_stable_gas_price, 100);
    }

    /// rskj `testIsEnabled`.
    #[test]
    fn test_is_enabled() {
        assert!(stable_config().enabled);
    }

    /// rskj `testGetMethod`.
    #[test]
    fn test_get_method() {
        assert_eq!(stable_config().method, MinGasPriceProviderType::Fixed);
    }

    /// rskj `wrongMethodShouldThrowException`.
    #[test]
    fn wrong_method_should_throw_exception() {
        let config = section(
            r#"
            enabled = true
            refresh_rate = 10
            min_stable_gas_price = 100
            source = { method = "INVALID" }
            "#,
        );
        assert!(StableGasPriceConfig::read(&config).is_err());
    }

    /// rskj `missingPropertyShouldThrowException`: a `method` beside
    /// `enabled` is no key of the section, and the file is refused; without
    /// it, the section names no `source.method`, which an enabled one must.
    #[test]
    fn missing_property_should_throw_exception() {
        let stray: std::result::Result<StableGasPriceSection, _> =
            toml::from_str("enabled = true\nmin_stable_gas_price = 100\nmethod = \"FIXED\"\n");
        assert!(stray.is_err());
        let config = section("enabled = true\nmin_stable_gas_price = 100\n");
        assert!(StableGasPriceConfig::read(&config).is_err());
    }

    /// rskj `testRanges`.
    #[test]
    fn test_ranges() {
        let config = section(
            r#"
            enabled = true
            min_valid_price = 10
            max_valid_price = 1000
            source = { method = "FIXED" }
            "#,
        );
        let config = StableGasPriceConfig::read(&config).unwrap();
        assert_eq!(config.min_valid_price, 10);
        assert_eq!(config.max_valid_price, 1000);
    }

    /// Off, nothing is read but the valid range: no refresh rate, no stable
    /// price, `FIXED`, whatever the section says.
    #[test]
    fn off_the_section_is_fixed_whatever_it_says() {
        let config = StableGasPriceConfig::read(&section(
            r#"
            refresh_rate = "not a duration"
            min_stable_gas_price = 100
            source = { method = "INVALID" }
            "#,
        ))
        .unwrap();
        assert!(!config.enabled);
        assert_eq!(config.refresh_rate_millis, 0);
        assert_eq!(config.min_stable_gas_price, 0);
        assert_eq!(config.method, MinGasPriceProviderType::Fixed);
        assert_eq!(
            (config.min_valid_price, config.max_valid_price),
            (0, i64::MAX)
        );
    }

    // ── reading as rskj reads ─────────────────────────────────────────────

    /// HOCON's durations: a number alone is milliseconds, units long or
    /// short, singular or plural, a fraction of one, and a space or none.
    #[test]
    fn durations_are_read_as_hocon_reads_them() {
        let ms = |text: &str| duration_millis(&DurationValue::Text(text.into()));
        assert_eq!(duration_millis(&DurationValue::Millis(10)).unwrap(), 10);
        assert_eq!(
            duration_millis(&DurationValue::FractionalMillis(1.5)).unwrap(),
            1
        );
        assert_eq!(ms("10").unwrap(), 10);
        assert_eq!(ms("6 hours").unwrap(), 6 * 3_600_000);
        assert_eq!(ms("1 hour").unwrap(), 3_600_000);
        assert_eq!(ms("2 seconds").unwrap(), 2_000);
        assert_eq!(ms("2s").unwrap(), 2_000);
        assert_eq!(ms("1.5 seconds").unwrap(), 1_500);
        assert_eq!(ms("2.5 minutes").unwrap(), 150_000);
        assert_eq!(ms("4 days").unwrap(), 4 * 86_400_000);
        assert_eq!(ms("1000 milliseconds").unwrap(), 1_000);
        assert_eq!(ms("1 millisecond").unwrap(), 1);
        assert_eq!(ms("1500 us").unwrap(), 1);
        assert_eq!(ms("-1 ns").unwrap(), -1, "Duration.toMillis rounds down");
        assert_eq!(ms("0x1p3").unwrap(), 8, "a Java hexadecimal double");
        assert_eq!(ms("0x1p3 s").unwrap(), 8_000);
        assert_eq!(ms("0x1p3d").unwrap(), 8 * 86_400_000, "whose d is the unit");
        assert_eq!(ms("NaN ms").unwrap(), 0);
        assert_eq!(ms("Infinity ms").unwrap(), i64::MAX / 1_000_000);
        assert!(ms("hours").is_err(), "no number");
        assert!(ms("3 fortnights").is_err(), "no such unit");
        assert!(ms("1.2.3 s").is_err(), "no number HOCON reads");
    }

    /// rskj's `parsePrice`, through Jackson's tree and `asLong`: a whole
    /// number's low 64 bits, a double cast as Java casts it, a string read
    /// as `parseAsLong` reads it, `true` as 1, anything else 0; nothing
    /// where the answer is no JSON or the pointer names nothing.
    #[test]
    fn a_price_is_read_as_jackson_reads_it() {
        let price =
            |answer: &str, at: &str| parse_price(answer, &JsonPointer::compile(at).unwrap());
        assert_eq!(
            price("60000", ""),
            Some(60_000),
            "the empty pointer is the answer"
        );
        assert_eq!(price(r#"{"p":60000.9}"#, "/p"), Some(60_000));
        assert_eq!(price(r#"{"p":-60000.9}"#, "/p"), Some(-60_000));
        assert_eq!(price(r#"{"p":1e4}"#, "/p"), Some(10_000));
        assert_eq!(
            price(r#"{"p":1e400}"#, "/p"),
            Some(i64::MAX),
            "past a double, infinite"
        );
        assert_eq!(price(r#"{"p":" 60000 "}"#, "/p"), Some(60_000));
        assert_eq!(price(r#"{"p":"60000.5"}"#, "/p"), Some(60_000));
        assert_eq!(price(r#"{"p":"+60000"}"#, "/p"), Some(60_000));
        assert_eq!(price(r#"{"p":"1.5e3d"}"#, "/p"), Some(1_500));
        assert_eq!(
            price(r#"{"p":"0x1p4"}"#, "/p"),
            Some(16),
            "a hexadecimal double"
        );
        assert_eq!(
            price(r#"{"p":"99999999999999999999"}"#, "/p"),
            Some(0),
            "past a long"
        );
        assert_eq!(price(r#"{"p":"abc"}"#, "/p"), Some(0));
        assert_eq!(price(r#"{"p":"NaN"}"#, "/p"), Some(0));
        assert_eq!(price(r#"{"p":"Infinity"}"#, "/p"), Some(i64::MAX));
        assert_eq!(price(r#"{"p":"inf"}"#, "/p"), Some(0), "Java reads no inf");
        assert_eq!(price(r#"{"p":true}"#, "/p"), Some(1));
        assert_eq!(price(r#"{"p":null}"#, "/p"), Some(0));
        assert_eq!(price(r#"{"p":{"q":1}}"#, "/p"), Some(0));
        assert_eq!(price(r#"{"p":[7,8]}"#, "/p/1"), Some(8));
        assert_eq!(
            price(r#"{"p":9223372036854775808}"#, "/p"),
            Some(i64::MIN),
            "a long's low 64 bits"
        );
        assert_eq!(
            price(r#"{"p":18446744073709551621}"#, "/p"),
            Some(5),
            "a BigInteger's too"
        );
        assert_eq!(
            price(r#"{"p":1} trailing"#, "/p"),
            Some(1),
            "what follows the value is left"
        );
        assert_eq!(
            price("7x", ""),
            None,
            "a number at the top runs on into no space"
        );
        assert_eq!(price(r#"{"p":1"#, "/p"), None, "no JSON");
        assert_eq!(price(r#"{"q":1}"#, "/p"), None, "no node");
        assert!(is_blank(" \t\u{1c}\u{3000}"), "Java's whitespace");
        assert!(!is_blank("\u{a0}"), "which a no-break space isn't");
    }

    /// Java's `readLine` loop drops every line ending; the cap stops an
    /// answer past a mebibyte.
    #[test]
    fn an_answer_is_read_as_rskjs_client_reads_it() {
        assert_eq!(
            joined_lines(b"{\"a\":\r\n1,\n\"b\":\r2}"),
            "{\"a\":1,\"b\":2}"
        );
        let mut answer = Capped::default();
        assert!(https::Sink::write(&mut answer, &vec![b' '; MAX_ANSWER]).is_ok());
        assert!(https::Sink::write(&mut answer, b" ").is_err());
        assert!(
            simple_http_get("https://price.example/", -1).is_err(),
            "a negative timeout"
        );
    }
}
