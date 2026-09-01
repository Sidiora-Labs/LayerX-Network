use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::ed25519;
use layerx_programs_runtime::terminal::{
    decode_terminal_payload, CandidateTerminalOutcome, ExecutionTerminal, FailureTerminal,
    TerminalAttachment, TerminalDetail,
};
use layerx_programs_runtime::{
    BudgetMeterRefusal, BudgetResourceKind, OccupancySettlement, WasmEngine,
};
use layerx_proof::receipt::verify_program_outcome_at_root;
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::intent::{
    CallBudget, Calldata, CapabilityRequest, ProgramCall, ProgramCallError, ProgramId,
    RequestedCapabilities, PROGRAM_CALL_CONTRACT_MAJOR,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_wire::activity::{decode_signed, encode_signed_envelope};
use layerx_wire::hash::{activity_id, payload_hash_for};
use layerx_wire::sign::preimage_unsigned;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use crate::encoding::{fixed_hex, hex_decode, hex_encode};
use crate::http::{validate_idempotency_key, validate_resource_id, Client};

const DESCRIPTOR: &str = "layerx-program.json";
const SIMULATION_EVIDENCE_DOMAIN: &[u8] = b"LayerX/agent/program-simulation-evidence/v1\0";
const EMULATOR_BOUNDARY_DOMAIN: &[u8] = b"LayerX/emulator/simulation-boundary/v1\0";

pub fn program_bindings(
    interface_path: &Path,
    expected_digest: &str,
    expected_code_hash: &str,
    output: &Path,
) -> Result<Value, String> {
    let interface_path = interface_path.canonicalize().map_err(|error| {
        format!(
            "could not resolve published interface {}: {error}",
            interface_path.display()
        )
    })?;
    let interface = fs::read(&interface_path).map_err(|error| {
        format!(
            "could not read published interface {}: {error}",
            interface_path.display()
        )
    })?;
    let digest: [u8; 32] = fixed_hex("published interface digest", expected_digest)?;
    let code_hash: [u8; 32] = fixed_hex("deployed program code hash", expected_code_hash)?;
    let generator = layerx_program_sdk::BindingGenerator::from_interface(&interface)
        .map_err(|error| format!("published interface is not canonical: {error}"))?;
    generator
        .require_digest(digest)
        .map_err(|error| format!("published interface digest is stale: {error}"))?;
    generator
        .require_code_hash(code_hash)
        .map_err(|error| format!("published interface is bound to different code: {error}"))?;

    fs::create_dir_all(output).map_err(|error| {
        format!(
            "could not create binding directory {}: {error}",
            output.display()
        )
    })?;
    let rust = generator.generate_rust();
    let typescript = generator.generate_typescript();
    let guest = generator.generate_guest();
    write_binding(output, "client.rs", rust.as_bytes())?;
    write_binding(output, "client.ts", typescript.as_bytes())?;
    write_binding(output, "guest.rs", guest.as_bytes())?;
    let manifest = serde_json::to_vec_pretty(&json!({
        "source": interface_path.display().to_string(),
        "interface_digest": hex_encode(&digest),
        "code_hash": hex_encode(&code_hash),
        "artifacts": ["client.rs", "client.ts", "guest.rs"],
    }))
    .map_err(|error| format!("could not encode binding manifest: {error}"))?;
    write_binding(output, "bindings.json", &manifest)?;
    Ok(json!({
        "output": output.display().to_string(),
        "interface_digest": hex_encode(&digest),
        "code_hash": hex_encode(&code_hash),
        "artifacts": ["client.rs", "client.ts", "guest.rs", "bindings.json"],
        "binding": "receipt-verified digest and deployed code hash required before generation and at generated call time",
    }))
}

fn write_binding(directory: &Path, name: &str, contents: &[u8]) -> Result<(), String> {
    let destination = directory.join(name);
    let temporary = directory.join(format!(".{name}.{}.tmp", std::process::id()));
    fs::write(&temporary, contents)
        .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &destination).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("could not publish {}: {error}", destination.display())
    })
}

struct Step {
    command: String,
    args: Vec<String>,
}

struct Toolchain {
    project: PathBuf,
    language: String,
    build: Step,
    artifact: PathBuf,
    lint: Option<Step>,
}

pub fn build(manifest: &Path, artifact: Option<&Path>) -> Result<Value, String> {
    let manifest = manifest
        .canonicalize()
        .map_err(|error| format!("could not resolve {}: {error}", manifest.display()))?;
    let project = if manifest.is_dir() {
        manifest.clone()
    } else {
        manifest
            .parent()
            .ok_or_else(|| "program manifest has no parent directory".to_string())?
            .to_path_buf()
    };
    match load_toolchain(&project)? {
        Some(toolchain) => build_with_toolchain(&toolchain, artifact),
        None => build_with_cargo(&manifest, &project, artifact),
    }
}

fn build_with_toolchain(toolchain: &Toolchain, artifact: Option<&Path>) -> Result<Value, String> {
    run_step(
        &toolchain.project,
        &toolchain.build,
        &format!("{} program toolchain", toolchain.language),
    )?;
    let artifact = match artifact {
        Some(path) => resolve(&toolchain.project, path),
        None => resolve(&toolchain.project, &toolchain.artifact),
    };
    if !artifact.exists() {
        return Err(format!(
            "the {} program toolchain produced no artifact at {}",
            toolchain.language,
            artifact.display()
        ));
    }
    let determinism_lint = match &toolchain.lint {
        Some(lint) => {
            run_step(
                &toolchain.project,
                lint,
                &format!("{} determinism lint", toolchain.language),
            )?;
            "passed"
        }
        None => "not declared by the toolchain descriptor",
    };
    let mut inspected = inspect_artifact(&artifact)?;
    if let Some(object) = inspected.as_object_mut() {
        object.insert("language".into(), json!(toolchain.language));
        object.insert("determinism_lint".into(), json!(determinism_lint));
    }
    Ok(inspected)
}

fn build_with_cargo(
    manifest: &Path,
    project: &Path,
    artifact: Option<&Path>,
) -> Result<Value, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(project)
        .args([
            "build",
            "--manifest-path",
            manifest
                .to_str()
                .ok_or_else(|| "program manifest path is not valid UTF-8".to_string())?,
            "--target",
            "wasm32-unknown-unknown",
            "--release",
        ])
        .status()
        .map_err(|error| format!("could not start the Rust program toolchain: {error}"))?;
    if !status.success() {
        return Err(format!("Rust program toolchain failed with {status}"));
    }
    let artifact = match artifact {
        Some(path) => resolve(project, path),
        None => discover_artifact(project)?,
    };
    let mut inspected = inspect_artifact(&artifact)?;
    if let Some(object) = inspected.as_object_mut() {
        object.insert("language".into(), json!("rust"));
        object.insert(
            "determinism_lint".into(),
            json!(format!(
                "not run; the project declares no {DESCRIPTOR} toolchain descriptor"
            )),
        );
    }
    Ok(inspected)
}

pub fn inspect_artifact(path: &Path) -> Result<Value, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("could not resolve {}: {error}", path.display()))?;
    let wasm =
        fs::read(&path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let engine = WasmEngine::declared()
        .map_err(|error| format!("could not initialize deterministic WASM engine: {error}"))?;
    let validated = engine
        .validate(&wasm)
        .map_err(|error| format!("program violates the deterministic WASM policy: {error}"))?;
    let code_hash: [u8; 32] = Sha256::digest(&wasm).into();
    Ok(json!({
        "artifact": path.display().to_string(),
        "code_hash": hex_encode(&code_hash),
        "byte_size": validated.byte_size(),
        "function_count": validated.function_count(),
        "abi_version": 1,
        "deterministic_validation": "passed",
    }))
}

