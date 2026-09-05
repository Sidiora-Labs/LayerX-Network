use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::ed25519;
use layerx_programs_runtime::access::AccessDeclaration;
use layerx_programs_runtime::terminal::{
    decode_terminal_payload, CandidateTerminalOutcome, ExecutionTerminal, FailureTerminal,
    TerminalAttachment, TerminalDetail,
};
use layerx_programs_runtime::{
    BudgetMeterRefusal, BudgetResourceKind, OccupancySettlement, WasmEngine,
};
use layerx_programs_runtime::{Capability, CapabilitySet};
use layerx_proof::receipt::verify_program_outcome_at_root;
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::intent::ProgramId;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_types::program_call::{NativeProgramCall, Resources};
use layerx_types::program_lifecycle::{
    NativeProgramDeploy, NativeProgramUpgrade, NativeProgramWindDown, ProgramUpgradePolicy,
    ProgramWindDownOperation,
};
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
    let abi_version = artifact_abi(&path)?;
    let validated = engine
        .validate_versioned(abi_version, &wasm)
        .map_err(|error| format!("program violates the deterministic WASM policy: {error}"))?;
    let code_hash: [u8; 32] = Sha256::digest(&wasm).into();
    Ok(json!({
        "artifact": path.display().to_string(),
        "code_hash": hex_encode(&code_hash),
        "byte_size": validated.byte_size(),
        "function_count": validated.function_count(),
        "abi_version": abi_version,
        "deterministic_validation": "passed",
    }))
}

pub struct DeployRequest<'a> {
    pub artifact: &'a Path,
    pub upgrade_authority: Option<&'a str>,
    pub interface: Option<&'a Path>,
}

pub fn deploy(
    client: &Client,
    request: &CallRequest<'_>,
    deployment: &DeployRequest<'_>,
    previous_state_root: &str,
) -> Result<Value, String> {
    let inspected = inspect_artifact(deployment.artifact)?;
    gate_artifact(deployment.artifact)?;
    let wasm = read_program_file(deployment.artifact)?;
    let interface = deployment.interface.map(read_program_file).transpose()?;
    validate_interface(
        interface.as_deref(),
        &wasm,
        artifact_abi(deployment.artifact)?,
    )?;
    let payload = NativeProgramDeploy {
        program_id: ProgramId::new(fixed_hex("program id", request.program_id)?),
        guest_abi: artifact_abi(deployment.artifact)?,
        policy: deployment
            .upgrade_authority
            .map(|authority| {
                fixed_hex("upgrade authority", authority).map(ProgramUpgradePolicy::Authority)
            })
            .transpose()?
            .unwrap_or(ProgramUpgradePolicy::Immutable),
        new_hash: Sha256::digest(&wasm).into(),
        interface: interface.as_deref(),
        wasm: &wasm,
    }
    .encode()
    .map_err(|error| format!("invalid native deployment: {error:?}"))?;
    let mut result = submit_lifecycle(client, request, 1, &payload, previous_state_root)?;
    result["artifact"] = inspected;
    Ok(result)
}

pub struct UpgradeRequest<'a> {
    pub artifact: &'a Path,
    pub old_hash: &'a str,
    pub migration_hook: Option<&'a Path>,
    pub interface: Option<&'a Path>,
    pub clear_interface: bool,
}

pub fn upgrade(
    client: &Client,
    request: &CallRequest<'_>,
    upgrade: &UpgradeRequest<'_>,
    previous_state_root: &str,
) -> Result<Value, String> {
    inspect_artifact(upgrade.artifact)?;
    gate_artifact(upgrade.artifact)?;
    if upgrade.clear_interface && upgrade.interface.is_some() {
        return Err("--clear-interface conflicts with --interface".into());
    }
    let wasm = read_program_file(upgrade.artifact)?;
    let hook = upgrade
        .migration_hook
        .map(read_program_file)
        .transpose()?
        .unwrap_or_default();
    if upgrade.migration_hook.is_some() && hook.is_empty() {
        return Err("migration hook must not be empty".into());
    }
    let interface = upgrade.interface.map(read_program_file).transpose()?;
    validate_interface(interface.as_deref(), &wasm, artifact_abi(upgrade.artifact)?)?;
    let payload = NativeProgramUpgrade {
        program_id: ProgramId::new(fixed_hex("program id", request.program_id)?),
        guest_abi: artifact_abi(upgrade.artifact)?,
        old_hash: fixed_hex("old code hash", upgrade.old_hash)?,
        new_hash: Sha256::digest(&wasm).into(),
        migration_hook: &hook,
        clear_interface: upgrade.clear_interface,
        interface: if upgrade.clear_interface {
            Some(&[])
        } else {
            interface.as_deref()
        },
        wasm: &wasm,
    }
    .encode()
    .map_err(|error| format!("invalid native upgrade: {error:?}"))?;
    submit_lifecycle(client, request, 2, &payload, previous_state_root)
}

pub fn wind_down(
    client: &Client,
    request: &CallRequest<'_>,
    operation: ProgramWindDownOperation<'_>,
    previous_state_root: &str,
) -> Result<Value, String> {
    let payload = NativeProgramWindDown {
        program_id: ProgramId::new(fixed_hex("program id", request.program_id)?),
        operation,
    }
    .encode()
    .map_err(|error| format!("invalid native wind-down: {error:?}"))?;
    submit_lifecycle(client, request, 7, &payload, previous_state_root)
}

fn read_program_file(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))
}

