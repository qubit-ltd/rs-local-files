//! Deterministic, instance-local fault injection for tests.

mod test_fault_plan;
mod test_fault_point;

pub use test_fault_plan::TestFaultPlan;
pub use test_fault_point::TestFaultPoint;
