//! Connection setup: connecting out or listening, over TCP or UDP.

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use socket2::{Domain, Socket, Type};

use crate::cli::{AddrFamily, Config, Proto};
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

fn make_tcp_listener(addr: SocketAddr) -> io::Result<TcpListener> {
    let domain = match addr {
        SocketAddr::V4(_) => Domain::IPV4,
        SocketAddr::V6(_) => Domain::IPV6,
    };
    let socket = Socket::new(domain, Type::STREAM, None)?;
    socket.set_reuse_address(true)?;
    if addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.bind(&addr.into())?;
    socket.listen(1)?;
    Ok(socket.into())
}

fn make_udp_socket(addr: SocketAddr) -> io::Result<UdpSocket> {
    let domain = match addr {
        SocketAddr::V4(_) => Domain::IPV4,
        SocketAddr::V6(_) => Domain::IPV6,
    };
    let socket = Socket::new(domain, Type::DGRAM, None)?;
    socket.set_reuse_address(true)?;
    if addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.bind(&addr.into())?;
    Ok(socket.into())
}

fn listen_addrs(config: &Config) -> io::Result<Vec<SocketAddr>> {
    let port = config.port;
    match config.host.as_deref() {
        Some(host) => {
            let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs()?.collect();
            let addrs: Vec<SocketAddr> = match config.addr_family {
                AddrFamily::Both => addrs,
                AddrFamily::Ipv4 => addrs.into_iter().filter(|a| a.is_ipv4()).collect(),
                AddrFamily::Ipv6 => addrs.into_iter().filter(|a| a.is_ipv6()).collect(),
            };
            if addrs.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrNotAvailable,
                    format!("no addresses resolved for {host}"),
                ));
            }
            Ok(addrs)
        }
        None => Ok(match config.addr_family {
            AddrFamily::Both => vec![
                SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
            ],
            AddrFamily::Ipv4 => {
                vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)]
            }
            AddrFamily::Ipv6 => {
                vec![SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port)]
            }
        }),
    }
}

/// Connect out to the remote described by `config` (client mode).
pub fn connect(config: &Config) -> io::Result<(Conn, SocketAddr)> {
    let host = config.host.as_deref().unwrap_or("localhost");
    let port = config.port;
    match config.proto {
        Proto::Tcp => connect_tcp(host, port, config.addr_family),
        Proto::Udp => connect_udp(host, port, config.addr_family),
    }
}

fn connect_tcp(host: &str, port: u16, family: AddrFamily) -> io::Result<(Conn, SocketAddr)> {
    let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs()?.collect();

    let (primary, fallback) = match family {
        AddrFamily::Ipv4 => {
            let v4: Vec<_> = addrs.into_iter().filter(|a| a.is_ipv4()).collect();
            (v4, vec![])
        }
        AddrFamily::Ipv6 => {
            let v6: Vec<_> = addrs.into_iter().filter(|a| a.is_ipv6()).collect();
            (v6, vec![])
        }
        AddrFamily::Both => {
            let (v6, v4): (Vec<_>, Vec<_>) = addrs.into_iter().partition(|a| a.is_ipv6());
            (v6, v4)
        }
    };

    if primary.is_empty() && fallback.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("no addresses resolved for {host}"),
        ));
    }

    if fallback.is_empty() {
        return try_connect_addrs(&primary);
    }

    if primary.is_empty() {
        return try_connect_addrs(&fallback);
    }

    happy_eyeballs(primary, fallback)
}

fn connect_udp(host: &str, port: u16, _family: AddrFamily) -> io::Result<(Conn, SocketAddr)> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect((host, port))?;
    let local = socket.local_addr()?;
    Ok((Conn::Udp(UdpStream::new(socket)), local))
}

