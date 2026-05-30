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
    if config.mode == Mode::ScanPort {
        return run_scan(config);
    }

    let (conn, initial) = match config.mode {
        Mode::Connect => {
            let (conn, local_addr) = net::connect(config)?;
            let remote_addr = conn.peer_addr()?;
            verbose::log_connected(config, local_addr, remote_addr);
            (conn, None)
        }
        Mode::Listen => {
            let listeners = net::bind(config)?;
            for (_, bind_addr) in &listeners {
                verbose::log_listening(config, *bind_addr);
            }
            let (conn, initial, local_addr, peer_addr) = net::accept_first(listeners)?;
            verbose::log_connected(config, local_addr, peer_addr);
            (conn, initial)
        }
        Mode::ScanPort => unreachable!(),
    };

    if let Some(bytes) = initial {
        let mut stdout = std::io::stdout();
        stdout.write_all(&bytes)?;
        stdout.flush()?;
    }

    let remote_addr = conn.peer_addr()?;
    pump_bidirectional(conn, config, remote_addr)
}

/// Zero-I/O port scan: connect, report, exit.
fn run_scan(config: &Config) -> std::io::Result<()> {
    let host = config.host.as_deref().unwrap_or("localhost");
    match net::scan_port(config) {
        Ok(remote_addr) => {
            verbose::log_scan_succeeded(host, config.port, remote_addr);
            Ok(())
        }
        Err(e) => {
            verbose::log_scan_failed(host, config.port, config.addr_family, &e);
            Err(e)
        }
    }
}

/// Spawn the stdin→socket and socket→stdout pumps and run until both directions
/// are done.
///
/// Both half-closes are honored symmetrically: our stdin EOF shuts down the send
/// side while we keep receiving, and the peer's FIN ends the receive side while
/// we keep sending our stdin. The process therefore exits only once *both* pumps
/// finish — waiting for the send pump guarantees buffered stdin data is delivered
/// before the socket closes.
fn pump_bidirectional(
    conn: Conn,
    config: &Config,
    remote_addr: std::net::SocketAddr,
) -> std::io::Result<()> {
    let send_conn = conn.try_clone()?;
    let recv_conn = conn;

    let (tx, rx) = std::sync::mpsc::channel::<()>();

    // Thread 1: stdin -> socket. On stdin EOF, half-close the write side so the
    // remote sees our EOF (TCP FIN); the receive side stays open.
    let send_thread = thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut writer = send_conn;
        let _ = io::pump(stdin.lock(), &mut writer);
        let _ = writer.shutdown_write();
        let _ = tx.send(());
    });

    // Thread 2 (this thread): socket -> stdout, until the remote closes.
    let stdout = std::io::stdout();
    let mut reader = recv_conn;
    let recv_result = io::pump(&mut reader, stdout.lock());

    verbose::log_disconnected(config, remote_addr);

    // Give the send thread a brief window to finish flushing any in-flight
    // stdin data. If it's still blocked on stdin.read() after the grace period
    // (nobody is typing, remote already closed), let the process exit — the OS
    // tears down the blocked thread.
    if rx
        .recv_timeout(std::time::Duration::from_millis(200))
        .is_ok()
    {
        let _ = send_thread.join();
    }
    recv_result
}
