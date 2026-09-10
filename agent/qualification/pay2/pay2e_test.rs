use std::collections::BTreeSet;

fn parse_http_with_connection(raw: &[u8], expected_connection: &str) -> HttpAnswer {
    let position = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .required("PAY2 HTTP header terminator");
    let head = std::str::from_utf8(&raw[..position]).required("PAY2 HTTP headers");
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .required("PAY2 HTTP status");
    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line.split_once(':').required("PAY2 HTTP header");
        assert!(
            headers
                .insert(name.trim().to_ascii_lowercase(), value.trim().to_owned())
                .is_none(),
            "duplicate PAY2 HTTP header"
        );
    }
    let body = &raw[position + 4..];
    assert_eq!(
        headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok()),
        Some(body.len())
    );
    assert_eq!(
        headers.get("connection").map(String::as_str),
        Some(expected_connection)
    );
    assert!(!headers.contains_key("transfer-encoding"));
    HttpAnswer {
        status,
        headers,
        body: std::str::from_utf8(body)
            .required("PAY2 HTTP body")
            .to_owned(),
    }
}

struct Pay2GatewayIdentity<'a> {
    rpc: &'a layerx_sdk::rpc::RpcClient,
    capability: [u8; 32],
}

impl layerx_agentd::identity::IdentityResolver for Pay2GatewayIdentity<'_> {
    fn resolve(
        &mut self,
        did: &layerx_types::ids::Did,
    ) -> Result<Option<layerx_agentd::identity::CoreIdentity>, layerx_agentd::identity::IdentityError>
    {
        let text = std::str::from_utf8(did.as_bytes())
            .map_err(|_| layerx_agentd::identity::IdentityError::BoundaryUnavailable)?;
        let snapshot = self
            .rpc
            .get_identity_sequence(text)
            .map_err(|_| layerx_agentd::identity::IdentityError::BoundaryUnavailable)?;
        let mut canonical = snapshot.state_root.to_vec();
        canonical.extend_from_slice(&snapshot.observed_head_sequence.to_be_bytes());
        canonical.extend_from_slice(&snapshot.next_sequence.to_be_bytes());
        Ok(Some(layerx_agentd::identity::CoreIdentity {
            canonical_bytes: canonical,
            head_sequence: snapshot.observed_head_sequence,
            revocation_sequence: 1,
            verification_level: layerx_types::verify::VerificationLevel::STATE_PROVEN,
            frozen: false,
            authorities: vec![layerx_agentd::identity::ProtocolAuthority::CapabilityGrant(
                self.capability,
            )],
        }))
    }
}

struct Pay2McpContext {
    server: layerx_mcp::server::Server,
    session: layerx_agentd::session::SessionRecord,
    capability: layerx_agentd::capability::Capability,
}

#[derive(Clone, Copy)]
struct Pay2McpSpec<'a> {
    root: &'a Path,
    rpc: &'a layerx_sdk::rpc::RpcClient,
    actor: &'a str,
    ordinal: u16,
    counterparty: [u8; 32],
    asset: [u8; 32],
    amount_ceiling: u128,
    window_sequences: u64,
    current_sequence: u64,
    scope: &'a str,
    purpose: &'a str,
}

