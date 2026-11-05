//! Mister Download Manager download engine. See notes/ENGINE.md.

pub mod disk;
pub mod dns;
pub mod download;
pub mod ftp;
pub mod hosts;
mod html;
pub mod limiter;
pub mod naming;
pub mod net;
pub mod pac;
pub mod planner;
pub mod sites;
mod sparse;
pub mod state;
pub mod verify;

pub use dns::{Dns, DnsChoice, DnsError};
pub use download::{
    Destination, Download, Error, Event, FileHook, KeptAnswer, NetKind, Options, Outcome,
    ProbeInfo, Proxy, Request, Snapshot, fetch, probe, probe_kept, supported,
};
pub use net::{NetError, Route};
pub use sites::Refusal;
