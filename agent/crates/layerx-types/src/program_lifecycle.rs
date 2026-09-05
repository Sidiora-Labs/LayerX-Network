use crate::intent::ProgramId;

const MAX_WASM_BYTES: usize = 1_048_576;
const MAX_INTERFACE_BYTES: usize = 952;
const MAX_SEED_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidNativeLifecycle;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramUpgradePolicy {
    Immutable,
    Authority([u8; 32]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeProgramDeploy<'a> {
    pub program_id: ProgramId,
    pub guest_abi: u16,
    pub policy: ProgramUpgradePolicy,
    pub new_hash: [u8; 32],
    pub interface: Option<&'a [u8]>,
    pub wasm: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeProgramUpgrade<'a> {
    pub program_id: ProgramId,
    pub guest_abi: u16,
    pub old_hash: [u8; 32],
    pub new_hash: [u8; 32],
    pub migration_hook: &'a [u8],
    pub clear_interface: bool,
    pub interface: Option<&'a [u8]>,
    pub wasm: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramWindDownOperation<'a> {
    Route {
        account: [u8; 32],
        asset: [u8; 32],
        destination: [u8; 32],
        seed: &'a [u8],
    },
    Deprecate {
        exit_program: [u8; 32],
        deadline_batch: u64,
    },
    Tombstone,
    Exit {
        account: [u8; 32],
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeProgramWindDown<'a> {
    pub program_id: ProgramId,
    pub operation: ProgramWindDownOperation<'a>,
}

fn body<'a>(remaining: &mut &'a [u8], length: usize) -> Result<&'a [u8], InvalidNativeLifecycle> {
    let (head, tail) = remaining
        .split_at_checked(length)
        .ok_or(InvalidNativeLifecycle)?;
    *remaining = tail;
    Ok(head)
}

fn take<const LENGTH: usize>(
    remaining: &mut &[u8],
) -> Result<[u8; LENGTH], InvalidNativeLifecycle> {
    body(remaining, LENGTH)?
        .try_into()
        .map_err(|_| InvalidNativeLifecycle)
}

fn length32(remaining: &mut &[u8]) -> Result<usize, InvalidNativeLifecycle> {
    usize::try_from(u32::from_be_bytes(take(remaining)?)).map_err(|_| InvalidNativeLifecycle)
}

fn append_length(bytes: &mut Vec<u8>, length: usize) -> Result<(), InvalidNativeLifecycle> {
    bytes.extend_from_slice(
        &u32::try_from(length)
            .map_err(|_| InvalidNativeLifecycle)?
            .to_be_bytes(),
    );
    Ok(())
}

fn validate_code(
    program_id: ProgramId,
    guest_abi: u16,
    wasm: &[u8],
) -> Result<(), InvalidNativeLifecycle> {
    if program_id.is_zero()
        || !matches!(guest_abi, 1..=3)
        || !wasm.starts_with(b"\0asm\x01\0\0\0")
        || wasm.len() > MAX_WASM_BYTES
    {
        return Err(InvalidNativeLifecycle);
    }
    Ok(())
}

impl<'a> NativeProgramDeploy<'a> {
    /// # Errors
    /// Rejects invalid policy, identifiers, ABI or body lengths.
    pub fn encode(&self) -> Result<Vec<u8>, InvalidNativeLifecycle> {
        validate_code(self.program_id, self.guest_abi, self.wasm)?;
        if self
            .interface
            .is_some_and(|interface| interface.is_empty() || interface.len() > MAX_INTERFACE_BYTES)
        {
            return Err(InvalidNativeLifecycle);
        }
        let (policy, authority) = match self.policy {
            ProgramUpgradePolicy::Immutable => (0, [0; 32]),
            ProgramUpgradePolicy::Authority(authority) if authority != [0; 32] => (1, authority),
            ProgramUpgradePolicy::Authority(_) => return Err(InvalidNativeLifecycle),
        };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.program_id.bytes());
        bytes.extend_from_slice(&self.guest_abi.to_be_bytes());
        bytes.extend_from_slice(&[policy, 0]);
        bytes.extend_from_slice(&authority);
        bytes.extend_from_slice(&self.new_hash);
        append_length(&mut bytes, self.wasm.len())?;
        if let Some(interface) = self.interface {
            append_length(&mut bytes, interface.len())?;
            bytes.extend_from_slice(interface);
        }
        bytes.extend_from_slice(self.wasm);
        Ok(bytes)
    }

    /// # Errors
    /// Rejects malformed headers, truncation, trailing bytes and invalid bounds.
    pub fn decode(payload: &'a [u8]) -> Result<Self, InvalidNativeLifecycle> {
        let mut remaining = payload;
        let program_id = ProgramId::new(take(&mut remaining)?);
        let guest_abi = u16::from_be_bytes(take(&mut remaining)?);
        let [policy, reserved] = take(&mut remaining)?;
        let authority = take(&mut remaining)?;
        let policy = match (policy, authority) {
            (0, authority) if authority == [0; 32] => ProgramUpgradePolicy::Immutable,
            (1, authority) if authority != [0; 32] => ProgramUpgradePolicy::Authority(authority),
            _ => return Err(InvalidNativeLifecycle),
        };
        if reserved != 0 {
            return Err(InvalidNativeLifecycle);
        }
        let new_hash = take(&mut remaining)?;
        let wasm_length = length32(&mut remaining)?;
        let interface = if wasm_length != 0 && remaining.len() == wasm_length {
            None
        } else {
            let length = length32(&mut remaining)?;
            if length == 0 || length > MAX_INTERFACE_BYTES {
                return Err(InvalidNativeLifecycle);
            }
            Some(body(&mut remaining, length)?)
        };
        let wasm = body(&mut remaining, wasm_length)?;
        validate_code(program_id, guest_abi, wasm)?;
        if !remaining.is_empty() {
            return Err(InvalidNativeLifecycle);
        }
        Ok(Self {
            program_id,
            guest_abi,
            policy,
            new_hash,
            interface,
            wasm,
        })
    }
}

