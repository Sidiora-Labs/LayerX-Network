use super::*;

pub fn configure(
    cluster: &Cluster,
    certificates: &Certificates,
    identity: &LocalIdentity,
    authority: &LocalAuthority,
    environment: &mut BTreeMap<&'static str, String>,
) -> (PathBuf, u16, Daemon) {
    let port = free_port();
    let request_token = local_secret(&cluster.root, "registry-request-token", &token());
    let node_port = free_port();
    let node_token = local_secret(&cluster.root, "registry-boundary-token", &token());
    let node = start_agent_boundary(cluster, certificates, node_port, &node_token);

    environment.insert(
        "LAYERX_GATEWAY_PROGRAM_REGISTRY_URL",
        format!("https://localhost:{port}"),
    );
    environment.insert(
        "LAYERX_GATEWAY_PROGRAM_REGISTRY_TOKEN_FILE",
        request_token.clone(),
    );
    let event_environment: BTreeMap<_, _> = environment
        .iter()
        .filter(|(name, _)| name.starts_with("LAYERX_EVENTS_"))
        .map(|(name, value)| (*name, value.clone()))
        .collect();
    let config = serde_json::json!({
        "root": cluster.root, "listen_port": port,
        "node_url": format!("https://localhost:{node_port}"),
        "node_token_file": node_token,
        "authority_url": format!("https://localhost:{}", authority.port),
        "authority_token_file": authority.token_file,
        "replica_id": hex_encode(&sha256(&[b"layerx-authority-replica:", hex_encode(&cluster.sequencer_key).as_bytes()])),
        "sequencer_id": hex_encode(&cluster.sequencer_id),
        "sequencer_public_key": hex_encode(&cluster.sequencer_key),
        "network_id": NETWORK_ID, "epoch": verified_epoch(cluster),
        "certificates_dir": certificates.path("ca.der").parent(),
        "service_bin_dir": std::env::var("LAYERX_TEST_REGISTRY_BIN_DIR").required("qualified registry binaries"),
        "runtime_image": std::env::var("LAYERX_TEST_REGISTRY_IMAGE").required("qualified registry runtime image"),
        "quota_root": std::env::var("LAYERX_TEST_REGISTRY_QUOTA_ROOT").required("qualified registry quota filesystem"),
        "builder_root": std::env::var("LAYERX_TEST_BUILDER_ROOT").required("qualified builder root"),
        "builder_digest_file": std::env::var("LAYERX_TEST_BUILDER_DIGEST_FILE").required("qualified builder digest"),
        "request_token_file": request_token,
        "event_environment": event_environment,
        "identity_url": format!("https://localhost:{}", identity.port),
        "identity_token_file": identity.tokens.join("registry"),
        "client_pkcs12": environment["LAYERX_GATEWAY_CLIENT_IDENTITY_PKCS12"],
        "client_password_file": environment["LAYERX_GATEWAY_CLIENT_IDENTITY_PASSWORD_FILE"],
        "lni_socket": cluster.lni_socket,
    });
    let path = cluster.root.join("registry-runtime.json");
    write(
        &path,
        serde_json::to_vec(&config)
            .required("registry runtime configuration")
            .as_slice(),
        0o600,
    );
    (path, port, node)
}

fn start_agent_boundary(
    cluster: &Cluster,
    certificates: &Certificates,
    port: u16,
    registry_token: &str,
) -> Daemon {
    let environment = BTreeMap::from([
        ("LAYERX_AGENT_BOUNDARY_LISTEN", format!("127.0.0.1:{port}")),
        (
            "LAYERX_AGENT_BOUNDARY_TLS_CERT_DER",
            text(&certificates.path("core.der")),
        ),
        (
            "LAYERX_AGENT_BOUNDARY_TLS_KEY_DER",
            text(&certificates.path("core-key.der")),
        ),
        (
            "LAYERX_AGENT_BOUNDARY_CLIENT_CA_DER",
            text(&certificates.path("ca.der")),
        ),
        (
            "LAYERX_AGENT_BOUNDARY_GATEWAY_TOKEN_FILE",
            local_secret(&cluster.root, "registry-boundary-gateway-token", &token()),
        ),
        (
            "LAYERX_AGENT_BOUNDARY_REGISTRY_TOKEN_FILE",
            registry_token.to_owned(),
        ),
        (
            "LAYERX_AGENT_BOUNDARY_WEBHOOK_TOKEN_FILE",
            local_secret(&cluster.root, "registry-boundary-webhook-token", &token()),
        ),
        (
            "LAYERX_AGENT_BOUNDARY_LNI_SOCKET",
            text(&cluster.lni_socket),
        ),
        (
            "LAYERX_AGENT_BOUNDARY_NODE_URL",
            format!("http://127.0.0.1:{}", cluster.program_port),
        ),
        (
            "LAYERX_AGENT_BOUNDARY_NODE_BEARER_TOKEN_FILE",
            local_secret(
                &cluster.root,
                "registry-native-node-token",
                &cluster.program_token,
            ),
        ),
        (
            "LAYERX_AGENT_BOUNDARY_STATE_DIR",
            text(&cluster.root.join("registry-node-boundary")),
        ),
        (
            "LAYERX_AGENT_BOUNDARY_PROTOCOL_NETWORK_ID",
            NETWORK_ID.to_string(),
        ),
        ("LAYERX_AGENT_BOUNDARY_NETWORK_ID", NETWORK_ID.to_string()),
    ]);
    local_service(cluster, "layerx-agent-boundary", port, &environment)
}

pub fn start(cluster: &Cluster, config: &Path, port: u16) -> Daemon {
    let python = std::env::var_os("LAYERX_TEST_PYTHON").required("qualified Python");
    let helper = repository_root().join("platform/hosted/gateway/tests/local/registry_runtime.py");
    let mut process = spawn(
        Path::new(&python),
        &[&text(&helper), "--config", &text(config)],
        &BTreeMap::new(),
        false,
        cluster.root.join("registry-runtime.stderr"),
    );
    process.supervised = true;
    wait_for_port(port, &mut process, "actual registry runtime");
    process
}

fn verified_epoch(cluster: &Cluster) -> u64 {
    let gate = ConnectionGate::new(1);
    let mut transport = must(
        Uds::connect(&cluster.lni_socket, &gate, lni_limits()),
        "registry LNI",
    );
    let handshake = must(
        perform(&mut transport, &handshake_config(), None),
        "registry handshake",
    );
    let header = must(
        layerx_client::batch::lookup(
            &mut transport,
            handshake.node().interface_version,
            handshake.node().latest_sealed_batch,
            81,
            cluster.sequencer_key,
        ),
        "registry signed batch",
    );
    must(
        layerx_wire::receipt::decode_batch_header(header.canonical_bytes()),
        "registry canonical header",
    )
    .epoch()
}
