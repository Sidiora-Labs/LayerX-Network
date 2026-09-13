use super::*;

pub struct Runtime {
    core_port: u16,
    payment_port: u16,
    webhook_port: u16,
    producer_token: String,
    webhook_token: String,
}

struct Context<'a> {
    cluster: &'a Cluster,
    certificates: &'a Certificates,
    identity: &'a LocalIdentity,
    authority: &'a LocalAuthority,
    redis: &'a LocalRedis,
    gateway: &'a Gateway,
    core_port: u16,
}

impl Runtime {
    pub fn prepare(cluster: &Cluster, boundary: &Boundary) -> Self {
        Self {
            core_port: boundary.core.port,
            payment_port: free_port(),
            webhook_port: free_port(),
            producer_token: local_secret(&cluster.root, "payment-producer-token", &token()),
            webhook_token: local_secret(&cluster.root, "payment-webhook-token", &token()),
        }
    }

    pub fn configure(
        &self,
        environment: &mut BTreeMap<&'static str, String>,
        certificates: &Certificates,
    ) {
        for (prefix, port, token_file) in [
            ("PAYMENT", self.payment_port, &self.producer_token),
            ("WEBHOOKS", self.webhook_port, &self.webhook_token),
        ] {
            let names = match prefix {
                "PAYMENT" => [
                    "LAYERX_EVENTS_PAYMENT_UPSTREAM_URL",
                    "LAYERX_EVENTS_PAYMENT_UPSTREAM_CA_DER",
                    "LAYERX_EVENTS_PAYMENT_UPSTREAM_TOKEN_FILE",
                    "LAYERX_EVENTS_PAYMENT_UPSTREAM_CLIENT_IDENTITY_PKCS12",
                    "LAYERX_EVENTS_PAYMENT_UPSTREAM_CLIENT_IDENTITY_PASSWORD_FILE",
                ],
                _ => [
                    "LAYERX_EVENTS_WEBHOOKS_UPSTREAM_URL",
                    "LAYERX_EVENTS_WEBHOOKS_UPSTREAM_CA_DER",
                    "LAYERX_EVENTS_WEBHOOKS_UPSTREAM_TOKEN_FILE",
                    "LAYERX_EVENTS_WEBHOOKS_UPSTREAM_CLIENT_IDENTITY_PKCS12",
                    "LAYERX_EVENTS_WEBHOOKS_UPSTREAM_CLIENT_IDENTITY_PASSWORD_FILE",
                ],
            };
            let values = [
                origin(port),
                text(&certificates.path("ca.der")),
                token_file.clone(),
                environment["LAYERX_GATEWAY_CLIENT_IDENTITY_PKCS12"].clone(),
                environment["LAYERX_GATEWAY_CLIENT_IDENTITY_PASSWORD_FILE"].clone(),
            ];
            environment.extend(names.into_iter().zip(values));
        }
    }

    pub fn start(
        &self,
        cluster: &Cluster,
        certificates: &Certificates,
        identity: &LocalIdentity,
        authority: &LocalAuthority,
        redis: &LocalRedis,
        gateway: &Gateway,
    ) -> Vec<Daemon> {
        let context = Context {
            cluster,
            certificates,
            identity,
            authority,
            redis,
            gateway,
            core_port: self.core_port,
        };
        let key = local_secret(&cluster.root, "event-api-key", &event_key(&context));
        let credentials = local_secret(
            &cluster.root,
            "event-credentials.json",
            &serde_json::json!({cluster.treasury_did.clone(): key}).to_string(),
        );
        let mut processes = Vec::new();
        let mut sources = Vec::new();
        for kind in ["payment", "program", "journey", "approval"] {
            let port = if kind == "payment" {
                self.payment_port
            } else {
                free_port()
            };
            let poll_token = local_secret(&cluster.root, &format!("{kind}-poll-token"), &token());
            processes.push(start_source(
                &context,
                kind,
                port,
                &credentials,
                &poll_token,
                &self.producer_token,
            ));
            sources.push((kind, port, poll_token));
        }
        let kms_port = free_port();
        let kms_token = local_secret(&cluster.root, "webhook-kms-token", &token());
        processes.push(start_kms(&context, kms_port, &kms_token));
        processes.push(start_webhooks(
            &context,
            self.webhook_port,
            &self.webhook_token,
            kms_port,
            &kms_token,
            &sources,
        ));
        processes
    }
}

