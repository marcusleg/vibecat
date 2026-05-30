# Verbose Flag (`-v`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `-v` / `--verbose` flag that prints colored diagnostic messages to stderr when connections are established or when listening begins.

**Architecture:** A new `verbose.rs` module owns all formatting and coloring logic, exposing two functions (`log_connected`, `log_listening`) that are no-ops when verbose is off. `net.rs` returns address metadata alongside connections. `main.rs` calls the verbose functions at the right moments.

**Tech Stack:** Rust, clap (existing), owo-colors 4 with `supports-colors` feature (new)

**Spec:** `docs/superpowers/specs/2026-05-30-verbose-flag-design.md`

---

## File Map

| File | Role | Change |
|------|------|--------|
| `Cargo.toml` | Dependencies | Add `owo-colors` |
| `src/cli.rs` | Argument parsing | Add `-v` flag to `RawArgs` and `Config` |
| `src/verbose.rs` | Verbose output (new) | `log_connected`, `log_listening` with colored formatting |
| `src/net.rs` | Connection setup | Split `listen()` into bind + accept, return `SocketAddr`s from both paths |
| `src/main.rs` | Entry point | Wire up verbose calls in `run()` |
| `tests/e2e.rs` | Integration tests | Verbose output assertions |

---

### Task 1: Add `owo-colors` dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add the dependency**

In `Cargo.toml`, add `owo-colors` under `[dependencies]`:

```toml
[dependencies]
clap = { version = "4.6.1", features = ["derive"] }
owo-colors = { version = "4", features = ["supports-colors"] }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "Add owo-colors dependency for colored verbose output"
```

---

### Task 2: Add `-v` / `--verbose` flag to CLI

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Write failing tests for the verbose flag**

Add these tests at the end of the `mod tests` block in `src/cli.rs`:

```rust
#[test]
fn verbose_flag_short() {
    let c = Config::from_args(["vibecat", "-v", "example.com", "80"]).unwrap();
    assert!(c.verbose);
}

#[test]
fn verbose_flag_long() {
    let c = Config::from_args(["vibecat", "--verbose", "-l", "8080"]).unwrap();
    assert!(c.verbose);
}

#[test]
fn verbose_defaults_to_false() {
    let c = Config::from_args(["vibecat", "example.com", "80"]).unwrap();
    assert!(!c.verbose);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli::tests`
Expected: all three new tests fail — `Config` has no `verbose` field.

- [ ] **Step 3: Add verbose to `RawArgs` and `Config`**

In `src/cli.rs`, add the verbose flag to `RawArgs`:

```rust
struct RawArgs {
    /// Listen for an incoming connection instead of connecting out.
    #[arg(short = 'l', long = "listen")]
    listen: bool,

    /// Use UDP instead of TCP.
    #[arg(short = 'u', long = "udp")]
    udp: bool,

    /// Print diagnostic messages to stderr.
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,

    /// Positional arguments: `[host] <port>`.
    #[arg(value_name = "ARGS")]
    positionals: Vec<String>,
}
```

Add `verbose: bool` to the `Config` struct:

```rust
pub struct Config {
    pub mode: Mode,
    pub proto: Proto,
    /// `None` only in listen mode with no explicit bind address (defaults later).
    pub host: Option<String>,
    pub port: u16,
    pub verbose: bool,
}
```

In `from_raw`, propagate the field — add `verbose: raw.verbose` to the `Ok(Config { ... })` return:

```rust
Ok(Config { mode, proto, host, port, verbose: raw.verbose })
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib cli::tests`
Expected: all tests pass, including the three new ones.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "Add -v/--verbose flag to CLI"
```

---

### Task 3: Create `verbose.rs` with message formatting

**Files:**
- Create: `src/verbose.rs`

This task creates the module with formatting helpers and unit tests. The public functions `log_connected` and `log_listening` will be wired up in a later task. For now they are defined and tested in isolation.

- [ ] **Step 1: Write the `verbose.rs` module with tests**

Create `src/verbose.rs` with the following content:

```rust
//! Colored diagnostic messages for verbose mode.

use std::net::SocketAddr;

