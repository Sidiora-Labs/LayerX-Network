use std::collections::BTreeSet;
use std::time::Duration;

use layerx_types::intent::EvmAddress;

use super::{
    ChainSignal, ConfirmationProgress, EndpointSignal, FinalityEvidence, FinalityReport,
    FinalityStage,
};
use crate::client::{
    BlockRef, EndpointError, ExecutionOutcome, LogRecord, QuorumBinding, TransactionHash,
    TransactionInclusion, TransactionView,
};
use crate::rpc::{EndpointFailure, EndpointFault, EndpointTransport};
use crate::wire::NativeWireError;

const MAX_ENDPOINTS: usize = 64;
const MAX_FAILURES: usize = 64;
const MAX_LOGS: usize = 16_384;
const MAX_TOPICS: usize = 64;
const MAX_STRING: usize = 4_096;
const MAX_BLOB: usize = 2 * 1024 * 1024;
const MAX_TRUST_ANCHOR: usize = 256 * 1024;

pub(crate) fn encode(
    value: &FinalityReport,
    maximum: usize,
    version: u8,
    tag: u8,
) -> Result<Vec<u8>, NativeWireError> {
    validate(value)?;
    let mut w = Writer::new(maximum)?;
    w.u8(version)?;
    w.u8(tag)?;
    w.bytes(&value.transaction.bytes())?;
    stage(&mut w, value.stage)?;
    signal(&mut w, &value.signal)?;
    endpoint_signal(&mut w, &value.endpoint)?;
    w.u64(value.progress.confirmed)?;
    w.u64(value.progress.required)?;
    w.u64(value.displacements)?;
    w.u64(value.polls)?;
    match &value.evidence {
        None => w.u8(0)?,
        Some(e) => {
            w.u8(1)?;
            evidence(&mut w, e)?;
        }
    }
    Ok(w.out)
}

pub(crate) fn decode(
    bytes: &[u8],
    maximum: usize,
    version: u8,
    tag: u8,
) -> Result<FinalityReport, NativeWireError> {
    if maximum == 0 || bytes.len() > maximum {
        return Err(NativeWireError::Limit);
    }
    let mut r = Reader::new(bytes);
    if r.u8()? != version || r.u8()? != tag {
        return Err(NativeWireError::Encoding);
    }
    let transaction = TransactionHash::new(r.array()?);
    let stage = read_stage(&mut r)?;
    let signal = read_signal(&mut r)?;
    let endpoint = read_endpoint_signal(&mut r)?;
    let progress = ConfirmationProgress {
        confirmed: r.u64()?,
        required: r.u64()?,
    };
    let displacements = r.u64()?;
    let polls = r.u64()?;
    let evidence = match r.u8()? {
        0 => None,
        1 => Some(read_evidence(&mut r)?),
        _ => return Err(NativeWireError::Encoding),
    };
    r.finish()?;
    let report = FinalityReport {
        transaction,
        stage,
        signal,
        endpoint,
        progress,
        displacements,
        polls,
        evidence,
    };
    validate(&report)?;
    Ok(report)
}

