# IPv6 Dual-Stack Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add IPv6 support with dual-stack listening, Happy Eyeballs connection racing, and `-4`/`-6` address family flags.

**Architecture:** `socket2` handles socket setup (IPV6_V6ONLY, SO_REUSEADDR, connect_timeout), then converts to std types for runtime I/O. Dual-socket listen uses threads + channels to race accept across IPv4/IPv6 listeners. Happy Eyeballs connect spawns IPv6 first, races IPv4 after 250ms.

**Tech Stack:** Rust, socket2 (new dep), std threading + mpsc channels

---

### Task 1: Add socket2 dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add socket2 to Cargo.toml**

```toml
[dependencies]
clap = { version = "4.6.1", features = ["derive"] }
owo-colors = { version = "4", features = ["supports-colors"] }
socket2 = { version = "0.5", features = ["all"] }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "Add socket2 dependency for IPv6 socket options"
```

---

### Task 2: CLI — AddrFamily enum and -4/-6 flags

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/net.rs` (test helper only)
- Modify: `src/verbose.rs` (test helper only)

- [ ] **Step 1: Write failing tests for -4/-6 flag parsing**

Add these tests at the bottom of the `#[cfg(test)] mod tests` block in `src/cli.rs`:

```rust
#[test]
fn ipv4_flag_short() {
    let c = Config::from_args(["vibecat", "-4", "example.com", "80"]).unwrap();
    assert_eq!(c.addr_family, AddrFamily::Ipv4);
}

#[test]
fn ipv6_flag_short() {
    let c = Config::from_args(["vibecat", "-6", "example.com", "80"]).unwrap();
    assert_eq!(c.addr_family, AddrFamily::Ipv6);
}

#[test]
fn ipv4_flag_long() {
    let c = Config::from_args(["vibecat", "--ipv4", "-l", "8080"]).unwrap();
    assert_eq!(c.addr_family, AddrFamily::Ipv4);
}

#[test]
fn ipv6_flag_long() {
    let c = Config::from_args(["vibecat", "--ipv6", "-l", "8080"]).unwrap();
    assert_eq!(c.addr_family, AddrFamily::Ipv6);
}

#[test]
fn addr_family_defaults_to_both() {
    let c = Config::from_args(["vibecat", "example.com", "80"]).unwrap();
    assert_eq!(c.addr_family, AddrFamily::Both);
}

#[test]
fn ipv4_and_ipv6_together_is_error() {
    assert!(Config::from_args(["vibecat", "-4", "-6", "example.com", "80"]).is_err());
}

#[test]
fn ipv4_with_udp_and_listen() {
    let c = Config::from_args(["vibecat", "-4", "-u", "-l", "8080"]).unwrap();
    assert_eq!(c.addr_family, AddrFamily::Ipv4);
    assert_eq!(c.proto, Proto::Udp);
    assert_eq!(c.mode, Mode::Listen);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli`
Expected: compilation errors — `AddrFamily` doesn't exist, `Config` has no `addr_family` field

- [ ] **Step 3: Implement AddrFamily enum, update RawArgs and Config**

Add the `AddrFamily` enum after the `Proto` enum in `src/cli.rs`:

```rust
/// Address family constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrFamily {
    Both,
    Ipv4,
    Ipv6,
}
```

Add the `addr_family` field to `Config`:

```rust
pub struct Config {
    pub mode: Mode,
    pub proto: Proto,
    pub host: Option<String>,
    pub port: u16,
    pub verbose: bool,
    pub addr_family: AddrFamily,
}
```

Add `-4` and `-6` flags to `RawArgs` (after the `verbose` field):

```rust
/// Use IPv4 only.
#[arg(short = '4', long = "ipv4")]
ipv4: bool,

/// Use IPv6 only.
#[arg(short = '6', long = "ipv6")]
ipv6: bool,
```

Update `from_raw` to validate and set `addr_family`. Add the validation after the `let port` line, before the final `Ok(Config { ... })`:

```rust
let addr_family = match (raw.ipv4, raw.ipv6) {
    (true, true) => return Err("-4 and -6 are mutually exclusive".to_string()),
    (true, false) => AddrFamily::Ipv4,
    (false, true) => AddrFamily::Ipv6,
    (false, false) => AddrFamily::Both,
};

Ok(Config { mode, proto, host, port, verbose: raw.verbose, addr_family })
```

