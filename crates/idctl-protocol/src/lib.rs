//! Audient iD mk3 wire protocol. Pure encode/decode; no I/O.

pub mod changes;
pub mod codec;
pub mod params;
pub mod request;

pub use changes::parse_changes;
pub use request::{Direction, ParamAddr, Request};
