# vibecat — IPv6 and Dual-Stack Support

**Date:** 2026-05-30
**Status:** Approved design

## Overview

vibecat currently hardcodes IPv4 addresses everywhere: listen defaults to
`0.0.0.0`, connect defaults to `127.0.0.1`, and the UDP ephemeral bind uses
`0.0.0.0:0`. This design adds full IPv6 support with dual-stack as the default
behavior, Happy Eyeballs (RFC 8305) for connect mode, and `-4`/`-6` flags to
force a specific address family.

## Goals

- Listen mode binds two sockets (IPv4 + IPv6) by default, accepting whichever
  gets a connection first.
- Connect mode uses Happy Eyeballs: prefer IPv6 with a 250ms head-start, race
  IPv4 in parallel, use whichever connects first.
- `-4` and `-6` flags restrict to a single address family.
- Verbose output includes the address family: `(IPv6/TCP)`, `(IPv4/UDP)`, etc.
- New dependency: `socket2` for `IPV6_V6ONLY` and `connect_timeout`.

## Non-Goals

- `-k` keep-listening for multiple clients (but the dual-socket channel
  architecture laid here supports it in the future).
- Dual-stack via a single `IPV6_V6ONLY=false` socket (not portable).
- Async I/O runtime.

## CLI Changes

Two new mutually exclusive flags in `cli.rs`:

```
-4, --ipv4    Use IPv4 only
-6, --ipv6    Use IPv6 only
```

Passing both is an error. When neither is set, behavior is dual-stack.

New type:

```rust
enum AddrFamily {
    Both,   // default
    Ipv4,   // -4
    Ipv6,   // -6
}
```

`Config` gains an `addr_family: AddrFamily` field. Existing flags, positionals,
and validation logic are unchanged.

## Listen Mode — Dual-Socket TCP

When `addr_family` is `Both`, `vibecat -l <port>` binds two TCP listeners: one
on `[::]:port` with `IPV6_V6ONLY=true` and one on `0.0.0.0:port`.

When `-4` or `-6` is set, only the corresponding listener is bound and behavior
is single-socket (same as today, just with the appropriate address).

### Accepting the First Connection

Each listener gets its own thread, blocking on `accept()`. Both threads send
their result through a shared `mpsc::channel`. The main thread does `recv()` on
the channel; whichever accepts first wins. The losing listener is dropped.

### socket2 Usage

Build a `socket2::Socket`, set `IPV6_V6ONLY(true)` and `SO_REUSEADDR(true)`,
bind, listen, then convert to `std::net::TcpListener` via `.into()`. This
confines `socket2` to setup; the rest of the code uses std types.

## Listen Mode — Dual-Socket UDP

Same dual-socket pattern as TCP. When `addr_family` is `Both`, bind two UDP
sockets: `[::]:port` (`IPV6_V6ONLY=true`) and `0.0.0.0:port`.

### Receiving the First Datagram

Two threads, each blocking on `recv_from` with a recv timeout (e.g. 1 second),
checking a shared "done" flag between iterations. Both threads send results
through a shared `mpsc::channel`. Whichever socket receives a datagram first:
`connect()` it to the peer, wrap in `UdpStream`, drop the other socket.

The first datagram's payload is preserved and delivered to stdout, same as today.

This two-thread + channel approach is consistent with TCP listen, and paves the
way for future `-k` keep-listening support (loop over the channel instead of
one-shot `recv()`).

## Connect Mode — Happy Eyeballs (RFC 8305)

When connecting to a hostname that resolves to both AAAA and A records, vibecat
uses a simplified Happy Eyeballs algorithm.

### Resolution

`std::net::ToSocketAddrs` resolves the hostname. Results are partitioned into
IPv6 and IPv4 address lists.

When `-4` or `-6` is set, filter to only the matching family and connect
sequentially (try each address in order, no Happy Eyeballs racing).

### When addr_family is Both (TCP)

1. Partition resolved addresses into IPv6 and IPv4 lists.
2. Spawn a thread that tries IPv6 addresses sequentially, each with
   `socket2`'s `connect_timeout` (~5 seconds per attempt).
