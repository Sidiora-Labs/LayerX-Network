use crate::lni::refusal::decode_core_refusal;
use crate::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use crate::lni::transport::FrameTransport;
use crate::read::ReadError;

#[derive(Clone, Copy, Debug)]
pub struct SnapshotContext {
    pub interface_version: Version,
    pub correlation_id: u64,
    pub minimum_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedSnapshot<T> {
    pub observed_sequence: u64,
    pub state_root: [u8; 32],
    pub value: T,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetMetadata {
    pub asset_id: [u8; 32],
    pub symbol: Vec<u8>,
    pub name: String,
    pub decimals: u8,
    pub custody_kind: u8,
    pub custody_reference: Vec<u8>,
    pub paused: bool,
    pub supply_cap: u128,
    pub issuer_did: [u8; 32],
    pub issuer_kind: u8,
    pub total_units: u128,
    pub salt: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeEstimate {
    pub parameter_version: u32,
    pub fee: u128,
    pub canonical_schedule: Vec<u8>,
}

fn take<'a>(bytes: &mut &'a [u8], count: usize) -> Result<&'a [u8], ReadError> {
    if count > bytes.len() {
        return Err(ReadError::MalformedValue);
    }
    let (value, tail) = bytes.split_at(count);
    *bytes = tail;
    Ok(value)
}

fn fixed<const N: usize>(bytes: &mut &[u8]) -> Result<[u8; N], ReadError> {
    take(bytes, N)?
        .try_into()
        .map_err(|_| ReadError::MalformedValue)
}

fn metadata(mut bytes: &[u8]) -> Result<AssetMetadata, ReadError> {
    if u16::from_be_bytes(fixed(&mut bytes)?) != 3 {
        return Err(ReadError::MalformedValue);
    }
    let asset_id = fixed(&mut bytes)?;
    let symbol_length = usize::from(fixed::<1>(&mut bytes)?[0]);
    if !(1..=16).contains(&symbol_length) {
        return Err(ReadError::MalformedValue);
    }
    let symbol = take(&mut bytes, symbol_length)?.to_vec();
    if !symbol.is_ascii() {
        return Err(ReadError::MalformedValue);
    }
    let decimals = fixed::<1>(&mut bytes)?[0];
    let custody_kind = fixed::<1>(&mut bytes)?[0];
    let reference_length = usize::from(u16::from_be_bytes(fixed(&mut bytes)?));
    if decimals > 38 || reference_length > 128 {
        return Err(ReadError::MalformedValue);
    }
    let custody_reference = take(&mut bytes, reference_length)?.to_vec();
    let pause = fixed::<1>(&mut bytes)?[0];
    let name_length = usize::from(fixed::<1>(&mut bytes)?[0]);
    if pause > 1 || name_length > 32 {
        return Err(ReadError::MalformedValue);
    }
    let name = String::from_utf8(take(&mut bytes, name_length)?.to_vec())
        .map_err(|_| ReadError::MalformedValue)?;
    let supply_cap = u128::from_be_bytes(fixed(&mut bytes)?);
    let issuer_did = fixed(&mut bytes)?;
    let issuer_kind = fixed::<1>(&mut bytes)?[0];
    let total_units = u128::from_be_bytes(fixed(&mut bytes)?);
    let salt = fixed(&mut bytes)?;
    if !bytes.is_empty()
        || issuer_kind > 2
        || (issuer_kind != 0 && (name.is_empty() || issuer_did == [0; 32]))
        || (issuer_kind == 1 && !custody_reference.is_empty())
        || (issuer_kind == 0 && custody_reference.is_empty())
    {
        return Err(ReadError::MalformedValue);
    }
    Ok(AssetMetadata {
        asset_id,
        symbol,
        name,
        decimals,
        custody_kind,
        custody_reference,
        paused: pause == 1,
        supply_cap,
        issuer_did,
        issuer_kind,
        total_units,
        salt,
    })
}

fn request(
    transport: &mut dyn FrameTransport,
    tag: u16,
    payload: &[u8],
    context: SnapshotContext,
) -> Result<CommittedSnapshot<Vec<u8>>, ReadError> {
    if context.interface_version.major != 1
        || context.interface_version.minor < 5
        || context.correlation_id == 0
    {
        return Err(ReadError::UnavailableCapability);
    }
    transport.send(&encode_envelope(Envelope {
        version: context.interface_version,
        message_tag: tag,
        correlation_id: context.correlation_id,
        canonical_payload: payload,
        proof_material: &[],
    })?)?;
    let response_bytes = transport.receive()?;
    let response = decode_envelope(&response_bytes)?;
    if response.version.major != 1
        || response.version.minor < 5
        || response.correlation_id != context.correlation_id
        || !response.proof_material.is_empty()
    {
        return Err(ReadError::UnexpectedResponse);
    }
    if response.message_tag == 25 {
        let refusal =
            decode_core_refusal(response.canonical_payload).ok_or(ReadError::UnexpectedResponse)?;
        return Err(ReadError::CoreRefusal {
            class: refusal.class,
            result: refusal.result,
        });
    }
    if response.message_tag != tag + 1 {
        return Err(ReadError::UnexpectedResponse);
    }
    let mut bytes = response.canonical_payload;
    if u16::from_be_bytes(fixed(&mut bytes)?) != 1 {
        return Err(ReadError::MalformedValue);
    }
    let observed_sequence = u64::from_be_bytes(fixed(&mut bytes)?);
    let state_root = fixed(&mut bytes)?;
    if observed_sequence < context.minimum_sequence || state_root == [0; 32] {
        return Err(ReadError::SelectorMismatch);
    }
    Ok(CommittedSnapshot {
        observed_sequence,
        state_root,
        value: bytes.to_vec(),
    })
}

pub fn list_assets(
    transport: &mut dyn FrameTransport,
    asset: Option<[u8; 32]>,
    context: SnapshotContext,
) -> Result<CommittedSnapshot<Vec<AssetMetadata>>, ReadError> {
    let mut payload = vec![0, 1, if asset.is_some() { 2 } else { 1 }];
    if let Some(id) = asset {
        if id == [0; 32] {
            return Err(ReadError::SelectorMismatch);
        }
        payload.extend_from_slice(&id);
    }
    let response = request(transport, 32, &payload, context)?;
    let mut bytes = response.value.as_slice();
    let count = usize::from(u16::from_be_bytes(fixed(&mut bytes)?));
    if count > 64 || (asset.is_some() && count != 1) {
        return Err(ReadError::MalformedValue);
    }
    let mut records: Vec<AssetMetadata> = Vec::with_capacity(count);
    for _ in 0..count {
        let length = usize::from(u16::from_be_bytes(fixed(&mut bytes)?));
        let record = metadata(take(&mut bytes, length)?)?;
        if asset.is_some_and(|id| id != record.asset_id)
            || records
                .last()
                .is_some_and(|last| last.asset_id >= record.asset_id)
        {
            return Err(ReadError::SelectorMismatch);
        }
        records.push(record);
    }
    if !bytes.is_empty() {
        return Err(ReadError::MalformedValue);
    }
    Ok(CommittedSnapshot {
        observed_sequence: response.observed_sequence,
        state_root: response.state_root,
        value: records,
    })
}

pub fn get_asset(
    transport: &mut dyn FrameTransport,
    asset: [u8; 32],
    context: SnapshotContext,
) -> Result<CommittedSnapshot<AssetMetadata>, ReadError> {
    let mut snapshot = list_assets(transport, Some(asset), context)?;
    let value = snapshot.value.pop().ok_or(ReadError::MalformedValue)?;
    Ok(CommittedSnapshot {
        observed_sequence: snapshot.observed_sequence,
        state_root: snapshot.state_root,
        value,
    })
}

pub fn estimate_fee(
    transport: &mut dyn FrameTransport,
    activity_type: u32,
    canonical_bytes: u64,
    execution_units: u64,
    storage_units: u64,
    context: SnapshotContext,
) -> Result<CommittedSnapshot<FeeEstimate>, ReadError> {
    let mut payload = vec![0, 1];
    payload.extend_from_slice(&activity_type.to_be_bytes());
    for value in [canonical_bytes, execution_units, storage_units] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    let response = request(transport, 34, &payload, context)?;
    let mut bytes = response.value.as_slice();
    let parameter_version = u32::from_be_bytes(fixed(&mut bytes)?);
    let fee = u128::from_be_bytes(fixed(&mut bytes)?);
    let length = usize::from(u16::from_be_bytes(fixed(&mut bytes)?));
    let schedule = take(&mut bytes, length)?;
    if !bytes.is_empty()
        || parameter_version == 0
        || !((length == 86 && schedule[..2] == [0, 1])
            || (length == 215 && schedule[..2] == [0, 2] && schedule[86] == 8))
    {
        return Err(ReadError::MalformedValue);
    }
    Ok(CommittedSnapshot {
        observed_sequence: response.observed_sequence,
        state_root: response.state_root,
        value: FeeEstimate {
            parameter_version,
            fee,
            canonical_schedule: schedule.to_vec(),
        },
    })
}
