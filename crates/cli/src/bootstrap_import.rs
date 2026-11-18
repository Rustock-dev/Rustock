//! `--import`: start from the newest state copy Rootstock publishes, the way
//! rskj's `--import` does (`co.rsk.db.importer`, VETIVER-9.0.4).
//!
//! The steps are rskj's, in rskj's order:
//!
//! 1. **The signers' indexes.** Each trusted key has `<url><key>/index.json`,
//!    listing the copies it vouches for: a height, a file, the file's SHA-256
//!    and a signature over that hash (`BootstrapIndexRetriever`). Every index
//!    has to be read.
//! 2. **The height.** The highest height at which more than half the keys, and
//!    never fewer than two, name the same hash (`BootstrapIndexCandidateSelector`,
//!    with the minimum `RskContext.getBootstrapImporter` sets). A key that lists
//!    one height twice is an error.
//! 3. **The signatures.** Each of those entries is checked against its key, and
//!    as many signatures as that minimum have to hold (`BootstrapDataVerifier`).
//! 4. **The copy.** Downloaded from one of those entries chosen at random, and
//!    its SHA-256 checked against the signed hash (`BootstrapFileHandler`).
//! 5. **The load.** `bootstrap-data.bin`, out of the zip, in either of its
//!    formats and with every check rskj's reader makes (`BootstrapImporter`):
//!    the blocks with their total difficulties, the long values, and the state
//!    nodes, each stored under the Keccak-256 that rskj's `TrieStoreImpl` stores
//!    it under.
//!
//! Where this differs, it is in what it refuses or how it recovers, never in
//! what it accepts as signed:
//!
//! * rskj deletes the database before importing. This refuses a data directory
//!   that already holds a chain instead, so a flag left on a command line
//!   cannot wipe a synced node.
//! * A copy that does not download, or does not hash to the signed hash, is
//!   fetched from the next signer that lists it, where rskj stops. Every
//!   signer's file is the same file: the signatures cover its hash.
//! * A state node whose message is not the one rskj would write for it is
//!   refused. rskj would store such a node re-encoded, under the hash of the
//!   re-encoding; the copies are written by rskj's own encoder, so none has one.
//! * Beyond rskj's checks, the blocks have to link, each body has to be the
//!   one its header names, the highest block has to be at the height the
//!   signers named, and its state root has to be among the nodes. Nothing
//!   executes those blocks, so these are the only checks they get; everything
//!   after them the node executes and checks against the network as usual.
//!
//! The node then starts from the highest block as from any other. The blocks
//! below the copy's lowest are recorded as absent, with the same floor a
//! pruned database has.

use crate::https::{self, Sink};
use alloy_primitives::{keccak256, B256, U256};
use alloy_rlp::Decodable;
use anyhow::{anyhow, bail, ensure, Context, Result};
use k256::ecdsa::signature::hazmat::PrehashVerifier;
use k256::ecdsa::{Signature, VerifyingKey};
use rand::seq::SliceRandom;
use rustock_core::config::ChainConfig;
use rustock_core::{Block, Header};
use rustock_storage::epoch_backend::CF_TRIE;
use rustock_storage::epoch_store::{EpochTrieStore, TrieBackend};
use rustock_storage::{
    BlockStore, FlatTrieStore, RocksDbTrieStore, Transition, TrieEngine, Validated,
};
use rustock_trie::{empty_trie_hash, MemoryTrieStore, NodeRef, TrieNode, TrieStore};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// rskj's `config/main.conf`, `database.import`.
const MAINNET_URL: &str = "https://import.mainnet.rskcomputing.net/dbs/mainnet/";
const MAINNET_KEYS: [&str; 3] = [
    "045499f5553ac687c8d196819f69536c0ae585b73e55caa4892fe86067cc1d43bae7edb52ad855a16a030e32cac4fc60b0fd1b0ee5c0230e584b8cde790daaff1a",
    "04639c3f22ffafff9fef6e2ae8a0e6b7ecd9751d765d986f05b68fff20f0d041ab2aa093e07ed613c25589f93e00005b1155a2303e8bfe42930e74e2398106f707",
    "0479df483b141170c247d4af4e5b92f752bd240ddb57abf021a06c57c7fbeb8a20ed9f35b0f914531a7601f863abd01f0dac722cf9d8fc1137992e18a778b1d8c4",
];
/// rskj's `config/testnet.conf`, `database.import`.
const TESTNET_URL: &str = "https://import.testnet.rskcomputing.net/dbs/testnet/";
const TESTNET_KEYS: [&str; 3] = [
    "0437e3ab7b714831d84c75fa05752ef013947a00adfd6e088b75a7d9ff8e55430b10962e8b35a3bf74c9323c502022d64e7960526f68b903a50eb6f6096ebeb7ff",
    "04b39e3cf64a3889c41dac45fd42d280787a101786ffd574faa212ab66e0197c9bfd5cf4a36d0c0756cbcfd845ed470059391209e0bea3103a1e65fcea92fa4a7b",
    "04c64c11b9d9e68a60bd39f186bc1daa736d1330415fc285dccb0cfcff45dd054bf7701758051141b7718c67b20b4065d8a60a411db70221ff463b5b1cc27e6365",
];

const INDEX_NAME: &str = "index.json";
/// The most an index may hold. Rootstock's run to about 72 KB.
const MAX_INDEX_BYTES: usize = 16 << 20;
/// The file in the zip that holds the copy.
const BOOTSTRAP_ENTRY: &str = "bootstrap-data.bin";
/// Where the copy is downloaded to, inside the data directory, and removed after.
const DOWNLOAD_DIR: &str = "import";
/// Present in the data directory while an import has written part of a chain.
const UNFINISHED: &str = "import-unfinished";
const PROGRESS_EVERY: Duration = Duration::from_secs(15);

/// rskj's `BootstrapV2Format`.
const V2_MAGIC: &[u8; 8] = b"RSKBOOT\n";
const V2_VERSION: u8 = 0x02;
const TAG_END: u8 = 0x00;
const TAG_BLOCKS: u8 = 0x01;
const TAG_NODES: u8 = 0x02;
const TAG_VALUES: u8 = 0x03;
/// `MAX_CHUNK_LEN`: the most a v2 chunk may declare.
const MAX_CHUNK_LEN: u64 = 512 << 20;

/// Trie entries per write.
const BATCH: usize = 50_000;

/// Where a network's copies are published, and whose signatures they need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Signers {
    /// Ends in `/`: an index is at `<url><key>/index.json`, and a copy at
    /// `<url><db>`, as rskj's `BootstrapURLProvider` builds them.
    pub url: String,
    /// Uncompressed SEC1 public keys, in lowercase hex.
    pub keys: Vec<String>,
}

