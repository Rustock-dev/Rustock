//! Where the miner's minimum gas price comes from: rskj's
//! `co.rsk.mine.gas.provider`.
//!
//! A fixed price, rskj's `miner.minGasPrice`. Or, with `miner.stableGasPrice`
//! on, one held stable in a currency: its `minStableGasPrice` over the price of
//! a bitcoin in that currency, which a source fetches now and then, from a web
//! API or a contract. The price is kept between fetches, and the fixed one
//! stands in until a fetch brings one. A block steps its minimum toward
//! whichever it is by RSKIP-09's 1% (`MinimumGasPriceCalculator`).
//!
//! A fetch runs on a thread of its own, as rskj's does: a miner building a
//! block takes the price in hand and doesn't wait on the network.

use alloy_primitives::U256;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, warn};

/// rskj `MinGasPriceProviderType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinGasPriceProviderType {
    Fixed,
    HttpGet,
    EthCall,
}

/// rskj `MinGasPriceProvider`.
pub trait MinGasPriceProvider: Send + Sync + fmt::Debug {
    /// `getMinGasPrice`: the price, in wei, a block steers toward now, a Java
    /// `long` as rskj holds it.
    fn min_gas_price(&self) -> i64;
    /// `getType`.
    fn provider_type(&self) -> MinGasPriceProviderType;
    /// `getMinGasPriceAsCoin`, as the miner steers by it.
    fn min_gas_price_as_coin(&self) -> U256 {
        as_coin(self.min_gas_price())
    }
}

/// A price as `MinimumGasPriceCalculator` steers by it. rskj's `Coin` holds a
/// price below zero, which is in no band a block's minimum may move in, so
/// rskj steps to the bottom of the band, as it does for zero: it reads as zero.
pub fn as_coin(price: i64) -> U256 {
    U256::from(u64::try_from(price).unwrap_or(0))
}

/// rskj `FixedMinGasPriceProvider`: `miner.minGasPrice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedMinGasPriceProvider {
    min_gas_price: i64,
}

impl FixedMinGasPriceProvider {
    pub fn new(min_gas_price: i64) -> Self {
        Self { min_gas_price }
    }
}

impl MinGasPriceProvider for FixedMinGasPriceProvider {
    fn min_gas_price(&self) -> i64 {
        self.min_gas_price
    }

    fn provider_type(&self) -> MinGasPriceProviderType {
        MinGasPriceProviderType::Fixed
    }
}

/// What a stable price is fetched from: rskj's `getBtcExchangeRate`, which
/// each of `StableMinGasPriceProvider`'s two kinds gives.
pub trait BtcExchangeRateSource: Send + Sync + fmt::Debug {
    /// The price of a bitcoin, or nothing where none could be had.
    fn btc_exchange_rate(&self) -> Option<i64>;
    /// `getType` of the provider it makes.
    fn provider_type(&self) -> MinGasPriceProviderType;
}

/// rskj `StableMinGasPriceProvider.ERR_NUM_OF_FAILURES`: failed fetches in a
/// row from which the log says so as an error rather than a warning.
const ERR_NUM_OF_FAILURES: i32 = 20;

/// rskj `StableMinGasPriceProvider`.
#[derive(Debug)]
pub struct StableMinGasPriceProvider {
    shared: Arc<Shared>,
}

/// What the provider and its fetches share.
#[derive(Debug)]
struct Shared {
    source: Arc<dyn BtcExchangeRateSource>,
    fallback: Arc<dyn MinGasPriceProvider>,
    min_stable_gas_price: i64,
    refresh_rate_millis: i64,
    min_valid_price: i64,
    max_valid_price: i64,
    num_of_failures: AtomicI32,
    /// The fetch under way, rskj's `priceFuture`.
    in_flight: Mutex<Option<Arc<Fetch>>>,
    last_min_gas_price: AtomicI64,
    last_update_time_millis: AtomicI64,
}

/// One fetch, rskj's `CompletableFuture<Long>`: the price it came to, once it
/// has.
#[derive(Debug, Default)]
struct Fetch {
    price: Mutex<Option<i64>>,
    done: Condvar,
}

