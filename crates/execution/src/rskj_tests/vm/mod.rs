//! The VM: opcodes, programs, and a transaction's execution (rskj's `co.rsk.vm`
//! and `org.ethereum.vm`).

mod contract_creating_dsl_rollback_test;
mod create2_test;
mod dsl_files_test;
mod evm_rules;
mod ext_code_hash_test;
pub(super) mod harness;
mod initcode_cost_calculator_test;
mod mcopy_test;
mod nested_calls_stack_depth_test;
mod rskip544_test;
mod transaction_executor_init_code_size_dsl_test;
mod transient_storage_dsl_test;
mod transient_storage_test;
mod vm_dsl_test;
mod vm_execution_test;
mod vm_test;
