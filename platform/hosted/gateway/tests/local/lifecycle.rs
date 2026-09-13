mod events;
mod funding;
mod registry_runtime;
mod required;
use required::Required;

include!(concat!(env!("OUT_DIR"), "/core_fixture.rs"));

fn local_binary(name: &str) -> PathBuf {
    let directory = PathBuf::from(
        std::env::var_os("LAYERX_TEST_SERVICE_BIN_DIR").required("parent-built service directory"),
    );
    let path = directory.join(name);
    assert!(
        path.is_absolute() && path.is_file(),
        "missing prebuilt {}",
        path.display()
    );
    path
}

fn local_secret(root: &Path, name: &str, value: &str) -> String {
    let path = root.join(name);
    write(&path, value.as_bytes(), 0o600);
    text(&path)
}

fn local_service(
    cluster: &Cluster,
    name: &str,
    port: u16,
    environment: &BTreeMap<&str, String>,
) -> Daemon {
    let mut process = spawn(
        &local_binary(name),
        &[],
        environment,
        false,
        cluster.root.join(format!("{name}.stderr")),
    );
    wait_for_port(port, &mut process, name);
    process
}

fn local_json(
    http: &Http,
    path: &str,
    bearer: &str,
    value: &serde_json::Value,
    expected: u16,
) -> serde_json::Value {
    local_json_with_idempotency(http, path, bearer, "local-key-issuance", value, expected)
}

fn local_json_with_idempotency(
    http: &Http,
    path: &str,
    bearer: &str,
    idempotency: &str,
    value: &serde_json::Value,
    expected: u16,
) -> serde_json::Value {
    let authorization = format!("Bearer {bearer}");
    let answer = http.request(
        "POST",
        path,
        &[
            ("Authorization", &authorization),
            ("Content-Type", "application/json"),
            ("Idempotency-Key", idempotency),
        ],
        &serde_json::to_vec(value).required("JSON"),
    );
    assert_eq!(
        answer.status, expected,
        "{path}: credential response body redacted"
    );
    serde_json::from_str(&answer.body).required("credential response JSON invalid; body redacted")
}

struct LocalIdentity {
    _process: Daemon,
    port: u16,
    tokens: PathBuf,
    session: String,
    signer: String,
}
struct LocalAuthority {
    _process: Daemon,
    port: u16,
    token_file: String,
}
struct LocalRedis {
    _process: Daemon,
    port: u16,
    password: String,
}
struct Gateway {
    _process: Daemon,
    _event_processes: Vec<Daemon>,
    _registry_process: Option<Daemon>,
    port: u16,
    signer_file: String,
}

#[test]
fn local_gateway_lifecycle() {
    let (cluster, _funding) = funding::start();
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let identity = start_local_identity(&cluster, &certificates);
    let authority = start_local_authority(&cluster, &certificates);
    let redis = start_local_redis(&cluster, &certificates);
    let gateway = start_gateway_runtime(
        &cluster,
        &certificates,
        &boundary,
        &identity,
        &authority,
        &redis,
        true,
    );
    let key = issue_local_key(&certificates, &gateway, &identity);
    run_lifecycle_script(&cluster, &certificates, &gateway, &authority, &key);
}

fn start_local_identity(cluster: &Cluster, certificates: &Certificates) -> LocalIdentity {
    let tokens = cluster.root.join("identity-tokens");
    make_dir(&tokens, 0o700);
    let provisioning = token();
    let gateway_token = token();
    for service in [
        "gateway",
        "registry",
        "registrar",
        "webhooks",
        "dashboard",
        "faucet",
        "testnet",
        "ramp",
        "provisioning",
    ] {
        let value = match service {
            "gateway" => gateway_token.clone(),
            "provisioning" => provisioning.clone(),
            _ => token(),
        };
        local_secret(&tokens, service, &value);
    }
    let identity_port = free_port();
    let identity_state = cluster.root.join("identity-state");
    make_dir(&identity_state, 0o700);
    let identity_env = BTreeMap::from([
        (
            "LAYERX_IDENTITY_LISTEN",
            format!("127.0.0.1:{identity_port}"),
        ),
        (
            "LAYERX_IDENTITY_TLS_CERT_DER",
            text(&certificates.path("core.der")),
        ),
        (
            "LAYERX_IDENTITY_TLS_KEY_DER",
            text(&certificates.path("core-key.der")),
        ),
        ("LAYERX_IDENTITY_STATE_DIR", text(&identity_state)),
        ("LAYERX_IDENTITY_SERVICE_TOKENS_DIR", text(&tokens)),
        (
            "LAYERX_IDENTITY_STORE_KEY_FILE",
            local_secret(&cluster.root, "identity-store-key", &token()),
        ),
        ("LAYERX_IDENTITY_SESSION_TTL_SECONDS", "3600".to_owned()),
    ]);
    let identity_process = local_service(cluster, "layerx-identity", identity_port, &identity_env);
    let identity_http = Http {
        port: identity_port,
        ca: Certificate::from_der(&certificates.ca_der).required("CA"),
        identity: None,
    };
    let signer = hex_encode(
        &SigningKey::from_bytes(&cluster.treasury_seed)
            .verifying_key()
            .to_bytes(),
    );
    local_json(
        &identity_http,
        "/v1/principals",
        &provisioning,
        &serde_json::json!({"tenant":"beta","sub":cluster.treasury_did,"allowed_signer_public_keys":[signer],"account":format!("agent:{}:main",cluster.treasury_did),"audiences":[]}),
        200,
    );
    let session = local_json(
        &identity_http,
        "/v1/sessions",
        &provisioning,
        &serde_json::json!({"sub":cluster.treasury_did}),
        200,
    );
    let session_token = session["token"]
        .as_str()
        .required("real identity session")
        .to_owned();

    LocalIdentity {
        _process: identity_process,
        port: identity_port,
        tokens,
        session: session_token,
        signer,
    }
}

fn start_local_authority(cluster: &Cluster, certificates: &Certificates) -> LocalAuthority {
    let authority_port = free_port();
    let authority_token_file = local_secret(&cluster.root, "authority-token", &token());
    let replica_token_file = local_secret(
        &cluster.root,
        "authority-replica-token",
        &cluster.replica_token,
    );
    let authority_env = BTreeMap::from([
        (
            "LAYERX_AUTHORITY_LISTEN",
            format!("127.0.0.1:{authority_port}"),
        ),
        (
            "LAYERX_AUTHORITY_TLS_CERT_DER",
            text(&certificates.path("core.der")),
        ),
        (
            "LAYERX_AUTHORITY_TLS_KEY_DER",
            text(&certificates.path("core-key.der")),
        ),
        (
            "LAYERX_AUTHORITY_CLIENT_CA_DER",
            text(&certificates.path("ca.der")),
        ),
        ("LAYERX_AUTHORITY_TOKEN_FILES", authority_token_file.clone()),
        (
            "LAYERX_AUTHORITY_REPLICA_URL",
            format!("http://127.0.0.1:{}", cluster.replica_port),
        ),
        (
            "LAYERX_AUTHORITY_REPLICA_BEARER_TOKEN_FILE",
            replica_token_file,
        ),
        (
            "LAYERX_AUTHORITY_REPLICA_ID",
            hex_encode(&sha256(&[
                b"layerx-authority-replica:",
                hex_encode(&cluster.sequencer_key).as_bytes(),
            ])),
        ),
        ("LAYERX_AUTHORITY_LNI_SOCKET", text(&cluster.lni_socket)),
        (
            "LAYERX_AUTHORITY_PROTOCOL_NETWORK_ID",
            NETWORK_ID.to_string(),
        ),
        ("LAYERX_AUTHORITY_NETWORK_ID", NETWORK_ID.to_string()),
        ("LAYERX_AUTHORITY_WIRE_VERSION", "3".to_owned()),
        (
            "LAYERX_AUTHORITY_SEQUENCER_ID",
            hex_encode(&cluster.sequencer_id),
        ),
        (
            "LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY",
            hex_encode(&cluster.sequencer_key),
        ),
        ("LAYERX_AUTHORITY_FIRST_BATCH", "1".to_owned()),
        ("LAYERX_AUTHORITY_LAST_BATCH", u64::MAX.to_string()),
    ]);
    let authority_process = local_service(
        cluster,
        "layerx-receipt-authority",
        authority_port,
        &authority_env,
    );

    LocalAuthority {
        _process: authority_process,
        port: authority_port,
        token_file: authority_token_file,
    }
}

fn start_local_redis(cluster: &Cluster, certificates: &Certificates) -> LocalRedis {
    let redis_port = free_port();
    let redis_password = token();
    let redis_directory = cluster.root.join("redis");
    make_dir(&redis_directory, 0o700);
    let acl = local_secret(
        &redis_directory,
        "users.acl",
        &format!("user default off\nuser qualification on >{redis_password} +@all ~* &*\n"),
    );
    let redis_config = format!(
        "bind 127.0.0.1\nport 0\ntls-port {redis_port}\ntls-cert-file {}\ntls-key-file {}\ntls-ca-cert-file {}\ntls-auth-clients no\naclfile {acl}\nappendonly yes\nappendfsync always\ndir {}\nprotected-mode yes\n",
        certificates.path("core.pem").display(),
        certificates.path("core-key.pem").display(),
        certificates.path("ca.pem").display(),
        redis_directory.display()
    );
    let redis_config_path = local_secret(&redis_directory, "redis.conf", &redis_config);
    let mut redis = spawn(
        Path::new("/usr/bin/redis-server"),
        &[&redis_config_path],
        &BTreeMap::new(),
        false,
        cluster.root.join("redis.stderr"),
    );
    wait_for_port(redis_port, &mut redis, "real TLS Redis");

    LocalRedis {
        _process: redis,
        port: redis_port,
        password: redis_password,
    }
}

fn start_local_gateway(
    cluster: &Cluster,
    certificates: &Certificates,
    boundary: &Boundary,
    identity: &LocalIdentity,
    authority: &LocalAuthority,
    redis: &LocalRedis,
) -> Gateway {
    start_gateway_runtime(
        cluster,
        certificates,
        boundary,
        identity,
        authority,
        redis,
        false,
    )
}