impl<'a> NativeProgramUpgrade<'a> {
    /// # Errors
    /// Rejects invalid identifiers, ABI, flags or body lengths.
    pub fn encode(&self) -> Result<Vec<u8>, InvalidNativeLifecycle> {
        validate_code(self.program_id, self.guest_abi, self.wasm)?;
        if self.migration_hook.len() > usize::from(u16::MAX)
            || (self.clear_interface && self.interface.is_none())
            || self.interface.is_some_and(|interface| {
                interface.len() > MAX_INTERFACE_BYTES
                    || (interface.is_empty() && !self.clear_interface)
            })
        {
            return Err(InvalidNativeLifecycle);
        }
        let flags =
            u8::from(!self.migration_hook.is_empty()) | (u8::from(self.clear_interface) << 1);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.program_id.bytes());
        bytes.extend_from_slice(&self.guest_abi.to_be_bytes());
        bytes.extend_from_slice(&[flags, 0]);
        bytes.extend_from_slice(&self.old_hash);
        bytes.extend_from_slice(&self.new_hash);
        bytes.extend_from_slice(
            &u16::try_from(self.migration_hook.len())
                .map_err(|_| InvalidNativeLifecycle)?
                .to_be_bytes(),
        );
        append_length(&mut bytes, self.wasm.len())?;
        if let Some(interface) = self.interface {
            append_length(&mut bytes, interface.len())?;
        }
        bytes.extend_from_slice(self.migration_hook);
        if let Some(interface) = self.interface {
            bytes.extend_from_slice(interface);
        }
        bytes.extend_from_slice(self.wasm);
        Ok(bytes)
    }

    /// # Errors
    /// Rejects malformed headers, truncation, trailing bytes and invalid bounds.
    pub fn decode(payload: &'a [u8]) -> Result<Self, InvalidNativeLifecycle> {
        let mut remaining = payload;
        let program_id = ProgramId::new(take(&mut remaining)?);
        let guest_abi = u16::from_be_bytes(take(&mut remaining)?);
        let [flags, reserved] = take(&mut remaining)?;
        if reserved != 0 || flags & 0xfc != 0 {
            return Err(InvalidNativeLifecycle);
        }
        let old_hash = take(&mut remaining)?;
        let new_hash = take(&mut remaining)?;
        let hook_length = usize::from(u16::from_be_bytes(take(&mut remaining)?));
        let wasm_length = length32(&mut remaining)?;
        if ((flags & 1) == 0) != (hook_length == 0) {
            return Err(InvalidNativeLifecycle);
        }
        let clear_interface = flags & 2 != 0;
        let variable_length = hook_length
            .checked_add(wasm_length)
            .ok_or(InvalidNativeLifecycle)?;
        let interface_length =
            if wasm_length != 0 && remaining.len() == variable_length && !clear_interface {
                None
            } else {
                let length = length32(&mut remaining)?;
                if length > MAX_INTERFACE_BYTES || (length == 0 && !clear_interface) {
                    return Err(InvalidNativeLifecycle);
                }
                Some(length)
            };
        let migration_hook = body(&mut remaining, hook_length)?;
        let interface = interface_length
            .map(|length| body(&mut remaining, length))
            .transpose()?;
        let wasm = body(&mut remaining, wasm_length)?;
        validate_code(program_id, guest_abi, wasm)?;
        if !remaining.is_empty() {
            return Err(InvalidNativeLifecycle);
        }
        Ok(Self {
            program_id,
            guest_abi,
            old_hash,
            new_hash,
            migration_hook,
            clear_interface,
            interface,
            wasm,
        })
    }
}

