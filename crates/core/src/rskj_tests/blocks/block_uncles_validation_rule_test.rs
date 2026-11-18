//! rskj `co.rsk.validators.BlockUnclesValidationRuleTest`: an uncle is a
//! block below the one that names it, never its sibling or descendant
//! (`BlockUnclesValidationRule.validateParentNumber`); and the rule's other
//! refusals, under rskj's limits of ten uncles and seven generations.

use super::support::header;
use crate::types::block::Block;
use crate::types::header::Header;
use crate::validation::uncles::{
    AncestorSource, UnclesValidationRule, UNCLE_GENERATION_LIMIT, UNCLE_LIST_LIMIT,
};
use crate::validation::{HeaderValidator, ParentHeaderValidator, ValidationError};
use alloy_primitives::{Address, B256};
use std::collections::HashMap;

/// The chain as a block store holds it: headers by hash, and the uncles
/// each block named.
#[derive(Default)]
struct Store(HashMap<B256, Header>, HashMap<B256, Vec<Header>>);

impl Store {
    fn put(&mut self, header: &Header) {
        self.0.insert(header.hash(), header.clone());
    }
}

impl AncestorSource for Store {
    fn header(&self, hash: B256) -> Option<Header> {
        self.0.get(&hash).cloned()
    }

    fn uncles_of(&self, hash: B256) -> Vec<Header> {
        self.1.get(&hash).cloned().unwrap_or_default()
    }
}

/// A child of `parent`, told apart from its siblings by its `miner`.
fn child_of(parent: &Header, miner: u8) -> Header {
    let mut child = header(parent.number + 1, parent.timestamp + 10);
    child.parent_hash = parent.hash();
    child.beneficiary = Address::repeat_byte(miner);
    child
}

/// rskj's chain: genesis and block 1 stored, and block 2, on block 1,
/// naming `uncle`. Uncle and generation limits of ten and no rule on the
/// uncle's own header, as rskj's test builds its rule.
fn judge(store: &Store, block_1: &Header, uncle: Header) -> Result<(), ValidationError> {
    let block = Block {
        header: child_of(block_1, 0xb0),
        transactions: Vec::new(),
        ommers: vec![uncle],
    };
    let header_rules: [Box<dyn HeaderValidator>; 0] = [];
    let parent_rules: [Box<dyn ParentHeaderValidator>; 0] = [];
    UnclesValidationRule {
        store,
        uncle_list_limit: 10,
        uncle_generation_limit: 10,
        header_rules: &header_rules,
        parent_rules: &parent_rules,
        fork_detection: None,
    }
    .validate(&block)
}

fn genesis_and_block_1() -> (Store, Header, Header) {
    let genesis = header(0, 1_000);
    let block_1 = child_of(&genesis, 0xa0);
    let mut store = Store::default();
    store.put(&genesis);
    store.put(&block_1);
    (store, genesis, block_1)
}

/// rskj `rejectBlockWithSiblingUncle`: block 2 can't name another child of
/// block 1, its sibling. A child of genesis, block 1's sibling, it can.
#[test]
fn a_sibling_of_the_block_is_no_uncle() {
    let (store, genesis, block_1) = genesis_and_block_1();
    let sibling = child_of(&block_1, 0xc0);
    assert_eq!(
        judge(&store, &block_1, sibling.clone()),
        Err(ValidationError::UncleIsSiblingOrDescendant {
            hash: sibling.hash()
        })
    );

    let parents_sibling = child_of(&genesis, 0xc1);
    assert_eq!(judge(&store, &block_1, parents_sibling), Ok(()));
}

/// rskj `rejectBlockWithUncleHavingHigherNumber`: block 2 can't name block 3,
/// a grandchild of block 1.
#[test]
fn a_block_above_the_one_naming_it_is_no_uncle() {
    let (store, _genesis, block_1) = genesis_and_block_1();
    let sibling = child_of(&block_1, 0xc0);
    let higher = child_of(&sibling, 0xc2);
    assert_eq!(higher.number, 3);
    assert_eq!(
        judge(&store, &block_1, higher.clone()),
        Err(ValidationError::UncleIsSiblingOrDescendant {
            hash: higher.hash()
        })
    );
}

/// Genesis and blocks 1 to `top`, each the child of the one before.
fn trunk(top: u64) -> (Store, Vec<Header>) {
    let mut store = Store::default();
    let mut chain = vec![header(0, 1_000)];
    store.put(&chain[0]);
    for _ in 1..=top {
        let next = child_of(chain.last().unwrap(), 0xa0);
        store.put(&next);
        chain.push(next);
    }
    (store, chain)
}

