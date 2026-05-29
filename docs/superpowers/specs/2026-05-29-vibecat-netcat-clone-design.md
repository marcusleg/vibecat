# vibecat — A Minimal Netcat Clone in Rust

**Date:** 2026-05-29
**Status:** Approved design

## Overview

`vibecat` is a minimal netcat clone written in Rust. It connects to a remote
host (client mode) or accepts a single connection (listen mode), then pipes data
bidirectionally between standard I/O and the network socket. It supports both TCP
and UDP.

The design favors readable, idiomatic code over exhaustive flag coverage. It uses
the standard library for networking and a simple two-thread blocking I/O model,
true to the original netcat's style. The one external dependency is `clap` for
argument parsing.

## Goals

- Connect mode: `vibecat <host> <port>` pipes stdin/stdout over a TCP or UDP socket.
- Listen mode: `vibecat -l <port>` accepts one connection and pipes data.
- Support both TCP (default) and UDP (`-u`).
- Honor half-close on stdin EOF: stop sending but keep receiving (TCP).
- Stay minimal and dependency-light (std lib + `clap` only).

## Non-Goals (Out of Scope for MVP)

YAGNI — explicitly excluded:

- `-p` source port selection
- `-w` connection/idle timeout
- `-k` keep listening for multiple/repeated clients
- `-e` / `-c` command execution (gaping security hole; never)
- Port scanning
- `-v` verbosity levels / hex dump output
- Any TLS/SSL support
- Listen mode serves exactly **one** client, then the process exits.

## Architecture

A single binary crate. Four small modules, each with one responsibility:

```
src/
  main.rs    — entry point: parse args, dispatch to mode, wire up the pumps
  cli.rs     — clap arg definition + parsed Config struct + validation
  io.rs      — the generic pump() function and the UdpStream adapter
  net.rs     — open the connection: connect (client) or listen+accept (server)
```

### Data Flow

Once a connection exists (either mode), two threads move bytes:

```
        stdin  ──pump──▶  socket (send half)      [Thread 1]
        stdout ◀──pump──  socket (recv half)      [Thread 2]
```

`main` obtains a connection object implementing `Read + Write`, clones it into a
read handle and a write handle, and spawns two threads each running `pump`. The
process exits when **Thread 2** (socket → stdout) finishes, which is what makes
"keep receiving after stdin EOF" work.

### Module Boundaries

- `net.rs` decides **what** the connection is (TCP/UDP, client/server).
- `io.rs` only knows how to move bytes between any `Read` and any `Write`.
- `cli.rs` is pure parsing/validation with no I/O.

Each module is testable in isolation.

## Connection Setup (`net.rs`)

One entry point per role. Returns a connection that implements
`Read + Write + Send` so `main` is agnostic to the concrete type.

### Client / Connect Mode

