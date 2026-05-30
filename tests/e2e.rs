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
    assert!(stderr.contains("(IPv4/TCP)"), "stderr should contain '(IPv4/TCP)', got: {stderr}");
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

    let client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
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

    let mut client = std::net::TcpStream::connect(("::1", port)).unwrap();
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