fn origin(port: u16) -> String {
    format!("https://localhost:{port}")
}

fn event_key(context: &Context<'_>) -> String {
    let http = Http {
        port: context.gateway.port,
        ca: Certificate::from_der(&context.certificates.ca_der).required("CA"),
        identity: None,
    };
    let key = local_json_with_idempotency(
        &http,
        "/v1/keys",
        &context.identity.session,
        "event-source-key",
        &serde_json::json!({"signer_public_key":context.identity.signer,"scopes":["activity:write","program:call"],"quota_requests":1000,"quota_window_seconds":60}),
        201,
    );
    format!(
        "{}:{}",
        key["key"]["id"].as_str().required("event key id"),
        key["key"]["secret"].as_str().required("event key secret")
    )
}

fn tls_environment(context: &Context<'_>, prefix: &str, port: u16) -> BTreeMap<String, String> {
    BTreeMap::from([
        (format!("{prefix}_LISTEN"), format!("127.0.0.1:{port}")),
        (
            format!("{prefix}_TLS_CERT_DER"),
            text(&context.certificates.path("core.der")),
        ),
        (
            format!("{prefix}_TLS_KEY_DER"),
            text(&context.certificates.path("core-key.der")),
        ),
        (
            format!("{prefix}_CLIENT_CA_DER"),
            text(&context.certificates.path("ca.der")),
        ),
    ])
}

fn upstream_environment(
    context: &Context<'_>,
    prefix: &str,
    port: u16,
    token_file: &str,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        (format!("{prefix}_UPSTREAM_URL"), origin(port)),
        (
            format!("{prefix}_UPSTREAM_CA_DER"),
            text(&context.certificates.path("ca.der")),
        ),
        (
            format!("{prefix}_UPSTREAM_TOKEN_FILE"),
            token_file.to_owned(),
        ),
        (
            format!("{prefix}_UPSTREAM_CLIENT_IDENTITY_PKCS12"),
            text(&context.certificates.path("gateway-client.p12")),
        ),
        (
            format!("{prefix}_UPSTREAM_CLIENT_IDENTITY_PASSWORD_FILE"),
            text(&context.cluster.root.join("client-password")),
        ),
    ])
}

fn start_process(
    context: &Context<'_>,
    name: &str,
    label: &str,
    port: u16,
    environment: &BTreeMap<String, String>,
) -> Daemon {
    let environment = environment
        .iter()
        .map(|(name, value)| (name.as_str(), value.clone()))
        .collect();
    let mut process = spawn(
        &local_binary(name),
        &[],
        &environment,
        false,
        context.cluster.root.join(format!("{label}.stderr")),
    );
    wait_for_port(port, &mut process, label);
    process
}

fn start_source(
    context: &Context<'_>,
    kind: &str,
    port: u16,
    credentials: &str,
    poll_token: &str,
    producer: &str,
) -> Daemon {
    let mut environment = tls_environment(context, "LAYERX_EVENTS", port);
    environment.extend(upstream_environment(
        context,
        "LAYERX_EVENTS",
        context.gateway.port,
        poll_token,
    ));
    let producers = local_secret(
        &context.cluster.root,
        &format!("{kind}-producers.json"),
        &serde_json::json!([{
            "token_file":producer,"allow_principal_digest":matches!(kind, "payment" | "program")
        }])
        .to_string(),
    );
    environment.extend([
        ("LAYERX_EVENTS_KIND".to_owned(), format!("{kind}s")),
        (
            "LAYERX_EVENTS_CREDENTIALS_FILE".to_owned(),
            credentials.to_owned(),
        ),
        ("LAYERX_EVENTS_TOKEN_FILE".to_owned(), poll_token.to_owned()),
        ("LAYERX_EVENTS_PRODUCERS_FILE".to_owned(), producers),
        (
            "LAYERX_EVENTS_STATE_DIR".to_owned(),
            text(&context.cluster.root.join(format!("{kind}-event-state"))),
        ),
    ]);
    start_process(
        context,
        "layerx-event-source",
        &format!("events-{kind}"),
        port,
        &environment,
    )
}