pub fn deploy(
    client: &Client,
    path: &Path,
    upgrade_authority: Option<&str>,
    source_uri: Option<&str>,
    idempotency_key: &str,
) -> Result<Value, String> {
    validate_idempotency_key(idempotency_key)?;
    let mut inspected = inspect_artifact(path)?;
    let gate = gate_artifact(path)?;
    if let Some(object) = inspected.as_object_mut() {
        object.insert("language".into(), json!(gate.0));
        object.insert("determinism_lint".into(), json!(gate.1));
    }
    let wasm =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let policy = match upgrade_authority {
        Some(authority) => json!({"kind": "upgradeable", "authority": authority}),
        None => json!({"kind": "immutable"}),
    };
    let request = json!({
        "abi_version": 1,
        "code_hash": inspected["code_hash"],
        "wasm_hex": hex_encode(&wasm),
        "upgrade_policy": policy,
        "source_uri": source_uri,
    });
    let response = client.post("/v1/programs/deploy", &request, Some(idempotency_key))?;
    Ok(json!({
        "artifact": inspected,
        "deployment": response,
        "verification": "server outcome; verify the returned receipt before treating the program as callable",
    }))
}

pub fn registry_get(client: &Client, program_id: &str) -> Result<Value, String> {
    validate_resource_id(program_id, "program id")?;
    let response = client.get(&format!("/v1/programs/registry/{program_id}"))?;
    if response["program_id"]
        .as_str()
        .is_none_or(|value| !value.eq_ignore_ascii_case(program_id))
    {
        return Err("registry response changed the requested program identity".to_owned());
    }
    let Some(value_accounts) = response["value_accounts"].as_object() else {
        return Err("registry response omitted receipt-proven program balances".to_owned());
    };
    match value_accounts.get("status").and_then(Value::as_str) {
        Some("current") => {
            let Some(accounts) = value_accounts.get("accounts").and_then(Value::as_array) else {
                return Err("registry response has no canonical program account list".to_owned());
            };
            for account in accounts {
                let Some(account_id) = account["account_id"].as_str() else {
                    return Err("program balance omitted its account id".to_owned());
                };
                let Some(asset_id) = account["asset_id"].as_str() else {
                    return Err("program balance omitted its asset id".to_owned());
                };
                let _: [u8; 32] = crate::encoding::fixed_hex("program account", account_id)?;
                let _: [u8; 32] = crate::encoding::fixed_hex("program account asset", asset_id)?;
                if account["balance"]
                    .as_str()
                    .and_then(|balance| balance.parse::<u128>().ok())
                    .is_none()
                    || account["frozen"].as_bool().is_none()
                {
                    return Err("program balance is not a canonical amount record".to_owned());
                }
            }
            let receipt = &value_accounts["receipt"];
            let Some(receipt_digest) = receipt["receipt_digest"].as_str() else {
                return Err("program balances omitted their receipt digest".to_owned());
            };
            let Some(state_root) = receipt["state_root"].as_str() else {
                return Err("program balances omitted their state root".to_owned());
            };
            let receipt_digest: [u8; 32] =
                crate::encoding::fixed_hex("program balance receipt", receipt_digest)?;
            let state_root: [u8; 32] =
                crate::encoding::fixed_hex("program balance state root", state_root)?;
            if receipt_digest == [0; 32] || state_root == [0; 32] {
                return Err("program balance proof contains a reserved zero root".to_owned());
            }
            if receipt["observed_sequence"]
                .as_u64()
                .filter(|value| *value != 0)
                .is_none()
                || receipt["observed_at"]
                    .as_u64()
                    .filter(|value| *value != 0)
                    .is_none()
                || receipt["verification"].as_str()
                    != Some("account-primary-and-state-proof-verified")
            {
                return Err("program balance freshness is absent or unverifiable".to_owned());
            }
        }
        Some("account-incapable-abi1")
            if value_accounts
                .get("accounts")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty) => {}
        _ => return Err("program balance status is absent or stale".to_owned()),
    }
    Ok(response)
}

pub fn registry_list(client: &Client) -> Result<Value, String> {
    let response = client.get("/v1/programs/registry")?;
    let result = response.get("result").unwrap_or(&response);
    let programs = result
        .get("program_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "program registry omitted its canonical program identifier list".to_owned()
        })?;
    if programs.is_empty()
        || programs.iter().any(|value| {
            value
                .as_str()
                .is_none_or(|id| fixed_hex::<32>("program id", id).is_err())
        })
    {
        return Err(
            "program registry returned no discoverable program or a malformed identifier"
                .to_owned(),
        );
    }
    Ok(json!({"program_ids":programs}))
}

pub fn discover(client: &Client, program_id: &str) -> Result<Value, String> {
    validate_resource_id(program_id, "program id")?;
    let response = client.get(&format!("/v1/programs/registry/{program_id}"))?;
    let value = response.get("result").unwrap_or(&response).clone();
    if value["program_id"]
        .as_str()
        .is_none_or(|value| !value.eq_ignore_ascii_case(program_id))
    {
        return Err("program discovery changed the requested program identity".to_owned());
    }
    if value["lifecycle"].as_str() != Some("active") {
        return Err("program discovery refused an inactive program".to_owned());
    }
    if value["observed_sequence"].as_u64().is_none() || value["state_root"].as_str().is_none() {
        return Err("program discovery omitted its current-state freshness".to_owned());
    }
    Ok(value)
}

pub fn interface_get(client: &Client, program_id: &str) -> Result<Value, String> {
    validate_resource_id(program_id, "program id")?;
    let response = client.get(&format!("/v1/programs/registry/{program_id}/interface"))?;
    let value = response.get("result").unwrap_or(&response).clone();
    let encoded = value["interface"]
        .as_str()
        .ok_or_else(|| "interface read omitted canonical bytes".to_owned())?;
    let bytes = hex_decode("program interface", encoded)?;
    let interface = layerx_programs::ProgramInterface::decode(&bytes)
        .map_err(|error| format!("program interface is not canonical: {error}"))?;
    let digest = value["interface_digest"]
        .as_str()
        .ok_or_else(|| "interface read omitted its digest".to_owned())?;
    let expected: [u8; 32] = fixed_hex("interface digest", digest)?;
    let code_hash: [u8; 32] = fixed_hex(
        "interface code hash",
        value["code_hash"]
            .as_str()
            .ok_or_else(|| "interface read omitted its code hash".to_owned())?,
    )?;
    if interface.digest().into_bytes() != expected || interface.code_hash() != code_hash {
        return Err("interface bytes disagree with their receipt-bound digest".to_owned());
    }
    if value["observed_sequence"].as_u64().is_none() || value["state_root"].as_str().is_none() {
        return Err("interface read omitted current-state freshness".to_owned());
    }
    Ok(value)
}

pub fn interface_publish(
    client: &Client,
    program_id: &str,
    interface_path: &Path,
    idempotency_key: &str,
) -> Result<Value, String> {
    validate_resource_id(program_id, "program id")?;
    validate_idempotency_key(idempotency_key)?;
    let bytes = fs::read(interface_path)
        .map_err(|error| format!("could not read {}: {error}", interface_path.display()))?;
    let interface = layerx_programs::ProgramInterface::decode(&bytes)
        .map_err(|error| format!("program interface is not canonical: {error}"))?;
    client.post(
        &format!("/v1/programs/registry/{program_id}/interface"),
        &json!({
            "interface": hex_encode(&bytes),
            "interface_digest": hex_encode(interface.digest().as_bytes()),
            "code_hash": hex_encode(&interface.code_hash()),
            "abi_version": interface.abi_version(),
        }),
        Some(idempotency_key),
    )
}

