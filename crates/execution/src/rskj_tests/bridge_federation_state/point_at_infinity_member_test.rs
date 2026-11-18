//! A federation member whose key is the one byte `00`, the point at
//! infinity.
//!
//! Not a port: no test of rskj's gives a member such a key. Each test holds
//! rustock to what rskj's code, read in the release, does with one. Bouncy
//! Castle's `decodePoint`, under `BtcECKey.fromPublicOnly` and
//! `ECKey.fromPublicOnly`, takes `00` as the point at infinity; the member
//! keeps it, `getPubKey()` gives `00` back, and `00` sorts before every other
//! key (`BtcECKey.PUBKEY_COMPARATOR`). So the vote takes the key, the pending
//! and the committed federation store it, and their hash, redeem script,
//! address, `commit_federation` event and getters carry it. Its RSK address
//! (`ECKey.getAddress`) is the last 20 bytes of the Keccak-256 hash of
//! nothing.
//!
//! On rskj's regtest three of the five federation-change authorizers
//! ('auth-a' to 'auth-e') vote such a key in, every upgrade active from block
//! 0; on mainnet and testnet two of their three.

use alloy_primitives::Address;

use crate::abi::encode::{self, Arg};
use crate::bridge::constants::BridgeConstants;
use crate::bridge::{getters, governance, peg, storage};
use crate::hardfork::RskHardforkConfig;
use crate::precompiles::BridgeTxContext;

use super::support::*;

/// The point at infinity's RSK address.
const INFINITY_ADDRESS: Address =
    alloy_primitives::address!("dcc703c0e500b653ca82273b7bfad8045d85a470");

/// The point at infinity's encoding.
const INFINITY: &[u8] = &[0x00];

type Answer = Result<revm::precompile::PrecompileOutput, revm::precompile::PrecompileError>;

/// Each deciding authorizer's vote through `vote`: their answers, `None`
/// where a vote throws.
fn votes(
    ctx: &mut Ctx,
    config: &BridgeConstants,
    mut vote: impl FnMut(&mut Ctx, &BridgeTxContext) -> Answer,
) -> Vec<Option<i128>> {
    deciding_authorizers(config)
        .into_iter()
        .map(|sender| {
            let tx = BridgeTxContext {
                rsk_sender: sender,
                ..BridgeTxContext::default()
            };
            vote(ctx, &tx).ok().map(|out| int_answer(&out.bytes))
        })
        .collect()
}

/// The deciding authorizers' votes to create a pending federation.
fn create(ctx: &mut Ctx, config: &BridgeConstants, cfg: &RskHardforkConfig) -> Vec<Option<i128>> {
    votes(ctx, config, |ctx, tx| {
        governance::create_federation(ctx, 0, config, cfg, tx)
    })
}

/// The deciding authorizers' votes to add a member of `keys`: its one key
/// (`addFederatorPublicKey`), or its BTC, RSK and MST keys
/// (`addFederatorPublicKeyMultikey`).
fn add(
    ctx: &mut Ctx,
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
    keys: &[&[u8]],
) -> Vec<Option<i128>> {
    let args: Vec<Arg> = keys.iter().map(|key| Arg::Bytes(key)).collect();
    let args = encode::args(&args);
    votes(ctx, config, |ctx, tx| {
        if keys.len() == 1 {
            governance::add_federator_public_key(ctx, &args, 0, config, cfg, tx)
        } else {
            governance::add_federator_public_key_multikey(ctx, &args, 0, config, cfg, tx)
        }
    })
}

/// The deciding authorizers' votes to commit the pending federation of hash
/// `hash`.
fn commit(
    ctx: &mut Ctx,
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
    hash: &[u8],
) -> Vec<Option<i128>> {
    let args = encode::args(&[Arg::Bytes(hash)]);
    votes(ctx, config, |ctx, tx| {
        governance::commit_federation(ctx, &args, 0, config, cfg, tx)
    })
}

/// Keccak-256.
fn keccak(data: &[u8]) -> Vec<u8> {
    use sha3::Digest as _;
    sha3::Keccak256::digest(data).to_vec()
}

