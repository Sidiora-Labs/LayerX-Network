use std::io::Read as _;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use zeroize::Zeroizing;

const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

enum Authorization {
    Bearer(Zeroizing<String>),
    Gateway(Zeroizing<String>),
}

pub struct Client {
    agent: ureq::Agent,
    endpoint: String,
    authorization: Option<Authorization>,
}

impl Client {
    pub fn new(endpoint: &str, token: Option<Zeroizing<String>>) -> Result<Self, String> {
        let endpoint = endpoint.trim_end_matches('/');
        if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
            return Err("environment endpoint must use http:// or https://".into());
        }
        if let Some(authority) = endpoint.strip_prefix("http://") {
            let host = authority.split('/').next().unwrap_or_default();
            let local = host == "localhost"
                || host.starts_with("localhost:")
                || host == "127.0.0.1"
                || host.starts_with("127.0.0.1:")
                || host == "[::1]"
                || host.starts_with("[::1]:");
            if !local {
                return Err("non-loopback environments must use https://".into());
            }
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .build();
        Ok(Self {
            agent: config.into(),
            endpoint: endpoint.to_owned(),
            authorization: token.map(Authorization::Bearer),
        })
    }

    pub fn new_gateway(endpoint: &str, credential: Zeroizing<String>) -> Result<Self, String> {
        let (id, secret) = credential
            .split_once(':')
            .ok_or_else(|| "stored gateway credential is malformed".to_owned())?;
        if id.is_empty()
            || id.len() > 64
            || !id.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || !secret.starts_with("lxp_live_")
            || secret.len() != 73
        {
            return Err("stored gateway credential is malformed".to_owned());
        }
        let mut client = Self::new(endpoint, None)?;
        client.authorization = Some(Authorization::Gateway(credential));
        Ok(client)
    }

    pub fn get(&self, path: &str) -> Result<Value, String> {
        let url = self.url(path)?;
        let mut request = self.agent.get(&url);
        let authorization = self.authorization_header();
        if let Some(value) = &authorization {
            request = request.header("Authorization", value.as_str());
        }
        let response = request
            .call()
            .map_err(|error| format!("GET {path} failed: {error}"))?;
        decode(response, "GET", path)
    }

    pub fn post(
        &self,
        path: &str,
        body: &Value,
        idempotency: Option<&str>,
    ) -> Result<Value, String> {
        let url = self.url(path)?;
        let mut request = self.agent.post(&url);
        let authorization = self.authorization_header();
        if let Some(value) = &authorization {
            request = request.header("Authorization", value.as_str());
        }
        if let Some(key) = idempotency {
            request = request.header("Idempotency-Key", key);
        }
        let response = request
            .send_json(body)
            .map_err(|error| format!("POST {path} failed: {error}"))?;
        decode(response, "POST", path)
    }

    pub fn post_stateful(
        &self,
        path: &str,
        body: &Value,
        idempotency: &str,
    ) -> Result<Value, String> {
        let url = self.url(path)?;
        let mut request = self.agent.post(&url).header("Idempotency-Key", idempotency);
        let authorization = self.authorization_header();
        if let Some(value) = &authorization {
            request = request.header("Authorization", value.as_str());
        }
        let response = match request.send_json(body) {
            Ok(response) => response,
            Err(error) => {
                return Ok(json!({
                    "state": "unknown",
                    "failure": {"code": "gateway_transport_unavailable", "detail": error.to_string()},
                }))
            }
        };
        decode_stateful(response, "POST", path)
    }

