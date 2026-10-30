//! labserver: an HTTP Range server with a request log and fault injection.
//! Used in-process by Mister Download Manager's tests and standalone for the behaviour lab (notes/BEHAVIOR-LAB.md).

pub mod content;
pub mod faults;
pub mod ftp;
mod server;

pub use faults::Faults;
pub use server::{Config, LabServer, LogEntry, LogSink};
