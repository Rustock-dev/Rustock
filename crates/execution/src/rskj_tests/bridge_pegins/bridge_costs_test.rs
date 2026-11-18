//! rskj's `BridgeCostsTest` (`co/rsk/peg/BridgeCostsTest.java`): what a call
//! of the Bridge costs before it runs, rskj's `Bridge.getGasForData`.
//!
//! Read in full. Besides `receiveHeaders`' cost it holds each method's fixed
//! cost, which the ports of `BridgeTest` hold, a free Bridge transaction's,
//! which is the executor's, and the cost of a call that names no method,
//! ported here.

use crate::abi::encode::{self, Arg};
use crate::bridge::constants::BridgeConstants;
use crate::hardfork::RskHardforkConfig;

use super::support::ctx_at;

/// A `receiveHeaders` call carrying `count` headers, each the ten bytes
/// rskj's test sends (`00112233445566778899`).
fn receive_headers(count: usize) -> Vec<u8> {
    let header: &[u8] = &[0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99];
    let headers = vec![header; count];
    let mut data = crate::abi::selector("receiveHeaders(bytes[])").to_vec();
    data.extend(encode::args(&[Arg::BytesArray(&headers)]));
    data
}

/// What `data` costs at block `number` of `cfg`, under `config`.
fn cost(data: &[u8], config: &BridgeConstants, cfg: &RskHardforkConfig, number: u64) -> u64 {
    let mut ctx = ctx_at(number);
    crate::bridge::bridge_call_gas_cost(&mut ctx, data, config, cfg).expect("a cost")
}

/// rskj `receiveHeadersGasCost_beforeDynamicCost`: before RSKIP124,
/// `receiveHeaders` costs 22,000 and two gas a byte of its data, however many
/// headers it carries. Mainnet's last block before wasabi100.
#[test]
fn before_rskip124_receive_headers_costs_22000_whatever_it_carries() {
    let (config, cfg) = (BridgeConstants::mainnet(), RskHardforkConfig::mainnet());
    assert!(!cfg.has_rskip124(1_590_999));
    for count in 0..10 {
        let data = receive_headers(count);
        assert_eq!(
            cost(&data, &config, &cfg, 1_590_999),
            22_000 + 2 * data.len() as u64,
            "{count} headers"
        );
    }
}

/// rskj `receiveHeadersGasCost_afterDynamicCost`: from RSKIP124 without
/// RSKIP132, 66,000, 1,650 for each header past the first, and two gas a byte
/// of its data. Testnet has RSKIP124 from block 0 and RSKIP132 from 43,550
/// (`testnet.conf`), so its blocks below 43,550 are the ones that charge it.
#[test]
fn from_rskip124_until_rskip132_receive_headers_costs_66000_and_1650_a_header() {
    let (config, cfg) = (BridgeConstants::testnet(), RskHardforkConfig::testnet());
    assert!(cfg.has_rskip124(43_549) && !cfg.has_rskip132(43_549));
    for count in 0..10u64 {
        let data = receive_headers(count as usize);
        assert_eq!(
            cost(&data, &config, &cfg, 43_549),
            66_000 + 2 * data.len() as u64 + 1_650 * count.saturating_sub(1),
            "{count} headers"
        );
    }
}

/// rskj `receiveHeadersGasCost_afterDynamicCost_review`: from RSKIP132,
/// 25,000, 3,500 for each header past the first, and two gas a byte of its
/// data: testnet from block 43,550 and mainnet from wasabi100.
#[test]
fn from_rskip132_receive_headers_costs_25000_and_3500_a_header() {
    for (config, cfg, number) in [
        (
            BridgeConstants::testnet(),
            RskHardforkConfig::testnet(),
            43_550,
        ),
        (
            BridgeConstants::mainnet(),
            RskHardforkConfig::mainnet(),
            1_591_000,
        ),
    ] {
        assert!(cfg.has_rskip124(number) && cfg.has_rskip132(number));
        for count in 0..10u64 {
            let data = receive_headers(count as usize);
            assert_eq!(
                cost(&data, &config, &cfg, number),
                25_000 + 2 * data.len() as u64 + 3_500 * count.saturating_sub(1),
                "{count} headers at {number}"
            );
        }
    }
}

/// rskj `getGasForDataInvalidFunction`: a call rskj can't parse as one of the
/// Bridge's methods (`Bridge.parseData` answers null) costs what `releaseBtc`
/// costs, 23,000, and nothing for its data: data of one to three bytes, a
/// selector no method has, arguments that don't decode, or a method that
/// isn't enabled yet.
#[test]
fn a_call_that_names_no_method_costs_23000_and_nothing_for_its_data() {
    let (config, cfg) = (BridgeConstants::mainnet(), RskHardforkConfig::mainnet());
    let number = 8_804_200;
    let mut unknown = vec![0xde, 0xad, 0xbe, 0xef];
    unknown.extend_from_slice(&[0x01; 64]);
    let mut undecodable = crate::abi::selector("receiveHeaders(bytes[])").to_vec();
    undecodable.extend_from_slice(&[0xff; 40]);
    for data in [vec![1u8, 2, 3], unknown, undecodable] {
        assert_eq!(cost(&data, &config, &cfg, number), 23_000, "{data:02x?}");
    }
    // getQueuedPegoutsCount comes with RSKIP271 (hop400): before it, it is
    // no method rskj will parse.
    let queued = crate::abi::selector("getQueuedPegoutsCount()").to_vec();
    assert!(!cfg.has_rskip271(4_598_499));
    assert_eq!(cost(&queued, &config, &cfg, 4_598_499), 23_000);
    assert_ne!(cost(&queued, &config, &cfg, 4_598_500), 23_000);
}
