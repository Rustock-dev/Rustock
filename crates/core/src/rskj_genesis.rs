//! A genesis block read from an rskj genesis JSON file (`genesis = <file>` in
//! an rskj config, loaded by rskj's `GenesisLoaderImpl`).
//!
//! The point is to start a regtest chain on exactly the block an rskj node
//! started on, so that the blocks it mined connect here: block 1's parent hash
//! is the genesis hash, and the genesis hash commits to every field below.
//!
//! Two details make that hash rskj's and not merely plausible:
//!
//! - the difficulty is hashed as the file's bytes (`GenesisHeader` keeps them
//!   verbatim), so `0x0010000000` and `0x10000000` are different genesis
//!   blocks with the same difficulty;
//! - the state root is not in the file. rskj builds the genesis state and
//!   writes its root into the header, so the caller does the same:
//!   [`GenesisSpec::with_state_root`].
//!
//! Only plain accounts are read (`balance`, `nonce`). An `alloc` entry with a
//! `contract` is refused rather than silently funded without its code.

use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_rlp::Encodable;
use anyhow::{anyhow, bail, Context, Result};

use crate::config::GenesisAlloc;
use crate::types::header::Header;

const EMPTY_LIST_HASH: B256 = B256::new(hex_literal::hex!(
    "1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347"
));
const EMPTY_TRIE_HASH: B256 = B256::new(hex_literal::hex!(
    "56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"
));

/// A genesis block as an rskj genesis JSON file describes it.
#[derive(Clone, Debug)]
pub struct GenesisSpec {
    pub parent_hash: B256,
    pub coinbase: Address,
    pub timestamp: u64,
    pub extra_data: Bytes,
    pub gas_limit: U256,
    /// The `difficulty` field's exact bytes, leading zeros included.
    pub difficulty_bytes: Bytes,
    pub minimum_gas_price: U256,
    pub bitcoin_merged_mining_header: Bytes,
    pub bitcoin_merged_mining_merkle_proof: Bytes,
    pub bitcoin_merged_mining_coinbase_transaction: Bytes,
    /// The `alloc` accounts, in file order.
    pub alloc: Vec<GenesisAlloc>,
    /// The root of the genesis state, once the caller has built it.
    pub state_root: Option<B256>,
}

/// rskj `org.ethereum.json.Utils.parseData`: optional `0x`, odd length padded.
fn parse_data(s: &str) -> Result<Vec<u8>> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let padded;
    let s = if s.len() % 2 == 1 {
        padded = format!("0{s}");
        padded.as_str()
    } else {
        s
    };
    hex::decode(s).with_context(|| format!("invalid hex '{s}'"))
}

fn u256_from_be(bytes: &[u8]) -> Result<U256> {
    if bytes.len() > 32 {
        bail!("value longer than 32 bytes");
    }
    Ok(U256::from_be_slice(bytes))
}

fn decimal(s: &str) -> Result<U256> {
    U256::from_str_radix(s, 10).with_context(|| format!("invalid decimal '{s}'"))
}

