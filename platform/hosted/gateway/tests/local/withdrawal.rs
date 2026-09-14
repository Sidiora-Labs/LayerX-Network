use super::*;
use layerx_intents::{BridgeWithdrawRequest, DisclosureCheck, Intent, IntentKind};
use layerx_sdk::rpc_verification::{AccountPolicy, VerifiedRpcAccount};
use layerx_types::account::AccountId;
use layerx_types::ids::AssetId;
use layerx_types::intent::EvmAddress;

pub(super) fn configure_genesis(request: &[u8]) -> Vec<u8> {
    assert_eq!(&request[..7], b"LXGB\x02\0\x03");
    assert_eq!(&request[19..21], &1_u16.to_be_bytes());
    let modules =
        fs::read_to_string(repository_root().join("platform/hosted/node/genesis-modules.conf"))
            .required("public testnet module configuration");
    assert_eq!(
        modules.lines().collect::<Vec<_>>(),
        ["budget", "escrow", "perps", "service", "stream"]
    );
    let mut parameters = BTreeMap::new();
    parameters.insert("parameter-version".to_owned(), 1_u8);
    parameters.insert("native-fee-authority-version".to_owned(), 2_u8);
    for name in modules.lines() {
        parameters.insert(format!("module-enable:{name}"), 1);
    }
    let mut configured = request[..19].to_vec();
    configured.extend_from_slice(&7_u16.to_be_bytes());
    for (key, value) in parameters {
        configured.extend_from_slice(&7_u16.to_be_bytes());
        let mut padded = [0; 32];
        padded[..key.len()].copy_from_slice(key.as_bytes());
        configured.extend_from_slice(&padded);
        configured.extend_from_slice(&[0; 31]);
        configured.push(value);
    }
    configured.extend_from_slice(&request[87..]);
    let offset = configured.len() - 247;
    assert_eq!(&configured[offset - 2..offset], &247_u16.to_be_bytes());
    assert_eq!(&configured[offset..offset + 2], &2_u16.to_be_bytes());
    assert_eq!(configured[offset + 86], 10);
    configured[offset - 2..offset].copy_from_slice(&255_u16.to_be_bytes());
    configured[offset..offset + 2].copy_from_slice(&3_u16.to_be_bytes());
    configured[offset + 86] = 11;
    configured.extend_from_slice(&17_u64.to_be_bytes());
    configured
}

fn account(
    http: &Http,
    authorization: &str,
    name: &str,
    cluster: &Cluster,
) -> layerx_proof::state::CanonicalAccount {
    let account = AccountId::parse(name).required("canonical account name");
    let id = layerx_wire::hash::account_id_for_protocol(&account, PROTOCOL_VERSION)
        .required("native account identifier");
    let response = local_rpc(
        http,
        authorization,
        "lx_getAccount",
        &serde_json::json!([hex_encode(&id)]),
        true,
    );
    assert!(response.get("error").is_none(), "account proof: {response}");
    VerifiedRpcAccount::from_rpc_result(
        &response["result"],
        id,
        &AccountPolicy {
            protocol_version: PROTOCOL_VERSION,
            network_id: NETWORK_ID,
            sequencer_key: cluster.sequencer_key,
        },
    )
    .required("independently verified public account")
    .evidence()
    .account()
    .clone()
}

fn identity(http: &Http, authorization: &str, did: &str) -> u64 {
    let response = local_rpc(
        http,
        authorization,
        "lx_getSequence",
        &serde_json::json!([did, "identity"]),
        true,
    );
    assert!(
        response.get("error").is_none(),
        "identity sequence: {response}"
    );
    assert_eq!(response["result"]["did"], did);
    assert_eq!(
        response["result"]["verification"],
        "authenticated_node_snapshot"
    );
    response["result"]["next_sequence"]
        .as_str()
        .required("identity sequence")
        .parse()
        .required("identity sequence integer")
}