pub fn registry_verify_source(
    client: &Client,
    program_id: &str,
    source_uri: &str,
    source_digest: &str,
    idempotency_key: &str,
) -> Result<Value, String> {
    validate_resource_id(program_id, "program id")?;
    validate_idempotency_key(idempotency_key)?;
    let _: [u8; 32] = crate::encoding::fixed_hex("source digest", source_digest)?;
    client.post(
        &format!("/v1/programs/registry/{program_id}/source"),
        &json!({
            "source_uri": source_uri,
            "source_digest": source_digest,
        }),
        Some(idempotency_key),
    )
}

/// One parsed `layerx program call` invocation. A call is a money-adjacent
/// state change, so an idempotency key is mandatory and the returned receipt is
/// verified before any typed result is rendered.
pub struct CallRequest<'a> {
    pub program_id: &'a str,
    pub calldata: &'a str,
    pub fuel: u64,
    pub fee_limit: &'a str,
    pub capabilities: &'a [String],
    pub idempotency_key: &'a str,
    pub network_id: u32,
    pub actor_did: &'a str,
    pub key_name: &'a str,
    pub account_sequence: u64,
    pub not_before_ms: u64,
    pub expires_at_ms: u64,
    pub sequencer_public_key: &'a str,
}

struct VerifiedCallHead {
    sequencer_public_key: [u8; 32],
    state_root: [u8; 32],
    abi_version: u16,
    version: u32,
    code_hash: [u8; 32],
    observed_sequence: u64,
    observed_at: u64,
}

/// Submits one program call through the active endpoint and renders the typed
/// outcome only after re-binding it to the returned canonical receipt.
///
/// # Errors
///
/// Returns a typed error for an invalid identifier, malformed calldata, an
/// unbounded budget, an unknown capability, a rejected idempotency key, or a
/// response whose receipt does not back the typed outcome it reports.
pub fn call(client: &Client, request: &CallRequest<'_>) -> Result<Value, String> {
    validate_idempotency_key(request.idempotency_key)?;
    let operation = build_call(request)?;
    let payload = operation.canonical_payload();
    let signed = signed_call(request, &payload)?;
    let head = discover_call_head(client, request)?;
    let body = json!({ "activity": hex_encode(&signed) });
    let response = client.post_stateful("/v1/programs/call", &body, request.idempotency_key)?;
    if response.get("state").and_then(Value::as_str) == Some("unknown") {
        let registry = program_call_registry()?;
        let retained_activity = activity_id(
            &decode_signed(&signed, &registry)
                .map_err(|_| "retained signed call could not be decoded".to_owned())?,
        )
        .map_err(|_| "retained signed call has no canonical activity id".to_owned())?;
        return Ok(
            json!({"program_id":request.program_id,"idempotency_key":request.idempotency_key,
            "activity_id":hex_encode(&retained_activity),"outcome":{"status":"unknown","retained_bytes":true},"failure":response.get("failure")}),
        );
    }
    render_call_result(request, &payload, &signed, &head, &response)
}

pub fn simulate(client: &Client, request: &CallRequest<'_>) -> Result<Value, String> {
    let operation = build_call(request)?;
    let payload = operation.canonical_payload();
    let signed = signed_call(request, &payload)?;
    let head = discover_call_head(client, request)?;
    let body = json!({ "activity": hex_encode(&signed) });
    let response = client.post("/v1/programs/simulate", &body, None)?;
    let result = response.get("result").unwrap_or(&response);
    if result["committed"].as_bool() != Some(false) {
        return Err("program simulation did not prove that it committed nothing".to_owned());
    }
    verify_simulation_evidence(request, &signed, &head, result)?;
    let mut rendered = render_call_result(request, &payload, &signed, &head, &response)?;
    if let Some(object) = rendered.as_object_mut() {
        object.insert("committed".to_owned(), Value::Bool(false));
    }
    Ok(rendered)
}

fn signed_call(request: &CallRequest<'_>, canonical_payload: &[u8]) -> Result<Vec<u8>, String> {
    signed_call_with_signer(request, canonical_payload, || {
        let seed = crate::credential::key_seed(request.key_name)?;
        Ok(SigningKey::from_bytes(&seed))
    })
}

fn signed_call_with_signer(
    request: &CallRequest<'_>,
    canonical_payload: &[u8],
    signer: impl FnOnce() -> Result<SigningKey, String>,
) -> Result<Vec<u8>, String> {
    if request.expires_at_ms <= request.not_before_ms
        || request.expires_at_ms - request.not_before_ms > 300_000
    {
        return Err(
            "program call validity must be non-empty and no wider than 300000 milliseconds"
                .to_owned(),
        );
    }
    let idempotency = fixed_hex::<32>("idempotency key", request.idempotency_key)?;
    let activity_type = ActivityType::new(ModuleId::Programs, 3)
        .map_err(|error| format!("program call activity is unavailable: {error:?}"))?;
    let registry = program_call_registry()?;
    let payload = Payload::new(&registry, activity_type, canonical_payload)
        .map_err(|error| format!("program call payload is invalid: {error:?}"))?;
    let payload_hash = payload_hash_for(&payload)
        .map_err(|error| format!("program payload hash is invalid: {error:?}"))?;
    let signing_key = signer()?;
    let public_key = signing_key.verifying_key().to_bytes();
    let actor = Did::new(request.actor_did.as_bytes())
        .map_err(|error| format!("program caller DID is invalid: {error:?}"))?;
    let authority = Authority::owner(&public_key)
        .map_err(|error| format!("program caller authority is invalid: {error:?}"))?;
    let timestamp = TimestampBound::new(request.not_before_ms, request.expires_at_ms)
        .map_err(|error| format!("program call timestamp is invalid: {error:?}"))?;
    let fee_limit = request
        .fee_limit
        .parse::<u128>()
        .map_err(|_| "fee limit must be an unsigned protocol integer".to_owned())?;
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(layerx_wire::limits::PROTOCOL_VERSION)
        .and_then(|value| value.network_id(request.network_id))
        .and_then(|value| value.activity_type(activity_type))
        .and_then(|value| value.actor_did(actor))
        .and_then(|value| value.authority(authority))
        .and_then(|value| value.account_sequence(request.account_sequence))
        .and_then(|value| value.timestamp_bound(timestamp))
        .and_then(|value| value.idempotency_key(IdempotencyKey::new(idempotency)))
        .and_then(|value| value.fee_limit(Amount::from_u128(fee_limit)))
        .and_then(|value| value.payload_hash(payload_hash))
        .and_then(|value| value.payload(payload))
        .map_err(|error| format!("program call envelope is invalid: {error:?}"))?;
    let unsigned = builder
        .build()
        .map_err(|error| format!("program call envelope is incomplete: {error:?}"))?;
    let preimage = preimage_unsigned(&unsigned)
        .map_err(|error| format!("program call signing preimage is invalid: {error:?}"))?;
    let signature = signing_key.sign(preimage.as_bytes()).to_bytes();
    let signed = unsigned.attach_signature(
        Signature::new(&signature)
            .map_err(|error| format!("program call signature is invalid: {error:?}"))?,
    );
    encode_signed_envelope(&signed)
        .map_err(|error| format!("signed program call is invalid: {error:?}"))
}

fn program_call_registry() -> Result<ModuleRegistry, String> {
    let activity_type = ActivityType::new(ModuleId::Programs, 3)
        .map_err(|error| format!("program call activity is unavailable: {error:?}"))?;
    let registration = ModuleRegistration::new(ModuleId::Programs, &[activity_type])
        .map_err(|error| format!("program module registration is invalid: {error:?}"))?;
    ModuleRegistry::new(&[registration])
        .map_err(|error| format!("program module registry is invalid: {error:?}"))
}