impl Signers {
    /// rskj's `database.import` for the network, with `--import-url` and
    /// `--import-trusted-key` in place of its `url` and `trusted-keys`.
    pub(crate) fn for_network(network_id: u64, url: Option<&str>, keys: &[String]) -> Result<Self> {
        let (published_url, published_keys): (Option<&str>, &[&str]) = match network_id {
            30 => (Some(MAINNET_URL), &MAINNET_KEYS),
            31 => (Some(TESTNET_URL), &TESTNET_KEYS),
            _ => (None, &[]),
        };
        let url = url.or(published_url).context(
            "Rootstock publishes copies of mainnet and testnet only; for another network \
             give --import-url and --import-trusted-key",
        )?;
        let keys: Vec<String> = if keys.is_empty() {
            published_keys.iter().map(|k| k.to_string()).collect()
        } else {
            keys.iter().map(|k| k.trim().to_ascii_lowercase()).collect()
        };
        for (i, key) in keys.iter().enumerate() {
            let bytes =
                hex::decode(key).with_context(|| format!("trusted key {key:?} is not hex"))?;
            VerifyingKey::from_sec1_bytes(&bytes)
                .map_err(|_| anyhow!("trusted key {key} is not a secp256k1 public key"))?;
            ensure!(!keys[..i].contains(key), "trusted key {key} is given twice");
        }
        ensure!(
            keys.len() >= 2,
            "{} trusted key: a copy has to be signed by at least two, as rskj requires",
            keys.len()
        );
        let mut url = url.to_string();
        if !url.ends_with('/') {
            url.push('/');
        }
        https::parse_url(&url)?;
        Ok(Self { url, keys })
    }

    /// How many have to agree, and sign: rskj's `keys / 2 + 1`, raised to two.
    pub(crate) fn threshold(&self) -> usize {
        (self.keys.len() / 2 + 1).max(2)
    }
}

/// A signer's `index.json` (rskj's `BootstrapDataIndex`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Index {
    pub dbs: Vec<Entry>,
}

/// One copy a signer lists (rskj's `BootstrapDataEntry`). Its `date` is
/// not read, by rskj or here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Entry {
    pub height: u64,
    /// The file, relative to the publishing URL.
    pub db: String,
    /// The file's SHA-256, in hex: what the signature covers.
    pub hash: String,
    pub sig: EntrySignature,
}

/// rskj's `BootstrapDataSignature`: secp256k1's `r` and `s`, in hex.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct EntrySignature {
    pub r: String,
    pub s: String,
}

/// The height chosen, and the entries at it that name the agreed hash, each
/// with its signer's position among the trusted keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub height: u64,
    pub entries: Vec<(usize, Entry)>,
}

/// rskj's `BootstrapIndexCandidateSelector.getHeightData`: the highest height
/// at which `threshold` signers name the same hash. `indexes[i]` is the index
/// `keys[i]` published.
pub(crate) fn select(indexes: &[Index], keys: &[String], threshold: usize) -> Result<Candidate> {
    let mut per_height: BTreeMap<u64, BTreeMap<usize, &Entry>> = BTreeMap::new();
    for (signer, (index, key)) in indexes.iter().zip(keys).enumerate() {
        for entry in &index.dbs {
            if per_height
                .entry(entry.height)
                .or_default()
                .insert(signer, entry)
                .is_some()
            {
                bail!(
                    "There is an invalid file from {key}: it has 2 entries from same height {}",
                    entry.height
                );
            }
        }
    }
    ensure!(
        !per_height.is_empty(),
        "Downloaded files contain no height entries"
    );
    for (&height, entries) in per_height.iter().rev() {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for entry in entries.values() {
            *counts.entry(entry.hash.as_str()).or_default() += 1;
        }
        // More than half the keys have to agree, so no two hashes can both reach it.
        let Some((&hash, &count)) = counts.iter().max_by_key(|(_, n)| **n) else {
            continue;
        };
        if count >= threshold {
            let entries = entries
                .iter()
                .filter(|(_, e)| e.hash == hash)
                .map(|(&signer, &e)| (signer, e.clone()))
                .collect();
            return Ok(Candidate { height, entries });
        }
    }
    bail!("Downloaded files doesn't contain enough entries for a common height")
}

/// rskj's `BootstrapDataVerifier.verifyEntries`: how many of the candidate's
/// entries are signed by their signer.
pub(crate) fn verify(candidate: &Candidate, keys: &[String]) -> Result<usize> {
    let Some((_, first)) = candidate.entries.first() else {
        return Ok(0);
    };
    let hash = hex_bytes(&first.hash).context("the copy's hash")?;
    let mut valid = 0;
    for (signer, entry) in &candidate.entries {
        ensure!(
            entry.hash == first.hash,
            "Error trying to verify different hashes: {} vs {}",
            first.hash,
            entry.hash
        );
        let key = &keys[*signer];
        let key = VerifyingKey::from_sec1_bytes(&hex_bytes(key)?)
            .map_err(|_| anyhow!("trusted key {key} is not a secp256k1 public key"))?;
        let r = hex_bytes(&entry.sig.r).context("a signature's r")?;
        let s = hex_bytes(&entry.sig.s).context("a signature's s")?;
        if signs(&key, &hash, &r, &s) {
            valid += 1;
        }
    }
    Ok(valid)
}

/// rskj reads every hex string with BouncyCastle's `Hex.decode`, which fails
/// on anything that is not hex.
fn hex_bytes(s: &str) -> Result<Vec<u8>> {
    hex::decode(s).with_context(|| format!("{s:?} is not hex"))
}

/// rskj's `Secp256k1.verify`, on BouncyCastle's `ECDSASigner`: `r` and `s`
/// are unsigned integers of any length, valid from 1 to the group order.
fn signs(key: &VerifyingKey, hash: &[u8], r: &[u8], s: &[u8]) -> bool {
    let (Some(r), Some(s)) = (scalar(r), scalar(s)) else {
        return false;
    };
    let Ok(signature) = Signature::from_scalars(r, s) else {
        return false;
    };
    // BouncyCastle takes an `s` from either half of the range, and two of
    // the three testnet signers use the upper half. k256 takes only the
    // lower, and `n - s` verifies exactly when `s` does.
    let signature = signature.normalize_s().unwrap_or(signature);
    key.verify_prehash(hash, &signature).is_ok()
}

