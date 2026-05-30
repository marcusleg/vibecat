//! Byte-pumping loop and the UDP `Read`/`Write` adapter.

use std::io::{self, Read, Write};
use std::net::UdpSocket;

const BUF_SIZE: usize = 8 * 1024;

/// Copy bytes from `reader` to `writer` until EOF.
///
/// Returns `Ok(())` on a clean end-of-stream. A broken pipe / connection reset
/// is treated as a normal end-of-session, not an error.
pub fn pump<R: Read, W: Write>(mut reader: R, mut writer: W) -> io::Result<()> {
    let mut buf = [0u8; BUF_SIZE];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        match writer.write_all(&buf[..n]) {
            Ok(()) => {}
            Err(ref e) if is_disconnect(e) => return Ok(()),
            Err(e) => return Err(e),
        }
        writer.flush()?;
    }
}

/// Whether an error represents the peer going away mid-transfer.
fn is_disconnect(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
    )
}

/// Adapts a connected `UdpSocket` to the `Read`/`Write` traits so it can be used
/// interchangeably with `TcpStream` in [`pump`].
///
/// Each `read` maps to one `recv` (one datagram) and each `write` to one `send`.
pub struct UdpStream {
    socket: UdpSocket,
}

impl UdpStream {
    /// Wrap an already-connected socket.
    pub fn new(socket: UdpSocket) -> UdpStream {
        UdpStream { socket }
    }

    /// Produce an independent handle to the same socket (for the read thread).
    pub fn try_clone(&self) -> io::Result<UdpStream> {
        Ok(UdpStream {
            socket: self.socket.try_clone()?,
        })
    }

    /// Get the remote address this socket is connected to.
    pub fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.socket.peer_addr()
    }
}

impl Read for UdpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.socket.recv(buf)
    }
}

impl Write for UdpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.socket.send(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pump_copies_all_bytes() {
        let input: &[u8] = b"hello world";
        let mut output: Vec<u8> = Vec::new();
        pump(input, &mut output).unwrap();
        assert_eq!(output, b"hello world");
    }

    #[test]
    fn pump_stops_at_eof_with_empty_input() {
        let input: &[u8] = b"";
        let mut output: Vec<u8> = Vec::new();
        pump(input, &mut output).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn pump_handles_data_larger_than_buffer() {
        let big = vec![0xABu8; BUF_SIZE * 3 + 7];
        let mut output: Vec<u8> = Vec::new();
        pump(big.as_slice(), &mut output).unwrap();
        assert_eq!(output, big);
    }

    #[test]
    fn udp_stream_round_trip_over_loopback() {
        // Two sockets connected to each other on loopback.
        let a = UdpSocket::bind("127.0.0.1:0").unwrap();
        let b = UdpSocket::bind("127.0.0.1:0").unwrap();
        let a_addr = a.local_addr().unwrap();
        let b_addr = b.local_addr().unwrap();
        a.connect(b_addr).unwrap();
        b.connect(a_addr).unwrap();

        let mut sender = UdpStream::new(a);
        let mut receiver = UdpStream::new(b);

        sender.write(b"ping").unwrap();
        let mut buf = [0u8; 16];
        let n = receiver.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"ping");
    }
}
