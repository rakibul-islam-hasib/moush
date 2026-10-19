//! Device identity and the encrypted, mutually authenticated channel.
//!
//! Every installation has a long-term X25519 key pair. Connections run the Noise `XX`
//! handshake (`Noise_XX_25519_ChaChaPoly_BLAKE2s`), which encrypts all traffic, proves
//! that each side holds its private key, and reveals each side's static public key.
//! Whether to *trust* that key is decided above this layer: a known key is accepted
//! automatically, an unknown one needs the user to compare a 6-digit code derived from
//! the handshake hash on both screens and approve the pairing.
//!
//! After the handshake each direction keeps its own nonce counter, so the reader and the
//! writer of a connection can live on different threads without locking.

use std::io::{self, Read, Write};
use std::sync::Arc;

use blake2::{Blake2s256, Digest};
use snow::{Builder, HandshakeState, StatelessTransportState};

const NOISE_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"Moush/1";
/// Largest Noise message.
const MAX_NOISE_MSG: usize = 65535;
const TAG_LEN: usize = 16;
/// Largest plaintext carried by one frame.
pub const MAX_FRAME_PLAINTEXT: usize = MAX_NOISE_MSG - TAG_LEN;

/// This device's long-term key pair.
#[derive(Clone)]
pub struct Identity {
    private: [u8; 32],
    public: [u8; 32],
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity").field("public", &hex(&self.public)).finish_non_exhaustive()
    }
}

impl Identity {
    pub fn generate() -> io::Result<Self> {
        let kp = builder()?.generate_keypair().map_err(noise_err)?;
        Ok(Self {
            private: kp.private.as_slice().try_into().map_err(|_| invalid("bad private key length"))?,
            public: kp.public.as_slice().try_into().map_err(|_| invalid("bad public key length"))?,
        })
    }

    /// Serialised form: 32 private bytes followed by 32 public bytes.
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut out = [0u8; 64];
        out[..32].copy_from_slice(&self.private);
        out[32..].copy_from_slice(&self.public);
        out
    }

    pub fn from_bytes(b: &[u8]) -> io::Result<Self> {
        if b.len() != 64 {
            return Err(invalid("identity must be 64 bytes"));
        }
        Ok(Self { private: b[..32].try_into().unwrap(), public: b[32..].try_into().unwrap() })
    }

    pub fn public_key(&self) -> &[u8; 32] {
        &self.public
    }

    pub fn device_id(&self) -> String {
        device_id(&self.public)
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(&self.public)
    }
}

/// Short stable identifier of a public key (16 hex characters).
pub fn device_id(public: &[u8]) -> String {
    hex(&Blake2s256::digest(public)[..8])
}

/// Human-readable form of the device id: `a1b2-c3d4-e5f6-a7b8`.
pub fn fingerprint(public: &[u8]) -> String {
    let id = device_id(public);
    id.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join("-")
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// An established secure channel, ready to be split into a reader and a writer.
pub struct SecureChannel {
    transport: Arc<StatelessTransportState>,
    remote_static: [u8; 32],
    handshake_hash: Vec<u8>,
}

impl SecureChannel {
    pub fn remote_static(&self) -> &[u8; 32] {
        &self.remote_static
    }

    /// The 6-digit code both users compare when pairing. An attacker in the middle runs
    /// two separate handshakes and ends up with different codes on the two screens.
    pub fn verification_code(&self) -> String {
        let d = Blake2s256::new().chain_update(b"pairing-code").chain_update(&self.handshake_hash).finalize();
        let n = u32::from_le_bytes(d[..4].try_into().unwrap()) % 1_000_000;
        format!("{n:06}")
    }

    pub fn split<R: Read, W: Write>(self, r: R, w: W) -> (FrameReader<R>, FrameWriter<W>) {
        (
            FrameReader {
                r,
                t: self.transport.clone(),
                nonce: 0,
                ct: vec![0; MAX_NOISE_MSG],
                pt: vec![0; MAX_NOISE_MSG],
            },
            FrameWriter { w, t: self.transport, nonce: 0, buf: Vec::with_capacity(512) },
        )
    }
}

fn builder() -> io::Result<Builder<'static>> {
    let params = NOISE_PARAMS.parse().map_err(noise_err)?;
    Ok(Builder::new(params))
}

/// Runs the Noise XX handshake. The caller sets socket timeouts beforehand.
pub fn handshake<S: Read + Write>(stream: &mut S, identity: &Identity, initiator: bool) -> io::Result<SecureChannel> {
    let b =
        builder()?.prologue(PROLOGUE).map_err(noise_err)?.local_private_key(&identity.private).map_err(noise_err)?;
    let mut hs: HandshakeState =
        if initiator { b.build_initiator() } else { b.build_responder() }.map_err(noise_err)?;
    let mut msg = vec![0u8; MAX_NOISE_MSG];
    let mut payload = vec![0u8; MAX_NOISE_MSG];
    // XX: -> e ; <- e, ee, s, es ; -> s, se
    let mut my_turn = initiator;
    while !hs.is_handshake_finished() {
        if my_turn {
            let n = hs.write_message(&[], &mut msg).map_err(noise_err)?;
            write_raw_frame(stream, &msg[..n])?;
        } else {
            let n = read_raw_frame(stream, &mut msg)?;
            hs.read_message(&msg[..n], &mut payload).map_err(noise_err)?;
        }
        my_turn = !my_turn;
    }
    stream.flush()?;
    let remote_static: [u8; 32] = hs
        .get_remote_static()
        .ok_or_else(|| invalid("peer sent no static key"))?
        .try_into()
        .map_err(|_| invalid("bad peer static key"))?;
    let handshake_hash = hs.get_handshake_hash().to_vec();
    let transport = hs.into_stateless_transport_mode().map_err(noise_err)?;
    Ok(SecureChannel { transport: Arc::new(transport), remote_static, handshake_hash })
}

