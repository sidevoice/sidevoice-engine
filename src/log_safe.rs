//! What a log line may show of a value (docs/logging.md in sidevoice/.github-private, section 8). Small typed wrappers,
//! so the rule is in the type and not at the call site.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[cfg(test)]
mod tests;

/// A URL from our code or the catalogue, as a log field (`%UrlPath(url)`): scheme, host, port and path, never the
/// userinfo, the query or the fragment. A string that does not parse, or has no host, prints `<unparsable url>`.
pub(crate) struct UrlPath<'a>(pub(crate) &'a str);

impl fmt::Display for UrlPath<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match url::Url::parse(self.0) {
            Ok(url) if url.host_str().is_some() => {
                write!(
                    f,
                    "{}://{}",
                    url.scheme(),
                    url.host_str().unwrap_or_default()
                )?;
                if let Some(port) = url.port() {
                    write!(f, ":{port}")?;
                }
                f.write_str(url.path())
            }
            _ => f.write_str("<unparsable url>"),
        }
    }
}

/// An id that came from a catalogue entry, which can be a third party's: as is if it matches `[a-z0-9_.-]{1,64}`,
/// else `invalid`. A hostile id could forge lines or inject instructions into an agent reading the log.
pub(crate) fn id(value: &str) -> &str {
    let fits = (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-'));
    if fits {
        value
    } else {
        "invalid"
    }
}

/// Where a URL that can be the user's own points, as log fields: scheme, host and port, never the path (it can be
/// the secret), userinfo, query or fragment. The host is shown only when it is a public DNS name; an IP address or a
/// local name shows its class (`loopback`, `private`, `public`) instead (docs/logging.md, 7.1).
pub(crate) struct Origin {
    /// `url.scheme`, or `<unparsable url>`.
    pub(crate) scheme: String,
    /// `server.address`: a public DNS name or a host class, or `<unparsable url>`.
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

/// A DNS name as is when it is public: more than one label and no local suffix. Else its class.
fn domain_address(name: &str) -> String {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    if name == "localhost" || name.ends_with(".localhost") {
        return "loopback".to_owned();
    }
    let local = [
        "local", "lan", "home", "internal", "intranet", "corp", "private", "arpa",
    ];
    let last = name.rsplit('.').next().unwrap_or_default();
    let public = name.contains('.')
        && !local.contains(&last)
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
    if public {
        name
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
