//! Mister Download Manager download engine. See notes/ENGINE.md.

pub mod disk;
mod dns;
pub mod download;
pub mod ftp;
pub mod hosts;
mod html;
pub mod limiter;
pub mod naming;
pub mod pac;
pub mod planner;
pub mod sites;
mod sparse;
pub mod state;
pub mod verify;

pub use download::{
    Destination, Download, Error, Event, FileHook, KeptAnswer, NetKind, Options, Outcome,
    ProbeInfo, Proxy, Request, Snapshot, fetch, probe, probe_kept, supported,
};
pub use sites::Refusal;
