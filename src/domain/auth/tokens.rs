//! Bearer secrets: random 256-bit values handed out once, of which only a hash is stored.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// Defines a secret type, so a link token can't be passed where a session token is expected.
macro_rules! secret {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq)]
        pub struct $name(String);

        impl $name {
            /// A fresh random value.
            #[must_use]
            pub fn generate() -> Self {
                Self(random())
            }

            /// A value a client sent back. Nothing about it is trusted until its hash is found.
            #[must_use]
            pub const fn from_client(value: String) -> Self {
                Self(value)
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// What the database stores in place of the value.
            #[must_use]
            pub fn hash(&self) -> Vec<u8> {
                Sha256::digest(self.0.as_bytes()).to_vec()
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

secret!(
    /// The secret in an Invite or Device link's URL.
    LinkToken
);

secret!(
    /// The session id a browser keeps in its cookie.
    SessionToken
);

/// 256 bits from rand's cryptographically secure thread generator, base64url without padding.
fn random() -> String {
    let bytes: [u8; 32] = rand::random();
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_256_bits_of_url_safe_text() {
        let token = SessionToken::generate();

        assert_eq!(URL_SAFE_NO_PAD.decode(token.as_str()).unwrap().len(), 32);
    }

    #[test]
    fn every_token_is_different() {
        assert_ne!(LinkToken::generate(), LinkToken::generate());
    }

    #[test]
    fn the_hash_is_stable_and_is_not_the_token() {
        let token = LinkToken::generate();
        let sent_back = LinkToken::from_client(token.as_str().to_owned());

        assert_eq!(token.hash(), sent_back.hash());
        assert_ne!(token.hash(), token.as_str().as_bytes());
    }

    #[test]
    fn never_prints_the_token() {
        let token = SessionToken::generate();

        assert!(!format!("{token:?}").contains(token.as_str()));
    }
}
