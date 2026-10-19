//! `moush` – headless Moush for testing, benchmarking and troubleshooting.
//!
//!   moush-cli monitors                  list monitors as Moush sees them
//!   moush-cli discover                  find hosts on the LAN
//!   moush-cli host [--port N]           run as host (pairing prompts on stdin)
//!   moush-cli client <addr|id> [--dry-run]
//!   moush-cli bench [--events N]        loopback latency benchmark of the full network path
//!   moush-cli selftest                  verify input APIs on this machine
//!   moush-cli identity                  print this device's id and public key
//!
//! Common options: --config-dir <dir>  (default %APPDATA%\Moush), --verbose

use std::io::{self, BufRead, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use moush_engine::client::{ClientEngine, ClientOptions, ClientState, ConnectTarget};
use moush_engine::core::crypto::Identity;
use moush_engine::core::discovery;
use moush_engine::core::inject::{InjectState, Injector};
use moush_engine::core::net::client::{ClientEvents, ClientLink, SessionConfig, run_session};
use moush_engine::core::net::host::{ClientHandle, HostEvents, HostServer};
use moush_engine::core::net::{Outgoing, PeerInfo};
use moush_engine::core::pairing::PairingBroker;
use moush_engine::core::protocol::{DEFAULT_PORT, Message, MonitorDesc, MouseButton, WheelAxis, key_flags};
use moush_engine::core::router::RouterSettings;
use moush_engine::core::stats::{LatencyRing, LatencySummary, Stats};
use moush_engine::core::trust::TrustStore;
use moush_engine::core::util::mono_us;
use moush_engine::host::{HostEngine, HostOptions};
use moush_engine::paths::AppPaths;
use moush_engine::win::{dpi, monitors, system};

struct Args {
    cmd: String,
    positional: Vec<String>,
    port: u16,
    events: u32,
    dry_run: bool,
    verbose: bool,
    config_dir: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        cmd: String::new(),
        positional: vec![],
        port: DEFAULT_PORT,
        events: 2000,
        dry_run: false,
        verbose: false,
        config_dir: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--port" => a.port = it.next().and_then(|v| v.parse().ok()).ok_or("--port needs a number")?,
            "--events" => a.events = it.next().and_then(|v| v.parse().ok()).ok_or("--events needs a number")?,
            "--dry-run" => a.dry_run = true,
            "--verbose" | "-v" => a.verbose = true,
            "--config-dir" => a.config_dir = Some(it.next().ok_or("--config-dir needs a path")?.into()),
            "-h" | "--help" => a.cmd = "help".into(),
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if a.cmd.is_empty() => a.cmd = s.to_owned(),
            s => a.positional.push(s.to_owned()),
        }
    }
    Ok(a)
}

