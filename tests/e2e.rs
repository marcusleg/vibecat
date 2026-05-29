//! End-to-end tests that spawn the real `vibecat` binary in listen and connect
//! modes and verify data flows between them.

use std::io::{Read, Write};
use std::net::{TcpListener, UdpSocket};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

/// Path to the compiled binary under test.
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_vibecat")
}

/// Find a free TCP port by binding to :0 and releasing it.
fn free_tcp_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

#[test]
fn tcp_client_sends_stdin_to_a_listening_server() {
    // Use a plain std listener as the "server" so the test controls timing,
    // and the vibecat binary as the client piping stdin to it.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut got = Vec::new();
        stream.read_to_end(&mut got).unwrap();
        got
    });

    let mut child = Command::new(bin())
        .arg("127.0.0.1")
        .arg(port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hello from client")
        .unwrap();
    // stdin dropped here -> EOF -> client should half-close.

    let received = server.join().unwrap();
    child.wait().unwrap();
    assert_eq!(received, b"hello from client");
}

#[test]
fn tcp_listen_mode_receives_and_responds() {
    let port = free_tcp_port();

    // vibecat in listen mode; we feed it stdin (its response to the client) and
    // capture what the client sent on its stdout.
    let mut server = Command::new(bin())
        .arg("-l")
        .arg(port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    // Give the listener time to bind.
    thread::sleep(Duration::from_millis(200));

    // Connect as a plain client, send a request, read the server's reply.
    let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    server
        .stdin
        .take()
        .unwrap()
        .write_all(b"reply-from-server")
        .unwrap();
    client.write_all(b"request-from-client").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();

    let mut reply = Vec::new();
    client.read_to_end(&mut reply).unwrap();

    let mut server_stdout = Vec::new();
    server
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut server_stdout)
        .unwrap();
    server.wait().unwrap();

    assert_eq!(reply, b"reply-from-server");
    assert_eq!(server_stdout, b"request-from-client");
}

#[test]
fn tcp_half_close_keeps_receiving_after_stdin_eof() {
    // Server reads the client's request, then sends a response AFTER a delay,
    // proving the client kept the read side open past its own stdin EOF.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut req = [0u8; 64];
        let n = stream.read(&mut req).unwrap();
        assert_eq!(&req[..n], b"ping");
        // Delay, then respond. If the client exited on stdin EOF, this is lost.
        thread::sleep(Duration::from_millis(200));
        stream.write_all(b"pong").unwrap();
    });

    let mut child = Command::new(bin())
        .arg("127.0.0.1")
        .arg(port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.take().unwrap().write_all(b"ping").unwrap();
    // stdin closed -> half-close, but read side must stay open.

    let mut out = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut out).unwrap();
    child.wait().unwrap();
    server.join().unwrap();

    assert_eq!(out, b"pong");
}

#[test]
fn udp_listen_delivers_first_datagram_to_stdout() {
    // free UDP port
    let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let mut server = Command::new(bin())
        .arg("-l")
        .arg("-u")
        .arg(port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(200));

    let client = UdpSocket::bind("0.0.0.0:0").unwrap();
    client.send_to(b"udp-hello", ("127.0.0.1", port)).unwrap();

    // Read exactly the first datagram's worth from the server's stdout.
    let mut buf = [0u8; 9];
    server
        .stdout
        .as_mut()
        .unwrap()
        .read_exact(&mut buf)
        .unwrap();
    assert_eq!(&buf, b"udp-hello");

    server.kill().unwrap();
    server.wait().unwrap();
}