impl GenesisSpec {
    /// Parse an rskj genesis JSON document.
    pub fn from_json(json: &str) -> Result<Self> {
        let v: serde_json::Value = serde_json::from_str(json).context("not valid JSON")?;
        let text = |key: &str| v.get(key).and_then(|x| x.as_str());
        let field = |key: &str| -> Result<Vec<u8>> {
            parse_data(text(key).ok_or_else(|| anyhow!("genesis lacks '{key}'"))?)
                .with_context(|| format!("in '{key}'"))
        };
        let optional = |key: &str| -> Result<Vec<u8>> {
            text(key).map(parse_data).unwrap_or_else(|| Ok(Vec::new()))
        };

        let mut alloc = Vec::new();
        if let Some(entries) = v.get("alloc").and_then(|a| a.as_object()) {
            for (key, entry) in entries {
                // rskj's loader skips this key.
                if key == "00" {
                    continue;
                }
                let address = parse_data(key)?;
                if address.len() != 20 {
                    bail!("alloc address '{key}' is not 20 bytes");
                }
                if entry.get("contract").is_some() {
                    bail!("alloc {key} has a contract; only plain accounts are supported");
                }
                let balance = entry
                    .get("balance")
                    .and_then(|b| b.as_str())
                    .ok_or_else(|| anyhow!("alloc {key} lacks a balance"))?;
                // rskj: the network's initial nonce, zero everywhere, when absent.
                let nonce = match entry.get("nonce").and_then(|n| n.as_str()) {
                    Some(n) => decimal(n)?,
                    None => U256::ZERO,
                };
                alloc.push(GenesisAlloc {
                    address: Address::from_slice(&address),
                    balance: decimal(balance)?,
                    nonce,
                });
            }
        }

        let coinbase = field("coinbase")?;
        // rskj reads the timestamp and the gas limit with
        // `ByteUtil.byteArrayToLong`, which refuses more than 8 bytes, leading
        // zeros or not.
        let long = |key: &str| -> Result<Vec<u8>> {
            let bytes = field(key)?;
            if bytes.len() > 8 {
                bail!("'{key}' is longer than 8 bytes, which rskj refuses");
            }
            Ok(bytes)
        };
        let timestamp = long("timestamp")?;
        // Eight bytes whose top bit is set are a negative `long` to rskj: a
        // timestamp no header here holds.
        if timestamp.len() == 8 && timestamp[0] & 0x80 != 0 {
            bail!("'timestamp' is below zero as rskj reads it");
        }
        let timestamp = u256_from_be(&timestamp)?;
        // `RLP.parseSignedCoinNonNullZero`: a signed number of at least one
        // byte, for `new BigInteger` throws on none, so a file without one
        // is refused. One whose top bit is set is below zero, which no
        // header here holds.
        let minimum_gas_price = field("minimumGasPrice")?;
        match minimum_gas_price.first() {
            None => bail!("'minimumGasPrice' is empty, which rskj refuses"),
            Some(first) if first & 0x80 != 0 => {
                bail!("'minimumGasPrice' is below zero as rskj reads it")
            }
            Some(_) => {}
        }
        Ok(Self {
            parent_hash: B256::try_from(field("parentHash")?.as_slice())
                .context("parentHash is not 32 bytes")?,
            // rskj's `new RskAddress(bytes)` throws on any other length, none
            // included.
            coinbase: Address::try_from(coinbase.as_slice()).context("coinbase is not 20 bytes")?,
            timestamp: u64::try_from(timestamp).context("timestamp out of range")?,
            extra_data: Bytes::from(field("extraData")?),
            gas_limit: u256_from_be(&long("gasLimit")?)?,
            difficulty_bytes: Bytes::from(field("difficulty")?),
            minimum_gas_price: u256_from_be(&minimum_gas_price)?,
            bitcoin_merged_mining_header: Bytes::from(optional("bitcoinMergedMiningHeader")?),
            bitcoin_merged_mining_merkle_proof: Bytes::from(optional(
                "bitcoinMergedMiningMerkleProof",
            )?),
            bitcoin_merged_mining_coinbase_transaction: Bytes::from(optional(
                "bitcoinMergedMiningCoinbaseTransaction",
            )?),
            alloc,
            state_root: None,
        })
    }