fn start_gateway_runtime(
    cluster: &Cluster,
    certificates: &Certificates,
    boundary: &Boundary,
    identity: &LocalIdentity,
    authority: &LocalAuthority,
    redis: &LocalRedis,
    registry: bool,
) -> Gateway {
    let password_file = local_secret(&cluster.root, "client-password", &token());
    let pkcs12 = certificates.path("gateway-client.p12");
    command(
        "openssl",
        &[
            "pkcs12",
            "-export",
            "-inkey",
            &text(&certificates.path("client-key.pem")),
            "-in",
            &text(&certificates.path("client.pem")),
            "-out",
            &text(&pkcs12),
            "-passout",
            &format!("file:{password_file}"),
        ],
    );
    let gateway_port = free_port();
    let signer_file = local_secret(
        &cluster.root,
        "trusted-sequencer.hex",
        &hex_encode(&cluster.sequencer_key),
    );
    let mut gateway_env = BTreeMap::from([
        ("LAYERX_GATEWAY_LISTEN", format!("127.0.0.1:{gateway_port}")),
        (
            "LAYERX_GATEWAY_TLS_CERT_DER",
            text(&certificates.path("core.der")),
        ),
        (
            "LAYERX_GATEWAY_TLS_KEY_DER",
            text(&certificates.path("core-key.der")),
        ),
        (
            "LAYERX_GATEWAY_OUTBOUND_CA_DER",
            text(&certificates.path("ca.der")),
        ),
        ("LAYERX_GATEWAY_CLIENT_IDENTITY_PKCS12", text(&pkcs12)),
        (
            "LAYERX_GATEWAY_CLIENT_IDENTITY_PASSWORD_FILE",
            password_file,
        ),
        (
            "LAYERX_GATEWAY_SEQUENCER_PUBLIC_KEY_FILE",
            signer_file.clone(),
        ),
        (
            "LAYERX_GATEWAY_SEQUENCER_ID_FILE",
            local_secret(
                &cluster.root,
                "sequencer-id.hex",
                &hex_encode(&cluster.sequencer_id),
            ),
        ),
        (
            "LAYERX_GATEWAY_SEQUENCER_FIRST_BATCH_FILE",
            local_secret(&cluster.root, "sequencer-first-batch", "1"),
        ),
        (
            "LAYERX_GATEWAY_SEQUENCER_LAST_BATCH_FILE",
            local_secret(&cluster.root, "sequencer-last-batch", &u64::MAX.to_string()),
        ),
        (
            "LAYERX_GATEWAY_KEY_PROVISIONING_KEY_FILE",
            local_secret(
                &cluster.root,
                "key-provisioning.hex",
                &hex_encode(&random32()),
            ),
        ),
        ("LAYERX_GATEWAY_NETWORK_ID", NETWORK_ID.to_string()),
        ("LAYERX_GATEWAY_PROTOCOL_NETWORK_ID", NETWORK_ID.to_string()),
        ("LAYERX_GATEWAY_LXP_WIRE_VERSION", "3".to_owned()),
        (
            "LAYERX_GATEWAY_MODULE_REGISTRY_FILE",
            local_secret(
                &cluster.root,
                "modules.json",
                &serde_json::json!({"schema_version":2,"assets":[{"asset":hex_encode(&cluster.asset),"currency":"NATIVE","decimals":0,"symbol":"LXR"}],"modules":[{"module":1,"ordinals":[1,4,5,6,7,8,10,11]},{"module":9,"ordinals":[1,2,3,5,6,7]}]}).to_string(),
            ),
        ),
    ]);
    gateway_env.extend(gateway_upstream_environment(
        cluster, boundary, identity, authority, redis,
    ));
    let events = events::Runtime::prepare(cluster, boundary);
    events.configure(&mut gateway_env, certificates);
    let registry_config = registry.then(|| {
        registry_runtime::configure(cluster, certificates, identity, authority, &mut gateway_env)
    });
    let gateway_process = local_service(cluster, "layerx-gateway", gateway_port, &gateway_env);
    let mut gateway = Gateway {
        _process: gateway_process,
        _event_processes: Vec::new(),
        _registry_process: None,
        port: gateway_port,
        signer_file,
    };
    gateway._event_processes =
        events.start(cluster, certificates, identity, authority, redis, &gateway);
    if let Some((path, port)) = registry_config {
        gateway._registry_process = Some(registry_runtime::start(cluster, &path, port));
    }
    gateway
}

fn gateway_upstream_environment(
    cluster: &Cluster,
    boundary: &Boundary,
    identity: &LocalIdentity,
    authority: &LocalAuthority,
    redis: &LocalRedis,
) -> BTreeMap<&'static str, String> {
    let identity_port = identity.port;
    let authority_port = authority.port;
    let authority_token_file = &authority.token_file;
    let tokens = &identity.tokens;
    let redis_port = redis.port;
    let redis_password = &redis.password;
    BTreeMap::from([
        (
            "LAYERX_GATEWAY_PUBLIC_CORE_URL",
            format!("https://localhost:{}", boundary.core.port),
        ),
        (
            "LAYERX_GATEWAY_COMPONENT_URL",
            format!("https://localhost:{}", boundary.core.port),
        ),
        (
            "LAYERX_GATEWAY_COMPONENT_TOKEN_FILE",
            local_secret(&cluster.root, "component-token", &cluster.program_token),
        ),
        (
            "LAYERX_GATEWAY_AUTHORITY_URL",
            format!("https://localhost:{authority_port}"),
        ),
        (
            "LAYERX_GATEWAY_AUTHORITY_TOKEN_FILE",
            authority_token_file.clone(),
        ),
        (
            "LAYERX_GATEWAY_IDENTITY_URL",
            format!("https://localhost:{identity_port}"),
        ),
        (
            "LAYERX_GATEWAY_IDENTITY_TOKEN_FILE",
            text(&tokens.join("gateway")),
        ),
        (
            "LAYERX_GATEWAY_PROGRAM_REGISTRY_URL",
            format!("https://localhost:{}", boundary.core.port),
        ),
        (
            "LAYERX_GATEWAY_PROGRAM_REGISTRY_TOKEN_FILE",
            local_secret(&cluster.root, "registry-token", &token()),
        ),
        (
            "LAYERX_GATEWAY_REDIS_URL",
            format!("rediss://localhost:{redis_port}"),
        ),
        (
            "LAYERX_GATEWAY_REDIS_USERNAME_FILE",
            local_secret(&cluster.root, "redis-user", "qualification"),
        ),
        (
            "LAYERX_GATEWAY_REDIS_PASSWORD_FILE",
            local_secret(&cluster.root, "redis-password", redis_password),
        ),
    ])
}

fn issue_local_key(
    certificates: &Certificates,
    gateway: &Gateway,
    identity: &LocalIdentity,
) -> serde_json::Value {
    issue_local_scoped_key(certificates, gateway, identity, &["program:call"])
}

fn issue_local_scoped_key(
    certificates: &Certificates,
    gateway: &Gateway,
    identity: &LocalIdentity,
    scopes: &[&str],
) -> serde_json::Value {
    let gateway_port = gateway.port;
    let session_token = identity.session.as_str();
    let signer = &identity.signer;
    let gateway_http = Http {
        port: gateway_port,
        ca: Certificate::from_der(&certificates.ca_der).required("CA"),
        identity: None,
    };
    let untrusted = Http {
        port: gateway_port,
        ca: Certificate::from_der(
            &fs::read(certificates.path("rogue-ca.der")).required("unrelated CA"),
        )
        .required("unrelated certificate"),
        identity: None,
    };
    assert!(
        untrusted
            .raw(
                "GET /livez HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                &[]
            )
            .is_err(),
        "gateway TLS must reject an unrelated trust root"
    );
    local_json(
        &gateway_http,
        "/v1/keys",
        session_token,
        &serde_json::json!({"signer_public_key":hex_encode(&SigningKey::from_bytes(&random32()).verifying_key().to_bytes()),"scopes":scopes,"quota_requests":1000,"quota_window_seconds":60}),
        403,
    );
    let key = local_json(
        &gateway_http,
        "/v1/keys",
        session_token,
        &serde_json::json!({"signer_public_key":signer,"scopes":scopes,"quota_requests":1000,"quota_window_seconds":60}),
        201,
    );
    let replayed_key = local_json(
        &gateway_http,
        "/v1/keys",
        session_token,
        &serde_json::json!({"signer_public_key":signer,"scopes":scopes,"quota_requests":1000,"quota_window_seconds":60}),
        200,
    );
    assert!(
        key["key"] == replayed_key["key"],
        "real key issuance must replay exactly; credentials redacted"
    );

    key
}

fn issue_recipient_scoped_key(
    certificates: &Certificates,
    gateway: &Gateway,
    identity: &LocalIdentity,
    funding: &funding::Funding,
    scopes: &[&str],
) -> serde_json::Value {
    let identity_http = Http {
        port: identity.port,
        ca: Certificate::from_der(&certificates.ca_der).required("CA"),
        identity: None,
    };
    let provisioning = fs::read_to_string(identity.tokens.join("provisioning"))
        .required("identity provisioning credential");
    let signer = hex_encode(
        &SigningKey::from_bytes(&funding.recipient_seed)
            .verifying_key()
            .to_bytes(),
    );
    local_json_with_idempotency(
        &identity_http,
        "/v1/principals",
        &provisioning,
        "pay6-recipient-principal",
        &serde_json::json!({"tenant":"beta","sub":funding.recipient_did,"allowed_signer_public_keys":[signer],"account":format!("agent:{}:main",funding.recipient_did),"audiences":[]}),
        200,
    );
    let session = local_json_with_idempotency(
        &identity_http,
        "/v1/sessions",
        &provisioning,
        "pay6-recipient-session",
        &serde_json::json!({"sub":funding.recipient_did}),
        200,
    );
    let gateway_http = Http {
        port: gateway.port,
        ca: Certificate::from_der(&certificates.ca_der).required("CA"),
        identity: None,
    };
    let request = serde_json::json!({"signer_public_key":signer,"scopes":scopes,"quota_requests":1000,"quota_window_seconds":60});
    let session = session["token"].as_str().required("recipient session");
    let key = local_json_with_idempotency(
        &gateway_http,
        "/v1/keys",
        session,
        "pay6-recipient-key",
        &request,
        201,
    );
    let replayed = local_json_with_idempotency(
        &gateway_http,
        "/v1/keys",
        session,
        "pay6-recipient-key",
        &request,
        200,
    );
    assert_eq!(
        key["key"], replayed["key"],
        "recipient key issuance must replay exactly; credentials redacted"
    );
    key
}