use owo_colors::{OwoColorize, Stream};

use crate::cli::{Config, Proto};

/// Print a "Listening on ..." message to stderr. No-op if verbose is off.
pub fn log_listening(config: &Config, bind_addr: SocketAddr) {
    if !config.verbose {
        return;
    }
    let proto = proto_str(config.proto);
    eprintln!(
        "vibecat: {} on {} on port {} ({proto}).",
        "Listening".if_supports_color(Stream::Stderr, |t| t.green()),
        bind_addr
            .ip()
            .if_supports_color(Stream::Stderr, |t| t.bold().cyan()),
        bind_addr
            .port()
            .if_supports_color(Stream::Stderr, |t| t.bold().cyan()),
    );
}

/// Print a "Connected to ..." message to stderr. No-op if verbose is off.
///
/// If `config.host` differs from the resolved IP in `remote_addr`, both are
/// shown as `hostname (ip)`. If they match (user passed a raw IP), only the
/// IP is shown.
pub fn log_connected(config: &Config, local_addr: SocketAddr, remote_addr: SocketAddr) {
    if !config.verbose {
        return;
    }
    let proto = proto_str(config.proto);
    let remote_ip = remote_addr.ip().to_string();

    let destination = format_destination(config.host.as_deref(), &remote_ip);

    eprintln!(
        "vibecat: {} to {destination} on port {} ({proto}) from {} on port {}.",
        "Connected".if_supports_color(Stream::Stderr, |t| t.green()),
        remote_addr
            .port()
            .if_supports_color(Stream::Stderr, |t| t.bold().cyan()),
        local_addr.ip(),
        local_addr.port(),
    );
}

/// Format the destination as `hostname (ip)` or just `ip`.
fn format_destination(host: Option<&str>, resolved_ip: &str) -> String {
    match host {
        Some(h) if h != resolved_ip => format!(
            "{} ({})",
            h.if_supports_color(Stream::Stderr, |t| t.bold().cyan()),
            resolved_ip.if_supports_color(Stream::Stderr, |t| t.bold().cyan()),
        ),
        _ => format!(
            "{}",
            resolved_ip.if_supports_color(Stream::Stderr, |t| t.bold().cyan()),
        ),
    }
}

fn proto_str(proto: Proto) -> &'static str {
    match proto {
        Proto::Tcp => "tcp",
        Proto::Udp => "udp",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Mode;
    use std::net::{IpAddr, Ipv4Addr};

    fn cfg(verbose: bool, proto: Proto, host: Option<&str>) -> Config {
        Config {
            mode: Mode::Connect,
            proto,
            host: host.map(String::from),
            port: 80,
            verbose,
        }
    }

    fn addr(ip: [u8; 4], port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3])), port)
    }

    #[test]
    fn format_destination_with_hostname() {
        owo_colors::set_override(false);
        let result = format_destination(Some("example.com"), "93.184.216.34");
        assert_eq!(result, "example.com (93.184.216.34)");
        owo_colors::unset_override();
    }

    #[test]
    fn format_destination_ip_only() {
        owo_colors::set_override(false);
        let result = format_destination(Some("93.184.216.34"), "93.184.216.34");
        assert_eq!(result, "93.184.216.34");
        owo_colors::unset_override();
    }

    #[test]
    fn format_destination_no_host() {
        owo_colors::set_override(false);
        let result = format_destination(None, "93.184.216.34");
        assert_eq!(result, "93.184.216.34");
        owo_colors::unset_override();
    }

    #[test]
    fn log_connected_is_noop_when_not_verbose() {
        // Should not panic or produce output.
        let config = cfg(false, Proto::Tcp, Some("example.com"));
        log_connected(
            &config,
            addr([192, 168, 1, 5], 54321),
            addr([93, 184, 216, 34], 80),
        );
    }

    #[test]
    fn log_listening_is_noop_when_not_verbose() {
        let config = cfg(false, Proto::Tcp, None);
        log_listening(&config, addr([0, 0, 0, 0], 9999));
    }
}
```

- [ ] **Step 2: Register the module in `main.rs`**

Add `mod verbose;` after the existing module declarations in `src/main.rs`, so the top looks like:

```rust
mod cli;
mod io;
mod net;
mod verbose;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib verbose::tests`
Expected: all 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/verbose.rs src/main.rs
git commit -m "Add verbose module with colored message formatting"
```

