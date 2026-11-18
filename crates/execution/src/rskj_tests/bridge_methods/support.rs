//! What the ported tests of the Bridge's methods share: a Bridge to call at a
//! block of an upgrade ladder, the calls rskj's tests make to it, and how its
//! answers read.
//!
//! rskj's `BridgeBuilder` builds a Bridge over its genesis block, with an
//! `ActivationConfig` that has every RSKIP of the upgrades up to the one a test
//! names active from block 0 (`ActivationConfigsForTest`), a transaction and
//! the kind of call that reaches the Bridge (`MessageCall.MsgType`). Here the
//! ladder is rustock's own, scheduled the same way ([`up_to`]), and the Bridge
//! is called through `execute_bridge`, as the executor and the precompile
//! frame call it.

use alloy_primitives::{Address, U256};
use revm::context_interface::{ContextTr, JournalTr};
use revm::precompile::{PrecompileError, PrecompileOutput};

pub(super) use crate::abi::encode::{self, Arg};
pub(super) use crate::bridge::constants::BridgeConstants;
pub(super) use crate::bridge::BridgeCallKind;
pub(super) use crate::hardfork::{
    RskHardforkConfig, RskNetworkUpgrade, RSK_MAINNET_CHAIN_ID, RSK_REGTEST_CHAIN_ID,
    RSK_TESTNET_CHAIN_ID,
};
use crate::precompiles::BridgeTxContext;

/// The world a ported test calls the Bridge in: no trie and no database, only
/// what the test and the Bridge write.
pub(super) type Ctx = revm::Context<
    revm::context::BlockEnv,
    revm::context::TxEnv,
    revm::context::CfgEnv,
    revm::database::EmptyDB,
    revm::Journal<revm::database::EmptyDB>,
    crate::raw_storage::RskChainExt,
>;

/// The upgrades after genesis, in order.
const LADDER: [RskNetworkUpgrade; 13] = [
    RskNetworkUpgrade::Orchid,
    RskNetworkUpgrade::Wasabi100,
    RskNetworkUpgrade::TwoToThree,
    RskNetworkUpgrade::Papyrus200,
    RskNetworkUpgrade::Iris300,
    RskNetworkUpgrade::Hop400,
    RskNetworkUpgrade::Fingerroot500,
    RskNetworkUpgrade::Arrowhead600,
    RskNetworkUpgrade::Arrowhead631,
    RskNetworkUpgrade::Lovell700,
    RskNetworkUpgrade::Reed800,
    RskNetworkUpgrade::Reed810,
    RskNetworkUpgrade::Vetiver900,
];

/// rskj's `ActivationConfigsForTest.<upgrade>()`: every upgrade up to `last`
/// active from block 0 and none after it, on the network `chain_id` names.
/// `Genesis` is rskj's `genesis()`, with nothing active, and `Vetiver900`
/// its `all()`, which has reed810's rules too.
pub(super) fn up_to(last: RskNetworkUpgrade, chain_id: u64) -> RskHardforkConfig {
    LADDER
        .into_iter()
        .fold(RskHardforkConfig::all_active(chain_id), |cfg, upgrade| {
            cfg.with_hardfork(upgrade, (upgrade <= last).then_some(0))
        })
}

/// [`up_to`] on mainnet, whose constants rskj's `BridgeBuilder` takes unless
/// a test gives others.
pub(super) fn mainnet_up_to(last: RskNetworkUpgrade) -> RskHardforkConfig {
    up_to(last, RSK_MAINNET_CHAIN_ID)
}

/// A Bridge to call, as rskj's `BridgeBuilder` builds one: an execution block,
/// the transaction that reaches the Bridge and the kind of call it is.
pub(super) struct Bridge {
    pub(super) ctx: Ctx,
    pub(super) cfg: RskHardforkConfig,
    pub(super) constants: BridgeConstants,
    /// Who calls: the transaction's sender, or the contract that calls.
    pub(super) caller: Address,
    /// rskj's `MessageCall.MsgType`.
    pub(super) kind: BridgeCallKind,
    /// rskj's `Transaction.isLocalCallTransaction`: an `eth_call`.
    pub(super) local_call: bool,
    /// 1 for a transaction's own call, 2 for a contract's
    /// (`BridgeUtils.isContractTx`).
    pub(super) depth: usize,
    /// What the call carries.
    pub(super) value: U256,
}

/// Nobody in particular: no federator, no authorizer.
pub(super) const STRANGER: Address = Address::repeat_byte(0x99);