    /// Read and parse an rskj genesis JSON file.
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read genesis file {}", path.display()))?;
        Self::from_json(&json).with_context(|| format!("in genesis file {}", path.display()))
    }

    /// The same genesis with its state root filled in.
    pub fn with_state_root(mut self, root: B256) -> Self {
        self.state_root = Some(root);
        self
    }

    fn has_mining_fields(&self) -> bool {
        !self.bitcoin_merged_mining_header.is_empty()
            || !self.bitcoin_merged_mining_merkle_proof.is_empty()
            || !self.bitcoin_merged_mining_coinbase_transaction.is_empty()
    }

    /// The gas limit as rskj's `GenesisHeader` holds it: the file's value
    /// (`ByteUtil.byteArrayToLong`, then `longToBytes`) without leading
    /// zeros, and one zero byte for zero (`ByteUtil.stripLeadingZeroes`).
    fn gas_limit_bytes(&self) -> Vec<u8> {
        crate::types::header::strip_leading_zeroes(&self.gas_limit.to_be_bytes::<32>()).to_vec()
    }

    /// The genesis hash, as rskj's `GenesisHeader.getEncoded` produces it: a
    /// version-0 header with the difficulty bytes as given and RSKIP92's
    /// encoding (only the merged-mining header, no proof or coinbase), which
    /// is what every regtest chain since Orchid uses.
    pub fn hash(&self) -> B256 {
        self.hash_encoded(true)
    }

    /// The genesis hash with RSKIP92's encoding or without it, as rskj's
    /// `GenesisLoaderImpl` is given `useRskip92Encoding`: whether RSKIP92 is
    /// active at block 0 (`RskContext`). Without it a header with mining
    /// fields hashes the merkle proof and the coinbase after the Bitcoin
    /// header (`BlockHeader.getEncodedForHash`).
    pub fn hash_encoded(&self, rskip92: bool) -> B256 {
        let mut list = Vec::new();
        self.parent_hash.encode(&mut list);
        EMPTY_LIST_HASH.encode(&mut list);
        // `new RskAddress(bytes)` holds the file's 20 bytes, and
        // `RLP.encodeRskAddress` writes them: only the null address, of no
        // bytes, is written empty, and the zero address is not it.
        self.coinbase.encode(&mut list);
        self.state_root.unwrap_or_default().encode(&mut list);
        EMPTY_TRIE_HASH.encode(&mut list);
        EMPTY_TRIE_HASH.encode(&mut list);
        alloy_primitives::Bloom::ZERO.encode(&mut list);
        self.difficulty_bytes.encode(&mut list);
        0u64.encode(&mut list); // number
        self.gas_limit_bytes().as_slice().encode(&mut list);
        0u64.encode(&mut list); // gasUsed
        self.timestamp.encode(&mut list);
        self.extra_data.encode(&mut list);
        U256::ZERO.encode(&mut list); // paidFees
                                      // RLP.encodeSignedCoinNonNullZero: zero is the single byte 0x00.
        let mgp = self.minimum_gas_price.to_be_bytes::<32>();
        match mgp.iter().position(|b| *b != 0) {
            None => list.push(0x00),
            Some(i) if mgp[i] & 0x80 != 0 => [&[0u8][..], &mgp[i..]].concat().encode(&mut list),
            Some(i) => mgp[i..].encode(&mut list),
        }
        0u64.encode(&mut list); // uncleCount
        if self.has_mining_fields() {
            self.bitcoin_merged_mining_header.encode(&mut list);
            if !rskip92 {
                self.bitcoin_merged_mining_merkle_proof.encode(&mut list);
                self.bitcoin_merged_mining_coinbase_transaction
                    .encode(&mut list);
            }
        }
        let mut out = Vec::with_capacity(list.len() + 4);
        alloy_rlp::Header {
            list: true,
            payload_length: list.len(),
        }
        .encode(&mut out);
        out.extend_from_slice(&list);
        alloy_primitives::keccak256(&out)
    }

    /// The genesis header, carrying [`GenesisSpec::hash`], and keeping the
    /// difficulty's bytes as the file writes them and the gas limit's as
    /// rskj holds them where the typed fields would write them otherwise, so
    /// that the header written to the store reads back with that hash.
    pub fn header(&self) -> Header {
        self.header_encoded(true)
    }

    /// The genesis header carrying [`GenesisSpec::hash_encoded`]'s hash.
    pub fn header_encoded(&self, rskip92: bool) -> Header {
        let some = |b: &Bytes| (!b.is_empty()).then(|| b.clone());
        let java = crate::types::header::java_signed_bytes;
        let difficulty = U256::from_be_slice(&self.difficulty_bytes);
        let gas_limit = self.gas_limit_bytes();
        let kept = crate::types::header::KeptFields {
            difficulty: (self.difficulty_bytes[..] != java(&difficulty)[..])
                .then(|| self.difficulty_bytes.clone()),
            gas_limit: (gas_limit != java(&self.gas_limit)).then(|| Bytes::from(gas_limit)),
            ..Default::default()
        };
        Header {
            parent_hash: self.parent_hash,
            ommers_hash: EMPTY_LIST_HASH,
            beneficiary: self.coinbase,
            state_root: self.state_root.unwrap_or_default(),
            transactions_root: EMPTY_TRIE_HASH,
            receipts_root: EMPTY_TRIE_HASH,
            logs_bloom: Default::default(),
            extension_data: None,
            difficulty,
            number: 0,
            gas_limit: self.gas_limit,
            gas_used: 0,
            timestamp: self.timestamp,
            extra_data: self.extra_data.clone(),
            paid_fees: U256::ZERO,
            minimum_gas_price: self.minimum_gas_price,
            uncle_count: 0,
            umm_root: None,
            version_fields: None,
            bitcoin_merged_mining_header: some(&self.bitcoin_merged_mining_header),
            bitcoin_merged_mining_merkle_proof: some(&self.bitcoin_merged_mining_merkle_proof),
            bitcoin_merged_mining_coinbase_transaction: some(
                &self.bitcoin_merged_mining_coinbase_transaction,
            ),
            cached_hash: Some(self.hash_encoded(rskip92)),
            cached_hash_for_merged_mining: None,
            kept: (kept != Default::default()).then(|| Box::new(kept)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENESIS: &str = r#"{
      "alloc": {
        "0000000000000000000000000000000001000006": { "balance": "21000000000000000000000000" },
        "cd2a3d9f938e13cd947ec05abc7fe734df8dd826": { "balance": "1000", "nonce": "7" },
        "00": { "balance": "1" }
      },
      "nonce": "0xdeadbeefdeadbeef",
      "mixhash": "0x00",
      "bitcoinMergedMiningHeader": "0x00",
      "bitcoinMergedMiningMerkleProof": "0x00",
      "bitcoinMergedMiningCoinbaseTransaction": "0x00",
      "timestamp": "0x00",
      "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
      "extraData": "0x686f727365",
      "gasLimit": "0x017D7840",
      "difficulty": "0x0010000000",
      "coinbase": "0x3333333333333333333333333333333333333333",
      "minimumGasPrice": "0x00"
    }"#;

    #[test]
    fn reads_an_rskj_genesis_file() {
        let g = GenesisSpec::from_json(GENESIS).unwrap();
        assert_eq!(g.alloc.len(), 2, "the 00 key is skipped, as rskj does");
        let funded = g.alloc.iter().find(|a| a.nonce == U256::from(7)).unwrap();
        assert_eq!(funded.balance, U256::from(1000));
        assert_eq!(g.gas_limit, U256::from(25_000_000));
        assert_eq!(g.difficulty_bytes.as_ref(), &[0x00, 0x10, 0x00, 0x00, 0x00]);
        assert_eq!(g.header().difficulty, U256::from(0x1000_0000u64));
        assert_eq!(g.extra_data.as_ref(), b"horse");
        assert!(g.state_root.is_none());
    }

    #[test]
    fn the_hash_keeps_the_difficulty_bytes_as_written() {
        let g = GenesisSpec::from_json(GENESIS)
            .unwrap()
            .with_state_root(B256::repeat_byte(1));
        let mut canonical = g.clone();
        canonical.difficulty_bytes = Bytes::from(vec![0x10, 0x00, 0x00, 0x00]);
        assert_ne!(g.hash(), canonical.hash(), "same value, different bytes");
        assert_eq!(
            g.header().hash(),
            g.hash(),
            "the header carries the cached hash"
        );
    }

    #[test]
    fn a_contract_in_the_alloc_is_refused() {
        let json = GENESIS.replace(
            r#"{ "balance": "1000", "nonce": "7" }"#,
            r#"{ "balance": "0", "contract": { "code": "0x00" } }"#,
        );
        assert!(GenesisSpec::from_json(&json).is_err());
    }

    /// rskj's `rsk-dev.json` has a gas limit whose top bit is set and a
    /// difficulty written with leading zeros; here with the zero address as
    /// coinbase too, which rskj writes as its 20 bytes, and, in a second file,
    /// a gas limit of zero, which a `GenesisHeader` holds as one zero byte.
    /// The genesis header's fields hash as the file does, and the header the
    /// store writes reads back with that hash.
    #[test]
    fn the_genesis_header_hashes_as_the_file_and_reads_back_so() {
        use crate::types::header::HeaderForm;
        let heights = crate::config::ActivationHeights::regtest();
        let dev = GENESIS
            .replace("0x017D7840", "0x989680")
            .replace("0x0010000000", "0x0000000001")
            .replace(
                "0x3333333333333333333333333333333333333333",
                "0x0000000000000000000000000000000000000000",
            );
        let no_gas = dev.replace("0x989680", "0x00");
        for json in [dev, no_gas] {
            let g = GenesisSpec::from_json(&json)
                .unwrap()
                .with_state_root(B256::repeat_byte(1));
            let header = g.header();
            let built = Header {
                cached_hash: None,
                ..header.clone()
            }
            .with_activations(&heights);
            assert_eq!(
                built.hash(),
                g.hash(),
                "the header's fields hash as the file's"
            );
            let read = Header::decode_in(
                &mut header.encode_stored().as_slice(),
                &heights,
                HeaderForm::Stored,
            )
            .unwrap();
            assert_eq!(
                read.hash(),
                g.hash(),
                "and so does what the store reads back"
            );
        }
    }

    /// rskj's `new RskAddress(bytes)` throws on a coinbase of any length but
    /// 20, none included.
    #[test]
    fn a_coinbase_that_is_not_20_bytes_is_refused() {
        for coinbase in ["0x", "0x33"] {
            let json = GENESIS.replace("0x3333333333333333333333333333333333333333", coinbase);
            assert!(GenesisSpec::from_json(&json).is_err(), "{coinbase}");
        }
    }

    /// rskj reads the timestamp and the gas limit with
    /// `ByteUtil.byteArrayToLong`, which refuses more than 8 bytes even where
    /// the first are zeros, and reads a timestamp of 8 bytes whose top bit is
    /// set as below zero. A gas limit of 8 bytes is the block the same value
    /// written short is.
    #[test]
    fn a_timestamp_or_gas_limit_over_8_bytes_is_refused() {
        let timestamp = r#""timestamp": "0x00""#;
        let gas_limit = r#""gasLimit": "0x017D7840""#;
        for (from, to) in [
            (timestamp, r#""timestamp": "0x000000000000000000""#),
            (timestamp, r#""timestamp": "0x8000000000000000""#),
            (gas_limit, r#""gasLimit": "0x0000000000017D7840""#),
        ] {
            let json = GENESIS.replace(from, to);
            assert!(GenesisSpec::from_json(&json).is_err(), "{to}");
        }
        let short = GenesisSpec::from_json(GENESIS).unwrap();
        let eight = GENESIS.replace(gas_limit, r#""gasLimit": "0x00000000017D7840""#);
        let eight = GenesisSpec::from_json(&eight).unwrap();
        assert_eq!(eight.gas_limit, U256::from(25_000_000));
        assert_eq!(eight.hash(), short.hash());
        let latest = GENESIS.replace(timestamp, r#""timestamp": "0x7fffffffffffffff""#);
        let latest = GenesisSpec::from_json(&latest).unwrap();
        assert_eq!(latest.timestamp, i64::MAX as u64);
    }

    /// rskj's `RLP.parseSignedCoinNonNullZero` reads the minimum gas price
    /// as a signed number with `new BigInteger`, which throws on no bytes: a
    /// file without one, or with an empty one, is refused, and so is one
    /// below zero, which a leading zero byte keeps a price from being.
    #[test]
    fn a_minimum_gas_price_is_required_and_not_below_zero() {
        let missing = GENESIS.replace("\"minimumGasPrice\"", "\"unread\"");
        assert!(GenesisSpec::from_json(&missing).is_err(), "missing");
        let price = r#""minimumGasPrice": "0x00""#;
        for written in ["0x", "0x80", "0xff00"] {
            let json = GENESIS.replace(price, &format!(r#""minimumGasPrice": "{written}""#));
            assert!(GenesisSpec::from_json(&json).is_err(), "{written}");
        }
        let json = GENESIS.replace(price, r#""minimumGasPrice": "0x0080""#);
        let g = GenesisSpec::from_json(&json).unwrap();
        assert_eq!(g.minimum_gas_price, U256::from(128));
    }

    /// Where orchid isn't at block 0, rskj loads the file without RSKIP92's
    /// encoding (`useRskip92Encoding` is RSKIP92 at block 0), so block 0's
    /// hash covers the merkle proof and the coinbase after the Bitcoin
    /// header. The chain's configuration hashes it so, the header's fields
    /// hash so under those heights, and the store reads it back so. A file
    /// without mining fields hashes alike either way.
    #[test]
    fn where_orchid_is_not_at_genesis_its_hash_covers_the_proof_and_coinbase() {
        use crate::types::header::HeaderForm;
        let g = GenesisSpec::from_json(GENESIS)
            .unwrap()
            .with_state_root(B256::repeat_byte(1));
        let without = g.hash_encoded(false);
        assert_ne!(without, g.hash(), "the file has mining fields");

        let mut config = crate::config::ChainConfig::regtest();
        config.genesis_spec = Some(g.clone());
        assert_eq!(config.known_genesis_hash(), Some(g.hash()));
        config.activation_heights.orchid = 5;
        assert_eq!(config.known_genesis_hash(), Some(without));

        let heights = config.activation_heights.clone();
        let header = config.genesis_header();
        assert_eq!(header.hash(), without);
        let built = Header {
            cached_hash: None,
            ..header.clone()
        }
        .with_activations(&heights);
        assert_eq!(
            built.hash(),
            without,
            "the header's fields hash as the file's"
        );
        let read = Header::decode_in(
            &mut header.encode_stored().as_slice(),
            &heights,
            HeaderForm::Stored,
        )
        .unwrap();
        assert_eq!(
            read.hash(),
            without,
            "and so does what the store reads back"
        );

        let bare = GENESIS.replace("\"bitcoinMergedMining", "\"unread");
        let bare = GenesisSpec::from_json(&bare).unwrap();
        assert_eq!(bare.hash_encoded(false), bare.hash());
    }
}
