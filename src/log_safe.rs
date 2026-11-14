//! What a log line may show of a value (the Sidevoice logging guideline, sidevoice/.github-private docs/logging.md, section 8). Small typed wrappers,
//! so the rule is in the type and not at the call site.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[cfg(test)]
mod tests;

/// An id that came from a catalogue entry, which can be a third party's: as is if every `/`-separated segment (a build
/// id is `model/build`) matches `[a-z0-9_.-]{1,64}` and the whole is at most 128 bytes, else `invalid`. A hostile id
/// could forge lines or inject instructions into an agent reading the log.
pub(crate) fn id(value: &str) -> &str {
    let fits = value.len() <= 128
        && value.split('/').all(|segment| {
            (1..=64).contains(&segment.len())
                && segment
                    .bytes()
                    .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-'))
        });
    if fits {
        value
    } else {
        "invalid"
    }
}

/// Where a URL that can be the user's own points, as log fields: scheme, host and port, never the path (it can be
/// the secret), userinfo, query or fragment. The host is shown only when it is one of our known hosts; any other name, an IP address or a
/// local name shows its class (`loopback`, `private`, `public`) instead (the Sidevoice logging guideline, 7.1).
pub(crate) struct Origin {
    /// `url.scheme`, or `<unparsable url>`.
    pub(crate) scheme: String,
    /// `server.address`: one of our known hosts or a host class, or `<unparsable url>`.
    pub(crate) address: String,
    /// `server.port`, when the URL names one.
    pub(crate) port: Option<u16>,
}

impl Origin {
    pub(crate) fn of(raw: &str) -> Self {
        let Ok(url) = url::Url::parse(raw) else {
            return Self::unparsable();
        };
        let Some(host) = url.host() else {
            return Self::unparsable();
        };
        let address = match host {
            url::Host::Domain(name) => domain_address(name),
            url::Host::Ipv4(ip) => ip_class(IpAddr::V4(ip)).to_owned(),
            url::Host::Ipv6(ip) => ip_class(IpAddr::V6(ip)).to_owned(),
        };
        Self {
            scheme: url.scheme().to_owned(),
            address,
            port: url.port(),
        }
    }

    fn unparsable() -> Self {
        Self {
            scheme: "<unparsable url>".to_owned(),
            address: "<unparsable url>".to_owned(),
            port: None,
        }
    }
}

/// The hosts our catalogue and its redirects use: a name equal to one of these, or under it, is shown as is.
const KNOWN_HOSTS: [&str; 4] = [
    "huggingface.co",
    "hf.co",
    "github.com",
    "githubusercontent.com",
];

/// A DNS name as is when it is one of our known hosts (a dotted name can still be a tailnet or a user's own domain).
/// Else `loopback` or `private` for a local name, `public` for any other.
fn domain_address(name: &str) -> String {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    if KNOWN_HOSTS
        .iter()
        .any(|known| name == *known || name.ends_with(&format!(".{known}")))
    {
        return name;
    }
    if name == "localhost" || name.ends_with(".localhost") {
        return "loopback".to_owned();
    }
    let local = [
        "local", "lan", "home", "internal", "intranet", "corp", "private", "arpa",
    ];
    let last = name.rsplit('.').next().unwrap_or_default();
    if name.contains('.') && !local.contains(&last) {
        "public".to_owned()
    } else {
        "private".to_owned()
    }
}

fn ip_class(ip: IpAddr) -> &'static str {
    match ip {
        IpAddr::V4(ip) => v4_class(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(mapped) => v4_class(mapped),
            None => v6_class(ip),
        },
    }
}

fn v4_class(ip: Ipv4Addr) -> &'static str {
    let [a, b, ..] = ip.octets();
    let shared = a == 100 && b & 0xc0 == 64;
    if ip.is_loopback() {
        "loopback"
    } else if ip.is_private() || ip.is_link_local() || ip.is_unspecified() || shared {
        "private"
    } else {
        "public"
    }
}

fn v6_class(ip: Ipv6Addr) -> &'static str {
    let first = ip.segments()[0];
    if ip.is_loopback() {
        "loopback"
    } else if ip.is_unspecified() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80 {
        "private"
    } else {
        "public"
    }
}

impl fmt::Display for Origin {
    /// `scheme://address[:port]`, nothing of the path.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.port.is_none() && self.scheme == "<unparsable url>" {
            return f.write_str(&self.scheme);
        }
        write!(f, "{}://{}", self.scheme, self.address)?;
        if let Some(port) = self.port {
            write!(f, ":{port}")?;
        }
        Ok(())
    }
}