fn validate_interface(interface: Option<&[u8]>, wasm: &[u8], abi: u16) -> Result<(), String> {
    if let Some(bytes) = interface {
        let interface = layerx_programs::ProgramInterface::decode(bytes).map_err(|error| {
            format!("interface must be canonical encoded bytes, not KVX source: {error}")
        })?;
        let code_hash: [u8; 32] = Sha256::digest(wasm).into();
        if interface.code_hash() != code_hash || interface.abi_version() != abi {
            return Err("interface is bound to another code hash or guest ABI".into());
        }
    }
    Ok(())
}

fn artifact_abi(path: &Path) -> Result<u16, String> {
    let absolute = path
        .canonicalize()
        .map_err(|error| format!("could not resolve {}: {error}", path.display()))?;
    for directory in absolute.ancestors().skip(1) {
        let manifest = directory.join("LayerX.toml");
        let descriptor = directory.join(DESCRIPTOR);
        let mut declared = None;
        if manifest.is_file() {
            let source = fs::read_to_string(&manifest)
                .map_err(|error| format!("could not read {}: {error}", manifest.display()))?;
            let document = source
                .parse::<toml_edit::DocumentMut>()
                .map_err(|error| format!("invalid {}: {error}", manifest.display()))?;
            for key in ["abi_version", "abi"] {
                if let Some(value) = document.get(key) {
                    let abi = value
                        .as_integer()
                        .and_then(|value| u16::try_from(value).ok())
                        .ok_or_else(|| {
                            format!("{}: {key} must be an ABI integer", manifest.display())
                        })?;
                    merge_abi(&mut declared, abi)?;
                }
            }
        }
        if descriptor.is_file() {
            let document: Value = serde_json::from_slice(&read_program_file(&descriptor)?)
                .map_err(|error| format!("invalid {}: {error}", descriptor.display()))?;
            for key in ["abi_version", "abi"] {
                if let Some(value) = document.get(key) {
                    let abi = value
                        .as_u64()
                        .and_then(|value| u16::try_from(value).ok())
                        .ok_or_else(|| {
                            format!("{}: {key} must be an ABI integer", descriptor.display())
                        })?;
                    merge_abi(&mut declared, abi)?;
                }
            }
        }
        if manifest.is_file() || descriptor.is_file() {
            return Ok(declared.unwrap_or(2));
        }
    }
    Ok(2)
}

fn merge_abi(declared: &mut Option<u16>, abi: u16) -> Result<(), String> {
    if !matches!(abi, 1 | 2) {
        return Err(format!("unsupported guest ABI {abi}"));
    }
    if declared.is_some_and(|previous| previous != abi) {
        return Err("program manifests declare conflicting ABI versions".into());
    }
    *declared = Some(abi);
    Ok(())
}

fn submit_lifecycle(
    client: &Client,
    request: &CallRequest<'_>,
    ordinal: u16,
    payload: &[u8],
    previous_state_root: &str,
) -> Result<Value, String> {
    validate_idempotency_key(request.idempotency_key)?;
    let root = fixed_hex("previous state root", previous_state_root)?;
    let key = fixed_hex("sequencer public key", request.sequencer_public_key)?;
    let signed = signed_program_with_signer(request, ordinal, payload, || {
        Ok(SigningKey::from_bytes(&*crate::credential::key_seed(
            request.key_name,
        )?))
    })?;
    let route = match ordinal {
        1 => "/v1/programs/deploy",
        2 => "/v1/programs/upgrade",
        7 => "/v1/programs/wind-down",
        _ => return Err("unsupported lifecycle ordinal".into()),
    };
    let response = client.post_activity(route, &signed, Some(request.idempotency_key))?;
    refuse_transport_response(&response)?;
    let activity = validate_signed_program(ordinal, payload, &signed)?;
    let expected =
        activity_id(&activity).map_err(|error| format!("invalid activity: {error:?}"))?;
    if response
        .get("result")
        .unwrap_or(&response)
        .get("state")
        .and_then(Value::as_str)
        == Some("unknown")
    {
        return Ok(json!({"activity_id":hex_encode(&expected),
            "idempotency_key":request.idempotency_key, "signed_activity":hex_encode(&signed),
            "outcome":{"status":"unknown"}, "failure":response.get("failure")}));
    }
    verify_lifecycle_result(ordinal, expected, root, key, &response)
}

fn verify_lifecycle_result(
    ordinal: u16,
    activity: [u8; 32],
    previous_root: [u8; 32],
    sequencer_key: [u8; 32],
    response: &Value,
) -> Result<Value, String> {
    let result = response
        .get("result")
        .ok_or("lifecycle response omitted result envelope")?;
    let returned_id = fixed_hex::<32>(
        "activity id",
        result["activity_id"]
            .as_str()
            .ok_or("lifecycle response omitted activity id")?,
    )?;
    if returned_id != activity {
        return Err("lifecycle response names another activity".into());
    }
    let receipt_hex = result["receipt"]
        .as_str()
        .ok_or("lifecycle response omitted receipt")?;
    let bytes = hex_decode("receipt", receipt_hex)?;
    let receipt = layerx_proof::receipt::verify_sequencer_signature(&bytes, sequencer_key)
        .map_err(|failure| {
            format!(
                "lifecycle receipt verification failed at {:?}",
                failure.check
            )
        })?;
    let facts = receipt
        .protocol()
        .ok_or("lifecycle receipt omitted protocol facts")?;
    if facts.protocol_version() != 3
        || facts.module_id() != 9
        || facts.module_version() != 4
        || facts.operation() != 0
        || facts.activity_id() != activity
        || facts.previous_state_root() != previous_root
    {
        return Err("lifecycle receipt does not bind the requested protocol, operation, activity and prior root".into());
    }
    if !matches!(ordinal, 1 | 2 | 7) {
        return Err("unsupported lifecycle ordinal".into());
    }
    validate_lifecycle_state(result, facts.result_code())?;
    if facts.result_code() == 0 {
        let authority = layerx_proof::receipt::AuthorizedBatch::new(
            facts.batch_id(),
            facts.asset(),
            previous_root,
            facts.resulting_state_root(),
            sequencer_key,
        );
        layerx_proof::receipt::verify_program_state(&bytes, &authority).map_err(|failure| {
            format!("lifecycle state verification failed at {:?}", failure.check)
        })?;
    }
    Ok(
        json!({"activity_id":hex_encode(&activity), "receipt":receipt_hex,
        "result_code":facts.result_code(),
        "outcome":{"status":if facts.result_code() == 0 { "completed" } else { "refused" }},
        "verified_previous_state_root":hex_encode(&facts.previous_state_root()),
        "verified_resulting_state_root":hex_encode(&facts.resulting_state_root()),
        "verification":"canonical receipt, pinned sequencer signature, exact activity and prior state root verified locally"}),
    )
}

