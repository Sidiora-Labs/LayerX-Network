//! Canonical decoding for the terminal availability bytes produced by the Programs call bridge.

use crate::{BudgetMeterRefusal, BudgetResourceKind, MeteredUsage, ProgramFailure, TransferLawError, DEFAULT_MAX_CALL_GRAPH_EDGES, MAX_CALL_RESPONSE_BYTES, MAX_TRACE_COMMITMENTS};

const EXECUTION_V2: &[u8] = b"LXP/program-execution/v2\0";
const EXECUTION_V3: &[u8] = b"LXP/program-execution/v3\0";
const EXECUTION_V4: &[u8] = b"LXP/program-execution/v4\0";
const OCCUPANCY: &[u8] = b"LXP/program-execution-with-occupancy/v1\0";
const AUTHORITY: &[u8] = b"LXP/program-execution-with-transfer-authority/v2\0";
const FAILURE: &[u8] = b"LXP/programs/failure-detail/v1\0";
const RESOURCE: &[u8] = b"LXP/programs/resource-detail/v1\0";
const SETTLEMENT: &[u8] = b"LXP/programs/settlement-failure/v1\0";
const CALLBACK: &[u8] = b"LXP/programs/callback-failure/v1\0";
const MAX_TRACE_EVIDENCE_BYTES: usize = 34 + MAX_TRACE_COMMITMENTS * 52;
const MAX_GRAPH_EVIDENCE_BYTES: usize = b"LayerX/programs/call-graph/v1\0".len()
    + 32 + 16 + 8 + DEFAULT_MAX_CALL_GRAPH_EDGES as usize * 68;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalAttachment {
    Occupancy(Vec<u8>),
    TransferAuthority { authorization: Vec<u8>, transfer_root: [u8; 32] },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionValue { I32(i32), I64(i64) }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionTerminal {
    Legacy { encoding_version: u8, runtime_version: u16, abi_version: u16, metering_schedule_version: u32,
        values: Vec<ExecutionValue>, usage: MeteredUsage, trace: Option<Vec<u8>> },
    CandidateV4 { runtime_version: u16, fee_schedule_version: u32,
        metering_schedule_version: u32, program: [u8; 32], abi_version: u16,
        values: Vec<ExecutionValue>, usage: MeteredUsage, trace: Option<Vec<u8>>, graph: Vec<u8>,
        outcome: CandidateTerminalOutcome },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateTerminalOutcome {
    Success { code: i32, response: Vec<u8> },
    Failure(ProgramFailure),
    Resource(BudgetMeterRefusal),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FailureTerminal {
    Program(ProgramFailure),
    Composition { tag: u8, fields: FailureFields },
    Entrypoint { tag: u8, fields: FailureFields },
    Abi { tag: u8, fields: FailureFields },
    Settlement(TransferLawError),
    Callback { stage: u8, status: i32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FailureFields {
    None,
    Code(i32),
    Sizes { first: u64, second: u64 },
    Bounds { limit: u32, attempted: u32 },
    Program([u8; 32]),
    ProgramCode { program: [u8; 32], code: i32 },
    ProgramBounds { program: [u8; 32], limit: u32, attempted: u32 },
    Revisions { expected: u8, actual: u8 },
    MeteringPlans { expected: [u8; 76], actual: [u8; 76] },
    ProgramFailure(ProgramFailure),
    Abi(AbiFailure),
    Fault(ExecutionFaultFailure),
    Meter(MeterFailure),
    Response(ResponseFailure),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AbiFailure { Simple(u8), Storage(u8), Meter(MeterFailure) }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MeterFailure {
    BudgetExceeded { resource: u8, limit: u64, attempted: u64 },
    CounterOverflow { resource: u8 },
    FeeOverflow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionFaultFailure {
    Named { tag: u8, name: String },
    Simple(u8),
    Resource(MeterFailure),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResponseFailure {
    Sizes { tag: u8, bytes: u64, bound: u64 },
    Simple(u8),
    CodeMismatch { published: i32, returned: i32 },
    Meter(MeterFailure),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalDetail {
    Execution(ExecutionTerminal),
    Failure(FailureTerminal),
    Resource(BudgetMeterRefusal),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedTerminal {
    pub detail: TerminalDetail,
    pub attachments: Vec<TerminalAttachment>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalDecodeError { Malformed, MismatchedKind, MismatchedAbi }

pub fn decode_terminal_payload(kind: u8, abi: u16, encoded: &[u8]) -> Result<DecodedTerminal, TerminalDecodeError> {
    let (inner, attachments) = unwrap(encoded)?;
    let detail = if inner.starts_with(EXECUTION_V2) || inner.starts_with(EXECUTION_V3) && abi == 1 {
        if kind != 1 { return Err(TerminalDecodeError::MismatchedKind); }
        if abi != 1 { return Err(TerminalDecodeError::MismatchedAbi); }
        TerminalDetail::Execution(decode_legacy(inner, abi, inner.starts_with(EXECUTION_V3))?)
    } else if inner.starts_with(EXECUTION_V4) {
        let execution = decode_v4(inner, abi)?;
        let expected = match &execution {
            ExecutionTerminal::CandidateV4 { outcome: CandidateTerminalOutcome::Success { .. }, .. } => 1,
            ExecutionTerminal::CandidateV4 { outcome: CandidateTerminalOutcome::Failure(_), .. } => 2,
            ExecutionTerminal::CandidateV4 { outcome: CandidateTerminalOutcome::Resource(_), .. } => 3,
            ExecutionTerminal::Legacy { .. } => return Err(TerminalDecodeError::Malformed),
        };
        if kind != expected { return Err(TerminalDecodeError::MismatchedKind); }
        TerminalDetail::Execution(execution)
    } else if inner.starts_with(FAILURE) {
        if kind != 2 { return Err(TerminalDecodeError::MismatchedKind); }
        TerminalDetail::Failure(decode_failure(inner)?)
    } else if inner.starts_with(RESOURCE) {
        if kind != 3 { return Err(TerminalDecodeError::MismatchedKind); }
        TerminalDetail::Resource(decode_resource(&inner[RESOURCE.len()..])?)
    } else if inner.starts_with(SETTLEMENT) {
        if kind != 2 || inner.len() != SETTLEMENT.len() + 1 { return Err(TerminalDecodeError::Malformed); }
        TerminalDetail::Failure(FailureTerminal::Settlement(transfer_error(inner[SETTLEMENT.len()])?))
    } else if inner.starts_with(CALLBACK) {
        if kind != 2 || inner.len() != CALLBACK.len() + 5 { return Err(TerminalDecodeError::Malformed); }
        TerminalDetail::Failure(FailureTerminal::Callback { stage: inner[CALLBACK.len()],
            status: i32::from_be_bytes(inner[CALLBACK.len()+1..].try_into().map_err(|_| TerminalDecodeError::Malformed)?) })
    } else { return Err(TerminalDecodeError::Malformed); };
    Ok(DecodedTerminal { detail, attachments })
}

fn unwrap(encoded: &[u8]) -> Result<(&[u8], Vec<TerminalAttachment>), TerminalDecodeError> {
    let mut inner = encoded; let mut attachments = Vec::new();
    if inner.starts_with(AUTHORITY) {
        let mut cursor = Cursor::new(&inner[AUTHORITY.len()..]);
        let execution = cursor.sized_u32()?; let authorization = cursor.sized_u32()?.to_vec();
        let transfer_root = cursor.array()?; cursor.end()?;
        attachments.push(TerminalAttachment::TransferAuthority { authorization, transfer_root });
        inner = execution;
    }
    if inner.starts_with(OCCUPANCY) {
        let mut cursor = Cursor::new(&inner[OCCUPANCY.len()..]);
        let execution = cursor.sized_u32()?; let occupancy = cursor.sized_u32()?.to_vec(); cursor.end()?;
        attachments.push(TerminalAttachment::Occupancy(occupancy)); inner = execution;
    }
    if inner.starts_with(AUTHORITY) || inner.starts_with(OCCUPANCY) { return Err(TerminalDecodeError::Malformed); }
    Ok((inner, attachments))
}

fn decode_legacy(encoded: &[u8], expected_abi: u16, traced: bool) -> Result<ExecutionTerminal, TerminalDecodeError> {
    let domain = if traced { EXECUTION_V3 } else { EXECUTION_V2 };
    let mut c = Cursor::new(&encoded[domain.len()..]);
    let runtime_version = c.u16()?; let abi_version = c.u16()?;
    let metering_schedule_version = c.u32()?;
    if abi_version != expected_abi || runtime_version == 0 || metering_schedule_version == 0 { return Err(TerminalDecodeError::MismatchedAbi); }
    let count_bytes: [u8; 16] = c.array()?;
    let count = u128::from_be_bytes(count_bytes);
    let count = usize::try_from(count).map_err(|_| TerminalDecodeError::Malformed)?;
    if count > c.remaining() / 5 { return Err(TerminalDecodeError::Malformed); }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count { values.push(match c.byte()? { 1 => ExecutionValue::I32(c.i32()?), 2 => ExecutionValue::I64(c.i64()?), _ => return Err(TerminalDecodeError::Malformed) }); }
    let usage = MeteredUsage { cpu_fuel:c.u64()?, memory_bytes:c.u64()?, storage_read_bytes:c.u64()?,
        storage_write_bytes:c.u64()?, output_values:c.u32()?, output_bytes:0, occupancy_byte_batches:0,
        occupancy_fee_units:0, fee_units:u128::from_be_bytes(c.array()?) };
    let trace = if traced { if c.byte()? != 1 { return Err(TerminalDecodeError::Malformed); } let bytes=c.sized_u64()?;if bytes.len()>MAX_TRACE_EVIDENCE_BYTES{return Err(TerminalDecodeError::Malformed)} Some(bytes.to_vec()) } else { None };
    c.end()?;
    Ok(ExecutionTerminal::Legacy { encoding_version:if traced{3}else{2}, runtime_version, abi_version, metering_schedule_version, values, usage, trace })
}

fn decode_v4(encoded: &[u8], expected_abi: u16) -> Result<ExecutionTerminal, TerminalDecodeError> {
    let mut c = Cursor::new(&encoded[EXECUTION_V4.len()..]);
    let runtime_version = c.u16()?; let fee_schedule_version = c.u32()?; let metering_schedule_version = c.u32()?;
    let count = usize::try_from(c.u64()?).map_err(|_| TerminalDecodeError::Malformed)?;
    if count > c.remaining() / 5 { return Err(TerminalDecodeError::Malformed); }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count { values.push(match c.byte()? { 1 => ExecutionValue::I32(c.i32()?), 2 => ExecutionValue::I64(c.i64()?), _ => return Err(TerminalDecodeError::Malformed) }); }
    let usage = MeteredUsage { cpu_fuel:c.u64()?, memory_bytes:c.u64()?, storage_read_bytes:c.u64()?,
        storage_write_bytes:c.u64()?, output_values:c.u32()?, output_bytes:c.u64()?, occupancy_byte_batches:0,
        occupancy_fee_units:0, fee_units:u128::from_be_bytes(c.array()?) };
    let trace = match c.byte()? { 0 => None, 1 => {let bytes=c.sized_u64()?;if bytes.len()>MAX_TRACE_EVIDENCE_BYTES{return Err(TerminalDecodeError::Malformed)} Some(bytes.to_vec())}, _ => return Err(TerminalDecodeError::Malformed) };
    let program = c.array()?; let abi_version = c.u16()?;
    if abi_version != expected_abi || abi_version != 2 || runtime_version == 0 || fee_schedule_version == 0 || metering_schedule_version == 0 { return Err(TerminalDecodeError::MismatchedAbi); }
    let outcome = match c.byte()? {
        0 => { let code = c.i32()?; if code < 0 { return Err(TerminalDecodeError::Malformed); } let response=c.sized_u64()?; if response.len()>MAX_CALL_RESPONSE_BYTES{return Err(TerminalDecodeError::Malformed)} CandidateTerminalOutcome::Success { code, response: response.to_vec() } },
        1 => CandidateTerminalOutcome::Failure(ProgramFailure::canonical_decode(c.sized_u64()?).map_err(|_| TerminalDecodeError::Malformed)?),
        2 => CandidateTerminalOutcome::Resource(decode_meter(&mut c, usage)?),
        _ => return Err(TerminalDecodeError::Malformed),
    };
    let graph=c.sized_u64()?; if graph.len()>MAX_GRAPH_EVIDENCE_BYTES{return Err(TerminalDecodeError::Malformed)} c.end()?;
    Ok(ExecutionTerminal::CandidateV4 { runtime_version, fee_schedule_version, metering_schedule_version, program, abi_version, values, usage, trace, graph:graph.to_vec(), outcome })
}

fn decode_failure(encoded: &[u8]) -> Result<FailureTerminal, TerminalDecodeError> {
    let mut c = Cursor::new(&encoded[FAILURE.len()..]);
    let tag = c.byte()?; let payload = c.sized_u32()?; c.end()?;
    Ok(match tag {
        1 => FailureTerminal::Program(ProgramFailure::canonical_decode(payload).map_err(|_| TerminalDecodeError::Malformed)?),
        2 => { let (tag, fields)=decode_composition(payload)?; FailureTerminal::Composition { tag, fields } },
        3 => { let (tag, fields)=decode_entrypoint(payload)?; FailureTerminal::Entrypoint { tag, fields } },
        4 => { let (tag, fields)=decode_abi(payload)?; FailureTerminal::Abi { tag, fields } }, _ => return Err(TerminalDecodeError::Malformed),
    })
}

fn decode_composition(payload:&[u8])->Result<(u8,FailureFields),TerminalDecodeError>{
    let mut c=Cursor::new(payload); let tag=c.byte()?;
    let fields=match tag{
        1|9|10|11|20|21|22=>FailureFields::None,
        2=>{let expected=c.byte()?;let actual=c.byte()?;if !matches!(expected,1|2)||!matches!(actual,1|2){return Err(TerminalDecodeError::Malformed)}FailureFields::Revisions{expected,actual}},
        23=>FailureFields::MeteringPlans{expected:c.array()?,actual:c.array()?},
        3|4=>FailureFields::Program(c.array()?),
        5|6|7=>FailureFields::Bounds{limit:c.u32()?,attempted:c.u32()?},
        8=>FailureFields::ProgramBounds{program:c.array()?,limit:c.u32()?,attempted:c.u32()?},
        12=>FailureFields::Code(c.i32()?),
        13=>FailureFields::Sizes{first:c.u64()?,second:c.u64()?},
        14=>FailureFields::ProgramCode{program:c.array()?,code:c.i32()?},
        15=>FailureFields::ProgramFailure(ProgramFailure::canonical_decode(c.rest()).map_err(|_|TerminalDecodeError::Malformed)?),
        16=>FailureFields::Abi(decode_abi_fields(&mut c)?),
        17=>FailureFields::Fault(decode_fault(&mut c)?),
        18=>FailureFields::Meter(decode_meter_failure(&mut c)?),
        19=>FailureFields::Response(decode_response(&mut c)?),
        _=>return Err(TerminalDecodeError::Malformed),
    }; c.end()?; Ok((tag,fields))
}
fn decode_entrypoint(payload:&[u8])->Result<(u8,FailureFields),TerminalDecodeError>{
    let mut c=Cursor::new(payload);let tag=c.byte()?;let fields=match tag{
        1=>FailureFields::Sizes{first:c.u64()?,second:c.u64()?},2|3|4=>FailureFields::None,
        5|6=>FailureFields::Code(c.i32()?),7=>FailureFields::Fault(decode_fault(&mut c)?),
        8=>FailureFields::Meter(decode_meter_failure(&mut c)?),_=>return Err(TerminalDecodeError::Malformed)};
    c.end()?;Ok((tag,fields))
}
fn decode_abi(payload:&[u8])->Result<(u8,FailureFields),TerminalDecodeError>{
    let mut c=Cursor::new(payload);let tag=c.byte()?;let fields=match tag{
        1..=10|13..=15=>FailureFields::None,
        11=>{let storage=c.byte()?;if !matches!(storage,1..=11){return Err(TerminalDecodeError::Malformed)}FailureFields::Abi(AbiFailure::Storage(storage))},
        12=>FailureFields::Meter(decode_meter_failure(&mut c)?),_=>return Err(TerminalDecodeError::Malformed)};
    c.end()?;Ok((tag,fields))
}
fn decode_abi_fields(c:&mut Cursor<'_>)->Result<AbiFailure,TerminalDecodeError>{let tag=c.byte()?;Ok(match tag{1..=10|13..=15=>AbiFailure::Simple(tag),11=>{let storage=c.byte()?;if !matches!(storage,1..=11){return Err(TerminalDecodeError::Malformed)}AbiFailure::Storage(storage)},12=>AbiFailure::Meter(decode_meter_failure(c)?),_=>return Err(TerminalDecodeError::Malformed)})}
fn decode_meter_failure(c:&mut Cursor<'_>)->Result<MeterFailure,TerminalDecodeError>{let tag=c.byte()?;Ok(match tag{1=>{let resource=c.byte()?;if !matches!(resource,1..=7){return Err(TerminalDecodeError::Malformed)}let limit=c.u64()?;let attempted=c.u64()?;if attempted<=limit{return Err(TerminalDecodeError::Malformed)}MeterFailure::BudgetExceeded{resource,limit,attempted}},2=>{let resource=c.byte()?;if !matches!(resource,1..=7){return Err(TerminalDecodeError::Malformed)}MeterFailure::CounterOverflow{resource}},3=>MeterFailure::FeeOverflow,_=>return Err(TerminalDecodeError::Malformed)})}
fn decode_fault(c:&mut Cursor<'_>)->Result<ExecutionFaultFailure,TerminalDecodeError>{let tag=c.byte()?;Ok(match tag{1|2|16=>{let bytes=c.sized_u32()?;let name=core::str::from_utf8(bytes).map_err(|_|TerminalDecodeError::Malformed)?.to_owned();ExecutionFaultFailure::Named{tag,name}},3..=13|15=>ExecutionFaultFailure::Simple(tag),14=>ExecutionFaultFailure::Resource(decode_meter_failure(c)?),_=>return Err(TerminalDecodeError::Malformed)})}
fn decode_response(c:&mut Cursor<'_>)->Result<ResponseFailure,TerminalDecodeError>{let tag=c.byte()?;Ok(match tag{1|2=>ResponseFailure::Sizes{tag,bytes:c.u64()?,bound:c.u64()?},3|4=>ResponseFailure::Simple(tag),5=>ResponseFailure::CodeMismatch{published:c.i32()?,returned:c.i32()?},6=>ResponseFailure::Meter(decode_meter_failure(c)?),_=>return Err(TerminalDecodeError::Malformed)})}

fn decode_resource(encoded: &[u8]) -> Result<BudgetMeterRefusal, TerminalDecodeError> {
    let mut c = Cursor::new(encoded); let tag = c.byte()?; let resource = resource(c.byte()?)?;
    let result = match tag { 1 => { let limit=c.u64()?;let attempted=c.u64()?;if attempted<=limit{return Err(TerminalDecodeError::Malformed)} BudgetMeterRefusal::BudgetExceeded { resource, limit, attempted } },
        2 => BudgetMeterRefusal::CounterOverflow { resource }, _ => return Err(TerminalDecodeError::Malformed) };
    c.end()?; Ok(result)
}
fn decode_meter(c: &mut Cursor<'_>, usage: MeteredUsage) -> Result<BudgetMeterRefusal, TerminalDecodeError> {
    let tag = c.byte()?; let resource = candidate_resource(c.byte()?)?;
    match tag { 0 => { let limit=c.u64()?;let attempted=c.u64()?;if attempted<=limit||usage_for(usage,resource)>limit{return Err(TerminalDecodeError::Malformed)} Ok(BudgetMeterRefusal::BudgetExceeded { resource, limit, attempted }) },
        1 => Ok(BudgetMeterRefusal::CounterOverflow { resource }), _ => Err(TerminalDecodeError::Malformed) }
}
fn resource(tag: u8) -> Result<BudgetResourceKind, TerminalDecodeError> { Ok(match tag { 1=>BudgetResourceKind::Cpu,2=>BudgetResourceKind::Memory,3=>BudgetResourceKind::StorageRead,4=>BudgetResourceKind::StorageWrite,5=>BudgetResourceKind::Output,6=>BudgetResourceKind::OutputBytes,7=>BudgetResourceKind::Table,_=>return Err(TerminalDecodeError::Malformed) }) }
fn candidate_resource(tag:u8)->Result<BudgetResourceKind,TerminalDecodeError>{Ok(match tag{0=>BudgetResourceKind::Cpu,1=>BudgetResourceKind::Memory,2=>BudgetResourceKind::StorageRead,3=>BudgetResourceKind::StorageWrite,4=>BudgetResourceKind::Output,5=>BudgetResourceKind::OutputBytes,6=>BudgetResourceKind::Table,_=>return Err(TerminalDecodeError::Malformed)})}
fn usage_for(usage:MeteredUsage,resource:BudgetResourceKind)->u64{match resource{BudgetResourceKind::Cpu=>usage.cpu_fuel,BudgetResourceKind::Memory=>usage.memory_bytes,BudgetResourceKind::StorageRead=>usage.storage_read_bytes,BudgetResourceKind::StorageWrite=>usage.storage_write_bytes,BudgetResourceKind::Output=>u64::from(usage.output_values),BudgetResourceKind::OutputBytes=>usage.output_bytes,BudgetResourceKind::Table=>0}}
fn transfer_error(tag: u8) -> Result<TransferLawError, TerminalDecodeError> { Ok(match tag { 1=>TransferLawError::UnverifiedAuthority,11=>TransferLawError::InvalidProgramAuthority,12=>TransferLawError::InvalidProgramFunding,2=>TransferLawError::InvalidTransfer,3=>TransferLawError::InvalidTransferSet,4=>TransferLawError::AmountOverflow,5=>TransferLawError::InvariantViolation,6=>TransferLawError::CapabilityEscalation,7=>TransferLawError::KernelRefused,8=>TransferLawError::ReceiptInvalid,9=>TransferLawError::ReceiptMismatch,10=>TransferLawError::StaleStorage,_=>return Err(TerminalDecodeError::Malformed) }) }

struct Cursor<'a> { bytes: &'a [u8], offset: usize }
impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self { Self { bytes, offset: 0 } }
    fn take(&mut self, n: usize) -> Result<&'a [u8], TerminalDecodeError> { let end=self.offset.checked_add(n).ok_or(TerminalDecodeError::Malformed)?; let out=self.bytes.get(self.offset..end).ok_or(TerminalDecodeError::Malformed)?; self.offset=end; Ok(out) }
    fn array<const N:usize>(&mut self)->Result<[u8;N],TerminalDecodeError>{self.take(N)?.try_into().map_err(|_|TerminalDecodeError::Malformed)}
    fn byte(&mut self)->Result<u8,TerminalDecodeError>{Ok(self.take(1)?[0])}
    fn u16(&mut self)->Result<u16,TerminalDecodeError>{Ok(u16::from_be_bytes(self.array()?))}
    fn u32(&mut self)->Result<u32,TerminalDecodeError>{Ok(u32::from_be_bytes(self.array()?))}
    fn u64(&mut self)->Result<u64,TerminalDecodeError>{Ok(u64::from_be_bytes(self.array()?))}
    fn i32(&mut self)->Result<i32,TerminalDecodeError>{Ok(i32::from_be_bytes(self.array()?))}
    fn i64(&mut self)->Result<i64,TerminalDecodeError>{Ok(i64::from_be_bytes(self.array()?))}
    fn sized_u32(&mut self)->Result<&'a [u8],TerminalDecodeError>{let n=usize::try_from(self.u32()?).map_err(|_|TerminalDecodeError::Malformed)?;self.take(n)}
    fn sized_u64(&mut self)->Result<&'a [u8],TerminalDecodeError>{let n=usize::try_from(self.u64()?).map_err(|_|TerminalDecodeError::Malformed)?;self.take(n)}
    fn end(&self)->Result<(),TerminalDecodeError>{if self.offset==self.bytes.len(){Ok(())}else{Err(TerminalDecodeError::Malformed)}}
    fn remaining(&self)->usize{self.bytes.len().saturating_sub(self.offset)}
    fn rest(&mut self)->&'a [u8]{let out=&self.bytes[self.offset..];self.offset=self.bytes.len();out}
}

#[cfg(test)]
mod source_vectors {
    use super::{decode_terminal_payload, CandidateTerminalOutcome, ExecutionTerminal, TerminalDetail};

    #[test]
    fn current_candidate_v4_success_decodes_exactly() {
        let mut bytes=b"LXP/program-execution/v4\0".to_vec();
        bytes.extend_from_slice(&1_u16.to_be_bytes()); bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&1_u32.to_be_bytes()); bytes.extend_from_slice(&0_u64.to_be_bytes());
        for value in [1_u64,2,3,4] { bytes.extend_from_slice(&value.to_be_bytes()); }
        bytes.extend_from_slice(&0_u32.to_be_bytes()); bytes.extend_from_slice(&0_u64.to_be_bytes());
        bytes.extend_from_slice(&10_u128.to_be_bytes()); bytes.push(0); bytes.extend_from_slice(&[7;32]);
        bytes.extend_from_slice(&2_u16.to_be_bytes()); bytes.push(0); bytes.extend_from_slice(&0_i32.to_be_bytes());
        bytes.extend_from_slice(&2_u64.to_be_bytes()); bytes.extend_from_slice(&[0xaa,0xbb]);
        bytes.extend_from_slice(&0_u64.to_be_bytes());
        let Ok(decoded)=decode_terminal_payload(1,2,&bytes) else { panic!("producer vector rejected") };
        assert!(matches!(decoded.detail,TerminalDetail::Execution(ExecutionTerminal::CandidateV4{outcome:CandidateTerminalOutcome::Success{code:0,response},..}) if response==[0xaa,0xbb]));
    }

    #[test]
    fn traced_abi_one_execution_v3_is_not_candidate_evidence() {
        let mut bytes=b"LXP/program-execution/v3\0".to_vec();
        bytes.extend_from_slice(&1_u16.to_be_bytes()); bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&1_u32.to_be_bytes()); bytes.extend_from_slice(&0_u128.to_be_bytes());
        for value in [1_u64,2,3,4] { bytes.extend_from_slice(&value.to_be_bytes()); }
        bytes.extend_from_slice(&0_u32.to_be_bytes()); bytes.extend_from_slice(&10_u128.to_be_bytes());
        bytes.push(1); bytes.extend_from_slice(&0_u64.to_be_bytes());
        let Ok(decoded)=decode_terminal_payload(1,1,&bytes) else { panic!("traced ABI1 producer vector rejected") };
        assert!(matches!(decoded.detail,TerminalDetail::Execution(ExecutionTerminal::Legacy{encoding_version:3,..})));
    }

    #[test]
    fn producer_impossible_wrapper_order_is_refused() {
        let mut inner=b"LXP/program-execution-with-transfer-authority/v2\0".to_vec();
        inner.extend_from_slice(&0_u32.to_be_bytes()); inner.extend_from_slice(&0_u32.to_be_bytes()); inner.extend_from_slice(&[1;32]);
        let mut outer=b"LXP/program-execution-with-occupancy/v1\0".to_vec();
        outer.extend_from_slice(&(inner.len() as u32).to_be_bytes()); outer.extend_from_slice(&inner); outer.extend_from_slice(&0_u32.to_be_bytes());
        assert!(decode_terminal_payload(1,2,&outer).is_err());
    }
}
