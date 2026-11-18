//! rskj `org.ethereum.config.ConstantsTest`: the limits rskj keeps as
//! constants, where a block's run reads them.

use super::support::{creation, run_on_mainnet, LOVELL700};

/// rskj `maxInitcodeSizeTest`, from the rest of the class: from RSKIP438 a
/// creation's initcode may be 49,152 bytes (`Constants.getMaxInitCodeSize`),
/// and a transaction carrying one byte more is refused
/// (`TransactionExecutor.init`); before RSKIP438 none is.
#[test]
fn from_rskip438_a_creations_initcode_is_held_to_49152_bytes() {
    let at_limit = run_on_mainnet(LOVELL700, &creation(vec![0; 49_152])).unwrap();
    assert!(at_limit.success);
    assert!(run_on_mainnet(LOVELL700, &creation(vec![0; 49_153])).is_err());
    let before = run_on_mainnet(LOVELL700 - 1, &creation(vec![0; 49_153])).unwrap();
    assert!(before.success);
}