/// `new BigInteger(1, bytes)` as 32 bytes, or `None` past 2^256.
fn scalar(bytes: &[u8]) -> Option<k256::FieldBytes> {
    let start = bytes.iter().position(|b| *b != 0).unwrap_or(bytes.len());
    let digits = &bytes[start..];
    if digits.len() > 32 {
        return None;
    }
    let mut out = k256::FieldBytes::default();
    out[32 - digits.len()..].copy_from_slice(digits);
    Some(out)
}

/// Fetches over HTTPS; the tests stand in for it.
#[async_trait::async_trait]
pub(crate) trait Source: Send + Sync {
    async fn get(&self, url: &str, sink: &mut dyn Sink) -> Result<u64>;
}

pub(crate) struct Web;

#[async_trait::async_trait]
impl Source for Web {
    async fn get(&self, url: &str, sink: &mut dyn Sink) -> Result<u64> {
        https::get(url, sink).await
    }
}

/// A body kept in memory, up to a limit.
struct Capped {
    body: Vec<u8>,
    limit: usize,
}

impl Sink for Capped {
    fn write(&mut self, data: &[u8]) -> Result<()> {
        ensure!(
            self.body.len() + data.len() <= self.limit,
            "more than {} bytes",
            self.limit
        );
        self.body.extend_from_slice(data);
        Ok(())
    }
}

async fn fetch_index(source: &dyn Source, url: &str) -> Result<Index> {
    let mut index = Capped {
        body: Vec::new(),
        limit: MAX_INDEX_BYTES,
    };
    source.get(url, &mut index).await?;
    serde_json::from_slice(&index.body).context("reading the index")
}

/// A download on its way to disk, hashed as it goes.
struct Download {
    file: BufWriter<File>,
    sha: Sha256,
    bytes: u64,
    started: Instant,
    said: Instant,
}

impl Sink for Download {
    fn write(&mut self, data: &[u8]) -> Result<()> {
        self.file
            .write_all(data)
            .context("writing the copy to disk")?;
        self.sha.update(data);
        self.bytes += data.len() as u64;
        if self.said.elapsed() >= PROGRESS_EVERY {
            self.said = Instant::now();
            let secs = self.started.elapsed().as_secs_f64();
            info!(
                "Import: {} MB downloaded ({:.1} MB/s)",
                self.bytes >> 20,
                self.bytes as f64 / (1 << 20) as f64 / secs
            );
        }
        Ok(())
    }
}

/// rskj's `BootstrapFileHandler.retrieveAndUnpack`, up to the unpacking: the
/// copy from one of the candidate's entries chosen at random, kept only if
/// it hashes to the signed hash. Where rskj stops at the first failure, the
/// next entry is tried.
async fn download(
    source: &dyn Source,
    url: &str,
    candidate: &Candidate,
    path: &Path,
) -> Result<()> {
    let mut order: Vec<&Entry> = candidate.entries.iter().map(|(_, e)| e).collect();
    order.shuffle(&mut rand::thread_rng());
    let mut failed = Vec::new();
    for entry in order {
        let from = format!("{url}{}", entry.db);
        match fetch_copy(source, &from, &entry.hash, path).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                warn!("Import: the copy at {from} cannot be used: {e:#}");
                failed.push(format!("{from}: {e:#}"));
            }
        }
    }
    bail!("no signer's copy could be used:\n  {}", failed.join("\n  "))
}

