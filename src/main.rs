mod cli;
mod io;
mod net;
mod verbose;

use std::io::Write;
use std::process::ExitCode;
use std::thread;

use cli::{Config, Mode};
use net::Conn;

fn main() -> ExitCode {
    let config = match Config::from_args(std::env::args()) {
        Ok(c) => c,
        Err(e) => {
            // clap errors already include their own formatting/usage; print as-is.
            eprint!("{e}");
            if !e.ends_with('\n') {
                eprintln!();
            }
            return ExitCode::FAILURE;
        }
    };

    match run(&config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vibecat: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Establish the connection and pump data until the remote closes.
fn run(config: &Config) -> std::io::Result<()> {
    let (conn, initial) = match config.mode {
        Mode::Connect => (net::connect(config)?, None),
        Mode::Listen => net::listen(config)?,
    };

    // For UDP listen, the first datagram was consumed during peer discovery;
    // emit its payload before starting the receive pump.
    if let Some(bytes) = initial {
        let mut stdout = std::io::stdout();
        stdout.write_all(&bytes)?;
        stdout.flush()?;
    }

    pump_bidirectional(conn)
}

/// Spawn the stdin→socket and socket→stdout pumps and run until both directions
/// are done.
///
/// Both half-closes are honored symmetrically: our stdin EOF shuts down the send
/// side while we keep receiving, and the peer's FIN ends the receive side while
/// we keep sending our stdin. The process therefore exits only once *both* pumps
/// finish — waiting for the send pump guarantees buffered stdin data is delivered
/// before the socket closes.
fn pump_bidirectional(conn: Conn) -> std::io::Result<()> {
    let send_conn = conn.try_clone()?;
    let recv_conn = conn;

    // Thread 1: stdin -> socket. On stdin EOF, half-close the write side so the
    // remote sees our EOF (TCP FIN); the receive side stays open.
    let send_thread = thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut writer = send_conn;
        let _ = io::pump(stdin.lock(), &mut writer);
        let _ = writer.shutdown_write();
    });

    // Thread 2 (this thread): socket -> stdout, until the remote closes.
    let stdout = std::io::stdout();
    let mut reader = recv_conn;
    let recv_result = io::pump(&mut reader, stdout.lock());

    // Wait for the send side so its data is fully flushed before we exit.
    let _ = send_thread.join();
    recv_result
}