---

### Task 4: Refactor `net.rs` to return address metadata

**Files:**
- Modify: `src/net.rs`
- Modify: `src/main.rs` (update call sites temporarily to keep compiling)

This task changes the return types of `connect()` and splits `listen()` into `bind()` + `accept()`, so the caller gets the `SocketAddr` information it needs for verbose logging.

- [ ] **Step 1: Update `connect()` to return `(Conn, SocketAddr)`**

In `src/net.rs`, change `connect` to return the local socket address:

```rust
/// Connect out to the remote described by `config` (client mode).
pub fn connect(config: &Config) -> io::Result<(Conn, SocketAddr)> {
    let host = config.host.as_deref().unwrap_or("127.0.0.1");
    let port = config.port;
    match config.proto {
        Proto::Tcp => {
            let stream = TcpStream::connect((host, port))?;
            let local = stream.local_addr()?;
            Ok((Conn::Tcp(stream), local))
        }
        Proto::Udp => {
            let socket = UdpSocket::bind("0.0.0.0:0")?;
            socket.connect((host, port))?;
            let local = socket.local_addr()?;
            Ok((Conn::Udp(UdpStream::new(socket)), local))
        }
    }
}
```

Add `SocketAddr` to the imports at the top of `net.rs`:

```rust
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
```

- [ ] **Step 2: Replace `listen()` with `bind()` and `accept()`**

Remove the existing `listen()` function and replace it with two new functions:

```rust
/// An intermediate listener, waiting to accept a connection.
pub enum Listener {
    Tcp(TcpListener),
    Udp(UdpSocket),
}

/// Bind to the listen address (server mode). Returns the listener and the
/// bound address so the caller can log it before blocking on accept.
pub fn bind(config: &Config) -> io::Result<(Listener, SocketAddr)> {
    let host = config.host.as_deref().unwrap_or("0.0.0.0");
    let port = config.port;
    match config.proto {
        Proto::Tcp => {
            let listener = TcpListener::bind((host, port))?;
            let addr = listener.local_addr()?;
            Ok((Listener::Tcp(listener), addr))
        }
        Proto::Udp => {
            let socket = UdpSocket::bind((host, port))?;
            let addr = socket.local_addr()?;
            Ok((Listener::Udp(socket), addr))
        }
    }
}

/// Accept a single connection from a bound listener.
///
/// Returns the connection, optional first UDP datagram payload, local
/// address, and peer address.
pub fn accept(listener: Listener) -> io::Result<(Conn, Option<Vec<u8>>, SocketAddr, SocketAddr)> {
    match listener {
        Listener::Tcp(l) => {
            let (stream, peer) = l.accept()?;
            let local = stream.local_addr()?;
            Ok((Conn::Tcp(stream), None, local, peer))
        }
        Listener::Udp(socket) => {
            let mut buf = vec![0u8; 64 * 1024];
            let (n, peer) = socket.recv_from(&mut buf)?;
            socket.connect(peer)?;
            let local = socket.local_addr()?;
            buf.truncate(n);
            Ok((Conn::Udp(UdpStream::new(socket)), Some(buf), local, peer))
        }
    }
}
```

- [ ] **Step 3: Update `main.rs` `run()` to use the new signatures**

Update `run()` in `src/main.rs` to use the new `connect`, `bind`, and `accept` functions. For now, ignore the address values — just make it compile. We'll add the verbose calls in the next task.

```rust
/// Establish the connection and pump data until the remote closes.
fn run(config: &Config) -> std::io::Result<()> {
    let (conn, initial) = match config.mode {
        Mode::Connect => {
            let (conn, _local_addr) = net::connect(config)?;
            (conn, None)
        }
        Mode::Listen => {
            let (listener, _bind_addr) = net::bind(config)?;
            let (conn, initial, _local_addr, _peer_addr) = net::accept(listener)?;
            (conn, initial)
        }
    };

    if let Some(bytes) = initial {
        let mut stdout = std::io::stdout();
        stdout.write_all(&bytes)?;
        stdout.flush()?;
    }

    pump_bidirectional(conn)
}
```

