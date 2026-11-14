//! What a log line may show of a value (docs/logging.md in sidevoice/.github-private, section 8). Small typed wrappers,
//! so the rule is in the type and not at the call site.

use std::fmt;

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
