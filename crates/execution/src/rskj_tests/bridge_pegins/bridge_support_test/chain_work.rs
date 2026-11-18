//! rskj's `BridgeSupportTest.ChainWorkTests`: Bitcoin mainnet's block
//! 849,138 is the first whose chain work bitcoinj's twelve bytes could not
//! hold, and before RSKIP434 rskj takes no mainnet header from that height on
//! (`BridgeSupport.cannotProcessNextBlock`); from RSKIP434, and on testnet and
//! regtest always, it takes them. rskj's real headers 849,134 to 849,139, and
//! the chain work rskj gives the ones it stores first; Arrowhead600 is before
//! RSKIP434 and Arrowhead631 brings it.

use alloy_primitives::U256;
use bitcoin::block::Header;

use crate::abi::encode::{self, Arg};
use crate::bridge::btc_chain;
use crate::bridge::btc_store::{self, StoredBlock};
use crate::bridge::constants::BridgeConstants;
use crate::hardfork::{
    RskHardforkConfig, RskNetworkUpgrade, RSK_MAINNET_CHAIN_ID, RSK_REGTEST_CHAIN_ID,
    RSK_TESTNET_CHAIN_ID,
};

use super::super::support::*;

use RskNetworkUpgrade::{Arrowhead600, Arrowhead631};

const H849134: &str = "0080b92c24f123130ae29e899f0cab72653722e54cdf3b30445202000000000000000000c72ead65a3b78ab637d1876c00414a77e47bcc5b52667ac1e573633563bea5a695aa7766255d031728d182a8";
const H849135: &str = "00004020bf67910b5d3996ee594848b482ee84d0e28c97a9a2d601000000000000000000865e218552bb92df36c962f5163e84a6c2542584fb36be2fa8b2a4246c73a701f1ae7766255d031791836b22";
const H849136: &str = "0000003a796f8b7a9d6ba6e13064e7c64e94570f877170262f1f0200000000000000000036b2ab17565a24a9be4626ca801cb31f91232034ba848295475f931a58dd5446e5b07766255d03173b01a491";
const H849137: &str = "00e00820925b77c9ff4d0036aa29f3238cde12e9af9d55c34ed30200000000000000000032a9fa3e12ef87a2327b55db6a16a1227bb381db8b269d90aa3a6e38cf39665f91b47766255d0317c1b1575f";
const H849138: &str = "006001207ca158816ffc9d45b9ecd6a49ffbf3038f3646cf13fc01000000000000000000e0182ce7cc10db785b5fb2fb4314053f5b12cd6116168797cb461aa339fc725078b87766255d0317ba5261e2";
const H849139: &str = "00a0b625ffa2f7cbf95219fc74c3db38f84ae265784bc1417c71020000000000000000008e5b319a229376089f4a7b77c90ed90ac19a0532fc4c62426f5a5931ee7e3e8dd2c67766255d03171e91a015";

/// The chain work rskj's tests give blocks 849,134 and 849,137.
const CHAIN_WORK_849134: &str = "7ffef81fa11393037c9df17b";
const CHAIN_WORK_849137: &str = "7fffdc6f043e4a69ea179a7a";

/// `receiveHeader`'s answers (`BridgeSupport.RECEIVE_HEADER_*`).
const SUCCESSFUL: i64 = 0;
const UNEXPECTED_EXCEPTION: i64 = -99;

fn header(hex: &str) -> Header {
    bitcoin::consensus::deserialize(&hex::decode(hex).expect("hex")).expect("a header")
}

/// A Bridge at RSK block 1 whose Bitcoin store holds the block `hex` at
/// `height` with the chain work `work`, its head.
fn holding(cfg: &RskHardforkConfig, hex: &str, height: u32, work: &str) -> Ctx {
    let mut ctx = ctx_at(1);
    let work = U256::from_str_radix(work, 16).expect("a chain work");
    let block = StoredBlock::new(header(hex), height, work);
    let v2 = cfg.has_stored_block_v2(1);
    btc_store::put_stored_block(&mut ctx, &block, v2);
    btc_store::store_chain_head(&mut ctx, &block, v2);
    ctx
}

/// `receiveHeader` of the block `hex`: its answer.
fn receive_header(
    ctx: &mut Ctx,
    hex: &str,
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
) -> i64 {
    let raw = hex::decode(hex).expect("hex");
    let args = encode::args(&[Arg::Bytes(&raw)]);
    let use_v2 = cfg.has_stored_block_v2(1);
    let answer = btc_chain::receive_header(ctx, &args, 0, config, use_v2, cfg).expect("an answer");
    i64::from_be_bytes(answer.bytes[24..32].try_into().expect("eight bytes"))
}

/// `receiveHeaders` of the blocks `hexes`.
fn receive_headers(
    ctx: &mut Ctx,
    hexes: &[&str],
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
) {
    let raws: Vec<Vec<u8>> = hexes
        .iter()
        .map(|hex| hex::decode(hex).expect("hex"))
        .collect();
    let headers: Vec<&[u8]> = raws.iter().map(Vec::as_slice).collect();
    let args = encode::args(&[Arg::BytesArray(&headers)]);
    let use_v2 = cfg.has_stored_block_v2(1);
    btc_chain::receive_headers(ctx, &args, 0, config, use_v2, cfg).expect("an answer");
}