impl<'a> NativeProgramWindDown<'a> {
    /// # Errors
    /// Rejects the reserved program identifier and oversized route seeds.
    pub fn encode(&self) -> Result<Vec<u8>, InvalidNativeLifecycle> {
        if self.program_id.is_zero() {
            return Err(InvalidNativeLifecycle);
        }
        let mut bytes = self.program_id.bytes().to_vec();
        match self.operation {
            ProgramWindDownOperation::Route {
                account,
                asset,
                destination,
                seed,
            } => {
                if seed.len() > MAX_SEED_BYTES {
                    return Err(InvalidNativeLifecycle);
                }
                bytes.push(1);
                bytes.extend_from_slice(&account);
                bytes.extend_from_slice(&asset);
                bytes.extend_from_slice(&destination);
                bytes.extend_from_slice(
                    &u16::try_from(seed.len())
                        .map_err(|_| InvalidNativeLifecycle)?
                        .to_be_bytes(),
                );
                bytes.extend_from_slice(seed);
            }
            ProgramWindDownOperation::Deprecate {
                exit_program,
                deadline_batch,
            } => {
                bytes.push(2);
                bytes.extend_from_slice(&exit_program);
                bytes.extend_from_slice(&deadline_batch.to_be_bytes());
            }
            ProgramWindDownOperation::Tombstone => bytes.push(3),
            ProgramWindDownOperation::Exit { account } => {
                bytes.push(4);
                bytes.extend_from_slice(&account);
            }
        }
        Ok(bytes)
    }

