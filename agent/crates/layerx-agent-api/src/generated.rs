//! Generated from `agent/schema/agent-api/v1.kvx`; do not hand-edit.

/// Exact source schema used to generate this module.
pub const AGENT_API_V1_SOURCE: &str = include_str!("../../../schema/agent-api/v1.kvx");

/// Contract metadata pinned into every generated consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContractSchema {
    pub name: &'static str,
    pub version: ContractVersion,
    pub node_interface_major: u16,
}

/// Agent API semantic version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContractVersion {
    pub major: u16,
    pub minor: u16,
}

/// Version negotiation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VersionRequest {
    pub request_id: Sequence,
    pub supported: ContractVersion,
}

/// Version negotiation response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VersionResponse {
    pub request_id: Sequence,
    pub contract: ContractVersion,
    pub node_interface_major: u16,
}

macro_rules! exact_integer {
    ($name:ident, $inner:ty) => {
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub struct $name(pub $inner);

        impl $name {
            #[must_use]
            pub const fn get(self) -> $inner {
                self.0
            }

            /// Parses a canonical decimal integer without a floating-point boundary.
            ///
            /// # Errors
            /// Returns the standard integer parse error for malformed or out-of-range input.
            pub fn parse_decimal(value: &str) -> Result<Self, std::num::ParseIntError> {
                value.parse::<$inner>().map(Self)
            }
        }
    };
}

exact_integer!(Amount, u128);
exact_integer!(Sequence, u64);
exact_integer!(BudgetLimit, u128);
exact_integer!(TimestampSeconds, u64);

/// Returns the immutable v1 contract descriptor.
#[must_use]
pub const fn agent_api_schema_v1() -> ContractSchema {
    ContractSchema {
        name: "LayerX Agent API",
        version: ContractVersion { major: 1, minor: 2 },
        node_interface_major: 1,
    }
}

/// Enforces additive-only compatibility within a contract major version.
///
/// # Errors
/// Returns the first removed or changed declaration within an unchanged major version.
pub fn agent_api_compat_gate(
    previous_major: u16,
    current_major: u16,
    previous: &[(&str, &str)],
    current: &[(&str, &str)],
) -> Result<(), String> {
    if previous_major != current_major {
        return Ok(());
    }
    for (key, old_value) in previous {
        match current.iter().find(|(candidate, _)| candidate == key) {
            Some((_, new_value)) if new_value == old_value => {}
            Some((_, new_value)) => {
                return Err(format!("breaking contract change at {key}: {old_value} -> {new_value}"));
            }
            None => return Err(format!("breaking contract removal: {key}")),
        }
    }
    Ok(())
}
