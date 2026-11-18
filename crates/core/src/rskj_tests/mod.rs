//! Tests ported from rskj's own test suite: headers, the rules a block is held
//! to, and transactions.
//!
//! Each module ports one rskj test class from the release VETIVER-9.0.4
//! (`rskj-core/src/test/java/`) and is named after it; each test asserts what
//! the rskj test asserts, in rustock's own terms. What the tree is for is said
//! in `rustock-execution`'s `rskj_tests`.

mod blocks;
