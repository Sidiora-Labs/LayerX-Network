use crate::gateway_http::{Client, Endpoint, OutboundRequest};
use crate::secret::{read_secret, required_env, valid_hex};
use native_tls::{Certificate, Identity};
use zeroize::Zeroizing;

pub struct PrincipalClient {
    client: Client,
    endpoint: Endpoint,
    token: Zeroizing<String>,
}

impl PrincipalClient {
    #[must_use]
    pub fn new(client: Client, endpoint: Endpoint, token: Zeroizing<String>) -> Self {
        Self {
            client,
            endpoint,
            token,
        }
    }

    /// # Errors
    /// Refuses incomplete mounted TLS or identity authority configuration.
    pub fn from_environment(prefix: &str) -> Result<Self, String> {
        let ca = std::fs::read(required_env(&format!("{prefix}_CA_DER"))?)
            .map_err(|_| "identity CA unavailable")?;
        let identity = std::fs::read(required_env(&format!("{prefix}_CLIENT_IDENTITY_PKCS12"))?)
            .map_err(|_| "identity client unavailable")?;
        let password = read_secret(&format!("{prefix}_CLIENT_IDENTITY_PASSWORD_FILE"))?;
        Ok(Self {
            client: Client::new(
                Certificate::from_der(&ca).map_err(|_| "identity CA invalid")?,
                Identity::from_pkcs12(&identity, &password)
                    .map_err(|_| "identity client invalid")?,
            ),
            endpoint: Endpoint::parse(&required_env(&format!("{prefix}_URL"))?)?,
            token: read_secret(&format!("{prefix}_TOKEN_FILE"))?,
        })
    }

    /// # Errors
    /// Refuses keys the identity authority cannot resolve to one digest.
    pub fn resolve(&self, key: &str) -> Result<String, String> {
        let response = self.client.request_with_principal(
            &self.endpoint,
            &format!("Bearer {}", self.token.as_str()),
            &OutboundRequest {
                method: "GET",
                path: "/internal/v1/principal",
                idempotency: None,
                content_type: "application/json",
                body: &[],
            },
            None,
            Some(key),
        )?;
        if response.status != 200 || response.content_type != "application/json" {
            return Err("publication principal unresolved".to_owned());
        }
        let envelope: serde_json::Value = serde_json::from_slice(&response.body)
            .map_err(|_| "identity principal response malformed")?;
        let digest = envelope["result"]["principal_digest"]
            .as_str()
            .filter(|digest| valid_hex(digest, 32))
            .filter(|_| envelope["ok"] == true)
            .ok_or_else(|| "identity principal response invalid".to_owned())?;
        Ok(digest.to_owned())
    }
}