impl Bridge {
    /// The Bridge at block `number` of `cfg`, under `constants`, called with a
    /// plain CALL by a transaction of [`STRANGER`]'s.
    pub(super) fn new(cfg: RskHardforkConfig, constants: BridgeConstants, number: u64) -> Self {
        use revm::MainContext;
        let mut ctx: Ctx =
            revm::Context::mainnet().with_chain(crate::raw_storage::RskChainExt::default());
        ctx.block.number = U256::from(number);
        ctx.block.timestamp = U256::from(1_700_000_000u64);
        Self {
            ctx,
            cfg,
            constants,
            caller: STRANGER,
            kind: BridgeCallKind::Call,
            local_call: false,
            depth: 1,
            value: U256::ZERO,
        }
    }

    /// At block 0 of rskj's `ActivationConfigsForTest.<last>()` on mainnet.
    pub(super) fn mainnet_up_to(last: RskNetworkUpgrade) -> Self {
        Self::new(mainnet_up_to(last), BridgeConstants::mainnet(), 0)
    }

    /// The number of the block the Bridge runs in.
    pub(super) fn number(&self) -> u64 {
        revm::context_interface::Block::number(self.ctx.block()).to::<u64>()
    }

    /// rskj's `bridge.execute(data)`, with all the gas it asks for. The call's
    /// writes are kept when it returns and dropped when it fails, as the
    /// executor and the precompile frame keep and drop them. What a test wrote
    /// before the call is the state the call starts from, and stays.
    pub(super) fn execute(&mut self, data: &[u8]) -> Result<PrecompileOutput, PrecompileError> {
        self.ctx.chain_mut().raw_storage.commit_call();
        let number = self.number();
        let tx = BridgeTxContext {
            rsk_sender: self.caller,
            local_call: self.local_call,
            ..Default::default()
        };
        let checkpoints: Vec<_> = (0..self.depth)
            .map(|_| self.ctx.journal_mut().checkpoint())
            .collect();
        let result = crate::bridge::execute_bridge(
            &mut self.ctx,
            data,
            u64::MAX,
            &self.constants,
            None,
            self.cfg.has_stored_block_v2(number),
            &self.cfg,
            &tx,
            self.caller,
            self.depth,
            self.kind,
            self.value,
        );
        if result.is_ok() {
            self.ctx.chain_mut().raw_storage.commit_call();
            for _ in &checkpoints {
                self.ctx.journal_mut().checkpoint_commit();
            }
        } else {
            self.ctx.chain_mut().raw_storage.discard_call();
            for checkpoint in checkpoints.into_iter().rev() {
                self.ctx.journal_mut().checkpoint_revert(checkpoint);
            }
        }
        result
    }

    /// [`Bridge::execute`] of a call that must return: what it answered.
    pub(super) fn answer(&mut self, data: &[u8]) -> Vec<u8> {
        match self.execute(data) {
            Ok(output) => output.bytes.to_vec(),
            Err(e) => panic!("the call failed: {e:?}"),
        }
    }

    /// What `address` holds.
    pub(super) fn balance(&mut self, address: Address) -> U256 {
        use revm::context_interface::journaled_state::account::JournaledAccountTr;
        *self
            .ctx
            .journal_mut()
            .load_account_mut(address)
            .unwrap()
            .balance()
    }

    /// Gives `address` `amount` more, as a call's value reaches its callee
    /// before the callee runs.
    pub(super) fn credit(&mut self, address: Address, amount: U256) {
        use revm::context_interface::journaled_state::account::JournaledAccountTr;
        let held = self.balance(address);
        self.ctx
            .journal_mut()
            .load_account_mut(address)
            .unwrap()
            .set_balance(held + amount);
    }
}

/// `Function.encode(args)`: the selector of `signature`, then its arguments.
pub(super) fn call_data(signature: &str, args: &[Arg]) -> Vec<u8> {
    let mut data = selector_only(signature);
    data.extend(encode::args(args));
    data
}

/// `Function.encodeSignature()`: the selector alone, no arguments. The
/// signature must be one of the Bridge's, or a misspelt one would read as a
/// method refused.
pub(super) fn selector_only(signature: &str) -> Vec<u8> {
    let selector = crate::abi::selector(signature);
    assert!(
        crate::bridge::find_bridge_method(&selector).is_some(),
        "{signature} is no method of the Bridge's"
    );
    selector.to_vec()
}

/// An integer argument.
pub(super) fn int(n: i64) -> Arg<'static> {
    Arg::Word(encode::int(n))
}

/// An `address` argument.
pub(super) fn address(a: Address) -> Arg<'static> {
    let mut word = [0u8; 32];
    word[12..].copy_from_slice(a.as_slice());
    Arg::Word(word)
}