/// What a `bytes` or `string` answer holds.
fn bytes_answer(answer: Answer) -> Vec<u8> {
    let out = answer.expect("the getter answers").bytes;
    let offset = alloy_primitives::U256::from_be_slice(&out[..32]).to::<usize>();
    let length = alloy_primitives::U256::from_be_slice(&out[offset..offset + 32]).to::<usize>();
    out[offset + 32..offset + 32 + length].to_vec()
}

/// A `string` answer.
fn string_answer(answer: Answer) -> String {
    String::from_utf8(bytes_answer(answer)).expect("UTF-8")
}

/// The arguments of a getter of a member's key: the index, and a key type
/// where one is named.
fn at(index: i64, key_type: Option<&str>) -> Vec<u8> {
    match key_type {
        None => encode::args(&[Arg::Word(encode::int(index))]),
        Some(key_type) => encode::args(&[
            Arg::Word(encode::int(index)),
            Arg::Bytes(key_type.as_bytes()),
        ]),
    }
}

/// `serializeFederationMember` of a member of one key for all three, `key`
/// as rskj encodes it.
fn member(key: &[u8]) -> Vec<u8> {
    rlp_list(&[rlp_element(key), rlp_element(key), rlp_element(key)])
}

/// `serializeFederation`: a federation of members of one key each, given
/// sorted and as rskj encodes them, each member an element of the list.
fn federation(keys: &[&[u8]], time_millis: u64, block: u64) -> Vec<u8> {
    let members: Vec<Vec<u8>> = keys.iter().map(|key| rlp_element(&member(key))).collect();
    rlp_list(&[
        rlp_number(time_millis),
        rlp_number(block),
        rlp_list(&members),
    ])
}

/// `ScriptBuilder.createRedeemScript`: `size / 2 + 1` of `keys`, given
/// sorted and as rskj encodes them, each pushed by `ScriptBuilder.data`, the
/// point at infinity's one byte as a one-byte push (`01 00`).
fn multisig_of(keys: &[&[u8]]) -> Vec<u8> {
    let mut script = vec![0x50 + (keys.len() / 2 + 1) as u8];
    for key in keys {
        script.push(key.len() as u8);
        script.extend_from_slice(key);
    }
    script.push(0x50 + keys.len() as u8);
    script.push(0xae);
    script
}

/// `P2shErpRedeemScriptBuilder`, which P2SH-P2WSH ERP federations' scripts
/// are built by too: the members' multisig, and after the network's delay
/// its emergency keys'.
fn p2sh_erp_of(keys: &[&[u8]], config: &BridgeConstants) -> Vec<u8> {
    let csv = signed_le(config.erp_fed_activation_delay);
    let mut script = vec![0x64];
    script.extend(multisig_of(keys));
    script.push(0x67);
    script.push(csv.len() as u8);
    script.extend(csv);
    script.extend_from_slice(&[0xb2, 0x75]);
    script.extend(standard_redeem(&keys_of(config.erp_fed_pubkeys)));
    script.push(0x68);
    script
}

