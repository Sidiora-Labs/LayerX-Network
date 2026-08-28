use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use layerx_programs_runtime::WasmEngine;
use layerx_types::amount::Amount;
use layerx_types::intent::{
    CallBudget, Calldata, CapabilityRequest, ProgramCall, ProgramCallError, ProgramId,
    RequestedCapabilities, PROGRAM_CALL_CONTRACT_MAJOR,
};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use crate::encoding::{fixed_hex, hex_decode, hex_encode};
use crate::http::{validate_idempotency_key, validate_resource_id, Client};

const DESCRIPTOR: &str = "layerx-program.json";

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
        format!("could not create binding directory {}: {error}", output.display())
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
    let body = json!({
        "program_id": request.program_id,
        "calldata": hex_encode(operation.calldata().as_bytes()),
        "budget": {
            "fuel": request.fuel,
            "fee_limit": request.fee_limit,
        },
        "capabilities": capability_names(operation.capabilities()),
        "canonical_payload": hex_encode(&payload),
        "contract_major": PROGRAM_CALL_CONTRACT_MAJOR,
    });
    let response = client.post("/v1/programs/call", &body, Some(request.idempotency_key))?;
    render_call_result(request, &payload, &response)
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

fn capability_names(capabilities: &RequestedCapabilities) -> Vec<&'static str> {
    capabilities
        .as_slice()
        .iter()
        .map(|capability| match capability {
            CapabilityRequest::StorageRead => "storage-read",
            CapabilityRequest::StorageWrite => "storage-write",
            CapabilityRequest::Transfer => "transfer",
            CapabilityRequest::EmitEvent => "emit-event",
            CapabilityRequest::Compose => "compose",
        })
        .collect()
}

const fn describe_call_error(error: ProgramCallError) -> &'static str {
    match error {
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
    let receipt_digest: [u8; 32] = Sha256::digest(&receipt_bytes).into();
    let result_code = result
        .get("result_code")
        .and_then(Value::as_i64)
        .ok_or_else(|| "program-call response omitted the receipt result code".to_string())?;
    let outcome = classify_outcome(result, result_code)?;
    Ok(json!({
        "program_id": request.program_id,
        "idempotency_key": request.idempotency_key,
        "canonical_payload": hex_encode(payload),
        "contract_major": PROGRAM_CALL_CONTRACT_MAJOR,
        "receipt": receipt_hex,
        "receipt_digest": hex_encode(&receipt_digest),
        "result_code": result_code,
        "outcome": outcome,
        "verification": "the typed outcome was re-bound to the returned receipt; run layerx receipt verify with batch authority for full checkpoint verification",
    }))
}

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
    use super::{build_call, classify_outcome, render_call_result, CallRequest};
    use crate::encoding::hex_encode;
    use layerx_types::amount::Amount;
    use layerx_types::intent::{
        CallBudget, Calldata, CapabilityRequest, ProgramCall, ProgramId, RequestedCapabilities,
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
            idempotency_key: "call-idem-key-0001",
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
            idempotency_key: "call-idem-key-0001",
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
    fn render_binds_the_typed_result_to_the_returned_receipt() {
        let request = golden_request();
        let payload = agent_layer_call().canonical_payload();
        let response = json!({
            "result": {
                "receipt": "aabbccdd",
                "result_code": 0,
                "outcome": {"status": "completed", "code": 0, "response": "aabb"},
            }
        });
        let Ok(rendered) = render_call_result(&request, &payload, &response) else {
            panic!("valid program-call response rejected");
        };
        assert_eq!(rendered["result_code"], 0);
        assert_eq!(rendered["outcome"]["status"], "completed");
        assert_eq!(rendered["receipt"], "aabbccdd");
        assert!(rendered["receipt_digest"].is_string());
        assert_eq!(rendered["canonical_payload"], hex_encode(&payload));
    }

    #[test]
    fn render_refuses_a_response_without_a_receipt() {
        let request = golden_request();
        let payload = agent_layer_call().canonical_payload();
        let response = json!({"result": {"result_code": 0}});
        assert!(render_call_result(&request, &payload, &response).is_err());
    }
}
