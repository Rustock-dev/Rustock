//! `--import` against rskj's own tests (`co.rsk.db.importer`), Rootstock's real
//! signatures, and copies built here in both of rskj's formats.

use super::*;
use alloy_primitives::{Address, Bytes};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::SigningKey;
use rustock_storage::epoch_store::EpochConfig;
use rustock_trie::TrieKeySlice;
use std::path::PathBuf;

// ------------------------------------------------------------------ signers

#[test]
fn mainnet_and_testnet_use_rskjs_url_and_keys() {
    let main = Signers::for_network(30, None, &[]).unwrap();
    assert_eq!(main.url, MAINNET_URL);
    assert_eq!(main.keys, MAINNET_KEYS.map(String::from).to_vec());
    assert_eq!(main.threshold(), 2);
    let test = Signers::for_network(31, None, &[]).unwrap();
    assert_eq!(test.url, TESTNET_URL);
    assert_eq!(test.keys, TESTNET_KEYS.map(String::from).to_vec());
}

#[test]
fn another_network_needs_a_url_and_keys_of_its_own() {
    let keys: Vec<String> = MAINNET_KEYS[..2].iter().map(|k| k.to_uppercase()).collect();
    assert!(Signers::for_network(33, None, &keys).is_err());
    let signers = Signers::for_network(33, Some("https://copies.example/regtest"), &keys).unwrap();
    assert_eq!(signers.url, "https://copies.example/regtest/");
    assert_eq!(
        signers.keys,
        MAINNET_KEYS[..2]
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn keys_that_cannot_sign_or_cannot_make_two_are_refused() {
    let one = vec![MAINNET_KEYS[0].to_string()];
    assert!(Signers::for_network(30, None, &one).is_err());
    let twice = vec![MAINNET_KEYS[0].to_string(), MAINNET_KEYS[0].to_uppercase()];
    assert!(Signers::for_network(30, None, &twice).is_err());
    let off_the_curve = vec![
        MAINNET_KEYS[0].to_string(),
        format!("04{}", "11".repeat(64)),
    ];
    assert!(Signers::for_network(30, None, &off_the_curve).is_err());
    let not_hex = vec![MAINNET_KEYS[0].to_string(), "zz".to_string()];
    assert!(Signers::for_network(30, None, &not_hex).is_err());
    assert!(Signers::for_network(30, Some("http://import.example/"), &[]).is_err());
}

#[test]
fn more_than_half_have_to_agree_and_never_fewer_than_two() {
    for (n, want) in [(2, 2), (3, 2), (4, 3), (5, 3), (6, 4), (7, 4)] {
        let keys = (0..n).map(|i| format!("k{i}")).collect();
        assert_eq!(
            Signers {
                url: String::new(),
                keys
            }
            .threshold(),
            want,
            "{n} keys"
        );
    }
}

// ---------------------------------------------- BootstrapIndexCandidateSelectorTest

fn entry(height: u64, hash: &str) -> Entry {
    Entry {
        height,
        db: "dbPath".into(),
        hash: hash.into(),
        sig: EntrySignature {
            r: "r".into(),
            s: "s".into(),
        },
    }
}

fn keys(n: usize) -> Vec<String> {
    (1..=n).map(|i| format!("key{i}")).collect()
}

#[test]
fn no_index_is_an_error() {
    // getMaximumCommonHeightDataEmpty
    assert!(select(&[], &keys(2), 2).is_err());
}

#[test]
fn one_signer_alone_is_not_enough() {
    // getMaximumCommonHeightDataOneEntry
    let indexes = [Index {
        dbs: vec![entry(1, "hash")],
    }];
    assert!(select(&indexes, &keys(2), 2).is_err());
}

#[test]
fn a_height_one_signer_lists_twice_is_an_error() {
    // getMaximumCommonHeightDataDuplicatedEntries
    let indexes = [Index {
        dbs: vec![entry(1, "hash"), entry(1, "hash")],
    }];
    let error = select(&indexes, &keys(2), 2).unwrap_err().to_string();
    assert!(error.contains("2 entries from same height 1"), "{error}");
}

#[test]
fn two_signers_agreeing_choose_their_height() {
    // getMaximumCommonHeightDataTwoEntries
    let indexes = [
        Index {
            dbs: vec![entry(1, "hash")],
        },
        Index {
            dbs: vec![entry(1, "hash")],
        },
    ];
    assert_eq!(select(&indexes, &keys(2), 2).unwrap().height, 1);
}

#[test]
fn the_highest_height_enough_signers_agree_on_is_chosen() {
    // getMaximumCommonHeightDataThreeEntries
    let indexes = [
        Index {
            dbs: vec![entry(1, "hash"), entry(2, "hash")],
        },
        Index {
            dbs: vec![entry(1, "hash")],
        },
    ];
    assert_eq!(select(&indexes, &keys(2), 2).unwrap().height, 1);
    // getMaximumCommonHeightDataManyEntries
    let both = Index {
        dbs: vec![entry(1, "hash"), entry(2, "hash")],
    };
    let indexes = [both.clone(), both.clone(), both];
    assert_eq!(select(&indexes, &keys(3), 2).unwrap().height, 2);
}

#[test]
fn signers_naming_different_copies_do_not_count_together() {
    let indexes = [
        Index {
            dbs: vec![entry(1, "x"), entry(2, "a")],
        },
        Index {
            dbs: vec![entry(1, "x"), entry(2, "b")],
        },
        Index {
            dbs: vec![entry(1, "y"), entry(2, "a")],
        },
    ];
    let candidate = select(&indexes, &keys(3), 2).unwrap();
    assert_eq!(candidate.height, 2);
    assert_eq!(
        candidate
            .entries
            .iter()
            .map(|(i, _)| *i)
            .collect::<Vec<_>>(),
        [0, 2]
    );
    let indexes = [
        Index {
            dbs: vec![entry(1, "x"), entry(2, "a")],
        },
        Index {
            dbs: vec![entry(1, "x"), entry(2, "b")],
        },
        Index {
            dbs: vec![entry(1, "y"), entry(2, "c")],
        },
    ];
    let candidate = select(&indexes, &keys(3), 2).unwrap();
    assert_eq!(candidate.height, 1, "no two agree at 2");
    assert_eq!(
        candidate
            .entries
            .iter()
            .map(|(i, _)| *i)
            .collect::<Vec<_>>(),
        [0, 1]
    );
}

// ------------------------------------------------------ BootstrapDataVerifierTest

fn signed_entry(hash: &str, r: &str, s: &str) -> Entry {
    Entry {
        height: 1,
        db: "dbPath".into(),
        hash: hash.into(),
        sig: EntrySignature {
            r: r.into(),
            s: s.into(),
        },
    }
}

fn candidate(hash: &str, signatures: &[(&str, &str)]) -> Candidate {
    Candidate {
        height: 1,
        entries: signatures
            .iter()
            .enumerate()
            .map(|(i, (r, s))| (i, signed_entry(hash, r, s)))
            .collect(),
    }
}

#[test]
fn nothing_to_verify_verifies_nothing() {
    // verifyFileEmpty
    let none = Candidate {
        height: 1,
        entries: Vec::new(),
    };
    assert_eq!(verify(&none, &[]).unwrap(), 0);
}

#[test]
fn rskjs_vectors_verify_two_of_three_both_with_a_high_s() {
    // verifyFileMany
    let keys = [
        "04330037e82c177d7108077c80440821e13c1c62105f85e030214b48d7b5dff0b8e7c158b171546a71139e4de56c8535c964514033b89a669a8e87a5e8770c147c",
        "0473602083afe175e7cae12dbc27da54ec5ac77f99920787f3e891e7af303aaed480770c0de4c991aea1712729260175e158fa73f63c60f0f1de057139c52714de",
        "04bf74915a14e96df0b520a659acc28ae21aada3b0415e35f82f0c7b546338fc48bbfdce858ce3e4690feeb443d7f4955881ba0b793999e4fef7e46732e7fedf02",
    ]
    .map(String::from);
    let entries = candidate(
        "53cb8e93030183c5ba198433e8cd1f013f3d113e0f4d1756de0d1f124ead155a",
        &[
            (
                "8dba957877d5bdcb26d551dfa2fa509dfe3fe327caf0166130b9f467a0a0c249",
                "dab3fdf2031515d2de1d420310c69153fcc356f22b50dfd53c6e13e74e346eee",
            ),
            (
                "f0e8aab4fdd83382292a1bbc5480e2ae8084dc245f000f4bc4534d383a3a7919",
                "a30891f2176bd87b4a3ac5c75167f2442453c17c6e2fbfb36c3b972ee67a4c2d",
            ),
            ("00", "00"),
        ],
    );
    assert_eq!(verify(&entries, &keys).unwrap(), 2);
}

#[test]
fn entries_naming_different_hashes_are_an_error() {
    // doNotVerifyForDifferentHashes
    let keys = [
        "04330037e82c177d7108077c80440821e13c1c62105f85e030214b48d7b5dff0b8e7c158b171546a71139e4de56c8535c964514033b89a669a8e87a5e8770c147c",
        "04dffaa346b18c26230d6050c6ab67f0761ab12136b0dc3a1f1c6ed31890ffac38baa3d8da1df4aa2acc72e45dc3599797ba668666c5395280217f2fd215262b8a",
        "04bf74915a14e96df0b520a659acc28ae21aada3b0415e35f82f0c7b546338fc48bbfdce858ce3e4690feeb443d7f4955881ba0b793999e4fef7e46732e7fedf02",
    ]
    .map(String::from);
    let mixed = Candidate {
        height: 1,
        entries: vec![
            (
                0,
                signed_entry(
                    "53cb8e93030183c5ba198433e8cd1f013f3d113e0f4d1756de0d1f124ead155a",
                    "8dba957877d5bdcb26d551dfa2fa509dfe3fe327caf0166130b9f467a0a0c249",
                    "dab3fdf2031515d2de1d420310c69153fcc356f22b50dfd53c6e13e74e346eee",
                ),
            ),
            (
                1,
                signed_entry(
                    "87d149cb424c0387656f211d2589fb5b1e16229921309e98588419ccca8a7362",
                    "0efa8e0738468e434e443676b5a91fe15b0c416ff7190d8d912dff65161ad33c",
                    "aae27edef80e137dc1a0e92b6ae7fed0d1dc1b4a0ea8930bad6abec5ca1297c1",
                ),
            ),
            (
                2,
                signed_entry(
                    "53cb8e93030183c5ba198433e8cd1f013f3d113e0f4d1756de0d1f124ead155c",
                    "00",
                    "00",
                ),
            ),
        ],
    };
    assert!(verify(&mixed, &keys).is_err());
}

/// The three signatures of Rootstock's mainnet copy at #9,405,000, from the
/// signers' indexes on 14 November 2026.
const MAINNET_9405000: (&str, [(&str, &str); 3]) = (
    "b5a4e411ea42f30346b63813592e7a21fa47fd8d84c706c3e03f22c7ccfe74a9",
    [
        (
            "bd6f96ba76b4a5f581dd93465fe7a4853be5525ab18ab02f16c476b2e2f88147",
            "0bf83fe46aadd800d7a542bde14fc9287554c854ec05beeedf2e73b94d6900ba",
        ),
        (
            "abf0a61cdf98ba617850fd6abcb7db3cb2a77d38b77cbd11f725a50c42466c56",
            "78fa0eae6bcb99a70b32f3188d323754fd18297dd526a70be0ca89c0da2364e2",
        ),
        (
            "7d81c5826834a8aa1fdd96d698b294c922390e23b5d477ec6d5a9ff2d43e6a39",
            "16f6507e11df9a2457c177d3d571fa1c96207c73e627de1bfb434dd7042509ff",
        ),
    ],
);

/// The three of testnet's at #8,280,000. The second and third carry an `s`
/// in the upper half of the range, which k256 alone would refuse.
const TESTNET_8280000: (&str, [(&str, &str); 3]) = (
    "c2c93308f8b15af0d03ac82c06c122c02b82ecd8403635143bb3ebc671f380d1",
    [
        (
            "18ee6a41542b8ff9114dbe072891fc1f5cad0f57d9643406fb521d1a64a17edd",
            "30575649f926e4d8779a584d59006bce53529d08e91d9535b894fe420e8ffbd1",
        ),
        (
            "685ee59686690ceaec185f6a3f5d4a5847bba7434ab61197463bd22990651783",
            "ad1858dcaa1f1a25476ac8d24cdd8089d8fa145c3fe60e312d4790ed8c2012f9",
        ),
        (
            "256b93d27095a10f9c703522de321c6c209128760e0fae4ed43985911d2124bc",
            "90c180d08d39168509682edfa4edec93d03a5b275181a9440d53f1715c893091",
        ),
    ],
);

#[test]
fn rootstocks_signers_signed_their_real_copies() {
    let mainnet = MAINNET_KEYS.map(String::from);
    let (hash, signatures) = MAINNET_9405000;
    assert_eq!(verify(&candidate(hash, &signatures), &mainnet).unwrap(), 3);
    let testnet = TESTNET_KEYS.map(String::from);
    let (hash, signatures) = TESTNET_8280000;
    assert_eq!(verify(&candidate(hash, &signatures), &testnet).unwrap(), 3);
}

#[test]
fn a_signature_counts_only_by_its_own_key_over_its_own_hash() {
    let (hash, signatures) = MAINNET_9405000;
    let mut rotated = MAINNET_KEYS.map(String::from);
    rotated.rotate_left(1);
    assert_eq!(verify(&candidate(hash, &signatures), &rotated).unwrap(), 0);
    let other = "a5a4e411ea42f30346b63813592e7a21fa47fd8d84c706c3e03f22c7ccfe74a9";
    let mainnet = MAINNET_KEYS.map(String::from);
    assert_eq!(verify(&candidate(other, &signatures), &mainnet).unwrap(), 0);
    // `new BigInteger(1, bytes)` reads leading zeros as nothing.
    let padded: Vec<(String, String)> = signatures
        .iter()
        .map(|(r, s)| (format!("0000{r}"), format!("00{s}")))
        .collect();
    let padded: Vec<(&str, &str)> = padded
        .iter()
        .map(|(r, s)| (r.as_str(), s.as_str()))
        .collect();
    assert_eq!(verify(&candidate(hash, &padded), &mainnet).unwrap(), 3);
    // Hex that is not hex fails the import, as BouncyCastle's `Hex.decode` does.
    assert!(verify(&candidate(hash, &[("0g", "00"), ("00", "00")]), &mainnet).is_err());
}

// ------------------------------------------------------------------- copies

/// A chain of empty blocks from `from`, each naming the one before.
fn chain(from: u64, count: u64, state_root: B256) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut parent = B256::repeat_byte(0xEE);
    for number in from..from + count {
        let header = Header {
            parent_hash: parent,
            ommers_hash: rustock_execution::processor::compute_ommers_hash(&[]),
            beneficiary: Address::repeat_byte(3),
            state_root,
            transactions_root: rustock_core::ordered_tx_trie_root(&[], true),
            receipts_root: B256::repeat_byte(6),
            logs_bloom: Default::default(),
            extension_data: None,
            difficulty: U256::from(100),
            number,
            gas_limit: U256::from(6_800_000),
            gas_used: 0,
            timestamp: 1_600_000_000 + number,
            extra_data: Bytes::new(),
            paid_fees: U256::ZERO,
            minimum_gas_price: U256::from(1),
            uncle_count: 0,
            umm_root: None,
            version_fields: None,
            bitcoin_merged_mining_header: None,
            bitcoin_merged_mining_merkle_proof: None,
            bitcoin_merged_mining_coinbase_transaction: None,
            cached_hash: None,
            cached_hash_for_merged_mining: None,
            kept: None,
        };
        parent = header.hash();
        blocks.push(Block {
            header,
            transactions: vec![],
            ommers: vec![],
        });
    }
    blocks
}

fn td_of(number: u64) -> U256 {
    U256::from(1_000_000 + 100 * number)
}

const CODE_KEY: &[u8] = b"a contract's code, which is long";

fn long_value() -> Vec<u8> {
    (0..=199u8).collect()
}

/// A state trie as rskj exports one: its nodes, and its long values apart.
struct State {
    root: B256,
    nodes: Vec<Vec<u8>>,
    values: Vec<Vec<u8>>,
}

fn state() -> State {
    let mem = MemoryTrieStore::new();
    let mut root = TrieNode::empty();
    for i in 0u8..40 {
        root = root.put(&TrieKeySlice::from_key(&[i, i ^ 0x5a, 7]), &[i; 12], &mem);
    }
    root = root.put(&TrieKeySlice::from_key(CODE_KEY), &long_value(), &mem);
    root.save(&mem, true);
    let hash = root.compute_hash(&mem);
    let value_key = keccak256(long_value());
    let mut nodes: Vec<Vec<u8>> = mem
        .keys()
        .into_iter()
        .filter(|k| k[..] != value_key[..])
        .map(|k| mem.get(&k).unwrap())
        .collect();
    nodes.sort();
    State {
        root: hash,
        nodes,
        values: vec![long_value()],
    }
}

fn element(bytes: &[u8]) -> Vec<u8> {
    alloy_rlp::encode(bytes)
}

fn list(items: &[Vec<u8>]) -> Vec<u8> {
    let payload = items.concat();
    let mut out = Vec::new();
    alloy_rlp::Header {
        list: true,
        payload_length: payload.len(),
    }
    .encode(&mut out);
    out.extend_from_slice(&payload);
    out
}

/// `BigInteger.toByteArray()` of a number that is not negative.
fn java_bytes(n: U256) -> Vec<u8> {
    let mut bytes = n.to_be_bytes_trimmed_vec();
    if bytes.first().is_none_or(|b| b & 0x80 != 0) {
        bytes.insert(0, 0);
    }
    bytes
}

/// What rskj's exporter writes for a block: `LIST[ELEMENT(block), ELEMENT(td)]`.
fn block_element(block: &Block, td: U256) -> Vec<u8> {
    list(&[element(&alloy_rlp::encode(block)), element(&java_bytes(td))])
}

/// What a copy holds, element by element, before it is framed.
struct Copy {
    blocks: Vec<Vec<u8>>,
    values: Vec<Vec<u8>>,
    nodes: Vec<Vec<u8>>,
}

impl Copy {
    /// `count` blocks up to `top`, and the state above at the top.
    fn new(top: u64, count: u64) -> (Self, State, Vec<Block>) {
        let state = state();
        let blocks = chain(top + 1 - count, count, state.root);
        let copy = Self {
            blocks: blocks
                .iter()
                .map(|b| block_element(b, td_of(b.header.number)))
                .collect(),
            values: state.values.iter().map(|v| element(v)).collect(),
            nodes: state.nodes.iter().map(|n| element(n)).collect(),
        };
        (copy, state, blocks)
    }

    /// rskj's v2, in its exporter's order, a few elements to a chunk.
    fn v2(&self) -> Vec<u8> {
        v2(&[
            (TAG_BLOCKS, chunks(&self.blocks, 3)),
            (TAG_VALUES, chunks(&self.values, 3)),
            (TAG_NODES, chunks(&self.nodes, 7)),
        ])
    }

    /// rskj's v1: `LIST[ELEMENT(blocks), LIST[nodes, values]]`.
    fn v1(&self) -> Vec<u8> {
        list(&[
            element(&list(&self.blocks)),
            list(&[list(&self.nodes), list(&self.values)]),
        ])
    }
}

fn chunks(elements: &[Vec<u8>], per_chunk: usize) -> Vec<Vec<u8>> {
    elements.chunks(per_chunk).map(|c| c.concat()).collect()
}

fn v2(sections: &[(u8, Vec<Vec<u8>>)]) -> Vec<u8> {
    let mut out = V2_MAGIC.to_vec();
    out.push(V2_VERSION);
    for (tag, chunks) in sections {
        out.push(*tag);
        for chunk in chunks {
            out.extend_from_slice(&(chunk.len() as u64).to_be_bytes());
            out.extend_from_slice(chunk);
        }
        out.extend_from_slice(&0u64.to_be_bytes());
    }
    out.push(TAG_END);
    out
}

/// A zip of `(name, deflated, contents)` entries, with zip64's records when
/// `zip64`.
fn zip(entries: &[(&str, bool, &[u8])], zip64: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, deflated, contents) in entries {
        let mut crc = flate2::Crc::new();
        crc.update(contents);
        let data = if *deflated {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(contents).unwrap();
            encoder.finish().unwrap()
        } else {
            contents.to_vec()
        };
        let method: u16 = if *deflated { 8 } else { 0 };
        let offset = out.len() as u32;
        out.extend_from_slice(&LOCAL_SIG.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&crc.sum().to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(contents.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&data);

        let wide = |v: u32| if zip64 { 0xFFFF_FFFF } else { v };
        central.extend_from_slice(&CENTRAL_SIG.to_le_bytes());
        central.extend_from_slice(&[20, 0, 20, 0]);
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&[0; 4]);
        central.extend_from_slice(&crc.sum().to_le_bytes());
        central.extend_from_slice(&wide(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&wide(contents.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&(if zip64 { 28u16 } else { 0 }).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&[0; 8]);
        central.extend_from_slice(&wide(offset).to_le_bytes());
        central.extend_from_slice(name.as_bytes());
        if zip64 {
            central.extend_from_slice(&1u16.to_le_bytes());
            central.extend_from_slice(&24u16.to_le_bytes());
            central.extend_from_slice(&(contents.len() as u64).to_le_bytes());
            central.extend_from_slice(&(data.len() as u64).to_le_bytes());
            central.extend_from_slice(&u64::from(offset).to_le_bytes());
        }
    }
    let cd_offset = out.len() as u64;
    let cd_size = central.len() as u64;
    let count = entries.len() as u64;
    out.extend_from_slice(&central);
    if zip64 {
        let record_at = out.len() as u64;
        out.extend_from_slice(&ZIP64_EOCD_SIG.to_le_bytes());
        out.extend_from_slice(&44u64.to_le_bytes());
        out.extend_from_slice(&[45, 0, 45, 0]);
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&ZIP64_LOCATOR_SIG.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&record_at.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
    }
    let (count, cd_size, cd_offset) = if zip64 {
        (0xFFFF, 0xFFFF_FFFF, 0xFFFF_FFFF)
    } else {
        (count as u16, cd_size as u32, cd_offset as u32)
    };
    out.extend_from_slice(&EOCD_SIG.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// A store loaded from a zip, with the directory that holds them.
struct Loaded {
    dir: tempfile::TempDir,
    store: BlockStore,
    result: Result<Summary>,
}

fn write_zip(dir: &Path, zip_bytes: &[u8]) -> PathBuf {
    let path = dir.join("copy.zip");
    std::fs::write(&path, zip_bytes).unwrap();
    path
}

fn load_zip(zip_bytes: &[u8], height: u64) -> Loaded {
    let dir = tempfile::tempdir().unwrap();
    let store = BlockStore::open(dir.path()).unwrap();
    let path = write_zip(dir.path(), zip_bytes);
    let trie = TrieTarget::Column(store.db().clone());
    let result = load(
        &path,
        height,
        dir.path(),
        &store,
        &trie,
        &ChainConfig::regtest(),
    );
    Loaded { dir, store, result }
}

fn load_bin(bin: &[u8], height: u64) -> Loaded {
    load_zip(&zip(&[(BOOTSTRAP_ENTRY, true, bin)], false), height)
}

fn refused(bin: &[u8], height: u64) -> String {
    match load_bin(bin, height).result {
        Ok(summary) => panic!("loaded {summary:?}"),
        Err(e) => format!("{e:#}"),
    }
}

/// What a node finds after a load: its position at the copy's top, the
/// blocks indexed, the floor at the lowest, the state readable through the
/// trie code the node executes with, and its own coherence checks content.
fn assert_loaded(store: &BlockStore, blocks: &[Block], state: &State, summary: &Summary) {
    let tip = &blocks.last().unwrap().header;
    assert_eq!(summary.highest, tip.number);
    assert_eq!(summary.lowest, blocks[0].header.number);
    assert_eq!(summary.state_root, state.root);
    assert_eq!(summary.nodes, state.nodes.len() as u64);
    assert_eq!(summary.values, 1);
    assert_eq!(store.head().unwrap(), Some(tip.hash()));
    assert_eq!(store.exec_head().unwrap(), Some((tip.hash(), state.root)));
    for block in blocks {
        let hash = block.header.hash();
        assert_eq!(
            store.canonical_hash(block.header.number).unwrap(),
            Some(hash)
        );
        assert_eq!(
            store.total_difficulty(hash).unwrap(),
            Some(td_of(block.header.number))
        );
        assert!(store.body(hash).unwrap().is_some());
    }
    let chain = ChainConfig::regtest();
    let genesis = chain
        .known_genesis_hash()
        .unwrap_or_else(|| chain.genesis_header().hash());
    assert_eq!(store.canonical_hash(0).unwrap(), Some(genesis));
    assert_eq!(
        store.prune_floor().unwrap().map(|f| f.number),
        Some(blocks[0].header.number)
    );

    let trie = RocksDbTrieStore::from_db(store.db().clone());
    let root = TrieNode::from_message(&trie.get(state.root.as_slice()).unwrap(), &trie);
    assert_eq!(root.compute_hash(&trie), state.root);
    assert_eq!(
        root.get(&TrieKeySlice::from_key(CODE_KEY), &trie),
        Some(long_value())
    );
    assert_eq!(
        root.get(&TrieKeySlice::from_key(&[5, 5 ^ 0x5a, 7]), &trie),
        Some(vec![5; 12])
    );
    rustock_sync::invariant::check(store, Some(&trie), rustock_sync::Scope::Full).unwrap();
}

#[test]
fn a_v2_copy_loads_as_rskj_loads_it() {
    let (copy, state, blocks) = Copy::new(1_009, 6);
    let loaded = load_bin(&copy.v2(), 1_009);
    let summary = loaded.result.unwrap();
    assert_loaded(&loaded.store, &blocks, &state, &summary);
    assert!(!loaded.dir.path().join(UNFINISHED).exists());
}

#[test]
fn a_v1_copy_loads_the_same() {
    let (copy, state, blocks) = Copy::new(1_009, 6);
    let loaded = load_bin(&copy.v1(), 1_009);
    assert_loaded(&loaded.store, &blocks, &state, &loaded.result.unwrap());
}

#[test]
fn a_stored_entry_and_zip64_records_read_the_same() {
    let (copy, state, blocks) = Copy::new(1_009, 6);
    let bin = copy.v2();
    for (deflated, zip64) in [(false, false), (true, true), (false, true)] {
        let with_others = zip(
            &[
                ("README", true, b"hello"),
                (BOOTSTRAP_ENTRY, deflated, &bin),
            ],
            zip64,
        );
        let loaded = load_zip(&with_others, 1_009);
        assert_loaded(&loaded.store, &blocks, &state, &loaded.result.unwrap());
    }
}

#[test]
fn a_section_this_reader_does_not_know_is_skipped() {
    let (copy, state, blocks) = Copy::new(1_009, 6);
    let bin = v2(&[
        (0x04, vec![b"a manifest".to_vec()]),
        (TAG_BLOCKS, chunks(&copy.blocks, 2)),
        (TAG_VALUES, chunks(&copy.values, 1)),
        (TAG_NODES, chunks(&copy.nodes, 100)),
    ]);
    let loaded = load_bin(&bin, 1_009);
    assert_loaded(&loaded.store, &blocks, &state, &loaded.result.unwrap());
}

#[test]
fn the_epoch_backend_takes_the_state_as_one_seed_epoch() {
    let (copy, state, _) = Copy::new(1_009, 6);
    let dir = tempfile::tempdir().unwrap();
    let store = BlockStore::open(dir.path()).unwrap();
    let epochs = dir.path().join("trie-epochs");
    let factory = || TrieEngine::RocksDb.factory(10_000);
    let trie = TrieTarget::Epochs(Box::new(
        EpochTrieStore::open_with(&epochs, EpochConfig::default(), None, factory()).unwrap(),
    ));
    let path = write_zip(
        dir.path(),
        &zip(&[(BOOTSTRAP_ENTRY, true, &copy.v2())], false),
    );
    load(
        &path,
        1_009,
        dir.path(),
        &store,
        &trie,
        &ChainConfig::regtest(),
    )
    .unwrap();
    drop(trie);
    // The node opens it with the height it resumes from, which closes the
    // seed epoch there and writes on into a new one.
    let reopened =
        EpochTrieStore::open_with(&epochs, EpochConfig::default(), Some(1_009), factory()).unwrap();
    assert_eq!(reopened.epoch_count(), 2);
    let root = TrieNode::from_message(&reopened.get(state.root.as_slice()).unwrap(), &reopened);
    assert_eq!(
        root.get(&TrieKeySlice::from_key(CODE_KEY), &reopened),
        Some(long_value())
    );
}

#[test]
fn rskjs_structural_refusals_hold() {
    let (copy, _, _) = Copy::new(1_009, 6);
    let good = copy.v2();
    let refusals: Vec<(Vec<u8>, &str)> = vec![
        (Vec::new(), "Empty bootstrap data file"),
        (vec![0x10, 0x00], "Unrecognized bootstrap-data format"),
        (
            b"RSKBOOX\n\x02\x00".to_vec(),
            "Invalid bootstrap-data v2 magic",
        ),
        (
            b"RSKBOOT\n\x03\x00".to_vec(),
            "Unsupported bootstrap-data v2 version: 3",
        ),
        (
            good[..good.len() - 1].to_vec(),
            "missing end-of-sections marker",
        ),
        ([&good[..], &[0]].concat(), "trailing bytes"),
        (good[..good.len() - 20].to_vec(), "exceeds the"),
        (
            [
                &b"RSKBOOT\n\x02\x01"[..],
                &(MAX_CHUNK_LEN + 1).to_be_bytes(),
            ]
            .concat(),
            "chunk length out of range",
        ),
        (
            v2(&[
                (TAG_BLOCKS, chunks(&copy.blocks, 3)),
                (TAG_NODES, chunks(&copy.nodes, 7)),
                (TAG_VALUES, chunks(&copy.values, 3)),
            ]),
            "out of order",
        ),
        (
            v2(&[
                (TAG_VALUES, chunks(&copy.values, 3)),
                (TAG_NODES, chunks(&copy.nodes, 7)),
            ]),
            "no blocks section",
        ),
        (
            v2(&[
                (TAG_BLOCKS, chunks(&copy.blocks, 3)),
                (TAG_VALUES, chunks(&copy.values, 3)),
            ]),
            "no state-nodes section",
        ),
        (
            v2(&[
                (TAG_BLOCKS, chunks(&copy.blocks, 3)),
                (TAG_VALUES, vec![[&copy.values[0][..], &[0x80]].concat()]),
                (TAG_NODES, chunks(&copy.nodes, 7)),
            ]),
            "empty element",
        ),
        (
            v2(&[
                (TAG_BLOCKS, chunks(&copy.blocks, 3)),
                (TAG_VALUES, Vec::new()),
                (TAG_NODES, chunks(&copy.nodes, 7)),
            ]),
            "a referenced long value is missing",
        ),
        (
            v2(&[
                (
                    TAG_BLOCKS,
                    vec![[&copy.blocks[0][..], &[0x05, 0x06]].concat()],
                ),
                (TAG_VALUES, chunks(&copy.values, 3)),
                (TAG_NODES, chunks(&copy.nodes, 7)),
            ]),
            "Bootstrap-data v2 blocks section is malformed",
        ),
    ];
    for (bin, why) in refusals {
        let error = refused(&bin, 1_009);
        assert!(error.contains(why), "wanted {why:?}, got {error}");
    }
}

#[test]
fn a_node_not_in_the_form_rskj_writes_is_refused() {
    let (mut copy, _, _) = Copy::new(1_009, 6);
    let mut node = State::node_bytes(&copy.nodes[0]);
    node[0] &= !0b0100_0000; // the version bits rskj writes as 01
    copy.nodes[0] = element(&node);
    assert!(refused(&copy.v2(), 1_009).contains("not in the form rskj writes"));
}

impl State {
    fn node_bytes(element: &[u8]) -> Vec<u8> {
        data(element).unwrap().to_vec()
    }
}

#[test]
fn a_negative_total_difficulty_is_refused_as_rskj_refuses_it() {
    let (mut copy, _, blocks) = Copy::new(1_009, 6);
    copy.blocks[2] = list(&[element(&alloy_rlp::encode(&blocks[2])), element(&[0x80])]);
    assert!(refused(&copy.v2(), 1_009).contains("must be positive or zero"));
    assert_eq!(total_difficulty(&[0x00, 0x80]).unwrap(), U256::from(0x80));
    assert_eq!(total_difficulty(&[0x00]).unwrap(), U256::ZERO);
}

#[test]
fn blocks_that_do_not_make_one_chain_are_refused() {
    let (mut copy, _, _) = Copy::new(1_009, 6);
    copy.blocks.remove(3);
    assert!(refused(&copy.v2(), 1_009).contains("do not form one chain"));
    let (mut copy, _, _) = Copy::new(1_009, 6);
    let again = copy.blocks[1].clone();
    copy.blocks.push(again);
    assert!(refused(&copy.v2(), 1_009).contains("two blocks at #1005"));
}

#[test]
fn a_body_that_is_not_its_headers_is_refused() {
    let (mut copy, _, blocks) = Copy::new(1_009, 6);
    let mut odd = blocks[2].clone();
    odd.ommers = vec![blocks[0].header.clone()];
    copy.blocks[2] = block_element(&odd, td_of(odd.header.number));
    assert!(refused(&copy.v2(), 1_009).contains("uncles are not the ones its header names"));
}

#[test]
fn the_top_has_to_be_where_the_signers_say_and_hold_its_state() {
    let (copy, _, _) = Copy::new(1_009, 6);
    assert!(refused(&copy.v2(), 1_010).contains("where its signers list it"));
    let (mut copy, _, _) = Copy::new(1_009, 6);
    copy.blocks = chain(1_004, 6, B256::repeat_byte(9))
        .iter()
        .map(|b| block_element(b, td_of(b.header.number)))
        .collect();
    assert!(refused(&copy.v2(), 1_009).contains("is not among the copy's state nodes"));
}

#[test]
fn a_copy_from_genesis_has_to_start_at_this_networks() {
    let (copy, _, _) = Copy::new(5, 6);
    assert!(refused(&copy.v2(), 5).contains("not this network's genesis"));
}

#[test]
fn the_zip_is_held_to_rskjs_unzipper_and_its_checksums() {
    let (copy, _, _) = Copy::new(1_009, 6);
    let bin = copy.v2();
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (
            zip(&[("../bootstrap-data.bin", true, &bin)], false),
            "illegal entry",
        ),
        (
            zip(&[("other.bin", true, &bin)], false),
            "holds no bootstrap-data.bin",
        ),
        (
            zip(
                &[(BOOTSTRAP_ENTRY, true, &bin), (BOOTSTRAP_ENTRY, true, &bin)],
                false,
            ),
            "bootstrap-data.bin twice",
        ),
        (
            b"not a zip at all, nowhere near twenty-two bytes of one".to_vec(),
            "not a zip",
        ),
    ];
    for (zip_bytes, why) in cases {
        let error = format!("{:#}", load_zip(&zip_bytes, 1_009).result.unwrap_err());
        assert!(error.contains(why), "wanted {why:?}, got {error}");
    }
    // The CRC-32 the directory gives no longer the one the entry has.
    let mut damaged = zip(&[(BOOTSTRAP_ENTRY, false, &bin)], false);
    let directory = 30 + BOOTSTRAP_ENTRY.len() + bin.len();
    damaged[directory + 16] ^= 1;
    let error = format!("{:#}", load_zip(&damaged, 1_009).result.unwrap_err());
    assert!(error.contains("invalid entry CRC"), "{error}");
    // A byte changed where nothing reads it, in a section this reader skips:
    // only the CRC-32 can tell.
    let skipped = b"a section nobody reads";
    let with_skipped = v2(&[
        (0x04, vec![skipped.to_vec()]),
        (TAG_BLOCKS, chunks(&copy.blocks, 3)),
        (TAG_VALUES, chunks(&copy.values, 3)),
        (TAG_NODES, chunks(&copy.nodes, 7)),
    ]);
    let mut damaged = zip(&[(BOOTSTRAP_ENTRY, false, &with_skipped)], false);
    let at = 30 + BOOTSTRAP_ENTRY.len() + V2_MAGIC.len() + 1 + 1 + 8 + 2;
    assert_eq!(damaged[at], skipped[2]);
    damaged[at] ^= 1;
    let error = format!("{:#}", load_zip(&damaged, 1_009).result.unwrap_err());
    assert!(error.contains("invalid entry CRC"), "{error}");
}

#[test]
fn a_chain_already_held_is_refused_and_an_unfinished_import_is_redone() {
    let (copy, _, _) = Copy::new(1_009, 6);
    let loaded = load_bin(&copy.v2(), 1_009);
    loaded.result.unwrap();
    let dir = loaded.dir.path();
    let error = refuse_held_chain(&loaded.store, dir)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("already holds a chain, up to #1009"),
        "{error}"
    );
    refuse_unfinished(dir).unwrap();

    File::create(dir.join(UNFINISHED)).unwrap();
    refuse_held_chain(&loaded.store, dir).unwrap();
    assert!(refuse_unfinished(dir).is_err());

    let empty = tempfile::tempdir().unwrap();
    let store = BlockStore::open(empty.path()).unwrap();
    refuse_held_chain(&store, empty.path()).unwrap();
}

#[test]
fn rlp_reads_as_rskjs_does() {
    assert_eq!(data(&[0x05]).unwrap(), [0x05]);
    assert_eq!(
        data(&[0x81, 0x05]).unwrap(),
        [0x05],
        "a single byte written long"
    );
    assert_eq!(
        data(&[0xc1, 0x05]).unwrap(),
        [0xc1, 0x05],
        "a list is its whole encoding"
    );
    assert!(data(&[0x80]).unwrap().is_empty());
    assert!(
        decode_list(&[0xc1, 0x05, 0x00]).is_err(),
        "bytes after the list"
    );
    assert!(decode_list(&[0x81, 0x05]).is_err(), "not a list");
    assert!(
        items(&[0x82, 0x05]).is_err(),
        "an item running past its container"
    );
}

// ----------------------------------------------------------- the whole import

/// Serves what it was given, as Rootstock's server would.
struct Served(HashMap<String, Vec<u8>>);

#[async_trait::async_trait]
impl Source for Served {
    async fn get(&self, url: &str, sink: &mut dyn Sink) -> Result<u64> {
        let body = self
            .0
            .get(url)
            .with_context(|| format!("HTTP 404 for {url}"))?;
        for piece in body.chunks(4096) {
            sink.write(piece)?;
        }
        Ok(body.len() as u64)
    }
}

fn signer(n: u8) -> (SigningKey, String) {
    let key = SigningKey::from_slice(&[n; 32]).unwrap();
    let public = hex::encode(key.verifying_key().to_encoded_point(false).as_bytes());
    (key, public)
}

fn sign(key: &SigningKey, hash: &[u8]) -> (String, String) {
    let signature: Signature = key.sign_prehash(hash).unwrap();
    let (r, s) = signature.split_bytes();
    (hex::encode(r), hex::encode(s))
}

const URL: &str = "https://copies.example/regtest/";

/// Three signers' indexes for a zip at #2,003, each signed by `signs`, and
/// an older copy at #1,000 all three agree on. The zip itself is served for
/// the signers in `serving` only.
fn published(
    zip_bytes: &[u8],
    signs: impl Fn(usize) -> bool,
    serving: impl Fn(usize) -> bool,
) -> (Served, Vec<String>) {
    let hash = Sha256::digest(zip_bytes);
    let mut served = HashMap::new();
    let mut keys = Vec::new();
    for i in 0..3 {
        let (key, public) = signer(i as u8 + 1);
        let (r, s) = if signs(i) {
            sign(&key, &hash)
        } else {
            ("01".into(), "01".into())
        };
        let db = format!("{public}/2003/output.zip");
        let index = serde_json::json!({ "dbs": [
            { "height": 1000, "date": "", "db": format!("{public}/1000/output.zip"),
              "hash": "00".repeat(32), "sig": { "r": "01", "s": "01" } },
            { "height": 2003, "date": "14/11/2026 12:00:00", "db": db,
              "hash": hex::encode(hash), "sig": { "r": r, "s": s } },
        ]});
        served.insert(
            format!("{URL}{public}/index.json"),
            serde_json::to_vec(&index).unwrap(),
        );
        if serving(i) {
            served.insert(format!("{URL}{db}"), zip_bytes.to_vec());
        }
        keys.push(public);
    }
    (Served(served), keys)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn import_reads_the_indexes_checks_the_signatures_and_loads_the_newest_copy() {
    let (copy, state, blocks) = Copy::new(2_003, 4);
    let zip_bytes = zip(&[(BOOTSTRAP_ENTRY, true, &copy.v2())], false);
    // Only the third signer serves the copy: whichever signer is tried
    // first, the import has to reach it.
    let (served, keys) = published(&zip_bytes, |_| true, |i| i == 2);
    let signers = Signers::for_network(33, Some(URL), &keys).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = BlockStore::open(dir.path()).unwrap();
    let trie = TrieTarget::Column(store.db().clone());
    let summary = import(
        &served,
        &signers,
        dir.path(),
        &store,
        &trie,
        &ChainConfig::regtest(),
    )
    .await
    .unwrap();
    assert_loaded(&store, &blocks, &state, &summary);
    assert!(
        !dir.path().join(DOWNLOAD_DIR).exists(),
        "the download is removed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn import_stops_without_enough_signatures_or_a_copy_that_hashes_right() {
    let (copy, _, _) = Copy::new(2_003, 4);
    let zip_bytes = zip(&[(BOOTSTRAP_ENTRY, true, &copy.v2())], false);
    let attempt = |served: Served, keys: Vec<String>| async move {
        let signers = Signers::for_network(33, Some(URL), &keys).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = BlockStore::open(dir.path()).unwrap();
        let trie = TrieTarget::Column(store.db().clone());
        let result = import(
            &served,
            &signers,
            dir.path(),
            &store,
            &trie,
            &ChainConfig::regtest(),
        )
        .await;
        let held = store.head().unwrap();
        (format!("{:#}", result.unwrap_err()), held)
    };

    let (served, keys) = published(&zip_bytes, |i| i == 0, |_| true);
    let (error, held) = attempt(served, keys).await;
    assert!(error.contains("Not enough valid signatures"), "{error}");
    assert_eq!(held, None);

    // Signed, but every server has a different file.
    let (mut served, keys) = published(&zip_bytes, |_| true, |_| true);
    for (url, body) in served.0.iter_mut() {
        if url.ends_with(".zip") {
            body.push(0);
        }
    }
    let (error, held) = attempt(served, keys).await;
    assert!(
        error.contains("does not match with expected hash"),
        "{error}"
    );
    assert_eq!(held, None);

    // A signer's index that cannot be read stops it, as in rskj.
    let (mut served, keys) = published(&zip_bytes, |_| true, |_| true);
    served
        .0
        .retain(|url, _| !url.ends_with(&format!("{}/index.json", keys[1])));
    let (error, _) = attempt(served, keys).await;
    assert!(
        error.contains("Failed to download and parse index"),
        "{error}"
    );
}
