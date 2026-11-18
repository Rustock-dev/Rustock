//! rskj's `BridgeConstantsTest` (`co/rsk/peg/constants/BridgeConstantsTest.java`).
//!
//! Read in full. Its other tests check each network's constants: the minimum
//! peg-out value and the pegout index's two heights, which
//! `bridge/constants.rs`'s own tests hold, the value of an SVP fund
//! transaction's outputs and the proposed federation's Flyover prefix, which
//! are values and no limit, and which constants object rskj builds for each
//! network, which has no counterpart here.

use crate::bridge::constants::BridgeConstants;
use crate::hardfork::{
    RskNetworkUpgrade, RSK_MAINNET_CHAIN_ID, RSK_REGTEST_CHAIN_ID, RSK_TESTNET_CHAIN_ID,
};

use super::support::*;

/// A whitelisted P2PKH sender's peg-in of `amount` satoshis to `config`'s
/// genesis federation, registered at RSK block 10 of the ladder up to `last`
/// on `chain_id`'s network, at a Bitcoin height `config` reads no block below:
/// whether the sender was credited.
fn credited(config: &BridgeConstants, chain_id: u64, last: RskNetworkUpgrade, amount: u64) -> bool {
    let cfg = up_to(last, chain_id);
    let sender = public_key(&[0x51]);
    let pegin = tx(
        vec![p2pkh_spend(&sender)],
        &[(amount, genesis_federation_output(config))],
    );
    let height = config.btc_height_when_block_index_activates + 100_000;
    let mut ctx = bridge_holding(&pegin, height, 10, config, &cfg);
    whitelist_once(&mut ctx, hash160(&sender), amount);
    let registered = register(&mut ctx, &pegin, i64::from(height), config, &cfg);
    assert!(registered.returned);
    let credited = balance(&mut ctx, rsk_address_of(&sender)) == wei(amount);
    // A peg-in under the minimum is no peg-in: nothing of it is kept.
    assert_eq!(registered.processed, credited, "{amount} on {chain_id}");
    credited
}

/// rskj `getMinimumPeginTxValue`: the minimum a peg-in must reach is the
/// legacy one, 0.01 BTC, before RSKIP219, and 0.005 BTC from it, on mainnet,
/// testnet and regtest. Run at papyrus200 and at iris300, which brings
/// RSKIP219.
#[test]
fn the_minimum_peg_in_is_the_legacy_one_before_rskip219_and_the_new_one_from_it() {
    use RskNetworkUpgrade::{Iris300, Papyrus200};
    for (config, chain_id) in [
        (BridgeConstants::mainnet(), RSK_MAINNET_CHAIN_ID),
        (BridgeConstants::testnet(), RSK_TESTNET_CHAIN_ID),
        (BridgeConstants::rskj_regtest(), RSK_REGTEST_CHAIN_ID),
    ] {
        assert!(!up_to(Papyrus200, chain_id).has_rskip219(10));
        assert!(up_to(Iris300, chain_id).has_rskip219(10));
        let (legacy, minimum) = (
            config.legacy_minimum_pegin_tx_value,
            config.minimum_pegin_tx_value,
        );
        assert_eq!((legacy, minimum), (1_000_000, 500_000));

        assert!(!credited(&config, chain_id, Papyrus200, legacy - 1));
        assert!(credited(&config, chain_id, Papyrus200, legacy));
        assert!(!credited(&config, chain_id, Papyrus200, minimum));

        assert!(!credited(&config, chain_id, Iris300, minimum - 1));
        assert!(credited(&config, chain_id, Iris300, minimum));
    }
}
