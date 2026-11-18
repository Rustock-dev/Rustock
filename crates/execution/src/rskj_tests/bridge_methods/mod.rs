//! The Bridge called through its methods (rskj's `BridgeTest` and `BridgeIT`),
//! Flyover (rskj's `BridgeSupportFlyoverTest`), the retiring federation's
//! funds migration (rskj's `BridgeSupportProcessFundsMigrationTest`), and the
//! Bitcoin store seeded from its checkpoint (rskj's `BridgeSupportIT`).

mod bridge_it;
mod bridge_support_flyover_test;
mod bridge_support_it;
mod bridge_support_process_funds_migration_test;
mod bridge_test;
mod support;