fn pay2_mcp_context(spec: Pay2McpSpec<'_>) -> Pay2McpContext {
    let tenant = layerx_agentd::store::TenantId::new(format!(
        "pay2-{}-{}",
        spec.ordinal, spec.current_sequence
    ))
    .required("PAY2 tenant");
    let capability_id = layerx_agentd::capability::CapabilityId(random32());
    let capability = layerx_agentd::capability::Capability::new(
        capability_id,
        tenant.clone(),
        layerx_agentd::capability::CapabilityDimensions {
            activity_types: BTreeSet::from([spec.ordinal]),
            counterparties: BTreeSet::from([spec.counterparty]),
            assets: BTreeSet::from([spec.asset]),
            amount_ceiling: spec.amount_ceiling,
            rate_ceiling: layerx_agentd::capability::RateCeiling {
                maximum_uses: 10,
                window_sequences: spec.window_sequences,
            },
            purposes: BTreeSet::from([spec.purpose.to_owned()]),
            expiry_sequence: spec.current_sequence.saturating_add(100),
        },
    )
    .required("PAY2 capability");
    let store_root = spec
        .root
        .join(format!("agentd-{}-{}", spec.ordinal, spec.current_sequence));
    let mut store = layerx_agentd::store::Store::open(&store_root).required("PAY2 store");
    capability
        .persist(&mut store)
        .required("PAY2 capability persistence");
    let agent = layerx_types::ids::Did::new(spec.actor.as_bytes()).required("PAY2 actor DID");
    let mut resolver = Pay2GatewayIdentity {
        rpc: spec.rpc,
        capability: capability_id.0,
    };
    let identity =
        layerx_agentd::identity::register(&mut store, tenant.clone(), agent.clone(), &mut resolver)
            .required("PAY2 gateway-backed identity");
    let request = layerx_agentd::session::OpenRequest {
        session_id: layerx_agentd::session::SessionId(random32()),
        token_id: random32(),
        tenant: tenant.clone(),
        agent,
        authority: layerx_agentd::identity::ProtocolAuthority::CapabilityGrant(capability_id.0),
        permitted_activity_types: BTreeSet::from([spec.ordinal]),
        scopes: BTreeSet::from([spec.scope.to_owned()]),
        expiry_sequence: spec.current_sequence.saturating_add(100),
        opening_client: "pay2-funded-qualification".to_owned(),
        policy_version: "pay2-funded-v1".to_owned(),
    };
    let mut sessions = layerx_agentd::session::SessionRegistry::default();
    let token = layerx_agentd::session::open(
        &mut store,
        &mut sessions,
        &identity,
        request,
        spec.current_sequence,
    )
    .required("PAY2 session");
    let session = sessions
        .get(&tenant, token.session_id())
        .cloned()
        .required("PAY2 opened session");
    let limiter =
        layerx_agentd::budget::BudgetLimiter::new(Vec::new()).required("PAY2 local limiter");
    let control = layerx_agentd::session_control::SessionControl::new(
        std::sync::Arc::new(std::sync::Mutex::new(store)),
        sessions,
        std::sync::Arc::new(layerx_agentd::prepare::PreparationLifecycle::default()),
        std::sync::Arc::new(limiter),
    );
    let server = layerx_mcp::server::Server::bind(
        control,
        token.credential(),
        capability_id,
        spec.current_sequence,
        spec.root.join(format!(
            "mcp-audit-{}-{}",
            spec.ordinal, spec.current_sequence
        )),
    )
    .required("PAY2 MCP server");
    Pay2McpContext {
        server,
        session,
        capability,
    }
}

fn pay2_policy(
    context: &Pay2McpContext,
    request: &layerx_agentd::policy::PolicyRequest,
) -> layerx_agentd::policy::PolicySet {
    layerx_agentd::policy::PolicySet {
        version: context.session.request.policy_version.clone(),
        rules: vec![layerx_agentd::policy::Rule {
            id: "pay2-funded-permit".to_owned(),
            effect: layerx_agentd::policy::RuleEffect::Permit,
            constraints: layerx_agentd::policy::RuleConstraints {
                activity_types: BTreeSet::from([request.activity_type]),
                counterparties: BTreeSet::from([request.counterparty]),
                assets: BTreeSet::from([request.asset]),
                maximum_amount: Some(context.capability.dimensions.amount_ceiling),
                maximum_cumulative_amount: Some(1_000_000_000_000_000),
                maximum_cumulative_count: Some(10),
                purposes: BTreeSet::from([request.purpose.clone()]),
                capability_ids: BTreeSet::from([context.capability.id]),
                session_ids: BTreeSet::from([context.session.request.session_id]),
                agents: BTreeSet::from([context.session.request.agent.clone()]),
                tenants: BTreeSet::from([context.session.request.tenant.clone()]),
                sequence_window: Some(layerx_agentd::policy::SequenceWindow {
                    first: request.core_sequence,
                    last: request.core_sequence,
                }),
                required_approval: false,
            },
        }],
        evaluation_step_limit: 1,
    }
}

fn pay2_credential(value: &serde_json::Value) -> layerx_sdk::programs::LayerXKeyCredential {
    let id = value["key"]["id"]
        .as_str()
        .required("PAY2 key id")
        .to_owned();
    let secret = value["key"]["secret"].as_str().required("PAY2 key secret");
    layerx_sdk::programs::LayerXKeyCredential::new(
        id,
        layerx_sdk::production::SecretBytes::new(secret.as_bytes()).required("PAY2 secret"),
    )
    .required("PAY2 credential")
}

