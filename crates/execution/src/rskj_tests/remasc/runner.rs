//! rskj's `RemascTestRunner`, which the REMASC tests share: a regtest chain
//! of blocks 1 to 16, each mined by its own coinbase and holding a transfer
//! whose gas is its fees, then REMASC's transaction; siblings of its blocks,
//! mined on their parents, are included as uncles. On regtest's REMASC
//! constants (maturity 10, synthetic span 5) block 16 pays for block 6, out
//! of a reward balance the blocks before it filled.
//!
//! rskj's runner gives its blocks a minimum gas price of 10, over their
//! transfers' price of 1, with its block validation off; validation refuses
//! such a block (`TxsMinGasPriceRule`), and so does rustock's execution.
//! Here a block's minimum is its transfer's price: with RSKIP85 off, rskj's
//! figures don't depend on it.

use alloy_primitives::{Address, B256, U256};
use rustock_core::{Header, Transaction};

use crate::bridge::constants::BridgeConstants;
use crate::hardfork::RskHardforkConfig;
use crate::precompiles::REMASC_ADDR;
use crate::remasc::{
    remasc_storage_key, RemascConfig, BURNED_BALANCE_KEY, FEDERATION_BALANCE_KEY,
    REWARD_BALANCE_KEY,
};
use crate::rskj_tests::vm::harness::{call, Chain, ONE_RBTC, SENDER};

/// The chain's last block, which pays for block 6.
pub(super) const LAST: u64 = 16;
/// Where each block's transfer goes.
const PAYEE: Address = Address::repeat_byte(0x0e);

/// The coinbase of block `number`.
pub(super) fn coinbase(number: u64) -> Address {
    let mut bytes = [0xcb; 20];
    bytes[19] = number as u8;
    Address::from(bytes)
}

/// The coinbase of the `index`th sibling.
pub(super) fn sibling_coinbase(index: usize) -> Address {
    let mut bytes = [0x5b; 20];
    bytes[19] = index as u8;
    Address::from(bytes)
}

/// Regtest's REMASC constants with RSKIP85 off, as
/// `ActivationConfigsForTest.allBut(RSKIP85)` leaves it: with it, a reward
/// under the minimum gas price times 200,000 is held back, as at rskj's
/// runner's minimum of 10 every reward would be.
pub(super) fn without_rskip85() -> RemascConfig {
    let mut remasc = RemascConfig::regtest();
    remasc.rskip85_activation_height = Some(u64::MAX);
    remasc
}

/// Runs the chain on `hardforks` and `remasc`, with transfers at
/// `gas_price` and `siblings`, each a pair of the block it is a sibling of
/// and the block that includes it; `prepare` writes what the chain holds
/// before block 1.
pub(super) fn run(
    hardforks: RskHardforkConfig,
    remasc: RemascConfig,
    gas_price: u64,
    siblings: &[(u64, u64)],
    prepare: impl FnOnce(&mut Chain),
) -> Chain {
    let mut chain = Chain::new(hardforks, 1).map_executor(|executor| {
        let executor = executor.with_remasc_config(remasc);
        executor.with_bridge_constants(BridgeConstants::mainnet())
    });
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    prepare(&mut chain);
    let mut hashes = vec![B256::ZERO];
    for number in 1..=LAST {
        let ommers: Vec<Header> = siblings
            .iter()
            .enumerate()
            .filter(|(_, (_, included))| *included == number)
            .map(|(index, (of, _))| {
                let parent = hashes[*of as usize - 1];
                header(&chain, *of, parent, sibling_coinbase(index), gas_price, 0)
            })
            .collect();
        let uncles = ommers.len() as u64;
        let parent = hashes[number as usize - 1];
        let header = header(&chain, number, parent, coinbase(number), gas_price, uncles);
        let transfer = Transaction {
            gas_price: U256::from(gas_price),
            ..call(number - 1, PAYEE, U256::from(10_000), 21_000, &[])
        };
        let remasc = Transaction::remasc(number);
        let transactions = [(transfer, SENDER), (remasc, Address::ZERO)];
        chain.import(&header, &ommers, &transactions);
        hashes.push(header.hash());
    }
    chain
}

/// rskj's runner with RSKIP85 off, transfers at a gas price of 1, and
/// `siblings`.
pub(super) fn run_without_rskip85(siblings: &[(u64, u64)]) -> Chain {
    let hardforks = RskHardforkConfig::rskj_regtest();
    run(hardforks, without_rskip85(), 1, siblings, |_| {})
}

/// The header of block `number` on `parent`, mined by `coinbase` at a
/// minimum gas price of `gas_price`, with the fees of 21,000 gas at that
/// price paid and `uncles` uncles.
fn header(
    chain: &Chain,
    number: u64,
    parent: B256,
    coinbase: Address,
    gas_price: u64,
    uncles: u64,
) -> Header {
    Header {
        parent_hash: parent,
        beneficiary: coinbase,
        number,
        paid_fees: U256::from(21_000 * gas_price),
        minimum_gas_price: U256::from(gas_price),
        uncle_count: uncles,
        ..chain.header()
    }
}

/// REMASC's reward balance.
pub(super) fn reward_balance(chain: &Chain) -> U256 {
    chain.storage_word(REMASC_ADDR, remasc_storage_key(REWARD_BALANCE_KEY))
}

/// REMASC's burned balance.
pub(super) fn burned_balance(chain: &Chain) -> U256 {
    chain.storage_word(REMASC_ADDR, remasc_storage_key(BURNED_BALANCE_KEY))
}

/// What REMASC keeps for the federation.
pub(super) fn federation_balance(chain: &Chain) -> U256 {
    chain.storage_word(REMASC_ADDR, remasc_storage_key(FEDERATION_BALANCE_KEY))
}
