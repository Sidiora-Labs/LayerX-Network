//! The sole versioned protocol boundary between clients and the `LayerX` core.

pub mod abi;
pub mod capabilities;
pub mod framing;
pub mod handshake;
pub mod preparation;
pub mod refusal;
pub mod report;
pub mod schema;
pub mod simulate;
pub mod transport;

pub use capabilities::Capabilities;
pub use handshake::NodeInfo;
pub use preparation::{PreparationState, PreparationStateError};
pub use report::capability_report;
pub use simulate::{SimulateError, SimulatedExecution, Simulation, SimulationEvidence};