fn pay2_options(
    actor: &str,
    idempotency_key: [u8; 32],
    commitment: layerx_sdk::rpc::Commitment,
) -> layerx_sdk::wallet::PaymentOptions {
    let now = now_ms();
    layerx_sdk::wallet::PaymentOptions {
        actor: actor.to_owned(),
        idempotency_key,
        fee_limit: 1_000_000_000_000,
        not_before: now.saturating_sub(1_000),
        not_after: now.saturating_add(60_000),
        commitment,
        wait_timeout: Duration::from_secs(60),
    }
}

fn pay2_account_sequence(rpc: &layerx_sdk::rpc::RpcClient, account: [u8; 32]) -> u64 {
    let id = hex_encode(&account);
    let value = rpc.get_account(&id).required("PAY2 account sequence");
    assert_eq!(value["account_id"], id);
    let sequence = value["next_sequence"]
        .as_str()
        .required("PAY2 account next_sequence");
    let parsed = sequence
        .parse::<u64>()
        .required("PAY2 account sequence integer");
    assert_eq!(parsed.to_string(), sequence);
    parsed
}

fn pay2_batch_header(
    receipt: &layerx_sdk::rpc_verification::VerifiedRpcReceipt,
) -> layerx_wire::receipt::BatchHeader {
    let canonical = receipt
        .batch_evidence()
        .required("PAY2 batch evidence")
        .canonical_header();
    layerx_wire::receipt::decode_batch_header(canonical).required("PAY2 batch header")
}

