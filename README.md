# vibecat

A [netcat](https://en.wikipedia.org/wiki/Netcat) clone vibe-coded in Rust.

`vibecat` connects to a remote host or listens for an incoming connection, then
pipes data bidirectionally between standard I/O and the network socket. It
supports both TCP and UDP, in a small, readable, dependency-light implementation
(std lib plus `clap` for argument parsing).

## Install

```sh
cargo install --git https://github.com/marcusleg/vibecat
```

## Build

```sh
cargo build --release
```

The binary is produced at `target/release/vibecat`.

## Usage

```
vibecat [OPTIONS] <host> <port>      # client: connect to host:port
vibecat -l [OPTIONS] <port>          # server: listen on port (host defaults to [::] and 0.0.0.0)
vibecat -z <host> <port>             # scan: test whether a port is open, then exit
```

| Flag           | Meaning                                            |
|----------------|----------------------------------------------------|
| `-l, --listen` | Listen mode (server). Without it: client mode.     |
| `-u, --udp`    | Use UDP instead of TCP.                            |
| `-z, --zero`   | Zero-I/O: test if a port is open, then exit (TCP). |
| `-4, --ipv4`   | Use IPv4 only.                                     |
| `-6, --ipv6`   | Use IPv6 only.                                     |
| `-v, --verbose` | Print diagnostic messages to stderr.              |
| `-h, --help`   | Print help.                                        |
| `-V, --version`| Print version.                                     |

In listen mode a single positional argument is the port; two positional
arguments are `<host> <port>` (an explicit bind address). Listen mode accepts
exactly one connection, then exits.

## Examples

Listen on a port and print what a client sends:

```sh
vibecat -l 9999
```

Send a line to a listener from another shell:

```sh
echo "hello" | vibecat 127.0.0.1 9999
```

Chat across two terminals (type in either, see it in the other):

```sh
vibecat -l 9999          # terminal 1
vibecat 127.0.0.1 9999   # terminal 2
```

Check if a port is open (zero-I/O scan):

```sh
vibecat -z example.com 443
```

UDP — listener and client (exit the listener with Ctrl-C):

```sh
vibecat -l -u 9999       # terminal 1
echo "ping" | vibecat -u 127.0.0.1 9999   # terminal 2
```

## Behavior notes

- **Half-close on stdin EOF (TCP).** When stdin ends, vibecat shuts down its send
  side (sending a FIN) but keeps receiving until the remote closes. This matches
  standard netcat behavior.
- **Early FIN and some servers.** Because vibecat sends a FIN on stdin EOF, some
  servers/CDNs abort the connection before replying (for example,
  `echo ... | vibecat example.com 80` may return nothing). This is not a vibecat
  bug — the system `nc`/`ncat` behaves identically. ncat's `--no-shutdown` flag
  works around it; a similar flag for vibecat is out of scope for now.
- **UDP has no end-of-stream.** A UDP receiver has no FIN to wait on, so listen
  mode runs until you interrupt it with Ctrl-C — again matching `nc -u`.
- **Dual-stack by default.** In listen mode, vibecat binds both IPv6 and IPv4
  sockets. In connect mode, it uses Happy Eyeballs (RFC 8305) to prefer IPv6
  with an IPv4 fallback. Use `-4` or `-6` to restrict to one address family.

## Scope

This is a minimal MVP. Out of scope (for now): source-port selection (`-p`),
timeouts (`-w`), keep-listening for multiple clients (`-k`), command execution
(`-e`/`-c`), port-range scanning, hex-dump output, and TLS.

## Development

```sh
cargo test        # unit tests (cli/io/net) + end-to-end integration tests
```

The design document lives in
[`docs/superpowers/specs/`](docs/superpowers/specs/2026-05-29-vibecat-netcat-clone-design.md).