- [ ] **Step 4: Update test helpers in net.rs and verbose.rs**

In `src/net.rs`, update the `cfg` helper in the test module to include `addr_family`:

```rust
fn cfg(mode: Mode, proto: Proto, host: Option<&str>, port: u16) -> Config {
    Config { mode, proto, host: host.map(String::from), port, verbose: false, addr_family: AddrFamily::Both }
}
```

Add `use crate::cli::AddrFamily;` to the net.rs test module's imports (next to `use crate::cli::Mode;`).

In `src/verbose.rs`, update the `cfg` helper in the test module:

```rust
fn cfg(verbose: bool, proto: Proto, host: Option<&str>) -> Config {
    Config {
        mode: Mode::Connect,
        proto,
        host: host.map(String::from),
        port: 80,
        verbose,
        addr_family: AddrFamily::Both,
    }
}
```

Add `use crate::cli::AddrFamily;` to the verbose.rs test module's imports (next to `use crate::cli::Mode;`).

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: all tests pass (existing + new)

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/net.rs src/verbose.rs
git commit -m "Add -4/--ipv4 and -6/--ipv6 CLI flags with AddrFamily enum"
```

---

### Task 3: Verbose — update protocol qualifier format

**Files:**
- Modify: `src/verbose.rs`
- Modify: `tests/e2e.rs`

- [ ] **Step 1: Write failing unit tests for new proto_label format**

Add these tests to the `#[cfg(test)] mod tests` block in `src/verbose.rs`. Add `use std::net::Ipv6Addr;` to the test module's imports.

```rust
#[test]
fn proto_label_ipv4_tcp() {
    assert_eq!(proto_label(Proto::Tcp, addr([127, 0, 0, 1], 80)), "IPv4/TCP");
}

#[test]
fn proto_label_ipv4_udp() {
    assert_eq!(proto_label(Proto::Udp, addr([0, 0, 0, 0], 9999)), "IPv4/UDP");
}

#[test]
fn proto_label_ipv6_tcp() {
    let a = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 80);
    assert_eq!(proto_label(Proto::Tcp, a), "IPv6/TCP");
}

#[test]
fn proto_label_ipv6_udp() {
    let a = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 9999);
    assert_eq!(proto_label(Proto::Udp, a), "IPv6/UDP");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib verbose`
Expected: compilation error — `proto_label` doesn't exist

- [ ] **Step 3: Replace proto_str with proto_label, update log functions**

In `src/verbose.rs`, replace the `proto_str` function with:

```rust
fn proto_label(proto: Proto, addr: SocketAddr) -> String {
    let family = if addr.is_ipv4() { "IPv4" } else { "IPv6" };
    let transport = match proto {
        Proto::Tcp => "TCP",
        Proto::Udp => "UDP",
    };
    format!("{family}/{transport}")
}
```

Update `log_listening` — replace the `let proto = proto_str(config.proto);` line and the format string:

```rust
pub fn log_listening(config: &Config, bind_addr: SocketAddr) {
    if !config.verbose {
        return;
    }
    let label = proto_label(config.proto, bind_addr);
    let bc = bold_cyan();
    eprintln!(
        "vibecat: {} on {} on port {} ({label}).",
        "Listening".if_supports_color(Stream::Stderr, |t| t.green()),
        bind_addr
            .ip()
            .if_supports_color(Stream::Stderr, |t| t.style(bc)),
        bind_addr
            .port()
            .if_supports_color(Stream::Stderr, |t| t.style(bc)),
    );
}
```

Update `log_connected` — replace the `let proto = proto_str(config.proto);` line and the format string:

```rust
pub fn log_connected(config: &Config, local_addr: SocketAddr, remote_addr: SocketAddr) {
    if !config.verbose {
        return;
    }
    let label = proto_label(config.proto, remote_addr);
    let remote_ip = remote_addr.ip().to_string();

    let destination = format_destination(config.host.as_deref(), &remote_ip);

    let bc = bold_cyan();
    eprintln!(
        "vibecat: {} to {destination} on port {} ({label}) from {} on port {}.",
        "Connected".if_supports_color(Stream::Stderr, |t| t.green()),
        remote_addr
            .port()
            .if_supports_color(Stream::Stderr, |t| t.style(bc)),
        local_addr.ip(),
        local_addr.port(),
    );
}
```