fn run_lifecycle_script(
    cluster: &Cluster,
    certificates: &Certificates,
    gateway: &Gateway,
    authority: &LocalAuthority,
    key: &serde_json::Value,
) {
    let gateway_port = gateway.port;
    let authority_port = authority.port;
    let authority_token_file = &authority.token_file;
    let signer_file = &gateway.signer_file;
    let manifest = local_manifest(cluster, 1_000_000_000_000);
    let evidence_directory = cluster.root.join("gateway-offline-evidence");
    make_dir(&evidence_directory, 0o700);
    let authority_token =
        fs::read_to_string(authority_token_file).required("local authority token");
    let authority_curl = local_secret(
        &cluster.root,
        "authority-curl.conf",
        &format!(
            "silent\nshow-error\nfail\nmax-time = 60\nproto = \"=https\"\ncacert = \"{}\"\ncert = \"{}\"\nkey = \"{}\"\nheader = \"Authorization: Bearer {}\"\n",
            certificates.path("ca.pem").display(),
            certificates.path("client.pem").display(),
            certificates.path("client-key.pem").display(),
            authority_token.trim()
        ),
    );
    let verifier_environment = BTreeMap::from([
        ("LAYERX_LIFECYCLE_MANIFEST", text(&manifest)),
        ("LAYERX_TEST_SEQUENCER_KEY_FILE", signer_file.clone()),
        (
            "LAYERX_TEST_SEQUENCER_ID",
            hex_encode(&cluster.sequencer_id),
        ),
        (
            "LAYERX_TEST_REPLICA_ID",
            hex_encode(&sha256(&[
                b"layerx-authority-replica:",
                hex_encode(&cluster.sequencer_key).as_bytes(),
            ])),
        ),
        ("LAYERX_TEST_EVIDENCE_DIR", text(&evidence_directory)),
    ]);
    let status = Command::new("/bin/sh")
        .arg(repository_root().join("platform/hosted/gateway/tests/lifecycle-boundary.sh"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env(
            "LAYERX_GATEWAY_URL",
            format!("https://localhost:{gateway_port}"),
        )
        .env("LAYERX_GATEWAY_CA_FILE", certificates.path("ca.pem"))
        .env(
            "LAYERX_GATEWAY_KEY_ID",
            key["key"]["id"].as_str().required("issued ID"),
        )
        .env(
            "LAYERX_GATEWAY_KEY_SECRET",
            key["key"]["secret"].as_str().required("issued secret"),
        )
        .env(
            "LAYERX_RECEIPT_VERIFY_BIN",
            env!("CARGO_BIN_EXE_gateway-lifecycle-receipt-verify"),
        )
        .envs(&verifier_environment)
        .env(
            "LAYERX_TEST_AUTHORITY_URL",
            format!("https://localhost:{authority_port}"),
        )
        .env("LAYERX_TEST_AUTHORITY_CURL_CONFIG", authority_curl)
        .status()
        .required("existing lifecycle qualification script");
    assert!(
        status.success(),
        "real gateway lifecycle script failed: {status}"
    );
    let receipts = fs::read_dir(&evidence_directory)
        .required("offline evidence directory")
        .map(|entry| entry.required("evidence file").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "receipt")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        receipts.len(),
        3,
        "each lifecycle operation must retain independent evidence"
    );
    for receipt in receipts {
        let status = Command::new(env!("CARGO_BIN_EXE_gateway-lifecycle-receipt-verify"))
            .arg(receipt)
            .env_clear()
            .envs(&verifier_environment)
            .status()
            .required("offline verifier without authority transport configuration");
        assert!(
            status.success(),
            "offline header/inclusion/state verification failed"
        );
    }
}

fn local_manifest(cluster: &Cluster, fee_limit: u128) -> PathBuf {
    use layerx_types::program_lifecycle::{
        NativeProgramDeploy, NativeProgramUpgrade, NativeProgramWindDown, ProgramUpgradePolicy,
        ProgramWindDownOperation,
    };
    let fixture: serde_json::Value = serde_json::from_slice(
        &fs::read(
            repository_root()
                .join("platform/sdk/conformance/fixtures/native-program-deploy-v3.json"),
        )
        .required("C fixture"),
    )
    .required("fixture JSON");
    let encoded =
        layerx_platform_core::hex_decode(fixture["payload_hex"].as_str().required("payload"))
            .required("hex");
    let original = NativeProgramDeploy::decode(&encoded).required("C deploy");
    let program = ProgramId::new(random32());
    let account =
        layerx_types::account::AccountId::parse(&format!("agent:{}:main", cluster.treasury_did))
            .required("account");
    let deploy = NativeProgramDeploy {
        program_id: program,
        policy: ProgramUpgradePolicy::Authority(
            layerx_wire::hash::account_id_for_protocol(&account, 3).required("account ID"),
        ),
        ..original
    };
    let mut wasm = deploy.wasm.to_vec();
    wasm.extend_from_slice(b"\0\x08\x07upgrade");
    let prior = layerx_programs::ProgramInterface::decode(deploy.interface.required("interface"))
        .required("interface");
    let interface = layerx_programs::ProgramInterface::bind_upgrade(
        &wasm,
        deploy.guest_abi,
        prior.entries().to_vec(),
        &prior,
        false,
    )
    .required("upgraded interface");
    let upgrade = NativeProgramUpgrade {
        program_id: program,
        guest_abi: deploy.guest_abi,
        old_hash: deploy.new_hash,
        new_hash: Sha256::digest(&wasm).into(),
        migration_hook: &[],
        clear_interface: false,
        interface: Some(interface.canonical_encoding()),
        wasm: &wasm,
    };
    let wind = NativeProgramWindDown {
        program_id: program,
        operation: ProgramWindDownOperation::Deprecate {
            exit_program: program.bytes(),
            deadline_batch: u64::MAX,
        },
    };
    let first_sequence = account_sequence(&cluster.lni_socket, &cluster.treasury_did);
    let mut entries = Vec::new();
    for (index, (ordinal, route, payload)) in [
        (1, "deploy", deploy.encode().required("deploy")),
        (2, "upgrade", upgrade.encode().required("upgrade")),
        (7, "wind-down", wind.encode().required("wind-down")),
    ]
    .into_iter()
    .enumerate()
    {
        let signed = signed_program_activity_with_fee(
            &cluster.treasury_seed,
            &cluster.treasury_did,
            first_sequence + u64::try_from(index).required("index"),
            ordinal,
            &payload,
            fee_limit,
        );
        let kind = ActivityType::new(ModuleId::Programs, ordinal).required("ordinal");
        let registry = ModuleRegistry::new(&[
            ModuleRegistration::new(ModuleId::Programs, &[kind]).required("module")
        ])
        .required("registry");
        let activity =
            layerx_wire::activity::decode_signed(&signed, &registry).required("signed lifecycle");
        assert_eq!(activity.protocol_version(), 3);
        assert_eq!(activity.activity_type(), kind);
        assert_eq!(
            layerx_wire::activity::encode_signed(&activity).required("canonical"),
            signed
        );
        let path = cluster.root.join(format!("gateway-{route}.bin"));
        write(&path, &signed, 0o600);
        entries.push(serde_json::json!({"route":route,"signed_file":path,"activity_id":hex_encode(&layerx_wire::hash::activity_id(&activity).required("activity ID")),"idempotency_key":hex_encode(&activity.idempotency_key())}));
    }
    let path = cluster.root.join("gateway-lifecycle.json");
    write(
        &path,
        &serde_json::to_vec(&entries).required("manifest JSON"),
        0o600,
    );
    path
}

#[test]
fn local_gateway_account_sequence_matches_authenticated_account() {
    let cluster = start_cluster(true);
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let activity = hex_encode(&establish_receipt_head(&boundary, &cluster));
    wait_for_published_receipt(&boundary, &cluster, &activity);
    let identity = start_local_identity(&cluster, &certificates);
    let authority = start_local_authority(&cluster, &certificates);
    let redis = start_local_redis(&cluster, &certificates);
    let gateway = start_local_gateway(
        &cluster,
        &certificates,
        &boundary,
        &identity,
        &authority,
        &redis,
    );
    let http = Http {
        port: gateway.port,
        ca: Certificate::from_der(&certificates.ca_der).required("CA"),
        identity: None,
    };
    let account = layerx_types::account::AccountId::parse("system:fees").required("account name");
    let account_id = layerx_wire::hash::account_id_for_protocol(&account, PROTOCOL_VERSION)
        .required("account id");
    let account_hex = hex_encode(&account_id);
    let direct = boundary
        .core
        .get(&format!("/v1/accounts/{account_hex}/balance"));
    assert_eq!(direct.status, 200, "{}", direct.body);
    let rpc = local_rpc(
        &http,
        "",
        "lx_getSequence",
        &serde_json::json!([account_hex]),
        false,
    );
    assert_eq!(rpc["result"], json(&direct)["result"]);
    let value = &rpc["result"];
    let canonical = hex_decode(
        value["canonical_value"]
            .as_str()
            .required("canonical account"),
    )
    .required("account hex");
    let material = hex_decode(value["proof_material"].as_str().required("account proof"))
        .required("proof hex");
    let proven = verify_account_evidence(
        &canonical,
        &material,
        account_id,
        None,
        AccountEvidencePolicy {
            expected_protocol_version: PROTOCOL_VERSION,
            expected_network_id: NETWORK_ID,
            handshake_sequencer_key: cluster.sequencer_key,
            root_selector: RootSelector::Latest,
        },
    )
    .required("independent account verification");
    assert_eq!(
        value["next_sequence"],
        proven.account().next_sequence.to_string()
    );
    assert_eq!(value["verification"], "state_proven");
    let malformed = local_rpc(
        &http,
        "",
        "lx_getSequence",
        &serde_json::json!(["ab"]),
        false,
    );
    assert_eq!(malformed["error"]["code"], -32602);
}

#[test]
fn local_gateway_rpc() {
    let cluster = start_cluster(true);
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    establish_receipt_head(&boundary, &cluster);
    let identity = start_local_identity(&cluster, &certificates);
    let authority = start_local_authority(&cluster, &certificates);
    let redis = start_local_redis(&cluster, &certificates);
    let gateway = start_local_gateway(
        &cluster,
        &certificates,
        &boundary,
        &identity,
        &authority,
        &redis,
    );
    let key = issue_local_scoped_key(
        &certificates,
        &gateway,
        &identity,
        &["activity:write", "program:call"],
    );
    let http = Http {
        port: gateway.port,
        ca: Certificate::from_der(&certificates.ca_der).required("CA"),
        identity: None,
    };
    let authorization = format!(
        "LayerX-Key {}:{}",
        key["key"]["id"].as_str().required("key id"),
        key["key"]["secret"].as_str().required("key secret")
    );
    let call = |method: &str, params: serde_json::Value, authenticated: bool| {
        local_rpc(&http, &authorization, method, &params, authenticated)
    };
    assert_eq!(
        call("lx_getNodeInfo", serde_json::json!([]), false)["result"]["network_id"],
        NETWORK_ID
    );
    assert_unavailable_reads(&call, &cluster.asset);
    let manifest = local_manifest(&cluster, 0);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest).required("manifest")).required("manifest JSON");
    let signed = fs::read(manifest[0]["signed_file"].as_str().required("signed path"))
        .required("signed bytes");
    let params = serde_json::json!([hex_encode(&signed), "executed"]);
    assert_eq!(
        call("lx_sendActivity", params.clone(), false)["error"]["code"],
        -32002
    );
    let first = call("lx_sendActivity", params.clone(), true);
    assert_eq!(first["result"]["commitment"], "executed", "{first}");
    assert!(first["result"]["receipt"]
        .as_str()
        .is_some_and(|r| !r.is_empty()));
    assert_eq!(
        call("lx_sendActivity", params, true)["result"],
        first["result"]
    );
    let batched = call(
        "lx_sendActivity",
        serde_json::json!([hex_encode(&signed), "batched"]),
        true,
    );
    assert_eq!(batched["result"]["commitment"], "batched", "{batched}");
    assert_eq!(
        batched["result"]["batch_evidence"]["canonical_value"],
        first["result"]["receipt"]
    );
    let finalised = call(
        "lx_sendActivity",
        serde_json::json!([hex_encode(&signed), "finalised"]),
        true,
    );
    assert_eq!(finalised["error"]["code"], -32001, "{finalised}");
    assert_eq!(
        finalised["error"]["data"]["requested_commitment"], "finalised",
        "{finalised}"
    );
    assert_eq!(
        finalised["error"]["data"]["state"], "pending",
        "{finalised}"
    );
    assert!(finalised.get("result").is_none(), "{finalised}");
    assert_eq!(
        call(
            "lx_sendActivity",
            serde_json::json!([hex_encode(&signed), "ack"]),
            true
        )["error"]["code"],
        -32602
    );
}

fn local_rpc(
    http: &Http,
    authorization: &str,
    method: &str,
    params: &serde_json::Value,
    authenticated: bool,
) -> serde_json::Value {
    let mut headers = vec![("Content-Type", "application/json")];
    if authenticated {
        headers.push(("Authorization", authorization));
    }
    let answer = http.request(
        "POST",
        "/rpc",
        &headers,
        &serde_json::to_vec(
            &serde_json::json!({"jsonrpc":"2.0","id":7,"method":method,"params":params}),
        )
        .required("RPC"),
    );
    assert_eq!(answer.status, 200, "{}", answer.body);
    let result = json(&answer);
    assert_eq!(result["id"], 7);
    result
}