/// rskj's regtest, every upgrade active: three authorizers vote a member of
/// the point at infinity into a pending federation, and two of 33-byte keys
/// beside it, and a key of that point again, of any of the three types, is a
/// member's already (-2). The pending federation, its getters and its hash
/// keep `00`, sorted first, and so do the proposed federation the commit
/// makes (RSKIP419), its P2SH-P2WSH ERP redeem script behind its address
/// (RSKIP305), the `commit_federation` event and the proposed federation's
/// getters; and a member's RSK key of that point is at its address.
#[test]
fn from_rskip123_the_vote_takes_the_point_at_infinity_and_the_proposed_federation_keeps_it() {
    const BLOCK: u64 = 100;
    let config = BridgeConstants::rskj_regtest();
    let cfg = RskHardforkConfig::rskj_regtest();
    assert!(cfg.has_rskip123(BLOCK) && cfg.has_rskip419(BLOCK) && cfg.has_rskip305(BLOCK));
    let decided = vec![Some(1); 3];
    assert_eq!(deciding_authorizers(&config).len(), 3);
    let mut ctx = ctx_at(BLOCK);
    let others = members(2);
    let (a, b): (&[u8], &[u8]) = (&others[0], &others[1]);

    assert_eq!(create(&mut ctx, &config, &cfg), decided, "createFederation");
    assert_eq!(add(&mut ctx, &config, &cfg, &[INFINITY; 3]), decided);
    assert_eq!(
        cell(&mut ctx, storage::PENDING_FEDERATION_KEY),
        rlp_list(&[member(INFINITY)])
    );
    for keys in [[INFINITY, a, a], [a, INFINITY, a], [a, a, INFINITY]] {
        assert_eq!(
            add(&mut ctx, &config, &cfg, &keys),
            vec![Some(-2); 3],
            "{keys:02x?}"
        );
    }
    assert_eq!(add(&mut ctx, &config, &cfg, &[b; 3]), decided);
    assert_eq!(add(&mut ctx, &config, &cfg, &[a; 3]), decided);
    // serializePendingFederation: its members sorted, each not an element.
    assert_eq!(
        cell(&mut ctx, storage::PENDING_FEDERATION_KEY),
        rlp_list(&[member(INFINITY), member(a), member(b)])
    );
    let pending_key = |ctx: &mut Ctx, key_type: Option<&str>| match key_type {
        None => bytes_answer(getters::get_pending_federator_public_key(
            ctx,
            &at(0, None),
            0,
        )),
        Some(_) => bytes_answer(getters::get_pending_federator_public_key_of_type(
            ctx,
            &at(0, key_type),
            0,
        )),
    };
    assert_eq!(pending_key(&mut ctx, None), INFINITY);
    for key_type in ["btc", "rsk", "mst"] {
        assert_eq!(
            pending_key(&mut ctx, Some(key_type)),
            INFINITY,
            "{key_type}"
        );
    }
    // PendingFederation.getHash: its BTC keys alone, sorted (serializeOnlyBtcKeys).
    let hash = keccak(&rlp_list(&[
        rlp_element(INFINITY),
        rlp_element(a),
        rlp_element(b),
    ]));
    assert_eq!(pending_hash(&mut ctx), hash);

    let old_address = string_answer(getters::get_federation_address(&mut ctx, 0, &config, &cfg));
    let _ = logs(&mut ctx);
    assert_eq!(
        commit(&mut ctx, &config, &cfg, &hash),
        decided,
        "commitFederation"
    );
    assert_eq!(
        cell(&mut ctx, PROPOSED.0),
        federation(&[INFINITY, a, b], TIMESTAMP * 1000, BLOCK)
    );
    assert_eq!(cell(&mut ctx, PROPOSED.1), rlp_number(4000));
    let new_address = governance::p2sh_base58_address(
        &p2sh_p2wsh_hash(&p2sh_erp_of(&[INFINITY, a, b], &config)),
        &config,
    );
    let activation = BLOCK + governance::federation_activation_age(&config, &cfg, BLOCK);
    let event = (
        vec![alloy_primitives::keccak256(
            "commit_federation(bytes,string,bytes,string,int256)",
        )],
        encode::args(&[
            Arg::Bytes(&keys_of(config.genesis_federation_public_keys).concat()),
            Arg::Bytes(old_address.as_bytes()),
            Arg::Bytes(&[INFINITY, a, b].concat()),
            Arg::Bytes(new_address.as_bytes()),
            Arg::Word(encode::int(activation as i64)),
        ]),
    );
    assert!(logs(&mut ctx).contains(&event), "commit_federation");

    let proposed_key =
        getters::get_proposed_federator_public_key_of_type(&mut ctx, &at(0, Some("btc")), 0);
    assert_eq!(bytes_answer(proposed_key), INFINITY);
    assert_eq!(
        string_answer(getters::get_proposed_federation_address(
            &mut ctx, 0, &config, &cfg
        )),
        new_address
    );
    let size = getters::get_proposed_federation_size(&mut ctx, 0).expect("an answer");
    assert_eq!(int_answer(&size.bytes), 3);
    assert!(peg::is_sender_proposed_fed_member(
        &mut ctx,
        &cfg,
        BLOCK,
        INFINITY_ADDRESS
    ));
}