- [ ] **Step 4: Update e2e verbose tests**

In `tests/e2e.rs`, update `verbose_connect_prints_connected_to_stderr`. Change:

```rust
assert!(stderr.contains("(tcp)"), "stderr should contain '(tcp)', got: {stderr}");
```

to:

```rust
assert!(stderr.contains("(IPv4/TCP)"), "stderr should contain '(IPv4/TCP)', got: {stderr}");
```

No changes needed in `verbose_listen_prints_listening_and_connected_to_stderr` — it doesn't check the protocol label.

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/verbose.rs tests/e2e.rs
git commit -m "Change verbose protocol label to IPv4/TCP format (layer 3 first)"
```

---

### Task 4: Dual-socket listen mode

**Files:**
- Modify: `src/net.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add new imports to net.rs**

Add these imports at the top of `src/net.rs`:

```rust
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use socket2::{Domain, Socket, Type};

use crate::cli::AddrFamily;
```

- [ ] **Step 2: Add socket2 helper — make_tcp_listener**

Add this function in `src/net.rs` above the `connect` function:

```rust
fn make_tcp_listener(addr: SocketAddr) -> io::Result<TcpListener> {
    let domain = match addr {
        SocketAddr::V4(_) => Domain::IPV4,
        SocketAddr::V6(_) => Domain::IPV6,
    };
    let socket = Socket::new(domain, Type::STREAM, None)?;
    socket.set_reuse_address(true)?;
    if addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.bind(&addr.into())?;
    socket.listen(1)?;
    Ok(socket.into())
}
```

- [ ] **Step 3: Add socket2 helper — make_udp_socket**

Add this function right after `make_tcp_listener`:

```rust
fn make_udp_socket(addr: SocketAddr) -> io::Result<UdpSocket> {
    let domain = match addr {
        SocketAddr::V4(_) => Domain::IPV4,
        SocketAddr::V6(_) => Domain::IPV6,
    };
    let socket = Socket::new(domain, Type::DGRAM, None)?;
    socket.set_reuse_address(true)?;
    if addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.bind(&addr.into())?;
    Ok(socket.into())
}
```

- [ ] **Step 4: Add listen_addrs helper**

Add this function after `make_udp_socket`:

```rust
fn listen_addrs(config: &Config) -> io::Result<Vec<SocketAddr>> {
    let port = config.port;
    match config.host.as_deref() {
        Some(host) => {
            let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs()?.collect();
            if addrs.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrNotAvailable,
                    format!("no addresses resolved for {host}"),
                ));
            }
            Ok(addrs)
        }
        None => Ok(match config.addr_family {
            AddrFamily::Both => vec![
                SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
            ],
            AddrFamily::Ipv4 => {
                vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)]
            }
            AddrFamily::Ipv6 => {
                vec![SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port)]
            }
        }),
    }
}
```

- [ ] **Step 5: Refactor bind() to return Vec and use socket2**

Replace the entire `bind` function with:

```rust
pub fn bind(config: &Config) -> io::Result<Vec<(Listener, SocketAddr)>> {
    let addrs = listen_addrs(config)?;
    let mut listeners = Vec::with_capacity(addrs.len());
    for addr in addrs {
        match config.proto {
            Proto::Tcp => {
                let listener = make_tcp_listener(addr)?;
                let bound = listener.local_addr()?;
                listeners.push((Listener::Tcp(listener), bound));
            }
            Proto::Udp => {
                let socket = make_udp_socket(addr)?;
                let bound = socket.local_addr()?;
                listeners.push((Listener::Udp(socket), bound));
            }
        }
    }
    Ok(listeners)
}
```

- [ ] **Step 6: Make accept private, add accept_first**

Change `pub fn accept` to `fn accept` (remove `pub`).

Add the public `accept_first` function and its helpers after `accept`:

