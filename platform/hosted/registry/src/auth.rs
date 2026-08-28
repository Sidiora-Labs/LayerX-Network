//! Constant-time authorization for the registry's two least-privilege planes.

use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;
use zeroize::{Zeroize, Zeroizing};

/// Result of authenticating a registry request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Authorization {
    Request,
    Publication,
}

#[cfg(test)]
mod tests {
    use super::RegistryAuthority;
    use zeroize::Zeroizing;

    #[test]
    fn bearer_authority_refuses_absent_wrong_and_ambiguous_credentials() {
        let authority = RegistryAuthority::new(Zeroizing::new("registry-secret".to_owned()))
            .unwrap_or_else(|error| panic!("authority fixture refused: {error}"));
        assert!(authority.verifies(Some("Bearer registry-secret")));
        assert!(!authority.verifies(None));
        assert!(!authority.verifies(Some("Bearer registry-secreu")));
        assert!(!authority.verifies(Some("bearer registry-secret")));
        assert!(!authority.verifies(Some("Bearer registry-secret extra")));
    }
}

/// Fixed-size verifier derived from one protected bearer secret.
#[derive(Clone)]
pub struct RegistryAuthority {
    digest: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for RegistryAuthority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RegistryAuthority([REDACTED])")
    }
}

impl RegistryAuthority {
    /// Consumes a bounded non-empty secret and retains only its digest.
    pub fn new(mut secret: Zeroizing<String>) -> Result<Self, String> {
        if secret.is_empty() || secret.len() > 4_096 || secret.bytes().any(|byte| byte.is_ascii_control()) {
            secret.zeroize();
            return Err("registry bearer secret is outside its bound".to_owned());
        }
        let digest: [u8; 32] = Sha256::digest(secret.as_bytes()).into();
        secret.zeroize();
        Ok(Self { digest: Zeroizing::new(digest) })
    }

    /// Verifies the exact bearer credential in constant time after fixed-size hashing.
    #[must_use]
    pub fn verifies(&self, header: Option<&str>) -> bool {
        let Some(candidate) = header.and_then(|value| value.strip_prefix("Bearer ")) else {
            return false;
        };
        if candidate.is_empty() || candidate.len() > 4_096 || candidate.bytes().any(|byte| byte.is_ascii_control()) {
            return false;
        }
        let mut candidate_digest: [u8; 32] = Sha256::digest(candidate.as_bytes()).into();
        let accepted = bool::from(candidate_digest.ct_eq(self.digest.as_ref()));
        candidate_digest.zeroize();
        accepted
    }

    /// Compares two configured authorities without exposing either digest.
    #[must_use]
    pub fn same_as(&self, other: &Self) -> bool {
        bool::from(self.digest.as_ref().ct_eq(other.digest.as_ref()))
    }
}
