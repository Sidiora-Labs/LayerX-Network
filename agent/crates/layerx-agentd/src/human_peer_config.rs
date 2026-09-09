use std::collections::BTreeMap;
use std::fmt;

use layerx_types::ids::Did;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ConfigError {
    entry_index: usize,
    reason: Rejection,
}

#[derive(Debug, Eq, PartialEq)]
enum Rejection {
    Fields,
    Value,
    Uid,
    Tenant,
    Principal,
    DuplicateUid,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "LAYERX_AGENT_HUMAN_PEERS entry {}: {:?}",
            self.entry_index, self.reason
        )
    }
}

impl std::error::Error for ConfigError {}

pub(super) fn parse(value: &str) -> Result<BTreeMap<u32, (String, String)>, ConfigError> {
    let mut peers = BTreeMap::new();
    for (entry_index, entry) in value.split(',').enumerate() {
        let error = |reason| ConfigError {
            entry_index,
            reason,
        };
        let mut fields = entry.split(';');
        let uid = fields.next().and_then(|field| field.strip_prefix("uid="));
        let tenant = fields
            .next()
            .and_then(|field| field.strip_prefix("tenant="));
        let principal = fields
            .next()
            .and_then(|field| field.strip_prefix("principal="));
        let (Some(uid), Some(tenant), Some(principal)) = (uid, tenant, principal) else {
            return Err(error(Rejection::Fields));
        };
        if fields.next().is_some() {
            return Err(error(Rejection::Fields));
        }
        if [uid, tenant, principal].iter().any(|value| {
            value.is_empty()
                || value
                    .chars()
                    .any(|ch| ch.is_whitespace() || ch.is_control())
        }) {
            return Err(error(Rejection::Value));
        }
        if !uid.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(error(Rejection::Uid));
        }
        let uid = uid.parse::<u32>().map_err(|_| error(Rejection::Uid))?;
        if tenant.len() > 128
            || !tenant
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(error(Rejection::Tenant));
        }
        let (method, id) = principal
            .strip_prefix("did:")
            .and_then(|value| value.split_once(':'))
            .ok_or_else(|| error(Rejection::Principal))?;
        if method.is_empty()
            || !method
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            || id.is_empty()
            || Did::new(principal.as_bytes()).is_err()
        {
            return Err(error(Rejection::Principal));
        }
        if peers
            .insert(uid, (principal.to_owned(), tenant.to_owned()))
            .is_some()
        {
            return Err(error(Rejection::DuplicateUid));
        }
    }
    Ok(peers)
}

#[cfg(test)]
mod tests {
    use super::{parse, Rejection};

    #[test]
    fn preserves_did_and_tenant_binding() -> Result<(), Box<dyn std::error::Error>> {
        let peers = parse("uid=4020;tenant=beta;principal=did:layerx:beta:alice,uid=4294967295;tenant=Beta_2-prod;principal=did:key:z123")?;
        assert_eq!(
            peers.get(&4020),
            Some(&("did:layerx:beta:alice".to_owned(), "beta".to_owned()))
        );
        assert_eq!(peers.len(), 2);
        Ok(())
    }

    #[test]
    fn refuses_positional_fields_and_separators() {
        for value in [
            "4020:did:layerx:beta:alice:beta",
            "4020:beta:did:layerx:alice",
            "",
            "uid=1;tenant=beta;principal=did:key:a;extra=x",
            "uid=1;tenant=be;ta;principal=did:key:a",
            "uid=1;tenant=beta;principal=did:key:a,broken",
            "uid=1;tenant=beta;principal=did:key:a,",
            "uid=1;tenant=beta;tenant=beta",
            "tenant=beta;uid=1;principal=did:key:a",
        ] {
            assert!(parse(value).is_err(), "accepted {value:?}");
        }
        for forbidden in [' ', '\t', '\n', '\r', '\0', '\u{7f}', '\u{a0}'] {
            for value in [
                format!("uid=1{forbidden};tenant=beta;principal=did:key:a"),
                format!("uid=1;tenant=be{forbidden}ta;principal=did:key:a"),
                format!("uid=1;tenant=beta;principal=did:key:a{forbidden}b"),
            ] {
                assert!(parse(&value).is_err(), "accepted {value:?}");
            }
        }
    }

    #[test]
    fn refuses_invalid_authority_and_duplicate_uids() {
        for (uid, tenant, principal) in [
            ("-1", "beta", "did:key:a"),
            ("+1", "beta", "did:key:a"),
            ("4294967296", "beta", "did:key:a"),
            ("", "beta", "did:key:a"),
            ("1", "", "did:key:a"),
            ("1", "beta.prod", "did:key:a"),
            ("1", "beta", "alice"),
            ("1", "beta", "did:key"),
            ("1", "beta", "did::a"),
            ("1", "beta", "did:key:"),
            ("1", "beta", "did:KEY:a"),
        ] {
            assert!(parse(&format!("uid={uid};tenant={tenant};principal={principal}")).is_err());
        }
        assert!(parse(&format!(
            "uid=1;tenant={};principal=did:key:a",
            "a".repeat(129)
        ))
        .is_err());
        assert!(parse(&format!(
            "uid=1;tenant=beta;principal=did:key:{}",
            "a".repeat(256)
        ))
        .is_err());
        let error =
            parse("uid=1;tenant=beta;principal=did:key:a,uid=1;tenant=other;principal=did:key:b")
                .err();
        assert_eq!(
            error
                .as_ref()
                .map(|error| (&error.reason, error.entry_index)),
            Some((&Rejection::DuplicateUid, 1))
        );
        assert_eq!(
            error.map(|error| error.to_string()),
            Some("LAYERX_AGENT_HUMAN_PEERS entry 1: DuplicateUid".to_owned())
        );
    }
}