fn write_raw_frame<W: Write>(w: &mut W, data: &[u8]) -> io::Result<()> {
    let mut buf = Vec::with_capacity(2 + data.len());
    buf.extend_from_slice(&(data.len() as u16).to_le_bytes());
    buf.extend_from_slice(data);
    w.write_all(&buf)
}

fn read_raw_frame<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut len = [0u8; 2];
    r.read_exact(&mut len)?;
    let n = u16::from_le_bytes(len) as usize;
    if n == 0 {
        return Err(invalid("empty frame"));
    }
    r.read_exact(&mut buf[..n])?;
    Ok(n)
}

/// Encrypting half of a channel.
pub struct FrameWriter<W: Write> {
    w: W,
    t: Arc<StatelessTransportState>,
    nonce: u64,
    buf: Vec<u8>,
}

impl<W: Write> FrameWriter<W> {
    /// Encrypts and writes `plaintext` as one or more frames with a single `write` call
    /// per frame. Returns the number of bytes written to the socket.
    pub fn write_frame(&mut self, plaintext: &[u8]) -> io::Result<usize> {
        let mut total = 0;
        for chunk in plaintext.chunks(MAX_FRAME_PLAINTEXT) {
            self.buf.clear();
            self.buf.resize(2 + chunk.len() + TAG_LEN, 0);
            let n = self.t.write_message(self.nonce, chunk, &mut self.buf[2..]).map_err(noise_err)?;
            self.nonce += 1;
            self.buf[..2].copy_from_slice(&(n as u16).to_le_bytes());
            self.w.write_all(&self.buf[..2 + n])?;
            total += 2 + n;
        }
        Ok(total)
    }

    pub fn get_ref(&self) -> &W {
        &self.w
    }
}

/// Decrypting half of a channel.
pub struct FrameReader<R: Read> {
    r: R,
    t: Arc<StatelessTransportState>,
    nonce: u64,
    ct: Vec<u8>,
    pt: Vec<u8>,
}

impl<R: Read> FrameReader<R> {
    /// Blocks until a full frame arrives and returns its plaintext. Any tampering,
    /// replay or reordering fails authentication and returns an error.
    pub fn read_frame(&mut self) -> io::Result<(&[u8], usize)> {
        let n = read_raw_frame(&mut self.r, &mut self.ct)?;
        let m = self.t.read_message(self.nonce, &self.ct[..n], &mut self.pt).map_err(noise_err)?;
        self.nonce += 1;
        Ok((&self.pt[..m], n + 2))
    }

    pub fn get_ref(&self) -> &R {
        &self.r
    }
}

fn noise_err(e: snow::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("secure channel: {e}"))
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    #[test]
    fn handshake_authenticates_and_encrypts() {
        let host_id = Identity::generate().unwrap();
        let client_id = Identity::generate().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let host_pub = *host_id.public_key();
        let client_pub = *client_id.public_key();

        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let ch = handshake(&mut s, &host_id, false).unwrap();
            assert_eq!(ch.remote_static(), &client_pub);
            let code = ch.verification_code();
            let (mut r, mut w) = ch.split(s.try_clone().unwrap(), s);
            let (msg, _) = r.read_frame().unwrap();
            assert_eq!(msg, b"hello host");
            w.write_frame(b"hello client").unwrap();
            code
        });

        let mut s = TcpStream::connect(addr).unwrap();
        let ch = handshake(&mut s, &client_id, true).unwrap();
        assert_eq!(ch.remote_static(), &host_pub);
        let code = ch.verification_code();
        let (mut r, mut w) = ch.split(s.try_clone().unwrap(), s);
        w.write_frame(b"hello host").unwrap();
        assert_eq!(r.read_frame().unwrap().0, b"hello client");
        assert_eq!(server.join().unwrap(), code, "both sides derive the same pairing code");
        assert_eq!(code.len(), 6);
    }

    #[test]
    fn tampered_frame_is_rejected() {
        let a = Identity::generate().unwrap();
        let b = Identity::generate().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let ch = handshake(&mut s, &a, false).unwrap();
            // Write a frame, then corrupt its ciphertext on the way out.
            let mut buf = Vec::new();
            let (_, mut w) = ch.split(io::empty(), &mut buf);
            w.write_frame(b"press A").unwrap();
            buf[5] ^= 0x40;
            s.write_all(&buf).unwrap();
            s
        });
        let mut s = TcpStream::connect(addr).unwrap();
        let ch = handshake(&mut s, &b, true).unwrap();
        let (mut r, _) = ch.split(s.try_clone().unwrap(), io::sink());
        assert!(r.read_frame().is_err());
        drop(server.join());
    }

    #[test]
    fn identity_roundtrip_and_fingerprint() {
        let id = Identity::generate().unwrap();
        let back = Identity::from_bytes(&id.to_bytes()).unwrap();
        assert_eq!(back.public_key(), id.public_key());
        assert_eq!(id.device_id().len(), 16);
        assert_eq!(id.fingerprint().len(), 19);
        assert_eq!(unhex(&hex(&[1, 2, 254])).unwrap(), vec![1, 2, 254]);
        assert!(unhex("zz").is_none());
    }
}
