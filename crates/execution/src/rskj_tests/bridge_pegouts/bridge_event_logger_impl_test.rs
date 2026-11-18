//! rskj's `BridgeEventLoggerImplTest`: the Bridge's events in the Solidity
//! format, here those whose data RSKIP326, RSKIP383 and RSKIP427 change.
//! rskj's tests give the logger an activation config; the ports call
//! mainnet's Bridge at a block before each rule and from it, and read what
//! it logs.

use alloy_primitives::U256;

use super::support::*;

/// The one log among `logged` whose first topic is `topic`.
fn the(logged: &[revm::primitives::Log], topic: [u8; 32]) -> &revm::primitives::Log {
    let matching: Vec<_> = logged
        .iter()
        .filter(|log| topics(log)[0] == topic)
        .collect();
    assert_eq!(matching.len(), 1, "one log of the topic");
    matching[0]
}

/// [`STRANGER`], who calls, as a topic.
fn sender_topic() -> [u8; 32] {
    let mut word = [0u8; 32];
    word[12..].copy_from_slice(STRANGER.as_slice());
    word
}

/// rskj's `logReleaseBtcRequestReceived_preRSKIP326_destinationAddressAsHash160`,
/// `logReleaseBtcRequestReceived_postRSKIP326_destinationAddressAsBase58` and
/// `logReleaseBtcRequestReceived_postRSKIP427_amountAsWeis`: a peg-out request
/// taken is logged as `release_request_received`, its sender a topic. Before
/// RSKIP326 (fingerroot500) its destination is the hash160 of its address, as
/// bytes; from it the address in Base58, and the event's signature changes
/// with it. Before RSKIP427 (lovell700) the amount is in satoshis, from it in
/// wei.
#[test]
fn a_pegout_request_taken_logs_its_destination_and_amount_as_its_era_has_them() {
    let eras = [
        (MAINNET_HOP400, false, false),
        (MAINNET_ARROWHEAD600, true, false),
        (MAINNET_LOVELL700, true, true),
    ];
    for (number, rskip326, rskip427) in eras {
        let mut bridge = Bridge::mainnet_at(number);
        let hash160 = bridge.tx.btc_sender_hash160;
        bridge.release(BTC);
        let logged = logs(&mut bridge.ctx);
        let (signature, destination) = if rskip326 {
            (
                "release_request_received(address,string,uint256)",
                bitcoin::base58::encode_check(&[&[0x00][..], &hash160].concat()).into_bytes(),
            )
        } else {
            (
                "release_request_received(address,bytes,uint256)",
                hash160.to_vec(),
            )
        };
        let log = the(&logged, event_topic(signature));
        assert_eq!(topics(log)[1], sender_topic(), "{number}");
        let amount = if rskip427 {
            U256::from(BTC) * U256::from(WEI_PER_SATOSHI)
        } else {
            U256::from(BTC)
        };
        let data = log.data.data.to_vec();
        assert_eq!(word(&data, 0), U256::from(64), "{number}");
        assert_eq!(word(&data, 1), amount, "{number}");
        assert_eq!(word(&data, 2), U256::from(destination.len()), "{number}");
        assert_eq!(
            &data[96..96 + destination.len()],
            destination.as_slice(),
            "{number}"
        );
    }
}

/// rskj's `logReleaseBtcRequestRejected_preRSKIP427_logAmountAsSatoshis` and
/// `logReleaseBtcRequestRejected_postRSKIP427_logAmountAsWeis`: a peg-out
/// request turned down, here for an amount below the minimum (LOW_AMOUNT,
/// 1), is logged as `release_request_rejected` with the amount in satoshis
/// before RSKIP427 and in wei from it. The fee per kilobyte stored, 10,000
/// satoshis, keeps the fee rskj's `requestRelease` estimates below the
/// minimum, so that the minimum turns the request down; with the genesis
/// fee per kilobyte the estimate is the larger, and the reason
/// FEE_ABOVE_VALUE.
#[test]
fn a_pegout_request_turned_down_logs_its_amount_in_satoshis_before_rskip427_and_in_wei_from_it() {
    let satoshis = BridgeConstants::mainnet().minimum_pegout_tx_value - 1;
    for (number, rskip427) in [(MAINNET_ARROWHEAD600, false), (MAINNET_LOVELL700, true)] {
        let mut bridge = Bridge::mainnet_at(number);
        put_named(
            &mut bridge.ctx,
            "feePerKb",
            &rlp_encode_element(&[0x27, 0x10]),
        );
        bridge.release(satoshis);
        let logged = logs(&mut bridge.ctx);
        let log = the(
            &logged,
            event_topic("release_request_rejected(address,uint256,int256)"),
        );
        assert_eq!(topics(log)[1], sender_topic(), "{number}");
        let amount = if rskip427 {
            U256::from(satoshis) * U256::from(WEI_PER_SATOSHI)
        } else {
            U256::from(satoshis)
        };
        assert_eq!(word(&log.data.data, 0), amount, "{number}");
        assert_eq!(word(&log.data.data, 1), U256::from(1), "{number}");
    }
}

/// rskj's `logCommitFederation`, run without RSKIP383 and with it: a
/// federation committed is logged with its activation block, the commit's
/// block and mainnet's activation age, 18,500 blocks before RSKIP383
/// (fingerroot500) and 40,320 from it.
#[test]
fn a_federation_committed_is_logged_with_the_activation_age_of_its_era() {
    for (number, age) in [(MAINNET_HOP400, 18_500u64), (MAINNET_FINGERROOT500, 40_320)] {
        let mut bridge = Bridge::mainnet_at(number);
        assert_eq!(elect(&mut bridge, &keys(20, 3)), 1, "{number}");
        let logged = logs(&mut bridge.ctx);
        let log = the(
            &logged,
            event_topic("commit_federation(bytes,string,bytes,string,int256)"),
        );
        assert_eq!(
            word(&log.data.data, 4),
            U256::from(number + age),
            "{number}"
        );
    }
}