fn assert_unavailable_reads(
    call: &impl Fn(&str, serde_json::Value, bool) -> serde_json::Value,
    native_asset: &[u8; 32],
) {
    let listed = call("lx_listAssets", serde_json::json!([]), false);
    let assets = listed["result"]["assets"]
        .as_array()
        .required("committed genesis assets");
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0]["asset_id"], hex_encode(native_asset));
    assert_eq!(assets[0]["symbol"], "TST");
    assert_eq!(
        call("lx_estimateFee", serde_json::json!(["abcd"]), false)["error"]["code"],
        -32602
    );
    for (method, params) in [
        ("lx_getAsset", serde_json::json!(["ab".repeat(32)])),
        ("lx_getBalances", serde_json::json!(["did:layerx:alice"])),
    ] {
        assert_eq!(call(method, params, false)["error"]["code"], -32001);
    }
    assert_eq!(
        call("lx_subscribe", serde_json::json!(["receipts"]), false)["error"]["code"],
        -32004
    );
}

fn rpc_account_sequence(http: &Http, source: &str, index: u64) -> u64 {
    let sequence_request = serde_json::to_vec(&serde_json::json!({
        "jsonrpc":"2.0", "id":index, "method":"lx_getSequence", "params":[source]
    }))
    .required("account sequence request");
    let sequence_response = http.request(
        "POST",
        "/rpc",
        &[("Content-Type", "application/json")],
        &sequence_request,
    );
    assert_eq!(sequence_response.status, 200, "{}", sequence_response.body);
    json(&sequence_response)["result"]["next_sequence"]
        .as_str()
        .required("account sequence")
        .parse::<u64>()
        .required("sequence integer")
}

fn gateway_rpc(
    http: &Http,
    authorization: &str,
    id: u64,
    method: &str,
    params: &serde_json::Value,
) -> serde_json::Value {
    let request = serde_json::to_vec(
        &serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )
    .required("gateway RPC request");
    let answer = http.request(
        "POST",
        "/rpc",
        &[
            ("Content-Type", "application/json"),
            ("Authorization", authorization),
        ],
        &request,
    );
    assert_eq!(answer.status, 200, "{}", answer.body);
    let value = json(&answer);
    assert_eq!(value["id"], id);
    value
}

fn rpc_account_balance(http: &Http, authorization: &str, account: &[u8; 32], id: u64) -> u128 {
    gateway_rpc(
        http,
        authorization,
        id,
        "lx_getAccount",
        &serde_json::json!([hex_encode(account)]),
    )["result"]["balance"]
        .as_str()
        .required("account balance")
        .parse()
        .required("account balance integer")
}

fn wait_payment_read(
    http: &Http,
    authorization: &str,
    id: u64,
    method: &str,
    params: &serde_json::Value,
    deadline: Instant,
) -> (serde_json::Value, u64) {
    let mut attempts = 0_u64;
    loop {
        assert!(Instant::now() < deadline, "payment read deadline: {method}");
        let recovered = gateway_rpc(http, authorization, id, method, params);
        if recovered.get("error").is_none() {
            return (recovered, attempts);
        }
        assert_eq!(recovered["error"]["code"], -32001, "{recovered}");
        attempts = attempts
            .checked_add(1)
            .required("payment read attempt bound");
        thread::sleep(Duration::from_millis(25));
    }
}

fn recovered_payment_result_code(
    result: &serde_json::Value,
    cluster: &Cluster,
    signed: &funding::SignedPayment,
) -> i32 {
    assert_eq!(
        result["result"]["activity_id"],
        hex_encode(&signed.activity_id)
    );
    let receipt_bytes = layerx_platform_core::hex_decode(
        result["result"]["receipt"]
            .as_str()
            .required("recovered canonical payment receipt"),
    )
    .required("recovered payment receipt hex");
    let verified =
        layerx_proof::receipt::verify_sequencer_signature(&receipt_bytes, cluster.sequencer_key)
            .required("recovered payment receipt signature");
    let protocol = verified
        .protocol()
        .required("recovered protocol payment receipt");
    assert_eq!(protocol.activity_id(), signed.activity_id);
    protocol.result_code()
}

fn send_payment(
    cluster: &Cluster,
    http: &Http,
    authorization: &str,
    id: u64,
    signed: &funding::SignedPayment,
    commitment: &str,
) -> (serde_json::Value, u128) {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(30);
    let expected_activity = hex_encode(&signed.activity_id);
    let submitted = gateway_rpc(
        http,
        authorization,
        id,
        "lx_sendActivity",
        &serde_json::json!([hex_encode(&signed.canonical), commitment]),
    );
    if submitted.get("error").is_none() {
        return (submitted, started.elapsed().as_micros());
    }
    assert_eq!(submitted["error"]["code"], -32001, "{submitted}");
    assert_eq!(
        submitted["error"]["data"]["state"], "pending",
        "{submitted}"
    );
    assert_eq!(
        submitted["error"]["data"]["requested_commitment"], commitment,
        "{submitted}"
    );
    let pending_activity = submitted["error"]["data"]["upstream"]["result"]["activity_id"]
        .as_str()
        .or_else(|| submitted["error"]["data"]["evidence"]["activity_id"].as_str())
        .required("pending activity id");
    assert_eq!(pending_activity, expected_activity, "{submitted}");

    let (receipt, receipt_attempts) = wait_payment_read(
        http,
        authorization,
        id,
        "lx_getReceipt",
        &serde_json::json!([expected_activity]),
        deadline,
    );
    let result_code = recovered_payment_result_code(&receipt, cluster, signed);

    let mut completed = receipt;
    completed["result"]["state"] = serde_json::json!(if result_code == 0 {
        "completed"
    } else {
        "refused"
    });
    completed["result"]["result_code"] = serde_json::json!(result_code);
    if commitment == "batched" {
        let (proof, proof_attempts) = wait_payment_read(
            http,
            authorization,
            id,
            "lx_getProof",
            &serde_json::json!(["receipt", expected_activity]),
            deadline,
        );
        assert_eq!(proof["result"]["activity_id"], expected_activity);
        assert_eq!(
            proof["result"]["canonical_value"],
            completed["result"]["receipt"]
        );
        completed["result"]["batch_evidence"] = proof["result"].clone();
        completed["result"]["commitment"] = serde_json::json!("batched");
        println!(
            "payment_commitment_recovered activity_id={expected_activity} commitment=batched receipt_attempts={receipt_attempts} proof_attempts={proof_attempts}"
        );
    } else {
        assert_eq!(commitment, "executed");
        completed["result"]["commitment"] = serde_json::json!("executed");
        println!(
            "payment_commitment_recovered activity_id={expected_activity} commitment=executed receipt_attempts={receipt_attempts}"
        );
    }
    (completed, started.elapsed().as_micros())
}

struct PaymentReceiptEvidence {
    bytes: Vec<u8>,
    timestamp: u64,
    settlement_reference: String,
}

fn verify_payment_receipt(
    result: &serde_json::Value,
    cluster: &Cluster,
    signed: &funding::SignedPayment,
    commitment: &str,
    expected_result: i32,
    draw: Option<(&layerx_crypto::payments::Grant, u128)>,
) -> PaymentReceiptEvidence {
    assert_eq!(result["result"]["commitment"], commitment, "{result}");
    assert_eq!(
        result["result"]["activity_id"],
        hex_encode(&signed.activity_id)
    );
    assert_eq!(result["result"]["result_code"], expected_result, "{result}");
    let bytes = layerx_platform_core::hex_decode(
        result["result"]["receipt"]
            .as_str()
            .required("canonical payment receipt"),
    )
    .required("payment receipt hex");
    let receipt = layerx_proof::receipt::verify_sequencer_signature(&bytes, cluster.sequencer_key)
        .required("payment receipt signature");
    let protocol = receipt.protocol().required("protocol payment receipt");
    assert_eq!(protocol.activity_id(), signed.activity_id);
    assert_eq!(protocol.result_code(), expected_result);
    assert_eq!(protocol.protocol_version(), PROTOCOL_VERSION);
    assert!(protocol.fee_charged() > 0);
    if let Some((grant, amount)) = draw {
        assert_eq!(protocol.module_id(), ModuleId::Asset as u16);
        assert_eq!(protocol.operation(), 6);
        assert_eq!(protocol.asset(), grant.asset);
        assert_eq!(protocol.amount(), amount);
        assert_eq!(protocol.from(), grant.from);
        assert_eq!(protocol.to(), grant.recipient);
        assert_eq!(
            protocol.debit_balance_before().checked_sub(amount),
            Some(protocol.debit_balance_after())
        );
        assert_eq!(
            protocol.credit_balance_before().checked_add(amount),
            Some(protocol.credit_balance_after())
        );
        let mut purpose = Vec::with_capacity(64);
        purpose.extend_from_slice(&grant.purpose_hash);
        if grant.has_reference {
            purpose.extend_from_slice(&grant.reference_hash);
        }
        assert_eq!(
            protocol.context_hash(),
            layerx_platform_core::domain_hash(layerx_wire::hash::Domain::ContextHash, &purpose)
        );
    }
    let settlement_reference = format!(
        "lxp:{}",
        hex_encode(&layerx_wire::hash::merkle_leaf(&bytes).required("settlement receipt digest"))
    );
    assert_eq!(settlement_reference.len(), 68);
    PaymentReceiptEvidence {
        bytes,
        timestamp: protocol.timestamp(),
        settlement_reference,
    }
}

fn verify_batch_inclusion(result: &serde_json::Value, cluster: &Cluster, receipt: &[u8]) {
    let evidence = &result["result"]["batch_evidence"];
    assert_eq!(evidence["kind"], "receipt");
    assert_eq!(evidence["activity_id"], result["result"]["activity_id"]);
    assert_eq!(evidence["canonical_value"], result["result"]["receipt"]);
    let proof = &evidence["proof"];
    let siblings = proof["siblings"]
        .as_array()
        .required("receipt proof siblings")
        .iter()
        .map(|value| {
            layerx_platform_core::hex_decode(value.as_str().required("receipt proof sibling"))
                .required("receipt proof sibling hex")
                .try_into()
                .required("receipt proof sibling size")
        })
        .collect::<Vec<[u8; 32]>>();
    let proof = layerx_proof::merkle::Proof::new(
        u32::try_from(proof["leaf_index"].as_u64().required("receipt proof index"))
            .required("receipt proof index bound"),
        u32::try_from(proof["leaf_count"].as_u64().required("receipt proof count"))
            .required("receipt proof count bound"),
        siblings,
    )
    .required("receipt proof shape");
    let header = layerx_platform_core::hex_decode(
        evidence["signed_header"]["canonical_header"]
            .as_str()
            .required("signed batch header"),
    )
    .required("signed batch header hex");
    let signature: [u8; 64] = layerx_platform_core::hex_decode(
        evidence["signed_header"]["signature"]
            .as_str()
            .required("signed batch signature"),
    )
    .required("signed batch signature hex")
    .try_into()
    .required("signed batch signature size");
    let authorization = layerx_proof::inclusion::SequencerAuthorization::new(
        cluster.sequencer_id,
        cluster.sequencer_key,
        1,
        u64::MAX,
    );
    let included = layerx_proof::inclusion::verify_receipt(
        receipt,
        &proof,
        &header,
        &signature,
        &authorization,
    )
    .required("signed batch receipt inclusion");
    assert_eq!(included.header().header().network_id(), NETWORK_ID);
    assert_eq!(
        included.header().header().protocol_version(),
        PROTOCOL_VERSION
    );
}