fn validate_lifecycle_state(result: &Value, result_code: i32) -> Result<(), String> {
    match result.get("state") {
        None => Ok(()),
        Some(Value::String(state))
            if (result_code == 0 && matches!(state.as_str(), "executed" | "completed"))
                || (result_code != 0 && state == "refused") =>
        {
            Ok(())
        }
        Some(_) => Err("lifecycle response state disagrees with its signed receipt".into()),
    }
}

fn refuse_transport_response(response: &Value) -> Result<(), String> {
    if response.get("state").and_then(Value::as_str) == Some("refused")
        && response.get("failure").is_some()
    {
        return Err(format!(
            "program submission refused before receipt acknowledgement: {}",
            response["failure"]
        ));
    }
    Ok(())
}

pub fn registry_get(client: &Client, program_id: &str) -> Result<Value, String> {
    validate_resource_id(program_id, "program id")?;
    let response = read_program_registry(client, program_id, false)?;
    let response = response.get("result").unwrap_or(&response).clone();
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

pub fn discover(client: &Client, program_id: &str) -> Result<Value, String> {
    validate_resource_id(program_id, "program id")?;
    let response = read_program_registry(client, program_id, false)?;
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
    let response = read_program_registry(client, program_id, true)?;
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
#[derive(clap::Args, Clone)]
pub struct NativeCallOptions {
    #[arg(
        long,
        default_value_t = 2,
        help = "Guest ABI; must match the authenticated deployed program head"
    )]
    pub abi_version: u16,
    #[arg(
        long,
        default_value = "layerx_call",
        help = "Exact native guest entrypoint"
    )]
    pub entrypoint: String,
    #[arg(
        long,
        help = "Canonical access-declaration hex, including its presence marker"
    )]
    pub access_declaration: Option<String>,
    #[arg(long, default_value_t = 1_048_576)]
    pub response_capacity: u32,
    #[arg(long, default_value_t = 16_777_216)]
    pub memory_bytes: u64,
    #[arg(long, default_value_t = 1_048_576)]
    pub storage_read_bytes: u64,
    #[arg(long, default_value_t = 1_048_576)]
    pub storage_write_bytes: u64,
    #[arg(long, default_value_t = 64)]
    pub output_values: u64,
    #[arg(long, default_value_t = 1_048_576)]
    pub output_bytes: u64,
    #[arg(long, default_value_t = 4096)]
    pub table_elements: u64,
}

impl Default for NativeCallOptions {
    fn default() -> Self {
        Self {
            abi_version: 2,
            entrypoint: "layerx_call".into(),
            access_declaration: None,
            response_capacity: 1_048_576,
            memory_bytes: 16_777_216,
            storage_read_bytes: 1_048_576,
            storage_write_bytes: 1_048_576,
            output_values: 64,
            output_bytes: 1_048_576,
            table_elements: 4096,
        }
    }
}

pub struct CallRequest<'a> {
    pub program_id: &'a str,
    pub calldata: &'a str,
    pub fuel: u64,
    pub native: NativeCallOptions,
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
    let payload = build_call(request)?;
    let signed = signed_call(request, &payload)?;
    let head = discover_call_head(client, request)?;
    let response =
        client.post_activity("/v1/programs/call", &signed, Some(request.idempotency_key))?;
    refuse_transport_response(&response)?;
    if response
        .get("result")
        .unwrap_or(&response)
        .get("state")
        .and_then(Value::as_str)
        == Some("unknown")
    {
        let registry = program_call_registry()?;
        let retained_activity = activity_id(
            &decode_signed(&signed, &registry)
                .map_err(|_| "retained signed call could not be decoded".to_owned())?,
        )
        .map_err(|_| "retained signed call has no canonical activity id".to_owned())?;
        return Ok(
            json!({"program_id":request.program_id,"idempotency_key":request.idempotency_key,
            "activity_id":hex_encode(&retained_activity),"signed_activity":hex_encode(&signed),"outcome":{"status":"unknown","retained_bytes":true},"failure":response.get("failure")}),
        );
    }
    let response = complete_call_response(client, &signed, &response)?;
    render_call_result(request, &payload, &signed, &head, &response)
}

fn complete_call_response(
    client: &Client,
    signed: &[u8],
    response: &Value,
) -> Result<Value, String> {
    let activity = decode_signed(signed, &program_call_registry()?)
        .map_err(|error| format!("invalid retained call: {error:?}"))?;
    let identifier =
        activity_id(&activity).map_err(|error| format!("invalid call identity: {error:?}"))?;
    let result = response.get("result").unwrap_or(response);
    let returned = fixed_hex::<32>(
        "call activity id",
        result["activity_id"]
            .as_str()
            .ok_or("call acknowledgement omitted activity id")?,
    )?;
    if returned != identifier {
        return Err("call acknowledgement names another signed activity".into());
    }
    if result.get("terminal_payload").is_some() && result.get("call_graph").is_some() {
        return Ok(result.clone());
    }
    let identifier = hex_encode(&identifier);
    let material = client.get_with_body(
        &format!("/v1/programs/activities/{identifier}"),
        &json!({"activity_id":identifier,"requested_verification_level":"sequencer-signed"}),
    )?;
    bind_execution_material(result, &material)
}