impl Fetch {
    fn complete(&self, price: i64) {
        *self.price.lock().unwrap_or_else(PoisonError::into_inner) = Some(price);
        self.done.notify_all();
    }

    fn is_done(&self) -> bool {
        self.price
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    fn get(&self) -> i64 {
        let mut price = self.price.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(price) = *price {
                return price;
            }
            price = self
                .done
                .wait(price)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// `System.currentTimeMillis()`.
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

impl StableMinGasPriceProvider {
    /// `refresh_rate_millis` is how long a price is kept before the next
    /// fetch, and a bitcoin priced outside `min_valid_price` to
    /// `max_valid_price` is no price.
    pub fn new(
        source: Arc<dyn BtcExchangeRateSource>,
        fallback: Arc<dyn MinGasPriceProvider>,
        min_stable_gas_price: i64,
        refresh_rate_millis: i64,
        min_valid_price: i64,
        max_valid_price: i64,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                source,
                fallback,
                min_stable_gas_price,
                refresh_rate_millis,
                min_valid_price,
                max_valid_price,
                num_of_failures: AtomicI32::new(0),
                in_flight: Mutex::new(None),
                last_min_gas_price: AtomicI64::new(0),
                last_update_time_millis: AtomicI64::new(0),
            }),
        }
    }

    /// rskj `getMinGasPrice(wait)`: once the refresh rate has passed since a
    /// price was last had, a fetch is started, unless one is under way, and
    /// its price is the answer if it is in hand, or once it is if `wait`.
    /// Otherwise the last price had, or the fallback's while there is none.
    pub fn get_min_gas_price(&self, wait: bool) -> i64 {
        let shared = &self.shared;
        let since =
            now_millis().wrapping_sub(shared.last_update_time_millis.load(Ordering::SeqCst));
        if since >= shared.refresh_rate_millis {
            let fetch = Shared::fetch_price_async(shared);
            if wait || fetch.is_done() {
                let price = fetch.get();
                debug!(target: "rustock::mining", "getMinGasPrice returning fetched minGasPrice: {price}");
                return price;
            }
        }
        let price = shared.last_min_gas_price();
        debug!(target: "rustock::mining", "getMinGasPrice returning cached minGasPrice: {price}");
        price
    }

    /// rskj `getMinGasPriceAsCoin(wait)`.
    pub fn get_min_gas_price_as_coin(&self, wait: bool) -> U256 {
        as_coin(self.get_min_gas_price(wait))
    }

    /// rskj `valueBetweenConfiguredRange`.
    pub fn value_between_configured_range(&self, value: i64) -> bool {
        self.shared.value_between_configured_range(value)
    }
}

impl MinGasPriceProvider for StableMinGasPriceProvider {
    fn min_gas_price(&self) -> i64 {
        self.get_min_gas_price(false)
    }

    fn provider_type(&self) -> MinGasPriceProviderType {
        self.shared.source.provider_type()
    }
}

impl Shared {
    /// rskj `getLastMinGasPrice`.
    fn last_min_gas_price(&self) -> i64 {
        let last = self.last_min_gas_price.load(Ordering::SeqCst);
        if last > 0 {
            last
        } else {
            self.fallback.min_gas_price()
        }
    }

    /// rskj `calculateMinGasPriceBasedOnBtcPrice`, in Java's `long`
    /// arithmetic.
    fn calculate_min_gas_price_based_on_btc_price(&self, btc_value: i64) -> i64 {
        if self.min_stable_gas_price == 0 || btc_value == 0 {
            return 0;
        }
        self.min_stable_gas_price.wrapping_div(btc_value)
    }

    /// rskj `fetchPriceAsync`: the fetch under way, or a new one on a thread
    /// of its own.
    fn fetch_price_async(this: &Arc<Self>) -> Arc<Fetch> {
        let mut in_flight = this
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(fetch) = in_flight.as_ref() {
            debug!(target: "rustock::mining", "fetchPriceAsync skipped as there is already price fetching in progress...");
            return fetch.clone();
        }
        let fetch = Arc::new(Fetch::default());
        *in_flight = Some(fetch.clone());
        drop(in_flight);
        debug!(target: "rustock::mining", "fetchPriceAsync...");

        let shared = this.clone();
        let theirs = fetch.clone();
        let spawned = std::thread::Builder::new()
            .name("stable-min-gas-price".into())
            .spawn(move || {
                let response = shared.fetch_price_sync();
                // `priceResponse.orElse(getLastMinGasPrice())`: Java reads the
                // last price first, whatever the response.
                let last = shared.last_min_gas_price();
                theirs.complete(response.unwrap_or(last));
                *shared
                    .in_flight
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = None;
            });
        if let Err(e) = spawned {
            error!(target: "rustock::mining", "Min gas price fetching could not start: {e}");
            *this
                .in_flight
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = None;
            fetch.complete(this.last_min_gas_price());
        }
        fetch
    }