fn validate(v: &FinalityReport) -> Result<(), NativeWireError> {
    if v.progress.required == 0 || v.progress.confirmed > v.progress.required {
        return Err(NativeWireError::Encoding);
    }
    match v.stage {
        FinalityStage::Announced
        | FinalityStage::Missing { .. }
        | FinalityStage::Pooled { .. }
        | FinalityStage::Displaced { .. }
            if v.progress.confirmed != 0 =>
        {
            return Err(NativeWireError::Encoding)
        }
        FinalityStage::Confirming {
            confirmations,
            required,
            ..
        }
        | FinalityStage::Final {
            confirmations,
            required,
            ..
        } if confirmations != v.progress.confirmed || required != v.progress.required => {
            return Err(NativeWireError::Encoding)
        }
        _ => {}
    }
    if let Some(e) = &v.evidence {
        if e.chain_id == 0
            || e.binding.chain_id() != e.chain_id
            || e.binding.endpoint_sources().is_empty()
            || e.binding.endpoint_sources().len() > MAX_ENDPOINTS
            || e.binding.minimum_agreement() == 0
            || e.binding.minimum_agreement() > e.binding.endpoint_sources().len()
        {
            return Err(NativeWireError::Encoding);
        }
        let mut ids = BTreeSet::new();
        for (id, transport) in e.binding.endpoint_sources() {
            if id.is_empty() || id.len() > MAX_STRING || !ids.insert(id) {
                return Err(NativeWireError::Encoding);
            }
            match transport {
                EndpointTransport::LocalEmulator
                    if !id.starts_with("http://127.0.0.1:")
                        && !id.starts_with("http://[::1]:")
                        && !id.starts_with("http://localhost:") =>
                {
                    return Err(NativeWireError::Encoding)
                }
                EndpointTransport::PinnedTls { trust_anchor_der }
                    if !id.starts_with("https://")
                        || trust_anchor_der.is_empty()
                        || trust_anchor_der.len() > MAX_TRUST_ANCHOR =>
                {
                    return Err(NativeWireError::Encoding)
                }
                _ => {}
            }
        }
        match (v.stage, e.transaction) {
            (
                FinalityStage::Confirming { inclusion, .. }
                | FinalityStage::Final { inclusion, .. },
                TransactionView::Included(actual),
            ) if inclusion == actual => {}
            (FinalityStage::Missing { .. }, TransactionView::Unknown)
            | (FinalityStage::Pooled { .. }, TransactionView::Pending)
            | (
                FinalityStage::Displaced { .. },
                TransactionView::Unknown | TransactionView::Pending | TransactionView::Included(_),
            ) => {}
            _ => return Err(NativeWireError::Encoding),
        }
        if let Some(block) = e.canonical_block {
            if block.number > e.head {
                return Err(NativeWireError::Encoding);
            }
        }
        if let Some(logs) = &e.receipt_logs {
            if logs.len() > MAX_LOGS {
                return Err(NativeWireError::Limit);
            }
            for l in logs {
                if l.topics.len() > MAX_TOPICS || l.data.len() > MAX_BLOB {
                    return Err(NativeWireError::Limit);
                }
            }
        }
    }
    validate_signal(&v.signal)
}

fn validate_signal(signal: &ChainSignal) -> Result<(), NativeWireError> {
    if let ChainSignal::Delayed {
        stalled_polls,
        threshold,
        stalled_for,
        delayed_after,
    } = signal
    {
        if *threshold == 0
            || stalled_polls < threshold
            || delayed_after.is_zero()
            || stalled_for < delayed_after
        {
            return Err(NativeWireError::Encoding);
        }
    }
    Ok(())
}