fn bind_execution_material(acknowledgement: &Value, material: &Value) -> Result<Value, String> {
    let material = material.get("result").unwrap_or(material);
    for field in ["activity_id", "receipt"] {
        let expected = acknowledgement[field]
            .as_str()
            .ok_or_else(|| format!("call acknowledgement omitted {field}"))?;
        if material[field].as_str() != Some(expected) {
            return Err(format!(
                "program execution material changed acknowledged {field}"
            ));
        }
    }
    if !material["terminal_payload"].is_string() || !material["call_graph"].is_string() {
        return Err("program execution material omitted terminal payload or call graph".into());
    }
    Ok(material.clone())
}

fn read_program_registry(
    client: &Client,
    program_id: &str,
    interface: bool,
) -> Result<Value, String> {
    let program = fixed_hex::<32>("program id", program_id)?;
    let program_id = hex_encode(&program);
    let suffix = if interface { "/interface" } else { "" };
    client.get_with_body(
        &format!("/v1/programs/registry/{program_id}{suffix}"),
        &json!({"program_id":program_id,"requested_verification_level":"sequencer-signed"}),
    )
}

pub fn simulate(client: &Client, request: &CallRequest<'_>) -> Result<Value, String> {
    let payload = build_call(request)?;
    let signed = signed_call(request, &payload)?;
    let head = discover_call_head(client, request)?;
    let response = client.post_activity("/v1/programs/simulate", &signed, None)?;
    let result = response.get("result").unwrap_or(&response);
    if result["committed"].as_bool() != Some(false) {
        return Err("program simulation did not prove that it committed nothing".to_owned());
    }
    verify_simulation_evidence(request, &signed, &head, result)?;
    let execution = result
        .get("execution")
        .ok_or_else(|| "program simulation omitted its execution document".to_owned())?;
    let mut rendered = render_call_result(request, &payload, &signed, &head, execution)?;
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
    signed_program_with_signer(request, 3, canonical_payload, signer)
}