3. After a **250ms head-start**, spawn a second thread that tries IPv4
   addresses sequentially, same per-attempt timeout.
4. Both threads send their first successful connection through a shared
   `mpsc::channel`. On success they stop trying further addresses.
5. First successful connection wins. The loser is dropped.
6. If all addresses in both families fail, return the last error.
7. If the hostname resolves to only one family, no racing occurs — a single
   thread tries addresses sequentially.

The 250ms delay is the RFC 8305 "connection attempt delay" -- long enough to
prefer IPv6 when it works, short enough to not penalize users on broken IPv6
networks.

### UDP Connect Mode

No handshake means there's nothing to race. Resolve the hostname, prefer the
first IPv6 address, fall back to IPv4 if no IPv6 addresses exist. The
`connect()` call always succeeds immediately since it just sets the default
destination.

The ephemeral bind address is chosen to match the family of the selected remote
address: `[::]:0` for IPv6, `0.0.0.0:0` for IPv4.

## Verbose Output Changes

The protocol qualifier in verbose messages changes from `(tcp)`/`(udp)` to
include the address family with proper capitalization, layer 3 first:
`(IPv4/TCP)`, `(IPv4/UDP)`, `(IPv6/TCP)`, `(IPv6/UDP)`.

The family is derived from the actual socket address (V4 vs V6), not from the
CLI flags.

### Listen Mode

One message per listener as each binds:

```
vibecat: Listening on :: on port 9999 (IPv6/TCP).
vibecat: Listening on 0.0.0.0 on port 9999 (IPv4/TCP).
```

### Connect Mode

Single message once connected:

```
vibecat: Connected to example.com (2606:2800:220:1:...) on port 80 (IPv6/TCP) from ::1 on port 54321.
```

### Implementation

`proto_str` becomes a function that takes both the protocol and a `SocketAddr`,
inspecting whether the address is V4 or V6 to produce the full qualifier.

The `format_destination` logic is unchanged: show `hostname (ip)` when the user
passed a hostname, raw IP otherwise. IPv6 addresses are printed bare (no
brackets) since there's no ambiguity in these structured messages.

## Dependencies

Add `socket2` with the `all` feature for cross-platform socket option support:

```toml
socket2 = { version = "0.5", features = ["all"] }
```

`socket2` is a thin, widely-used wrapper over platform socket APIs. It is used
only during socket setup; all runtime I/O uses std types.

## Testing Strategy

### Unit Tests

- **`cli.rs`:** Parse `-4`, `-6`, both (error), neither (defaults to `Both`).
  Combinations with `-l`, `-u`, `-v`.
- **`verbose.rs`:** Verify the new `IPv6/TCP`-style qualifiers for all four
  protocol/family combinations. Existing tests updated for the new format.

### Integration Tests (`tests/e2e.rs`)

- **TCP dual-listen:** Spawn `vibecat -l <port>`, connect from an IPv4 client,
  assert data flows. Repeat with an IPv6 client connecting to `[::1]:<port>`.
- **TCP `-4` and `-6` listen:** Spawn with `-4 -l`, confirm only IPv4 works.
  Likewise for `-6`.
- **UDP dual-listen:** Send a datagram from IPv4, assert first-datagram
  delivery. Repeat from IPv6.
- **Happy Eyeballs fallback:** Bind a TCP listener on `127.0.0.1:<port>`
  (IPv4 only). Spawn `vibecat localhost <port>` (no `-4`/`-6`). IPv6 attempt to
  `[::1]` fails, falls back to IPv4, connection succeeds. Assert data flows.
- **Happy Eyeballs prefer-IPv6:** Bind a TCP listener on `[::1]:<port>` (IPv6
  only). Spawn `vibecat localhost <port>`. IPv6 connects first within the 250ms
  head-start. Assert data flows.
- **Verbose output:** Existing verbose e2e tests updated to assert `(IPv4/TCP)`
  format. New test for dual-listen asserting both `Listening` lines appear in
  stderr.

### Existing Tests

All existing tests continue to pass. Tests that use `127.0.0.1` explicitly
work unchanged since they hit IPv4 directly. The verbose output tests are
updated for the new `(IPv4/TCP)` format.
