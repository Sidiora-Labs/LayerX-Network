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
    let authorization = format!("Bearer {bearer}");
    let answer = http.request(
        "POST",
        path,
        &[
            ("Authorization", &authorization),
            ("Content-Type", "application/json"),
            ("Idempotency-Key", "local-key-issuance"),
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
    port: u16,
    signer_file: String,
}

#[test]
fn local_gateway_lifecycle() {
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
        &serde_json::json!({"sub":cluster.treasury_did,"allowed_signer_public_keys":[signer],"account":format!("agent:{}:main",cluster.treasury_did),"audiences":[]}),
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
    let redis_config = format!("bind 127.0.0.1\nport 0\ntls-port {redis_port}\ntls-cert-file {}\ntls-key-file {}\ntls-ca-cert-file {}\ntls-auth-clients no\naclfile {acl}\nappendonly yes\nappendfsync always\ndir {}\nprotected-mode yes\n", certificates.path("core.pem").display(), certificates.path("core-key.pem").display(), certificates.path("ca.pem").display(), redis_directory.display());
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
                &serde_json::json!({"schema_version":2,"assets":[{"asset":hex_encode(&cluster.asset),"currency":"NATIVE","decimals":0,"symbol":"LXR"}],"modules":[{"module":1,"ordinals":[5]},{"module":9,"ordinals":[1,2,3,5,6,7]}]}).to_string(),
            ),
        ),
    ]);
    gateway_env.extend(gateway_upstream_environment(
        cluster, boundary, identity, authority, redis,
    ));
    let gateway_process = local_service(cluster, "layerx-gateway", gateway_port, &gateway_env);
    Gateway {
        _process: gateway_process,
        port: gateway_port,
        signer_file,
    }
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
            local_secret(&cluster.root, "component-token", &token()),
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
    let manifest = local_manifest(cluster);
    let evidence_directory = cluster.root.join("gateway-offline-evidence");
    make_dir(&evidence_directory, 0o700);
    let authority_token =
        fs::read_to_string(authority_token_file).required("local authority token");
    let authority_curl = local_secret(&cluster.root, "authority-curl.conf", &format!(
        "silent\nshow-error\nfail\nmax-time = 60\nproto = \"=https\"\ncacert = \"{}\"\ncert = \"{}\"\nkey = \"{}\"\nheader = \"Authorization: Bearer {}\"\n",
        certificates.path("ca.pem").display(), certificates.path("client.pem").display(), certificates.path("client-key.pem").display(), authority_token.trim()
    ));
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

fn local_manifest(cluster: &Cluster) -> PathBuf {
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
        let signed = signed_program_activity(
            &cluster.treasury_seed,
            &cluster.treasury_did,
            first_sequence + u64::try_from(index).required("index"),
            ordinal,
            &payload,
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
        let mut headers = vec![("Content-Type", "application/json")];
        if authenticated {
            headers.push(("Authorization", authorization.as_str()));
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
    };
    assert_eq!(
        call("lx_getNodeInfo", serde_json::json!([]), false)["result"]["network_id"],
        NETWORK_ID
    );
    let manifest = local_manifest(&cluster);
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
    assert_eq!(finalised["result"]["state"], "pending", "{finalised}");
    assert_eq!(finalised["result"]["commitment"], "executed");
    assert_eq!(
        call(
            "lx_sendActivity",
            serde_json::json!([hex_encode(&signed), "ack"]),
            true
        )["error"]["code"],
        -32602
    );
}