fn build_call(request: &CallRequest<'_>) -> Result<ProgramCall, String> {
    let program = ProgramId::new(fixed_hex::<32>("program id", request.program_id)?);
    let calldata_bytes = if request.calldata.is_empty() {
        Vec::new()
    } else {
        hex_decode("calldata", request.calldata)?
    };
    let calldata = Calldata::new(&calldata_bytes).map_err(describe_call_error)?;
    let fee = request
        .fee_limit
        .parse::<u128>()
        .map_err(|_| "fee limit must be an unsigned protocol integer".to_string())?;
    let budget =
        CallBudget::new(request.fuel, Amount::from_u128(fee)).map_err(describe_call_error)?;
    let mut requested = Vec::with_capacity(request.capabilities.len());
    for name in request.capabilities {
        requested.push(parse_capability(name)?);
    }
    let capabilities = RequestedCapabilities::new(&requested).map_err(describe_call_error)?;
    Ok(ProgramCall::new(program, calldata, budget, capabilities))
}

fn parse_capability(name: &str) -> Result<CapabilityRequest, String> {
    match name {
        "storage-read" => Ok(CapabilityRequest::StorageRead),
        "storage-write" => Ok(CapabilityRequest::StorageWrite),
        "transfer" => Ok(CapabilityRequest::Transfer),
        "emit-event" => Ok(CapabilityRequest::EmitEvent),
        "compose" => Ok(CapabilityRequest::Compose),
        other => Err(format!(
            "unknown capability {other}; expected one of storage-read, storage-write, transfer, emit-event, compose"
        )),
    }
}

const fn describe_call_error(error: ProgramCallError) -> &'static str {
    match error {
        ProgramCallError::NonCanonicalPayload => "program call payload is not canonical",
        ProgramCallError::ZeroFuel => "declared call fuel must be greater than zero",
        ProgramCallError::UnknownCapability(_) => {
            "a requested capability is outside the closed set"
        }
        ProgramCallError::DuplicateCapability(_) => "a capability was requested more than once",
        ProgramCallError::CalldataLength(_) => "calldata exceeds the protocol maximum",
        ProgramCallError::ResponseLength(_) => "the response exceeds the protocol maximum",
        ProgramCallError::NegativeResponseCode(_) => {
            "a successful response cannot carry a negative code"
        }
    }
}

/// Re-binds the typed outcome to the returned receipt. The rendered result is
/// refused unless the receipt's own result code agrees with the typed outcome,
/// so a call is never reported as completed against a receipt that failed, nor
/// as refused against a receipt that succeeded.
fn render_call_result(
    request: &CallRequest<'_>,
    payload: &[u8],
    signed_activity: &[u8],
    head: &VerifiedCallHead,
    response: &Value,
) -> Result<Value, String> {
    let result = response.get("result").unwrap_or(response);
    let receipt_hex = result
        .get("receipt")
        .and_then(Value::as_str)
        .ok_or_else(|| "program-call response omitted the canonical receipt".to_string())?;
    let receipt_bytes = hex_decode("receipt", receipt_hex)?;
    if receipt_bytes.is_empty() {
        return Err("program-call response carried an empty receipt".into());
    }
    let verified =
        verify_program_outcome_at_root(&receipt_bytes, head.sequencer_public_key, head.state_root)
            .map_err(|failure| {
                format!("program receipt verification failed at {:?}", failure.check)
            })?;
    let activity = validate_signed_call(payload, signed_activity)?;
    let expected_activity = activity_id(&activity)
        .map_err(|error| format!("program activity id is invalid: {error:?}"))?;
    let protocol = verified
        .receipt()
        .protocol()
        .ok_or_else(|| "verified program receipt omitted protocol facts".to_owned())?;
    if protocol.activity_id() != expected_activity {
        return Err("program receipt names a different signed activity".to_owned());
    }
    let receipt_digest = verified
        .evidence()
        .receipt_digest()
        .ok_or_else(|| "program receipt verifier produced no digest".to_owned())?;
    let program = protocol
        .program_outcome()
        .ok_or_else(|| "verified receipt omitted its Programs outcome".to_owned())?;
    if program.abi_version() != head.abi_version {
        return Err("program receipt ABI does not match verified discovery".to_owned());
    }
    if head.observed_sequence.checked_add(1) != Some(protocol.global_sequence()) {
        return Err("program receipt sequence does not extend verified discovery".to_owned());
    }
    let terminal_payload = hex_decode(
        "terminal payload",
        result["terminal_payload"]
            .as_str()
            .ok_or_else(|| "program response omitted authenticated terminal payload".to_owned())?,
    )?;
    let terminal_digest: [u8; 32] = Sha256::digest(&terminal_payload).into();
    if terminal_digest != program.terminal_payload_root() {
        return Err("terminal payload does not match the signed receipt commitment".to_owned());
    }
    let result_code = program.result_code();
    let detail = decode_terminal_payload(
        program.terminal_kind(),
        program.abi_version(),
        &terminal_payload,
    )
    .map_err(|error| format!("program terminal detail is invalid: {error:?}"))?;
    let call_graph = hex_decode(
        "call graph",
        result["call_graph"]
            .as_str()
            .ok_or_else(|| "program response omitted authenticated call graph".to_owned())?,
    )?;
    verify_terminal_commitments(&detail, &call_graph, protocol.protocol_version(), program)?;
    let outcome = render_terminal(&detail.detail, request.program_id, program, result_code)?;
    Ok(json!({
        "program_id": request.program_id,
        "program_version": head.version,
        "program_code_hash": hex_encode(&head.code_hash),
        "idempotency_key": request.idempotency_key,
        "canonical_payload": hex_encode(payload),
        "contract_major": PROGRAM_CALL_CONTRACT_MAJOR,
        "receipt": receipt_hex,
        "receipt_digest": hex_encode(&receipt_digest),
        "result_code": result_code,
        "verified_previous_state_root": hex_encode(&protocol.previous_state_root()),
        "verified_resulting_state_root": hex_encode(&protocol.resulting_state_root()),
        "metered_cost": program.fee_units().to_string(),
        "fee_units": program.fee_units().to_string(),
        "resources": {"cpu_fuel":program.cpu_fuel(),"memory_bytes":program.memory_bytes(),"storage_read_bytes":program.storage_read_bytes(),"storage_write_bytes":program.storage_write_bytes(),"output_values":program.output_values(),"output_bytes":program.output_bytes()},
        "outcome": outcome,
        "execution_evidence": render_execution_evidence(&detail.detail),
        "call_graph":hex_encode(&call_graph),
        "terminal_attachments": detail.attachments.iter().map(render_attachment).collect::<Vec<_>>(),
        "verification": "canonical receipt, configured sequencer signature, pinned prior state root and exact signed activity id verified locally",
    }))
}

fn render_execution_evidence(detail: &TerminalDetail) -> Value {
    match detail {
        TerminalDetail::Execution(ExecutionTerminal::Legacy { trace, .. }) => {
            json!({"trace":trace.as_ref().map(|value|hex_encode(value)),"call_graph":Value::Null})
        }
        TerminalDetail::Execution(ExecutionTerminal::CandidateV4 { trace, graph, .. }) => {
            json!({"trace":trace.as_ref().map(|value|hex_encode(value)),"call_graph":hex_encode(graph)})
        }
        _ => Value::Null,
    }
}

