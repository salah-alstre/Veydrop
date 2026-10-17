//! Veydrop core: the local transfer engine (HTTP server, pairing, transfers,
//! history, settings) with no dependency on any UI toolkit.

pub mod auth;
pub mod bus;
pub mod diag;
pub mod engine;
pub mod error;
pub mod fsutil;
pub mod history;
pub mod incoming;
pub mod model;
pub mod net;
pub mod partials;
pub mod qr;
pub mod server;
pub mod settings;
pub mod shares;
pub mod transfers;
pub mod zipstream;

pub use error::{Error, Result};
