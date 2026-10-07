//! Persistent schedules with host-supplied Agent execution, independent of legacy Ait scheduling.

mod cadence;
mod engine;
pub mod ports;
pub mod protocol;
pub mod service;
pub mod storage;

pub mod capabilities;
pub mod dispatch;

/// Complete service installed as one capability component.
pub use service::Schedules as Service;
