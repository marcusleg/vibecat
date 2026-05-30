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

/// Address family constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrFamily {
    Both,
    Ipv4,
    Ipv6,
}

/// Fully resolved, validated configuration handed to the rest of the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub mode: Mode,
    pub proto: Proto,
    /// `None` only in listen mode with no explicit bind address (defaults later).
    pub host: Option<String>,
    pub port: u16,
    pub verbose: bool,
    pub addr_family: AddrFamily,
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

    /// Print diagnostic messages to stderr.
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,

    /// Use IPv4 only.
    #[arg(short = '4', long = "ipv4")]
    ipv4: bool,

    /// Use IPv6 only.
    #[arg(short = '6', long = "ipv6")]
    ipv6: bool,

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

        let addr_family = match (raw.ipv4, raw.ipv6) {
            (true, true) => return Err("-4 and -6 are mutually exclusive".to_string()),
            (true, false) => AddrFamily::Ipv4,
            (false, true) => AddrFamily::Ipv6,
            (false, false) => AddrFamily::Both,
        };

        Ok(Config { mode, proto, host, port, verbose: raw.verbose, addr_family })
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

    #[test]
    fn verbose_flag_short() {
        let c = Config::from_args(["vibecat", "-v", "example.com", "80"]).unwrap();
        assert!(c.verbose);
    }

    #[test]
    fn verbose_flag_long() {
        let c = Config::from_args(["vibecat", "--verbose", "-l", "8080"]).unwrap();
        assert!(c.verbose);
    }

    #[test]
    fn verbose_defaults_to_false() {
        let c = Config::from_args(["vibecat", "example.com", "80"]).unwrap();
        assert!(!c.verbose);
    }

    #[test]
    fn ipv4_flag_short() {
        let c = Config::from_args(["vibecat", "-4", "example.com", "80"]).unwrap();
        assert_eq!(c.addr_family, AddrFamily::Ipv4);
    }

    #[test]
    fn ipv6_flag_short() {
        let c = Config::from_args(["vibecat", "-6", "example.com", "80"]).unwrap();
        assert_eq!(c.addr_family, AddrFamily::Ipv6);
    }

    #[test]
    fn ipv4_flag_long() {
        let c = Config::from_args(["vibecat", "--ipv4", "-l", "8080"]).unwrap();
        assert_eq!(c.addr_family, AddrFamily::Ipv4);
    }

    #[test]
    fn ipv6_flag_long() {
        let c = Config::from_args(["vibecat", "--ipv6", "-l", "8080"]).unwrap();
        assert_eq!(c.addr_family, AddrFamily::Ipv6);
    }

    #[test]
    fn addr_family_defaults_to_both() {
        let c = Config::from_args(["vibecat", "example.com", "80"]).unwrap();
        assert_eq!(c.addr_family, AddrFamily::Both);
    }

    #[test]
    fn ipv4_and_ipv6_together_is_error() {
        assert!(Config::from_args(["vibecat", "-4", "-6", "example.com", "80"]).is_err());
    }

    #[test]
    fn ipv4_with_udp_and_listen() {
        let c = Config::from_args(["vibecat", "-4", "-u", "-l", "8080"]).unwrap();
        assert_eq!(c.addr_family, AddrFamily::Ipv4);
        assert_eq!(c.proto, Proto::Udp);
        assert_eq!(c.mode, Mode::Listen);
    }
}