```rust
pub fn accept_first(
    mut listeners: Vec<(Listener, SocketAddr)>,
) -> io::Result<(Conn, Option<Vec<u8>>, SocketAddr, SocketAddr)> {
    if listeners.len() == 1 {
        let (listener, _) = listeners.pop().unwrap();
        return accept(listener);
    }

    let is_tcp = matches!(listeners[0].0, Listener::Tcp(_));
    if is_tcp {
        accept_first_tcp(listeners)
    } else {
        accept_first_udp(listeners)
    }
}

fn accept_first_tcp(
    listeners: Vec<(Listener, SocketAddr)>,
) -> io::Result<(Conn, Option<Vec<u8>>, SocketAddr, SocketAddr)> {
    let (tx, rx) = mpsc::channel();

    for (listener, _) in listeners {
        let tx = tx.clone();
        thread::spawn(move || {
            let _ = tx.send(accept(listener));
        });
    }
    drop(tx);

    let mut last_err = io::Error::new(io::ErrorKind::Other, "no listeners accepted a connection");
    for result in rx {
        match result {
            Ok(conn_tuple) => return Ok(conn_tuple),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn accept_first_udp(
    listeners: Vec<(Listener, SocketAddr)>,
) -> io::Result<(Conn, Option<Vec<u8>>, SocketAddr, SocketAddr)> {
    let done = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();

    for (listener, _) in listeners {
        let tx = tx.clone();
        let done = Arc::clone(&done);
        thread::spawn(move || {
            if let Listener::Udp(socket) = listener {
                socket
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .ok();
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    if done.load(Ordering::Relaxed) {
                        return;
                    }
                    match socket.recv_from(&mut buf) {
                        Ok((n, peer)) => {
                            done.store(true, Ordering::Relaxed);
                            buf.truncate(n);
                            if let Err(e) = socket.connect(peer) {
                                let _ = tx.send(Err(e));
                                return;
                            }
                            let local = match socket.local_addr() {
                                Ok(a) => a,
                                Err(e) => {
                                    let _ = tx.send(Err(e));
                                    return;
                                }
                            };
                            let _ = tx.send(Ok((
                                Conn::Udp(UdpStream::new(socket)),
                                Some(buf),
                                local,
                                peer,
                            )));
                            return;
                        }
                        Err(ref e)
                            if e.kind() == io::ErrorKind::WouldBlock
                                || e.kind() == io::ErrorKind::TimedOut =>
                        {
                            continue;
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            return;
                        }
                    }
                }
            }
        });
    }
    drop(tx);

    let mut last_err =
        io::Error::new(io::ErrorKind::Other, "no UDP listener received a datagram");
    for result in rx {
        match result {
            Ok(conn_tuple) => return Ok(conn_tuple),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}
```

- [ ] **Step 7: Update main.rs listen branch**

In `src/main.rs`, replace the `Mode::Listen` branch of `run()`:

```rust
Mode::Listen => {
    let listeners = net::bind(config)?;
    for (_, bind_addr) in &listeners {
        verbose::log_listening(config, *bind_addr);
    }
    let (conn, initial, local_addr, peer_addr) = net::accept_first(listeners)?;
    verbose::log_connected(config, local_addr, peer_addr);
    (conn, initial)
}
```

- [ ] **Step 8: Update net.rs unit test for new bind/accept API**

In the `src/net.rs` test module, update `udp_bind_accept_returns_first_datagram_payload`. Replace:

```rust
let server = thread::spawn(move || {
    let (listener, bind_addr) = bind(&listen_cfg).unwrap();
    assert_eq!(bind_addr.port(), port);
    accept(listener).unwrap()
});
```

with:

```rust
let server = thread::spawn(move || {
    let listeners = bind(&listen_cfg).unwrap();
    assert_eq!(listeners[0].1.port(), port);
    accept_first(listeners).unwrap()
});
```

- [ ] **Step 9: Run all tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 10: Commit**

```bash
git add src/net.rs src/main.rs
git commit -m "Dual-socket listen: bind IPv4+IPv6, race accept via threads"
```

---

### Task 5: E2E — dual-listen TCP tests

**Files:**
- Modify: `tests/e2e.rs`

- [ ] **Step 1: Write dual-listen TCP test — IPv4 client**

Add to `tests/e2e.rs`:

```rust
#[test]
fn dual_listen_accepts_ipv4_client() {
    let port = free_tcp_port();

    let mut server = Command::new(bin())
        .args(["-l", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(200));

    let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    client.write_all(b"ipv4-hello").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();

    drop(server.stdin.take());

    let mut server_stdout = Vec::new();
    server
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut server_stdout)
        .unwrap();
    server.wait().unwrap();

    assert_eq!(server_stdout, b"ipv4-hello");
}
```