/// Mainnet before RSKIP123: two of the three authorizers vote the point at
/// infinity in with the vote of one key (`addFederatorPublicKey`), and again
/// it is a member's already. The pending federation (its BTC keys alone),
/// the new federation the commit hands over to (before RSKIP419), and the
/// legacy `commit_federation` event (before RSKIP146) keep `00`, sorted
/// first, and so does the standard multisig whose hash the event names.
#[test]
fn before_rskip123_the_vote_takes_the_point_at_infinity_and_the_new_federation_keeps_it() {
    const BLOCK: u64 = 1_000_000;
    let config = BridgeConstants::mainnet();
    let cfg = RskHardforkConfig::mainnet();
    assert!(!cfg.has_rskip123(BLOCK) && !cfg.has_rskip146(BLOCK) && !cfg.has_rskip201(BLOCK));
    let decided = vec![Some(1); 2];
    assert_eq!(deciding_authorizers(&config).len(), 2);
    let mut ctx = ctx_at(BLOCK);
    let others = members(2);
    let (a, b): (&[u8], &[u8]) = (&others[0], &others[1]);

    assert_eq!(create(&mut ctx, &config, &cfg), decided, "createFederation");
    for key in [b, INFINITY, a] {
        assert_eq!(add(&mut ctx, &config, &cfg, &[key]), decided, "{key:02x?}");
    }
    assert_eq!(add(&mut ctx, &config, &cfg, &[INFINITY]), vec![Some(-2); 2]);
    let btc_keys = rlp_list(&[rlp_element(INFINITY), rlp_element(a), rlp_element(b)]);
    assert_eq!(cell(&mut ctx, storage::PENDING_FEDERATION_KEY), btc_keys);
    assert_eq!(
        bytes_answer(getters::get_pending_federator_public_key(
            &mut ctx,
            &at(0, None),
            0
        )),
        INFINITY
    );
    let hash = keccak(&btc_keys);
    assert_eq!(pending_hash(&mut ctx), hash);

    let _ = logs(&mut ctx);
    assert_eq!(
        commit(&mut ctx, &config, &cfg, &hash),
        decided,
        "commitFederation"
    );
    // serializeFederationOnlyBtcKeys, created at the block with its timestamp
    // read as milliseconds.
    assert_eq!(
        cell(&mut ctx, NEW.0),
        rlp_list(&[rlp_number(TIMESTAMP), rlp_number(BLOCK), btc_keys])
    );
    let genesis = keys_of(config.genesis_federation_public_keys);
    let genesis: Vec<&[u8]> = genesis.iter().map(|key| &key[..]).collect();
    // BrigeEventLoggerLegacyImpl.logCommitFederation: a federation's address's
    // hash, then its BTC keys, each an element.
    let federation_data = |keys: &[&[u8]]| {
        let elements: Vec<Vec<u8>> = keys.iter().map(|key| rlp_element(key)).collect();
        rlp_list(&[
            rlp_element(&hash160(&multisig_of(keys))),
            rlp_list(&elements),
        ])
    };
    let activation = BLOCK + governance::federation_activation_age(&config, &cfg, BLOCK);
    let mut topic = [0u8; 32];
    topic[32 - b"commit_federation_topic".len()..].copy_from_slice(b"commit_federation_topic");
    let event = (
        vec![topic.into()],
        rlp_list(&[
            federation_data(&genesis),
            federation_data(&[INFINITY, a, b]),
            rlp_element(activation.to_string().as_bytes()),
        ]),
    );
    assert!(logs(&mut ctx).contains(&event), "commit_federation_topic");
}