fn start_kms(context: &Context<'_>, port: u16, credential: &str) -> Daemon {
    let mut environment = tls_environment(context, "LAYERX_KMS", port);
    environment.extend([
        ("LAYERX_KMS_TOKEN_FILE".to_owned(), credential.to_owned()),
        (
            "LAYERX_KMS_SEAL_SECRET_FILE".to_owned(),
            local_secret(&context.cluster.root, "event-kms-seal", &token()),
        ),
        (
            "LAYERX_KMS_STATE_DIR".to_owned(),
            text(&context.cluster.root.join("event-kms-state")),
        ),
    ]);
    start_process(context, "layerx-kms", "event-kms", port, &environment)
}

fn start_webhooks(
    context: &Context<'_>,
    port: u16,
    trigger: &str,
    kms_port: u16,
    kms_token: &str,
    sources: &[(&str, u16, String)],
) -> Daemon {
    let mut environment = tls_environment(context, "LAYERX_WEBHOOKS", port);
    for (name, value) in [
        (
            "INTERNAL_CA_DER",
            text(&context.certificates.path("ca.der")),
        ),
        ("PUBLIC_CA_DER", text(&context.certificates.path("ca.der"))),
        (
            "CLIENT_IDENTITY_PKCS12",
            text(&context.certificates.path("gateway-client.p12")),
        ),
        (
            "CLIENT_IDENTITY_PASSWORD_FILE",
            text(&context.cluster.root.join("client-password")),
        ),
        (
            "CURSOR_KEY_FILE",
            local_secret(
                &context.cluster.root,
                "event-cursor-key",
                &hex_encode(&random32()),
            ),
        ),
        ("INSTANCE_ID", "gateway-integration".to_owned()),
        ("KMS_URL", origin(kms_port)),
        ("KMS_TOKEN_FILE", kms_token.to_owned()),
        (
            "REDIS_URL",
            format!("rediss://localhost:{}", context.redis.port),
        ),
        (
            "REDIS_USERNAME_FILE",
            text(&context.cluster.root.join("redis-user")),
        ),
        (
            "REDIS_PASSWORD_FILE",
            text(&context.cluster.root.join("redis-password")),
        ),
        ("IDENTITY_URL", origin(context.identity.port)),
        (
            "IDENTITY_TOKEN_FILE",
            text(&context.identity.tokens.join("webhooks")),
        ),
        ("COMPONENT_URL", origin(context.core_port)),
        (
            "COMPONENT_TOKEN_FILE",
            text(&context.cluster.root.join("component-token")),
        ),
        ("AUTHORITY_URL", origin(context.authority.port)),
        ("AUTHORITY_TOKEN_FILE", context.authority.token_file.clone()),
        (
            "SEQUENCER_PUBLIC_KEY_FILE",
            context.gateway.signer_file.clone(),
        ),
        (
            "SEQUENCER_ID_FILE",
            text(&context.cluster.root.join("sequencer-id.hex")),
        ),
        (
            "SEQUENCER_FIRST_BATCH_FILE",
            text(&context.cluster.root.join("sequencer-first-batch")),
        ),
        (
            "SEQUENCER_LAST_BATCH_FILE",
            text(&context.cluster.root.join("sequencer-last-batch")),
        ),
        ("NETWORK_ID", NETWORK_ID.to_string()),
        ("LXP_WIRE_VERSION", PROTOCOL_VERSION.to_string()),
        ("SOURCE_TRIGGER_TOKEN_FILE", trigger.to_owned()),
        (
            "OPERATOR_TOKEN_FILE",
            local_secret(&context.cluster.root, "event-operator-token", &token()),
        ),
    ] {
        environment.insert(format!("LAYERX_WEBHOOKS_{name}"), value);
    }
    for (kind, port, token_file) in sources {
        let stem = kind.to_ascii_uppercase();
        environment.insert(format!("LAYERX_WEBHOOKS_{stem}_SOURCE_URL"), origin(*port));
        environment.insert(
            format!("LAYERX_WEBHOOKS_{stem}_SOURCE_TOKEN_FILE"),
            token_file.clone(),
        );
    }
    start_process(
        context,
        "layerx-webhooks",
        "payment-webhooks",
        port,
        &environment,
    )
}