async fn fetch_copy(source: &dyn Source, url: &str, hash: &str, path: &Path) -> Result<()> {
    let want = hex_bytes(hash)?;
    let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let now = Instant::now();
    let mut copy = Download {
        file: BufWriter::with_capacity(1 << 20, file),
        sha: Sha256::new(),
        bytes: 0,
        started: now,
        said: now,
    };
    info!("Import: downloading {url}");
    source.get(url, &mut copy).await?;
    copy.file.flush().context("writing the copy to disk")?;
    ensure!(
        copy.sha.finalize()[..] == want[..],
        "File: {} does not match with expected hash: {hash}",
        path.display()
    );
    info!(
        "Import: {} MB downloaded in {:.0} s, and its SHA-256 is the signed one",
        copy.bytes >> 20,
        copy.started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// Refuses `--import` into a data directory that holds a chain: rskj deletes
/// the database first, which a flag left on a command line would turn into
/// the loss of a synced node. A store that never got past genesis holds
/// nothing to lose, and one an import left unfinished is redone.
pub(crate) fn refuse_held_chain(store: &BlockStore, data_dir: &Path) -> Result<()> {
    if data_dir.join(UNFINISHED).exists() {
        return Ok(());
    }
    let head = match store.head()? {
        Some(hash) => store.header(hash)?.map_or(0, |h| h.number),
        None => 0,
    };
    ensure!(
        head == 0 && store.exec_head()?.is_none(),
        "{} already holds a chain, up to #{head}. --import loads a copy into a data \
         directory that holds none; start without it to run on this chain, or give an \
         empty --data-dir",
        data_dir.display()
    );
    Ok(())
}

/// Refuses to start on a chain an import left half-written.
pub(crate) fn refuse_unfinished(data_dir: &Path) -> Result<()> {
    ensure!(
        !data_dir.join(UNFINISHED).exists(),
        "an --import into {} did not finish, so it holds part of a chain. Run it again \
         with --import, or start from an empty --data-dir",
        data_dir.display()
    );
    Ok(())
}

/// What an import put in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Summary {
    pub lowest: u64,
    pub highest: u64,
    pub hash: B256,
    pub state_root: B256,
    pub nodes: u64,
    pub values: u64,
}

/// Everything `--import` does, from the signers' indexes to the node's new
/// position (rskj's `BootstrapImporter.importData`).
pub(crate) async fn import(
    source: &dyn Source,
    signers: &Signers,
    data_dir: &Path,
    store: &BlockStore,
    trie: &TrieTarget,
    chain: &ChainConfig,
) -> Result<Summary> {
    let started = Instant::now();
    let mut indexes = Vec::with_capacity(signers.keys.len());
    for key in &signers.keys {
        let url = format!("{}{key}/{INDEX_NAME}", signers.url);
        let index = fetch_index(source, &url)
            .await
            .with_context(|| format!("Failed to download and parse index from {url}"))?;
        indexes.push(index);
    }
    let threshold = signers.threshold();
    let candidate = select(&indexes, &signers.keys, threshold)?;
    let valid = verify(&candidate, &signers.keys)?;
    ensure!(
        valid >= threshold,
        "Not enough valid signatures: selected height {} doesn't have enough trustworthy \
         sources: {valid} of {threshold}",
        candidate.height
    );
    info!(
        "Import: Rootstock's copy at #{} ({}), signed by {valid} of the {} trusted keys",
        candidate.height,
        candidate.entries[0].1.hash,
        signers.keys.len()
    );

    let dir = data_dir.join(DOWNLOAD_DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let zip = dir.join(format!("{}.zip", candidate.height));
    let loaded = async {
        download(source, &signers.url, &candidate, &zip).await?;
        tokio::task::block_in_place(|| load(&zip, candidate.height, data_dir, store, trie, chain))
    }
    .await;
    // Spent either way: a failed load starts again from the indexes.
    let _ = std::fs::remove_file(&zip);
    let _ = std::fs::remove_dir(&dir);
    let summary = loaded?;
    info!(
        "Import: the node starts at #{} {:?}, {:.0} s after the first index was asked for",
        summary.highest,
        summary.hash,
        started.elapsed().as_secs_f64()
    );
    Ok(summary)
}

/// rskj's `BootstrapImporter.updateDatabase`, from the downloaded zip:
/// `bootstrap-data.bin` into the store, then the node's position at its
/// highest block.
pub(crate) fn load(
    zip: &Path,
    height: u64,
    data_dir: &Path,
    store: &BlockStore,
    trie: &TrieTarget,
    chain: &ChainConfig,
) -> Result<Summary> {
    let started = Instant::now();
    let mut file = File::open(zip).with_context(|| format!("opening {}", zip.display()))?;
    let entries = zip_entries(&mut file).context("The file is corrupted or incomplete")?;
    let entry = bootstrap_entry(&entries)?.clone();
    let reader = open_entry(file, &entry)?;

    // From the first write until the position is recorded, the store holds
    // part of a chain; this says so to the next start.
    let unfinished = data_dir.join(UNFINISHED);
    File::create(&unfinished).with_context(|| format!("creating {}", unfinished.display()))?;

    let mut loader = Loader::new(store, trie);
    let format = read_bootstrap(reader, entry.size, &mut loader)?;
    info!(
        "Import: bootstrap-data {format} read: {} blocks, {} long values and {} state nodes \
         in {:.1} s",
        loader.blocks.len(),
        loader.values_read,
        loader.nodes,
        started.elapsed().as_secs_f64()
    );
    let summary = loader.finish(height, chain)?;
    std::fs::remove_file(&unfinished)
        .with_context(|| format!("removing {}", unfinished.display()))?;
    info!(
        "Import: loaded in {:.1} s: blocks #{}..#{}, state root {:?}",
        started.elapsed().as_secs_f64(),
        summary.lowest,
        summary.highest,
        summary.state_root
    );
    Ok(summary)
}

/// Where the state goes: the store the node will run on, as `--trie-backend`
/// and `--trie-engine` choose it.
pub(crate) enum TrieTarget {
    /// `trie_nodes` in the node's own database: `--trie-backend single`.
    Column(Arc<rocksdb::DB>),
    /// `--trie-backend external` on rocksdb.
    Rocks(RocksDbTrieStore),
    /// `--trie-backend external` on flatdb.
    Flat(FlatTrieStore),
    /// `--trie-backend epoch`: one seed epoch, which the node closes at the
    /// height it resumes from when it opens the store.
    Epochs(Box<EpochTrieStore>),
}

impl TrieTarget {
    pub(crate) fn open(
        backend: &TrieBackend,
        engine: TrieEngine,
        expected_keys: u64,
        trie_dir: &str,
        store: &BlockStore,
    ) -> Result<Self> {
        let dir = Path::new(trie_dir);
        Ok(match backend {
            TrieBackend::Single => Self::Column(store.db().clone()),
            TrieBackend::External => {
                engine.check_matches(dir)?;
                match engine {
                    TrieEngine::RocksDb => Self::Rocks(RocksDbTrieStore::open(dir)?),
                    TrieEngine::FlatDb => Self::Flat(FlatTrieStore::open(
                        dir,
                        rustock_flatdb::Options {
                            expected_keys,
                            ..rustock_flatdb::Options::default()
                        },
                    )?),
                }
            }
            TrieBackend::Epochs(config) => {
                engine.check_matches(dir)?;
                // No resume point: the node gives it when it opens the store.
                Self::Epochs(Box::new(EpochTrieStore::open_with(
                    dir,
                    config.clone(),
                    None,
                    engine.factory(expected_keys),
                )?))
            }
        })
    }

    fn put(&self, entries: &[(B256, &[u8])]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        if let Self::Column(db) = self {
            let cf = db
                .cf_handle(CF_TRIE)
                .context("the database has no trie_nodes column family")?;
            let mut batch = rocksdb::WriteBatch::default();
            for (key, value) in entries {
                batch.put_cf(cf, key.as_slice(), value);
            }
            return db.write(batch).context("writing state to the database");
        }
        let owned: Vec<(Vec<u8>, Vec<u8>)> = entries
            .iter()
            .map(|(k, v)| (k.to_vec(), v.to_vec()))
            .collect();
        match self {
            Self::Rocks(s) => s.put_many(&owned),
            Self::Flat(s) => s.put_many(&owned),
            Self::Epochs(s) => s.put_many(&owned),
            Self::Column(_) => {}
        }
        self.healthy()
    }

    /// The stores that cannot return an error from a write say afterwards.
    fn healthy(&self) -> Result<()> {
        let failed = match self {
            Self::Rocks(s) => s.write_failed(),
            Self::Flat(s) => s.write_failed(),
            Self::Column(_) | Self::Epochs(_) => false,
        };
        ensure!(
            !failed,
            "a write to the trie store failed; the log says why"
        );
        Ok(())
    }

    /// Durable before the node's position names the state.
    fn finish(&self) -> Result<()> {
        match self {
            // The database's own log; synced with the position.
            Self::Column(_) => Ok(()),
            Self::Rocks(s) => {
                s.flush();
                self.healthy()
            }
            Self::Flat(s) => {
                s.flush();
                self.healthy()
            }
            Self::Epochs(s) => EpochTrieStore::flush(s),
        }
    }

    fn holds(&self, key: &B256) -> Result<bool> {
        Ok(match self {
            Self::Column(db) => {
                let cf = db
                    .cf_handle(CF_TRIE)
                    .context("no trie_nodes column family")?;
                db.get_cf(cf, key.as_slice())?.is_some()
            }
            Self::Rocks(s) => s.get(key.as_slice()).is_some(),
            Self::Flat(s) => s.get(key.as_slice()).is_some(),
            Self::Epochs(s) => s.get(key.as_slice()).is_some(),
        })
    }
}

/// What has been read from the copy so far, and where it went.
struct Loader<'a> {
    store: &'a BlockStore,
    trie: &'a TrieTarget,
    threads: usize,
    blocks: BTreeMap<u64, (B256, Header, U256)>,
    /// Each long value's hash and length, which the nodes are checked against.
    values: HashMap<B256, usize>,
    /// The long values read, as rskj counts them: one copy can hold a value
    /// more than once.
    values_read: u64,
    nodes: u64,
}

impl<'a> Loader<'a> {
    fn new(store: &'a BlockStore, trie: &'a TrieTarget) -> Self {
        let threads = std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(16);
        Self {
            store,
            trie,
            threads,
            blocks: BTreeMap::new(),
            values: HashMap::new(),
            values_read: 0,
            nodes: 0,
        }
    }

    /// rskj's `saveBlockFromTuple`: `[block, totalDifficulty]`.
    fn block(&mut self, tuple: &[u8]) -> Result<()> {
        let parts = decode_list(tuple)?;
        ensure!(parts.len() >= 2, "a block entry of {} item(s)", parts.len());
        let encoded = non_empty(data(parts[0])?, "block data is missing")?;
        let td = total_difficulty(non_empty(
            data(parts[1])?,
            "block difficulty data is missing",
        )?)?;
        let mut rest = encoded;
        let block = Block::decode(&mut rest).context("decoding a block")?;
        let number = block.header.number;
        ensure!(
            rest.is_empty(),
            "{} bytes after block #{number}",
            rest.len()
        );
        check_body(&block)?;
        ensure!(
            !self.blocks.contains_key(&number),
            "the copy holds two blocks at #{number}"
        );
        let hash = block.header.hash();
        self.store.put_header_with_hash(hash, &block.header)?;
        self.store
            .put_body(hash, &block.transactions, &block.ommers)?;
        self.store.put_total_difficulty(hash, td)?;
        self.blocks.insert(number, (hash, block.header, td));
        Ok(())
    }

    /// rskj's `TrieStore.saveValue`: each long value under its Keccak-256.
    fn values(&mut self, elements: &[&[u8]]) -> Result<()> {
        let mut batch = Vec::with_capacity(elements.len().min(BATCH));
        for element in elements {
            let value = non_empty(data(element)?, "the values section holds an empty element")?;
            let key = keccak256(value);
            self.values.insert(key, value.len());
            self.values_read += 1;
            batch.push((key, value));
            if batch.len() == BATCH {
                self.trie.put(&batch)?;
                batch.clear();
            }
        }
        self.trie.put(&batch)
    }

    /// rskj's `Trie.fromMessage` and `TrieStore.save` for each node.
    fn nodes(&mut self, elements: &[&[u8]]) -> Result<()> {
        for part in elements.chunks(BATCH) {
            let checked = check_nodes(part, &self.values, self.threads)?;
            self.trie.put(&checked)?;
            self.nodes += checked.len() as u64;
        }
        Ok(())
    }

    /// One v2 chunk: whole RLP elements, all of one section. Whatever fails in
    /// it is reported as rskj's `importElements` reports it, with the section.
    fn chunk(&mut self, tag: u8, chunk: &[u8]) -> Result<()> {
        let section = match tag {
            TAG_BLOCKS => "blocks",
            TAG_VALUES => "values",
            _ => "nodes",
        };
        let malformed = || format!("Bootstrap-data v2 {section} section is malformed or corrupt");
        let elements = items(chunk).with_context(malformed)?;
        match tag {
            TAG_BLOCKS => elements.iter().try_for_each(|element| {
                self.block(non_empty(
                    data(element)?,
                    "the blocks section holds an empty element",
                )?)
            }),
            TAG_VALUES => self.values(&elements),
            _ => self.nodes(&elements),
        }
        .with_context(malformed)
    }

    /// The node's position at the copy's highest block, once everything in
    /// the copy is stored.
    fn finish(self, height: u64, chain: &ChainConfig) -> Result<Summary> {
        let (&lowest, _) = self
            .blocks
            .first_key_value()
            .context("Bootstrap-data has no blocks; refusing to import incomplete data")?;
        let (&highest, (tip_hash, tip, _)) = self.blocks.last_key_value().context("no blocks")?;
        ensure!(
            highest == height,
            "the copy's highest block is #{highest}, where its signers list it as the copy \
             at #{height}"
        );
        self.trie.finish()?;
        ensure!(
            self.trie.holds(&tip.state_root)?,
            "the state root of #{highest}, {:?}, is not among the copy's state nodes",
            tip.state_root
        );

        // Genesis, as the node's own start writes it, without moving the head
        // to it: that start finds it and leaves the position alone.
        let genesis = chain.genesis_header();
        let genesis_hash = chain.known_genesis_hash().unwrap_or_else(|| genesis.hash());
        match self.blocks.get(&0) {
            Some((hash, _, _)) => ensure!(
                *hash == genesis_hash,
                "the copy's block #0 is {hash:?}, not this network's genesis {genesis_hash:?}"
            ),
            None => {
                self.store.put_header_with_hash(genesis_hash, &genesis)?;
                self.store
                    .put_total_difficulty(genesis_hash, genesis.difficulty)?;
            }
        }
        self.store.put_canonical_hash(0, genesis_hash)?;
        // Nothing below the copy's lowest block is held: the floor a pruned
        // database records, which everything that walks the chain stops at.
        if lowest > 0 {
            let (hash, _, td) = &self.blocks[&lowest];
            self.store.set_history_floor(lowest, *hash, *td)?;
        }

        // Adopting the tip proves that every block down to the floor is held
        // and names the one below as its parent.
        let head = Validated::prove(self.store, *tip_hash)
            .map_err(|e| anyhow!("the copy's blocks do not form one chain: {e}"))?;
        self.store.apply(&Transition::Adopt { head })?;
        let at = Validated::prove(self.store, *tip_hash)
            .map_err(|e| anyhow!("the copy's blocks do not form one chain: {e}"))?;
        self.store.apply(&Transition::Executed {
            at,
            state_root: tip.state_root,
        })?;
        // Into the tables, so the log the load wrote can go: the node's next
        // start then has none of it to replay.
        self.store.flush_all()?;

        Ok(Summary {
            lowest,
            highest,
            hash: *tip_hash,
            state_root: tip.state_root,
            nodes: self.nodes,
            values: self.values_read,
        })
    }
}

/// Every node checked on as many threads as there are cores, then keyed for
/// the store in the copy's order.
fn check_nodes<'a>(
    elements: &[&'a [u8]],
    values: &HashMap<B256, usize>,
    threads: usize,
) -> Result<Vec<(B256, &'a [u8])>> {
    let per_thread = elements.len().div_ceil(threads.max(1)).max(1);
    std::thread::scope(|scope| {
        let workers: Vec<_> = elements
            .chunks(per_thread)
            .map(|part| {
                scope.spawn(move || {
                    let none = MemoryTrieStore::new();
                    part.iter()
                        .map(|e| check_node(e, values, &none))
                        .collect::<Result<Vec<_>>>()
                })
            })
            .collect();
        let mut out = Vec::with_capacity(elements.len());
        for worker in workers {
            out.extend(
                worker
                    .join()
                    .map_err(|_| anyhow!("checking state nodes panicked"))??,
            );
        }
        Ok(out)
    })
}

/// rskj's `Trie.fromMessage`, then the key `TrieStoreImpl.save` gives it.
fn check_node<'a>(
    element: &'a [u8],
    values: &HashMap<B256, usize>,
    none: &MemoryTrieStore,
) -> Result<(B256, &'a [u8])> {
    let message = non_empty(data(element)?, "the nodes section holds an empty element")?;
    let node = TrieNode::try_from_message(message, none)
        .context("Bootstrap-data nodes section holds a node that does not parse")?;
    ensure!(
        node.to_message(none) == message,
        "Bootstrap-data nodes section holds a node not in the form rskj writes it"
    );
    long_values_held(&node, values)?;
    let key = if node.is_empty_trie() {
        empty_trie_hash()
    } else {
        keccak256(message)
    };
    Ok((key, message))
}