fn signed(cluster: &Cluster, sequence: u64) -> (Vec<u8>, [u8; 32]) {
    use layerx_types::ids::CheckpointId;
    let key = SigningKey::from_bytes(&cluster.treasury_seed);
    let idempotency = random32();
    let activity_type = ActivityType::new(ModuleId::Asset, 9).required("withdrawal type");
    let registry =
        ModuleRegistry::new(&[ModuleRegistration::new(ModuleId::Asset, &[activity_type])
            .required("withdrawal registration")])
        .required("withdrawal registry");
    let request = BridgeWithdrawRequest::new(
        CheckpointId::new([0x42; 32]),
        17,
        AccountId::parse(&format!("agent:{}:main", cluster.treasury_did))
            .required("withdrawal owner account"),
        AccountId::parse("system:paxeer-withdrawals").required("withdrawal custody account"),
        EvmAddress::new([0x31; 20]),
        AssetId::new(cluster.asset),
        Amount::from_u128(1),
        IdempotencyKey::new(idempotency),
    )
    .required("withdrawal intent");
    let intent = Intent::v2(IntentKind::BridgeWithdrawRequest(request));
    let compiled =
        layerx_intents::compile(&intent, &registry).required("canonical withdrawal payload");
    DisclosureCheck::verify(&intent, &compiled).required("withdrawal disclosure roundtrip");
    let now = now_ms();
    let envelope = layerx_crypto::send::encode_payment_envelope(
        ModuleId::Asset,
        9,
        compiled.payload().as_bytes(),
        &layerx_crypto::send::EnvelopeOptions {
            actor: &cluster.treasury_did,
            public_key: key.verifying_key().to_bytes(),
            protocol_version: PROTOCOL_VERSION,
            network_id: NETWORK_ID,
            identity_sequence: sequence,
            idempotency_key: idempotency,
            fee_limit: 17,
            not_before: now - 1000,
            not_after: now + 120_000,
        },
    )
    .required("withdrawal envelope");
    let disclosure = layerx_crypto::disclosure::bind(&envelope.canonical, &envelope.registry)
        .required("canonical signed withdrawal disclosure");
    let local = layerx_crypto::signer::LocalSigner::new(cluster.treasury_seed);
    let mut future = layerx_crypto::signer::sign_disclosed(
        &local,
        &envelope.canonical,
        &disclosure,
        &envelope.registry,
    );
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let std::task::Poll::Ready(signature) =
        std::future::Future::poll(future.as_mut(), &mut context)
    else {
        panic!("local signer unexpectedly pending")
    };
    let signature = signature.required("withdrawal owner signature");
    drop(future);
    let signed = envelope
        .envelope
        .attach_signature(Signature::new(signature.as_bytes()).required("canonical signature"));
    let bytes =
        layerx_wire::activity::encode_signed_envelope(&signed).required("signed withdrawal");
    let decoded =
        layerx_wire::activity::decode_signed(&bytes, &registry).required("withdrawal roundtrip");
    assert_eq!(
        (decoded.protocol_version(), decoded.network_id()),
        (PROTOCOL_VERSION, NETWORK_ID)
    );
    let id = layerx_wire::hash::activity_id(&decoded).required("withdrawal activity identifier");
    (bytes, id)
}

fn executed(http: &Http, authorization: &str, params: &serde_json::Value) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let response = local_rpc(http, authorization, "lx_sendActivity", params, true);
        if response.get("error").is_none() {
            return response;
        }
        assert_eq!(response["error"]["code"], -32001, "{response}");
        assert_eq!(response["error"]["data"]["state"], "pending", "{response}");
        assert!(
            Instant::now() < deadline,
            "withdrawal execution deadline: {response}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn complete_module_registry(gateway: &Gateway) {
    let bytes = fs::read(&gateway.environment["LAYERX_GATEWAY_MODULE_REGISTRY_FILE"])
        .required("gateway committed module registry");
    let document: serde_json::Value =
        serde_json::from_slice(&bytes).required("gateway module registry JSON");
    let modules = document["modules"]
        .as_array()
        .required("committed module declarations")
        .iter()
        .map(|entry| entry["module"].as_u64().required("module identifier"))
        .collect::<Vec<_>>();
    assert_eq!(
        modules,
        layerx_types::payload::ModuleId::ALL.map(|module| u64::from(module as u16))
    );
}