    /// rskj `fetchPriceSync`.
    fn fetch_price_sync(&self) -> Option<i64> {
        debug!(target: "rustock::mining", "fetchPriceSync...");
        let response =
            std::panic::catch_unwind(AssertUnwindSafe(|| self.source.btc_exchange_rate()))
                .unwrap_or(None);
        if let Some(price) = response.filter(|price| self.value_between_configured_range(*price)) {
            let result = self.calculate_min_gas_price_based_on_btc_price(price);
            self.last_min_gas_price.store(result, Ordering::SeqCst);
            self.last_update_time_millis
                .store(now_millis(), Ordering::SeqCst);
            self.num_of_failures.store(0, Ordering::SeqCst);
            debug!(
                target: "rustock::mining",
                "fetchPriceSync completed with priceResponse: {price}, lastMinGasPrice: {result}"
            );
            return Some(result);
        }
        let failed_attempts = self
            .num_of_failures
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        if failed_attempts >= ERR_NUM_OF_FAILURES {
            error!(
                target: "rustock::mining",
                "Gas price was not updated as it was not possible to obtain valid price from provider. \
                 Check your provider setup. Number of failed attempts: {failed_attempts}"
            );
        } else {
            warn!(
                target: "rustock::mining",
                "Gas price was not updated as it was not possible to obtain valid price from provider. \
                 Number of failed attempts: {failed_attempts}"
            );
        }
        None
    }

    fn value_between_configured_range(&self, value: i64) -> bool {
        if value >= self.min_valid_price && value <= self.max_valid_price {
            return true;
        }
        warn!(
            target: "rustock::mining",
            "Gas price value {value} is not in the valid range {} - {}",
            self.min_valid_price,
            self.max_valid_price
        );
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    // ── rskj FixedMinGasPriceProviderTest ───────────────────────────────────

    /// rskj `testConstructorAndGetMinGasPrice`.
    #[test]
    fn test_constructor_and_get_min_gas_price() {
        assert_eq!(FixedMinGasPriceProvider::new(100).min_gas_price(), 100);
    }

    /// rskj `testGetType`.
    #[test]
    fn test_get_type() {
        let provider = FixedMinGasPriceProvider::new(100);
        assert_eq!(provider.provider_type(), MinGasPriceProviderType::Fixed);
    }

    /// rskj `testGetMinGasPriceAsCoin`.
    #[test]
    fn test_get_min_gas_price_as_coin() {
        let provider = FixedMinGasPriceProvider::new(50);
        assert_eq!(provider.min_gas_price_as_coin(), U256::from(50));
    }

    /// A price below zero steers as zero does, to the bottom of the band.
    #[test]
    fn a_price_below_zero_reads_as_zero() {
        assert_eq!(as_coin(-5), U256::ZERO);
        assert_eq!(as_coin(i64::MAX), U256::from(i64::MAX));
    }

    // ── rskj StableMinGasPriceProviderTest ──────────────────────────────────

    const FALLBACK_PRICE: i64 = 10;

    /// The test's fallback, a fixed price of 10 that counts how often it is
    /// asked, as the test's spy does.
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

    /// `TestStableMingGasPriceProvider`'s exchange rate: a bitcoin at 15, or
    /// what a test says, counting how often it is asked.
    #[derive(Debug)]
    struct TestSource {
        rate: Option<i64>,
        asked: AtomicUsize,
    }

    impl BtcExchangeRateSource for TestSource {
        fn btc_exchange_rate(&self) -> Option<i64> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            self.rate
        }

        fn provider_type(&self) -> MinGasPriceProviderType {
            MinGasPriceProviderType::Fixed
        }
    }