/// rskj saves a node's long value with it, and those of the nodes embedded
/// in it, reading each from the values already stored: one that is missing,
/// or of another length, fails the import (`BootstrapImporter.saveNode`).
fn long_values_held(node: &TrieNode, values: &HashMap<B256, usize>) -> Result<()> {
    if node.has_long_value() {
        let hash = node.value_hash.context("a long value without its hash")?;
        ensure!(
            values.get(&hash) == Some(&node.value_length()),
            "Failed to save a state node during bootstrap import: a referenced long value is \
             missing or inconsistent in the values section (incomplete or corrupt bootstrap data)"
        );
    }
    for child in [&node.left, &node.right] {
        if let NodeRef::Node(child) = child {
            long_values_held(child, values)?;
        }
    }
    Ok(())
}

/// A body has to be the one its header names, as snapshot sync checks the
/// blocks it fetches below its state: nothing executes these blocks, so
/// nothing else would.
fn check_body(block: &Block) -> Result<()> {
    let number = block.header.number;
    let root = block.header.transactions_root;
    ensure!(
        rustock_core::ordered_tx_trie_root(&block.transactions, true) == root
            || rustock_core::ordered_tx_trie_root(&block.transactions, false) == root,
        "block #{number}: its transactions are not the ones its header names"
    );
    ensure!(
        rustock_execution::processor::compute_ommers_hash(&block.ommers)
            == block.header.ommers_hash,
        "block #{number}: its uncles are not the ones its header names"
    );
    Ok(())
}