fn render_terminal(
    detail: &TerminalDetail,
    program_id: &str,
    receipt: &layerx_wire::receipt::ProgramOutcome,
    result_code: i32,
) -> Result<Value, String> {
    Ok(match detail {
        TerminalDetail::Execution(ExecutionTerminal::Legacy {
            encoding_version,
            runtime_version,
            abi_version,
            metering_schedule_version,
            values,
            usage,
            trace,
        }) => {
            if *runtime_version != receipt.runtime_version()
                || *abi_version != 1
                || *metering_schedule_version != receipt.metering_schedule_version()
                || usage.cpu_fuel != receipt.cpu_fuel()
                || usage.memory_bytes != receipt.memory_bytes()
                || usage.storage_read_bytes != receipt.storage_read_bytes()
                || usage.storage_write_bytes != receipt.storage_write_bytes()
                || usage.output_values != receipt.output_values()
                || usage.fee_units != receipt.fee_units()
            {
                return Err(
                    "legacy terminal detail disagrees with signed receipt versions".to_owned(),
                );
            }
            json!({"status":"completed","format":format!("execution-v{encoding_version}"),"code":result_code,
                "values":values.iter().map(|value| format!("{value:?}")).collect::<Vec<_>>(),
                "trace":trace.as_ref().map(|value|hex_encode(value))})
        }
        TerminalDetail::Execution(ExecutionTerminal::CandidateV4 {
            runtime_version,
            fee_schedule_version,
            metering_schedule_version,
            program,
            abi_version,
            usage,
            outcome,
            trace,
            graph,
            ..
        }) => {
            if *runtime_version != receipt.runtime_version()
                || *fee_schedule_version != receipt.fee_schedule_version()
                || *metering_schedule_version != receipt.metering_schedule_version()
                || *abi_version != 2
                || *program != fixed_hex("program id", program_id)?
                || usage.cpu_fuel != receipt.cpu_fuel()
                || usage.memory_bytes != receipt.memory_bytes()
                || usage.storage_read_bytes != receipt.storage_read_bytes()
                || usage.storage_write_bytes != receipt.storage_write_bytes()
                || usage.output_values != receipt.output_values()
                || usage.output_bytes != receipt.output_bytes()
                || usage.fee_units != receipt.fee_units()
            {
                return Err(
                    "candidate terminal detail disagrees with signed receipt identity or versions"
                        .to_owned(),
                );
            }
            match outcome {
                CandidateTerminalOutcome::Success { code, response } => {
                    json!({"status":"completed","format":"execution-v4","code":code,"response":hex_encode(response),"trace":trace.as_ref().map(|value|hex_encode(value)),"call_graph":hex_encode(graph)})
                }
                CandidateTerminalOutcome::Failure(failure) => {
                    render_program_failure(failure, result_code)
                }
                CandidateTerminalOutcome::Resource(resource) => {
                    json!({"status":"refused","failure":{"kind":"resource","detail":render_resource_refusal(*resource),"result_code":result_code}})
                }
            }
        }
        TerminalDetail::Failure(FailureTerminal::Program(failure)) => {
            render_program_failure(failure, result_code)
        }
        TerminalDetail::Failure(FailureTerminal::Composition { tag, fields }) => {
            json!({"status":"refused","failure":{"kind":"composition","tag":tag,"fields":format!("{fields:?}"),"result_code":result_code}})
        }
        TerminalDetail::Failure(FailureTerminal::Entrypoint { tag, fields }) => {
            json!({"status":"refused","failure":{"kind":"entrypoint","tag":tag,"fields":format!("{fields:?}"),"result_code":result_code}})
        }
        TerminalDetail::Failure(FailureTerminal::Abi { tag, fields }) => {
            json!({"status":"refused","failure":{"kind":"abi","tag":tag,"fields":format!("{fields:?}"),"result_code":result_code}})
        }
        TerminalDetail::Failure(FailureTerminal::Settlement(error)) => {
            json!({"status":"refused","failure":{"kind":"settlement","detail":format!("{error:?}"),"result_code":result_code}})
        }
        TerminalDetail::Failure(FailureTerminal::Callback { stage, status }) => {
            json!({"status":"refused","failure":{"kind":"callback","stage":stage,"status":status,"result_code":result_code}})
        }
        TerminalDetail::Resource(resource) => {
            json!({"status":"refused","failure":{"kind":"resource","detail":render_resource_refusal(*resource),"result_code":result_code}})
        }
    })
}

fn verify_terminal_commitments(
    detail: &layerx_programs_runtime::terminal::DecodedTerminal,
    available_graph: &[u8],
    protocol_version: u16,
    receipt: &layerx_wire::receipt::ProgramOutcome,
) -> Result<(), String> {
    if available_graph.is_empty()
        || <[u8; 32]>::from(Sha256::digest(available_graph)) != receipt.call_graph_root()
    {
        return Err("call graph bytes disagree with the signed receipt root".to_owned());
    }
    if let TerminalDetail::Execution(ExecutionTerminal::CandidateV4 { graph, .. }) = &detail.detail
    {
        if graph != available_graph {
            return Err("embedded and separately authenticated call graphs disagree".to_owned());
        }
    }
    let candidate = matches!(
        &detail.detail,
        TerminalDetail::Execution(ExecutionTerminal::CandidateV4 { .. })
    );
    let successful_execution = receipt.terminal_kind() == 1
        && matches!(
            &detail.detail,
            TerminalDetail::Execution(
                ExecutionTerminal::Legacy { .. }
                    | ExecutionTerminal::CandidateV4 {
                        outcome: CandidateTerminalOutcome::Success { .. },
                        ..
                    }
            )
        );
    let occupancy_required = protocol_version == 2 && successful_execution;
    if !matches!(protocol_version, 1 | 2) {
        return Err("unsupported receipt protocol version for terminal evidence".to_owned());
    }
    let mut occupancy_seen = false;
    let mut occupancy_present = false;
    let mut authority_seen = false;
    for attachment in &detail.attachments {
        match attachment {
            TerminalAttachment::Occupancy(bytes) => {
                if occupancy_seen || !occupancy_required {
                    return Err("occupancy wrapper is not permitted by the receipt protocol and terminal family".to_owned());
                }
                occupancy_seen = true;
                if bytes.is_empty() {
                    if receipt.occupancy_evidence_digest() != [0; 32]
                        || receipt.occupancy_transfer_root() != [0; 32]
                        || receipt.occupancy_byte_batches() != 0
                        || receipt.occupancy_fee_units() != 0
                    {
                        return Err(
                            "empty occupancy wrapper disagrees with nonempty signed receipt facts"
                                .to_owned(),
                        );
                    }
                    continue;
                }
                occupancy_present = true;
                if <[u8; 32]>::from(Sha256::digest(bytes)) != receipt.occupancy_evidence_digest() {
                    return Err(
                        "occupancy evidence disagrees with the signed receipt digest".to_owned(),
                    );
                }
                let settlement = OccupancySettlement::canonical_decode(bytes)
                    .map_err(|_| "occupancy attachment is not canonical".to_owned())?;
                if settlement.usage().byte_batches != receipt.occupancy_byte_batches()
                    || settlement.usage().fee_units != receipt.occupancy_fee_units()
                    || settlement
                        .transfer_root(receipt.occupancy_asset_id())
                        .map_err(|_| "occupancy transfer evidence is invalid".to_owned())?
                        != receipt.occupancy_transfer_root()
                {
                    return Err(
                        "occupancy evidence disagrees with signed count, fee, or transfer root"
                            .to_owned(),
                    );
                }
            }
            TerminalAttachment::TransferAuthority {
                authorization,
                transfer_root,
            } => {
                if !candidate
                    || authority_seen
                    || *transfer_root != receipt.transfer_root()
                    || layerx_programs_runtime::transfer::verify_authorization_root(
                        authorization,
                        *transfer_root,
                    )
                    .is_err()
                {
                    return Err("transfer-authority attachment disagrees with the candidate authorization regime or signed transfer root".to_owned());
                }
                authority_seen = true;
            }
        }
    }
    if occupancy_required && !occupancy_seen
        || occupancy_present != (receipt.occupancy_evidence_digest() != [0; 32])
        || candidate && authority_seen != (receipt.transfer_root() != [0; 32])
    {
        return Err(
            "signed receipt attachment presence is not represented by the terminal ABI regime"
                .to_owned(),
        );
    }
    Ok(())
}