- [ ] **Step 2: Write dual-listen TCP test — IPv6 client**

Add to `tests/e2e.rs`:

```rust
#[test]
fn dual_listen_accepts_ipv6_client() {
    let port = free_tcp_port();

    let mut server = Command::new(bin())
        .args(["-l", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(200));

    let mut client = std::net::TcpStream::connect(("[::1]", port)).unwrap();
    client.write_all(b"ipv6-hello").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();

    drop(server.stdin.take());

    let mut server_stdout = Vec::new();
    server
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut server_stdout)
        .unwrap();
    server.wait().unwrap();

    assert_eq!(server_stdout, b"ipv6-hello");
}
```

- [ ] **Step 3: Write verbose dual-listen test**

Add to `tests/e2e.rs`:

```rust
#[test]
fn verbose_dual_listen_prints_both_listening_lines() {
    let port = free_tcp_port();

    let mut server = Command::new(bin())
        .args(["-v", "-l", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(200));

    let client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    client.shutdown(std::net::Shutdown::Both).unwrap();
    drop(client);

    drop(server.stdin.take());

    let output = server.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("(IPv6/TCP)"),
        "stderr should contain '(IPv6/TCP)', got: {stderr}"
    );
    assert!(
        stderr.contains("(IPv4/TCP)"),
        "stderr should contain '(IPv4/TCP)', got: {stderr}"
    );
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add tests/e2e.rs
git commit -m "Add e2e tests for dual-socket TCP listen (IPv4 and IPv6 clients)"
```

---

### Task 6: E2E — dual-listen UDP tests

**Files:**
- Modify: `tests/e2e.rs`

- [ ] **Step 1: Write dual-listen UDP test — IPv4 client**

Add to `tests/e2e.rs`:

```rust
#[test]
fn dual_listen_udp_accepts_ipv4_datagram() {
    let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let mut server = Command::new(bin())
        .args(["-l", "-u", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(200));

    let client = UdpSocket::bind("0.0.0.0:0").unwrap();
    client
        .send_to(b"udp4-hello", ("127.0.0.1", port))
        .unwrap();

    let mut buf = [0u8; 10];
    server
        .stdout
        .as_mut()
        .unwrap()
        .read_exact(&mut buf)
        .unwrap();
    assert_eq!(&buf, b"udp4-hello");

    server.kill().unwrap();
    server.wait().unwrap();
}
```

- [ ] **Step 2: Write dual-listen UDP test — IPv6 client**

Add to `tests/e2e.rs`:

