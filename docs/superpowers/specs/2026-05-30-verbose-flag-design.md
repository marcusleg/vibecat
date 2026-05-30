# vibecat — Verbose Flag (`-v`) Design

**Date:** 2026-05-30
**Status:** Draft

## Overview

Add a `-v` / `--verbose` flag that prints diagnostic messages to stderr when
connections are established or when listening begins. Messages use ANSI colors
when stderr is a TTY (auto-disabled otherwise). This brings vibecat in line
with standard netcat behavior, where verbose output goes to stderr to keep
stdout clean for data.

## Goals

- Print a connection-established message showing hostname (if applicable),
  resolved IP, destination port, protocol, and source address/port.
- Print a listening message showing bind address, port, and protocol.
- Use colors to make messages scannable: green for success verbs, bold cyan
  for key identifiers, plain text for everything else.
- Auto-disable colors when stderr is not a TTY, and respect `NO_COLOR` /
  `FORCE_COLOR` environment variables.

## Non-Goals

- Multi-level verbosity (`-vv`, `-vvv`). Single boolean on/off only.
- Verbose output during data transfer (byte counts, throughput, etc.).
- Color configuration flags (`--color=always/auto/never`). May be added later
  but is out of scope here.

## Message Format

### Connection Established (connect mode and listen mode after accept)

When a hostname was provided:

```
vibecat: connected to example.com (93.184.216.34) on port 80 (tcp) from 192.168.1.5 on port 54321
```

When an IP was provided directly (no hostname to show):

```
vibecat: connected to 93.184.216.34 on port 80 (tcp) from 192.168.1.5 on port 54321
```

UDP variant:

```
vibecat: connected to example.com (93.184.216.34) on port 53 (udp) from 192.168.1.5 on port 41022
```

### Listening

```
vibecat: listening on 0.0.0.0 on port 9999 (tcp)
```

With explicit bind address:

```
vibecat: listening on 127.0.0.1 on port 9999 (udp)
```

### Color Scheme

All verbose messages are written to stderr.

| Element                              | Style     |
|--------------------------------------|-----------|
| Success verb ("connected", "listening") | Green     |
| Hostname, IPs, port numbers (before "from") | Bold cyan |
| "from" and everything after it       | Default   |
| Connective words ("to", "on port", parens, protocol) | Default   |
| "vibecat:" prefix                    | Default   |

Colors are applied conditionally using `owo_colors`'s `if_supports_color`
with `Stream::Stderr`, so they are automatically stripped when stderr is
redirected to a file or pipe, or when `NO_COLOR` is set.

## Dependencies

Add `owo-colors` with the `supports-colors` feature. This provides:

- Trait-based coloring API (`OwoColorize`)
- Per-stream TTY detection (`Stream::Stderr`)
- `NO_COLOR` / `FORCE_COLOR` support

## Architecture

### New module: `verbose.rs`

Two public functions:

- `log_connected(config, local_addr, remote_addr)` — formats and prints the
  connection message. No-op if `config.verbose` is false.
- `log_listening(config, bind_addr)` — formats and prints the listening
  message. No-op if `config.verbose` is false.

Both functions determine whether to show a hostname by comparing
`config.host` against the resolved IP string. If they match (user passed a
raw IP), the hostname is omitted; if they differ (user passed a hostname),
both are shown as `hostname (ip)`.

### CLI changes (`cli.rs`)

Add `-v, --verbose` boolean flag to `RawArgs`. Propagate into `Config` as a
`verbose: bool` field.

### Net changes (`net.rs`)

Extend return types to include address information:

- `connect()` returns `(Conn, SocketAddr)` — the connection and the local
  socket address (source IP and ephemeral port).
- `listen()` is split into two steps so that `run()` can log "listening"
  before blocking on accept:
  1. A bind step that returns the bound `SocketAddr` (for the listening
     message) and an intermediate listener value.
  2. An accept step that blocks and returns `(Conn, Option<Vec<u8>>,
     SocketAddr, SocketAddr)` — the connection, optional first UDP datagram,
     local address, and peer address.

The verbose functions are NOT called inside `net.rs`. Address information is
returned to the caller.

### Main changes (`main.rs`)

`run()` calls verbose functions at two points:

1. **Listen mode:** call `log_listening()` immediately after binding, before
   blocking on accept. Then call `log_connected()` after accept returns.
2. **Connect mode:** call `log_connected()` after `connect()` returns
   successfully.

Both calls happen before `pump_bidirectional()` starts.

### No changes to `io.rs`

The pump remains purely a byte mover with no awareness of verbosity.

## Testing Strategy

### Unit tests

- `cli.rs`: verify `-v` flag parses into `Config { verbose: true }`, and that
  absence leaves it `false`.
- `verbose.rs`: call formatting helpers with known addresses and assert the
  output string contains expected substrings (hostname, IP, port, protocol).
  Test both hostname-present and IP-only cases.

### Integration tests (`tests/e2e.rs`)

- Run `vibecat -v -l <port>` and a client, capture stderr, assert it contains
  "listening" and "connected" messages with the correct port and protocol.
- Run `vibecat -v <host> <port>` against a local listener, capture stderr,
  assert the connected message appears.
- Verify that without `-v`, stderr is empty (no diagnostic output).

## File Summary

| File         | Change                                              |
|--------------|-----------------------------------------------------|
| `Cargo.toml` | Add `owo-colors` dependency with `supports-colors`  |
| `cli.rs`     | Add `-v` / `--verbose` flag and `Config.verbose`    |
| `verbose.rs` | New module: `log_connected`, `log_listening`         |
| `net.rs`     | Return `SocketAddr` info from `connect` and `listen` |
| `main.rs`    | Wire up verbose calls in `run()`, declare `mod verbose` |
| `tests/e2e.rs` | Add verbose output integration tests              |
