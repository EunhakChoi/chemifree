//! Finds the address of the running Chemifree API server.
//!
//! The adapter never starts a server and never takes the server lock. Unless an address is given
//! explicitly, it reads the address that the running server recorded in its data directory, with
//! [`ServerLock::recorded_holder`]. A record can outlive its server, so the address is only where to
//! try: the client checks that a server answers there.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use chemifree_providers::ServerLock;

/// The text that every accepted address starts with: the scheme, the loopback host and the colon.
const URL_PREFIX: &str = "http://127.0.0.1:";
/// The scheme, which the `Host` header leaves out.
const SCHEME: &str = "http://";
/// The most digits that the port of an accepted address may have.
const MAX_PORT_DIGITS: usize = 5;

/// The address of a running Chemifree API server: `http://127.0.0.1:` followed by a port of one to
/// five ASCII digits, and nothing else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerUrl(String);

impl ServerUrl {
    /// The address as it was given, for example `http://127.0.0.1:4141`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The host and port that the `Host` header names, for example `127.0.0.1:4141`.
    #[must_use]
    pub fn authority(&self) -> &str {
        // Every value starts with the scheme, because `from_str` accepts only the URL prefix.
        &self.0[SCHEME.len()..]
    }
}

impl fmt::Display for ServerUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ServerUrl {
    type Err = InvalidServerUrl;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let port = input.strip_prefix(URL_PREFIX).unwrap_or_default();
        // A canonical port number from 1 to 65535: the API's Host guard compares the numeric port, so a
        // leading zero would connect and then be refused as a foreign host.
        let is_port = (1..=MAX_PORT_DIGITS).contains(&port.len())
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && !port.starts_with('0')
            && port.parse::<u16>().is_ok();
        if is_port {
            Ok(Self(input.to_owned()))
        } else {
            Err(InvalidServerUrl)
        }
    }
}

/// The text is not the address of a server on loopback. The form is `http://127.0.0.1:<port>`, with a
/// port from 1 to 65535 without leading zeros and nothing after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidServerUrl;

impl fmt::Display for InvalidServerUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a server address must be http://127.0.0.1:<port>, with a port from 1 to 65535 written without leading zeros and nothing after it",
        )
    }
}

impl std::error::Error for InvalidServerUrl {}

/// Where the running server is looked for: at an explicit address, or at the record in a data
/// directory. An explicit address takes precedence.
#[derive(Clone, Debug)]
pub struct Discovery {
    explicit: Option<ServerUrl>,
    data_dir: Option<PathBuf>,
}

impl Discovery {
    /// Discovery at `explicit` when it is given, and otherwise at the record in `data_dir`.
    #[must_use]
    pub fn new(explicit: Option<ServerUrl>, data_dir: Option<PathBuf>) -> Self {
        Self { explicit, data_dir }
    }

    /// Finds the server's address now.
    ///
    /// An explicit address is returned without reading anything. Otherwise the record is read on a
    /// blocking thread, so that the file system does not stall the async runtime.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError::NoDataDirectory`] when there is neither an address nor a data
    /// directory, [`DiscoveryError::NoServerRecorded`] when the data directory's record names no
    /// server, and [`DiscoveryError::Lookup`] when the blocking read was interrupted.
    pub async fn locate(&self) -> Result<ServerUrl, DiscoveryError> {
        if let Some(url) = &self.explicit {
            return Ok(url.clone());
        }
        let data_dir = self
            .data_dir
            .clone()
            .ok_or(DiscoveryError::NoDataDirectory)?;
        let lookup_dir = data_dir.clone();
        let recorded =
            tokio::task::spawn_blocking(move || ServerLock::recorded_holder(&lookup_dir).url)
                .await
                .map_err(DiscoveryError::Lookup)?;
        // The provider accepts the same address form as `ServerUrl`, so a recorded address parses.
        recorded
            .and_then(|url| url.parse::<ServerUrl>().ok())
            .ok_or(DiscoveryError::NoServerRecorded { data_dir })
    }
}

/// The server's address could not be found.
#[derive(Debug)]
#[non_exhaustive]
pub enum DiscoveryError {
    /// Neither an address nor a data directory is configured.
    NoDataDirectory,
    /// The data directory's record names no running server.
    NoServerRecorded { data_dir: PathBuf },
    /// The blocking task that reads the record was interrupted.
    Lookup(tokio::task::JoinError),
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDataDirectory => f.write_str(
                "No data directory is known, so no running Chemifree API server can be found. Pass --server-url, or --data-dir, or set CHEMIFREE_DATA_DIR.",
            ),
            Self::NoServerRecorded { data_dir } => write!(
                f,
                "No running Chemifree API server was found for {}. Start chemifree-api for that data directory.",
                data_dir.display()
            ),
            Self::Lookup(error) => write!(f, "The server record could not be read: {error}."),
        }
    }
}

impl std::error::Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lookup(error) => Some(error),
            Self::NoDataDirectory | Self::NoServerRecorded { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_loopback_addresses_with_a_port_of_one_to_five_digits() {
        for input in [
            "http://127.0.0.1:1",
            "http://127.0.0.1:4141",
            "http://127.0.0.1:65535",
        ] {
            let url = input
                .parse::<ServerUrl>()
                .unwrap_or_else(|error| panic!("{input:?} should be accepted, got {error}"));
            assert_eq!(url.as_str(), input, "the address is kept as given");
        }
    }

    #[test]
    fn the_authority_is_the_host_header_value() {
        let url: ServerUrl = "http://127.0.0.1:4141"
            .parse()
            .expect("the address is accepted");
        assert_eq!(url.authority(), "127.0.0.1:4141");
    }

    #[test]
    fn refuses_other_hosts_paths_ports_and_empty_text() {
        let cases = [
            ("", "empty"),
            ("http://127.0.0.1:", "no port"),
            ("http://localhost:4141", "foreign host"),
            ("https://127.0.0.1:4141", "https"),
            ("http://127.0.0.1:4141/", "trailing slash"),
            ("http://127.0.0.1:4141/api/v1/session", "trailing path"),
            ("http://127.0.0.1:123456", "six-digit port"),
            ("http://127.0.0.1:41a1", "letter in the port"),
            ("http://127.0.0.1:+4141", "sign in the port"),
            ("http://127.0.0.1:0", "port zero"),
            ("http://127.0.0.1:04141", "leading zero"),
            ("http://127.0.0.1:65536", "port above 65535"),
            ("http://127.0.0.1:99999", "five digits above 65535"),
        ];
        for (input, case) in cases {
            assert_eq!(
                input.parse::<ServerUrl>(),
                Err(InvalidServerUrl),
                "{case}: {input:?} must be refused"
            );
        }
    }

    #[tokio::test]
    async fn an_explicit_address_is_used_without_a_data_directory() {
        let url: ServerUrl = "http://127.0.0.1:4141"
            .parse()
            .expect("the address is accepted");
        let found = Discovery::new(Some(url.clone()), None)
            .locate()
            .await
            .expect("the explicit address is returned");
        assert_eq!(found, url);
    }

    #[tokio::test]
    async fn without_an_address_or_a_data_directory_there_is_nothing_to_look_up() {
        let error = Discovery::new(None, None)
            .locate()
            .await
            .expect_err("nothing is configured");
        assert!(
            matches!(error, DiscoveryError::NoDataDirectory),
            "got {error}"
        );
    }
}
