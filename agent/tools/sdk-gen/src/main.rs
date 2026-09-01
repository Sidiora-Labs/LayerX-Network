use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const TYPESCRIPT_TEMPLATE: &str = include_str!("../templates/typescript.tpl");
const PYTHON_TEMPLATE: &str = include_str!("../templates/python.tpl");
const PYTHON_STUB_TEMPLATE: &str = include_str!("../templates/python.pyi.tpl");
const GUARANTEES_TEMPLATE: &str = include_str!("../templates/guarantees.md.tpl");
const COMPATIBILITY_TEMPLATE: &str = include_str!("../templates/compatibility.md.tpl");

#[derive(Clone, Debug, Eq, PartialEq)]
struct Scalar {
    name: String,
    rust: String,
    typescript: String,
    python: String,
    wire: String,
    consensus_integer: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Model {
    scalars: Vec<Scalar>,
    operations: Vec<String>,
    levels: Vec<String>,
    errors: Vec<String>,
    guarantees: Vec<(String, String, String)>,
    compatibility: Compatibility,
    approval: Approval,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Compatibility {
    sdk: String,
    contract: String,
    node_interface: String,
    daemon: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Approval {
    states: Vec<String>,
    outcomes: Vec<String>,
    events: Vec<String>,
    introduced_contract: String,
    enforcement_notice: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Generated {
    pub files: BTreeMap<PathBuf, String>,
}

fn section_map(source: &str) -> Result<BTreeMap<String, String>, String> {
    let mut section = String::new();
    let mut entries = BTreeMap::new();
    for (number, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            name.clone_into(&mut section);
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {} is not a key/value declaration",
                number + 1
            ));
        };
        if section.is_empty() {
            return Err(format!("line {} is outside a section", number + 1));
        }
        let path = format!("{}.{}", section, key.trim());
        if entries
            .insert(path.clone(), value.trim().to_owned())
            .is_some()
        {
            return Err(format!("duplicate schema declaration {path}"));
        }
    }
    Ok(entries)
}

fn sections(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
                .map(str::to_owned)
        })
        .collect()
}

fn unquote(value: &str) -> Result<String, String> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_owned)
        .ok_or_else(|| format!("expected quoted value, got {value}"))
}

fn quoted_list(value: &str) -> Result<Vec<String>, String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| format!("expected list, got {value}"))?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    inner.split(',').map(|item| unquote(item.trim())).collect()
}

fn required<'a>(entries: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    entries
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("missing {key}"))
}