fn signed_draw(
    cluster: &Cluster,
    funding: &funding::Funding,
    http: &Http,
    grant: &layerx_crypto::payments::Grant,
    amount: u128,
    id: u64,
) -> funding::SignedPayment {
    let recipient = hex_encode(&grant.recipient);
    let identity_sequence = account_sequence(&cluster.lni_socket, &funding.recipient_did);
    let receiver_sequence = rpc_account_sequence(http, &recipient, id);
    assert_eq!(
        identity_sequence.checked_sub(receiver_sequence),
        Some(1),
        "the custody credit advances identity sequencing without consuming the payment account sequence"
    );
    let idempotency_key = random32();
    let receive = funding::receive(
        &funding.recipient_seed,
        grant,
        receiver_sequence,
        idempotency_key,
        amount,
    )
    .required("canonical receive");
    funding::payment(
        &funding.recipient_seed,
        &funding.recipient_did,
        identity_sequence,
        idempotency_key,
        &receive,
    )
    .required("signed receive")
}

fn wait_for_next_grant_window(timestamp: u64, window: u64) {
    let boundary = timestamp
        .checked_sub(timestamp % window)
        .and_then(|start| start.checked_add(window))
        .required("grant window boundary");
    let deadline = Instant::now() + Duration::from_secs(5);
    while now_ms() <= boundary {
        assert!(
            Instant::now() < deadline,
            "grant renewal window did not advance"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn run_metered_draws(
    cluster: &Cluster,
    funding: &funding::Funding,
    http: &Http,
    payer_authorization: &str,
    recipient_authorization: &str,
) {
    const SAMPLES: u64 = 20;
    let payer_sequence = account_sequence(&cluster.lni_socket, &cluster.treasury_did);
    let metered_grant = funding::payer_grant(&funding::GrantRequest {
        payer_seed: &cluster.treasury_seed,
        payer_did: &cluster.treasury_did,
        recipient_did: &funding.recipient_did,
        asset: cluster.asset,
        per_draw_maximum: 1,
        allowance: u128::from(SAMPLES),
        recurring_window: None,
        expiration: now_ms().saturating_add(300_000),
        purpose_hash: random32(),
        revocation_sequence: payer_sequence,
    })
    .required("metered grant");
    let grant_key = random32();
    let signed_grant = funding::payment(
        &cluster.treasury_seed,
        &cluster.treasury_did,
        payer_sequence,
        grant_key,
        &layerx_crypto::payments::Payment::IssueGrant(metered_grant.clone()),
    )
    .required("signed metered grant");
    let (grant_result, _) = send_payment(
        cluster,
        http,
        payer_authorization,
        100,
        &signed_grant,
        "executed",
    );
    verify_payment_receipt(&grant_result, cluster, &signed_grant, "executed", 0, None);

    let mut samples = Vec::new();
    for index in 0..SAMPLES {
        let signed = signed_draw(cluster, funding, http, &metered_grant, 1, 200 + index);
        let payer_before = rpc_account_balance(
            http,
            payer_authorization,
            &metered_grant.from,
            300 + index * 3,
        );
        let (result, elapsed) = send_payment(
            cluster,
            http,
            recipient_authorization,
            301 + index * 3,
            &signed,
            "executed",
        );
        let evidence = verify_payment_receipt(
            &result,
            cluster,
            &signed,
            "executed",
            0,
            Some((&metered_grant, 1)),
        );
        let payer_after = rpc_account_balance(
            http,
            payer_authorization,
            &metered_grant.from,
            302 + index * 3,
        );
        assert_eq!(payer_before.checked_sub(1), Some(payer_after));
        if index == 0 {
            let (replay, _) = send_payment(
                cluster,
                http,
                recipient_authorization,
                400,
                &signed,
                "executed",
            );
            for field in ["activity_id", "receipt", "result_code", "commitment"] {
                assert_eq!(replay["result"][field], result["result"][field], "{field}");
            }
            assert_eq!(
                rpc_account_balance(http, payer_authorization, &metered_grant.from, 401),
                payer_after
            );
        }
        println!(
            "metered_draw_sample index={index} elapsed_us={elapsed} settlement_reference={}",
            evidence.settlement_reference
        );
        samples.push(elapsed);
    }
    samples.sort_unstable();
    println!(
        "metered_draw_submit_to_receipt_us samples={SAMPLES} p50={} p99={} transport=gateway_https_json_rpc commitment=executed funding=verified_custody receipt_wait=commit_condition",
        samples[9], samples[19]
    );
}

fn verify_recovered_payment_reads(
    cluster: &Cluster,
    http: &Http,
    authorization: &str,
    signed: &funding::SignedPayment,
    expected: &serde_json::Value,
) {
    for (id, method) in [(506, "lx_getActivityStatus"), (507, "lx_getReceipt")] {
        let recovered = gateway_rpc(
            http,
            authorization,
            id,
            method,
            &serde_json::json!([hex_encode(&signed.activity_id)]),
        );
        assert_eq!(
            recovered["result"]["receipt"],
            expected["result"]["receipt"]
        );
        assert_eq!(
            recovered_payment_result_code(&recovered, cluster, signed),
            0
        );
    }
}

fn assert_payment_ack_refused(
    http: &Http,
    authorization: &str,
    id: u64,
    signed: &funding::SignedPayment,
) {
    assert_eq!(
        gateway_rpc(
            http,
            authorization,
            id,
            "lx_sendActivity",
            &serde_json::json!([hex_encode(&signed.canonical), "ack"]),
        )["error"]["code"],
        -32602
    );
}

fn run_subscription_renewal(
    cluster: &Cluster,
    funding: &funding::Funding,
    http: &Http,
    payer_authorization: &str,
    recipient_authorization: &str,
) {
    const SUBSCRIPTION_WINDOW_MS: u64 = 1_000;
    let subscription_issue_sequence = account_sequence(&cluster.lni_socket, &cluster.treasury_did);
    let subscription_grant = funding::payer_grant(&funding::GrantRequest {
        payer_seed: &cluster.treasury_seed,
        payer_did: &cluster.treasury_did,
        recipient_did: &funding.recipient_did,
        asset: cluster.asset,
        per_draw_maximum: 1,
        allowance: 1,
        recurring_window: Some(SUBSCRIPTION_WINDOW_MS),
        expiration: now_ms().saturating_add(300_000),
        purpose_hash: random32(),
        revocation_sequence: subscription_issue_sequence,
    })
    .required("subscription grant");
    let subscription_key = random32();
    let signed_subscription = funding::payment(
        &cluster.treasury_seed,
        &cluster.treasury_did,
        subscription_issue_sequence,
        subscription_key,
        &layerx_crypto::payments::Payment::IssueGrant(subscription_grant.clone()),
    )
    .required("signed subscription grant");
    let (subscription_result, _) = send_payment(
        cluster,
        http,
        payer_authorization,
        500,
        &signed_subscription,
        "executed",
    );
    verify_payment_receipt(
        &subscription_result,
        cluster,
        &signed_subscription,
        "executed",
        0,
        None,
    );
    let first_draw = signed_draw(cluster, funding, http, &subscription_grant, 1, 501);
    let (first_result, _) = send_payment(
        cluster,
        http,
        recipient_authorization,
        502,
        &first_draw,
        "executed",
    );
    let first_evidence = verify_payment_receipt(
        &first_result,
        cluster,
        &first_draw,
        "executed",
        0,
        Some((&subscription_grant, 1)),
    );
    wait_for_next_grant_window(first_evidence.timestamp, SUBSCRIPTION_WINDOW_MS);
    let renewal = signed_draw(cluster, funding, http, &subscription_grant, 1, 503);
    assert_payment_ack_refused(http, recipient_authorization, 504, &renewal);
    let (renewal_result, renewal_elapsed) = send_payment(
        cluster,
        http,
        recipient_authorization,
        505,
        &renewal,
        "batched",
    );
    let renewal_evidence = verify_payment_receipt(
        &renewal_result,
        cluster,
        &renewal,
        "batched",
        0,
        Some((&subscription_grant, 1)),
    );
    assert!(
        renewal_evidence.timestamp / SUBSCRIPTION_WINDOW_MS
            > first_evidence.timestamp / SUBSCRIPTION_WINDOW_MS
    );
    verify_batch_inclusion(&renewal_result, cluster, &renewal_evidence.bytes);
    verify_recovered_payment_reads(
        cluster,
        http,
        recipient_authorization,
        &renewal,
        &renewal_result,
    );
    println!(
        "subscription_renewal elapsed_us={renewal_elapsed} first_window={} renewal_window={} commitment=batched settlement_reference={}",
        first_evidence.timestamp / SUBSCRIPTION_WINDOW_MS,
        renewal_evidence.timestamp / SUBSCRIPTION_WINDOW_MS,
        renewal_evidence.settlement_reference
    );
}

#[test]
fn local_gateway_grant_draw_and_subscription_renewal_latency() {
    let (cluster, funding) = funding::start();
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let identity = start_local_identity(&cluster, &certificates);
    let authority = start_local_authority(&cluster, &certificates);
    let redis = start_local_redis(&cluster, &certificates);
    let gateway = start_local_gateway(
        &cluster,
        &certificates,
        &boundary,
        &identity,
        &authority,
        &redis,
    );
    let payer_key = issue_local_scoped_key(&certificates, &gateway, &identity, &["activity:write"]);
    let recipient_key = issue_recipient_scoped_key(
        &certificates,
        &gateway,
        &identity,
        &funding,
        &["activity:write"],
    );
    let http = Http {
        port: gateway.port,
        ca: Certificate::from_der(&certificates.ca_der).required("CA"),
        identity: None,
    };
    let payer_authorization = format!(
        "LayerX-Key {}:{}",
        payer_key["key"]["id"].as_str().required("payer key id"),
        payer_key["key"]["secret"]
            .as_str()
            .required("payer key secret")
    );
    let recipient_authorization = format!(
        "LayerX-Key {}:{}",
        recipient_key["key"]["id"]
            .as_str()
            .required("recipient key id"),
        recipient_key["key"]["secret"]
            .as_str()
            .required("recipient key secret")
    );
    run_metered_draws(
        &cluster,
        &funding,
        &http,
        &payer_authorization,
        &recipient_authorization,
    );
    run_subscription_renewal(
        &cluster,
        &funding,
        &http,
        &payer_authorization,
        &recipient_authorization,
    );
    print_payment_timings(&cluster);
}

struct PersistentGatewayHttp {
    stream: native_tls::TlsStream<TcpStream>,
}

impl PersistentGatewayHttp {
    fn connect(http: &Http) -> Self {
        let tcp = must(TcpStream::connect(("127.0.0.1", http.port)), "connect");
        must(
            tcp.set_read_timeout(Some(Duration::from_secs(60))),
            "read timeout",
        );
        Self {
            stream: must(http.connector().connect("localhost", tcp), "TLS connect"),
        }
    }

    fn rpc(&mut self, authorization: &str, body: &[u8]) -> HttpAnswer {
        let request = format!(
            "POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: {authorization}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
            body.len()
        );
        must(self.stream.write_all(request.as_bytes()), "write request");
        must(self.stream.write_all(body), "write body");
        must(self.stream.flush(), "flush request");

        let mut raw = Vec::with_capacity(2048);
        let mut chunk = [0_u8; 2048];
        let header_end = loop {
            let count = must(self.stream.read(&mut chunk), "read response headers");
            assert!(count > 0, "gateway closed a reusable connection");
            raw.extend_from_slice(&chunk[..count]);
            assert!(raw.len() <= 8 * 1024 * 1024, "gateway response is bounded");
            if let Some(position) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = std::str::from_utf8(&raw[..header_end]).required("response headers");
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.split_once(':').and_then(|(name, value)| {
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().required("content length"))
                })
            })
            .required("content-length");
        let total = header_end + content_length;
        assert!(total <= 8 * 1024 * 1024, "gateway response is bounded");
        while raw.len() < total {
            let count = must(self.stream.read(&mut chunk), "read response body");
            assert!(count > 0, "gateway truncated a reusable response");
            raw.extend_from_slice(&chunk[..count]);
        }
        assert_eq!(
            raw.len(),
            total,
            "gateway response did not overrun its frame"
        );
        parse_http_with_connection(&raw, "keep-alive")
    }
}

#[test]
fn local_gateway_successful_send_latency() {
    let (cluster, funding) = funding::start();
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let identity = start_local_identity(&cluster, &certificates);
    let authority = start_local_authority(&cluster, &certificates);
    let redis = start_local_redis(&cluster, &certificates);
    let gateway = start_local_gateway(
        &cluster,
        &certificates,
        &boundary,
        &identity,
        &authority,
        &redis,
    );
    let key = issue_local_scoped_key(&certificates, &gateway, &identity, &["activity:write"]);
    let http = Http {
        port: gateway.port,
        ca: Certificate::from_der(&certificates.ca_der).required("CA"),
        identity: None,
    };
    let authorization = format!(
        "LayerX-Key {}:{}",
        key["key"]["id"].as_str().required("key id"),
        key["key"]["secret"].as_str().required("secret")
    );
    let source =
        hex_encode(&layerx_platform_core::main_account(&cluster.treasury_did).required("source"));
    let balance = boundary.core.get(&format!("/v1/accounts/{source}/balance"));
    assert_eq!(balance.status, 200, "{}", balance.body);
    assert_eq!(json(&balance)["result"]["balance"], "100000000000000");
    println!(
        "funded_send_sequences identity_next={} account_next={} balance={}",
        account_sequence(&cluster.lni_socket, &cluster.treasury_did),
        json(&balance)["result"]["next_sequence"],
        json(&balance)["result"]["balance"]
    );
    let mut send_http = PersistentGatewayHttp::connect(&http);
    let mut samples = Vec::new();
    let mut last_signed = None;
    for index in 0..20 {
        let account_next = rpc_account_sequence(&http, &source, index);
        let identity_next = account_sequence(&cluster.lni_socket, &cluster.treasury_did);
        let signed = funding::send(
            &cluster.treasury_seed,
            identity_next,
            &SendRequest {
                network_id: NETWORK_ID,
                source_did: cluster.treasury_did.clone(),
                destination_did: funding.recipient_did.clone(),
                asset: cluster.asset,
                amount: 1,
                account_sequence: account_next,
                idempotency_key: random32(),
                not_before_ms: now_ms() - 1000,
                expires_at_ms: now_ms() + 60000,
                fee_limit: 1_000_000_000_000,
            },
        )
        .required("real funded SEND");
        let request = serde_json::to_vec(
            &serde_json::json!({"jsonrpc":"2.0","id":index,"method":"lx_sendActivity",
            "params":[hex_encode(&signed.canonical),"executed"]}),
        )
        .required("RPC SEND");
        let started = Instant::now();
        let answer = send_http.rpc(&authorization, &request);
        let elapsed = started.elapsed().as_micros();
        assert_eq!(answer.status, 200, "{}", answer.body);
        let result = json(&answer);
        assert_eq!(result["result"]["commitment"], "executed", "{result}");
        assert_eq!(result["result"]["result_code"], 0, "{result}");
        assert_eq!(
            result["result"]["activity_id"],
            hex_encode(&signed.activity_id)
        );
        verify_funded_receipt(&result, &cluster, &signed);
        println!("successful_send_sample index={index} elapsed_us={elapsed}");
        samples.push(elapsed);
        last_signed = Some(signed);
    }
    samples.sort_unstable();
    println!(
        "successful_send_submit_to_receipt_us samples=20 p50={} p99={} transport=gateway_https_json_rpc commitment=executed funding=verified_custody destination=funded_agent_main receipt_wait=commit_condition",
        samples[9], samples[19]
    );
    print_payment_timings(&cluster);
    assert_wallet_receipt_verifier(
        &cluster,
        &certificates,
        &gateway,
        &key,
        &last_signed.as_ref().required("successful SEND").activity_id,
    );
    assert_funded_commitments(
        &http,
        &authorization,
        &last_signed.required("successful SEND"),
        &funding,
        &cluster,
        &boundary,
    );
}

fn print_payment_timings(cluster: &Cluster) {
    for file in [
        "sequencer.stderr",
        "boundary.stderr",
        "layerx-gateway.stderr",
    ] {
        let lines = must(fs::read_to_string(cluster.root.join(file)), "timing log");
        for line in lines
            .lines()
            .filter(|line| line.contains("pay_timing") || line.starts_with("pay-native "))
        {
            println!("{file} {line}");
        }
    }
}

fn verify_funded_receipt(
    result: &serde_json::Value,
    cluster: &Cluster,
    signed: &layerx_platform_core::SignedSend,
) {
    let receipt =
        layerx_platform_core::hex_decode(result["result"]["receipt"].as_str().required("receipt"))
            .required("receipt hex");
    let receipt =
        layerx_proof::receipt::verify_sequencer_signature(&receipt, cluster.sequencer_key)
            .required("receipt signature");
    let receipt = receipt.protocol().required("protocol SEND");
    assert_eq!(receipt.result_code(), 0);
    assert_eq!(receipt.activity_id(), signed.activity_id);
    println!(
        "successful_send_meter canonical_bytes={} fee_charged={}",
        signed.canonical.len(),
        receipt.fee_charged()
    );
}

fn assert_funded_commitments(
    http: &Http,
    authorization: &str,
    signed: &layerx_platform_core::SignedSend,
    funding: &funding::Funding,
    cluster: &Cluster,
    boundary: &Boundary,
) {
    let call = |id: u64, canonical: &[u8], commitment: &str| {
        let started = Instant::now();
        let answer = http.request(
            "POST",
            "/rpc",
            &[
                ("Content-Type", "application/json"),
                ("Authorization", authorization),
            ],
            &serde_json::to_vec(&serde_json::json!({
                "jsonrpc":"2.0", "id":id, "method":"lx_sendActivity",
                "params":[hex_encode(canonical), commitment]
            }))
            .required("commitment request"),
        );
        assert_eq!(answer.status, 200, "{}", answer.body);
        (json(&answer), started.elapsed())
    };
    let (batched, elapsed) = call(21, &signed.canonical, "batched");
    assert_eq!(batched["result"]["result_code"], 0, "{batched}");
    assert_eq!(
        batched["result"]["activity_id"],
        hex_encode(&signed.activity_id)
    );
    assert_eq!(batched["result"]["commitment"], "batched", "{batched}");
    assert_eq!(
        batched["result"]["batch_evidence"]["canonical_value"],
        batched["result"]["receipt"]
    );
    println!(
        "successful_send_commitment requested=batched returned={} state={} elapsed_us={}",
        batched["result"]["commitment"],
        batched["result"]["state"],
        elapsed.as_micros()
    );

    let checkpoint_id = funding.finalise_first_batch(cluster);
    let checkpoint_hex = hex_encode(&checkpoint_id);
    let evidence = boundary
        .core
        .get(&format!("/v1/checkpoints/{checkpoint_hex}"));
    assert_eq!(evidence.status, 200, "{}", evidence.body);
    let evidence = json(&evidence);
    assert_eq!(
        evidence["result"]["checkpoint_id"], checkpoint_hex,
        "{evidence}"
    );
    for field in ["checkpoint", "context", "canonical_header"] {
        assert!(
            evidence["result"][field]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "{field}: {evidence}"
        );
    }
    let (later, elapsed) = call(22, &signed.canonical, "finalised");
    assert_eq!(later["error"]["code"], -32001, "{later}");
    assert_eq!(later["error"]["data"]["state"], "pending", "{later}");
    assert_eq!(
        later["error"]["data"]["requested_commitment"], "finalised",
        "{later}"
    );
    assert_eq!(
        later["error"]["data"]["evidence"]["activity_id"],
        hex_encode(&signed.activity_id)
    );
    assert_eq!(later["error"]["data"]["evidence"]["result_code"], 0);
    assert!(elapsed < Duration::from_secs(10));
    println!(
        "successful_send_commitment requested=finalised returned=pending state={} elapsed_us={}",
        later["error"]["data"]["state"],
        elapsed.as_micros()
    );

    println!(
        "successful_finalised_checkpoint checkpoint_id={} evidence_bytes={}",
        checkpoint_hex,
        evidence["result"]["checkpoint"]
            .as_str()
            .required("checkpoint bytes")
            .len()
    );
}

fn ws_connect(
    certificates: &Certificates,
    port: u16,
    authorization: &str,
) -> native_tls::TlsStream<std::net::TcpStream> {
    let tcp = std::net::TcpStream::connect(("127.0.0.1", port)).required("WS TCP");
    tcp.set_read_timeout(Some(Duration::from_secs(15)))
        .required("WS timeout");
    let connector = native_tls::TlsConnector::builder()
        .add_root_certificate(Certificate::from_der(&certificates.ca_der).required("CA"))
        .build()
        .required("connector");
    let mut stream = connector.connect("localhost", tcp).required("WS TLS");
    write!(stream, "GET /rpc/ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nAuthorization: {authorization}\r\n\r\n").required("upgrade");
    stream.flush().required("flush");
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).required("upgrade response");
        response.push(byte[0]);
        assert!(response.len() < 4096);
    }
    let response = String::from_utf8(response).required("HTTP UTF8");
    assert!(response.starts_with("HTTP/1.1 101 "), "upgrade refused");
    assert!(response.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
    stream
}