```rust
#[test]
fn dual_listen_udp_accepts_ipv6_datagram() {
    let probe = UdpSocket::bind("[::1]:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let mut server = Command::new(bin())
        .args(["-l", "-u", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(200));

    let client = UdpSocket::bind("[::]:0").unwrap();
    client.send_to(b"udp6-hello", ("[::1]", port)).unwrap();

    let mut buf = [0u8; 10];
    server
        .stdout
        .as_mut()
        .unwrap()
        .read_exact(&mut buf)
        .unwrap();
    assert_eq!(&buf, b"udp6-hello");

    server.kill().unwrap();
    server.wait().unwrap();
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Commit**

```bash
git add tests/e2e.rs
git commit -m "Add e2e tests for dual-socket UDP listen (IPv4 and IPv6 datagrams)"
```

---

### Task 7: Happy Eyeballs TCP connect

**Files:**
- Modify: `src/net.rs`

- [ ] **Step 1: Add try_connect_addrs helper**

Add this function in `src/net.rs` after the `connect` function:

```rust
fn try_connect_addrs(addrs: &[SocketAddr]) -> io::Result<(Conn, SocketAddr)> {
    let mut last_err = io::Error::new(io::ErrorKind::AddrNotAvailable, "no addresses to try");
    for addr in addrs {
        let domain = match addr {
            SocketAddr::V4(_) => Domain::IPV4,
            SocketAddr::V6(_) => Domain::IPV6,
        };
        let socket = match Socket::new(domain, Type::STREAM, None) {
            Ok(s) => s,
            Err(e) => {
                last_err = e;
                continue;
            }
        };
        match socket.connect_timeout(&(*addr).into(), Duration::from_secs(5)) {
            Ok(()) => {
                let stream: TcpStream = socket.into();
                let local = stream.local_addr()?;
                return Ok((Conn::Tcp(stream), local));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}
```

- [ ] **Step 2: Add happy_eyeballs function**

Add this function after `try_connect_addrs`:

```rust
fn happy_eyeballs(
    primary: Vec<SocketAddr>,
    fallback: Vec<SocketAddr>,
) -> io::Result<(Conn, SocketAddr)> {
    let (tx, rx) = mpsc::channel();

    let tx1 = tx.clone();
    thread::spawn(move || {
        let _ = tx1.send(try_connect_addrs(&primary));
    });

    let tx2 = tx.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        let _ = tx2.send(try_connect_addrs(&fallback));
    });

    drop(tx);

    let mut last_err = io::Error::new(
        io::ErrorKind::ConnectionRefused,
        "all connection attempts failed",
    );
    for result in rx {
        match result {
            Ok(conn) => return Ok(conn),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}
```

- [ ] **Step 3: Refactor connect() to use Happy Eyeballs**

Replace the entire `connect` function with:

```rust
pub fn connect(config: &Config) -> io::Result<(Conn, SocketAddr)> {
    let host = config.host.as_deref().unwrap_or("localhost");
    let port = config.port;
    match config.proto {
        Proto::Tcp => connect_tcp(host, port, config.addr_family),
        Proto::Udp => connect_udp(host, port, config.addr_family),
    }
}

fn connect_tcp(host: &str, port: u16, family: AddrFamily) -> io::Result<(Conn, SocketAddr)> {
    let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs()?.collect();

    let (primary, fallback) = match family {
        AddrFamily::Ipv4 => {
            let v4: Vec<_> = addrs.into_iter().filter(|a| a.is_ipv4()).collect();
            (v4, vec![])
        }
        AddrFamily::Ipv6 => {
            let v6: Vec<_> = addrs.into_iter().filter(|a| a.is_ipv6()).collect();
            (v6, vec![])
        }
        AddrFamily::Both => {
            let (v6, v4): (Vec<_>, Vec<_>) = addrs.into_iter().partition(|a| a.is_ipv6());
            (v6, v4)
        }
    };

    if primary.is_empty() && fallback.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("no addresses resolved for {host}"),
        ));
    }

    if fallback.is_empty() {
        return try_connect_addrs(&primary);
    }

    if primary.is_empty() {
        return try_connect_addrs(&fallback);
    }

    happy_eyeballs(primary, fallback)
}
```

- [ ] **Step 4: Add connect_udp stub**

`connect` now references `connect_udp` which doesn't exist yet (Task 8). Add a temporary stub after `connect_tcp` so the code compiles:

```rust
fn connect_udp(host: &str, port: u16, _family: AddrFamily) -> io::Result<(Conn, SocketAddr)> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect((host, port))?;
    let local = socket.local_addr()?;
    Ok((Conn::Udp(UdpStream::new(socket)), local))
}
```

This preserves the old IPv4-only UDP behavior. Task 8 replaces it with the full implementation.

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: all tests pass (existing connect tests use explicit `127.0.0.1`, which resolves to a single IPv4 address — no racing needed)

- [ ] **Step 6: Commit**

```bash
git add src/net.rs
git commit -m "Happy Eyeballs TCP connect: prefer IPv6, race IPv4 after 250ms"
```

---

### Task 8: UDP connect with IPv6 preference

**Files:**
- Modify: `src/net.rs`

- [ ] **Step 1: Write failing test for UDP IPv6 connect preference**

Add to the `#[cfg(test)] mod tests` block in `src/net.rs`:

```rust
#[test]
fn udp_connect_ipv6_preferred() {
    let remote = UdpSocket::bind("[::1]:0").unwrap();
    let port = remote.local_addr().unwrap().port();

    let config = Config {
        mode: Mode::Connect,
        proto: Proto::Udp,
        host: Some("::1".to_string()),
        port,
        verbose: false,
        addr_family: AddrFamily::Ipv6,
    };
    let (conn, local) = connect(&config).unwrap();
    assert!(local.is_ipv6());
    drop(conn);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib net::tests::udp_connect_ipv6_preferred`
Expected: FAIL — current `connect_udp` is a stub that panics with `todo!()`.

- [ ] **Step 3: Implement connect_udp**

In `src/net.rs`, replace the `connect_udp` stub (from Task 7) with the full implementation:

