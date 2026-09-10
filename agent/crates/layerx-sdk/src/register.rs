//! Self-service principal registration against the public JSON-RPC gateway.
//!
//! A principal is named by a subject derived from the signer public key it
//! authorises, so registration is idempotent and no caller can name the
//! principal of a key it does not hold. Possession is proven by an Ed25519
//! signature over [`binding`], which the gateway verifies before it provisions
//! anything.

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::rpc::{encode_hex, RpcError};

/// The public beta tenant every self-service principal is created in.
pub const TENANT: &str = "beta";

const BINDING_DOMAIN: &[u8] = b"layerx-register-binding-v1";
const SUBJECT_DOMAIN: &[u8] = b"layerx-register-subject-v1";

fn tagged(domain: &[u8], tenant: &str, signer_public_key: &[u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for part in [domain, tenant.as_bytes(), signer_public_key.as_slice()] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hash.finalize().into()
}

/// The 32-byte digest a registering key must sign to prove possession.
#[must_use]
pub fn binding(tenant: &str, signer_public_key: &[u8; 32]) -> [u8; 32] {
    tagged(BINDING_DOMAIN, tenant, signer_public_key)
}

/// The identity subject the gateway derives for one tenant and signer key.
#[must_use]
pub fn subject(tenant: &str, signer_public_key: &[u8; 32]) -> String {
    format!(
        "{tenant}.{}",
        &encode_hex(&tagged(SUBJECT_DOMAIN, tenant, signer_public_key))[..32]
    )
}

/// One identity principal confirmed to name the registering signer key.
#[derive(Clone, Debug, PartialEq)]
pub struct Registration {
    tenant: String,
    sub: String,
    signer_public_key: [u8; 32],
    raw: serde_json::Map<String, Value>,
}

impl Registration {
    #[must_use]
    pub fn tenant(&self) -> &str {
        &self.tenant
    }

    #[must_use]
    pub fn sub(&self) -> &str {
        &self.sub
    }

    #[must_use]
    pub const fn signer_public_key(&self) -> &[u8; 32] {
        &self.signer_public_key
    }

    #[must_use]
    pub const fn unverified_fields(&self) -> &serde_json::Map<String, Value> {
        &self.raw
    }

    #[must_use]
    pub fn into_value(self) -> Value {
        Value::Object(self.raw)
    }
}

pub(crate) fn decode(signer_public_key: [u8; 32], value: Value) -> Result<Registration, RpcError> {
    let Value::Object(raw) = value else {
        return Err(RpcError::InvalidResponse);
    };
    let sub = subject(TENANT, &signer_public_key);
    let signer = encode_hex(&signer_public_key);
    if raw.get("tenant").and_then(Value::as_str) != Some(TENANT)
        || raw.get("sub").and_then(Value::as_str) != Some(sub.as_str())
        || raw
            .get("allowed_signer_public_keys")
            .and_then(Value::as_array)
            .is_none_or(|keys| {
                keys.len() != 1 || keys.first().and_then(Value::as_str) != Some(signer.as_str())
            })
    {
        return Err(RpcError::InvalidResponse);
    }
    Ok(Registration {
        tenant: TENANT.to_owned(),
        sub,
        signer_public_key,
        raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn confirmed(sub: &str, signer: &str) -> Value {
        json!({
            "tenant": TENANT,
            "sub": sub,
            "allowed_signer_public_keys": [signer],
            "account": Value::Null,
            "audiences": [],
        })
    }

    #[test]
    fn the_binding_and_subject_match_the_gateway_vectors() {
        let key = [0xab_u8; 32];
        assert_eq!(
            encode_hex(&binding(TENANT, &key)),
            "425fa6c5988efc16bce8a7931f1efbc039b75d10f0ffa617c3c2d3de2366e128"
        );
        assert_eq!(
            subject(TENANT, &key),
            "beta.7399f031b011aa1198718d62c3f79984"
        );
        assert_ne!(binding(TENANT, &key), binding("gamma", &key));
        assert_ne!(subject(TENANT, &key), subject("gamma", &key));
        assert_ne!(subject(TENANT, &key), subject(TENANT, &[0xac_u8; 32]));
        assert!(!subject(TENANT, &key).contains(':'));
    }

    #[test]
    fn a_registration_is_accepted_only_when_it_names_the_registering_key() {
        let key = [0xab_u8; 32];
        let sub = subject(TENANT, &key);
        let signer = encode_hex(&key);
        let registration =
            decode(key, confirmed(&sub, &signer)).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(registration.tenant(), TENANT);
        assert_eq!(registration.sub(), sub);
        assert_eq!(registration.signer_public_key(), &key);
        assert_eq!(
            registration.unverified_fields().get("audiences"),
            Some(&json!([]))
        );
        assert_eq!(registration.into_value(), confirmed(&sub, &signer));

        for divergent in [
            confirmed("beta.0000000000000000000000000000000", &signer),
            confirmed(&sub, &encode_hex(&[0xac_u8; 32])),
            json!({"tenant":"gamma","sub":sub,"allowed_signer_public_keys":[signer]}),
            json!({"tenant":TENANT,"sub":sub,"allowed_signer_public_keys":[signer, signer]}),
            json!({"tenant":TENANT,"sub":sub,"allowed_signer_public_keys":[]}),
            json!({"tenant":TENANT,"sub":sub}),
            json!([]),
            Value::Null,
        ] {
            assert!(
                matches!(
                    decode(key, divergent.clone()),
                    Err(RpcError::InvalidResponse)
                ),
                "{divergent}"
            );
        }
    }

    #[test]
    fn a_registration_confirmed_for_another_key_is_refused() {
        let key = [0xab_u8; 32];
        let other = [0xac_u8; 32];
        let document = confirmed(&subject(TENANT, &other), &encode_hex(&other));
        assert!(matches!(
            decode(key, document.clone()),
            Err(RpcError::InvalidResponse)
        ));
        assert!(decode(other, document).is_ok());
    }
}