fn read_schema(schema_root: &Path) -> Result<Model, String> {
    let root_source = fs::read_to_string(schema_root.join("v1.kvx"))
        .map_err(|error| format!("read v1.kvx: {error}"))?;
    let root = section_map(&root_source)?;
    let includes = quoted_list(required(&root, "schema.includes")?)?;
    let mut module_sources = Vec::new();
    let mut module_entries = Vec::new();
    for include in &includes {
        let source = fs::read_to_string(schema_root.join(include))
            .map_err(|error| format!("read {include}: {error}"))?;
        module_entries.push(section_map(&source)?);
        module_sources.push(source);
    }

    let scalar_names = sections(&root_source)
        .into_iter()
        .filter_map(|section| section.strip_prefix("scalar.").map(str::to_owned))
        .collect::<Vec<_>>();
    let mut scalars = Vec::new();
    for name in scalar_names {
        let prefix = format!("scalar.{name}");
        scalars.push(Scalar {
            name,
            rust: unquote(required(&root, &format!("{prefix}.rust"))?)?,
            typescript: unquote(required(&root, &format!("{prefix}.typescript"))?)?,
            python: unquote(required(&root, &format!("{prefix}.python"))?)?,
            wire: unquote(required(&root, &format!("{prefix}.wire"))?)?,
            consensus_integer: required(&root, &format!("{prefix}.consensus_integer"))? == "true",
        });
    }

    let operations = module_sources
        .iter()
        .flat_map(|source| sections(source))
        .filter_map(|section| section.strip_prefix("operation.").map(str::to_owned))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let combined = module_entries
        .iter()
        .flat_map(|entries| {
            entries
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let levels = quoted_list(required(&combined, "type.Level.variants")?)?;
    let errors = quoted_list(required(&combined, "type.ErrorClass.variants")?)?;
    let protocol = unquote(required(
        &combined,
        "type.BudgetEnforcement.ProtocolBudget.guarantee",
    )?)?;
    let daemon = unquote(required(
        &combined,
        "type.BudgetEnforcement.DaemonLimit.guarantee",
    )?)?;
    let notice = unquote(required(
        &combined,
        "type.BudgetEnforcement.DaemonLimit.notice",
    )?)?;
    let approval_enforcement = unquote(required(&combined, "guarantee.ApprovalHold.enforcement")?)?;
    let approval_notice = unquote(required(&combined, "guarantee.ApprovalHold.notice")?)?;
    let model = Model {
        scalars,
        operations,
        levels,
        errors,
        guarantees: vec![
            (
                "ProtocolBudget".to_owned(),
                protocol,
                "Enforced by the LayerX protocol state machine.".to_owned(),
            ),
            ("DaemonLimit".to_owned(), daemon, notice),
            (
                "ApprovalHold".to_owned(),
                approval_enforcement,
                approval_notice.clone(),
            ),
        ],
        compatibility: Compatibility {
            sdk: unquote(required(&root, "compatibility.matrix.sdk")?)?,
            contract: unquote(required(&root, "compatibility.matrix.contract")?)?,
            node_interface: unquote(required(&root, "compatibility.matrix.node_interface")?)?,
            daemon: unquote(required(&root, "compatibility.matrix.daemon")?)?,
        },
        approval: Approval {
            states: quoted_list(required(&combined, "type.ApprovalState.variants")?)?,
            outcomes: quoted_list(required(
                &combined,
                "type.ApprovalDecisionOutcome.variants",
            )?)?,
            events: quoted_list(required(&combined, "type.ApprovalLifecycleEvent.variants")?)?,
            introduced_contract: unquote(required(
                &root,
                "compatibility.feature.approval.introduced_contract",
            )?)?,
            enforcement_notice: approval_notice,
        },
    };
    validate_model(&model)?;
    Ok(model)
}

fn validate_model(model: &Model) -> Result<(), String> {
    if model.scalars.is_empty() || model.operations.is_empty() {
        return Err("schema has no SDK surface".to_owned());
    }
    for scalar in &model.scalars {
        if !scalar.consensus_integer {
            continue;
        }
        if scalar.typescript != "bigint"
            || scalar.python != "int"
            || !matches!(scalar.rust.as_str(), "u8" | "u16" | "u32" | "u64" | "u128")
            || scalar.wire != "decimal_string"
        {
            return Err(format!(
                "consensus integer {} has a lossy language boundary",
                scalar.name
            ));
        }
    }
    let daemon = model
        .guarantees
        .iter()
        .find(|(name, _, _)| name == "DaemonLimit")
        .ok_or_else(|| "missing DaemonLimit guarantee".to_owned())?;
    if daemon.1 != "daemon_enforced"
        || !daemon
            .2
            .contains("Bypassing the daemon bypasses this limit")
        || daemon.2.contains("protocol-enforced")
    {
        return Err("daemon-only restriction is overstated".to_owned());
    }
    let approval = model
        .guarantees
        .iter()
        .find(|(name, _, _)| name == "ApprovalHold")
        .ok_or_else(|| "missing ApprovalHold guarantee".to_owned())?;
    if approval.1 != "daemon_enforced"
        || !approval.2.contains("confers no protocol authority")
        || !approval
            .2
            .contains("bypassing the daemon bypasses the restriction")
    {
        return Err("approval restriction is overstated as protocol authority".to_owned());
    }
    for operation in [
        "approval.list",
        "approval.get",
        "approval.approve",
        "approval.reject",
    ] {
        if !model
            .operations
            .iter()
            .any(|candidate| candidate == operation)
        {
            return Err(format!("approval SDK operation missing {operation}"));
        }
    }
    for operation in [
        "program.discover",
        "program.interface",
        "program.simulate",
        "program.call",
        "program.receipt",
        "program.activity",
    ] {
        if !model
            .operations
            .iter()
            .any(|candidate| candidate == operation)
        {
            return Err(format!("Programs SDK operation missing {operation}"));
        }
    }
    if model.approval.states.is_empty()
        || model.approval.outcomes.is_empty()
        || model.approval.events.is_empty()
        || !model.approval.introduced_contract.contains('.')
    {
        return Err("approval SDK vocabulary or introducing contract is incomplete".to_owned());
    }
    Ok(())
}

fn integer_bound(rust: &str) -> Result<&'static str, String> {
    match rust {
        "u8" => Ok("255"),
        "u16" => Ok("65535"),
        "u32" => Ok("4294967295"),
        "u64" => Ok("18446744073709551615"),
        "u128" => Ok("340282366920938463463374607431768211455"),
        _ => Err(format!("unsupported exact integer {rust}")),
    }
}

fn snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() && index != 0 {
            output.push('_');
        }
        output.push(character.to_ascii_lowercase());
    }
    output
}