/// rskj's `new BlockDifficulty(new BigInteger(bytes))`: two's complement, so
/// a set top bit is a negative difficulty, which it refuses.
fn total_difficulty(bytes: &[u8]) -> Result<U256> {
    ensure!(
        bytes.first().is_none_or(|b| b & 0x80 == 0),
        "A block difficulty must be positive or zero"
    );
    let start = bytes.iter().position(|b| *b != 0).unwrap_or(bytes.len());
    let digits = &bytes[start..];
    ensure!(
        digits.len() <= 32,
        "a total difficulty of {} bytes",
        digits.len()
    );
    Ok(U256::from_be_slice(digits))
}

/// rskj's `RLPElement.getRLPData()` is `null` for an empty item, which every
/// reader of the copy refuses.
fn non_empty<'b>(bytes: &'b [u8], what: &str) -> Result<&'b [u8]> {
    ensure!(!bytes.is_empty(), "{what}; the file is corrupt");
    Ok(bytes)
}

/// One RLP item at the front of `buf`: whether it is a list, its payload, and
/// what follows it. Lenient about the form of a length, as rskj's reader is.
fn item(buf: &[u8]) -> Result<(bool, &[u8], &[u8])> {
    let first = *buf
        .first()
        .context("RLP: expected an item, found the end")?;
    let (list, start, len) = match first {
        0x00..=0x7f => (false, 0, 1),
        0x80..=0xb7 => (false, 1, usize::from(first - 0x80)),
        0xb8..=0xbf => {
            let n = usize::from(first - 0xb7);
            (
                false,
                1 + n,
                be_len(buf.get(1..1 + n).context("RLP: truncated length")?)?,
            )
        }
        0xc0..=0xf7 => (true, 1, usize::from(first - 0xc0)),
        _ => {
            let n = usize::from(first - 0xf7);
            (
                true,
                1 + n,
                be_len(buf.get(1..1 + n).context("RLP: truncated length")?)?,
            )
        }
    };
    let end = start
        .checked_add(len)
        .context("RLP: a length that overflows")?;
    ensure!(end <= buf.len(), "RLP: an item runs past its container");
    Ok((list, &buf[start..end], &buf[end..]))
}

fn be_len(bytes: &[u8]) -> Result<usize> {
    ensure!(bytes.len() <= 4, "RLP: a length of {} bytes", bytes.len());
    Ok(bytes
        .iter()
        .fold(0usize, |acc, b| (acc << 8) | usize::from(*b)))
}

/// The items `payload` holds, each as its whole encoding: rskj's `RLP.decode2`.
fn items(mut payload: &[u8]) -> Result<Vec<&[u8]>> {
    let mut out = Vec::new();
    while !payload.is_empty() {
        let (_, _, rest) = item(payload)?;
        out.push(&payload[..payload.len() - rest.len()]);
        payload = rest;
    }
    Ok(out)
}

/// rskj's `RLPElement.getRLPData()`: a string's payload, or a list's whole
/// encoding.
fn data(encoded: &[u8]) -> Result<&[u8]> {
    let (is_list, payload, _) = item(encoded)?;
    Ok(if is_list { encoded } else { payload })
}

/// rskj's `RLP.decodeList(bytes)`: `bytes` is exactly one list, whose items
/// these are.
fn decode_list(bytes: &[u8]) -> Result<Vec<&[u8]>> {
    let (is_list, payload, rest) = item(bytes)?;
    ensure!(is_list, "RLP: expected a list");
    ensure!(rest.is_empty(), "RLP: {} bytes after a list", rest.len());
    items(payload)
}

/// A reader that counts what it has given out.
struct Counted<R> {
    inner: R,
    read: u64,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        Ok(n)
    }
}