fn pay2_run<F: std::future::Future>(future: F) -> F::Output {
    struct WakeThread(std::thread::Thread);
    impl std::task::Wake for WakeThread {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let mut future = std::pin::pin!(future);
    let waker = std::task::Waker::from(std::sync::Arc::new(WakeThread(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => return value,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

fn pay2_evidence_authority(
    cluster: &Cluster,
) -> layerx_agentd::protocol_evidence::EvidenceAuthority {
    let source = cluster.root.join("pay2-sequencer-authority.txt");
    write(
        &source,
        format!(
            "layerx-sequencer-authority-v1\n{},{},1,1,{},active\n",
            hex_encode(&cluster.sequencer_id),
            hex_encode(&cluster.sequencer_key),
            u64::MAX
        )
        .as_bytes(),
        0o600,
    );
    let config = layerx_agentd::config::StartupConfig {
        network_id: NETWORK_ID,
        node_endpoint: cluster.lni_socket.clone(),
        expected_protocol_version: PROTOCOL_VERSION,
        tenants: BTreeSet::new(),
        policy_sources: BTreeMap::new(),
        signer_configurations: BTreeMap::new(),
        verification_defaults: BTreeMap::new(),
        sequencer_authority_source: source,
    };
    let mut gate = layerx_agentd::boot::Gate::new(&config).required("PAY2 evidence gate");
    let connection_gate = ConnectionGate::new(1);
    let mut transport = Uds::connect(&cluster.lni_socket, &connection_gate, lni_limits())
        .required("PAY2 evidence handshake transport");
    layerx_agentd::boot::handshake_gate(&mut gate, &mut transport)
        .required("PAY2 evidence handshake");
    gate.evidence_authority()
        .required("PAY2 evidence authority")
        .clone()
}

fn pay2_receipt_record(
    receipt: &layerx_sdk::rpc_verification::VerifiedRpcReceipt,
) -> serde_json::Value {
    let protocol = receipt
        .receipt()
        .protocol()
        .required("PAY2 protocol receipt");
    serde_json::json!({
        "activity_id": hex_encode(&protocol.activity_id()),
        "receipt_ref": hex_encode(&sha256(&[receipt.canonical_bytes()])),
        "batch_id": hex_encode(&protocol.batch_id()),
        "global_sequence": protocol.global_sequence(),
        "commitment": receipt.commitment().as_str(),
        "result_code": protocol.result_code()
    })
}

fn pay2_write_transcript(
    receipt: &layerx_sdk::rpc_verification::VerifiedRpcReceipt,
    tracking_only: bool,
) -> layerx_mcp::tools::write::WriteTranscript {
    let protocol = receipt
        .receipt()
        .protocol()
        .required("PAY2 transcript receipt");
    let receipt_ref = layerx_agent_api::track::ReceiptRef::new("receipt-funded-draw")
        .required("PAY2 receipt reference");
    layerx_mcp::tools::write::WriteTranscript {
        stages: if tracking_only {
            vec![layerx_mcp::tools::write::WriteStage::Track]
        } else {
            layerx_mcp::tools::write::ORDINARY_WRITE_STAGES.to_vec()
        },
        submission: Ok(layerx_agent_api::track::TrackedSubmission {
            submission_ref: layerx_agent_api::track::SubmissionRef::new("submission-funded-draw")
                .required("PAY2 submission reference"),
            state: layerx_agent_api::track::SubmissionState::Executed {
                receipt_ref: receipt_ref.clone(),
            },
            evidence: vec![layerx_agent_api::track::EvidenceRef {
                kind: "receipt".to_owned(),
                digest: protocol.batch_id(),
            }],
            verification_level: layerx_agent_api::verify::Level::BatchIncluded,
            transitions: Vec::new(),
        }),
        receipt: Some(layerx_mcp::tools::write::VerifiedReceipt {
            receipt_ref,
            canonical_receipt: layerx_agent_api::prepare::CanonicalBytes::new(
                receipt.canonical_bytes().to_vec(),
            )
            .required("PAY2 canonical receipt transcript"),
            verification_level: layerx_agent_api::verify::Level::BatchIncluded,
            evidence_ids: vec![protocol.batch_id()],
        }),
    }
}

struct Pay2FundedRun<'a> {
    cluster: &'a Cluster,
    funding: &'a funding::Funding,
    payer_rpc: &'a layerx_sdk::rpc::RpcClient,
    recipient_rpc: &'a layerx_sdk::rpc::RpcClient,
    payer_wallet: &'a layerx_sdk::wallet::Wallet<'a>,
    recipient_wallet: &'a layerx_sdk::wallet::Wallet<'a>,
    evidence_authority: layerx_agentd::protocol_evidence::EvidenceAuthority,
}

struct Pay2SendResult {
    executed: layerx_sdk::rpc_verification::VerifiedRpcReceipt,
    batched: layerx_sdk::rpc_verification::VerifiedRpcReceipt,
    identity_sequence: u64,
    source_sequence: u64,
}

struct Pay2PriorGrant {
    issue: layerx_sdk::rpc_verification::VerifiedRpcReceipt,
    draw: layerx_sdk::rpc_verification::VerifiedRpcReceipt,
}

impl Pay2FundedRun<'_> {
    fn qualify_send(&self) -> Pay2SendResult {
        let payer = layerx_platform_core::main_account(&self.cluster.treasury_did)
            .required("PAY2 payer account");
        let recipient = layerx_platform_core::main_account(&self.funding.recipient_did)
            .required("PAY2 recipient account");
        let idempotency = random32();
        let amount = 3_u128;
        let source_sequence = pay2_account_sequence(self.payer_rpc, payer);
        let send = layerx_crypto::send::SendDebit {
            from: payer,
            to: recipient,
            asset: self.cluster.asset,
            amount,
            source_sequence,
            idempotency_key: idempotency,
            expires_at: now_ms().saturating_add(60_000),
            context_hash: layerx_platform_core::send_context_hash(
                &payer,
                &recipient,
                &self.cluster.asset,
                amount,
                &idempotency,
            ),
            conditions: Vec::new(),
            authorization_kind: 1,
            network_id: NETWORK_ID,
            protocol_version: PROTOCOL_VERSION,
        };
        let disclosed = send
            .authorization_message()
            .required("PAY2 Send disclosure bytes");
        assert!(matches!(
            send.signing_request(&disclosed)
                .required("PAY2 disclosed Send signing request")
                .disclosure(),
            layerx_crypto::signer::SigningDisclosure::SendDebit(found) if found == &send
        ));
        let options = pay2_options(
            &self.cluster.treasury_did,
            idempotency,
            layerx_sdk::rpc::Commitment::Executed,
        );
        let identity_sequence = self
            .payer_rpc
            .get_identity_sequence(&self.cluster.treasury_did)
            .required("PAY2 Send identity sequence")
            .next_sequence;
        let executed =
            pay2_run(self.payer_wallet.send_debit(&send, &options)).required("PAY2 executed Send");
        let protocol = executed
            .receipt()
            .protocol()
            .required("PAY2 executed Send receipt");
        assert_eq!(protocol.result_code(), 0);
        let (registry, _) = layerx_platform_core::asset_registry().required("PAY2 Asset registry");
        let activity = layerx_wire::activity::decode_signed(
            executed
                .canonical_activity()
                .required("PAY2 signed Send activity"),
            &registry,
        )
        .required("PAY2 canonical Send");
        assert_eq!(activity.account_sequence(), identity_sequence);
        assert_ne!(activity.account_sequence(), source_sequence);
        let batched = self
            .payer_wallet
            .wait_for(
                protocol.activity_id(),
                layerx_sdk::rpc::Commitment::Batched,
                Duration::from_secs(60),
            )
            .required("PAY2 batched Send");
        assert_eq!(batched.canonical_bytes(), executed.canonical_bytes());
        assert!(batched.batch_evidence().is_some());
        Pay2SendResult {
            executed,
            batched,
            identity_sequence,
            source_sequence,
        }
    }

    fn qualify_prior_grant(&self) -> Pay2PriorGrant {
        let grant = funding::payer_grant(&funding::GrantRequest {
            payer_seed: &self.cluster.treasury_seed,
            payer_did: &self.cluster.treasury_did,
            recipient_did: &self.funding.recipient_did,
            asset: self.cluster.asset,
            per_draw_maximum: 5,
            allowance: 10,
            recurring_window: None,
            expiration: now_ms().saturating_add(300_000),
            purpose_hash: random32(),
            revocation_sequence: self
                .payer_rpc
                .get_identity_sequence(&self.cluster.treasury_did)
                .required("PAY2 first grant sequence")
                .next_sequence,
        })
        .required("PAY2 first grant");
        let issue_options = pay2_options(
            &self.cluster.treasury_did,
            random32(),
            layerx_sdk::rpc::Commitment::Batched,
        );
        let issue = pay2_run(self.payer_wallet.issue(grant.clone(), &issue_options))
            .required("PAY2 SDK grant issue");
        assert_eq!(
            issue
                .receipt()
                .protocol()
                .required("PAY2 first grant receipt")
                .result_code(),
            0
        );
        let draw_key = random32();
        let recipient = layerx_platform_core::main_account(&self.funding.recipient_did)
            .required("PAY2 recipient account");
        let payment = funding::receive(
            &self.funding.recipient_seed,
            &grant,
            pay2_account_sequence(self.recipient_rpc, recipient),
            draw_key,
            2,
        )
        .required("PAY2 first Receive");
        let draw_options = pay2_options(
            &self.funding.recipient_did,
            draw_key,
            layerx_sdk::rpc::Commitment::Batched,
        );
        let draw = pay2_run(self.recipient_wallet.draw(payment, &draw_options))
            .required("PAY2 SDK grant draw");
        let protocol = draw
            .receipt()
            .protocol()
            .required("PAY2 first draw receipt");
        assert_eq!(protocol.result_code(), 0);
        assert_eq!(protocol.operation(), 6);
        assert_eq!(protocol.amount(), 2);
        Pay2PriorGrant { issue, draw }
    }

    fn next_grant(&self) -> layerx_crypto::payments::Grant {
        funding::payer_grant(&funding::GrantRequest {
            payer_seed: &self.cluster.treasury_seed,
            payer_did: &self.cluster.treasury_did,
            recipient_did: &self.funding.recipient_did,
            asset: self.cluster.asset,
            per_draw_maximum: 5,
            allowance: 10,
            recurring_window: None,
            expiration: now_ms().saturating_add(300_000),
            purpose_hash: random32(),
            revocation_sequence: self
                .payer_rpc
                .get_identity_sequence(&self.cluster.treasury_did)
                .required("PAY2 MCP grant sequence")
                .next_sequence,
        })
        .required("PAY2 MCP grant")
    }

    fn next_draw(
        &self,
        grant: &layerx_crypto::payments::Grant,
    ) -> (Vec<u8>, layerx_sdk::wallet::PaymentOptions) {
        let recipient = layerx_platform_core::main_account(&self.funding.recipient_did)
            .required("PAY2 recipient account");
        let idempotency = random32();
        let payment = funding::receive(
            &self.funding.recipient_seed,
            grant,
            pay2_account_sequence(self.recipient_rpc, recipient),
            idempotency,
            2,
        )
        .required("PAY2 MCP Receive");
        let payload = payment
            .encode(self.funding.recipient_did.as_bytes())
            .required("PAY2 MCP Receive payload");
        let options = pay2_options(
            &self.funding.recipient_did,
            idempotency,
            layerx_sdk::rpc::Commitment::Batched,
        );
        (payload, options)
    }

    fn execute_mcp_issue(
        &self,
        prior_draw: &layerx_sdk::rpc_verification::VerifiedRpcReceipt,
    ) -> (
        layerx_crypto::payments::Grant,
        layerx_sdk::rpc_verification::VerifiedRpcReceipt,
    ) {
        let prior_header = pay2_batch_header(prior_draw);
        let sequence = prior_header
            .last_sequence()
            .checked_add(1)
            .required("PAY2 issue sequence");
        let entry = layerx_mcp::tools::wallet::cumulative_evidence(prior_draw)
            .required("PAY2 first draw cumulative ingress");
        let did = layerx_types::ids::Did::new(self.cluster.treasury_did.as_bytes())
            .required("PAY2 payer DID");
        let usage = self
            .evidence_authority
            .authenticate_cumulative_use(
                &did,
                layerx_agentd::protocol_evidence::CumulativeUseWindow {
                    first: prior_header.first_sequence(),
                    last: prior_header.last_sequence(),
                },
                &[entry],
            )
            .unwrap_or_else(|error| panic!("PAY2 authenticated issue usage: {error:?}"));
        let grant = self.next_grant();
        let payload = layerx_crypto::payments::Payment::IssueGrant(grant.clone())
            .encode(self.cluster.treasury_did.as_bytes())
            .required("PAY2 MCP grant payload");
        let options = pay2_options(
            &self.cluster.treasury_did,
            random32(),
            layerx_sdk::rpc::Commitment::Batched,
        );
        let recipient = layerx_platform_core::main_account(&self.funding.recipient_did)
            .required("PAY2 recipient account");
        let mut context = pay2_mcp_context(Pay2McpSpec {
            root: &self.cluster.root,
            rpc: self.payer_rpc,
            actor: &self.cluster.treasury_did,
            ordinal: 7,
            counterparty: recipient,
            asset: self.cluster.asset,
            amount_ceiling: 10,
            window_sequences: prior_header
                .last_sequence()
                .checked_sub(prior_header.first_sequence())
                .and_then(|value| value.checked_add(1))
                .required("PAY2 issue window length"),
            current_sequence: sequence,
            scope: "write:grant:issue",
            purpose: "grant.issue",
        });
        let request = layerx_agentd::policy::PolicyRequest {
            activity_type: 7,
            counterparty: recipient,
            asset: self.cluster.asset,
            amount: grant.allowance,
            purpose: "grant.issue".to_owned(),
            core_sequence: sequence,
        };
        let policy = pay2_policy(&context, &request);
        let input = layerx_agentd::policy::EvaluationInput::with_authenticated_cumulative_use(
            &request,
            &context.session,
            &context.capability,
            &usage,
        );
        let execution = layerx_mcp::tools::wallet::PaymentExecution {
            wallet: self.payer_wallet,
            options: &options,
            canonical_payload: &payload,
            policy: &policy,
            evaluation: &input,
        };
        let receipt = layerx_mcp::tools::wallet::execute(
            &mut context.server,
            sequence,
            layerx_mcp::tools::write::PaymentTool::IssueGrant,
            &execution,
        )
        .required("PAY2 MCP grant issue");
        let protocol = receipt
            .receipt()
            .protocol()
            .required("PAY2 MCP issue receipt");
        assert_eq!(protocol.result_code(), 0);
        assert_eq!(protocol.operation(), 7);
        assert_eq!(protocol.global_sequence(), sequence);
        (grant, receipt)
    }

    fn execute_mcp_draw(
        &self,
        prior_draw: &layerx_sdk::rpc_verification::VerifiedRpcReceipt,
        grant: &layerx_crypto::payments::Grant,
        issue: &layerx_sdk::rpc_verification::VerifiedRpcReceipt,
    ) -> layerx_sdk::rpc_verification::VerifiedRpcReceipt {
        let prior_header = pay2_batch_header(prior_draw);
        let issue_header = pay2_batch_header(issue);
        let sequence = issue_header
            .last_sequence()
            .checked_add(1)
            .required("PAY2 MCP draw sequence");
        let entries = [
            layerx_mcp::tools::wallet::cumulative_evidence(prior_draw)
                .required("PAY2 draw history ingress"),
            layerx_mcp::tools::wallet::cumulative_evidence(issue)
                .required("PAY2 issue history ingress"),
        ];
        let did = layerx_types::ids::Did::new(self.funding.recipient_did.as_bytes())
            .required("PAY2 recipient DID");
        assert!(matches!(
            self.evidence_authority.authenticate_cumulative_use(
                &did,
                layerx_agentd::protocol_evidence::CumulativeUseWindow {
                    first: prior_header.first_sequence(),
                    last: issue_header.last_sequence(),
                },
                &entries[..1],
            ),
            Err(layerx_agentd::protocol_evidence::CumulativeUseError::IncompleteWindow)
        ));
        let usage = self
            .evidence_authority
            .authenticate_cumulative_use(
                &did,
                layerx_agentd::protocol_evidence::CumulativeUseWindow {
                    first: prior_header.first_sequence(),
                    last: issue_header.last_sequence(),
                },
                &entries,
            )
            .unwrap_or_else(|error| panic!("PAY2 authenticated draw usage: {error:?}"));
        let payer = layerx_platform_core::main_account(&self.cluster.treasury_did)
            .required("PAY2 payer account");
        let (payload, options) = self.next_draw(grant);
        let mut context = pay2_mcp_context(Pay2McpSpec {
            root: &self.cluster.root,
            rpc: self.recipient_rpc,
            actor: &self.funding.recipient_did,
            ordinal: 6,
            counterparty: payer,
            asset: self.cluster.asset,
            amount_ceiling: 5,
            window_sequences: sequence
                .checked_sub(prior_header.first_sequence())
                .required("PAY2 draw window length"),
            current_sequence: sequence,
            scope: "write:grant:draw",
            purpose: "grant.draw",
        });
        let request = layerx_agentd::policy::PolicyRequest {
            activity_type: 6,
            counterparty: payer,
            asset: self.cluster.asset,
            amount: 2,
            purpose: "grant.draw".to_owned(),
            core_sequence: sequence,
        };
        let policy = pay2_policy(&context, &request);
        let input = layerx_agentd::policy::EvaluationInput::with_authenticated_cumulative_use(
            &request,
            &context.session,
            &context.capability,
            &usage,
        );
        let execution = layerx_mcp::tools::wallet::PaymentExecution {
            wallet: self.recipient_wallet,
            options: &options,
            canonical_payload: &payload,
            policy: &policy,
            evaluation: &input,
        };
        let receipt = layerx_mcp::tools::wallet::execute(
            &mut context.server,
            sequence,
            layerx_mcp::tools::write::PaymentTool::DrawGrant,
            &execution,
        )
        .required("PAY2 MCP grant draw");
        let protocol = receipt
            .receipt()
            .protocol()
            .required("PAY2 MCP draw receipt");
        assert_eq!(protocol.result_code(), 0);
        assert_eq!(protocol.operation(), 6);
        assert_eq!(protocol.amount(), 2);
        assert_eq!(protocol.global_sequence(), sequence);
        receipt
    }

    fn qualify_exported_mcp_paths(
        &self,
        receipt: &layerx_sdk::rpc_verification::VerifiedRpcReceipt,
    ) {
        let protocol = receipt
            .receipt()
            .protocol()
            .required("PAY2 exported MCP receipt");
        let sequence = protocol
            .global_sequence()
            .checked_add(1)
            .required("PAY2 exported MCP sequence");
        let payer = layerx_platform_core::main_account(&self.cluster.treasury_did)
            .required("PAY2 payer account");
        let mut execute_context = pay2_mcp_context(Pay2McpSpec {
            root: &self.cluster.root,
            rpc: self.recipient_rpc,
            actor: &self.funding.recipient_did,
            ordinal: 6,
            counterparty: payer,
            asset: self.cluster.asset,
            amount_ceiling: 5,
            window_sequences: 1,
            current_sequence: sequence,
            scope: "write:grant:draw",
            purpose: "grant.draw",
        });
        let executed = layerx_mcp::tools::write::execute_payment(
            &mut execute_context.server,
            sequence,
            layerx_mcp::tools::write::PaymentTool::DrawGrant,
            protocol.activity_id().to_vec(),
            |_| pay2_write_transcript(receipt, false),
            0,
        )
        .required("PAY2 exported execute_payment");
        assert!(matches!(
            executed,
            layerx_mcp::tools::write::WriteOutcome::Executed { .. }
        ));
        let wait_sequence = sequence.checked_add(1).required("PAY2 wait sequence");
        let mut wait_context = pay2_mcp_context(Pay2McpSpec {
            root: &self.cluster.root,
            rpc: self.recipient_rpc,
            actor: &self.funding.recipient_did,
            ordinal: 6,
            counterparty: payer,
            asset: self.cluster.asset,
            amount_ceiling: 5,
            window_sequences: 1,
            current_sequence: wait_sequence,
            scope: "write:activity:wait",
            purpose: "activity.wait",
        });
        let waited = layerx_mcp::tools::write::wait(
            &mut wait_context.server,
            wait_sequence,
            protocol.activity_id().to_vec(),
            |_| pay2_write_transcript(receipt, true),
            0,
        )
        .required("PAY2 exported activity.wait");
        assert!(matches!(
            waited,
            layerx_mcp::tools::write::WriteOutcome::Executed { .. }
        ));
    }
}

fn pay2_finish(run: &Pay2FundedRun<'_>) {
    let send = run.qualify_send();
    let prior = run.qualify_prior_grant();
    let (grant, mcp_issue) = run.execute_mcp_issue(&prior.draw);
    let mcp_draw = run.execute_mcp_draw(&prior.draw, &grant, &mcp_issue);
    run.qualify_exported_mcp_paths(&mcp_draw);
    let prior_sequence = prior
        .draw
        .receipt()
        .protocol()
        .required("PAY2 prior draw receipt")
        .global_sequence();
    let prior_header = pay2_batch_header(&prior.draw);
    let issue_sequence = mcp_issue
        .receipt()
        .protocol()
        .required("PAY2 issue receipt")
        .global_sequence();
    let issue_header = pay2_batch_header(&mcp_issue);
    let draw_sequence = mcp_draw
        .receipt()
        .protocol()
        .required("PAY2 draw receipt")
        .global_sequence();
    let evidence = serde_json::json!({
        "send_executed": pay2_receipt_record(&send.executed),
        "send_batched": pay2_receipt_record(&send.batched),
        "sdk_grant_issue": pay2_receipt_record(&prior.issue),
        "sdk_grant_draw": pay2_receipt_record(&prior.draw),
        "mcp_grant_issue": pay2_receipt_record(&mcp_issue),
        "mcp_grant_draw": pay2_receipt_record(&mcp_draw),
        "cumulative_draw_window": {
            "first": prior_header.first_sequence(),
            "last": issue_header.last_sequence(),
            "next": draw_sequence,
            "activity_sequences": [prior_sequence, issue_sequence],
            "batch_numbers": [prior_header.batch_number(), issue_header.batch_number()]
        },
        "send_sequence_domains": {
            "identity": send.identity_sequence,
            "source_account": send.source_sequence
        }
    });
    let output =
        PathBuf::from(std::env::var_os("PAY2_EVIDENCE_OUT").required("PAY2 evidence output path"));
    fs::write(
        &output,
        serde_json::to_vec_pretty(&evidence).required("PAY2 evidence JSON"),
    )
    .required("PAY2 evidence output");
    println!(
        "pay2_funded_complete send={} mcp_issue={} mcp_draw={} evidence={}",
        evidence["send_executed"]["activity_id"],
        evidence["mcp_grant_issue"]["activity_id"],
        evidence["mcp_grant_draw"]["activity_id"],
        output.display()
    );
}

#[test]
fn pay2e_funded_sdk_send_and_mcp_grant_draw() {
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
    let endpoint = format!("https://localhost:{}", gateway.port);
    let payer_rpc = layerx_sdk::rpc::RpcClient::connect_with_ca_der(
        &endpoint,
        Some(pay2_credential(&payer_key)),
        &certificates.ca_der,
    )
    .required("PAY2 payer RPC");
    let recipient_rpc = layerx_sdk::rpc::RpcClient::connect_with_ca_der(
        &endpoint,
        Some(pay2_credential(&recipient_key)),
        &certificates.ca_der,
    )
    .required("PAY2 recipient RPC");
    let receipt_policy = layerx_sdk::rpc_verification::ReceiptPolicy {
        protocol_version: PROTOCOL_VERSION,
        network_id: NETWORK_ID,
        sequencer: layerx_proof::inclusion::SequencerAuthorization::new(
            cluster.sequencer_id,
            cluster.sequencer_key,
            1,
            u64::MAX,
        ),
        trusted_checkpoint_context_digest: None,
    };
    let payer_signer = layerx_crypto::local::LocalSigner::new(cluster.treasury_seed);
    let recipient_signer = layerx_crypto::local::LocalSigner::new(funding.recipient_seed);
    let payer_wallet = layerx_sdk::wallet::Wallet {
        rpc: &payer_rpc,
        signer: &payer_signer,
        policy: &receipt_policy,
        native_asset: cluster.asset,
    };
    let recipient_wallet = layerx_sdk::wallet::Wallet {
        rpc: &recipient_rpc,
        signer: &recipient_signer,
        policy: &receipt_policy,
        native_asset: cluster.asset,
    };
    pay2_finish(&Pay2FundedRun {
        cluster: &cluster,
        funding: &funding,
        payer_rpc: &payer_rpc,
        recipient_rpc: &recipient_rpc,
        payer_wallet: &payer_wallet,
        recipient_wallet: &recipient_wallet,
        evidence_authority: pay2_evidence_authority(&cluster),
    });
}