fn stage(w: &mut Writer, v: FinalityStage) -> Result<(), NativeWireError> {
    match v {
        FinalityStage::Announced => w.u8(0),
        FinalityStage::Missing { head } => {
            w.u8(1)?;
            w.u64(head)
        }
        FinalityStage::Pooled { head } => {
            w.u8(2)?;
            w.u64(head)
        }
        FinalityStage::Confirming {
            inclusion,
            confirmations,
            required,
        } => {
            w.u8(3)?;
            inclusion_write(w, inclusion)?;
            w.u64(confirmations)?;
            w.u64(required)
        }
        FinalityStage::Final {
            inclusion,
            confirmations,
            required,
        } => {
            w.u8(4)?;
            inclusion_write(w, inclusion)?;
            w.u64(confirmations)?;
            w.u64(required)
        }
        FinalityStage::Displaced {
            lost,
            head,
            requeued,
        } => {
            w.u8(5)?;
            inclusion_write(w, lost)?;
            w.u64(head)?;
            w.bool(requeued)
        }
    }
}
fn read_stage(r: &mut Reader<'_>) -> Result<FinalityStage, NativeWireError> {
    Ok(match r.u8()? {
        0 => FinalityStage::Announced,
        1 => FinalityStage::Missing { head: r.u64()? },
        2 => FinalityStage::Pooled { head: r.u64()? },
        3 => FinalityStage::Confirming {
            inclusion: inclusion_read(r)?,
            confirmations: r.u64()?,
            required: r.u64()?,
        },
        4 => FinalityStage::Final {
            inclusion: inclusion_read(r)?,
            confirmations: r.u64()?,
            required: r.u64()?,
        },
        5 => FinalityStage::Displaced {
            lost: inclusion_read(r)?,
            head: r.u64()?,
            requeued: r.bool()?,
        },
        _ => return Err(NativeWireError::Encoding),
    })
}
fn inclusion_write(w: &mut Writer, v: TransactionInclusion) -> Result<(), NativeWireError> {
    w.u64(v.block.number)?;
    w.bytes(&v.block.hash)?;
    w.u64(v.transaction_index)?;
    w.u8(match v.execution {
        ExecutionOutcome::Succeeded => 0,
        ExecutionOutcome::Reverted => 1,
    })?;
    match v.deployed_contract {
        None => w.u8(0),
        Some(a) => {
            w.u8(1)?;
            w.bytes(&a.bytes())
        }
    }
}
fn inclusion_read(r: &mut Reader<'_>) -> Result<TransactionInclusion, NativeWireError> {
    let block = BlockRef {
        number: r.u64()?,
        hash: r.array()?,
    };
    let transaction_index = r.u64()?;
    let execution = match r.u8()? {
        0 => ExecutionOutcome::Succeeded,
        1 => ExecutionOutcome::Reverted,
        _ => return Err(NativeWireError::Encoding),
    };
    let deployed_contract = match r.u8()? {
        0 => None,
        1 => Some(EvmAddress::new(r.array()?)),
        _ => return Err(NativeWireError::Encoding),
    };
    Ok(TransactionInclusion {
        block,
        transaction_index,
        execution,
        deployed_contract,
    })
}
fn view_write(w: &mut Writer, v: TransactionView) -> Result<(), NativeWireError> {
    match v {
        TransactionView::Unknown => w.u8(0),
        TransactionView::Pending => w.u8(1),
        TransactionView::Included(i) => {
            w.u8(2)?;
            inclusion_write(w, i)
        }
    }
}
fn view_read(r: &mut Reader<'_>) -> Result<TransactionView, NativeWireError> {
    Ok(match r.u8()? {
        0 => TransactionView::Unknown,
        1 => TransactionView::Pending,
        2 => TransactionView::Included(inclusion_read(r)?),
        _ => return Err(NativeWireError::Encoding),
    })
}
fn signal(w: &mut Writer, v: &ChainSignal) -> Result<(), NativeWireError> {
    match v {
        ChainSignal::Progressing => w.u8(0),
        ChainSignal::Delayed {
            stalled_polls,
            threshold,
            stalled_for,
            delayed_after,
        } => {
            w.u8(1)?;
            w.u64(*stalled_polls)?;
            w.u64(*threshold)?;
            duration(w, *stalled_for)?;
            duration(w, *delayed_after)
        }
        ChainSignal::Unreachable { error } => {
            w.u8(2)?;
            endpoint_error(w, error)
        }
    }
}
fn read_signal(r: &mut Reader<'_>) -> Result<ChainSignal, NativeWireError> {
    Ok(match r.u8()? {
        0 => ChainSignal::Progressing,
        1 => ChainSignal::Delayed {
            stalled_polls: r.u64()?,
            threshold: r.u64()?,
            stalled_for: read_duration(r)?,
            delayed_after: read_duration(r)?,
        },
        2 => ChainSignal::Unreachable {
            error: read_endpoint_error(r)?,
        },
        _ => return Err(NativeWireError::Encoding),
    })
}
fn endpoint_signal(w: &mut Writer, v: &EndpointSignal) -> Result<(), NativeWireError> {
    match v {
        EndpointSignal::Serving => w.u8(0),
        EndpointSignal::Degraded { failovers } => {
            w.u8(1)?;
            failures(w, failovers)
        }
        EndpointSignal::Unreachable { error } => {
            w.u8(2)?;
            endpoint_error(w, error)
        }
    }
}
fn read_endpoint_signal(r: &mut Reader<'_>) -> Result<EndpointSignal, NativeWireError> {
    Ok(match r.u8()? {
        0 => EndpointSignal::Serving,
        1 => EndpointSignal::Degraded {
            failovers: read_failures(r)?,
        },
        2 => EndpointSignal::Unreachable {
            error: read_endpoint_error(r)?,
        },
        _ => return Err(NativeWireError::Encoding),
    })
}
fn evidence(w: &mut Writer, e: &FinalityEvidence) -> Result<(), NativeWireError> {
    w.u64(e.binding.chain_id())?;
    w.count(e.binding.endpoint_sources().len(), MAX_ENDPOINTS)?;
    for (id, t) in e.binding.endpoint_sources() {
        w.string(id)?;
        match t {
            EndpointTransport::LocalEmulator => w.u8(0)?,
            EndpointTransport::PinnedTls { trust_anchor_der } => {
                w.u8(1)?;
                w.blob(trust_anchor_der, MAX_TRUST_ANCHOR)?;
            }
        }
    }
    w.u32(u32::try_from(e.binding.minimum_agreement()).map_err(|_| NativeWireError::Limit)?)?;
    w.u64(e.chain_id)?;
    w.u64(e.head)?;
    view_write(w, e.transaction)?;
    option_block(w, e.canonical_block)?;
    match &e.receipt_logs {
        None => w.u8(0),
        Some(logs) => {
            w.u8(1)?;
            w.count(logs.len(), MAX_LOGS)?;
            for l in logs {
                w.bytes(&l.address.bytes())?;
                w.count(l.topics.len(), MAX_TOPICS)?;
                for t in &l.topics {
                    w.bytes(t)?;
                }
                w.blob(&l.data, MAX_BLOB)?;
            }
            Ok(())
        }
    }
}
fn read_evidence(r: &mut Reader<'_>) -> Result<FinalityEvidence, NativeWireError> {
    let binding_chain = r.u64()?;
    let n = r.count(MAX_ENDPOINTS)?;
    let mut sources = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.string()?;
        let t = match r.u8()? {
            0 => EndpointTransport::LocalEmulator,
            1 => EndpointTransport::PinnedTls {
                trust_anchor_der: r.blob(MAX_TRUST_ANCHOR)?,
            },
            _ => return Err(NativeWireError::Encoding),
        };
        sources.push((id, t));
    }
    let minimum_agreement = usize::try_from(r.u32()?).map_err(|_| NativeWireError::Limit)?;
    let chain_id = r.u64()?;
    let head = r.u64()?;
    let transaction = view_read(r)?;
    let canonical_block = read_option_block(r)?;
    let receipt_logs = match r.u8()? {
        0 => None,
        1 => {
            let n = r.count(MAX_LOGS)?;
            let mut logs = Vec::with_capacity(n);
            for _ in 0..n {
                let address = EvmAddress::new(r.array()?);
                let m = r.count(MAX_TOPICS)?;
                let mut topics = Vec::with_capacity(m);
                for _ in 0..m {
                    topics.push(r.array()?);
                }
                let data = r.blob(MAX_BLOB)?;
                logs.push(LogRecord {
                    address,
                    topics,
                    data,
                });
            }
            Some(logs)
        }
        _ => return Err(NativeWireError::Encoding),
    };
    Ok(FinalityEvidence {
        binding: QuorumBinding::from_wire(binding_chain, sources, minimum_agreement),
        chain_id,
        head,
        transaction,
        canonical_block,
        receipt_logs,
    })
}
fn option_block(w: &mut Writer, v: Option<BlockRef>) -> Result<(), NativeWireError> {
    match v {
        None => w.u8(0),
        Some(b) => {
            w.u8(1)?;
            w.u64(b.number)?;
            w.bytes(&b.hash)
        }
    }
}
fn read_option_block(r: &mut Reader<'_>) -> Result<Option<BlockRef>, NativeWireError> {
    match r.u8()? {
        0 => Ok(None),
        1 => Ok(Some(BlockRef {
            number: r.u64()?,
            hash: r.array()?,
        })),
        _ => Err(NativeWireError::Encoding),
    }
}
fn duration(w: &mut Writer, d: Duration) -> Result<(), NativeWireError> {
    w.u64(d.as_secs())?;
    w.u32(d.subsec_nanos())
}
fn read_duration(r: &mut Reader<'_>) -> Result<Duration, NativeWireError> {
    let s = r.u64()?;
    let n = r.u32()?;
    if n >= 1_000_000_000 {
        return Err(NativeWireError::Encoding);
    }
    Ok(Duration::new(s, n))
}
fn endpoint_error(w: &mut Writer, e: &EndpointError) -> Result<(), NativeWireError> {
    failures(w, &e.failures)
}
fn read_endpoint_error(r: &mut Reader<'_>) -> Result<EndpointError, NativeWireError> {
    Ok(EndpointError {
        failures: read_failures(r)?,
    })
}
fn failures(w: &mut Writer, v: &[EndpointFailure]) -> Result<(), NativeWireError> {
    if v.is_empty() {
        return Err(NativeWireError::Encoding);
    }
    w.count(v.len(), MAX_FAILURES)?;
    for f in v {
        if f.url.is_empty() {
            return Err(NativeWireError::Encoding);
        }
        w.string(&f.url)?;
        fault(w, &f.fault)?;
    }
    Ok(())
}
fn read_failures(r: &mut Reader<'_>) -> Result<Vec<EndpointFailure>, NativeWireError> {
    let n = r.count(MAX_FAILURES)?;
    if n == 0 {
        return Err(NativeWireError::Encoding);
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let url = r.string()?;
        if url.is_empty() {
            return Err(NativeWireError::Encoding);
        }
        v.push(EndpointFailure {
            url,
            fault: read_fault(r)?,
        });
    }
    Ok(v)
}
fn fault(w: &mut Writer, v: &EndpointFault) -> Result<(), NativeWireError> {
    match v {
        EndpointFault::UnsupportedUrl => w.u8(0),
        EndpointFault::InsecureTransport => w.u8(1),
        EndpointFault::InvalidTrustAnchor => w.u8(2),
        EndpointFault::Authentication { detail } => tag_string(w, 3, detail),
        EndpointFault::Connect { detail } => tag_string(w, 4, detail),
        EndpointFault::Transport { detail } => tag_string(w, 5, detail),
        EndpointFault::Http { status } => {
            w.u8(6)?;
            w.u16(*status)
        }
        EndpointFault::ResponseTooLarge => w.u8(7),
        EndpointFault::AmbiguousFraming => w.u8(8),
        EndpointFault::MalformedResponse => w.u8(9),
        EndpointFault::Rpc { code, message } => {
            w.u8(10)?;
            w.i64(*code)?;
            w.string(message)
        }
        EndpointFault::ChainMismatch { expected, actual } => {
            w.u8(11)?;
            w.u64(*expected)?;
            w.u64(*actual)
        }
        EndpointFault::InconsistentObservation => w.u8(12),
        EndpointFault::UnexpectedValue { detail } => tag_string(w, 13, detail),
    }
}
fn read_fault(r: &mut Reader<'_>) -> Result<EndpointFault, NativeWireError> {
    Ok(match r.u8()? {
        0 => EndpointFault::UnsupportedUrl,
        1 => EndpointFault::InsecureTransport,
        2 => EndpointFault::InvalidTrustAnchor,
        3 => EndpointFault::Authentication {
            detail: r.string()?,
        },
        4 => EndpointFault::Connect {
            detail: r.string()?,
        },
        5 => EndpointFault::Transport {
            detail: r.string()?,
        },
        6 => EndpointFault::Http { status: r.u16()? },
        7 => EndpointFault::ResponseTooLarge,
        8 => EndpointFault::AmbiguousFraming,
        9 => EndpointFault::MalformedResponse,
        10 => EndpointFault::Rpc {
            code: r.i64()?,
            message: r.string()?,
        },
        11 => EndpointFault::ChainMismatch {
            expected: r.u64()?,
            actual: r.u64()?,
        },
        12 => EndpointFault::InconsistentObservation,
        13 => EndpointFault::UnexpectedValue {
            detail: r.string()?,
        },
        _ => return Err(NativeWireError::Encoding),
    })
}
fn tag_string(w: &mut Writer, t: u8, s: &str) -> Result<(), NativeWireError> {
    w.u8(t)?;
    w.string(s)
}