fn ws_send(stream: &mut impl Write, body: &serde_json::Value) {
    let body = serde_json::to_vec(body).required("WS JSON");
    let mut frame = vec![0x81];
    if body.len() < 126 {
        frame.push(128 | u8::try_from(body.len()).required("length"));
    } else {
        frame.push(254);
        frame.extend_from_slice(&u16::try_from(body.len()).required("length").to_be_bytes());
    }
    let mask = [1, 2, 3, 4];
    frame.extend_from_slice(&mask);
    frame.extend(body.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    stream.write_all(&frame).required("WS frame");
    stream.flush().required("flush");
}

fn ws_receive(stream: &mut (impl Read + Write)) -> serde_json::Value {
    loop {
        let mut header = [0; 2];
        stream.read_exact(&mut header).required("WS header");
        assert_eq!(header[1] & 128, 0);
        if header[0] == 0x89 {
            assert!(header[1] <= 125, "control frame length");
            let mut body = vec![0; usize::from(header[1])];
            stream.read_exact(&mut body).required("ping payload");
            let mask = [1, 2, 3, 4];
            let mut pong = vec![0x8a, 128 | header[1]];
            pong.extend_from_slice(&mask);
            pong.extend(
                body.iter()
                    .enumerate()
                    .map(|(index, byte)| byte ^ mask[index % 4]),
            );
            stream.write_all(&pong).required("pong frame");
            stream.flush().required("pong flush");
            continue;
        }
        assert_eq!(header[0], 0x81, "expected text frame");
        let length = match header[1] {
            126 => {
                let mut n = [0; 2];
                stream.read_exact(&mut n).required("length");
                usize::from(u16::from_be_bytes(n))
            }
            127 => {
                let mut n = [0; 8];
                stream.read_exact(&mut n).required("length");
                usize::try_from(u64::from_be_bytes(n)).required("length")
            }
            n => usize::from(n),
        };
        assert!(length <= 1024 * 1024);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).required("WS body");
        return serde_json::from_slice(&body).required("WS JSON");
    }
}