fn read_byte(input: &mut impl Read) -> Result<Option<u8>> {
    let mut byte = [0u8; 1];
    loop {
        match input.read(&mut byte) {
            Ok(0) => return Ok(None),
            Ok(_) => return Ok(Some(byte[0])),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
}

/// rskj's `BootstrapImporter.isV2`, then the reader for the format found.
/// `size` is what the zip says the file holds.
fn read_bootstrap(entry: impl Read, size: u64, loader: &mut Loader) -> Result<&'static str> {
    let mut input = Counted {
        inner: BufReader::with_capacity(1 << 20, entry),
        read: 0,
    };
    let first = read_byte(&mut input)?.context("Empty bootstrap data file")?;
    if first == V2_MAGIC[0] {
        let mut magic = [0u8; 7];
        input
            .read_exact(&mut magic)
            .context("Invalid bootstrap-data v2 magic")?;
        ensure!(magic == V2_MAGIC[1..], "Invalid bootstrap-data v2 magic");
        info!("Import: bootstrap-data v2 (chunked)");
        read_v2(&mut input, size, loader)?;
        Ok("v2")
    } else if first >= 0xc0 {
        info!("Import: bootstrap-data v1 (legacy)");
        let mut all = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
        all.push(first);
        input.read_to_end(&mut all)?;
        read_v1(&all, loader)?;
        Ok("v1")
    } else {
        bail!(
            "Unrecognized bootstrap-data format: first byte 0x{first:02X} is neither the v2 \
             magic ('R') nor a v1 RLP list prefix (0xc0+)"
        )
    }
}

/// rskj's `updateDatabaseV1`: one RLP list,
/// `[blocks, [nodes, values]]`, read whole.
fn read_v1(all: &[u8], loader: &mut Loader) -> Result<()> {
    let top = decode_list(all)?;
    ensure!(
        top.len() >= 2,
        "a v1 copy of {} part(s), not blocks and state",
        top.len()
    );
    for entry in decode_list(data(top[0])?)? {
        loader.block(non_empty(data(entry)?, "block data is missing")?)?;
    }
    let state = decode_list(data(top[1])?)?;
    ensure!(
        state.len() >= 2,
        "a v1 state of {} list(s), not nodes and values",
        state.len()
    );
    // The values first: the nodes are checked against them.
    loader.values(&decode_list(data(state[1])?)?)?;
    loader.nodes(&decode_list(data(state[0])?)?)
}

/// rskj's `updateDatabaseV2`: sections of length-prefixed chunks, read a
/// chunk at a time, with every structural check its `scanSections` makes.
fn read_v2<R: Read>(input: &mut Counted<R>, size: u64, loader: &mut Loader) -> Result<()> {
    let version = read_byte(input)?.context("the copy ends inside its header")?;
    ensure!(
        version == V2_VERSION,
        "Unsupported bootstrap-data v2 version: {version}"
    );
    let mut values_seen = false;
    loop {
        let tag = read_byte(input)?
            .context("Truncated bootstrap-data v2: missing end-of-sections marker")?;
        if tag == TAG_END {
            break;
        }
        if tag == TAG_VALUES {
            values_seen = true;
        }
        ensure!(
            tag != TAG_NODES || values_seen,
            "Bootstrap-data v2 sections are out of order: the nodes section appears before the \
             values section, but long values must be co-located before the nodes that reference them"
        );
        loop {
            let mut len = [0u8; 8];
            input
                .read_exact(&mut len)
                .context("the copy ends inside a section")?;
            let len = u64::from_be_bytes(len);
            if len == 0 {
                break;
            }
            ensure!(
                len <= MAX_CHUNK_LEN,
                "Bootstrap-data v2 chunk length out of range: {len}"
            );
            let remaining = size.saturating_sub(input.read);
            ensure!(
                len <= remaining,
                "Bootstrap-data v2 chunk length {len} exceeds the {remaining} bytes remaining in the \
                 file; the file is truncated or its length field is corrupt"
            );
            if matches!(tag, TAG_BLOCKS | TAG_VALUES | TAG_NODES) {
                let mut chunk = vec![0u8; usize::try_from(len)?];
                input
                    .read_exact(&mut chunk)
                    .context("the copy ends inside a chunk")?;
                loader.chunk(tag, &chunk)?;
            } else {
                // A section this reader does not know, as rskj skips one.
                let skipped = std::io::copy(&mut (&mut *input).take(len), &mut std::io::sink())?;
                ensure!(skipped == len, "the copy ends inside a chunk");
            }
        }
    }
    ensure!(
        read_byte(input)?.is_none(),
        "Bootstrap-data v2 has trailing bytes after the end-of-sections marker; the file is \
         corrupt or not canonical"
    );
    ensure!(
        !loader.blocks.is_empty(),
        "Bootstrap-data v2 has no blocks section (or it is empty); refusing to import incomplete data"
    );
    ensure!(
        loader.nodes > 0,
        "Bootstrap-data v2 has no state-nodes section (or it is empty); refusing to import a \
         stateless snapshot"
    );
    Ok(())
}

/// One entry of a zip's central directory.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ZipEntry {
    name: String,
    flags: u16,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    offset: u64,
}

const EOCD_SIG: u32 = 0x0605_4b50;
const ZIP64_LOCATOR_SIG: u32 = 0x0706_4b50;
const ZIP64_EOCD_SIG: u32 = 0x0606_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;

/// Little-endian fields, bounds-checked.
struct Fields<'b> {
    bytes: &'b [u8],
    at: usize,
}

impl<'b> Fields<'b> {
    fn new(bytes: &'b [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'b [u8]> {
        let end = self
            .at
            .checked_add(n)
            .context("zip: a length that overflows")?;
        let field = self
            .bytes
            .get(self.at..end)
            .context("zip: a record runs past its end")?;
        self.at = end;
        Ok(field)
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }
}

fn read_at(file: &mut File, at: u64, len: usize) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(at))?;
    let mut buf = vec![0; len];
    file.read_exact(&mut buf)
        .context("zip: the file ends early")?;
    Ok(buf)
}

