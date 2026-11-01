//! Mister Download Manager download engine. See notes/ENGINE.md.

pub mod disk;
mod dns;
pub mod download;
pub mod ftp;
pub mod hosts;
pub mod limiter;
pub mod naming;
pub mod pac;
pub mod planner;
mod sparse;
pub mod state;
pub mod verify;

pub use download::{
    Destination, Download, Error, Event, NetKind, Options, Outcome, ProbeInfo, Proxy, Request,
    Snapshot, fetch, probe, supported,
};