- **TCP:** `TcpStream::connect((host, port))`. If a hostname resolves to multiple
  addresses (IPv4 + IPv6), connect attempts them in order until one succeeds
  (std's `ToSocketAddrs` + connect behavior).
- **UDP:** Bind a local `UdpSocket` to an ephemeral port (`0.0.0.0:0`), then
  `connect()` it to the remote so `send`/`recv` work without per-call addresses.
  Wrap in `UdpStream`. Note: UDP has no handshake — "connection" succeeds even if
  nothing is listening; the first data exchange reveals reachability.

### Server / Listen Mode (`-l`)

- **TCP:** `TcpListener::bind((host, port))`, then `accept()` exactly one
  connection. Use the accepted `TcpStream`. After the session ends, the process
  exits (no repeat accept).
- **UDP:** Bind a `UdpSocket` to the listen address. There is no `accept` for UDP.
  Perform a first `recv_from` to learn the peer's address, then `connect()` back
  to that peer so subsequent traffic is bound to it, and wrap in `UdpStream`. The
  payload of that **first datagram must be preserved** and delivered to stdout —
  it is not dropped during peer discovery.

### Address Handling

`host:port` resolved via std's `ToSocketAddrs`. In listen mode the host is
optional and defaults to `0.0.0.0`.

### UDP First-Datagram Handling

`UdpStream` exposes an `accept_first()`-style constructor for listen mode that
returns both the adapter and the initial bytes received during peer discovery, so
no data is lost between binding and starting the pump loop.

## I/O Pump and UDP Adapter (`io.rs`)

### `pump` — the byte mover

```rust
fn pump<R: Read, W: Write>(mut reader: R, mut writer: W) -> io::Result<()>
```

Loop: read into a fixed 8 KiB buffer; break on `Ok(0)` (EOF); write all bytes
read; flush. It knows nothing about sockets, stdin, or TCP vs UDP — which makes it
trivially testable against in-memory buffers.

### Half-Close Coordination (the EOF requirement)

- **Thread 1** runs `pump(stdin, socket_write_handle)`. When stdin hits EOF the
  pump returns; we then call `shutdown(Shutdown::Write)` on the TCP socket to send
  FIN so the remote sees our EOF — but we keep receiving.
- **Thread 2** runs `pump(socket_read_handle, stdout)`. It runs until the remote
  closes (read returns 0).
- The process exits when **both** pumps finish. Thread 2 runs on the main thread;
  after it returns (peer FIN), `main` joins Thread 1 so any buffered stdin data is
  flushed to the socket before exit. This makes both half-closes symmetric: our
  stdin EOF stops sending while we keep receiving, and the peer's FIN stops
  receiving while we keep sending. (An earlier draft exited as soon as Thread 2
  finished; that dropped a server-mode reply that was still being written by
  Thread 1, so the design waits for both.)
- **Caveat — early FIN and real servers:** sending FIN on stdin EOF is standard
  netcat behavior, but some servers/CDNs (e.g. example.com) abort the connection
  when they receive a client FIN before they have responded. This is not a vibecat
  bug — the system `nc`/`ncat` and a raw socket doing `shutdown(SHUT_WR)` behave
  identically. A future `--no-shutdown` flag (matching ncat) would suppress the
  FIN for these peers; it is out of scope for the MVP.
- **UDP:** No `shutdown`/FIN semantics. stdin EOF simply ends Thread 1. Thread 2's
  `recv` blocks waiting for more datagrams with no natural end-of-stream signal.
  This matches real `nc -u`: the user exits with Ctrl-C / process termination.
  No special UDP teardown logic is implemented.

### Getting Two Handles to One Socket

- **TCP:** `TcpStream::try_clone()` yields an independent handle; both point at the
  same underlying socket, and `shutdown` on either affects the connection.
- **UDP:** `UdpStream` wraps a `UdpSocket`; clone the socket via `try_clone()`.

### `UdpStream` Adapter

Implements `Read` and `Write` over a connected `UdpSocket`:

- `read` → `recv` (one datagram per call, into the provided buffer)
- `write` → `send` (one datagram per call)

Because it implements `Read + Write`, `pump` treats it identically to `TcpStream`.

## CLI (`cli.rs`)

Built with `clap`.

```
vibecat [OPTIONS] <host> <port>      # client: connect to host:port
vibecat -l [OPTIONS] <port>          # server: listen on port
                                     # (host optional in listen mode; default 0.0.0.0)
```

| Flag           | Meaning                                            |
|----------------|----------------------------------------------------|
| `-l, --listen` | Listen mode (server). Without it: client mode.     |
| `-u, --udp`    | Use UDP instead of TCP.                            |
| `<host>`       | Hostname/IP. Required in client mode; optional bind address in listen mode. |
| `<port>`       | Port number. Required.                             |

clap parses into:

```rust
struct Config {
    mode: Mode,        // Connect | Listen
    proto: Proto,      // Tcp | Udp
    host: Option<String>,
    port: u16,
}
```

Validation that clap can't express directly (e.g. host required unless `-l`) lives
in `Config::validate()`, returning a clear error. clap provides `--help` and
`--version` automatically.

### Positional Disambiguation

Both positionals are defined as optional at the clap level (`[host] [port]`) and
resolved by `validate()` based on the count and mode:

- **Client mode** (no `-l`): exactly two positionals required → `<host> <port>`.
  Zero or one positional is an error.
- **Listen mode** (`-l`): one positional → it is the `<port>`, host defaults to
  `0.0.0.0`. Two positionals → `<host> <port>` (explicit bind address). Zero
  positionals is an error.

The last positional is always the port and must parse as a `u16`; a non-numeric or
out-of-range port is a clear error.

## Error Handling

- Functions return `io::Result` (or a small error type) up to `main`.
- `main` prints `vibecat: <message>` to stderr and exits with a non-zero code on
  expected failures (connection refused, address in use, unresolvable host, bad
  arguments). No panics on expected failures.
- Broken pipe / remote reset mid-transfer is treated as a normal end-of-session:
  quiet teardown, exit code 0 (matches nc behavior).

## Testing Strategy

### Unit Tests

- `pump()` against in-memory `Read`/`Write` buffers: copies all bytes, stops at
  EOF, flushes.
- `Config` parsing and `validate()` against argument vectors (valid and invalid
  combinations).
- `UdpStream` read/write against a loopback `UdpSocket` pair.

### Integration Tests

- Spin up `vibecat -l` in a thread and a client `vibecat` against it on a loopback
  ephemeral port; assert bytes pipe end-to-end for both TCP and UDP.
- Test the half-close case against a local server that replies *after* a delay:
  pipe input in, close stdin, and confirm the delayed response is still received
  (proves the receive side stays open past stdin EOF).
- Note: do not use `example.com:80` as a half-close test target — that CDN aborts
  on an early client FIN, so it returns nothing here just as it does for real
  `nc`. Use a local server (or `--no-shutdown`-style behavior) for the manual
  interop smoke test instead.

## Implementation Notes

- Dependencies: `clap` only. Everything else from `std`.
- Buffer size: 8 KiB for the pump loop.
- Working binary name: `vibecat`.