    /// # Errors
    /// Rejects unknown operations, non-exact body lengths and oversized seeds.
    pub fn decode(payload: &'a [u8]) -> Result<Self, InvalidNativeLifecycle> {
        let mut remaining = payload;
        let program_id = ProgramId::new(take(&mut remaining)?);
        if program_id.is_zero() {
            return Err(InvalidNativeLifecycle);
        }
        let operation = match take(&mut remaining)? {
            [1] => {
                let account = take(&mut remaining)?;
                let asset = take(&mut remaining)?;
                let destination = take(&mut remaining)?;
                let length = usize::from(u16::from_be_bytes(take(&mut remaining)?));
                if length > MAX_SEED_BYTES {
                    return Err(InvalidNativeLifecycle);
                }
                ProgramWindDownOperation::Route {
                    account,
                    asset,
                    destination,
                    seed: body(&mut remaining, length)?,
                }
            }
            [2] => ProgramWindDownOperation::Deprecate {
                exit_program: take(&mut remaining)?,
                deadline_batch: u64::from_be_bytes(take(&mut remaining)?),
            },
            [3] => ProgramWindDownOperation::Tombstone,
            [4] => ProgramWindDownOperation::Exit {
                account: take(&mut remaining)?,
            },
            _ => return Err(InvalidNativeLifecycle),
        };
        if !remaining.is_empty() {
            return Err(InvalidNativeLifecycle);
        }
        Ok(Self {
            program_id,
            operation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_c_lifecycle_vectors_reencode_exactly() -> Result<(), String> {
        for (name, ordinal) in [
            ("deploy", 1),
            ("upgrade", 2),
            ("wind-down-route", 7),
            ("wind-down-deprecate", 7),
            ("wind-down-tombstone", 7),
            ("wind-down-exit", 7),
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../platform/sdk/conformance/fixtures")
                .join(format!("native-program-{name}-v3.json"));
            let source = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let document = crate::json::parse(&source).map_err(|error| error.to_string())?;
            assert_eq!(
                document
                    .u64_at("protocol_version")
                    .map_err(|error| error.to_string())?,
                3
            );
            assert_eq!(
                document
                    .u64_at("module")
                    .map_err(|error| error.to_string())?,
                9
            );
            assert_eq!(
                document
                    .u64_at("ordinal")
                    .map_err(|error| error.to_string())?,
                ordinal
            );
            let payload_hex = document
                .str_at("payload_hex")
                .map_err(|error| error.to_string())?;
            assert!(!payload_hex.is_empty() && payload_hex.len().is_multiple_of(2));
            assert!(payload_hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
            let payload = crate::json::decode_hex(&format!("0x{payload_hex}"))?;
            let encoded = match ordinal {
                1 => NativeProgramDeploy::decode(&payload).and_then(|value| value.encode()),
                2 => NativeProgramUpgrade::decode(&payload).and_then(|value| value.encode()),
                7 => NativeProgramWindDown::decode(&payload).and_then(|value| value.encode()),
                _ => return Err("unexpected lifecycle ordinal".into()),
            }
            .map_err(|error| format!("{name}: {error:?}"))?;
            assert_eq!(encoded, payload);
        }
        Ok(())
    }

    #[test]
    fn deploy_framing_and_policy_refusals() -> Result<(), InvalidNativeLifecycle> {
        let mut deploy = NativeProgramDeploy {
            program_id: ProgramId::new([1; 32]),
            guest_abi: 2,
            policy: ProgramUpgradePolicy::Authority([2; 32]),
            new_hash: [3; 32],
            interface: None,
            wasm: b"\0asm\x01\0\0\0",
        };
        for interface in [None, Some(&b"interface"[..])] {
            deploy.interface = interface;
            let encoded = deploy.encode()?;
            assert_eq!(NativeProgramDeploy::decode(&encoded)?, deploy);
            for length in 0..encoded.len() {
                assert!(NativeProgramDeploy::decode(&encoded[..length]).is_err());
            }
            let mut trailing = encoded.clone();
            trailing.push(0);
            assert!(NativeProgramDeploy::decode(&trailing).is_err());
            let mut bad_policy = encoded.clone();
            bad_policy[34] = 0;
            assert!(NativeProgramDeploy::decode(&bad_policy).is_err());
            let mut reserved = encoded;
            reserved[35] = 1;
            assert!(NativeProgramDeploy::decode(&reserved).is_err());
        }
        deploy.policy = ProgramUpgradePolicy::Authority([0; 32]);
        assert!(deploy.encode().is_err());
        deploy.policy = ProgramUpgradePolicy::Immutable;
        deploy.interface = Some(&[]);
        assert!(deploy.encode().is_err());
        deploy.interface = Some(&[0; MAX_INTERFACE_BYTES + 1]);
        assert!(deploy.encode().is_err());
        Ok(())
    }

    #[test]
    fn upgrade_framing_hook_and_clear_refusals() -> Result<(), InvalidNativeLifecycle> {
        let mut upgrade = NativeProgramUpgrade {
            program_id: ProgramId::new([1; 32]),
            guest_abi: 2,
            old_hash: [2; 32],
            new_hash: [3; 32],
            migration_hook: b"migrate",
            clear_interface: false,
            interface: None,
            wasm: b"\0asm\x01\0\0\0",
        };
        for (interface, clear) in [
            (None, false),
            (Some(&b"interface"[..]), false),
            (Some(&b""[..]), true),
        ] {
            upgrade.interface = interface;
            upgrade.clear_interface = clear;
            let encoded = upgrade.encode()?;
            assert_eq!(NativeProgramUpgrade::decode(&encoded)?, upgrade);
            for length in 0..encoded.len() {
                assert!(NativeProgramUpgrade::decode(&encoded[..length]).is_err());
            }
            let mut trailing = encoded.clone();
            trailing.push(0);
            assert!(NativeProgramUpgrade::decode(&trailing).is_err());
            for mask in [1, 4, 128] {
                let mut invalid = encoded.clone();
                invalid[34] ^= mask;
                assert!(NativeProgramUpgrade::decode(&invalid).is_err());
            }
        }
        upgrade.interface = None;
        upgrade.clear_interface = true;
        assert!(upgrade.encode().is_err());
        upgrade.interface = Some(&[]);
        upgrade.clear_interface = false;
        assert!(upgrade.encode().is_err());
        Ok(())
    }

    #[test]
    fn wind_down_operations_require_exact_lengths() -> Result<(), InvalidNativeLifecycle> {
        for (operation, length) in [
            (
                ProgramWindDownOperation::Route {
                    account: [2; 32],
                    asset: [3; 32],
                    destination: [4; 32],
                    seed: b"seed",
                },
                135,
            ),
            (
                ProgramWindDownOperation::Deprecate {
                    exit_program: [5; 32],
                    deadline_batch: 42,
                },
                73,
            ),
            (ProgramWindDownOperation::Tombstone, 33),
            (ProgramWindDownOperation::Exit { account: [6; 32] }, 65),
        ] {
            let value = NativeProgramWindDown {
                program_id: ProgramId::new([1; 32]),
                operation,
            };
            let mut encoded = value.encode()?;
            assert_eq!(encoded.len(), length);
            assert_eq!(NativeProgramWindDown::decode(&encoded)?, value);
            for prefix in 0..encoded.len() {
                assert!(NativeProgramWindDown::decode(&encoded[..prefix]).is_err());
            }
            encoded.push(0);
            assert!(NativeProgramWindDown::decode(&encoded).is_err());
        }
        let oversized = NativeProgramWindDown {
            program_id: ProgramId::new([1; 32]),
            operation: ProgramWindDownOperation::Route {
                account: [2; 32],
                asset: [3; 32],
                destination: [4; 32],
                seed: &[0; MAX_SEED_BYTES + 1],
            },
        };
        assert!(oversized.encode().is_err());
        Ok(())
    }
}