/// Whether the Bridge's Bitcoin store holds the block `hex`.
fn stored(ctx: &mut Ctx, hex: &str) -> bool {
    btc_store::get_stored_block(ctx, &header(hex).block_hash()).is_some()
}

/// The networks other than mainnet rskj's `notMainnetAndActivationsArgs`
/// gives, each before RSKIP434 and from it.
fn off_mainnet() -> Vec<(BridgeConstants, RskHardforkConfig)> {
    let mut cases = Vec::new();
    for (config, chain_id) in [
        (BridgeConstants::testnet(), RSK_TESTNET_CHAIN_ID),
        (BridgeConstants::rskj_regtest(), RSK_REGTEST_CHAIN_ID),
    ] {
        for last in [Arrowhead600, Arrowhead631] {
            cases.push((config.clone(), up_to(last, chain_id)));
        }
    }
    cases
}

/// `receiveHeader_beforeRSKIP434_returnsUnexpectedExceptionResponseCodeAndDoesNotSaveTheBlock`.
#[test]
fn before_rskip434_receive_header_does_not_take_mainnets_block_849138() {
    let (config, cfg) = (
        BridgeConstants::mainnet(),
        up_to(Arrowhead600, RSK_MAINNET_CHAIN_ID),
    );
    let mut ctx = holding(&cfg, H849137, 849_137, CHAIN_WORK_849137);
    assert_eq!(
        receive_header(&mut ctx, H849138, &config, &cfg),
        UNEXPECTED_EXCEPTION
    );
    assert!(!stored(&mut ctx, H849138));
}

/// `receiveHeaders_beforeRSKIP434_savesBlocksUntilReachesBlockWithTooMuchChainWork`.
#[test]
fn before_rskip434_receive_headers_stops_at_mainnets_block_849138() {
    let (config, cfg) = (
        BridgeConstants::mainnet(),
        up_to(Arrowhead600, RSK_MAINNET_CHAIN_ID),
    );
    let mut ctx = holding(&cfg, H849134, 849_134, CHAIN_WORK_849134);
    receive_headers(
        &mut ctx,
        &[H849135, H849136, H849137, H849138, H849139],
        &config,
        &cfg,
    );
    for (hex, taken) in [
        (H849135, true),
        (H849136, true),
        (H849137, true),
        (H849138, false),
        (H849139, false),
    ] {
        assert_eq!(stored(&mut ctx, hex), taken, "{hex}");
    }
}

/// `receiveHeader_afterRSKIP434_returnsSuccessfulAndSavesTheBlock`.
#[test]
fn from_rskip434_receive_header_takes_mainnets_block_849138() {
    let (config, cfg) = (
        BridgeConstants::mainnet(),
        up_to(Arrowhead631, RSK_MAINNET_CHAIN_ID),
    );
    let mut ctx = holding(&cfg, H849137, 849_137, CHAIN_WORK_849137);
    assert_eq!(receive_header(&mut ctx, H849138, &config, &cfg), SUCCESSFUL);
    assert!(stored(&mut ctx, H849138));
}

/// `receiveHeaders_afterRSKIP434_savesAllBlocks`.
#[test]
fn from_rskip434_receive_headers_takes_mainnets_blocks_from_849138() {
    let (config, cfg) = (
        BridgeConstants::mainnet(),
        up_to(Arrowhead631, RSK_MAINNET_CHAIN_ID),
    );
    let mut ctx = holding(&cfg, H849134, 849_134, CHAIN_WORK_849134);
    let hexes = [H849135, H849136, H849137, H849138, H849139];
    receive_headers(&mut ctx, &hexes, &config, &cfg);
    for hex in hexes {
        assert!(stored(&mut ctx, hex), "{hex}");
    }
}

/// `receiveHeader_networkNotMainnet_returnsSuccessfulAndSavesTheBlock`.
#[test]
fn off_mainnet_receive_header_takes_block_849138() {
    for (config, cfg) in off_mainnet() {
        let mut ctx = holding(&cfg, H849137, 849_137, CHAIN_WORK_849137);
        let case = format!("{:?} {}", config.btc_network, cfg.has_rskip434(1));
        assert_eq!(
            receive_header(&mut ctx, H849138, &config, &cfg),
            SUCCESSFUL,
            "{case}"
        );
        assert!(stored(&mut ctx, H849138), "{case}");
    }
}

/// `receiveHeaders_networkNotMainnet_savesAllBlocks`.
#[test]
fn off_mainnet_receive_headers_takes_the_blocks_from_849138() {
    for (config, cfg) in off_mainnet() {
        let mut ctx = holding(&cfg, H849134, 849_134, CHAIN_WORK_849134);
        let hexes = [H849135, H849136, H849137, H849138, H849139];
        receive_headers(&mut ctx, &hexes, &config, &cfg);
        let case = format!("{:?} {}", config.btc_network, cfg.has_rskip434(1));
        for hex in hexes {
            assert!(stored(&mut ctx, hex), "{case} {hex}");
        }
    }
}