    pub fn post_sensitive<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &Value,
        idempotency: &str,
    ) -> Result<(u16, T), String> {
        let url = self.url(path)?;
        let mut request = self.agent.post(&url).header("Idempotency-Key", idempotency);
        let authorization = self.authorization_header();
        if let Some(value) = &authorization {
            request = request.header("Authorization", value.as_str());
        }
        let mut response = request
            .send_json(body)
            .map_err(|error| format!("POST {path} failed: {error}"))?;
        let status = response.status().as_u16();
        let source = Zeroizing::new(read_response_body(&mut response, "POST", path)?);
        if !response.status().is_success() {
            let detail = serde_json::from_str::<Value>(&source)
                .map_or_else(|_| "non-JSON error".to_owned(), |value| concise(&value));
            return Err(format!("POST {path} returned HTTP {status}: {detail}"));
        }
        serde_json::from_str(&source)
            .map(|value| (status, value))
            .map_err(|error| format!("POST {path} returned invalid JSON: {error}"))
    }

    pub fn delete(&self, path: &str) -> Result<Value, String> {
        let url = self.url(path)?;
        let mut request = self.agent.delete(&url);
        let authorization = self.authorization_header();
        if let Some(value) = &authorization {
            request = request.header("Authorization", value.as_str());
        }
        let response = request
            .call()
            .map_err(|error| format!("DELETE {path} failed: {error}"))?;
        decode(response, "DELETE", path)
    }

    fn authorization_header(&self) -> Option<Zeroizing<String>> {
        self.authorization
            .as_ref()
            .map(|authorization| match authorization {
                Authorization::Bearer(value) => {
                    Zeroizing::new(format!("Bearer {}", value.as_str()))
                }
                Authorization::Gateway(value) => {
                    Zeroizing::new(format!("LayerX-Key {}", value.as_str()))
                }
            })
    }

    fn url(&self, path: &str) -> Result<String, String> {
        if !path.starts_with('/') || path.starts_with("//") {
            return Err("request path must be absolute and cannot contain an authority".into());
        }
        Ok(format!("{}{path}", self.endpoint))
    }
}

fn decode(
    mut response: ureq::http::Response<ureq::Body>,
    method: &str,
    path: &str,
) -> Result<Value, String> {
    let status = response.status();
    let body = read_response_body(&mut response, method, path)?;
    let value = if body.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&body)
            .map_err(|error| format!("{method} {path} returned non-JSON data: {error}"))?
    };
    if status.is_success() {
        Ok(value)
    } else {
        Err(format!(
            "{method} {path} returned HTTP {}: {}",
            status.as_u16(),
            concise(&value)
        ))
    }
}

fn decode_stateful(
    mut response: ureq::http::Response<ureq::Body>,
    method: &str,
    path: &str,
) -> Result<Value, String> {
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get("Retry-After")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let body = match read_response_body(&mut response, method, path) {
        Ok(body) => body,
        Err(error) => {
            return Ok(json!({
                "state": "unknown",
                "failure": {
                    "code": "gateway_response_unreadable",
                    "detail": format!("{method} {path} returned an unreadable response: {error}"),
                    "http_status": status,
                    "retry_after_seconds": retry_after,
                },
            }))
        }
    };
    let value = if body.trim().is_empty() {
        Value::Null
    } else {
        match serde_json::from_str(&body) {
            Ok(value) => value,
            Err(error) => {
                return Ok(json!({
                    "state": "unknown",
                    "failure": {
                        "code": "gateway_response_invalid",
                        "detail": format!("{method} {path} returned non-JSON data: {error}"),
                        "http_status": status,
                        "retry_after_seconds": retry_after,
                    },
                }))
            }
        }
    };
    if (200..300).contains(&status) {
        return Ok(value);
    }
    let state = if (400..500).contains(&status) {
        "refused"
    } else {
        "unknown"
    };
    Ok(json!({
        "state": state,
        "failure": {
            "http_status": status,
            "response": value,
            "retry_after_seconds": retry_after,
        }
    }))
}

fn read_response_body(
    response: &mut ureq::http::Response<ureq::Body>,
    method: &str,
    path: &str,
) -> Result<String, String> {
    let mut body = String::new();
    response
        .body_mut()
        .as_reader()
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_string(&mut body)
        .map_err(|error| format!("{method} {path} returned an unreadable response: {error}"))?;
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "{method} {path} returned a response larger than {MAX_RESPONSE_BYTES} bytes"
        ));
    }
    Ok(body)
}

fn concise(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "response encoding failed".into())
}

pub fn validate_resource_id(value: &str, name: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
    {
        return Err(format!(
            "{name} contains characters unsafe for an endpoint path"
        ));
    }
    Ok(())
}

pub fn validate_idempotency_key(value: &str) -> Result<(), String> {
    if value.len() < 16
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(
            "idempotency key must be 16-128 ASCII letters, digits, dashes, or underscores".into(),
        );
    }
    Ok(())
}
