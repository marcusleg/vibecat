//! Command-line argument parsing and validation.

use std::ffi::OsString;

use clap::Parser;

/// Whether vibecat connects out or listens for an incoming connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Connect,
    Listen,
}

/// Transport protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Tcp,
    Udp,
}

/// Fully resolved, validated configuration handed to the rest of the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub mode: Mode,
    pub proto: Proto,
    /// `None` only in listen mode with no explicit bind address (defaults later).
    pub host: Option<String>,
    pub port: u16,
}

/// Raw clap-parsed arguments, before semantic validation.
#[derive(Parser, Debug)]
#[command(name = "vibecat", about = "A minimal netcat clone")]
struct RawArgs {
    /// Listen for an incoming connection instead of connecting out.
    #[arg(short = 'l', long = "listen")]
    listen: bool,

    /// Use UDP instead of TCP.
    #[arg(short = 'u', long = "udp")]
    udp: bool,

    /// Positional arguments: `[host] <port>`.
    #[arg(value_name = "ARGS")]
    positionals: Vec<String>,
}

impl Config {
    /// Parse and validate from an argument iterator (program name first).
    pub fn from_args<I, T>(args: I) -> Result<Config, String>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let raw = RawArgs::try_parse_from(args).map_err(|e| e.to_string())?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawArgs) -> Result<Config, String> {
        let mode = if raw.listen { Mode::Listen } else { Mode::Connect };
        let proto = if raw.udp { Proto::Udp } else { Proto::Tcp };

        let (host, port_str) = match (mode, raw.positionals.as_slice()) {
            (Mode::Connect, [host, port]) => (Some(host.clone()), port),
            (Mode::Connect, _) => {
                return Err("connect mode requires <host> <port>".to_string());
            }
            (Mode::Listen, [port]) => (None, port),
            (Mode::Listen, [host, port]) => (Some(host.clone()), port),
            (Mode::Listen, _) => {
                return Err("listen mode requires <port> (and optional bind host)".to_string());
            }
        };

        let port: u16 = port_str
            .parse()
            .map_err(|_| format!("invalid port: {port_str}"))?;

        Ok(Config { mode, proto, host, port })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_tcp_host_and_port() {
        let c = Config::from_args(["vibecat", "example.com", "80"]).unwrap();
        assert_eq!(c.mode, Mode::Connect);
        assert_eq!(c.proto, Proto::Tcp);
        assert_eq!(c.host.as_deref(), Some("example.com"));
        assert_eq!(c.port, 80);
    }

    #[test]
    fn connect_udp_flag() {
        let c = Config::from_args(["vibecat", "-u", "1.2.3.4", "9"]).unwrap();
        assert_eq!(c.proto, Proto::Udp);
        assert_eq!(c.mode, Mode::Connect);
    }

    #[test]
    fn listen_single_positional_is_port() {
        let c = Config::from_args(["vibecat", "-l", "8080"]).unwrap();
        assert_eq!(c.mode, Mode::Listen);
        assert_eq!(c.host, None);
        assert_eq!(c.port, 8080);
    }

    #[test]
    fn listen_two_positionals_is_host_and_port() {
        let c = Config::from_args(["vibecat", "-l", "127.0.0.1", "8080"]).unwrap();
        assert_eq!(c.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(c.port, 8080);
    }

    #[test]
    fn connect_missing_port_is_error() {
        assert!(Config::from_args(["vibecat", "example.com"]).is_err());
    }

    #[test]
    fn listen_missing_port_is_error() {
        assert!(Config::from_args(["vibecat", "-l"]).is_err());
    }

    #[test]
    fn non_numeric_port_is_error() {
        assert!(Config::from_args(["vibecat", "example.com", "http"]).is_err());
    }

    #[test]
    fn out_of_range_port_is_error() {
        assert!(Config::from_args(["vibecat", "example.com", "70000"]).is_err());
    }
}