fn render_program_failure(
    failure: &layerx_programs_runtime::ProgramFailure,
    result_code: i32,
) -> Value {
    json!({"status":"refused","failure":{"kind":"program","class":failure.class().code(),
        "program_id":hex_encode(&failure.program().bytes()),"reason":hex_encode(failure.reason().bytes()),
        "result_code":result_code}})
}

fn render_attachment(attachment: &TerminalAttachment) -> Value {
    match attachment {
        TerminalAttachment::Occupancy(bytes) => {
            json!({"kind":"occupancy","canonical_evidence":hex_encode(bytes)})
        }
        TerminalAttachment::TransferAuthority {
            authorization,
            transfer_root,
        } => {
            json!({"kind":"transfer-authority","authorization":hex_encode(authorization),"transfer_root":hex_encode(transfer_root)})
        }
    }
}

fn discover_call_head(
    client: &Client,
    request: &CallRequest<'_>,
) -> Result<VerifiedCallHead, String> {
    let response = client.get(&format!("/v1/programs/registry/{}", request.program_id))?;
    let result = response.get("result").unwrap_or(&response);
    if result.get("program_id").and_then(Value::as_str) != Some(request.program_id)
        || result.get("lifecycle").and_then(Value::as_str) != Some("active")
    {
        return Err("program discovery identity or lifecycle is invalid".to_owned());
    }
    let discovered_root = result
        .get("state_root")
        .and_then(Value::as_str)
        .ok_or_else(|| "program discovery omitted state root".to_owned())?;
    let state_root = fixed_hex("discovery state root", discovered_root)?;
    let abi = result
        .get("abi_version")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| "program discovery omitted ABI version".to_owned())?;
    if !matches!(abi, 1 | 2) {
        return Err("program discovery returned unsupported ABI".to_owned());
    }
    let observed_sequence = result
        .get("observed_sequence")
        .and_then(Value::as_u64)
        .ok_or_else(|| "program discovery omitted observed sequence".to_owned())?;
    let version = result
        .get("version")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "program discovery omitted version".to_owned())?;
    let code_hash = fixed_hex(
        "program code hash",
        result
            .get("code_hash")
            .and_then(Value::as_str)
            .ok_or_else(|| "program discovery omitted code hash".to_owned())?,
    )?;
    let observed_at = result
        .get("observed_at")
        .and_then(Value::as_u64)
        .ok_or_else(|| "program discovery omitted observed time".to_owned())?;
    let valid_through = result
        .get("valid_through")
        .and_then(Value::as_u64)
        .ok_or_else(|| "program discovery omitted validity bound".to_owned())?;
    if request.not_before_ms < observed_at || request.not_before_ms > valid_through {
        return Err("program discovery is outside its signed freshness interval".to_owned());
    }
    let mut proof = b"LayerX/program-discovery-proof/v1\0".to_vec();
    proof.extend_from_slice(&fixed_hex::<32>("program id", request.program_id)?);
    proof.push(1);
    proof.extend_from_slice(&version.to_be_bytes());
    proof.extend_from_slice(&code_hash);
    proof.extend_from_slice(&abi.to_be_bytes());
    proof.extend_from_slice(&observed_sequence.to_be_bytes());
    proof.extend_from_slice(&observed_at.to_be_bytes());
    proof.extend_from_slice(&valid_through.to_be_bytes());
    proof.extend_from_slice(&state_root);
    let digest: [u8; 32] = Sha256::digest(&proof).into();
    let expected_digest = hex_encode(&digest);
    if result.get("receipt_digest").and_then(Value::as_str) != Some(expected_digest.as_str()) {
        return Err("program discovery receipt digest is invalid".to_owned());
    }
    let public_key = fixed_hex(
        "discovery public key",
        result
            .get("discovery_public_key")
            .and_then(Value::as_str)
            .ok_or_else(|| "program discovery omitted public key".to_owned())?,
    )?;
    if public_key
        != fixed_hex(
            "configured sequencer public key",
            request.sequencer_public_key,
        )?
    {
        return Err("program discovery authority differs from configured trust anchor".to_owned());
    }
    let signature = hex_decode(
        "discovery signature",
        result
            .get("discovery_signature")
            .and_then(Value::as_str)
            .ok_or_else(|| "program discovery omitted signature".to_owned())?,
    )?;
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| "discovery signature must be 64 bytes".to_owned())?;
    ed25519::verify_digest(&public_key, &signature, &digest)
        .map_err(|_| "program discovery signature is invalid".to_owned())?;
    Ok(VerifiedCallHead {
        sequencer_public_key: public_key,
        state_root,
        abi_version: abi,
        version,
        code_hash,
        observed_sequence,
        observed_at,
    })
}

