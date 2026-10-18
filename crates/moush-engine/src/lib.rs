//! Moush engine: Windows input capture and injection plus the host and client
//! orchestration shared by the desktop app and the command-line tool.

#[cfg(not(windows))]
compile_error!("moush-engine targets Windows; the platform-independent logic lives in moush-core");

pub mod client;
pub mod clipboard;
pub mod discovered;
pub mod features;
pub mod host;
pub mod identity;
pub mod layout;
pub mod logger;
pub mod paths;
pub mod transfer;
pub mod win;

pub use moush_core as core;