/// The block on `chain`'s last naming `uncles`, judged under rskj's limits.
fn names(store: &Store, chain: &[Header], uncles: Vec<Header>) -> Result<(), ValidationError> {
    let block = Block {
        header: child_of(chain.last().unwrap(), 0xb0),
        transactions: Vec::new(),
        ommers: uncles,
    };
    let header_rules: [Box<dyn HeaderValidator>; 0] = [];
    let parent_rules: [Box<dyn ParentHeaderValidator>; 0] = [];
    UnclesValidationRule {
        store,
        uncle_list_limit: UNCLE_LIST_LIMIT,
        uncle_generation_limit: UNCLE_GENERATION_LIMIT,
        header_rules: &header_rules,
        parent_rules: &parent_rules,
        fork_detection: None,
    }
    .validate(&block)
}

/// `validateParentNumber`: an uncle's parent must be no further than seven
/// blocks below the block naming it. Block 10 may name a child of block 3,
/// and not one of block 2.
#[test]
fn an_uncle_more_than_seven_generations_back_is_too_old() {
    let (store, chain) = trunk(9);
    assert_eq!(
        names(&store, &chain, vec![child_of(&chain[3], 0xc0)]),
        Ok(())
    );
    let old = child_of(&chain[2], 0xc0);
    assert_eq!(
        names(&store, &chain, vec![old.clone()]),
        Err(ValidationError::UncleTooOld {
            hash: old.hash(),
            limit: 7
        })
    );
}

/// `validateUncleList`: ten uncles at most.
#[test]
fn more_than_ten_uncles_are_too_many() {
    let (store, chain) = trunk(3);
    let uncles = |count: u8| (0..count).map(|miner| child_of(&chain[2], miner)).collect();
    assert_eq!(names(&store, &chain, uncles(10)), Ok(()));
    assert_eq!(
        names(&store, &chain, uncles(11)),
        Err(ValidationError::TooManyUncles { max: 10, got: 11 })
    );
}

/// `validateUncleList`: no uncle twice in one block.
#[test]
fn an_uncle_named_twice_is_refused() {
    let (store, chain) = trunk(3);
    let uncle = child_of(&chain[2], 0xc0);
    assert_eq!(
        names(&store, &chain, vec![uncle.clone(), uncle.clone()]),
        Err(ValidationError::UncleRepeated { hash: uncle.hash() })
    );
}

/// `validateUnclesAncestors`: a block's own ancestor is no uncle of it.
#[test]
fn an_ancestor_is_no_uncle() {
    let (store, chain) = trunk(3);
    assert_eq!(
        names(&store, &chain, vec![chain[2].clone()]),
        Err(ValidationError::UncleIsAncestor {
            hash: chain[2].hash()
        })
    );
}

/// `validateIfUncleWasNeverUsed`: an uncle one of the last seven ancestors
/// named already is used, and naming it again is refused.
#[test]
fn an_uncle_an_ancestor_named_is_used() {
    let (mut store, chain) = trunk(3);
    let uncle = child_of(&chain[1], 0xc0);
    store.1.insert(chain[3].hash(), vec![uncle.clone()]);
    assert_eq!(
        names(&store, &chain, vec![uncle.clone()]),
        Err(ValidationError::UncleAlreadyUsed { hash: uncle.hash() })
    );
}

/// `validateUncleParent`: an uncle's parent is one of the block's ancestors.
/// A child of a block the store doesn't hold, or of one off the block's
/// chain, has none in common with it.
#[test]
fn an_uncle_whose_parent_is_no_ancestor_has_no_common_parent() {
    let (mut store, chain) = trunk(3);
    let stranger = child_of(&header(1, 2_000), 0xd0);
    let off_chain = child_of(&chain[0], 0xe0);
    store.put(&off_chain);
    let nephew = child_of(&off_chain, 0xe1);
    for uncle in [stranger, nephew] {
        assert_eq!(
            names(&store, &chain, vec![uncle.clone()]),
            Err(ValidationError::UncleHasNoCommonParent { hash: uncle.hash() })
        );
    }
}

/// rskj `FamilyUtils.getAncestors` and `getUsedUncles`: for block 10, the
/// ancestors from its parent down to block 3, ten less seven, and the
/// uncles those named; block 2 and its uncles are out of reach.
#[test]
fn the_family_reaches_seven_blocks_back() {
    use crate::validation::uncles::{ancestors, used_uncles};
    let (mut store, chain) = trunk(9);
    let named_by = |at: usize| child_of(&chain[at - 1], 0xc0 + at as u8);
    store.1.insert(chain[3].hash(), vec![named_by(3)]);
    store.1.insert(chain[2].hash(), vec![named_by(2)]);
    let family = ancestors(&store, 10, chain[9].hash(), UNCLE_GENERATION_LIMIT);
    assert!(family.contains(&chain[3].hash()) && !family.contains(&chain[2].hash()));
    let used = used_uncles(&store, 10, chain[9].hash(), UNCLE_GENERATION_LIMIT);
    assert!(used.contains(&named_by(3).hash()));
    assert!(!used.contains(&named_by(2).hash()));
}
