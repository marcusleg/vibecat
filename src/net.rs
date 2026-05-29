//! Connection setup: connecting out or listening, over TCP or UDP.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, UdpSocket};

use crate::cli::{Config, Proto};
use crate::io::UdpStream;

/// A live connection, abstracting over TCP and UDP so the rest of the program
/// can treat both uniformly while still allowing a TCP-only write shutdown.
pub enum Conn {
    Tcp(TcpStream),
    Udp(UdpStream),
}

impl Conn {
    /// Produce an independent handle to the same connection, for the second
    /// pump thread.
    pub fn try_clone(&self) -> io::Result<Conn> {
        match self {
            Conn::Tcp(s) => Ok(Conn::Tcp(s.try_clone()?)),
            Conn::Udp(s) => Ok(Conn::Udp(s.try_clone()?)),
        }
    }

    /// Signal end-of-stream to the peer on the send side. TCP sends FIN; UDP has
    /// no such concept, so this is a no-op.
    pub fn shutdown_write(&self) -> io::Result<()> {
        match self {
            Conn::Tcp(s) => s.shutdown(Shutdown::Write),
            Conn::Udp(_) => Ok(()),
        }
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Tcp(s) => s.read(buf),
            Conn::Udp(s) => s.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Conn::Tcp(s) => s.write(buf),
            Conn::Udp(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Conn::Tcp(s) => s.flush(),
            Conn::Udp(s) => s.flush(),
        }
    }
}

/// Connect out to the remote described by `config` (client mode).
pub fn connect(config: &Config) -> io::Result<Conn> {
    let host = config.host.as_deref().unwrap_or("127.0.0.1");
    let port = config.port;
    match config.proto {
        Proto::Tcp => {
            let stream = TcpStream::connect((host, port))?;
            Ok(Conn::Tcp(stream))
        }
        Proto::Udp => {
            let socket = UdpSocket::bind("0.0.0.0:0")?;
            socket.connect((host, port))?;
            Ok(Conn::Udp(UdpStream::new(socket)))
        }
    }
}

/// Listen for and accept a single connection (server mode).
///
/// Returns the connection and, for UDP, the payload of the first datagram used
/// to discover the peer (so no data is lost).
pub fn listen(config: &Config) -> io::Result<(Conn, Option<Vec<u8>>)> {
    let host = config.host.as_deref().unwrap_or("0.0.0.0");
    let port = config.port;
    match config.proto {
        Proto::Tcp => {
            let listener = TcpListener::bind((host, port))?;
            let (stream, _peer) = listener.accept()?;
            Ok((Conn::Tcp(stream), None))
        }
        Proto::Udp => {
            let socket = UdpSocket::bind((host, port))?;
            let mut buf = vec![0u8; 64 * 1024];
            let (n, peer) = socket.recv_from(&mut buf)?;
            socket.connect(peer)?;
            buf.truncate(n);
            Ok((Conn::Udp(UdpStream::new(socket)), Some(buf)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Mode;
    use std::thread;

    fn cfg(mode: Mode, proto: Proto, host: Option<&str>, port: u16) -> Config {
        Config { mode, proto, host: host.map(String::from), port }
    }

    #[test]
    fn tcp_connect_and_listen_exchange_bytes() {
        // Bind a listener on an ephemeral port first to learn the port number.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 16];
            let n = stream.read(&mut buf).unwrap();
            stream.write_all(&buf[..n]).unwrap();
        });

        let client_cfg = cfg(Mode::Connect, Proto::Tcp, Some("127.0.0.1"), port);
        let mut conn = connect(&client_cfg).unwrap();
        conn.write_all(b"hi").unwrap();
        let mut buf = [0u8; 16];
        let n = conn.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"hi");
        server.join().unwrap();
    }

    #[test]
    fn udp_listen_returns_first_datagram_payload() {
        // Pick a port by binding then dropping is racy; instead bind the server
        // via listen() in a thread and have it report the chosen port back.
        let server_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = server_socket.local_addr().unwrap().port();
        drop(server_socket); // free it for listen() to rebind

        let listen_cfg = cfg(Mode::Listen, Proto::Udp, Some("127.0.0.1"), port);
        let server = thread::spawn(move || listen(&listen_cfg).unwrap());

        // Give the server a moment to bind, then send.
        thread::sleep(std::time::Duration::from_millis(100));
        let client = UdpSocket::bind("0.0.0.0:0").unwrap();
        client.send_to(b"first", ("127.0.0.1", port)).unwrap();

        let (_conn, initial) = server.join().unwrap();
        assert_eq!(initial.as_deref(), Some(&b"first"[..]));
    }
}