```rust
fn connect_udp(host: &str, port: u16, family: AddrFamily) -> io::Result<(Conn, SocketAddr)> {
    let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs()?.collect();

    let addr = match family {
        AddrFamily::Ipv4 => addrs.iter().find(|a| a.is_ipv4()),
        AddrFamily::Ipv6 => addrs.iter().find(|a| a.is_ipv6()),
        AddrFamily::Both => addrs
            .iter()
            .find(|a| a.is_ipv6())
            .or_else(|| addrs.iter().find(|a| a.is_ipv4())),
    }
    .ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            format!("no matching addresses for {host}"),
        )
    })?;

    let bind_addr: SocketAddr = if addr.is_ipv4() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
    } else {
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
    };
    let socket = UdpSocket::bind(bind_addr)?;
    socket.connect(addr)?;
    let local = socket.local_addr()?;
    Ok((Conn::Udp(UdpStream::new(socket)), local))
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/net.rs
git commit -m "UDP connect: prefer IPv6 address, match ephemeral bind family"
```

---

### Task 9: E2E — Happy Eyeballs tests

**Files:**
- Modify: `tests/e2e.rs`

- [ ] **Step 1: Write Happy Eyeballs fallback test**

Bind an IPv4-only listener. Connect with `vibecat localhost <port>` (dual-stack). IPv6 attempt to `[::1]` fails because nothing listens there; falls back to IPv4 and succeeds.

Add to `tests/e2e.rs`:

```rust
#[test]
fn happy_eyeballs_falls_back_to_ipv4() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut got = Vec::new();
        stream.read_to_end(&mut got).unwrap();
        got
    });

    let mut child = Command::new(bin())
        .args(["localhost", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"happy-eyeballs-v4")
        .unwrap();

    let received = server.join().unwrap();
    child.wait().unwrap();
    assert_eq!(received, b"happy-eyeballs-v4");
}
```

- [ ] **Step 2: Write Happy Eyeballs prefer-IPv6 test**

Bind an IPv6-only listener. Connect with `vibecat localhost <port>`. IPv6 connects first within the 250ms head-start.

Add to `tests/e2e.rs`:

```rust
#[test]
fn happy_eyeballs_prefers_ipv6() {
    let listener = TcpListener::bind("[::1]:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut got = Vec::new();
        stream.read_to_end(&mut got).unwrap();
        got
    });

    let mut child = Command::new(bin())
        .args(["localhost", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"happy-eyeballs-v6")
        .unwrap();

    let received = server.join().unwrap();
    child.wait().unwrap();
    assert_eq!(received, b"happy-eyeballs-v6");
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Commit**

```bash
git add tests/e2e.rs
git commit -m "Add e2e tests for Happy Eyeballs fallback and IPv6 preference"
```

---

### Task 10: Update README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update usage table with new flags**

In the flags table in `README.md`, add rows for `-4`, `-6`, and `-v`:

```
| Flag           | Meaning                                            |
|----------------|----------------------------------------------------|
| `-l, --listen` | Listen mode (server). Without it: client mode.     |
| `-u, --udp`    | Use UDP instead of TCP.                            |
| `-4, --ipv4`   | Use IPv4 only.                                     |
| `-6, --ipv6`   | Use IPv6 only.                                     |
| `-v, --verbose` | Print diagnostic messages to stderr.              |
| `-h, --help`   | Print help.                                        |
| `-V, --version`| Print version.                                     |
```

- [ ] **Step 2: Update scope section**

Remove "verbose/hex-dump output" from the out-of-scope list (verbose was implemented). Update the list to remove items that are now implemented:

```
This is a minimal MVP. Out of scope (for now): source-port selection (`-p`),
timeouts (`-w`), keep-listening for multiple clients (`-k`), command execution
(`-e`/`-c`), port scanning, hex-dump output, and TLS.
```

- [ ] **Step 3: Add behavior note about IPv6**

Add a bullet to the "Behavior notes" section:

```
- **Dual-stack by default.** In listen mode, vibecat binds both IPv6 and IPv4
  sockets. In connect mode, it uses Happy Eyeballs (RFC 8305) to prefer IPv6
  with an IPv4 fallback. Use `-4` or `-6` to restrict to one address family.
```

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "Update README with IPv6 dual-stack support and new flags"
```
