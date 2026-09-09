use super::*;
use std::sync::Mutex;

static READ_WINDOW: Mutex<(u64, u32)> = Mutex::new((0, 0));
const READS_PER_SECOND: u32 = 120;

fn consume_read() -> bool {
    let Ok(second) = now() else { return false };
    let Ok(mut window) = READ_WINDOW.lock() else { return false };
    if window.0 != second {
        *window = (second, 0);
    }
    if window.1 >= READS_PER_SECOND {
        return false;
    }
    window.1 += 1;
    true
}

pub(super) fn target(path: &str) -> Result<Option<&str>, ()> {
    if path == "/v1/state" {
        return Ok(Some(path));
    }
    let parts: Vec<_> = path.split('/').collect();
    match parts.as_slice() {
        ["", "v1", "accounts", id, "balance"] => {
            parse_hex32(id).map_err(|_| ())?;
            Ok(Some(path))
        }
        ["", "v1", "dids", did, "accounts"] => {
            layerx_types::ids::Did::new(did.as_bytes()).map_err(|_| ())?;
            if did.bytes().any(|b| !b.is_ascii_alphanumeric() && !b"-._:".contains(&b)) {
                return Err(());
            }
            Ok(Some(path))
        }
        _ => Ok(None),
    }
}

pub(super) fn read(config: &Config, path: &str) -> OutgoingResponse {
    if !consume_read() {
        return response(429, "public_read_rate_limit", Some(1));
    }
    let Some(endpoint) = &config.public_core else {
        return response(503, "public_core_not_configured", Some(30));
    };
    let upstream = match upstream_json(config, endpoint, config.component_token.as_str(), "GET", path, None, &[]) {
        Ok(upstream) => upstream,
        Err(error) => return error,
    };
    if upstream.content_type != "application/json" {
        return response(502, "invalid_core_response", None);
    }
    let Ok(body) = serde_json::from_slice::<serde_json::Value>(&upstream.body) else {
        return response(502, "invalid_core_response", None);
    };
    json_response(upstream.status, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_read_routes_validate_identifiers_and_exact_paths() {
        let id = "ab".repeat(32);
        let path = format!("/v1/accounts/{id}/balance");
        assert_eq!(target(&path), Ok(Some(path.as_str())));
        assert_eq!(target("/v1/state"), Ok(Some("/v1/state")));
        assert_eq!(target("/v1/dids/did:layerx:alice/accounts"), Ok(Some("/v1/dids/did:layerx:alice/accounts")));
        for path in ["/v1/accounts/123/balance", "/v1/dids/did:layerx:alice?admin/accounts", "/v1/dids//accounts"] {
            assert!(target(path).is_err(), "{path}");
        }
        for path in ["/v1/state?x=1", "/v1/accounts", "/v1/activities", "/v1/dids/did:layerx:alice/accounts/extra"] {
            assert_eq!(target(path), Ok(None));
        }
    }
}