fn signed_program_with_signer(
    request: &CallRequest<'_>,
    ordinal: u16,
    canonical_payload: &[u8],
    load_key: impl FnOnce() -> Result<SigningKey, String>,
) -> Result<Vec<u8>, String> {
    validate_program_payload(ordinal, canonical_payload)?;
    if request.expires_at_ms <= request.not_before_ms
        || request.expires_at_ms - request.not_before_ms > 300_000
    {
        return Err(
            "program call validity must be non-empty and no wider than 300000 milliseconds"
                .to_owned(),
        );
    }
    let idempotency = fixed_hex::<32>("idempotency key", request.idempotency_key)?;
    let activity_type = ActivityType::new(ModuleId::Programs, ordinal)
        .map_err(|error| format!("program call activity is unavailable: {error:?}"))?;
    let registry = program_call_registry()?;
    let payload = Payload::new(&registry, activity_type, canonical_payload)
        .map_err(|error| format!("program call payload is invalid: {error:?}"))?;
    let payload_hash = payload_hash_for(&payload)
        .map_err(|error| format!("program payload hash is invalid: {error:?}"))?;
    let signing_key = load_key()?;
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
        .protocol_version(layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION)
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
    let activities = [1, 2, 3, 7]
        .into_iter()
        .map(|ordinal| {
            ActivityType::new(ModuleId::Programs, ordinal)
                .map_err(|error| format!("program activity unavailable: {error:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let registration = ModuleRegistration::new(ModuleId::Programs, &activities)
        .map_err(|error| format!("program module registration is invalid: {error:?}"))?;
    ModuleRegistry::new(&[registration])
        .map_err(|error| format!("program module registry is invalid: {error:?}"))
}

fn validate_program_payload(ordinal: u16, payload: &[u8]) -> Result<(), String> {
    let reproduced = match ordinal {
        1 => {
            let operation = NativeProgramDeploy::decode(payload)
                .map_err(|error| format!("invalid deployment: {error:?}"))?;
            let hash: [u8; 32] = Sha256::digest(operation.wasm).into();
            if hash != operation.new_hash {
                return Err("deployment code hash mismatch".into());
            }
            operation
                .encode()
                .map_err(|error| format!("invalid deployment: {error:?}"))?
        }
        2 => {
            let operation = NativeProgramUpgrade::decode(payload)
                .map_err(|error| format!("invalid upgrade: {error:?}"))?;
            let hash: [u8; 32] = Sha256::digest(operation.wasm).into();
            if hash != operation.new_hash {
                return Err("upgrade code hash mismatch".into());
            }
            operation
                .encode()
                .map_err(|error| format!("invalid upgrade: {error:?}"))?
        }
        3 => NativeProgramCall::decode(payload)
            .and_then(|operation| operation.encode())
            .map_err(|error| format!("invalid native call: {error:?}"))?,
        7 => NativeProgramWindDown::decode(payload)
            .and_then(|operation| operation.encode())
            .map_err(|error| format!("invalid wind-down: {error:?}"))?,
        _ => return Err("unsupported Programs activity ordinal".into()),
    };
    if reproduced != payload {
        return Err("noncanonical Programs payload".into());
    }
    Ok(())
}

fn build_call(request: &CallRequest<'_>) -> Result<Vec<u8>, String> {
    if request.fuel == 0 {
        return Err("declared call fuel must be greater than zero".into());
    }
    let calldata = if request.calldata.is_empty() {
        Vec::new()
    } else {
        hex_decode("calldata", request.calldata)?
    };
    let grants = request
        .capabilities
        .iter()
        .map(|value| parse_capability(value))
        .collect::<Result<Vec<_>, _>>()?;
    let capabilities = CapabilitySet::new(grants)
        .map_err(|error| format!("invalid capability set: {error:?}"))?
        .canonical_encoding();
    let access = match &request.native.access_declaration {
        Some(encoded) => {
            let bytes = hex_decode("access declaration", encoded)?;
            AccessDeclaration::canonical_decode(&bytes)
                .map_err(|error| format!("invalid access declaration: {error:?}"))?;
            bytes
        }
        None => AccessDeclaration::absent()
            .canonical_bytes()
            .map_err(|error| format!("invalid access declaration: {error:?}"))?,
    };
    NativeProgramCall {
        program_id: ProgramId::new(fixed_hex("program id", request.program_id)?),
        guest_abi: request.native.abi_version,
        entrypoint: request.native.entrypoint.as_bytes(),
        calldata: &calldata,
        capabilities: &capabilities,
        access_declaration: &access,
        response_capacity: request.native.response_capacity,
        resources: Resources([
            request.fuel,
            request.native.memory_bytes,
            request.native.storage_read_bytes,
            request.native.storage_write_bytes,
            request.native.output_values,
            request.native.output_bytes,
            request.native.table_elements,
        ]),
    }
    .encode()
    .map_err(|error| format!("invalid native call: {error:?}"))
}

fn parse_capability(value: &str) -> Result<Capability, String> {
    let fields = value.split(':').collect::<Vec<_>>();
    let amount = |encoded: &str| {
        encoded
            .parse::<u128>()
            .map_err(|_| "capability maximum must be an unsigned 128-bit integer".to_owned())
    };
    let program = |encoded: &str| {
        layerx_programs_runtime::storage::ProgramId::new(fixed_hex("capability program", encoded)?)
            .map_err(|error| format!("invalid capability program: {error:?}"))
    };
    match fields.as_slice() {
        ["storage-read"] => Ok(Capability::StorageRead),
        ["storage-write"] => Ok(Capability::StorageWrite),
        ["shared-storage-read"] => Ok(Capability::SharedStorageRead),
        ["shared-storage-write"] => Ok(Capability::SharedStorageWrite),
        ["emit-event"] => Ok(Capability::EmitEvent),
        ["call", callee] => Ok(Capability::Call {
            program: program(callee)?,
        }),
        ["transfer402", asset, to, maximum] => Ok(Capability::Transfer402 {
            asset: fixed_hex("asset", asset)?,
            to: fixed_hex("recipient", to)?,
            maximum_amount: amount(maximum)?,
        }),
        ["receipt-read", digest] => Ok(Capability::ReceiptRead {
            receipt_digest: fixed_hex("receipt digest", digest)?,
        }),
        ["program-spend", owner, seed, source, asset, to, maximum] => {
            Ok(Capability::ProgramSpend {
                owner_program: program(owner)?,
                seed: if seed.is_empty() {
                    Vec::new()
                } else {
                    hex_decode("account seed", seed)?
                },
                source_account: fixed_hex("source account", source)?,
                asset: fixed_hex("asset", asset)?,
                to: fixed_hex("recipient", to)?,
                maximum_amount: amount(maximum)?,
            })
        }
        ["balance-view", account, asset, digest] => Ok(Capability::BalanceView {
            account: fixed_hex("account", account)?,
            asset: fixed_hex("asset", asset)?,
            receipt_digest: fixed_hex("receipt digest", digest)?,
        }),
        _ => Err(format!(
            "invalid capability {value}; use a native scoped grant (see Programs guide)"
        )),
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
    if protocol.activity_id() != expected_activity
        || protocol.protocol_version() != 3
        || protocol.module_version() != 4
    {
        return Err("program receipt names a different signed activity".to_owned());
    }
    let receipt_digest = verified
        .evidence()
        .receipt_digest()
        .ok_or_else(|| "program receipt verifier produced no digest".to_owned())?;
    let program = protocol
        .program_outcome()
        .ok_or_else(|| "verified receipt omitted its Programs outcome".to_owned())?;
    if program.abi_version() != head.abi_version
        || program.abi_version() != request.native.abi_version
    {
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
        "protocol_version": 3,
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
    let response = read_program_registry(client, request.program_id, false)?;
    let result = response.get("result").unwrap_or(&response);
    if result
        .get("program_id")
        .and_then(Value::as_str)
        .is_none_or(|program| !program.eq_ignore_ascii_case(request.program_id))
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
    let observed_sequence = canonical_u64(result, "observed_sequence")?;
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
    let observed_at = canonical_u64(result, "observed_at")?;
    let valid_through = canonical_u64(result, "valid_through")?;
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
        || canonical_u64(evidence, "observed_sequence")? != head.observed_sequence
        || canonical_u64(evidence, "observed_at")? != head.observed_at
    {
        return Err("program simulation evidence does not extend verified discovery".to_owned());
    }
    let activity = validate_signed_call(&build_call(request)?, signed_activity)?;
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
            .get("execution")
            .ok_or("program simulation omitted execution")?
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

fn canonical_u64(document: &Value, field: &str) -> Result<u64, String> {
    match document.get(field) {
        Some(Value::Number(value)) => value.as_u64(),
        Some(Value::String(value))
            if !value.is_empty()
                && value.bytes().all(|byte| byte.is_ascii_digit())
                && (value.len() == 1 || !value.starts_with('0')) =>
        {
            value.parse().ok()
        }
        _ => None,
    }
    .ok_or_else(|| format!("{field} must be a canonical unsigned 64-bit integer"))
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
    validate_signed_program(3, payload, signed_activity)
}

fn validate_signed_program(
    ordinal: u16,
    payload: &[u8],
    signed_activity: &[u8],
) -> Result<layerx_wire::activity::Activity, String> {
    validate_program_payload(ordinal, payload)?;
    let activity = decode_signed(signed_activity, &program_call_registry()?)
        .map_err(|error| format!("signed program activity is invalid: {error:?}"))?;
    if activity.protocol_version() != 3
        || activity.activity_type().module() != ModuleId::Programs
        || activity.activity_type().ordinal() != ordinal
        || activity.payload() != payload
    {
        return Err("signed activity does not carry this exact native Programs payload".into());
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
        build_call, classify_outcome, render_call_result, signed_call_with_signer,
        validate_signed_call, CallRequest, VerifiedCallHead,
    };
    use crate::encoding::hex_encode;
    use ed25519_dalek::SigningKey;
    use layerx_types::amount::Amount;
    use layerx_types::intent::{
        CallBudget, Calldata, CapabilityRequest, ProgramCall, ProgramId, RequestedCapabilities,
    };
    use serde_json::json;

    const GOLDEN_PAYLOAD_HEX: &str = "4c61796572582f70726f6772616d732f63616c6c2f763100111111111111111111111111111111111111111111111111111111111111111100000000000003e8000000000000000000000000000000fa0002010300000002aabb";

    fn golden_request() -> CallRequest<'static> {
        CallRequest {
            program_id: "1111111111111111111111111111111111111111111111111111111111111111",
            calldata: "aabb",
            fuel: 1000,
            native: super::NativeCallOptions::default(),
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
    fn cli_encodes_native_call_and_preserves_legacy_agent_layout() {
        let capabilities = ["emit-event".to_string(), "storage-read".to_string()];
        let request = CallRequest {
            program_id: "1111111111111111111111111111111111111111111111111111111111111111",
            calldata: "aabb",
            fuel: 1000,
            native: super::NativeCallOptions::default(),
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
        assert_eq!(
            hex_encode(&agent_layer_call().canonical_payload()),
            GOLDEN_PAYLOAD_HEX
        );
        let native = super::NativeProgramCall::decode(&built)
            .unwrap_or_else(|error| panic!("native call rejected: {error:?}"));
        assert_eq!(native.guest_abi, 2);
        assert_eq!(native.entrypoint, b"layerx_call");
        assert_eq!(native.calldata, &[0xaa, 0xbb]);
        assert_eq!(native.capabilities, &[0, 2, 1, 3]);
        assert_eq!(native.resources.0[0], 1000);
        assert_eq!(&built[106..117], b"layerx_call");
        assert_ne!(built, agent_layer_call().canonical_payload());
    }

    #[test]
    fn non_canonical_call_payload_has_an_explicit_cli_refusal() {
        assert!(
            super::validate_program_payload(3, &agent_layer_call().canonical_payload()).is_err()
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
        let payload = build_call(&request).unwrap_or_else(|error| panic!("{error}"));
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
        let payload = build_call(&request).unwrap_or_else(|error| panic!("{error}"));
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
        let call_a = build_call(&request).unwrap_or_else(|error| panic!("{error}"));
        let mut call_b = call_a.clone();
        let last = 117;
        call_b[last] ^= 1;
        let signed_b =
            signed_call_with_signer(&request, &call_b, || Ok(SigningKey::from_bytes(&[7; 32])))
                .unwrap_or_else(|error| panic!("source vector signing failed: {error}"));
        assert!(validate_signed_call(&call_a, &signed_b).is_err());
    }

    #[test]
    fn signing_uses_protocol_three_and_exact_native_payload() -> Result<(), String> {
        use ed25519_dalek::Verifier as _;
        let request = golden_request();
        let payload = build_call(&request)?;
        let key = SigningKey::from_bytes(&[7; 32]);
        let signed = signed_call_with_signer(&request, &payload, || Ok(key.clone()))?;
        let activity = validate_signed_call(&payload, &signed)?;
        assert_eq!(activity.protocol_version(), 3);
        assert_eq!(activity.network_id(), request.network_id);
        assert_eq!(activity.activity_type().ordinal(), 3);
        assert_eq!(activity.payload(), payload);
        let preimage =
            layerx_wire::sign::preimage(&activity).map_err(|error| format!("{error:?}"))?;
        let signature =
            ed25519_dalek::Signature::from_slice(activity.signature().ok_or("missing signature")?)
                .map_err(|error| error.to_string())?;
        key.verifying_key()
            .verify(preimage.as_bytes(), &signature)
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn c_lifecycle_fixtures_sign_exactly_and_refuse_wrong_ordinal() -> Result<(), String> {
        let request = golden_request();
        for (name, ordinal) in [
            ("deploy", 1),
            ("upgrade", 2),
            ("wind-down-route", 7),
            ("wind-down-deprecate", 7),
            ("wind-down-tombstone", 7),
            ("wind-down-exit", 7),
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../sdk/conformance/fixtures")
                .join(format!("native-program-{name}-v3.json"));
            let fixture: serde_json::Value =
                serde_json::from_slice(&super::read_program_file(&path)?)
                    .map_err(|error| error.to_string())?;
            let payload = crate::encoding::hex_decode(
                "C payload",
                fixture["payload_hex"].as_str().ok_or("missing C payload")?,
            )?;
            let canonical = crate::encoding::hex_decode(
                "C signed activity",
                fixture["signed_activity_hex"]
                    .as_str()
                    .ok_or("missing C signed activity")?,
            )?;
            let decoded = super::validate_signed_program(ordinal, &payload, &canonical)?;
            assert_eq!(decoded.network_id(), 7);
            assert_eq!(decoded.actor_did(), b"did:lxp:native-lifecycle-fixture");
            assert_eq!(
                hex_encode(&decoded.idempotency_key()),
                fixture["idempotency_key_hex"]
                    .as_str()
                    .ok_or("missing C idempotency key")?
            );
            assert_eq!(
                hex_encode(&super::activity_id(&decoded).map_err(|error| format!("{error:?}"))?),
                fixture["activity_id_hex"]
                    .as_str()
                    .ok_or("missing C activity id")?
            );
            let c_key = fixture["idempotency_key_hex"]
                .as_str()
                .ok_or("missing C key")?;
            let c_request = CallRequest {
                network_id: 7,
                actor_did: "did:lxp:native-lifecycle-fixture",
                fee_limit: "1000",
                account_sequence: 0,
                not_before_ms: 1,
                expires_at_ms: 100,
                idempotency_key: c_key,
                ..golden_request()
            };
            let mut seed = [0; 32];
            seed[0] = 0x33;
            let fixture_key = SigningKey::from_bytes(&seed);
            assert_eq!(
                hex_encode(&fixture_key.verifying_key().to_bytes()),
                fixture["public_key_hex"]
                    .as_str()
                    .ok_or("missing C public key")?
            );
            let reproduced =
                super::signed_program_with_signer(&c_request, ordinal, &payload, || {
                    Ok(fixture_key)
                })?;
            assert_eq!(reproduced, canonical);
            let signed = super::signed_program_with_signer(&request, ordinal, &payload, || {
                Ok(SigningKey::from_bytes(&[7; 32]))
            })?;
            let activity = super::validate_signed_program(ordinal, &payload, &signed)?;
            assert_eq!(activity.protocol_version(), 3);
            assert_eq!(activity.activity_type().ordinal(), ordinal);
            assert_eq!(activity.payload(), payload);
            assert!(super::validate_signed_program(3, &payload, &signed).is_err());
            for length in 0..payload.len() {
                assert!(super::validate_program_payload(ordinal, &payload[..length]).is_err());
            }
            let mut trailing = payload.clone();
            trailing.push(0);
            assert!(super::validate_program_payload(ordinal, &trailing).is_err());
            if ordinal != 7 {
                let mut bad_hash = payload;
                bad_hash[68] ^= 1;
                assert!(super::validate_program_payload(ordinal, &bad_hash).is_err());
            }
        }
        Ok(())
    }

    #[test]
    fn call_bounds_and_ambiguous_legacy_capabilities_are_refused() {
        let mut request = golden_request();
        request.native.response_capacity = 1_048_577;
        assert!(build_call(&request).is_err());
        request.native = super::NativeCallOptions::default();
        request.native.entrypoint = "not-an-entrypoint".into();
        assert!(build_call(&request).is_err());
        request.native = super::NativeCallOptions::default();
        request.native.access_declaration = Some("00".into());
        assert!(build_call(&request).is_err());
        for name in ["transfer", "compose", "unknown", "transfer402:00:00:1"] {
            assert!(super::parse_capability(name).is_err());
        }
        request.native = super::NativeCallOptions::default();
        request.fuel = 0;
        assert!(build_call(&request).is_err());
        let duplicates = vec!["storage-read".into(), "storage-read".into()];
        request.fuel = 1;
        request.capabilities = &duplicates;
        assert!(build_call(&request).is_err());
        let payload = vec![0; layerx_wire::limits::MAX_MESSAGE_BYTES + 1];
        assert!(
            super::signed_program_with_signer(&request, 1, &payload, || Ok(
                SigningKey::from_bytes(&[7; 32])
            ))
            .is_err()
        );
    }

    #[test]
    fn lifecycle_response_requires_exact_envelope_and_verified_receipt() {
        let response = json!({"result":{"activity_id":hex_encode(&[1; 32]),"receipt":"aabb"}});
        assert!(super::verify_lifecycle_result(1, [1; 32], [2; 32], [3; 32], &response).is_err());
        assert!(super::verify_lifecycle_result(1, [4; 32], [2; 32], [3; 32], &response).is_err());
        let bare = json!({"activity_id":hex_encode(&[1; 32]),"receipt":"aabb"});
        assert!(super::verify_lifecycle_result(1, [1; 32], [2; 32], [3; 32], &bare).is_err());
    }

    #[test]
    fn lifecycle_transport_state_is_consistent_with_receipt_result() {
        for state in ["executed", "completed"] {
            assert!(super::validate_lifecycle_state(&json!({"state":state}), 0).is_ok());
            assert!(super::validate_lifecycle_state(&json!({"state":state}), -1).is_err());
        }
        assert!(super::validate_lifecycle_state(&json!({"state":"refused"}), -1).is_ok());
        assert!(super::validate_lifecycle_state(&json!({"state":"refused"}), 0).is_err());
        assert!(super::validate_lifecycle_state(&json!({"state":"unknown"}), 0).is_err());
        assert!(super::validate_lifecycle_state(&json!({"state":null}), 0).is_err());
        assert!(super::validate_lifecycle_state(&json!({}), 0).is_ok());
    }

    #[test]
    fn canonical_integer_transport_forms_preserve_exact_u64_values() -> Result<(), String> {
        for value in [0, 1, u64::MAX] {
            assert_eq!(
                super::canonical_u64(&json!({"sequence":value}), "sequence")?,
                value
            );
            assert_eq!(
                super::canonical_u64(&json!({"sequence":value.to_string()}), "sequence")?,
                value
            );
        }
        for value in ["", "-1", "+1", "01", "1.0", "18446744073709551616", " 1"] {
            assert!(super::canonical_u64(&json!({"sequence":value}), "sequence").is_err());
        }
        assert!(super::canonical_u64(&json!({"sequence":-1}), "sequence").is_err());
        assert!(super::canonical_u64(&json!({}), "sequence").is_err());
        Ok(())
    }

    #[test]
    fn pre_receipt_refusals_retain_the_transport_reason() {
        let response = json!({"state":"refused","failure":{"http_status":400,
            "response":{"error":{"code":"program_payload_hash_mismatch"}}}});
        let error = super::refuse_transport_response(&response)
            .err()
            .unwrap_or_else(|| panic!("transport refusal accepted"));
        assert!(error.contains("program_payload_hash_mismatch"));
        assert!(error.contains("400"));
    }

    #[test]
    fn separate_execution_material_is_bound_to_the_acknowledged_receipt() -> Result<(), String> {
        let acknowledgement = json!({"activity_id":hex_encode(&[1; 32]), "receipt":"aabb"});
        let material = json!({"result":{
            "activity_id":hex_encode(&[1; 32]), "receipt":"aabb",
            "terminal_payload":"cc", "call_graph":"dd",
        }});
        assert_eq!(
            super::bind_execution_material(&acknowledgement, &material)?,
            material["result"]
        );
        let mut changed = material.clone();
        changed["result"]["receipt"] = json!("aabc");
        assert!(super::bind_execution_material(&acknowledgement, &changed).is_err());
        changed = material.clone();
        changed["result"]["activity_id"] = json!(hex_encode(&[2; 32]));
        assert!(super::bind_execution_material(&acknowledgement, &changed).is_err());
        changed = material;
        changed["result"]["terminal_payload"] = serde_json::Value::Null;
        assert!(super::bind_execution_material(&acknowledgement, &changed).is_err());
        Ok(())
    }

    #[test]
    fn interface_source_is_not_accepted_as_canonical_interface_bytes() {
        let source = include_bytes!("../../../programs/sdk/rust/examples/escrow/interface.kvx");
        assert!(super::validate_interface(Some(source), b"\0asm\x01\0\0\0", 2).is_err());
    }

    #[test]
    fn scoped_capabilities_preserve_all_authority_fields() -> Result<(), String> {
        let identifier = hex_encode(&[1; 32]);
        let asset = hex_encode(&[2; 32]);
        let recipient = hex_encode(&[3; 32]);
        let digest = hex_encode(&[4; 32]);
        let transfer = super::parse_capability(&format!("transfer402:{asset}:{recipient}:17"))?;
        assert_eq!(
            transfer,
            super::Capability::Transfer402 {
                asset: [2; 32],
                to: [3; 32],
                maximum_amount: 17,
            }
        );
        let spend = super::parse_capability(&format!(
            "program-spend:{identifier}:aabb:{identifier}:{asset}:{recipient}:19"
        ))?;
        match spend {
            super::Capability::ProgramSpend {
                owner_program,
                seed,
                source_account,
                asset,
                to,
                maximum_amount,
            } => {
                assert_eq!(owner_program.bytes(), [1; 32]);
                assert_eq!(seed, [0xaa, 0xbb]);
                assert_eq!(source_account, [1; 32]);
                assert_eq!(asset, [2; 32]);
                assert_eq!(to, [3; 32]);
                assert_eq!(maximum_amount, 19);
            }
            _ => return Err("wrong capability variant".into()),
        }
        assert_eq!(
            super::parse_capability(&format!("balance-view:{identifier}:{asset}:{digest}"))?,
            super::Capability::BalanceView {
                account: [1; 32],
                asset: [2; 32],
                receipt_digest: [4; 32],
            }
        );
        Ok(())
    }

    #[test]
    fn abi_configuration_defaults_to_two_and_refuses_conflicts() -> Result<(), String> {
        let directory = std::env::temp_dir().join(format!("layerx-cli-abi-{}", std::process::id()));
        std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
        let result = (|| {
            let artifact = directory.join("program.wasm");
            std::fs::write(&artifact, b"\0asm\x01\0\0\0").map_err(|error| error.to_string())?;
            assert_eq!(super::artifact_abi(&artifact)?, 2);
            let manifest = directory.join("LayerX.toml");
            std::fs::write(&manifest, "abi = 1\n").map_err(|error| error.to_string())?;
            assert_eq!(super::artifact_abi(&artifact)?, 1);
            let descriptor = directory.join(super::DESCRIPTOR);
            std::fs::write(&descriptor, r#"{"abi_version":2}"#)
                .map_err(|error| error.to_string())?;
            assert!(super::artifact_abi(&artifact).is_err());
            std::fs::write(&manifest, "abi_version = 2\n").map_err(|error| error.to_string())?;
            assert_eq!(super::artifact_abi(&artifact)?, 2);
            std::fs::write(&manifest, "abi = 1\nabi_version = 2\n")
                .map_err(|error| error.to_string())?;
            assert!(super::artifact_abi(&artifact).is_err());
            std::fs::write(&manifest, "abi = 2\nabi_version = 2\n")
                .map_err(|error| error.to_string())?;
            assert_eq!(super::artifact_abi(&artifact)?, 2);
            std::fs::write(&descriptor, r#"{"abi_version":"2"}"#)
                .map_err(|error| error.to_string())?;
            assert!(super::artifact_abi(&artifact).is_err());
            std::fs::write(&descriptor, r#"{"abi_version":4}"#)
                .map_err(|error| error.to_string())?;
            assert!(super::artifact_abi(&artifact).is_err());
            Ok(())
        })();
        std::fs::remove_dir_all(&directory).map_err(|error| error.to_string())?;
        result
    }
}
