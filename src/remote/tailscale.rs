//! Tailscale IPv4 discovery and pairing URI generation.
use super::http::percent_encode;
use std::{
    net::{IpAddr, Ipv4Addr},
    process::Command,
};

/// Returns true if the IPv4 address is in Tailscale's CGNAT range (`100.64.0.0/10`).
pub fn is_tailscale_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (octets[1] & 0b1100_0000) == 64
}

/// Best-effort detection of the host's Tailscale IPv4 address.
pub fn detect_tailscale_ip() -> Option<Ipv4Addr> {
    if let Ok(ip) = std::env::var("PUI_TAILSCALE_IP") {
        if let Ok(parsed) = ip.trim().parse::<Ipv4Addr>() {
            return Some(parsed);
        }
    }
    // Try standard Tailscale CLI locations (Homebrew, macOS App Store/GUI, Linux PATH).
    for bin in [
        "tailscale",
        "/opt/homebrew/bin/tailscale",
        "/usr/local/bin/tailscale",
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    ] {
        let Ok(output) = Command::new(bin).args(["ip", "-4"]).output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Ok(ip) = line.trim().parse::<Ipv4Addr>() {
                if is_tailscale_ipv4(ip) {
                    return Some(ip);
                }
            }
        }
    }
    // Fallback: inspect local interfaces via `ifconfig` on macOS/BSD.
    if let Ok(output) = Command::new("ifconfig").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for token in stdout.split_whitespace() {
                if let Ok(ip) = token.parse::<Ipv4Addr>() {
                    if is_tailscale_ipv4(ip) {
                        return Some(ip);
                    }
                }
            }
        }
    }
    None
}

pub fn default_host_name() -> String {
    if let Ok(name) = std::env::var("PUI_HOST_NAME") {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    if let Ok(output) = Command::new("hostname").arg("-s").output() {
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if !name.is_empty() {
                return name;
            }
        }
    }
    "Pui Host".to_owned()
}

pub fn pairing_uri(host: &str, port: u16, token: &str, name: &str, tls: bool) -> String {
    format!(
        "pui://pair?v=1&host={}&port={port}&token={}&name={}&tls={}",
        percent_encode(host),
        percent_encode(token),
        percent_encode(name),
        if tls { "1" } else { "0" }
    )
}

pub fn ws_url(host: &str, port: u16, tls: bool) -> String {
    let scheme = if tls { "wss" } else { "ws" };
    if let Ok(IpAddr::V6(_)) = host.parse::<IpAddr>() {
        format!("{scheme}://[{host}]:{port}/v1/ws")
    } else {
        format!("{scheme}://{host}:{port}/v1/ws")
    }
}