pub(super) fn run(
    cluster: &Cluster,
    certificates: &Certificates,
    gateway: &mut Gateway,
    key: &serde_json::Value,
) {
    complete_module_registry(gateway);
    let http = Http {
        port: gateway.port,
        ca: Certificate::from_der(&certificates.ca_der).required("gateway CA"),
        identity: None,
    };
    let authorization = format!(
        "LayerX-Key {}:{}",
        key["key"]["id"].as_str().required("key id"),
        key["key"]["secret"].as_str().required("key secret")
    );
    let source = format!("agent:{}:main", cluster.treasury_did);
    let names = [source.as_str(), "system:fees", "system:paxeer-withdrawals"];
    let before = names.map(|name| account(&http, &authorization, name, cluster));
    let sequence = identity(&http, &authorization, &cluster.treasury_did);
    let (canonical, activity_id) = signed(cluster, sequence);
    write(
        &cluster.root.join("public-withdrawal.lxa"),
        &canonical,
        0o600,
    );
    let params = serde_json::json!([hex_encode(&canonical), "executed"]);
    let executed = executed(&http, &authorization, &params);
    assert!(
        executed.get("error").is_none(),
        "public withdrawal: {executed}"
    );
    assert_eq!(executed["result"]["activity_id"], hex_encode(&activity_id));
    assert_eq!(executed["result"]["result_code"], 0);
    let response = local_rpc(
        &http,
        &authorization,
        "lx_getReceipt",
        &serde_json::json!([hex_encode(&activity_id)]),
        true,
    );
    let receipt = hex_decode(
        response["result"]["receipt"]
            .as_str()
            .required("public withdrawal receipt"),
    )
    .required("withdrawal receipt bytes");
    let verified =
        layerx_proof::receipt::verify_sequencer_signature(&receipt, cluster.sequencer_key)
            .required("independent withdrawal signature");
    let verified = verified.protocol().required("native withdrawal receipt");
    assert_eq!(
        (
            verified.protocol_version(),
            verified.module_id(),
            verified.operation(),
            verified.result_code()
        ),
        (PROTOCOL_VERSION, 1, 9, 0)
    );
    assert_eq!(verified.activity_id(), activity_id);
    assert_eq!(verified.fee_charged(), 17);
    let authorized = layerx_proof::receipt::AuthorizedBatch::new(
        verified.batch_id(), verified.asset(), verified.previous_state_root(),
        verified.resulting_state_root(), cluster.sequencer_key,
    );
    layerx_proof::receipt::withdrawal::verify(&receipt, &authorized, &canonical, NETWORK_ID)
        .required("withdrawal receipt bound to original owner activity and monetary effects");
    let after = names.map(|name| account(&http, &authorization, name, cluster));
    assert_eq!(after[0].balance() + 18, before[0].balance());
    assert_eq!(after[1].balance(), before[1].balance() + 17);
    assert_eq!(after[2].balance(), before[2].balance() + 1);
    assert_eq!(after[0].next_sequence, before[0].next_sequence + 1);
    assert_eq!(
        identity(&http, &authorization, &cluster.treasury_did),
        sequence + 1
    );
    for restart in [false, true] {
        if restart {
            gateway.process.stop();
            gateway.process = local_service(
                cluster,
                "layerx-gateway",
                gateway.port,
                &gateway.environment,
            );
        }
        assert_eq!(
            local_rpc(&http, &authorization, "lx_sendActivity", &params, true),
            executed
        );
        assert_eq!(
            names.map(|name| account(&http, &authorization, name, cluster)),
            after
        );
        assert_eq!(
            identity(&http, &authorization, &cluster.treasury_did),
            sequence + 1
        );
        assert_eq!(
            local_rpc(
                &http,
                &authorization,
                "lx_getReceipt",
                &serde_json::json!([hex_encode(&activity_id)]),
                true
            ),
            response
        );
    }
    println!("public owner withdrawal binds the native receipt, exact fee, verified balances and restart replay");
}

#[test]
fn local_gateway_paid_withdrawal_preserves_fees_across_restart() {
    let (cluster, _funding) = funding::start_withdrawal();
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let identity_service = start_local_identity(&cluster, &certificates);
    let authority = start_local_authority(&cluster, &certificates);
    let redis = start_local_redis(&cluster, &certificates);
    let mut gateway = start_local_gateway(
        &cluster,
        &certificates,
        &boundary,
        &identity_service,
        &authority,
        &redis,
    );
    let key = issue_local_scoped_key(
        &certificates,
        &gateway,
        &identity_service,
        &["activity:write", "receipt:read"],
    );
    run(&cluster, &certificates, &mut gateway, &key);
}

#[test]
fn local_gateway_legacy_genesis_refuses_withdrawal_without_mutation() {
    let (cluster, _funding) = funding::start();
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let identity_service = start_local_identity(&cluster, &certificates);
    let authority = start_local_authority(&cluster, &certificates);
    let redis = start_local_redis(&cluster, &certificates);
    let gateway = start_local_gateway(
        &cluster,
        &certificates,
        &boundary,
        &identity_service,
        &authority,
        &redis,
    );
    let key = issue_local_scoped_key(
        &certificates,
        &gateway,
        &identity_service,
        &["activity:write"],
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
    let name = format!("agent:{}:main", cluster.treasury_did);
    let before = account(&http, &authorization, &name, &cluster);
    let sequence = identity(&http, &authorization, &cluster.treasury_did);
    let (canonical, _) = signed(&cluster, sequence);
    let response = local_rpc(
        &http,
        &authorization,
        "lx_sendActivity",
        &serde_json::json!([hex_encode(&canonical), "executed"]),
        true,
    );
    assert_eq!(response["error"]["code"], -32001, "{response}");
    assert_eq!(
        response["error"]["data"]["error"]["code"], "activity_refused",
        "{response}"
    );
    assert!(response.get("result").is_none());
    assert_eq!(account(&http, &authorization, &name, &cluster), before);
    assert_eq!(
        identity(&http, &authorization, &cluster.treasury_did),
        sequence
    );
}