- [ ] **Step 4: Update `net.rs` unit tests**

The existing `tcp_connect_and_listen_exchange_bytes` test calls `connect()` and needs to destructure the new return type. The `udp_listen_returns_first_datagram_payload` test calls `listen()` which no longer exists — update it to call `bind()` + `accept()`.

Replace the entire `#[cfg(test)] mod tests` block in `src/net.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Mode;
    use std::thread;

    fn cfg(mode: Mode, proto: Proto, host: Option<&str>, port: u16) -> Config {
        Config { mode, proto, host: host.map(String::from), port, verbose: false }
    }

    #[test]
    fn tcp_connect_and_listen_exchange_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 16];
            let n = stream.read(&mut buf).unwrap();
            stream.write_all(&buf[..n]).unwrap();
        });

        let client_cfg = cfg(Mode::Connect, Proto::Tcp, Some("127.0.0.1"), port);
        let (mut conn, local_addr) = connect(&client_cfg).unwrap();
        assert_eq!(local_addr.ip().to_string(), "127.0.0.1");
        assert_ne!(local_addr.port(), 0);
        conn.write_all(b"hi").unwrap();
        let mut buf = [0u8; 16];
        let n = conn.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"hi");
        server.join().unwrap();
    }

    #[test]
    fn udp_bind_accept_returns_first_datagram_payload() {
        let server_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = server_socket.local_addr().unwrap().port();
        drop(server_socket);

        let listen_cfg = cfg(Mode::Listen, Proto::Udp, Some("127.0.0.1"), port);
        let server = thread::spawn(move || {
            let (listener, bind_addr) = bind(&listen_cfg).unwrap();
            assert_eq!(bind_addr.port(), port);
            accept(listener).unwrap()
        });

        thread::sleep(std::time::Duration::from_millis(100));
        let client = UdpSocket::bind("0.0.0.0:0").unwrap();
        client.send_to(b"first", ("127.0.0.1", port)).unwrap();

        let (_conn, initial, _local, peer) = server.join().unwrap();
        assert_eq!(initial.as_deref(), Some(&b"first"[..]));
        assert_ne!(peer.port(), 0);
    }
}
```

- [ ] **Step 5: Run all tests to verify everything passes**

Run: `cargo test`
Expected: all unit and integration tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/net.rs src/main.rs
git commit -m "Return address metadata from connect/bind/accept"
```

---

### Task 5: Wire up verbose calls in `main.rs`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add verbose calls to `run()`**

Replace the `run()` function in `src/main.rs`:

```rust
/// Establish the connection and pump data until the remote closes.
fn run(config: &Config) -> std::io::Result<()> {
    let (conn, initial) = match config.mode {
        Mode::Connect => {
            let (conn, local_addr) = net::connect(config)?;
            let remote_addr = conn.peer_addr()?;
            verbose::log_connected(config, local_addr, remote_addr);
            (conn, None)
        }
        Mode::Listen => {
            let (listener, bind_addr) = net::bind(config)?;
            verbose::log_listening(config, bind_addr);
            let (conn, initial, _local_addr, peer_addr) = net::accept(listener)?;
            verbose::log_connected(config, bind_addr, peer_addr);
            (conn, initial)
        }
    };

    if let Some(bytes) = initial {
        let mut stdout = std::io::stdout();
        stdout.write_all(&bytes)?;
        stdout.flush()?;
    }

    pump_bidirectional(conn)
}
```

- [ ] **Step 2: Add `peer_addr()` to `Conn`**

The connect-mode path needs `conn.peer_addr()` to get the remote address for the verbose message. Add this method to `Conn` in `src/net.rs`:

```rust
impl Conn {
    /// Get the remote address of the connection. TCP returns the peer;
    /// UDP returns the connected peer.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        match self {
            Conn::Tcp(s) => s.peer_addr(),
            Conn::Udp(s) => s.peer_addr(),
        }
    }

    // ... existing methods ...
}
```

And add `peer_addr` to `UdpStream` in `src/io.rs`:

```rust
impl UdpStream {
    // ... existing methods ...

