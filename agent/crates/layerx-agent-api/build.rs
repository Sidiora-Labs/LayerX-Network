use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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

fn unquote(value: &str) -> Result<&str, String> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| format!("expected quoted value, got {value}"))
}

fn string_list(value: &str) -> Result<Vec<String>, String> {
    let body = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| format!("expected string list, got {value}"))?;
    if body.trim().is_empty() {
        return Ok(Vec::new());
    }
    body.split(',')
        .map(|item| unquote(item.trim()).map(str::to_owned))
        .collect()
}

fn integer(entries: &BTreeMap<String, String>, key: &str) -> Result<u16, String> {
    entries
        .get(key)
        .ok_or_else(|| format!("missing {key}"))?
        .parse()
        .map_err(|error| format!("invalid {key}: {error}"))
}

fn validate(entries: &BTreeMap<String, String>) -> Result<(), String> {
    for (key, value) in entries {
        let language_type = key
            .rsplit_once('.')
            .is_some_and(|(_, suffix)| matches!(suffix, "rust" | "typescript" | "python"));
        if language_type
            && ["f32", "f64", "float", "double", "number", "decimal"]
                .iter()
                .any(|forbidden| unquote(value).is_ok_and(|actual| actual == *forbidden))
        {
            return Err(format!("floating-point schema type is forbidden at {key}"));
        }
        if key.ends_with(".consensus_integer") && value != "true" {
            return Err(format!("consensus integer marker must be true at {key}"));
        }
    }
    Ok(())
}

fn compatibility_gate(
    baseline: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> Result<(), String> {
    let major = |entries: &BTreeMap<String, String>| {
        integer(entries, "schema.major").or_else(|_| integer(entries, "module.contract_major"))
    };
    let old_major = major(baseline)?;
    let new_major = major(current)?;
    if old_major != new_major {
        return Ok(());
    }
    for (key, old_value) in baseline {
        if key == "schema.minor" {
            continue;
        }
        match current.get(key) {
            Some(new_value) if new_value == old_value => {}
            Some(new_value)
                if key == "schema.includes" && additive_string_list(old_value, new_value) => {}
            Some(new_value) => {
                return Err(format!(
                    "breaking contract change without a major increment at {key}: {old_value} -> {new_value}"
                ));
            }
            None => {
                return Err(format!(
                    "breaking contract removal without a major increment: {key}"
                ));
            }
        }
    }
    Ok(())
}

fn additive_string_list(previous: &str, current: &str) -> bool {
    fn values(source: &str) -> Option<Vec<&str>> {
        source
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .collect()
            })
    }
    match (values(previous), values(current)) {
        (Some(previous), Some(current)) => {
            current.len() >= previous.len()
                && previous
                    .iter()
                    .zip(current.iter())
                    .all(|(old, new)| old == new)
        }
        _ => false,
    }
}

const CUSTODY_CLAIM_TAGS: &[(&str, &str)] = &[
    (
        "write.kvx",
        "type.SubmissionState.Executed.settlement_domain",
    ),
    (
        "stream.kvx",
        "type.ReceiptReference.Verified.settlement_domain",
    ),
    ("read.kvx", "operation.export.offline.settlement_domain"),
];

fn settlement_domain_gate(
    root: &BTreeMap<String, String>,
    modules: &[(String, BTreeMap<String, String>)],
) -> Result<(), String> {
    let variants = root
        .get("type.SettlementDomain.variants")
        .ok_or_else(|| "missing the type.SettlementDomain claim vocabulary".to_owned())?;
    if !variants.starts_with("[\"Paxeer\"") {
        return Err(format!(
            "Paxeer must be the first settlement domain variant, got {variants}"
        ));
    }
    for (module, entries) in modules {
        for (key, value) in entries {
            if key.ends_with(".settlement_domain") && value != "\"Paxeer\"" {
                return Err(format!(
                    "{module} names a foreign settlement domain at {key}: {value}"
                ));
            }
        }
    }
    for (module, key) in CUSTODY_CLAIM_TAGS {
        let tagged = modules.iter().any(|(name, entries)| {
            name == module && entries.get(*key).map(String::as_str) == Some("\"Paxeer\"")
        });
        if !tagged {
            return Err(format!(
                "untagged custody claim: {module} must declare {key} = \"Paxeer\""
            ));
        }
    }
    Ok(())
}

fn generated_source(entries: &BTreeMap<String, String>) -> Result<String, String> {
    let name = unquote(
        entries
            .get("schema.name")
            .ok_or_else(|| "missing schema.name".to_owned())?,
    )?;
    let major = integer(entries, "schema.major")?;
    let minor = integer(entries, "schema.minor")?;
    let node_major = integer(entries, "schema.node_interface_major")?;
    Ok(format!(
        r#"//! Generated from `agent/schema/agent-api/v1.kvx`; do not hand-edit.

/// Exact source schema used to generate this module.
pub const AGENT_API_V1_SOURCE: &str = include_str!("../../../schema/agent-api/v1.kvx");

/// Contract metadata pinned into every generated consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContractSchema {{
    pub name: &'static str,
    pub version: ContractVersion,
    pub node_interface_major: u16,
}}

/// Agent API semantic version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContractVersion {{
    pub major: u16,
    pub minor: u16,
}}

