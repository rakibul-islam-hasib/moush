//! CPU-bound throughput of the transport (encode + encrypt, decrypt + decode) without any
//! network or scheduling in the way. Used to compare optimisation levels: the latency
//! benchmarks are dominated by thread wake-ups and barely move with the compiler settings,
//! this one shows what the crypto code costs.
//!
//!   cargo test --release -p moush-core --test throughput -- --ignored --nocapture

use std::io::{self, Cursor};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::thread;
use std::time::Instant;

use moush_core::crypto::{Identity, MAX_FRAME_PLAINTEXT, handshake};
use moush_core::protocol::{MAX_FEATURE_BODY, Message, decode_all, encode};

#[test]
#[ignore = "benchmark; run explicitly with --ignored"]
fn transport_throughput() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        handshake(&mut s, &Identity::generate().unwrap(), false).unwrap()
    });
    let mut c = TcpStream::connect(addr).unwrap();
    let sending = handshake(&mut c, &Identity::generate().unwrap(), true).unwrap();
    let receiving = server.join().unwrap();

    // 2048 bulk chunks of 32 KiB (64 MiB), each in its own frame as the writer sends them.
    const CHUNKS: usize = 2048;
    let body = vec![0xA5u8; MAX_FEATURE_BODY];
    let (_, mut fw) = sending.split(io::empty(), Vec::with_capacity(CHUNKS * (MAX_FEATURE_BODY + 64)));
    let mut plain = Vec::with_capacity(MAX_FRAME_PLAINTEXT);
    let t = Instant::now();
    for _ in 0..CHUNKS {
        plain.clear();
        encode(&Message::Feature { channel: 2, body: body.clone() }, &mut plain);
        fw.write_frame(&plain).unwrap();
    }
    let enc = t.elapsed();
    let wire = fw.get_ref().clone();

    let (mut fr, _) = receiving.split(Cursor::new(wire), io::sink());
    let mut msgs = Vec::new();
    let t = Instant::now();
    for _ in 0..CHUNKS {
        let (p, _) = fr.read_frame().unwrap();
        msgs.clear();
        decode_all(p, &mut msgs).unwrap();
        assert_eq!(msgs.len(), 1);
    }
    let dec = t.elapsed();
    let mb = (CHUNKS * MAX_FEATURE_BODY) as f64 / 1e6;
    println!(
        "transport throughput: encode+encrypt {:.0} MB/s, decrypt+decode {:.0} MB/s ({mb:.0} MB in 32 KiB chunks)",
        mb / enc.as_secs_f64(),
        mb / dec.as_secs_f64()
    );
}
