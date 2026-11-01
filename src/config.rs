//! Environment configuration, validated once at startup.
//!
//! The variables are documented in `docs/operations.md`.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openssl::bn::{BigNum, BigNumContext};
use openssl::ec::{EcGroup, EcPoint, PointConversionForm};
use openssl::nid::Nid;
use sqlx::sqlite::SqliteConnectOptions;
use url::Url;

const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
const DEFAULT_PORT: u16 = 8080;

#[derive(Debug, Clone)]
pub struct Config {
    pub database: SqliteConnectOptions,
    pub public_url: Url,
    pub vapid_private_key: VapidPrivateKey,
    pub listen: SocketAddr,
}

/// The Web Push signing key: a raw P-256 private key, base64url without padding, with the
/// public key derived from it.
#[derive(Clone)]
pub struct VapidPrivateKey {
    private: String,
    public: String,
}

impl VapidPrivateKey {
    #[must_use]
    pub fn as_base64(&self) -> &str {
        &self.private
    }

    /// The public key a browser subscribes with (its `applicationServerKey`): the uncompressed
    /// P-256 point, base64url without padding.
    #[must_use]
    pub fn public_key(&self) -> &str {
        &self.public
    }
}

impl fmt::Debug for VapidPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VapidPrivateKey(<redacted>)")
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{0} is not set")]
    Missing(&'static str),
    #[error("{var} is invalid: {reason}")]
    Invalid { var: &'static str, reason: String },
}

impl Config {
    /// Reads and validates the configuration from the process environment.
    ///
    /// # Errors
    ///
    /// Returns the first variable that is missing or invalid.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Reads and validates the configuration from `lookup`, which maps a variable name to its value.
    ///
    /// # Errors
    ///
    /// Returns the first variable that is missing or invalid.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |var: &'static str| lookup(var).filter(|value| !value.trim().is_empty());
        let require = |var: &'static str| get(var).ok_or(ConfigError::Missing(var));

        let database = parse_database(&require("DATABASE_URL")?)?;
        let public_url = parse_public_url(&require("PUBLIC_URL")?)?;
        let vapid_private_key = parse_vapid_private_key(require("VAPID_PRIVATE_KEY")?)?;
        let host = parse_or("HOST", get("HOST"), DEFAULT_HOST, "expected an IP address")?;
        let port = parse_or("PORT", get("PORT"), DEFAULT_PORT, "expected a port number")?;

        Ok(Self {
            database,
            public_url,
            vapid_private_key,
            listen: SocketAddr::new(host, port),
        })
    }
}

fn invalid(var: &'static str, reason: impl Into<String>) -> ConfigError {
    ConfigError::Invalid {
        var,
        reason: reason.into(),
    }
}

fn parse_or<T: FromStr>(
    var: &'static str,
    value: Option<String>,
    default: T,
    reason: &str,
) -> Result<T, ConfigError> {
    value.map_or(Ok(default), |value| {
        value.parse().map_err(|_| invalid(var, reason))
    })
}

fn parse_database(value: &str) -> Result<SqliteConnectOptions, ConfigError> {
    if !value.starts_with("sqlite:") {
        return Err(invalid("DATABASE_URL", "expected a sqlite: URL"));
    }
    SqliteConnectOptions::from_str(value)
        .map(|options| options.create_if_missing(true))
        .map_err(|err| invalid("DATABASE_URL", err.to_string()))
}

fn parse_public_url(value: &str) -> Result<Url, ConfigError> {
    let url = Url::parse(value).map_err(|err| invalid("PUBLIC_URL", err.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid("PUBLIC_URL", "expected an http or https URL"));
    }
    if url.host().is_none() {
        return Err(invalid("PUBLIC_URL", "expected a host"));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(invalid("PUBLIC_URL", "expected an origin with no path"));
    }
    Ok(url)
}

fn parse_vapid_private_key(value: String) -> Result<VapidPrivateKey, ConfigError> {
    let bytes = match URL_SAFE_NO_PAD.decode(&value) {
        Ok(bytes) if bytes.len() == 32 => bytes,
        _ => {
            return Err(invalid(
                "VAPID_PRIVATE_KEY",
                "expected a 32-byte key, base64url without padding",
            ));
        }
    };
    let public = vapid_public_key(&bytes)
        .ok_or_else(|| invalid("VAPID_PRIVATE_KEY", "expected a valid P-256 private key"))?;
    Ok(VapidPrivateKey {
        private: value,
        public: URL_SAFE_NO_PAD.encode(public),
    })
}