/// Version negotiation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VersionRequest {{
    pub request_id: Sequence,
    pub supported: ContractVersion,
}}

/// Version negotiation response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VersionResponse {{
    pub request_id: Sequence,
    pub contract: ContractVersion,
    pub node_interface_major: u16,
}}

macro_rules! exact_integer {{
    ($name:ident, $inner:ty) => {{
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub struct $name(pub $inner);

        impl $name {{
            #[must_use]
            pub const fn get(self) -> $inner {{
                self.0
            }}

            /// Parses a canonical decimal integer without a floating-point boundary.
            ///
            /// # Errors
            /// Returns the standard integer parse error for malformed or out-of-range input.
            pub fn parse_decimal(value: &str) -> Result<Self, std::num::ParseIntError> {{
                value.parse::<$inner>().map(Self)
            }}
        }}
    }};
}}

exact_integer!(Amount, u128);
exact_integer!(Sequence, u64);
exact_integer!(BudgetLimit, u128);
exact_integer!(TimestampSeconds, u64);

/// Returns the immutable v1 contract descriptor.
#[must_use]
pub const fn agent_api_schema_v1() -> ContractSchema {{
    ContractSchema {{
        name: "{name}",
        version: ContractVersion {{ major: {major}, minor: {minor} }},
        node_interface_major: {node_major},
    }}
}}

/// Enforces additive-only compatibility within a contract major version.
///
/// # Errors
/// Returns the first removed or changed declaration within an unchanged major version.
pub fn agent_api_compat_gate(
    previous_major: u16,
    current_major: u16,
    previous: &[(&str, &str)],
    current: &[(&str, &str)],
) -> Result<(), String> {{
    if previous_major != current_major {{
        return Ok(());
    }}
    for (key, old_value) in previous {{
        match current.iter().find(|(candidate, _)| candidate == key) {{
            Some((_, new_value)) if new_value == old_value => {{}}
            Some((_, new_value)) => {{
                return Err(format!("breaking contract change at {{key}}: {{old_value}} -> {{new_value}}"));
            }}
            None => return Err(format!("breaking contract removal: {{key}}")),
        }}
    }}
    Ok(())
}}
"#
    ))
}

fn read_file(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn main() {
    let crate_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .unwrap_or_else(|| panic!("CARGO_MANIFEST_DIR is unavailable")),
    );
    let schema = crate_dir.join("../../schema/agent-api/v1.kvx");
    let baseline = crate_dir.join("../../schema/agent-api/golden/v1.kvx");
    let committed = crate_dir.join("src/generated.rs");
    println!("cargo:rerun-if-changed={}", schema.display());
    println!("cargo:rerun-if-changed={}", baseline.display());
    println!("cargo:rerun-if-changed={}", committed.display());

    let current =
        section_map(&read_file(&schema)).unwrap_or_else(|error| panic!("invalid schema: {error}"));
    let old = section_map(&read_file(&baseline))
        .unwrap_or_else(|error| panic!("invalid baseline: {error}"));
    validate(&current).unwrap_or_else(|error| panic!("invalid schema: {error}"));
    compatibility_gate(&old, &current)
        .unwrap_or_else(|error| panic!("incompatible schema: {error}"));

    let includes = string_list(
        current
            .get("schema.includes")
            .unwrap_or_else(|| panic!("schema.includes is unavailable")),
    )
    .unwrap_or_else(|error| panic!("invalid schema.includes: {error}"));
    let mut modules: Vec<(String, BTreeMap<String, String>)> =
        vec![("v1.kvx".to_owned(), current.clone())];
    for module in includes {
        let current_module = schema
            .parent()
            .unwrap_or_else(|| panic!("schema has no parent"))
            .join(&module);
        let baseline_module = baseline
            .parent()
            .unwrap_or_else(|| panic!("baseline has no parent"))
            .join(&module);
        println!("cargo:rerun-if-changed={}", current_module.display());
        println!("cargo:rerun-if-changed={}", baseline_module.display());
        let current_entries = section_map(&read_file(&current_module))
            .unwrap_or_else(|error| panic!("invalid {}: {error}", current_module.display()));
        validate(&current_entries)
            .unwrap_or_else(|error| panic!("invalid {}: {error}", current_module.display()));
        if baseline_module.exists() {
            let baseline_entries = section_map(&read_file(&baseline_module))
                .unwrap_or_else(|error| panic!("invalid {}: {error}", baseline_module.display()));
            compatibility_gate(&baseline_entries, &current_entries).unwrap_or_else(|error| {
                panic!("incompatible {}: {error}", current_module.display())
            });
        }
        modules.push((module, current_entries));
    }
    settlement_domain_gate(&current, &modules)
        .unwrap_or_else(|error| panic!("invalid schema: {error}"));
    let fresh =
        generated_source(&current).unwrap_or_else(|error| panic!("generation failed: {error}"));

    let out =
        PathBuf::from(env::var_os("OUT_DIR").unwrap_or_else(|| panic!("OUT_DIR is unavailable")))
            .join("generated.rs");
    fs::write(&out, &fresh)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", out.display()));
    let checked_in = read_file(&committed);
    assert_eq!(
        checked_in, fresh,
        "generated Rust contract drift; regenerate src/generated.rs from v1.kvx"
    );
}