/// The zip's central directory, zip64 included.
fn zip_entries(file: &mut File) -> Result<Vec<ZipEntry>> {
    let len = file.seek(SeekFrom::End(0))?;
    let tail_len = len.min(22 + 0xFFFF);
    let tail_at = len - tail_len;
    let tail = read_at(file, tail_at, usize::try_from(tail_len)?)?;
    let eocd = (0..=tail.len().checked_sub(22).context("not a zip file")?)
        .rev()
        .find(|&i| tail[i..i + 4] == EOCD_SIG.to_le_bytes())
        .context("not a zip file: no end of central directory")?;
    let mut end = Fields::new(&tail[eocd..]);
    end.take(10)?; // signature, this disk, the directory's disk, entries on this disk
    let mut count = u64::from(end.u16()?);
    let mut cd_size = u64::from(end.u32()?);
    let mut cd_offset = u64::from(end.u32()?);
    if count == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
        let eocd_at = tail_at + eocd as u64;
        let at = eocd_at.checked_sub(20).context("zip64: no locator")?;
        let locator = read_at(file, at, 20)?;
        let mut locator = Fields::new(&locator);
        ensure!(locator.u32()? == ZIP64_LOCATOR_SIG, "zip64: no locator");
        locator.u32()?;
        let record_at = locator.u64()?;
        let record = read_at(file, record_at, 56)?;
        let mut record = Fields::new(&record);
        ensure!(
            record.u32()? == ZIP64_EOCD_SIG,
            "zip64: no end of central directory"
        );
        record.take(28)?; // its size, versions, disks, entries on this disk
        count = record.u64()?;
        cd_size = record.u64()?;
        cd_offset = record.u64()?;
    }
    ensure!(
        cd_offset.checked_add(cd_size).is_some_and(|end| end <= len),
        "zip: the central directory runs past the file"
    );
    ensure!(
        cd_size <= 64 << 20,
        "zip: a central directory of {cd_size} bytes"
    );
    let directory = read_at(file, cd_offset, usize::try_from(cd_size)?)?;
    let mut directory = Fields::new(&directory);
    let mut entries = Vec::new();
    for _ in 0..count {
        ensure!(
            directory.u32()? == CENTRAL_SIG,
            "zip: a damaged central directory"
        );
        directory.take(4)?; // versions
        let flags = directory.u16()?;
        let method = directory.u16()?;
        directory.take(4)?; // time and date
        let crc = directory.u32()?;
        let mut compressed = u64::from(directory.u32()?);
        let mut size = u64::from(directory.u32()?);
        let name_len = usize::from(directory.u16()?);
        let extra_len = usize::from(directory.u16()?);
        let comment_len = usize::from(directory.u16()?);
        directory.take(8)?; // disk and attributes
        let mut offset = u64::from(directory.u32()?);
        let name = String::from_utf8_lossy(directory.take(name_len)?).into_owned();
        let mut extra = Fields::new(directory.take(extra_len)?);
        directory.take(comment_len)?;
        while extra.at + 4 <= extra.bytes.len() {
            let id = extra.u16()?;
            let field_len = usize::from(extra.u16()?);
            let mut field = Fields::new(extra.take(field_len)?);
            if id == 0x0001 {
                if size == 0xFFFF_FFFF {
                    size = field.u64()?;
                }
                if compressed == 0xFFFF_FFFF {
                    compressed = field.u64()?;
                }
                if offset == 0xFFFF_FFFF {
                    offset = field.u64()?;
                }
            }
        }
        entries.push(ZipEntry {
            name,
            flags,
            method,
            crc,
            compressed,
            size,
            offset,
        });
    }
    Ok(entries)
}

/// `bootstrap-data.bin`, once. rskj's `Unzipper` extracts every entry and
/// refuses one whose name would place it outside its directory.
fn bootstrap_entry(entries: &[ZipEntry]) -> Result<&ZipEntry> {
    for entry in entries {
        ensure!(
            !entry.name.contains("..")
                && !entry.name.starts_with(['/', '\\'])
                && !entry.name.contains(':'),
            "The source contains an illegal entry: {}",
            entry.name
        );
    }
    let mut found = entries.iter().filter(|e| e.name == BOOTSTRAP_ENTRY);
    let entry = found
        .next()
        .context("the copy holds no bootstrap-data.bin")?;
    ensure!(
        found.next().is_none(),
        "the copy holds bootstrap-data.bin twice"
    );
    Ok(entry)
}

type Stored = std::io::Take<BufReader<File>>;

enum Body {
    Stored(Stored),
    Deflated(flate2::bufread::DeflateDecoder<Stored>),
}

/// An entry's contents, held to its CRC-32 and sizes once read to the end,
/// as Java's `ZipInputStream` holds them.
struct EntryReader {
    body: Body,
    crc: flate2::Crc,
    read: u64,
    entry: ZipEntry,
    ended: bool,
}

fn open_entry(mut file: File, entry: &ZipEntry) -> Result<EntryReader> {
    ensure!(entry.flags & 1 == 0, "encrypted ZIP entry not supported");
    let local = read_at(&mut file, entry.offset, 30)?;
    let mut local = Fields::new(&local);
    ensure!(
        local.u32()? == LOCAL_SIG,
        "zip: no local header at {}",
        entry.offset
    );
    local.take(22)?;
    let name_len = u64::from(local.u16()?);
    let extra_len = u64::from(local.u16()?);
    file.seek(SeekFrom::Start(entry.offset + 30 + name_len + extra_len))?;
    let data = BufReader::with_capacity(1 << 20, file).take(entry.compressed);
    let body = match entry.method {
        0 => Body::Stored(data),
        8 => Body::Deflated(flate2::bufread::DeflateDecoder::new(data)),
        m => bail!("invalid compression method {m}"),
    };
    Ok(EntryReader {
        body,
        crc: flate2::Crc::new(),
        read: 0,
        entry: entry.clone(),
        ended: false,
    })
}

impl EntryReader {
    fn check(&self) -> Result<()> {
        if let Body::Deflated(d) = &self.body {
            ensure!(
                d.total_in() == self.entry.compressed,
                "invalid entry compressed size (expected {} but got {} bytes)",
                self.entry.compressed,
                d.total_in()
            );
        }
        ensure!(
            self.read == self.entry.size,
            "invalid entry size (expected {} but got {} bytes)",
            self.entry.size,
            self.read
        );
        ensure!(
            self.crc.sum() == self.entry.crc,
            "invalid entry CRC (expected 0x{:x} but got 0x{:x})",
            self.entry.crc,
            self.crc.sum()
        );
        Ok(())
    }
}

impl Read for EntryReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = match &mut self.body {
            Body::Stored(r) => r.read(buf)?,
            Body::Deflated(r) => r.read(buf)?,
        };
        if n == 0 && !buf.is_empty() && !self.ended {
            self.ended = true;
            self.check()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        }
        self.crc.update(&buf[..n]);
        self.read += n as u64;
        Ok(n)
    }
}

#[cfg(test)]
#[path = "bootstrap_import_tests.rs"]
mod tests;
