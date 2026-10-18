//! Loads or creates this device's long-term key pair. The private key is stored
//! encrypted with DPAPI, so only the same Windows user on the same machine can use it.

use std::fs;
use std::io;
use std::path::Path;

use moush_core::crypto::Identity;
use moush_core::util::write_atomic;

use crate::win::system::{protect, unprotect};

const MAGIC: &[u8; 4] = b"MSK1";

pub fn load_or_create(path: &Path) -> io::Result<Identity> {
    match fs::read(path) {
        Ok(data) => {
            if data.len() < 4 || &data[..4] != MAGIC {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} is not a Moush identity file", path.display()),
                ));
            }
            let plain = unprotect(&data[4..]).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!(
                        "cannot decrypt {} ({e}); it belongs to another Windows user or machine. \
                         Delete it to create a new identity (devices will need to pair again)",
                        path.display()
                    ),
                )
            })?;
            Identity::from_bytes(&plain)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let id = Identity::generate()?;
            let mut data = MAGIC.to_vec();
            data.extend(protect(&id.to_bytes())?);
            write_atomic(path, &data)?;
            log::info!("created device identity {}", id.fingerprint());
            Ok(id)
        }
        Err(e) => Err(e),
    }
}