fn main() {
    dpi::make_process_dpi_aware();
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let level = if args.verbose { log::LevelFilter::Debug } else { log::LevelFilter::Info };
    moush_engine::logger::init(None, level, true);
    let result = match args.cmd.as_str() {
        "monitors" => cmd_monitors(),
        "discover" => cmd_discover(),
        "host" => cmd_host(&args),
        "client" => cmd_client(&args),
        "bench" => cmd_bench(&args),
        "selftest" => cmd_selftest(),
        "identity" => cmd_identity(&args),
        _ => {
            println!(
                "{}",
                include_str!("main.rs")
                    .lines()
                    .skip(2)
                    .take(9)
                    .map(|l| l.trim_start_matches("//!"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            Ok(())
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn paths(args: &Args) -> AppPaths {
    match &args.config_dir {
        Some(d) => AppPaths::with_dirs(d.clone(), d.join("logs")),
        None => AppPaths::from_env(),
    }
}

fn load_identity_and_trust(args: &Args) -> Result<(Arc<Identity>, Arc<TrustStore>), String> {
    let p = paths(args);
    let id = moush_engine::identity::load_or_create(&p.identity()).map_err(|e| e.to_string())?;
    let trust = TrustStore::load(p.trusted()).map_err(|e| e.to_string())?;
    println!("device {} ({})", system::computer_name(), id.fingerprint());
    Ok((Arc::new(id), Arc::new(trust)))
}

fn cmd_identity(args: &Args) -> Result<(), String> {
    let p = paths(args);
    let id = moush_engine::identity::load_or_create(&p.identity()).map_err(|e| e.to_string())?;
    println!("id          {}", id.device_id());
    println!("fingerprint {}", id.fingerprint());
    println!("public key  {}", moush_engine::core::crypto::hex(id.public_key()));
    Ok(())
}

fn cmd_monitors() -> Result<(), String> {
    for m in monitors::enumerate() {
        println!(
            "{:<14} {:<24} {:>5}x{:<5} at ({:>6},{:>6})  {:>3}% {}",
            m.device,
            m.name,
            m.rect.w,
            m.rect.h,
            m.rect.x,
            m.rect.y,
            m.dpi * 100 / 96,
            if m.primary { "primary" } else { "" }
        );
    }
    Ok(())
}

fn cmd_discover() -> Result<(), String> {
    println!("probing {:?} ...", discovery::broadcast_targets());
    let found = discovery::scan(Duration::from_millis(1500)).map_err(|e| e.to_string())?;
    if found.is_empty() {
        println!(
            "no hosts answered (is a host running, and is UDP {} allowed by its firewall?)",
            moush_engine::core::protocol::DISCOVERY_PORT
        );
    }
    for f in found {
        println!("{:<24} {:<22} id {}", f.announce.name, f.addr, f.announce.id);
    }
    Ok(())
}

/// Answers pairing prompts from stdin: prints each new request and reads y/n.
fn stdin_broker() -> Arc<PairingBroker> {
    let slot: Arc<Mutex<Option<Arc<PairingBroker>>>> = Arc::new(Mutex::new(None));
    let (tx, rx) = mpsc::channel::<()>();
    let tx = Mutex::new(tx);
    let broker = PairingBroker::new(Arc::new(move || {
        let _ = tx.lock().unwrap().send(());
    }));
    *slot.lock().unwrap() = Some(broker.clone());
    thread::spawn(move || {
        let mut asked: Vec<String> = Vec::new();
        while rx.recv().is_ok() {
            let Some(b) = slot.lock().unwrap().clone() else { continue };
            for r in b.list() {
                if !r.needs_my_approval || asked.contains(&r.request_id) {
                    continue;
                }
                asked.push(r.request_id.clone());
                print!(
                    "\nPairing request from {} ({}).\nVerification code: {} {}\nDoes the other computer show the same code? Allow? [y/N] ",
                    r.peer_name,
                    r.peer_address,
                    &r.code[..3],
                    &r.code[3..]
                );
                let _ = io::stdout().flush();
                let mut line = String::new();
                let _ = io::stdin().lock().read_line(&mut line);
                let accept = line.trim().eq_ignore_ascii_case("y");
                b.respond(&r.request_id, accept);
                println!("{}", if accept { "allowed" } else { "rejected" });
            }
        }
    });
    broker
}

/// Returns when Enter is pressed. Without an interactive stdin (EOF) it never returns,
/// so the process can run under a script until it is terminated.
fn wait_for_enter() {
    let mut s = String::new();
    match io::stdin().lock().read_line(&mut s) {
        Ok(n) if n > 0 => {}
        _ => loop {
            thread::park();
        },
    }
}

fn cmd_host(args: &Args) -> Result<(), String> {
    let (id, trust) = load_identity_and_trust(args)?;
    let stats = Arc::new(Stats::new());
    let engine = HostEngine::start(
        HostOptions {
            port: args.port,
            name: system::computer_name(),
            discovery: true,
            router: RouterSettings::default(),
            layout_path: Some(paths(args).layout()),
        },
        id,
        trust,
        stdin_broker(),
        stats.clone(),
        moush_engine::features::FeatureHub::new(),
        Arc::new(|| {}),
    )
    .map_err(|e| e.to_string())?;
    let v = engine.view();
    println!("hosting on port {}; Ctrl+Alt+Esc returns the cursor; press Enter to stop", v.port);
    for m in &v.monitors {
        println!("  host monitor {} {}x{} at ({},{})", m.name, m.rect.w, m.rect.h, m.rect.x, m.rect.y);
    }
    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        thread::spawn(move || {
            wait_for_enter();
            stop.store(true, Ordering::SeqCst);
        });
    }
    let mut last = String::new();
    while !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(500));
        let v = engine.view();
        let clients = engine.clients();
        let line = format!(
            "clients: [{}]  active: {}  layout: [{}]",
            clients
                .iter()
                .map(|c| format!("{} {} rtt {:.1}ms", c.name, c.addr, c.rtt_ms.unwrap_or(-1.0)))
                .collect::<Vec<_>>()
                .join(", "),
            v.active.as_deref().unwrap_or("host"),
            v.layout
                .iter()
                .map(|c| format!(
                    "{}@({:.0},{:.0}) {:.0}x{:.0}{}",
                    c.name,
                    c.rect.x,
                    c.rect.y,
                    c.rect.w,
                    c.rect.h,
                    if c.adjacent { "" } else { " UNREACHABLE" }
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
        if line != last {
            println!("{line}");
            last = line;
        }
    }
    print_stats(&stats);
    engine.stop();
    Ok(())
}

fn cmd_client(args: &Args) -> Result<(), String> {
    let target = args.positional.first().ok_or("usage: moush-cli client <address|host-id> [--dry-run]")?;
    let (id, trust) = load_identity_and_trust(args)?;
    let target = if target.len() == 16 && target.chars().all(|c| c.is_ascii_hexdigit()) {
        ConnectTarget::Host { id: target.clone(), hint: None }
    } else {
        ConnectTarget::Address(target.clone())
    };
    let stats = Arc::new(Stats::new());
    let engine = ClientEngine::start(
        target,
        ClientOptions {
            name: system::computer_name(),
            default_port: args.port,
            auto_reconnect: true,
            dry_run: args.dry_run,
            focus_on_enter: false,
        },
        id,
        trust,
        stdin_broker(),
        stats.clone(),
        moush_engine::features::FeatureHub::new(),
        Arc::new(|| {}),
        |_| {},
    )
    .map_err(|e| e.to_string())?;
    println!("press Enter to stop");
    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        thread::spawn(move || {
            wait_for_enter();
            stop.store(true, Ordering::SeqCst);
        });
    }
    let mut last = String::new();
    while !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(300));
        let v = engine.view();
        let line = format!(
            "{:?} host={} active={} rtt={} {}",
            v.state,
            v.host_name.as_deref().unwrap_or("-"),
            v.active,
            engine.rtt_ms().map(|r| format!("{r:.1}ms")).unwrap_or("-".into()),
            v.last_error.as_deref().unwrap_or("")
        );
        if line != last {
            println!("{line}");
            last = line;
        }
        if v.state == ClientState::Error {
            break;
        }
    }
    print_stats(&stats);
    engine.stop();
    Ok(())
}

fn print_stats(s: &Stats) {
    let us = |n: &str, l: LatencySummary| {
        if l.count > 0 {
            println!(
                "  {n:<26} n={:<6} p50 {:>7.0} us  p95 {:>7.0} us  p99 {:>7.0} us  max {:>7.0} us",
                l.count, l.p50, l.p95, l.p99, l.max
            );
        }
    };
    println!("statistics:");
    us("hook callback", Stats::summary(&s.hook_us));
    us("hook -> socket write", Stats::summary(&s.send_us));
    us("frame -> SendInput done", Stats::summary(&s.inject_us));
    us("ping round trip", Stats::summary(&s.rtt_us));
    println!(
        "  events captured {} forwarded {} injected {} failed {}; reconnects {}",
        Stats::get(&s.events_captured),
        Stats::get(&s.events_forwarded),
        Stats::get(&s.events_injected),
        Stats::get(&s.inject_failures),
        Stats::get(&s.reconnects)
    );
}

// ------------------------------------------------------------------ bench

struct Timestamps(mpsc::Sender<u64>);

impl Injector for Timestamps {
    fn move_abs(&mut self, _: i32, _: i32) -> bool {
        self.0.send(mono_us()).is_ok()
    }
    fn button(&mut self, _: MouseButton, _: bool) -> bool {
        self.0.send(mono_us()).is_ok()
    }
    fn wheel(&mut self, _: WheelAxis, _: i16) -> bool {
        true
    }
    fn key(&mut self, _: u16, _: u16, _: bool, _: bool, _: bool) -> bool {
        self.0.send(mono_us()).is_ok()
    }
}

struct BenchHost(Mutex<mpsc::Sender<ClientHandle>>);
impl HostEvents for BenchHost {
    fn client_connected(&self, c: ClientHandle) {
        let _ = self.0.lock().unwrap().send(c);
    }
    fn client_screens(&self, _: &str, _: u64, _: Vec<MonitorDesc>) {}
    fn client_disconnected(&self, _: &str, _: u64, _: &str) {}
}

struct Quiet;
impl ClientEvents for Quiet {
    fn connected(&self, _: &PeerInfo, _: &moush_engine::core::net::PeerSender) {}
    fn active_changed(&self, _: bool) {}
}

/// Full network path on loopback: encode, encrypt, TCP, decrypt, decode, inject (mock).
fn cmd_bench(args: &Args) -> Result<(), String> {
    let host_id = Arc::new(Identity::generate().map_err(|e| e.to_string())?);
    let client_id = Identity::generate().map_err(|e| e.to_string())?;
    let host_trust = Arc::new(TrustStore::ephemeral());
    let client_trust = Arc::new(TrustStore::ephemeral());
    // Pre-trust both sides so no prompt is needed.
    host_trust.add(client_id.public_key(), "bench-client", moush_engine::core::trust::TrustRole::Client, None).ok();
    client_trust.add(host_id.public_key(), "bench-host", moush_engine::core::trust::TrustRole::Host, None).ok();
    let (ctx, crx) = mpsc::channel();
    let host_stats = Arc::new(Stats::new());
    let server = HostServer::start(
        std::net::SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        "bench-host".into(),
        host_id,
        host_trust,
        PairingBroker::new(Arc::new(|| {})),
        host_stats.clone(),
        Arc::new(BenchHost(Mutex::new(ctx))),
    )
    .map_err(|e| e.to_string())?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, server.port()));
    let (etx, erx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let link = ClientLink::new();
    let client_stats = Arc::new(Stats::new());
    let session = {
        let (stop, link, client_stats) = (stop.clone(), link.clone(), client_stats.clone());
        thread::spawn(move || {
            let broker = PairingBroker::new(Arc::new(|| {}));
            let mut inject = InjectState::new(Timestamps(etx));
            run_session(
                SessionConfig {
                    addr,
                    expected_host: None,
                    identity: &client_id,
                    name: "bench-client",
                    trust: &client_trust,
                    broker: &broker,
                    stats: &client_stats,
                    screens: vec![MonitorDesc {
                        x: 0,
                        y: 0,
                        width: 1920,
                        height: 1080,
                        dpi: 96,
                        primary: true,
                        name: "bench".into(),
                    }],
                    stop: &stop,
                },
                &mut inject,
                &link,
                &Quiet,
            )
        })
    };
    let t_connect = Instant::now();
    let client = crx.recv_timeout(Duration::from_secs(5)).map_err(|_| "bench client did not connect")?;
    println!("connected (handshake + authorization) in {:.1} ms", t_connect.elapsed().as_secs_f64() * 1000.0);
    client.outbox.send(Outgoing::now(Message::Enter { x: 0, y: 0 })).map_err(|e| e.to_string())?;
    erx.recv_timeout(Duration::from_secs(5)).map_err(|_| "no enter")?;

    let mut ring = LatencyRing::default();
    let n = args.events;
    println!("sending {n} key events at 1000 Hz ...");
    let start = Instant::now();
    for i in 0..n {
        let t = mono_us();
        let flags = if i % 2 == 0 { key_flags::DOWN } else { 0 };
        client
            .outbox
            .send(Outgoing { msg: Message::Key { vk: 0x41, scan: 0x1e, flags }, t_us: t })
            .map_err(|e| e.to_string())?;
        let at = erx.recv_timeout(Duration::from_secs(5)).map_err(|_| "event lost")?;
        ring.record(at.saturating_sub(t) as u32);
        // Pace to 1 kHz like a gaming mouse.
        let next = start + Duration::from_micros(1000 * (i as u64 + 1));
        if let Some(d) = next.checked_duration_since(Instant::now()) {
            thread::sleep(d);
        }
    }
    let s = ring.summary();
    println!(
        "enqueue -> injected (one way, loopback): mean {:.0} us  p50 {:.0} us  p95 {:.0} us  p99 {:.0} us  max {:.0} us",
        s.mean, s.p50, s.p95, s.p99, s.max
    );
    let (_, replies, rtt) = server.latency_test(&client.id, 200).map_err(|e| e.to_string())?;
    println!("ping round trip: {replies}/200, p50 {:.3} ms  p99 {:.3} ms", rtt.p50, rtt.p99);
    println!("host send path:");
    print_stats(&host_stats);
    stop.store(true, Ordering::SeqCst);
    link.close();
    let _ = session.join();
    server.stop();
    Ok(())
}

// ------------------------------------------------------------------ selftest

/// Checks the Win32 behaviour the input layer relies on. Briefly moves the cursor and
/// puts it back; installs the hooks for a moment in pass-through mode.
fn cmd_selftest() -> Result<(), String> {
    use moush_engine::core::geometry::{Layout, Rect, Screen};
    use moush_engine::win::capture::{Capture, CaptureEvents};
    use moush_engine::win::inject::SendInputInjector;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

    let mut ok = true;
    let mons = monitors::enumerate();
    println!("[1] monitors: {}", mons.len());
    for m in &mons {
        println!("    {} {}x{} at ({},{}) dpi {}", m.name, m.rect.w, m.rect.h, m.rect.x, m.rect.y, m.dpi);
    }
    if mons.is_empty() {
        return Err("no monitors".into());
    }

    println!("[2] SendInput absolute positioning (exact pixel)");
    let mut orig = POINT::default();
    unsafe { GetCursorPos(&mut orig) }.map_err(|e| e.to_string())?;
    let mut inj = SendInputInjector::new();
    let mut misses = 0;
    let mut checked = 0;
    for m in &mons {
        let r = m.rect;
        for (x, y) in [
            (r.x, r.y),
            (r.x + 1, r.y + 1),
            (r.x + r.w / 2, r.y + r.h / 2),
            (r.x + r.w - 2, r.y + r.h - 2),
            (r.x + r.w - 1, r.y + r.h - 1),
            (r.x + 333, r.y + 777 % r.h),
        ] {
            if !inj.move_abs(x, y) {
                println!("    SendInput rejected the move (another program may be blocking input)");
                ok = false;
                break;
            }
            thread::sleep(Duration::from_millis(15));
            let mut p = POINT::default();
            let _ = unsafe { GetCursorPos(&mut p) };
            checked += 1;
            if (p.x, p.y) != (x, y) {
                misses += 1;
                println!("    target ({x},{y}) landed at ({},{})", p.x, p.y);
            }
        }
    }
    unsafe {
        let _ = SetCursorPos(orig.x, orig.y);
    }
    println!("    {}/{checked} positions exact", checked - misses);
    ok &= misses == 0;

    println!("[3] low-level hooks install/uninstall");
    struct NoEv;
    impl CaptureEvents for NoEv {
        fn active_changed(&self, _: Option<String>) {}
        fn lock_changed(&self, _: bool) {}
        fn displays_changed(&self) {}
        fn send_failed(&self, _: &str) {}
    }
    let layout = Layout { host: Screen::new(mons.iter().map(|m| m.rect).collect::<Vec<Rect>>()), clients: vec![] };
    let stats = Arc::new(Stats::new());
    let t = Instant::now();
    match Capture::start(layout, RouterSettings::default(), stats.clone(), Arc::new(NoEv)) {
        Ok(c) => {
            println!("    installed in {:.1} ms (pass-through, no clients)", t.elapsed().as_secs_f64() * 1000.0);
            thread::sleep(Duration::from_millis(1500));
            let installed = c.hooks_installed();
            c.stop();
            let h = Stats::summary(&stats.hook_us);
            println!("    events seen while installed: {} (move the mouse during the test to sample)", h.count);
            if h.count > 0 {
                println!("    hook callback time: p50 {:.1} us  p99 {:.1} us  max {:.1} us", h.p50, h.p99, h.max);
            }
            ok &= installed;
        }
        Err(e) => {
            println!("    FAILED: {e}");
            ok = false;
        }
    }

    println!("[4] elevated: {}; OS build {}", system::is_elevated(), system::os_build());
    println!("[5] firewall rule: {:?}", system::firewall_state());
    println!("[6] identity storage (DPAPI)");
    let secret = b"moush-selftest";
    let enc = system::protect(secret).map_err(|e| e.to_string())?;
    let dec = system::unprotect(&enc).map_err(|e| e.to_string())?;
    println!("    roundtrip {}", if dec == secret { "ok" } else { "MISMATCH" });
    ok &= dec == secret;

    if ok {
        println!("selftest passed");
        Ok(())
    } else {
        Err("selftest found problems (see above)".into())
    }
}