/// The uncompressed public point of the P-256 private key `scalar`, if it is one: from 1 to
/// the curve's order less 1.
fn vapid_public_key(scalar: &[u8]) -> Option<Vec<u8>> {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).ok()?;
    let mut ctx = BigNumContext::new().ok()?;
    let mut order = BigNum::new().ok()?;
    group.order(&mut order, &mut ctx).ok()?;
    let scalar = BigNum::from_slice(scalar).ok()?;
    if scalar.num_bits() == 0 || scalar >= order {
        return None;
    }
    let mut point = EcPoint::new(&group).ok()?;
    point.mul_generator2(&group, &scalar, &mut ctx).ok()?;
    point
        .to_bytes(&group, PointConversionForm::UNCOMPRESSED, &mut ctx)
        .ok()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// The private key 1, whose public key is the curve's generator.
    const VALID_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE";
    /// P-256's generator point, uncompressed.
    const GENERATOR: &str =
        "BGsX0fLhLEJH-Lzm5WOkQPJ3A32BLeszoPShOUXYmMKWT-NC4v4af5uO5-tKfA-eFivOM1drMV7Oy7ZAaDe_UfU";
    /// P-256's order, one more than the largest private key.
    const ORDER: &str = "_____wAAAAD__________7zm-q2nF56E87nKwvxjJVE";

    fn valid_env() -> HashMap<&'static str, String> {
        HashMap::from([
            ("DATABASE_URL", "sqlite:asgard.db".to_owned()),
            ("PUBLIC_URL", "https://asgard.joseforge.cz".to_owned()),
            ("VAPID_PRIVATE_KEY", VALID_KEY.to_owned()),
        ])
    }

    fn load(env: &HashMap<&'static str, String>) -> Result<Config, ConfigError> {
        Config::from_lookup(|name| env.get(name).cloned())
    }

    fn load_with(var: &'static str, value: &str) -> Result<Config, ConfigError> {
        let mut env = valid_env();
        env.insert(var, value.to_owned());
        load(&env)
    }

    fn invalid_var(result: Result<Config, ConfigError>) -> &'static str {
        match result {
            Err(ConfigError::Invalid { var, .. }) => var,
            other => panic!("expected an invalid variable, got {other:?}"),
        }
    }

    #[test]
    fn loads_a_valid_environment_with_default_listen_address() {
        let config = load(&valid_env()).unwrap();

        assert_eq!(config.public_url.as_str(), "https://asgard.joseforge.cz/");
        assert_eq!(config.vapid_private_key.as_base64(), VALID_KEY);
        assert_eq!(config.listen, "0.0.0.0:8080".parse().unwrap());
    }

    #[test]
    fn reads_host_and_port() {
        let mut env = valid_env();
        env.insert("HOST", "127.0.0.1".to_owned());
        env.insert("PORT", "3000".to_owned());

        let config = load(&env).unwrap();

        assert_eq!(config.listen, "127.0.0.1:3000".parse().unwrap());
    }

    #[test]
    fn names_each_missing_required_variable() {
        for var in ["DATABASE_URL", "PUBLIC_URL", "VAPID_PRIVATE_KEY"] {
            let mut env = valid_env();
            env.remove(var);
            assert_eq!(load(&env).unwrap_err(), ConfigError::Missing(var));
        }
    }

    #[test]
    fn treats_a_blank_value_as_missing() {
        assert_eq!(
            load_with("PUBLIC_URL", "  ").unwrap_err(),
            ConfigError::Missing("PUBLIC_URL")
        );
    }

    #[test]
    fn rejects_a_non_sqlite_database_url() {
        assert_eq!(
            invalid_var(load_with("DATABASE_URL", "postgres://localhost/asgard")),
            "DATABASE_URL"
        );
    }

    #[test]
    fn rejects_a_public_url_that_is_not_an_http_origin() {
        for value in [
            "asgard.joseforge.cz",
            "ftp://asgard.joseforge.cz",
            "https://asgard.joseforge.cz/app",
            "https://asgard.joseforge.cz/?x=1",
        ] {
            assert_eq!(
                invalid_var(load_with("PUBLIC_URL", value)),
                "PUBLIC_URL",
                "{value}"
            );
        }
    }

    #[test]
    fn rejects_a_malformed_vapid_key() {
        for value in ["not base64!", "AAAA", &format!("{VALID_KEY}=")] {
            assert_eq!(
                invalid_var(load_with("VAPID_PRIVATE_KEY", value)),
                "VAPID_PRIVATE_KEY",
                "{value}"
            );
        }
    }

    #[test]
    fn derives_the_vapid_public_key() {
        let config = load(&valid_env()).unwrap();

        assert_eq!(config.vapid_private_key.public_key(), GENERATOR);
    }

    #[test]
    fn rejects_a_vapid_key_outside_the_curve_order() {
        for value in ["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", ORDER] {
            assert_eq!(
                load_with("VAPID_PRIVATE_KEY", value).unwrap_err(),
                invalid("VAPID_PRIVATE_KEY", "expected a valid P-256 private key"),
                "{value}"
            );
        }
    }

    #[test]
    fn rejects_an_invalid_host_or_port() {
        assert_eq!(invalid_var(load_with("HOST", "localhost")), "HOST");
        assert_eq!(invalid_var(load_with("PORT", "99999")), "PORT");
    }

    #[test]
    fn never_prints_the_vapid_key() {
        let config = load(&valid_env()).unwrap();

        assert!(!format!("{config:?}").contains(VALID_KEY));
    }
}