/// A committed federation keeps a member of the point at infinity: the
/// getters of the active federation and of the retiring one answer `00` for
/// its keys, each one's address is that of its multisig with `00` in it, and
/// that key's RSK address is a federator's, whose transactions the Bridge's
/// federators-only methods take (`isFromFederateMember`), though no one can
/// sign one.
#[test]
fn the_active_and_the_retiring_federation_answer_00_for_the_point_at_infinity() {
    const BLOCK: u64 = 2_000_000;
    let config = BridgeConstants::mainnet();
    let cfg = RskHardforkConfig::mainnet();
    assert!(cfg.has_rskip123(BLOCK) && !cfg.has_rskip201(BLOCK));
    let mut ctx = ctx_at(BLOCK);
    let keys = members(4);
    let (a, b, c, d): (&[u8], &[u8], &[u8], &[u8]) = (&keys[0], &keys[1], &keys[2], &keys[3]);
    put(&mut ctx, NEW.0, &federation(&[INFINITY, a, b], 0, 1));
    put(&mut ctx, NEW.1, &rlp_number(1000));
    put(&mut ctx, OLD.0, &federation(&[INFINITY, c, d], 0, 0));
    put(&mut ctx, OLD.1, &rlp_number(1000));

    let key = |ctx: &mut Ctx, key_type: Option<&str>, retiring: bool| {
        let args = at(0, key_type);
        bytes_answer(match (key_type, retiring) {
            (None, false) => getters::get_federator_public_key(ctx, &args, 0, &config, &cfg),
            (Some(_), false) => {
                getters::get_federator_public_key_of_type(ctx, &args, 0, &config, &cfg)
            }
            (None, true) => {
                getters::get_retiring_federator_public_key(ctx, &args, 0, &config, &cfg)
            }
            (Some(_), true) => {
                getters::get_retiring_federator_public_key_of_type(ctx, &args, 0, &config, &cfg)
            }
        })
    };
    for retiring in [false, true] {
        assert_eq!(
            key(&mut ctx, None, retiring),
            INFINITY,
            "retiring: {retiring}"
        );
        for key_type in ["btc", "rsk", "mst"] {
            let answer = key(&mut ctx, Some(key_type), retiring);
            assert_eq!(answer, INFINITY, "{key_type}, retiring: {retiring}");
        }
    }
    let size = getters::get_federation_size(&mut ctx, 0, &config, &cfg).expect("an answer");
    assert_eq!(int_answer(&size.bytes), 3);
    assert_eq!(
        string_answer(getters::get_federation_address(&mut ctx, 0, &config, &cfg)),
        mainnet_p2sh_address(&p2sh_hash(&multisig_of(&[INFINITY, a, b])))
    );
    assert_eq!(
        string_answer(getters::get_retiring_federation_address(
            &mut ctx, 0, &config, &cfg
        )),
        mainnet_p2sh_address(&p2sh_hash(&multisig_of(&[INFINITY, c, d])))
    );
    assert!(peg::is_sender_active_or_retiring_fed_member(
        &mut ctx,
        &config,
        &cfg,
        BLOCK,
        INFINITY_ADDRESS
    ));
}

/// A non-standard ERP federation, RSKIP201's before RSKIP353, keeps a member
/// of the point at infinity too: `NonStandardErpRedeemScriptBuilder` takes
/// the members' multisig, `00` pushed first, and after the network's delay
/// the emergency keys', each without its `OP_CHECKMULTISIG`, and ends with
/// one. The federation's address is that script's.
#[test]
fn a_non_standard_erp_federations_address_is_that_of_its_script_with_00_first() {
    const BLOCK: u64 = 4_700_000;
    let config = BridgeConstants::mainnet();
    let cfg = RskHardforkConfig::mainnet();
    assert_eq!(governance::federation_format_version(&cfg, BLOCK), 2000);
    assert!(cfg.has_rskip293(BLOCK) && !cfg.has_rskip353(BLOCK));
    let mut ctx = ctx_at(BLOCK);
    let keys = members(2);
    let (a, b): (&[u8], &[u8]) = (&keys[0], &keys[1]);
    put(&mut ctx, NEW.0, &federation(&[INFINITY, a, b], 0, 1));
    put(&mut ctx, NEW.1, &rlp_number(2000));

    let without_checkmultisig = |script: Vec<u8>| script[..script.len() - 1].to_vec();
    let csv = signed_le(config.erp_fed_activation_delay);
    let mut script = vec![0x64];
    script.extend(without_checkmultisig(multisig_of(&[INFINITY, a, b])));
    script.push(0x67);
    script.push(csv.len() as u8);
    script.extend(csv);
    script.extend_from_slice(&[0xb2, 0x75]);
    script.extend(without_checkmultisig(standard_redeem(&keys_of(
        config.erp_fed_pubkeys,
    ))));
    script.extend_from_slice(&[0x68, 0xae]);
    assert_eq!(script[1..4], [0x52, 0x01, 0x00]);
    assert_eq!(
        string_answer(getters::get_federation_address(&mut ctx, 0, &config, &cfg)),
        mainnet_p2sh_address(&p2sh_hash(&script))
    );
}