fn try_connect_addrs(addrs: &[SocketAddr]) -> io::Result<(Conn, SocketAddr)> {
    let mut last_err = io::Error::new(io::ErrorKind::AddrNotAvailable, "no addresses to try");
    for addr in addrs {
        let domain = match addr {
            SocketAddr::V4(_) => Domain::IPV4,
            SocketAddr::V6(_) => Domain::IPV6,
        };
        let socket = match Socket::new(domain, Type::STREAM, None) {
            Ok(s) => s,
            Err(e) => {
                last_err = e;
                continue;
            }
        };
        match socket.connect_timeout(&(*addr).into(), Duration::from_secs(5)) {
            Ok(()) => {
                let stream: TcpStream = socket.into();
                let local = stream.local_addr()?;
                return Ok((Conn::Tcp(stream), local));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn happy_eyeballs(
    primary: Vec<SocketAddr>,
    fallback: Vec<SocketAddr>,
) -> io::Result<(Conn, SocketAddr)> {
    let (tx, rx) = mpsc::channel();

    let tx1 = tx.clone();
    thread::spawn(move || {
        let _ = tx1.send(try_connect_addrs(&primary));
    });

    let tx2 = tx.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        let _ = tx2.send(try_connect_addrs(&fallback));
    });

    drop(tx);

    let mut last_err = io::Error::new(
        io::ErrorKind::ConnectionRefused,
        "all connection attempts failed",
    );
    for result in rx {
        match result {
            Ok(conn) => return Ok(conn),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// An intermediate listener, waiting to accept a connection.
pub enum Listener {
    Tcp(TcpListener),
    Udp(UdpSocket),
}

/// Bind to the listen address(es) (server mode). Returns one or more listeners
/// and their bound addresses so the caller can log them before blocking on accept.
pub fn bind(config: &Config) -> io::Result<Vec<(Listener, SocketAddr)>> {
    let addrs = listen_addrs(config)?;
    let mut listeners = Vec::with_capacity(addrs.len());
    for addr in addrs {
        match config.proto {
            Proto::Tcp => {
                let listener = make_tcp_listener(addr)?;
                let bound = listener.local_addr()?;
                listeners.push((Listener::Tcp(listener), bound));
            }
            Proto::Udp => {
                let socket = make_udp_socket(addr)?;
                let bound = socket.local_addr()?;
                listeners.push((Listener::Udp(socket), bound));
            }
        }
    }
    Ok(listeners)
}

/// Accept a single connection from a bound listener.
///
/// Returns the connection, optional first UDP datagram payload, local
/// address, and peer address.
fn accept(listener: Listener) -> io::Result<(Conn, Option<Vec<u8>>, SocketAddr, SocketAddr)> {
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

pub fn accept_first(
    mut listeners: Vec<(Listener, SocketAddr)>,
) -> io::Result<(Conn, Option<Vec<u8>>, SocketAddr, SocketAddr)> {
    if listeners.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "no listeners provided"));
    }
    if listeners.len() == 1 {
        let (listener, _) = listeners.pop().unwrap();
        return accept(listener);
    }

    let is_tcp = matches!(listeners[0].0, Listener::Tcp(_));
    if is_tcp {
        accept_first_tcp(listeners)
    } else {
        accept_first_udp(listeners)
    }
}

fn accept_first_tcp(
    listeners: Vec<(Listener, SocketAddr)>,
) -> io::Result<(Conn, Option<Vec<u8>>, SocketAddr, SocketAddr)> {
    let done = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();

    for (listener, _) in listeners {
        let tx = tx.clone();
        let done = Arc::clone(&done);
        thread::spawn(move || {
            if let Listener::Tcp(l) = listener {
                l.set_nonblocking(true).ok();
                loop {
                    if done.load(Ordering::Relaxed) {
                        return;
                    }
                    match l.accept() {
                        Ok((stream, peer)) => {
                            done.store(true, Ordering::Relaxed);
                            stream.set_nonblocking(false).ok();
                            let local = match stream.local_addr() {
                                Ok(a) => a,
                                Err(e) => {
                                    let _ = tx.send(Err(e));
                                    return;
                                }
                            };
                            let _ = tx.send(Ok((
                                Conn::Tcp(stream),
                                None,
                                local,
                                peer,
                            )));
                            return;
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(50));
                            continue;
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            return;
                        }
                    }
                }
            }
        });
    }
    drop(tx);

    let mut last_err = io::Error::new(io::ErrorKind::Other, "no listeners accepted a connection");
    for result in rx {
        match result {
            Ok(conn_tuple) => return Ok(conn_tuple),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn accept_first_udp(
    listeners: Vec<(Listener, SocketAddr)>,
) -> io::Result<(Conn, Option<Vec<u8>>, SocketAddr, SocketAddr)> {
    let done = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();

    for (listener, _) in listeners {
        let tx = tx.clone();
        let done = Arc::clone(&done);
        thread::spawn(move || {
            if let Listener::Udp(socket) = listener {
                socket
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .ok();
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    if done.load(Ordering::Relaxed) {
                        return;
                    }
                    match socket.recv_from(&mut buf) {
                        Ok((n, peer)) => {
                            done.store(true, Ordering::Relaxed);
                            buf.truncate(n);
                            if let Err(e) = socket.connect(peer) {
                                let _ = tx.send(Err(e));
                                return;
                            }
                            let local = match socket.local_addr() {
                                Ok(a) => a,
                                Err(e) => {
                                    let _ = tx.send(Err(e));
                                    return;
                                }
                            };
                            let _ = tx.send(Ok((
                                Conn::Udp(UdpStream::new(socket)),
                                Some(buf),
                                local,
                                peer,
                            )));
                            return;
                        }
                        Err(ref e)
                            if e.kind() == io::ErrorKind::WouldBlock
                                || e.kind() == io::ErrorKind::TimedOut =>
                        {
                            continue;
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            return;
                        }
                    }
                }
            }
        });
    }
    drop(tx);

    let mut last_err =
        io::Error::new(io::ErrorKind::Other, "no UDP listener received a datagram");
    for result in rx {
        match result {
            Ok(conn_tuple) => return Ok(conn_tuple),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
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
            let listeners = bind(&listen_cfg).unwrap();
            assert_eq!(listeners[0].1.port(), port);
            accept_first(listeners).unwrap()
        });

        thread::sleep(std::time::Duration::from_millis(100));
        let client = UdpSocket::bind("0.0.0.0:0").unwrap();
        client.send_to(b"first", ("127.0.0.1", port)).unwrap();

        let (_conn, initial, _local, peer) = server.join().unwrap();
        assert_eq!(initial.as_deref(), Some(&b"first"[..]));
        assert_ne!(peer.port(), 0);
    }
}
