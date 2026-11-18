//! rskj's `WhitelistStorageProviderImplTest`
//! (`co/rsk/peg/whitelist/WhitelistStorageProviderImplTest.java`): the lock
//! whitelist as the Bridge stores it, its one-off entries in one cell and,
//! from RSKIP87, its unlimited entries in another.
//!
//! Read in full. Its other tests are of rskj's in-memory cache of the
//! whitelist and of a provider that was never read writing nothing, which
//! `storage::LockWhitelist` has no counterpart of: it is written back only by
//! the call that read it.

use crate::bridge::storage::{self, LockWhitelist};

use super::support::{ctx_at, COIN};

/// The hash of rskj's `firstBtcAddress`.
const FIRST: [u8; 20] = [0x11; 20];
/// The hash of rskj's `secondBtcAddress`.
const SECOND: [u8; 20] = [0x22; 20];

/// rskj `save_whenLockWhiteListIsNotNullAndRSKIP87IsNotActive_shouldReturnSavedValue`:
/// before RSKIP87 a one-off entry put in the whitelist read from an empty
/// store is saved in the one-off cell and read back, and the unlimited cell
/// is neither written nor read.
#[test]
fn before_rskip87_a_one_off_entry_is_saved_and_the_unlimited_cell_is_not() {
    let mut ctx = ctx_at(1);
    let mut whitelist = LockWhitelist::load(&mut ctx, false);
    assert!(whitelist.one_off.is_empty() && whitelist.unlimited.is_empty());
    whitelist.one_off.push((FIRST, COIN));
    whitelist.save(&mut ctx);

    let read = LockWhitelist::load(&mut ctx, false);
    assert_eq!(read.one_off, vec![(FIRST, COIN)]);
    assert!(read.unlimited.is_empty());
    assert!(read.contains(&FIRST));
    assert_eq!(
        storage::load_one_off_whitelist(&mut ctx).0,
        vec![(FIRST, COIN)]
    );
    assert!(
        storage::bridge_load_bytes_named(&mut ctx, storage::UNLIMITED_LOCK_WHITELIST_KEY)
            .is_empty()
    );
}

/// rskj `save_whenIsActiveRSKIP87_shouldSavedUnlimitedLockWhitelist`: from
/// RSKIP87 a one-off and an unlimited entry are each saved in its own cell,
/// and both are read back.
#[test]
fn from_rskip87_a_one_off_and_an_unlimited_entry_are_saved_each_in_its_cell() {
    let mut ctx = ctx_at(1);
    let mut whitelist = LockWhitelist::load(&mut ctx, true);
    assert!(whitelist.one_off.is_empty() && whitelist.unlimited.is_empty());
    whitelist.one_off.push((FIRST, COIN));
    whitelist.unlimited.push(SECOND);
    whitelist.save(&mut ctx);

    let read = LockWhitelist::load(&mut ctx, true);
    assert!(read.contains(&FIRST) && read.contains(&SECOND));
    assert_eq!(read.one_off.len() + read.unlimited.len(), 2);
    assert_eq!(
        storage::load_one_off_whitelist(&mut ctx).0,
        vec![(FIRST, COIN)]
    );
    assert_eq!(storage::load_unlimited_whitelist(&mut ctx), vec![SECOND]);
}

/// rskj `getWhitelist_whenIsActiveRSKIP87_shouldReturnUnlimitedEntries`: from
/// RSKIP87 the whitelist read from the store holds both cells' entries, the
/// one-off one written with a disable height of 100.
#[test]
fn from_rskip87_the_whitelist_read_holds_both_cells_entries() {
    let mut ctx = ctx_at(1);
    storage::store_one_off_whitelist(&mut ctx, &[(FIRST, COIN)], 100);
    storage::store_unlimited_whitelist(&mut ctx, &[SECOND]);

    let read = LockWhitelist::load(&mut ctx, true);
    assert_eq!(read.one_off, vec![(FIRST, COIN)]);
    assert_eq!(read.unlimited, vec![SECOND]);
    assert_eq!(read.disable_height, 100);
    assert!(read.contains(&FIRST) && read.contains(&SECOND));

    // Before RSKIP87 the same store gives the one-off entry alone.
    let before = LockWhitelist::load(&mut ctx, false);
    assert!(before.contains(&FIRST) && !before.contains(&SECOND));
}