struct Writer {
    out: Vec<u8>,
    maximum: usize,
}
impl Writer {
    fn new(maximum: usize) -> Result<Self, NativeWireError> {
        if maximum == 0 {
            Err(NativeWireError::Limit)
        } else {
            Ok(Self {
                out: Vec::new(),
                maximum,
            })
        }
    }
    fn bytes(&mut self, b: &[u8]) -> Result<(), NativeWireError> {
        let Some(n) = self.out.len().checked_add(b.len()) else {
            return Err(NativeWireError::Limit);
        };
        if n > self.maximum {
            return Err(NativeWireError::Limit);
        }
        self.out.extend_from_slice(b);
        Ok(())
    }
    fn u8(&mut self, v: u8) -> Result<(), NativeWireError> {
        self.bytes(&[v])
    }
    fn u16(&mut self, v: u16) -> Result<(), NativeWireError> {
        self.bytes(&v.to_be_bytes())
    }
    fn u32(&mut self, v: u32) -> Result<(), NativeWireError> {
        self.bytes(&v.to_be_bytes())
    }
    fn u64(&mut self, v: u64) -> Result<(), NativeWireError> {
        self.bytes(&v.to_be_bytes())
    }
    fn i64(&mut self, v: i64) -> Result<(), NativeWireError> {
        self.bytes(&v.to_be_bytes())
    }
    fn bool(&mut self, v: bool) -> Result<(), NativeWireError> {
        self.u8(u8::from(v))
    }
    fn count(&mut self, n: usize, max: usize) -> Result<(), NativeWireError> {
        if n > max {
            return Err(NativeWireError::Limit);
        }
        self.u32(u32::try_from(n).map_err(|_| NativeWireError::Limit)?)
    }
    fn blob(&mut self, b: &[u8], max: usize) -> Result<(), NativeWireError> {
        if b.len() > max {
            return Err(NativeWireError::Limit);
        }
        self.count(b.len(), max)?;
        self.bytes(b)
    }
    fn string(&mut self, s: &str) -> Result<(), NativeWireError> {
        if s.len() > MAX_STRING {
            return Err(NativeWireError::Encoding);
        }
        self.blob(s.as_bytes(), MAX_STRING)
    }
}
struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, p: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], NativeWireError> {
        let e = self.p.checked_add(n).ok_or(NativeWireError::Encoding)?;
        let v = self.b.get(self.p..e).ok_or(NativeWireError::Encoding)?;
        self.p = e;
        Ok(v)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], NativeWireError> {
        self.take(N)?
            .try_into()
            .map_err(|_| NativeWireError::Encoding)
    }
    fn u8(&mut self) -> Result<u8, NativeWireError> {
        Ok(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, NativeWireError> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32, NativeWireError> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, NativeWireError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn i64(&mut self) -> Result<i64, NativeWireError> {
        Ok(i64::from_be_bytes(self.array()?))
    }
    fn bool(&mut self) -> Result<bool, NativeWireError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(NativeWireError::Encoding),
        }
    }
    fn count(&mut self, max: usize) -> Result<usize, NativeWireError> {
        let n = usize::try_from(self.u32()?).map_err(|_| NativeWireError::Limit)?;
        if n > max {
            Err(NativeWireError::Limit)
        } else {
            Ok(n)
        }
    }
    fn blob(&mut self, max: usize) -> Result<Vec<u8>, NativeWireError> {
        let n = self.count(max)?;
        Ok(self.take(n)?.to_vec())
    }
    fn string(&mut self) -> Result<String, NativeWireError> {
        let b = self.blob(MAX_STRING)?;
        if b.is_empty() {
            return Err(NativeWireError::Encoding);
        }
        String::from_utf8(b).map_err(|_| NativeWireError::Encoding)
    }
    fn finish(self) -> Result<(), NativeWireError> {
        if self.p == self.b.len() {
            Ok(())
        } else {
            Err(NativeWireError::Encoding)
        }
    }
}