impl Http {
    fn upgrade_request(&self, target: &str, headers: &[(&str, &str)]) -> HttpAnswer {
        let mut request = format!(
            "GET {target} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: Upgrade\r\n"
        );
        for (name, value) in headers {
            must(write!(request, "{name}: {value}\r\n"), "format header");
        }
        request.push_str("\r\n");
        must(self.raw(&request, &[]), "tls request")
    }
}

#[test]
fn local_gateway_websocket_receipt_wake() {
    let cluster = start_cluster(true);
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let identity = start_local_identity(&cluster, &certificates);
    let authority = start_local_authority(&cluster, &certificates);
    let redis = start_local_redis(&cluster, &certificates);
    let gateway = start_local_gateway(
        &cluster,
        &certificates,
        &boundary,
        &identity,
        &authority,
        &redis,
    );
    let key = issue_local_scoped_key(
        &certificates,
        &gateway,
        &identity,
        &["receipt:read", "state:read"],
    );
    let authorization = format!(
        "LayerX-Key {}:{}",
        key["key"]["id"].as_str().required("key id"),
        key["key"]["secret"].as_str().required("key secret")
    );
    let http = Http {
        port: gateway.port,
        ca: Certificate::from_der(&certificates.ca_der).required("CA"),
        identity: None,
    };
    let upgrade = [
        ("Upgrade", "websocket"),
        ("Sec-WebSocket-Version", "13"),
        ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
    ];
    assert_eq!(http.upgrade_request("/rpc/ws", &upgrade).status, 401);
    let mut stream = ws_connect(&certificates, gateway.port, &authorization);
    for (id, params) in [
        (1, serde_json::json!(["receipts"])),
        (2, serde_json::json!(["checkpoints"])),
    ] {
        ws_send(
            &mut stream,
            &serde_json::json!({"jsonrpc":"2.0","id":id,"method":"lx_subscribe","params":params}),
        );
        assert_eq!(ws_receive(&mut stream)["result"], id.to_string());
    }
    let account = hex_encode(
        &layerx_wire::hash::account_id_for_protocol(
            &layerx_types::account::AccountId::parse("system:fees").required("account"),
            PROTOCOL_VERSION,
        )
        .required("account id"),
    );
    ws_send(
        &mut stream,
        &serde_json::json!({"jsonrpc":"2.0","id":3,"method":"lx_subscribe","params":["account",account]}),
    );
    assert_eq!(ws_receive(&mut stream)["result"], "3");
    establish_receipt_head(&boundary, &cluster);
    let event = ws_receive(&mut stream);
    assert_eq!(event["method"], "lx_subscription");
    assert_eq!(event["params"]["subscription"], "1");
    let activity = event["params"]["result"]["activity_id"]
        .as_str()
        .required("activity");
    let read = boundary.core.get(&format!("/v1/receipts/{activity}"));
    assert_eq!(read.status, 200);
    assert_eq!(
        event["params"]["result"]["receipt"],
        json(&read)["result"]["receipt"]
    );
    let account_event = ws_receive(&mut stream);
    assert_eq!(account_event["params"]["subscription"], "3");
    assert_eq!(account_event["params"]["result"]["account_id"], account);
    ws_send(
        &mut stream,
        &serde_json::json!({"jsonrpc":"2.0","id":3,"method":"lx_subscribe","params":["account", "00".repeat(32)]}),
    );
    assert_eq!(ws_receive(&mut stream)["error"]["code"], -32602);
}

#[test]
fn local_gateway_committed_payment_reads() {
    let (cluster, funding) = funding::start();
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let identity = start_local_identity(&cluster, &certificates);
    let authority = start_local_authority(&cluster, &certificates);
    let redis = start_local_redis(&cluster, &certificates);
    let gateway = start_local_gateway(
        &cluster,
        &certificates,
        &boundary,
        &identity,
        &authority,
        &redis,
    );
    let http = Http {
        port: gateway.port,
        ca: Certificate::from_der(&certificates.ca_der).required("CA"),
        identity: None,
    };
    let read = |method: &str, params: serde_json::Value| {
        let request = serde_json::to_vec(
            &serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
        )
        .required("read request");
        let answer = http.request(
            "POST",
            "/rpc",
            &[("Content-Type", "application/json")],
            &request,
        );
        assert_eq!(answer.status, 200, "{}", answer.body);
        json(&answer)
    };
    let balances = read("lx_getBalances", serde_json::json!([cluster.treasury_did]));
    let accounts = balances["result"]["accounts"]
        .as_array()
        .required("committed accounts");
    let source = layerx_platform_core::main_account(&cluster.treasury_did).required("source");
    let account = accounts
        .iter()
        .find(|account| account["account_id"] == hex_encode(&source))
        .required("funded main account");
    assert_eq!(account["balance"], "100000000000000");
    assert_eq!(account["asset_id"], hex_encode(&cluster.asset));
    assert!(!account["proof_material"]
        .as_str()
        .required("native proof")
        .is_empty());
    let assets = read("lx_listAssets", serde_json::json!([]));
    let assets = assets["result"]["assets"].as_array().required("assets");
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0]["asset_id"], hex_encode(&cluster.asset));
    assert_eq!(assets[0]["symbol"], "TST");
    let detail = read(
        "lx_getAsset",
        serde_json::json!([hex_encode(&cluster.asset)]),
    );
    assert_eq!(detail["result"]["asset"], assets[0]);
    assert_eq!(
        detail["result"]["verification"],
        "authenticated_committed_snapshot"
    );
    assert_committed_fee_reads(&cluster, &funding, &http, source, &read);
}

fn assert_committed_fee_reads(
    cluster: &Cluster,
    funding: &funding::Funding,
    http: &Http,
    source: [u8; 32],
    read: &impl Fn(&str, serde_json::Value) -> serde_json::Value,
) {
    let signed = funding::send(
        &cluster.treasury_seed,
        account_sequence(&cluster.lni_socket, &cluster.treasury_did),
        &SendRequest {
            network_id: NETWORK_ID,
            source_did: cluster.treasury_did.clone(),
            destination_did: funding.recipient_did.clone(),
            asset: cluster.asset,
            amount: 1,
            account_sequence: rpc_account_sequence(http, &hex_encode(&source), 2),
            idempotency_key: random32(),
            not_before_ms: now_ms() - 1000,
            expires_at_ms: now_ms() + 60000,
            fee_limit: 1_000_000_000_000,
        },
    )
    .required("signed estimate activity");
    assert_canonical_fee(read, &signed.canonical, 4);
    for (ordinal, fixture) in ASSET_FEE_FIXTURES {
        let canonical = signed_fee_activity(ModuleId::Asset, ordinal, &fee_fixture(fixture));
        if ordinal == 6 {
            assert_eq!(
                read(
                    "lx_estimateFee",
                    serde_json::json!([hex_encode(&canonical)])
                )["error"]["code"],
                -32602
            );
            assert_canonical_fee(read, &signed_receive_fee_activity(cluster, funding), 4);
        } else {
            assert_canonical_fee(read, &canonical, 0);
        }
    }
    let program = signed_program_call(&cluster.treasury_seed, &cluster.treasury_did, 1, random32());
    assert_canonical_fee(read, &program, 0);
    for (ordinal, fixture) in PROGRAM_FEE_FIXTURES {
        assert_canonical_fee(
            read,
            &signed_fee_activity(ModuleId::Programs, ordinal, &fee_fixture(fixture)),
            0,
        );
    }
    for (ordinal, payload) in program_lifecycle_fee_payloads() {
        assert_canonical_fee(
            read,
            &signed_fee_activity(ModuleId::Programs, ordinal, &payload),
            0,
        );
    }
    let reserved = signed_fee_activity(ModuleId::Asset, 9, &[]);
    let reserved = read("lx_estimateFee", serde_json::json!([hex_encode(&reserved)]));
    assert_eq!(reserved["error"]["code"], -32001);
    assert_eq!(
        reserved["error"]["data"]["error"]["code"],
        "asset_ordinal_reserved"
    );
    assert_eq!(
        read("lx_getAsset", serde_json::json!(["63".repeat(32)]))["error"]["code"],
        -32001
    );
    assert_eq!(
        read("lx_getAsset", serde_json::json!(["00".repeat(32)]))["error"]["code"],
        -32602
    );
    assert_eq!(
        read("lx_estimateFee", serde_json::json!(["abcd"]))["error"]["code"],
        -32602
    );
    assert_eq!(
        read("lx_getBalances", serde_json::json!(["bad/path"]))["error"]["code"],
        -32602
    );
}

const ASSET_FEE_FIXTURES: [(u16, &str); 7] = [
    (
        1,
        include_str!("../../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-1.hex"),
    ),
    (
        4,
        include_str!("../../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-4.hex"),
    ),
    (
        6,
        include_str!("../../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-6.hex"),
    ),
    (
        7,
        include_str!("../../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-7.hex"),
    ),
    (
        8,
        include_str!("../../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-8.hex"),
    ),
    (
        10,
        include_str!("../../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-10.hex"),
    ),
    (
        11,
        include_str!("../../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-11.hex"),
    ),
];

const PROGRAM_FEE_FIXTURES: [(u16, &str); 2] = [
    (
        5,
        include_str!("../../../../../agent/crates/layerx-crypto/tests/fixtures/payments/9-5.hex"),
    ),
    (
        6,
        include_str!("../../../../../agent/crates/layerx-crypto/tests/fixtures/payments/9-6.hex"),
    ),
];

fn fee_fixture(value: &str) -> Vec<u8> {
    value
        .trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).required("fee fixture UTF-8");
            u8::from_str_radix(digits, 16).required("fee fixture hex")
        })
        .collect()
}

