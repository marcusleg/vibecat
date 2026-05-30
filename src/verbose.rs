//! Colored diagnostic messages for verbose mode.

use std::net::SocketAddr;

use owo_colors::{OwoColorize, Stream, Style};

use crate::cli::{Config, Proto};

/// Bold cyan style used for addresses and ports.
fn bold_cyan() -> Style {
    Style::new().bold().cyan()
}

/// Print a "Listening on ..." message to stderr. No-op if verbose is off.
pub fn log_listening(config: &Config, bind_addr: SocketAddr) {
    if !config.verbose {
        return;
    }
    let proto = proto_str(config.proto);
    let bc = bold_cyan();
    eprintln!(
        "vibecat: {} on {} on port {} ({proto}).",
        "Listening".if_supports_color(Stream::Stderr, |t| t.green()),
        bind_addr
            .ip()
            .if_supports_color(Stream::Stderr, |t| t.style(bc)),
        bind_addr
            .port()
            .if_supports_color(Stream::Stderr, |t| t.style(bc)),
    );
}

/// Print a "Connected to ..." message to stderr. No-op if verbose is off.
///
/// If `config.host` differs from the resolved IP in `remote_addr`, both are
/// shown as `hostname (ip)`. If they match (user passed a raw IP), only the
/// IP is shown.
pub fn log_connected(config: &Config, local_addr: SocketAddr, remote_addr: SocketAddr) {
    if !config.verbose {
        return;
    }
    let proto = proto_str(config.proto);
    let remote_ip = remote_addr.ip().to_string();

    let destination = format_destination(config.host.as_deref(), &remote_ip);

    let bc = bold_cyan();
    eprintln!(
        "vibecat: {} to {destination} on port {} ({proto}) from {} on port {}.",
        "Connected".if_supports_color(Stream::Stderr, |t| t.green()),
        remote_addr
            .port()
            .if_supports_color(Stream::Stderr, |t| t.style(bc)),
        local_addr.ip(),
        local_addr.port(),
    );
}

/// Format the destination as `hostname (ip)` or just `ip`.
fn format_destination(host: Option<&str>, resolved_ip: &str) -> String {
    let bc = bold_cyan();
    match host {
        Some(h) if h != resolved_ip => format!(
            "{} ({})",
            h.if_supports_color(Stream::Stderr, |t| t.style(bc)),
            resolved_ip.if_supports_color(Stream::Stderr, |t| t.style(bc)),
        ),
        _ => format!(
            "{}",
            resolved_ip.if_supports_color(Stream::Stderr, |t| t.style(bc)),
        ),
    }
}

fn proto_str(proto: Proto) -> &'static str {
    match proto {
        Proto::Tcp => "tcp",
        Proto::Udp => "udp",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Mode;
    use std::net::{IpAddr, Ipv4Addr};

    fn cfg(verbose: bool, proto: Proto, host: Option<&str>) -> Config {
        Config {
            mode: Mode::Connect,
            proto,
            host: host.map(String::from),
            port: 80,
            verbose,
        }
    }

    fn addr(ip: [u8; 4], port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3])), port)
    }

    #[test]
    fn format_destination_with_hostname() {
        owo_colors::set_override(false);
        let result = format_destination(Some("example.com"), "93.184.216.34");
        assert_eq!(result, "example.com (93.184.216.34)");
        owo_colors::unset_override();
    }

    #[test]
    fn format_destination_ip_only() {
        owo_colors::set_override(false);
        let result = format_destination(Some("93.184.216.34"), "93.184.216.34");
        assert_eq!(result, "93.184.216.34");
        owo_colors::unset_override();
    }

    #[test]
    fn format_destination_no_host() {
        owo_colors::set_override(false);
        let result = format_destination(None, "93.184.216.34");
        assert_eq!(result, "93.184.216.34");
        owo_colors::unset_override();
    }

    #[test]
    fn log_connected_is_noop_when_not_verbose() {
        let config = cfg(false, Proto::Tcp, Some("example.com"));
        log_connected(
            &config,
            addr([192, 168, 1, 5], 54321),
            addr([93, 184, 216, 34], 80),
        );
    }

    #[test]
    fn log_listening_is_noop_when_not_verbose() {
        let config = cfg(false, Proto::Tcp, None);
        log_listening(&config, addr([0, 0, 0, 0], 9999));
    }
}
