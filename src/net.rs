//! Connection setup: connecting out or listening, over TCP or UDP.

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};

use crate::cli::{Config, Proto};
use crate::io::UdpStream;

/// A live connection, abstracting over TCP and UDP so the rest of the program
/// can treat both uniformly while still allowing a TCP-only write shutdown.
pub enum Conn {
    Tcp(TcpStream),
    Udp(UdpStream),
}

impl Conn {
    /// Get the remote address of the connection. TCP returns the peer;
    /// UDP returns the connected peer.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        match self {
            Conn::Tcp(s) => s.peer_addr(),
            Conn::Udp(s) => s.peer_addr(),
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{AddrFamily, Mode};
    use std::thread;

    fn cfg(mode: Mode, proto: Proto, host: Option<&str>, port: u16) -> Config {
        Config { mode, proto, host: host.map(String::from), port, verbose: false, addr_family: AddrFamily::Both }
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