    /// Get the remote address this socket is connected to.
    pub fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.socket.peer_addr()
    }
}
```

- [ ] **Step 3: Run all tests to verify everything passes**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs src/net.rs src/io.rs
git commit -m "Wire up verbose logging in run()"
```

---

### Task 6: Integration tests for verbose output

**Files:**
- Modify: `tests/e2e.rs`

- [ ] **Step 1: Add verbose integration tests**

Add the following tests at the end of `tests/e2e.rs`:

```rust
#[test]
fn verbose_connect_prints_connected_to_stderr() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        drop(stream);
    });

    let child = Command::new(bin())
        .args(["-v", "127.0.0.1", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    server.join().unwrap();

    assert!(
        stderr.contains("Connected"),
        "stderr should contain 'Connected', got: {stderr}"
    );
    assert!(
        stderr.contains(&port.to_string()),
        "stderr should contain the port, got: {stderr}"
    );
    assert!(stderr.contains("(tcp)"), "stderr should contain '(tcp)', got: {stderr}");
}

#[test]
fn verbose_listen_prints_listening_and_connected_to_stderr() {
    let port = free_tcp_port();

    let mut server = Command::new(bin())
        .args(["-v", "-l", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(200));

    let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    client.shutdown(std::net::Shutdown::Both).unwrap();
    drop(client);

    // Close server stdin so the send pump finishes.
    drop(server.stdin.take());

    let output = server.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Listening"),
        "stderr should contain 'Listening', got: {stderr}"
    );
    assert!(
        stderr.contains("Connected"),
        "stderr should contain 'Connected', got: {stderr}"
    );
    assert!(
        stderr.contains(&port.to_string()),
        "stderr should contain the port, got: {stderr}"
    );
}

#[test]
fn no_verbose_flag_produces_no_stderr() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        drop(stream);
    });

    let child = Command::new(bin())
        .args(["127.0.0.1", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    server.join().unwrap();

    assert!(
        stderr.is_empty(),
        "stderr should be empty without -v, got: {stderr}"
    );
}
```

- [ ] **Step 2: Run the integration tests**

Run: `cargo test --test e2e`
Expected: all tests pass (including the existing ones and the three new ones).

- [ ] **Step 3: Run the full test suite**

Run: `cargo test`
Expected: all unit and integration tests pass.

- [ ] **Step 4: Commit**

```bash
git add tests/e2e.rs
git commit -m "Add integration tests for verbose output"
```

---

### Task 7: Manual smoke test

**Files:** none (verification only)

- [ ] **Step 1: Build the binary**

Run: `cargo build`

- [ ] **Step 2: Test verbose TCP listen + connect**

In one terminal:
```bash
cargo run -- -v -l 9999
```
Expected on stderr: `vibecat: Listening on 0.0.0.0 on port 9999 (tcp).`

In another terminal:
```bash
echo "hello" | cargo run -- -v 127.0.0.1 9999
```
Expected on stderr of client: `vibecat: Connected to 127.0.0.1 on port 9999 (tcp) from 127.0.0.1 on port <ephemeral>.`
Expected on stderr of server: `vibecat: Connected to 127.0.0.1 on port 9999 (tcp) from 127.0.0.1 on port <ephemeral>.`

Verify colors appear when running interactively and are absent when piping stderr:
```bash
cargo run -- -v 127.0.0.1 9999 2>/tmp/stderr.txt
cat /tmp/stderr.txt
```
Expected: no ANSI escape codes in the file.

- [ ] **Step 3: Test verbose UDP**

```bash
cargo run -- -v -l -u 9999       # terminal 1
echo "ping" | cargo run -- -v -u 127.0.0.1 9999   # terminal 2
```
Expected: messages show `(udp)` instead of `(tcp)`.

- [ ] **Step 4: Test without verbose**

```bash
cargo run -- -l 9999
```
Expected: no diagnostic output on stderr at all.