fn signed_fee_activity(module: ModuleId, ordinal: u16, payload_bytes: &[u8]) -> Vec<u8> {
    use ed25519_dalek::Signer as _;
    let key = SigningKey::from_bytes(&[42; 32]);
    let activity_type = ActivityType::new(module, ordinal).required("fee activity type");
    let registration =
        ModuleRegistration::new(module, &[activity_type]).required("fee module registration");
    let registry = ModuleRegistry::new(&[registration]).required("fee registry");
    let payload = Payload::new(&registry, activity_type, payload_bytes).required("fee payload");
    let payload_hash = layerx_wire::hash::payload_hash_for(&payload).required("fee payload hash");
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(PROTOCOL_VERSION)
        .and_then(|value| value.network_id(NETWORK_ID))
        .and_then(|value| value.activity_type(activity_type))
        .and_then(|value| value.actor_did(Did::new(b"did:layerx:alice").required("fee actor DID")))
        .and_then(|value| {
            value.authority(
                Authority::owner(&key.verifying_key().to_bytes()).required("fee authority"),
            )
        })
        .and_then(|value| value.account_sequence(7))
        .and_then(|value| {
            value.timestamp_bound(TimestampBound::new(1, u64::MAX).required("fee validity"))
        })
        .and_then(|value| value.idempotency_key(IdempotencyKey::new([0x71; 32])))
        .and_then(|value| value.fee_limit(Amount::from_u128(1_000_000)))
        .and_then(|value| value.payload_hash(payload_hash))
        .and_then(|value| value.payload(payload))
        .required("fee envelope fields");
    let unsigned = builder.build().required("fee envelope");
    let preimage = layerx_wire::sign::preimage_unsigned(&unsigned).required("fee signing preimage");
    let signature = key.sign(preimage.as_bytes()).to_bytes();
    layerx_wire::activity::encode_signed_envelope(
        &unsigned.attach_signature(Signature::new(&signature).required("fee signature")),
    )
    .required("signed fee activity")
}

fn signed_receive_fee_activity(cluster: &Cluster, funding: &funding::Funding) -> Vec<u8> {
    let grant = funding::payer_grant(&funding::GrantRequest {
        payer_seed: &cluster.treasury_seed,
        payer_did: &cluster.treasury_did,
        recipient_did: &funding.recipient_did,
        asset: cluster.asset,
        per_draw_maximum: 1,
        allowance: 1,
        recurring_window: None,
        expiration: now_ms() + 60_000,
        purpose_hash: random32(),
        revocation_sequence: 0,
    })
    .required("fee payer grant");
    let key = random32();
    let receive = funding::receive(&funding.recipient_seed, &grant, 1, key, 1)
        .required("fee Receive authorization");
    funding::payment(
        &funding.recipient_seed,
        &funding.recipient_did,
        1,
        key,
        &receive,
    )
    .required("signed fee Receive")
    .canonical
}

fn program_lifecycle_fee_payloads() -> [(u16, Vec<u8>); 3] {
    use layerx_types::program_lifecycle::{
        NativeProgramDeploy, NativeProgramUpgrade, NativeProgramWindDown, ProgramUpgradePolicy,
        ProgramWindDownOperation,
    };
    use sha2::Digest as _;
    let wasm = b"\0asm\x01\0\0\0";
    let hash = sha2::Sha256::digest(wasm).into();
    [
        (
            1,
            NativeProgramDeploy {
                program_id: ProgramId::new([1; 32]),
                guest_abi: 2,
                policy: ProgramUpgradePolicy::Immutable,
                new_hash: hash,
                interface: None,
                wasm,
            }
            .encode()
            .required("fee deploy"),
        ),
        (
            2,
            NativeProgramUpgrade {
                program_id: ProgramId::new([1; 32]),
                guest_abi: 2,
                old_hash: [2; 32],
                new_hash: hash,
                migration_hook: &[],
                clear_interface: false,
                interface: None,
                wasm,
            }
            .encode()
            .required("fee upgrade"),
        ),
        (
            7,
            NativeProgramWindDown {
                program_id: ProgramId::new([1; 32]),
                operation: ProgramWindDownOperation::Tombstone,
            }
            .encode()
            .required("fee wind-down"),
        ),
    ]
}

fn assert_canonical_fee(
    read: &impl Fn(&str, serde_json::Value) -> serde_json::Value,
    canonical: &[u8],
    activity_price: usize,
) {
    let fee = read("lx_estimateFee", serde_json::json!([hex_encode(canonical)]));
    assert_eq!(
        fee["result"]["fee"],
        (canonical.len() + activity_price).to_string(),
        "{fee}"
    );
    assert_eq!(fee["result"]["canonical_bytes"], canonical.len());
    assert_eq!(
        fee["result"]["canonical_schedule"]
            .as_str()
            .required("schedule")
            .len(),
        494
    );
}

#[test]
fn local_funding_recovers_a_submitted_operation_after_restart() {
    let (cluster, funding) = funding::start();
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let public_key = hex_encode(
        &SigningKey::from_bytes(&funding.recipient_seed)
            .verifying_key()
            .to_bytes(),
    );
    let body = funding_body(&funding.recipient_did, &public_key, 25);
    let key = "durable-funded-send";
    let path = "/admin/v1/testnet/fund";
    let first = boundary.admin_post(path, key, &body);
    assert_eq!(first.status, 200, "{}", first.body);
    let account = hex_encode(
        &layerx_platform_core::main_account(&funding.recipient_did).required("recipient account"),
    );
    let balance_path = format!("/v1/accounts/{account}/balance");
    let balance = boundary.core.get(&balance_path);
    assert_eq!(balance.status, 200, "{}", balance.body);
    let digest = hex_encode(&sha256(&[b"fund", &[0], key.as_bytes()]));
    let journal = cluster
        .root
        .join("state/journal")
        .join(format!("{digest}.json"));
    let canonical_digest = hex_encode(&sha256(&[b"fund-canonical", &[0], key.as_bytes()]));
    let canonical_path = cluster
        .root
        .join("state/journal")
        .join(format!("{canonical_digest}.json"));
    let canonical = fs::read(&canonical_path).required("durable canonical funding");
    drop(boundary);
    let complete = fs::read(&journal).required("complete funding journal");
    fs::write(journal.with_extension("complete.json"), &complete)
        .required("retain complete outcome");
    let first_record = complete
        .iter()
        .position(|byte| *byte == b'\n')
        .required("pending journal record")
        + 1;
    let mut interrupted = complete[..first_record].to_vec();
    interrupted.extend_from_slice(b"{\"request_digest\":");
    fs::write(&journal, interrupted).required("interrupt result persistence");
    fs::File::open(&journal)
        .required("journal file")
        .sync_all()
        .required("persist interruption");
    let boundary = start_boundary(&cluster, &certificates);
    let recovered = boundary.admin_post(path, key, &body);
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(json(&recovered), json(&first));
    assert_eq!(json(&boundary.core.get(&balance_path)), json(&balance));
    assert_eq!(
        fs::read(&canonical_path).required("recovered canonical funding"),
        canonical
    );
    let mut conflicting: serde_json::Value =
        serde_json::from_str(&body).required("funding request");
    conflicting["amount"] = serde_json::json!(26);
    assert_refusal(
        &boundary.admin_post(path, key, &conflicting.to_string()),
        409,
        "idempotency_conflict",
    );
}

#[test]
fn local_gateway_program_custody_journey() {
    let (cluster, _funding) = funding::start();
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let identity = start_local_identity(&cluster, &certificates);
    let authority = start_local_authority(&cluster, &certificates);
    let redis = start_local_redis(&cluster, &certificates);
    let gateway = start_gateway_runtime(
        &cluster,
        &certificates,
        &boundary,
        &identity,
        &authority,
        &redis,
        true,
    );
    let key = issue_local_scoped_key(
        &certificates,
        &gateway,
        &identity,
        &["activity:write", "program:call"],
    );
    let config = cluster.root.join("program-journey.curl");
    write(
        &config,
        format!(
            "header = \"Authorization: LayerX-Key {}:{}\"\n",
            key["key"]["id"].as_str().required("key id"),
            key["key"]["secret"].as_str().required("key secret"),
        )
        .as_bytes(),
        0o600,
    );
    let signer = cluster.root.join("program-journey.signer");
    write(&signer, &cluster.treasury_seed, 0o600);
    let artifact = std::env::var_os("LAYERX_TEST_ESCROW_WASM").required("built escrow WASM");
    let output = cluster.root.join("program-custody-journey");
    let status = Command::new(
        std::env::var_os("LAYERX_TEST_PYTHON").required("qualified Python with cryptography"),
    )
    .arg(repository_root().join("platform/hosted/testnet/tests/program-journey.py"))
    .arg("--gateway")
    .arg(format!("https://localhost:{}", gateway.port))
    .arg("--ca")
    .arg(certificates.path("ca.pem"))
    .arg("--auth-config")
    .arg(config)
    .arg("--signer")
    .arg(signer)
    .arg("--did")
    .arg(&cluster.treasury_did)
    .arg("--asset")
    .arg(hex_encode(&cluster.asset))
    .arg("--network-id")
    .arg(NETWORK_ID.to_string())
    .arg("--wasm")
    .arg(artifact)
    .arg("--output")
    .arg(&output)
    .status()
    .required("real funded Programs journey");
    assert!(status.success(), "real Programs journey failed: {status}");
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("result.json")).required("Programs evidence"))
            .required("Programs evidence JSON");
    let bytes = layerx_platform_core::hex_decode(
        result["result"]["receipt"]
            .as_str()
            .required("Programs receipt"),
    )
    .required("Programs receipt bytes");
    let receipt = layerx_proof::receipt::verify_sequencer_signature(&bytes, cluster.sequencer_key)
        .required("real Programs signature");
    let protocol = receipt.protocol().required("Programs protocol");
    assert_eq!(protocol.result_code(), 0);
    assert_eq!((protocol.module_id(), protocol.operation()), (9, 3));
    let outcome = protocol
        .program_outcome()
        .required("committed Programs outcome");
    for (field, expected) in [
        ("terminal_payload", outcome.terminal_payload_root()),
        ("call_graph", outcome.call_graph_root()),
    ] {
        let bytes = layerx_platform_core::hex_decode(
            result["result"][field]
                .as_str()
                .required("committed artifact"),
        )
        .required("artifact bytes");
        assert!(!bytes.is_empty());
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        assert_eq!(digest, expected);
    }
    assert_eq!(result["after"]["balance"], "1");
}

fn assert_wallet_receipt_verifier(
    cluster: &Cluster,
    certificates: &Certificates,
    gateway: &Gateway,
    key: &serde_json::Value,
    activity: &[u8; 32],
) {
    let request = cluster.root.join("wallet-verifier-request.json");
    write(&request, &serde_json::to_vec(&serde_json::json!({
        "action": "wait", "activity_id": hex_encode(activity), "commitment": "executed", "timeout_ms": "30000",
        "configuration": {
            "endpoint": format!("https://localhost:{}/rpc", gateway.port),
            "key_id": key["key"]["id"], "key_secret": key["key"]["secret"],
            "ca_der": hex_encode(&certificates.ca_der), "protocol_version": PROTOCOL_VERSION.to_string(),
            "network_id": NETWORK_ID.to_string(), "sequencer_id": hex_encode(&cluster.sequencer_id),
            "sequencer_key": hex_encode(&cluster.sequencer_key), "first_batch": "1", "last_batch": u64::MAX.to_string(),
        }
    })).required("wallet verifier request"), 0o600);
    let status = Command::new(std::env::var_os("LAYERX_TEST_PYTHON").required("qualified Python"))
        .arg(repository_root().join("platform/hosted/testnet/tests/wallet-receipt-journey.py"))
        .arg("--wallet")
        .arg(local_binary("layerx-wallet-rpc"))
        .arg("--request")
        .arg(request)
        .arg("--ca")
        .arg(certificates.path("ca.pem"))
        .arg("--certificate")
        .arg(certificates.path("core.pem"))
        .arg("--key")
        .arg(certificates.path("core-key.pem"))
        .arg("--output")
        .arg(cluster.root.join("wallet-receipt-evidence.json"))
        .status()
        .required("actual wallet receipt verifier");
    assert!(
        status.success(),
        "wallet receipt verification failed: {status}"
    );
}