fn verify_simulation_evidence(
    request: &CallRequest<'_>,
    signed_activity: &[u8],
    head: &VerifiedCallHead,
    result: &Value,
) -> Result<(), String> {
    let evidence = result
        .get("simulation_evidence")
        .ok_or_else(|| "program simulation omitted sealed non-commit evidence".to_owned())?;
    if evidence.get("committed").and_then(Value::as_bool) != Some(false) {
        return Err("program simulation evidence claims a committed transition".to_owned());
    }
    let key = head.sequencer_public_key;
    let mut boundary_material = EMULATOR_BOUNDARY_DOMAIN.to_vec();
    boundary_material.extend_from_slice(&key);
    let boundary_id: [u8; 32] = Sha256::digest(boundary_material).into();
    if fixed_hex::<32>(
        "simulation boundary",
        evidence
            .get("boundary_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "program simulation omitted boundary identity".to_owned())?,
    )? != boundary_id
        || fixed_hex::<32>(
            "simulation prior root",
            evidence
                .get("previous_state_root")
                .and_then(Value::as_str)
                .ok_or_else(|| "program simulation omitted prior root".to_owned())?,
        )? != head.state_root
        || evidence.get("observed_sequence").and_then(Value::as_u64) != Some(head.observed_sequence)
        || evidence.get("observed_at").and_then(Value::as_u64) != Some(head.observed_at)
    {
        return Err("program simulation evidence does not extend verified discovery".to_owned());
    }
    let activity =
        validate_signed_call(&build_call(request)?.canonical_payload(), signed_activity)?;
    let expected_activity = activity_id(&activity)
        .map_err(|error| format!("program simulation activity id is invalid: {error:?}"))?;
    let evidence_activity: [u8; 32] = fixed_hex(
        "simulation activity id",
        evidence
            .get("activity_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "program simulation omitted activity id".to_owned())?,
    )?;
    if evidence_activity != expected_activity {
        return Err("program simulation evidence names another activity".to_owned());
    }
    let hypothetical_root: [u8; 32] = fixed_hex(
        "simulation hypothetical root",
        evidence
            .get("hypothetical_state_root")
            .and_then(Value::as_str)
            .ok_or_else(|| "program simulation omitted hypothetical root".to_owned())?,
    )?;
    let receipt = hex_decode(
        "simulation receipt",
        result
            .get("receipt")
            .and_then(Value::as_str)
            .ok_or_else(|| "program simulation omitted receipt".to_owned())?,
    )?;
    let verified =
        verify_program_outcome_at_root(&receipt, key, head.state_root).map_err(|failure| {
            format!(
                "simulation receipt verification failed at {:?}",
                failure.check
            )
        })?;
    let protocol = verified
        .receipt()
        .protocol()
        .ok_or_else(|| "verified simulation receipt omitted protocol facts".to_owned())?;
    if protocol.resulting_state_root() != hypothetical_root
        || protocol.activity_id() != expected_activity
        || head.observed_sequence.checked_add(1) != Some(protocol.global_sequence())
    {
        return Err("program simulation evidence disagrees with its verified receipt".to_owned());
    }
    let mut preimage = SIMULATION_EVIDENCE_DOMAIN.to_vec();
    preimage.extend_from_slice(&boundary_id);
    preimage.extend_from_slice(&expected_activity);
    preimage.extend_from_slice(&head.state_root);
    preimage.extend_from_slice(&hypothetical_root);
    preimage.extend_from_slice(&head.observed_sequence.to_be_bytes());
    preimage.extend_from_slice(&head.observed_at.to_be_bytes());
    preimage.push(0);
    let digest: [u8; 32] = Sha256::digest(preimage).into();
    let declared_key: [u8; 32] = fixed_hex(
        "simulation evidence public key",
        evidence
            .get("public_key")
            .and_then(Value::as_str)
            .ok_or_else(|| "program simulation omitted evidence public key".to_owned())?,
    )?;
    if declared_key != key {
        return Err(
            "simulation evidence authority differs from configured trust anchor".to_owned(),
        );
    }
    let signature: [u8; 64] = hex_decode(
        "simulation evidence signature",
        evidence
            .get("signature")
            .and_then(Value::as_str)
            .ok_or_else(|| "program simulation omitted evidence signature".to_owned())?,
    )?
    .try_into()
    .map_err(|_| "simulation evidence signature must be 64 bytes".to_owned())?;
    ed25519::verify_digest(&key, &signature, &digest)
        .map_err(|_| "program simulation evidence signature is invalid".to_owned())
}

fn render_resource_refusal(refusal: BudgetMeterRefusal) -> Value {
    let resource_name = |resource| match resource {
        BudgetResourceKind::Cpu => "cpu",
        BudgetResourceKind::Memory => "memory",
        BudgetResourceKind::StorageRead => "storage-read",
        BudgetResourceKind::StorageWrite => "storage-write",
        BudgetResourceKind::Output => "output",
        BudgetResourceKind::OutputBytes => "output-bytes",
        BudgetResourceKind::Table => "table",
    };
    match refusal {
        BudgetMeterRefusal::BudgetExceeded {
            resource,
            limit,
            attempted,
        } => json!({
            "type":"budget-exceeded", "resource":resource_name(resource),
            "limit":limit, "attempted":attempted
        }),
        BudgetMeterRefusal::CounterOverflow { resource } => json!({
            "type":"counter-overflow", "resource":resource_name(resource)
        }),
    }
}

fn validate_signed_call(
    payload: &[u8],
    signed_activity: &[u8],
) -> Result<layerx_wire::activity::Activity, String> {
    let registration = ModuleRegistration::new(
        ModuleId::Programs,
        &[ActivityType::new(ModuleId::Programs, 3)
            .map_err(|error| format!("program activity unavailable: {error:?}"))?],
    )
    .map_err(|error| format!("program registry invalid: {error:?}"))?;
    let registry = ModuleRegistry::new(&[registration])
        .map_err(|error| format!("program registry invalid: {error:?}"))?;
    let activity = decode_signed(signed_activity, &registry)
        .map_err(|error| format!("signed program activity is invalid: {error:?}"))?;
    if activity.activity_type().module() != ModuleId::Programs
        || activity.activity_type().ordinal() != 3
        || activity.payload() != payload
    {
        return Err("signed activity does not carry this exact Programs CALL payload".to_owned());
    }
    Ok(activity)
}

#[cfg(test)]
fn classify_outcome(result: &Value, result_code: i64) -> Result<Value, String> {
    let declared = result.get("outcome");
    let status = declared
        .and_then(|outcome| outcome.get("status"))
        .and_then(Value::as_str);
    match status {
        Some("completed") => {
            if result_code < 0 {
                return Err(
                    "response reports a completed call but the receipt carries a failure code"
                        .into(),
                );
            }
            let code = declared
                .and_then(|outcome| outcome.get("code"))
                .and_then(Value::as_i64)
                .unwrap_or(result_code);
            if code != result_code {
                return Err("response outcome code disagrees with the receipt result code".into());
            }
            Ok(json!({
                "status": "completed",
                "code": result_code,
                "response": declared
                    .and_then(|outcome| outcome.get("response"))
                    .cloned()
                    .unwrap_or(Value::Null),
            }))
        }
        Some("refused") => {
            if result_code >= 0 {
                return Err(
                    "response reports a refused call but the receipt carries a success code".into(),
                );
            }
            Ok(json!({
                "status": "refused",
                "failure": declared
                    .and_then(|outcome| outcome.get("failure"))
                    .cloned()
                    .unwrap_or(Value::Null),
            }))
        }
        Some(other) => Err(format!(
            "response carried an unknown call outcome status {other}"
        )),
        None => {
            if result_code >= 0 {
                Ok(json!({"status": "completed", "code": result_code, "response": Value::Null}))
            } else {
                Ok(json!({"status": "refused", "failure": {"result_code": result_code}}))
            }
        }
    }
}

fn gate_artifact(path: &Path) -> Result<(String, String), String> {
    let artifact = path
        .canonicalize()
        .map_err(|error| format!("could not resolve {}: {error}", path.display()))?;
    let Some(project) = enclosing_project(&artifact) else {
        return Ok((
            "unknown".into(),
            format!("not run; no {DESCRIPTOR} toolchain descriptor encloses the artifact"),
        ));
    };
    let Some(toolchain) = load_toolchain(&project)? else {
        return Ok((
            "unknown".into(),
            format!("not run; no {DESCRIPTOR} toolchain descriptor encloses the artifact"),
        ));
    };
    match &toolchain.lint {
        Some(lint) => {
            run_step(
                &toolchain.project,
                lint,
                &format!("{} determinism lint", toolchain.language),
            )?;
            Ok((toolchain.language, "passed".into()))
        }
        None => Ok((
            toolchain.language,
            "not declared by the toolchain descriptor".into(),
        )),
    }
}

fn enclosing_project(artifact: &Path) -> Option<PathBuf> {
    let mut directory = artifact.parent();
    while let Some(candidate) = directory {
        if candidate.join(DESCRIPTOR).is_file() {
            return Some(candidate.to_path_buf());
        }
        directory = candidate.parent();
    }
    None
}

fn load_toolchain(project: &Path) -> Result<Option<Toolchain>, String> {
    let descriptor = project.join(DESCRIPTOR);
    if !descriptor.is_file() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&descriptor)
        .map_err(|error| format!("could not read {}: {error}", descriptor.display()))?;
    let document: Value = serde_json::from_str(&contents)
        .map_err(|error| format!("could not parse {}: {error}", descriptor.display()))?;
    let language = string_field(&document, "language", &descriptor)?;
    let build = document
        .get("build")
        .ok_or_else(|| format!("{} declares no build step", descriptor.display()))?;
    let artifact = string_field(build, "artifact", &descriptor)?;
    Ok(Some(Toolchain {
        project: project.to_path_buf(),
        language,
        build: step(build, "build", &descriptor)?,
        artifact: PathBuf::from(artifact),
        lint: match document.get("lint") {
            Some(value) => Some(step(value, "lint", &descriptor)?),
            None => None,
        },
    }))
}

fn step(value: &Value, name: &str, descriptor: &Path) -> Result<Step, String> {
    let command = string_field(value, "command", descriptor)?;
    let args = match value.get("args") {
        Some(Value::Array(entries)) => entries
            .iter()
            .map(|entry| {
                entry.as_str().map(str::to_owned).ok_or_else(|| {
                    format!(
                        "{} declares a non-string argument in its {name} step",
                        descriptor.display()
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err(format!(
                "{} declares a non-array args in its {name} step",
                descriptor.display()
            ))
        }
        None => Vec::new(),
    };
    Ok(Step { command, args })
}

fn string_field(value: &Value, key: &str, descriptor: &Path) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{} declares no {key}", descriptor.display()))
}

fn run_step(project: &Path, step: &Step, description: &str) -> Result<(), String> {
    let status = Command::new(&step.command)
        .current_dir(project)
        .args(&step.args)
        .status()
        .map_err(|error| format!("could not start the {description}: {error}"))?;
    if !status.success() {
        return Err(format!("the {description} failed with {status}"));
    }
    Ok(())
}

fn resolve(project: &Path, artifact: &Path) -> PathBuf {
    if artifact.is_absolute() {
        artifact.to_owned()
    } else {
        project.join(artifact)
    }
}

fn discover_artifact(project: &Path) -> Result<PathBuf, String> {
    let directory = project.join("target/wasm32-unknown-unknown/release");
    let mut artifacts = fs::read_dir(&directory)
        .map_err(|error| format!("could not inspect {}: {error}", directory.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "wasm")
        })
        .collect::<Vec<_>>();
    artifacts.sort();
    match artifacts.as_slice() {
        [artifact] => Ok(artifact.clone()),
        [] => Err(format!(
            "the Rust program toolchain produced no .wasm artifact in {}",
            directory.display()
        )),
        _ => Err("multiple .wasm artifacts were produced; select one with --artifact".into()),
    }
}

#[cfg(test)]
mod call_tests {
    use super::{
        build_call, classify_outcome, describe_call_error, render_call_result,
        signed_call_with_signer, validate_signed_call, CallRequest, VerifiedCallHead,
    };
    use crate::encoding::hex_encode;
    use ed25519_dalek::SigningKey;
    use layerx_types::amount::Amount;
    use layerx_types::intent::{
        CallBudget, Calldata, CapabilityRequest, ProgramCall, ProgramCallError, ProgramId,
        RequestedCapabilities,
    };
    use serde_json::json;

    /// The shared canonical program-call payload the agent layer, the CLI and
    /// the emulator all encode for the same call, so the same call yields the
    /// same receipt on every surface.
    const GOLDEN_PAYLOAD_HEX: &str = "4c61796572582f70726f6772616d732f63616c6c2f763100111111111111111111111111111111111111111111111111111111111111111100000000000003e8000000000000000000000000000000fa0002010300000002aabb";

    fn golden_request() -> CallRequest<'static> {
        CallRequest {
            program_id: "1111111111111111111111111111111111111111111111111111111111111111",
            calldata: "aabb",
            fuel: 1000,
            fee_limit: "250",
            capabilities: &[],
            idempotency_key: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            network_id: 402,
            actor_did: "did:layerx:test",
            key_name: "test",
            account_sequence: 0,
            not_before_ms: 1_700_000_000_000,
            expires_at_ms: 1_700_000_300_000,
            sequencer_public_key:
                "2152f8d19b791d24453242e15f2eab6cb7cffa7b6a5ed30097960e069881db12",
        }
    }

    fn agent_layer_call() -> ProgramCall {
        let program = ProgramId::new([0x11; 32]);
        let Ok(calldata) = Calldata::new(&[0xAA, 0xBB]) else {
            panic!("bounded calldata rejected");
        };
        let Ok(budget) = CallBudget::new(1000, Amount::from_u128(250)) else {
            panic!("non-zero fuel rejected");
        };
        let Ok(capabilities) = RequestedCapabilities::new(&[
            CapabilityRequest::Transfer,
            CapabilityRequest::StorageRead,
        ]) else {
            panic!("unique capabilities rejected");
        };
        ProgramCall::new(program, calldata, budget, capabilities)
    }

    #[test]
    fn cli_and_agent_layer_encode_the_same_canonical_payload() {
        let capabilities = ["transfer".to_string(), "storage-read".to_string()];
        let request = CallRequest {
            program_id: "1111111111111111111111111111111111111111111111111111111111111111",
            calldata: "aabb",
            fuel: 1000,
            fee_limit: "250",
            capabilities: &capabilities,
            idempotency_key: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            network_id: 402,
            actor_did: "did:layerx:test",
            key_name: "test",
            account_sequence: 0,
            not_before_ms: 1_700_000_000_000,
            expires_at_ms: 1_700_000_300_000,
            sequencer_public_key:
                "2152f8d19b791d24453242e15f2eab6cb7cffa7b6a5ed30097960e069881db12",
        };
        let Ok(built) = build_call(&request) else {
            panic!("valid call request rejected");
        };
        assert_eq!(hex_encode(&built.canonical_payload()), GOLDEN_PAYLOAD_HEX);
        assert_eq!(
            built.canonical_payload(),
            agent_layer_call().canonical_payload()
        );
    }

    #[test]
    fn non_canonical_call_payload_has_an_explicit_cli_refusal() {
        assert_eq!(
            describe_call_error(ProgramCallError::NonCanonicalPayload),
            "program call payload is not canonical"
        );
    }

    #[test]
    fn completed_outcome_is_bound_to_a_successful_receipt() {
        let result = json!({
            "result_code": 0,
            "outcome": {"status": "completed", "code": 0, "response": "aabb"},
        });
        let Ok(outcome) = classify_outcome(&result, 0) else {
            panic!("consistent completed outcome rejected");
        };
        assert_eq!(outcome["status"], "completed");
        assert_eq!(outcome["code"], 0);
    }

    #[test]
    fn completed_outcome_against_a_failed_receipt_is_refused() {
        let result = json!({
            "result_code": -736,
            "outcome": {"status": "completed", "code": 0},
        });
        assert!(classify_outcome(&result, -736).is_err());
    }

    #[test]
    fn refused_outcome_is_bound_to_a_failed_receipt() {
        let result = json!({
            "result_code": -736,
            "outcome": {"status": "refused", "failure": {"class": "guest-refused"}},
        });
        let Ok(outcome) = classify_outcome(&result, -736) else {
            panic!("consistent refused outcome rejected");
        };
        assert_eq!(outcome["status"], "refused");
    }

    #[test]
    fn render_refuses_unverified_receipt_bytes_even_with_success_siblings() {
        let request = golden_request();
        let payload = agent_layer_call().canonical_payload();
        let response = json!({
            "result": {
                "receipt": "aabbccdd",
                "result_code": 0,
                "outcome": {"status": "completed", "code": 0, "response": "aabb"},
            }
        });
        let head = VerifiedCallHead {
            sequencer_public_key: [0; 32],
            state_root: [0; 32],
            abi_version: 1,
            version: 1,
            code_hash: [1; 32],
            observed_sequence: 0,
            observed_at: 1,
        };
        assert!(render_call_result(&request, &payload, &[], &head, &response).is_err());
    }

    #[test]
    fn render_refuses_a_response_without_a_receipt() {
        let request = golden_request();
        let payload = agent_layer_call().canonical_payload();
        let response = json!({"result": {"result_code": 0}});
        let head = VerifiedCallHead {
            sequencer_public_key: [0; 32],
            state_root: [0; 32],
            abi_version: 1,
            version: 1,
            code_hash: [1; 32],
            observed_sequence: 0,
            observed_at: 1,
        };
        assert!(render_call_result(&request, &payload, &[], &head, &response).is_err());
    }

    #[test]
    fn call_a_refuses_activity_signed_for_call_b() {
        let request = golden_request();
        let call_a = agent_layer_call().canonical_payload();
        let mut call_b = call_a.clone();
        let last = call_b.len() - 1;
        call_b[last] ^= 1;
        let signed_b =
            signed_call_with_signer(&request, &call_b, || Ok(SigningKey::from_bytes(&[7; 32])))
                .expect("source vector signs");
        assert!(validate_signed_call(&call_a, &signed_b).is_err());
    }
}
