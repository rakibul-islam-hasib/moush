//! Moush core: everything that does not touch the operating system.
//!
//! * [`protocol`] – compact binary wire format.
//! * [`crypto`] – device identity and the Noise-encrypted channel.
//! * [`geometry`] – layout of computers and monitors, edge crossing rules.
//! * [`router`] – host-side state machine deciding where each input event goes.
//! * [`inject`] – client-side application of input with stuck-key protection.
//! * [`net`] – host server and client sessions over TCP.
//! * [`discovery`] – LAN discovery over UDP broadcast.
//! * [`trust`], [`pairing`] – paired devices and pending pairing prompts.
//! * [`features`], [`flow`] – optional features (clipboard, files) and their flow control.
//! * [`clipboard`] – clipboard sync and text sharing: clip model, wire format, sync rules.

pub mod clipboard;
pub mod crypto;
pub mod discovery;
pub mod features;
pub mod flow;
pub mod geometry;
pub mod inject;
pub mod keys;
pub mod net;
pub mod pairing;
pub mod protocol;
pub mod router;
pub mod stats;
pub mod trust;
pub mod util;
