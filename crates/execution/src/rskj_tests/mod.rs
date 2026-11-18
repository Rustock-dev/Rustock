//! Tests ported from rskj's own test suite.
//!
//! An rskj test names a behaviour rskj holds itself to, and many of them are of
//! cases no block on either network has reached: a refusal, a limit, an
//! overflow, the two sides of an activation height. Syncing the chain cannot
//! show whether rustock has such a case right. A test can.
//!
//! Each module ports one rskj test class from the release VETIVER-9.0.4
//! (`rskj-core/src/test/java/`) and is named after it; each test asserts what
//! the rskj test asserts, in rustock's own terms and without rskj's mocks.
//! `docs/rskj-test-parity.md` keeps the table: each rskj test, and the rustock
//! test that covers it.
//!
//! The folders follow the code a test answers for.

mod blocks;
mod bridge_federation;
mod bridge_federation_state;
mod bridge_methods;
mod bridge_pegins;
mod bridge_pegouts;
mod precompiles;
mod remasc;
mod vm;
