use sha2::{Digest, Sha256};

use super::{Abi, AbiError};
use crate::storage::Storage;
use crate::transfer::TransferSource;

const DOMAIN: &[u8] = b"LayerX/programs/v2/host-state\0";
const STORAGE_DOMAIN: &[u8] = b"LayerX/programs/v2/storage-root\0";
const MAX_CANONICAL_HOST_STATE_BYTES: usize = 64 * 1024 * 1024;

fn resource_code(resource: crate::meter::ResourceKind) -> u8 { match resource { crate::meter::ResourceKind::Cpu => 0, crate::meter::ResourceKind::Memory => 1, crate::meter::ResourceKind::StorageRead => 2, crate::meter::ResourceKind::StorageWrite => 3, crate::meter::ResourceKind::StorageOccupancy => 4, crate::meter::ResourceKind::Output => 5, crate::meter::ResourceKind::OutputBytes => 6 } }
fn storage_code(error: crate::storage::StorageError) -> u8 { match error { crate::storage::StorageError::InvalidProgram => 0, crate::storage::StorageError::InvalidPrincipal => 1, crate::storage::StorageError::EmptyKey => 2, crate::storage::StorageError::KeyTooLarge => 3, crate::storage::StorageError::ValueTooLarge => 4, crate::storage::StorageError::PrefixTooLarge => 5, crate::storage::StorageError::InvalidScanCursor => 6, crate::storage::StorageError::InvalidScanLimits => 7, crate::storage::StorageError::ScanCeilingExceeded => 8, crate::storage::StorageError::FrozenNamespace => 9, crate::storage::StorageError::SizeOverflow => 10 } }
pub(crate) fn abi_error_bytes(error: &AbiError) -> Vec<u8> {
    let mut out = Vec::with_capacity(19);
    match error {
        AbiError::WrongVersion => out.push(0), AbiError::InvalidCapability => out.push(1), AbiError::DuplicateCapability => out.push(2), AbiError::CapabilityDenied => out.push(3), AbiError::CapabilityEscalation => out.push(4), AbiError::EventBounds => out.push(5), AbiError::CallBounds => out.push(6), AbiError::AmountBounds => out.push(7), AbiError::ReceiptMismatch => out.push(8), AbiError::BalanceAbsent => out.push(9), AbiError::BalanceEvidenceUnavailable => out.push(10), AbiError::InvalidEncoding => out.push(11),
        AbiError::Storage(error) => { out.extend_from_slice(&[12, storage_code(*error)]); }
        AbiError::Meter(crate::meter::MeterRefusal::BudgetExceeded { resource, limit, attempted }) => { out.extend_from_slice(&[13, 0, resource_code(*resource)]); out.extend_from_slice(&limit.to_be_bytes()); out.extend_from_slice(&attempted.to_be_bytes()); }
        AbiError::Meter(crate::meter::MeterRefusal::CounterOverflow { resource }) => out.extend_from_slice(&[13, 1, resource_code(*resource)]),
        AbiError::Meter(crate::meter::MeterRefusal::FeeOverflow) => out.extend_from_slice(&[13, 2]),
        AbiError::AccessDeclaration => out.push(14),
    }
    out
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HostStateCommitment {
    pub(crate) root: [u8; 32],
    pub(crate) canonical_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HostStateIdentity {
    pub(crate) base_state_root: [u8; 32],
    pub(crate) receipt_oracle_root: [u8; 32],
    pub(crate) balance_oracle_root: [u8; 32],
}

fn storage_root(storage: &Storage) -> Result<[u8; 32], AbiError> {
    let mut hasher = Sha256::new();
    hasher.update(STORAGE_DOMAIN);
    let mut failed = false;
    storage.for_each_commitment_entry(|key, value| {
        let Ok(key_len) = u32::try_from(key.len()) else { failed = true; return };
        let Ok(value_len) = u32::try_from(value.len()) else { failed = true; return };
        hasher.update(key_len.to_be_bytes());
        hasher.update(&key);
        hasher.update(value_len.to_be_bytes());
        hasher.update(value);
    });
    if failed { return Err(AbiError::InvalidEncoding) }
    Ok(hasher.finalize().into())
}

fn field(write: &mut impl FnMut(&[u8]) -> Result<(), AbiError>, bytes: &[u8]) -> Result<(), AbiError> {
    let len = u32::try_from(bytes.len()).map_err(|_| AbiError::InvalidEncoding)?;
    write(&len.to_be_bytes())?;
    write(bytes)?;
    Ok(())
}

pub(super) fn identity(abi: &Abi, baseline: &Storage) -> Result<HostStateIdentity, AbiError> {
    let mut receipts = Sha256::new();
    receipts.update(b"LayerX/programs/v2/receipt-oracle\0");
    for (digest, view) in &abi.receipts {
        receipts.update(digest); receipts.update(view.receipt_digest);
        receipts.update(view.result_code.to_be_bytes()); receipts.update(view.asset);
        receipts.update(view.amount.to_be_bytes()); receipts.update(view.state_root);
    }
    let mut balances = Sha256::new();
    balances.update(b"LayerX/programs/v2/balance-oracle\0");
    for ((account, asset), view) in &abi.balances {
        balances.update(account); balances.update(asset);
        match view {
            Ok(view) => { balances.update([1]); balances.update(view.account); balances.update(view.asset); balances.update(view.balance.to_be_bytes()); balances.update(view.receipt_digest); balances.update(view.state_root); balances.update(view.observed_sequence.to_be_bytes()); }
            Err(error) => { balances.update([0]); let error = abi_error_bytes(error); balances.update((error.len() as u64).to_be_bytes()); balances.update(error); }
        }
    }
    Ok(HostStateIdentity {
        base_state_root: storage_root(baseline)?,
        receipt_oracle_root: receipts.finalize().into(),
        balance_oracle_root: balances.finalize().into(),
    })
}

pub(super) fn commit(abi: &Abi, hash: bool) -> Result<HostStateCommitment, AbiError> {
    fn write_state(
        abi: &Abi,
        write: &mut impl FnMut(&[u8]) -> Result<(), AbiError>,
    ) -> Result<(), AbiError> {
        write(DOMAIN)?;
        write(&abi.version.to_be_bytes())?;
        write(&abi.program.bytes())?;
        write(&abi.authorization.principal().bytes())?;
        let (frame, depth) = abi.authorization.frame().canonical_bytes();
        write(&frame)?; write(&[depth])?;
        field(write, &abi.authorization.capabilities().canonical_encoding())?;
        field(write, &abi.principal_namespace.canonical_bytes())?;
        field(write, &abi.shared_namespace.canonical_bytes())?;
        write(b"storage-overlay/v1\0")?;
        field(write, &abi.access_declaration.canonical_bytes().map_err(|_| AbiError::InvalidEncoding)?)?;
        write(&(abi.event_count_base as u64).to_be_bytes())?;
        let receipts = u32::try_from(abi.receipts.len()).map_err(|_| AbiError::InvalidEncoding)?;
        write(&receipts.to_be_bytes())?;
        for (digest, view) in &abi.receipts {
            write(digest)?; write(&view.receipt_digest)?; write(&view.result_code.to_be_bytes())?;
            write(&view.asset)?; write(&view.amount.to_be_bytes())?; write(&view.state_root)?;
        }
        let balances = u32::try_from(abi.balances.len()).map_err(|_| AbiError::InvalidEncoding)?;
        write(&balances.to_be_bytes())?;
        for ((account, asset), view) in &abi.balances {
            write(account)?; write(asset)?;
            match view {
                Ok(view) => {
                    write(&[1])?; write(&view.account)?; write(&view.asset)?;
                    write(&view.balance.to_be_bytes())?; write(&view.receipt_digest)?;
                    write(&view.state_root)?; write(&view.observed_sequence.to_be_bytes())?;
                }
                Err(error) => { write(&[0])?; field(write, &abi_error_bytes(error))?; }
            }
        }
        write(&u32::try_from(abi.effects.events.len()).map_err(|_| AbiError::EventBounds)?.to_be_bytes())?;
        for event in &abi.effects.events {
            write(&event.program.bytes())?; write(&event.principal.bytes())?;
            let (path, depth) = event.frame.canonical_bytes(); write(&path)?; write(&[depth])?;
            field(write, &event.topic)?; field(write, &event.data)?;
        }
        write(&u32::try_from(abi.effects.calls.len()).map_err(|_| AbiError::CallBounds)?.to_be_bytes())?;
        for call in &abi.effects.calls {
            write(&call.caller.bytes())?; write(&call.callee.bytes())?; write(&call.principal.bytes())?;
            let (path, depth) = call.caller_frame.canonical_bytes(); write(&path)?; write(&[depth])?;
            let (path, depth) = call.callee_frame.canonical_bytes(); write(&path)?; write(&[depth])?;
            field(write, &call.input)?; field(write, &call.capabilities.canonical_encoding())?;
        }
        write(&u32::try_from(abi.effects.transfers.len()).map_err(|_| AbiError::AmountBounds)?.to_be_bytes())?;
        for transfer in &abi.effects.transfers {
            write(&transfer.program.bytes())?; write(&transfer.principal.bytes())?;
            let (path, depth) = transfer.frame.canonical_bytes(); write(&path)?; write(&[depth])?;
            match &transfer.source {
                TransferSource::Principal(principal) => { write(&[0])?; write(&principal.bytes())?; }
                TransferSource::ProgramFunding { principal, binding } => {
                    write(&[1])?; write(&principal.bytes())?; write(&binding.owner_program().bytes())?;
                    field(write, binding.seed())?; write(&binding.destination_account())?; write(&binding.asset())?;
                }
                TransferSource::Program(authority) => {
                    write(&[2])?; write(&authority.owner_program().bytes())?; field(write, authority.seed())?;
                    write(&authority.source_account())?; let (path, depth) = authority.staging_frame().canonical_bytes();
                    write(&path)?; write(&[depth])?; write(&authority.asset())?; write(&authority.to())?;
                    write(&authority.amount().to_be_bytes())?;
                }
            }
            write(&transfer.asset)?; write(&transfer.to)?; write(&transfer.amount.to_be_bytes())?;
        }
        write(&u32::try_from(abi.effects.namespace_drops.len()).map_err(|_| AbiError::InvalidEncoding)?.to_be_bytes())?;
        for drop in &abi.effects.namespace_drops {
            field(write, &drop.namespace().canonical_bytes())?;
            write(&drop.reclaimed_cells().to_be_bytes())?;
            write(&drop.reclaimed_key_value_bytes().to_be_bytes())?;
            write(&drop.metered_work().to_be_bytes())?;
        }
        Ok(())
    }
    let mut canonical_bytes = 0_u64;
    write_state(abi, &mut |bytes| {
        canonical_bytes = canonical_bytes.checked_add(u64::try_from(bytes.len()).map_err(|_| AbiError::InvalidEncoding)?)
            .ok_or(AbiError::InvalidEncoding)?;
        if canonical_bytes > MAX_CANONICAL_HOST_STATE_BYTES as u64 { return Err(AbiError::InvalidEncoding) }
        Ok(())
    })?;
    if !hash {
        return Ok(HostStateCommitment { root: [0; 32], canonical_bytes });
    }
    let mut hasher = Sha256::new();
    let mut written = 0_u64;
    write_state(abi, &mut |bytes| {
        written = written.checked_add(u64::try_from(bytes.len()).map_err(|_| AbiError::InvalidEncoding)?)
            .ok_or(AbiError::InvalidEncoding)?;
        if written > canonical_bytes { return Err(AbiError::InvalidEncoding) }
        hasher.update(bytes);
        Ok(())
    })?;
    if written != canonical_bytes { return Err(AbiError::InvalidEncoding) }
    Ok(HostStateCommitment { root: hasher.finalize().into(), canonical_bytes })
}