    /// `new TestStableMingGasPriceProvider(fallBackProvider, 100,
    /// Duration.ofSeconds(10))`, which takes a bitcoin priced 10 to 100.
    fn provider(
        rate: Option<i64>,
    ) -> (
        StableMinGasPriceProvider,
        Arc<CountedFallback>,
        Arc<TestSource>,
    ) {
        let fallback = Arc::new(CountedFallback::default());
        let source = Arc::new(TestSource {
            rate,
            asked: AtomicUsize::new(0),
        });
        let provider =
            StableMinGasPriceProvider::new(source.clone(), fallback.clone(), 100, 10_000, 10, 100);
        (provider, fallback, source)
    }

    /// rskj `testGetMinGasPrice`: 100 over a bitcoin at 15 is 6, and the
    /// fallback isn't asked.
    #[test]
    fn test_get_min_gas_price() {
        let (provider, fallback, _) = provider(Some(15));
        assert_eq!(provider.get_min_gas_price(true), 6);
        assert_eq!(fallback.asked.load(Ordering::SeqCst), 0);
    }

    /// rskj `GetMinGasPriceUsesFallbackWhenReturnIsNull`.
    #[test]
    fn get_min_gas_price_uses_fallback_when_return_is_null() {
        let (provider, fallback, _) = provider(None);
        assert_eq!(provider.get_min_gas_price(true), FALLBACK_PRICE);
        assert_eq!(fallback.asked.load(Ordering::SeqCst), 1);
    }

    /// rskj `whenRequestingTheValueTwiceCachedValueIsUsed`: the second asking
    /// takes the first's fetch, or the price it brought.
    #[test]
    fn when_requesting_the_value_twice_cached_value_is_used() {
        let (provider, _, source) = provider(Some(15));
        provider.min_gas_price();
        assert_eq!(provider.get_min_gas_price(true), 6);
        assert_eq!(source.asked.load(Ordering::SeqCst), 1);
    }

    /// rskj `testGetMinGasPriceAsCoin`.
    #[test]
    fn test_get_min_gas_price_as_coin_of_a_stable_price() {
        let (provider, _, _) = provider(Some(15));
        assert_eq!(provider.get_min_gas_price_as_coin(true), U256::from(6));
    }

    /// rskj `fallBackPriceIsReturnedWhenProvidedValueIsAboveMaxRange`.
    #[test]
    fn fall_back_price_is_returned_when_provided_value_is_above_max_range() {
        let (provider, _, _) = provider(Some(500));
        assert!(!provider.value_between_configured_range(500));
        assert_eq!(provider.get_min_gas_price(true), FALLBACK_PRICE);
    }

    /// rskj `fallBackPriceIsReturnedWhenProvidedValueIsBelowMinRange`.
    #[test]
    fn fall_back_price_is_returned_when_provided_value_is_below_min_range() {
        let (provider, _, _) = provider(Some(5));
        assert!(!provider.value_between_configured_range(5));
        assert_eq!(provider.get_min_gas_price(true), FALLBACK_PRICE);
    }

    /// A price had is kept for the refresh rate: asked again, the provider
    /// answers it without a fetch, and a price fetched as zero (a minimum
    /// stable price under a bitcoin's) gives way to the fallback between
    /// fetches, as rskj's `getLastMinGasPrice` does.
    #[test]
    fn a_price_is_kept_and_zero_gives_way_to_the_fallback() {
        let (provider, _, source) = provider(Some(15));
        assert_eq!(provider.get_min_gas_price(true), 6);
        assert_eq!(provider.get_min_gas_price(true), 6);
        assert_eq!(source.asked.load(Ordering::SeqCst), 1);

        let fallback = Arc::new(CountedFallback::default());
        let source = Arc::new(TestSource {
            rate: Some(50),
            asked: AtomicUsize::new(0),
        });
        let provider = StableMinGasPriceProvider::new(source, fallback, 10, 10_000, 10, 100);
        assert_eq!(provider.get_min_gas_price(true), 0, "10 over 50, fetched");
        assert_eq!(
            provider.get_min_gas_price(true),
            FALLBACK_PRICE,
            "then kept as none"
        );
    }
}
