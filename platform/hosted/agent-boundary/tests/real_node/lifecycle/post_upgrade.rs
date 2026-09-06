use super::*;

#[test]
fn calls_execute_before_and_after_upgrade_then_exit() {
    let fixture: serde_json::Value = must(
        serde_json::from_slice(&must(
            fs::read(
                repository_root()
                    .join("platform/sdk/conformance/fixtures/native-program-deploy-v3.json"),
            ),
            "C deploy fixture",
        )),
        "C fixture JSON",
    );
    let payload = unhex(field(&fixture, "payload_hex"));
    let original = must(NativeProgramDeploy::decode(&payload), "C deploy payload");
    let wasm = original.wasm;
    let mut upgraded = wasm.to_vec();
    upgraded.extend_from_slice(&[0, 2, 1, b'u']);
    assert_ne!(Sha256::digest(wasm), Sha256::digest(&upgraded));
    let (cluster, custody) = custody::start_funded_cluster();
    custody.verify_evidence();
    check_readiness(&cluster);
    let program = random32();
    let account = derived_account(program);
    let call = must(
        NativeProgramCall {
            program_id: ProgramId::new(program),
            guest_abi: 2,
            entrypoint: b"layerx_call",
            calldata: &[],
            capabilities: &[0, 0],
            access_declaration: b"LayerX/programs/access-declaration/v1\0\0",
            response_capacity: 16,
            resources: escrow_resources(),
        }
        .encode(),
        "echo call",
    );
    let operations = [
        (
            1,
            "/v1/programs/deploy",
            deploy_payload(&cluster, program, wasm),
        ),
        (3, "/v1/programs/call", call.clone()),
        (
            2,
            "/v1/programs/upgrade",
            upgrade_payload(program, wasm, &upgraded),
        ),
        (3, "/v1/programs/call", call),
        (
            6,
            "/v1/activities",
            escrow_account_registration(program, cluster.asset),
        ),
        (
            7,
            "/v1/programs/wind-down",
            wind_down_payload(
                program,
                ProgramWindDownOperation::Route {
                    account,
                    asset: cluster.asset,
                    destination: cluster.actor.source,
                    seed: SEED,
                },
            ),
        ),
        (
            7,
            "/v1/programs/wind-down",
            wind_down_payload(
                program,
                ProgramWindDownOperation::Deprecate {
                    exit_program: program,
                    deadline_batch: LAST_BATCH,
                },
            ),
        ),
        (
            7,
            "/v1/programs/wind-down",
            wind_down_payload(program, ProgramWindDownOperation::Exit { account }),
        ),
    ];
    for (index, (ordinal, path, payload)) in operations.iter().enumerate() {
        let sequence = must(u64::try_from(index + 2), "sequence after custody credit");
        let signed =
            signed_program_operation(&cluster.actor, *ordinal, sequence, FEE_LIMIT, payload);
        let key = format!("upgraded-call-{index}-{}", token());
        let submitted = submit_lifecycle(&cluster, path, &signed, *ordinal, &key);
        check_idempotency_lookup(&cluster, &signed, &submitted);
        let replay =
            cluster
                .client
                .call(&Call::submit(path, &cluster.gateway_token, &key, &signed));
        assert_eq!(replay.status, 200, "{}", replay.text());
        assert_eq!(replay.text(), submitted.body);
        assert_eq!(journal_record(&cluster, &key)["attempts"], 1);
    }
}