/// A `bool` argument.
pub(super) fn boolean(b: bool) -> Arg<'static> {
    Arg::Word(encode::int(i64::from(b)))
}

/// Whether the call threw as rskj's `Bridge.execute` throws a `VMException`:
/// the failure a transaction's own call or a contract's call is given, its
/// cost spent and nothing it wrote kept.
pub(super) fn threw(result: &Result<PrecompileOutput, PrecompileError>) -> bool {
    matches!(
        result,
        Err(e) if crate::bridge::invisible_exception_gas(e).is_some()
            || crate::bridge::internal_throw_gas(e).is_some()
    )
}

/// An `int256` answer, as a Java `long`.
pub(super) fn int_of(answer: &[u8]) -> i64 {
    assert_eq!(answer.len(), 32, "an int256 answer is one word");
    i64::from_be_bytes(answer[24..32].try_into().unwrap())
}

/// A `uint256` answer.
pub(super) fn uint_of(answer: &[u8]) -> U256 {
    assert_eq!(answer.len(), 32, "a uint256 answer is one word");
    U256::from_be_slice(answer)
}

/// A `bool` answer.
pub(super) fn bool_of(answer: &[u8]) -> bool {
    uint_of(answer) != U256::ZERO
}

/// An `address` answer.
pub(super) fn address_of(answer: &[u8]) -> Address {
    assert_eq!(answer.len(), 32, "an address answer is one word");
    Address::from_slice(&answer[12..])
}

/// A `bytes` answer: its offset, its length, then its data.
pub(super) fn bytes_of(answer: &[u8]) -> Vec<u8> {
    let offset = uint_of(&answer[..32]).to::<usize>();
    let len = uint_of(&answer[offset..offset + 32]).to::<usize>();
    answer[offset + 32..offset + 32 + len].to_vec()
}

/// A `string` answer.
pub(super) fn string_of(answer: &[u8]) -> String {
    String::from_utf8(bytes_of(answer)).expect("a string answer is UTF-8")
}

/// A `string[]` answer: where the array is, how many, where each string is
/// from the first head, then each string.
pub(super) fn strings_of(answer: &[u8]) -> Vec<String> {
    let offset = uint_of(&answer[..32]).to::<usize>();
    let count = uint_of(&answer[offset..offset + 32]).to::<usize>();
    let heads = offset + 32;
    (0..count)
        .map(|i| {
            let head = heads + 32 * i;
            let at = heads + uint_of(&answer[head..head + 32]).to::<usize>();
            let len = uint_of(&answer[at..at + 32]).to::<usize>();
            String::from_utf8(answer[at + 32..at + 32 + len].to_vec()).unwrap()
        })
        .collect()
}

/// The RSK address of a public key, given in hex.
pub(super) fn address_of_key(hex: &str) -> Address {
    let key = alloy_primitives::hex::decode(hex).unwrap();
    crate::bridge::federation::rsk_address_from_public_key(&key).unwrap()
}

/// The RSK address of a member of `constants`' genesis federation, which is
/// the active one while no other is stored.
pub(super) fn genesis_federator(constants: &BridgeConstants) -> Address {
    let keys = crate::bridge::peg::genesis_federation_keys(constants);
    crate::bridge::federation::rsk_address_from_public_key(&keys[0]).unwrap()
}

/// The compressed public key of a private key.
pub(super) fn public_key_of(private: &[u8; 32]) -> [u8; 33] {
    use k256::elliptic_curve::sec1::ToEncodedPoint;
    let key = k256::SecretKey::from_slice(private).unwrap();
    key.public_key()
        .to_encoded_point(true)
        .as_bytes()
        .try_into()
        .unwrap()
}

/// rskj's `BitcoinTestUtils.getBtcEcKeyFromSeed`: the key whose private part
/// is the seed's Keccak-256 hash.
pub(super) fn key_from_seed(seed: &str) -> [u8; 33] {
    use sha3::Digest;
    public_key_of(&sha3::Keccak256::digest(seed.as_bytes()).into())
}

/// rskj's `BtcECKey.fromPrivate(Hex.decode(hex))`: the key of a private
/// number written in a few hex digits.
pub(super) fn key_of_private(hex: &str) -> [u8; 33] {
    let digits = alloy_primitives::hex::decode(hex).unwrap();
    let mut private = [0u8; 32];
    private[32 - digits.len()..].copy_from_slice(&digits);
    public_key_of(&private)
}

/// RIPEMD-160 of SHA-256, Bitcoin's hash of a key or a script.
pub(super) fn hash160(data: &[u8]) -> [u8; 20] {
    use ripemd::Digest as _;
    ripemd::Ripemd160::digest(sha2::Sha256::digest(data)).into()
}
