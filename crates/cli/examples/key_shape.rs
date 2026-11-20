//! Key and value shape of a trie store: are keys fixed-length, and what do
//! the value sizes look like? Decides whether a static map can drop per-entry
//! pointers in favour of a fixed stride.
use rocksdb::{IteratorMode, Options, DB};
use std::collections::BTreeMap;
fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: key_shape <dir>");
    let mut o = Options::default(); o.create_if_missing(false);
    let db = DB::open_cf_for_read_only(&o, &dir, vec!["trie_nodes"], false)?;
    let cf = db.cf_handle("trie_nodes").unwrap();
    let mut klen: BTreeMap<usize, u64> = BTreeMap::new();
    let mut vlen: BTreeMap<usize, u64> = BTreeMap::new();
    let (mut n, mut vtot, mut sorted, mut prev) = (0u64, 0u64, true, Vec::<u8>::new());
    for item in db.iterator_cf(cf, IteratorMode::Start) {
        let (k, v) = item?;
        *klen.entry(k.len()).or_default() += 1;
        *vlen.entry(v.len() / 32 * 32).or_default() += 1;
        vtot += v.len() as u64; n += 1;
        if !prev.is_empty() && k.as_ref() < prev.as_slice() { sorted = false; }
        prev = k.to_vec();
    }
    println!("entries        {n}");
    println!("iter sorted    {sorted}");
    println!("key lengths    {:?}", klen);
    println!("value total    {:.0} MB  (mean {:.0} B)", vtot as f64/1048576.0, vtot as f64/n as f64);
    println!("value len hist (bucketed by 32B) {:?}", vlen.iter().take(8).collect::<Vec<_>>());
    Ok(())
}