fn typescript(model: &Model) -> Result<String, String> {
    let mut scalars = String::new();
    for scalar in &model.scalars {
        let bound = integer_bound(&scalar.rust)?;
        write!(
            &mut scalars,
            "export type {0} = bigint;\nexport function parse{0}(value: string): {0} {{\n  if (!/^(0|[1-9][0-9]*)$/.test(value)) throw new RangeError(\"invalid {0}\");\n  const parsed = BigInt(value);\n  if (parsed > {1}n) throw new RangeError(\"{0} out of range\");\n  return parsed;\n}}\n",
            scalar.name, bound
        )
        .map_err(|_| "format TypeScript scalar".to_owned())?;
    }
    let levels = model
        .levels
        .iter()
        .enumerate()
        .map(|(rank, level)| format!("  {level} = {rank},"))
        .collect::<Vec<_>>()
        .join("\n");
    let errors = model
        .errors
        .iter()
        .map(|error| format!("\"{error}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    let operations = model
        .operations
        .iter()
        .map(|operation| format!("\"{operation}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    let tuple = |values: &[String]| {
        values
            .iter()
            .map(|value| format!("\"{value}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let approval = format!(
        r#"export const APPROVAL_CONTRACT_INTRODUCED = "{}" as const;
export const APPROVAL_ENFORCEMENT_NOTICE = "{}" as const;
export const APPROVAL_STATES = [{}] as const;
export const APPROVAL_DECISION_OUTCOMES = [{}] as const;
export const APPROVAL_EVENT_KINDS = [{}] as const;

export type ApprovalState = (typeof APPROVAL_STATES)[number];
export type ApprovalDecisionOutcome = (typeof APPROVAL_DECISION_OUTCOMES)[number];
export type ApprovalEventKind = (typeof APPROVAL_EVENT_KINDS)[number];

export interface StructuredActivityDisclosure {{
  canonicalDigest: string;
  activityType: string;
  actor: string;
  authority: string;
  counterparties: readonly string[];
  amounts: readonly Amount[];
  asset: string;
  feeLimit: Amount;
  expiry: TimestampSeconds;
  idempotencyKey: string;
}}

export interface HoldReason {{ code: string; message: string }}
export interface ApprovalRecord {{
  approvalId: string;
  tenant: string;
  heldActivity: StructuredActivityDisclosure;
  canonicalBytesDigest: string;
  holdReason: HoldReason;
  createdAt: TimestampSeconds;
  expiresAt: TimestampSeconds;
  state: ApprovalState;
  enforcement: "daemon_enforced";
  authorityNotice: typeof APPROVAL_ENFORCEMENT_NOTICE;
}}
export interface ApprovalPage {{ approvals: readonly ApprovalRecord[]; nextCursor: string | null }}
export interface ApprovalListRequest {{ tenant: string; cursor: string | null; pageLimit: number }}
export interface ApprovalGetRequest {{ tenant: string; approvalId: string }}
export interface ApprovalApproveRequest {{ tenant: string; approvalId: string; idempotencyKey: string }}
export interface ApprovalRejectRequest {{ tenant: string; approvalId: string; idempotencyKey: string; reason: string }}
export interface ApprovalDecision {{
  outcome: ApprovalDecisionOutcome;
  submissionRef: string | null;
  winningOutcome: ApprovalDecisionOutcome | null;
  enforcement: "daemon_enforced";
  authorityNotice: typeof APPROVAL_ENFORCEMENT_NOTICE;
}}
export interface ApprovalLifecycleEvent {{
  eventId: string;
  tenant: string;
  approvalId: string;
  kind: ApprovalEventKind;
  at: TimestampSeconds;
  recordDigest: string;
  holdReason?: HoldReason;
  expiresAt?: TimestampSeconds;
  submissionRef?: string;
  reason?: string;
  deterministicExpiry?: boolean;
  defectCode?: string;
}}"#,
        model.approval.introduced_contract,
        model.approval.enforcement_notice,
        tuple(&model.approval.states),
        tuple(&model.approval.outcomes),
        tuple(&model.approval.events),
    );
    Ok(TYPESCRIPT_TEMPLATE
        .replace("{{SCALARS}}", scalars.trim_end())
        .replace("{{LEVELS}}", &levels)
        .replace("{{ERRORS}}", &errors)
        .replace("{{OPERATIONS}}", &operations)
        .replace("{{APPROVAL}}", &approval))
}

fn python(model: &Model) -> Result<String, String> {
    let mut scalars = String::new();
    for scalar in &model.scalars {
        let bound = integer_bound(&scalar.rust)?;
        let function = snake_case(&scalar.name);
        write!(
            &mut scalars,
            "{0} = int\n\ndef parse_{1}(value: str) -> {0}:\n    if not value or (value != \"0\" and value.startswith(\"0\")) or not value.isascii() or not value.isdigit():\n        raise ValueError(\"invalid {0}\")\n    parsed = int(value)\n    if parsed > {2}:\n        raise OverflowError(\"{0} out of range\")\n    return parsed\n\n",
            scalar.name, function, bound
        )
        .map_err(|_| "format Python scalar".to_owned())?;
    }
    let levels = model
        .levels
        .iter()
        .enumerate()
        .map(|(rank, level)| format!("    {} = {rank}", snake_case(level).to_ascii_uppercase()))
        .collect::<Vec<_>>()
        .join("\n");
    let errors = model
        .errors
        .iter()
        .map(|error| format!("\"{error}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let operations = model
        .operations
        .iter()
        .map(|operation| format!("\"{operation}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let literal = |values: &[String]| {
        values
            .iter()
            .map(|value| format!("\"{value}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let approval = format!(
        r#"APPROVAL_CONTRACT_INTRODUCED = "{}"
APPROVAL_ENFORCEMENT_NOTICE = "{}"
APPROVAL_STATES = ({},)
APPROVAL_DECISION_OUTCOMES = ({},)
APPROVAL_EVENT_KINDS = ({},)

ApprovalState = Literal[{}]
ApprovalDecisionOutcome = Literal[{}]
ApprovalEventKind = Literal[{}]

@dataclass(frozen=True)
class StructuredActivityDisclosure:
    canonical_digest: str
    activity_type: str
    actor: str
    authority: str
    counterparties: tuple[str, ...]
    amounts: tuple[Amount, ...]
    asset: str
    fee_limit: Amount
    expiry: TimestampSeconds
    idempotency_key: str

@dataclass(frozen=True)
class HoldReason:
    code: str
    message: str

@dataclass(frozen=True)
class ApprovalRecord:
    approval_id: str
    tenant: str
    held_activity: StructuredActivityDisclosure
    canonical_bytes_digest: str
    hold_reason: HoldReason
    created_at: TimestampSeconds
    expires_at: TimestampSeconds
    state: ApprovalState
    enforcement: Literal["daemon_enforced"] = "daemon_enforced"
    authority_notice: str = APPROVAL_ENFORCEMENT_NOTICE

@dataclass(frozen=True)
class ApprovalPage:
    approvals: tuple[ApprovalRecord, ...]
    next_cursor: str | None

@dataclass(frozen=True)
class ApprovalListRequest:
    tenant: str
    cursor: str | None
    page_limit: int

@dataclass(frozen=True)
class ApprovalGetRequest:
    tenant: str
    approval_id: str

@dataclass(frozen=True)
class ApprovalApproveRequest:
    tenant: str
    approval_id: str
    idempotency_key: str

@dataclass(frozen=True)
class ApprovalRejectRequest:
    tenant: str
    approval_id: str
    idempotency_key: str
    reason: str

@dataclass(frozen=True)
class ApprovalDecision:
    outcome: ApprovalDecisionOutcome
    submission_ref: str | None
    winning_outcome: ApprovalDecisionOutcome | None
    enforcement: Literal["daemon_enforced"] = "daemon_enforced"
    authority_notice: str = APPROVAL_ENFORCEMENT_NOTICE

@dataclass(frozen=True)
class ApprovalLifecycleEvent:
    event_id: str
    tenant: str
    approval_id: str
    kind: ApprovalEventKind
    at: TimestampSeconds
    record_digest: str
    hold_reason: HoldReason | None = None
    expires_at: TimestampSeconds | None = None
    submission_ref: str | None = None
    reason: str | None = None
    deterministic_expiry: bool | None = None
    defect_code: str | None = None"#,
        model.approval.introduced_contract,
        model.approval.enforcement_notice,
        literal(&model.approval.states),
        literal(&model.approval.outcomes),
        literal(&model.approval.events),
        literal(&model.approval.states),
        literal(&model.approval.outcomes),
        literal(&model.approval.events),
    );
    Ok(PYTHON_TEMPLATE
        .replace("{{SCALARS}}", scalars.trim_end())
        .replace("{{LEVELS}}", &levels)
        .replace("{{ERRORS}}", &errors)
        .replace("{{OPERATIONS}}", &operations)
        .replace("{{APPROVAL}}", &approval))
}

fn python_stub(model: &Model) -> Result<String, String> {
    let mut scalars = String::new();
    for scalar in &model.scalars {
        let function = snake_case(&scalar.name);
        write!(
            &mut scalars,
            "{0}: TypeAlias = int\ndef parse_{1}(value: str) -> {0}: ...\n\n",
            scalar.name, function
        )
        .map_err(|_| "format Python scalar stub".to_owned())?;
    }
    let levels = model
        .levels
        .iter()
        .enumerate()
        .map(|(rank, level)| format!("    {} = {rank}", snake_case(level).to_ascii_uppercase()))
        .collect::<Vec<_>>()
        .join("\n");
    let errors = model
        .errors
        .iter()
        .map(|error| format!("\"{error}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let operations = model
        .operations
        .iter()
        .map(|operation| format!("\"{operation}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let literal = |values: &[String]| {
        values
            .iter()
            .map(|value| format!("\"{value}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let approval = format!(
        r#"ApprovalState: TypeAlias = Literal[{}]
ApprovalDecisionOutcome: TypeAlias = Literal[{}]
ApprovalEventKind: TypeAlias = Literal[{}]

APPROVAL_CONTRACT_INTRODUCED: Literal["{}"]
APPROVAL_ENFORCEMENT_NOTICE: str
APPROVAL_STATES: tuple[ApprovalState, ...]
APPROVAL_DECISION_OUTCOMES: tuple[ApprovalDecisionOutcome, ...]
APPROVAL_EVENT_KINDS: tuple[ApprovalEventKind, ...]

class StructuredActivityDisclosure:
    canonical_digest: str
    activity_type: str
    actor: str
    authority: str
    counterparties: tuple[str, ...]
    amounts: tuple[Amount, ...]
    asset: str
    fee_limit: Amount
    expiry: TimestampSeconds
    idempotency_key: str
    def __init__(self, canonical_digest: str, activity_type: str, actor: str, authority: str, counterparties: tuple[str, ...], amounts: tuple[Amount, ...], asset: str, fee_limit: Amount, expiry: TimestampSeconds, idempotency_key: str) -> None: ...

class HoldReason:
    code: str
    message: str
    def __init__(self, code: str, message: str) -> None: ...

class ApprovalRecord:
    approval_id: str
    tenant: str
    held_activity: StructuredActivityDisclosure
    canonical_bytes_digest: str
    hold_reason: HoldReason
    created_at: TimestampSeconds
    expires_at: TimestampSeconds
    state: ApprovalState
    enforcement: Literal["daemon_enforced"]
    authority_notice: str

class ApprovalPage:
    approvals: tuple[ApprovalRecord, ...]
    next_cursor: str | None

class ApprovalListRequest:
    tenant: str
    cursor: str | None
    page_limit: int
    def __init__(self, tenant: str, cursor: str | None, page_limit: int) -> None: ...

class ApprovalGetRequest:
    tenant: str
    approval_id: str
    def __init__(self, tenant: str, approval_id: str) -> None: ...

class ApprovalApproveRequest:
    tenant: str
    approval_id: str
    idempotency_key: str
    def __init__(self, tenant: str, approval_id: str, idempotency_key: str) -> None: ...

class ApprovalRejectRequest:
    tenant: str
    approval_id: str
    idempotency_key: str
    reason: str
    def __init__(self, tenant: str, approval_id: str, idempotency_key: str, reason: str) -> None: ...

class ApprovalDecision:
    outcome: ApprovalDecisionOutcome
    submission_ref: str | None
    winning_outcome: ApprovalDecisionOutcome | None
    enforcement: Literal["daemon_enforced"]
    authority_notice: str

class ApprovalLifecycleEvent:
    event_id: str
    tenant: str
    approval_id: str
    kind: ApprovalEventKind
    at: TimestampSeconds
    record_digest: str"#,
        literal(&model.approval.states),
        literal(&model.approval.outcomes),
        literal(&model.approval.events),
        model.approval.introduced_contract,
    );
    Ok(PYTHON_STUB_TEMPLATE
        .replace("{{SCALARS}}", scalars.trim_end())
        .replace("{{LEVELS}}", &levels)
        .replace("{{ERRORS}}", &errors)
        .replace("{{OPERATIONS}}", &operations)
        .replace("{{APPROVAL}}", &approval))
}

fn guarantees(model: &Model) -> String {
    let rows = model
        .guarantees
        .iter()
        .map(|(name, enforcement, statement)| {
            format!("| `{name}` | `{enforcement}` | {statement} |")
        })
        .collect::<Vec<_>>()
        .join("\n");
    GUARANTEES_TEMPLATE
        .replace("{{GUARANTEES}}", &rows)
        .replace(
            "{{APPROVAL_INTRODUCED}}",
            &model.approval.introduced_contract,
        )
}

fn compatibility(model: &Model) -> String {
    COMPATIBILITY_TEMPLATE
        .replace("{{SDK}}", &model.compatibility.sdk)
        .replace("{{CONTRACT}}", &model.compatibility.contract)
        .replace("{{NODE_INTERFACE}}", &model.compatibility.node_interface)
        .replace("{{DAEMON}}", &model.compatibility.daemon)
        .replace(
            "{{APPROVAL_INTRODUCED}}",
            &model.approval.introduced_contract,
        )
}

/// Generates both language SDKs and their guarantee documentation from one model.
///
/// # Errors
///
/// Refuses malformed schema, lossy integer mappings and overstated guarantees.
pub fn agent_sdk_generator(schema_root: &Path) -> Result<Generated, String> {
    let model = read_schema(schema_root)?;
    let documentation = guarantees(&model);
    let compatibility = compatibility(&model);
    Ok(Generated {
        files: BTreeMap::from([
            (PathBuf::from("COMPATIBILITY.md"), compatibility),
            (
                PathBuf::from("typescript/src/generated/client.ts"),
                typescript(&model)?,
            ),
            (
                PathBuf::from("typescript/src/generated/guarantees.md"),
                documentation.clone(),
            ),
            (
                PathBuf::from("python/layerx_sdk/generated/client.py"),
                python(&model)?,
            ),
            (
                PathBuf::from("python/layerx_sdk/generated/client.pyi"),
                python_stub(&model)?,
            ),
            (
                PathBuf::from("python/layerx_sdk/generated/guarantees.md"),
                documentation,
            ),
        ]),
    })
}

/// Compares every generated byte with the committed SDK tree.
///
/// # Errors
///
/// Names the first missing or hand-edited generated file.
pub fn agent_sdk_drift_gate(generated: &Generated, output_root: &Path) -> Result<(), String> {
    for (relative, expected) in &generated.files {
        let path = output_root.join(relative);
        let actual = fs::read_to_string(&path)
            .map_err(|error| format!("generated file missing {}: {error}", path.display()))?;
        if &actual != expected {
            return Err(format!("generated SDK drift at {}", path.display()));
        }
    }
    Ok(())
}

fn write_generated(generated: &Generated, output_root: &Path) -> Result<(), String> {
    for (relative, source) in &generated.files {
        let path = output_root.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| format!("generated path has no parent: {}", path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
        fs::write(&path, source).map_err(|error| format!("write {}: {error}", path.display()))?;
    }
    Ok(())
}

/// Compares the locked Programs contract with the canonical Programs schema source.
///
/// # Errors
///
/// Names a missing or byte-stale locked Programs contract.
pub fn programs_golden_drift_gate(schema_root: &Path) -> Result<(), String> {
    let source_path = schema_root.join("programs.kvx");
    let golden_path = schema_root.join("golden/programs.kvx");
    let source = fs::read(&source_path)
        .map_err(|error| format!("read {}: {error}", source_path.display()))?;
    let golden = fs::read(&golden_path)
        .map_err(|error| format!("read {}: {error}", golden_path.display()))?;
    if source != golden {
        return Err(format!(
            "generated Programs contract drift at {}; run make agent-sdk-generate",
            golden_path.display()
        ));
    }
    Ok(())
}

/// Regenerates the locked Programs contract from its canonical schema source.
///
/// # Errors
///
/// Fails when the canonical source cannot be read or the locked output cannot be written.
pub fn write_programs_golden(schema_root: &Path) -> Result<(), String> {
    let source_path = schema_root.join("programs.kvx");
    let golden_path = schema_root.join("golden/programs.kvx");
    let source = fs::read(&source_path)
        .map_err(|error| format!("read {}: {error}", source_path.display()))?;
    let parent = golden_path
        .parent()
        .ok_or_else(|| format!("generated path has no parent: {}", golden_path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;
    fs::write(&golden_path, source)
        .map_err(|error| format!("write {}: {error}", golden_path.display()))
}

fn run() -> Result<(), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let mode = arguments.first().map_or("--check", String::as_str);
    let schema = arguments
        .get(1)
        .map_or_else(|| PathBuf::from("agent/schema/agent-api"), PathBuf::from);
    let output = arguments
        .get(2)
        .map_or_else(|| PathBuf::from("agent/sdk"), PathBuf::from);
    let generated = agent_sdk_generator(&schema)?;
    match mode {
        "--write" => {
            write_generated(&generated, &output)?;
            write_programs_golden(&schema)
        }
        "--check" => {
            agent_sdk_drift_gate(&generated, &output)?;
            programs_golden_drift_gate(&schema)
        }
        _ => Err("usage: agent-sdk-gen [--write|--check] [schema-root] [output-root]".to_owned()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("agent-sdk-gen: {error}");
        std::process::exit(1);
    }
}
