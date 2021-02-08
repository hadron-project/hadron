//! CRC application commands.
//!
//! The CRC is responsible for controlling the state of the Hadron cluster as a whole, and exposes
//! a control signal to the application in order to drive the system. That logic is encapsulated here.

use crate::models::{Pipeline, Stream};

/// A command signal coming from the CRC.
pub enum CRCCommand {
    /// Create a new Stream Partition Controller (SPC).
    CreateSPC(SPCSpec),
    /// Create a new Stream Consumer Controller (SCC).
    CreateSCC(SCCSpec),
    /// Create a new Pipeline Controller (PLC).
    CreatePLC(PLCSpec),
}

/// A spec for building a Stream Partition Controller (SPC).
pub struct SPCSpec {
    /// The stream's data model.
    stream: Stream,
    /// The partition number for this controller.
    partition: u8,
}

/// A spec for building a Stream Consumer Controller (SCC).
pub struct SCCSpec {
    /// The stream's data model.
    stream: Stream,
}

/// A spec for building a Pipeline Controller (PLC).
pub struct PLCSpec {
    /// The pipeline's data model.
    pipeline: Pipeline,
}
