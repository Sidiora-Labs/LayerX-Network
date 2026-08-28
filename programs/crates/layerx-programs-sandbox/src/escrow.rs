//! Kernel-settled 402LXP custody for sandbox leases.

use core::fmt::{self, Display};

use layerx_programs_runtime::{
    KernelTransferPrimitive, PreparedAuthorizedActivity, PreparedMonetarySummary,
    ProgramAuthority, Storage, TransferLawError, TransferSource, VerifiedProgramSettlement,
    VerifiedStorageAssignment,
};

use crate::{Lease, LeaseId, LeaseState};

const ESCROW_SEED_DOMAIN: &[u8] = b"sandbox-lease-escrow/v1\0";
const ESCROW_STATE_DOMAIN: &[u8] = b"LayerX/programs/sandbox/escrow-state/v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Escrow {
    lease: LeaseId,
    account: [u8; 32],
    asset: [u8; 32],
    funded: u128,
    spent: u128,
    refunded: u128,
    funding_root: [u8; 32],
    settlement_root: Option<[u8; 32]>,
    finalized: bool,
}

impl Escrow {
    #[must_use] pub const fn lease(self) -> LeaseId { self.lease }
    #[must_use] pub const fn account(self) -> [u8; 32] { self.account }
    #[must_use] pub const fn asset(self) -> [u8; 32] { self.asset }
    #[must_use] pub const fn funded(self) -> u128 { self.funded }
    #[must_use] pub const fn spent(self) -> u128 { self.spent }
    #[must_use] pub const fn refunded(self) -> u128 { self.refunded }
    #[must_use] pub const fn funding_root(self) -> [u8; 32] { self.funding_root }
    #[must_use] pub const fn settlement_root(self) -> Option<[u8; 32]> { self.settlement_root }

    #[must_use]
    pub fn canonical_state(self) -> Vec<u8> {
        let mut state = Vec::with_capacity(ESCROW_STATE_DOMAIN.len() + 210);
        state.extend_from_slice(ESCROW_STATE_DOMAIN);
        state.extend_from_slice(&self.lease.bytes());
        state.extend_from_slice(&self.account);
        state.extend_from_slice(&self.asset);
        state.extend_from_slice(&self.funded.to_be_bytes());
        state.extend_from_slice(&self.spent.to_be_bytes());
        state.extend_from_slice(&self.refunded.to_be_bytes());
        state.extend_from_slice(&self.funding_root);
        match self.settlement_root {
            None => state.push(0),
            Some(root) => { state.push(1); state.extend_from_slice(&root); }
        }
        state.push(u8::from(self.finalized));
        state
    }

    pub fn decode_state(lease: &Lease, state: &[u8]) -> Result<Self, EscrowRefusal> {
        let fixed = ESCROW_STATE_DOMAIN.len() + 32 + 32 + 32 + 16 + 16 + 16 + 32 + 1 + 1;
        if state.len() != fixed && state.len() != fixed + 32 {
            return Err(EscrowRefusal::InvalidStateEncoding);
        }
        let mut offset = ESCROW_STATE_DOMAIN.len();
        if state.get(..offset) != Some(ESCROW_STATE_DOMAIN) {
            return Err(EscrowRefusal::InvalidStateEncoding);
        }
        let mut take = |length: usize| -> Result<&[u8], EscrowRefusal> {
            let end = offset.checked_add(length).ok_or(EscrowRefusal::InvalidStateEncoding)?;
            let value = state.get(offset..end).ok_or(EscrowRefusal::InvalidStateEncoding)?;
            offset = end;
            Ok(value)
        };
        let escrow_lease = LeaseId::new(take(32)?.try_into().map_err(|_| EscrowRefusal::InvalidStateEncoding)?)
            .map_err(|_| EscrowRefusal::InvalidStateEncoding)?;
        let account = take(32)?.try_into().map_err(|_| EscrowRefusal::InvalidStateEncoding)?;
        let asset = take(32)?.try_into().map_err(|_| EscrowRefusal::InvalidStateEncoding)?;
        let funded = u128::from_be_bytes(take(16)?.try_into().map_err(|_| EscrowRefusal::InvalidStateEncoding)?);
        let spent = u128::from_be_bytes(take(16)?.try_into().map_err(|_| EscrowRefusal::InvalidStateEncoding)?);
        let refunded = u128::from_be_bytes(take(16)?.try_into().map_err(|_| EscrowRefusal::InvalidStateEncoding)?);
        let funding_root = take(32)?.try_into().map_err(|_| EscrowRefusal::InvalidStateEncoding)?;
        let settlement_root = match take(1)?[0] {
            0 => None,
            1 => Some(take(32)?.try_into().map_err(|_| EscrowRefusal::InvalidStateEncoding)?),
            _ => return Err(EscrowRefusal::InvalidStateEncoding),
        };
        let finalized = match take(1)?[0] { 0 => false, 1 => true,
            _ => return Err(EscrowRefusal::InvalidStateEncoding) };
        if offset != state.len() || funding_root == [0; 32]
            || refunded > 0 && settlement_root.is_none()
            || settlement_root.is_some() && !finalized
            || !finalized && refunded != 0
        {
            return Err(EscrowRefusal::InvalidStateEncoding);
        }
        let escrow = Self { lease: escrow_lease, account, asset, funded, spent, refunded,
            funding_root, settlement_root, finalized };
        escrow.binds(lease)?;
        if escrow.remaining().is_err() || finalized && funded != spent.checked_add(refunded)
            .ok_or(EscrowRefusal::InvalidStateEncoding)? {
            return Err(EscrowRefusal::InvalidStateEncoding);
        }
        if escrow.canonical_state() != state { return Err(EscrowRefusal::InvalidStateEncoding); }
        Ok(escrow)
    }

    #[must_use]
    pub fn remaining(self) -> Result<u128, EscrowRefusal> {
        self.funded.checked_sub(self.spent)
            .and_then(|remaining| remaining.checked_sub(self.refunded))
            .ok_or(EscrowRefusal::ConservationViolation)
    }

    #[must_use]
    pub fn permits_execution(self, lease: &Lease, maximum_charge: u128) -> Result<(), EscrowRefusal> {
        self.binds(lease)?;
        if !matches!(lease.state(), LeaseState::Funded | LeaseState::Active) {
            return Err(EscrowRefusal::LeaseNotExecutable);
        }
        if self.finalized { return Err(EscrowRefusal::AlreadySettled); }
        ensure_charge(maximum_charge, self.remaining()?)
    }

    pub fn spend(
        &mut self,
        lease: &Lease,
        maximum_charge: u128,
        prepared: PreparedAuthorizedActivity,
        storage: &mut Storage,
        kernel: &mut impl KernelTransferPrimitive,
    ) -> Result<EscrowOutcome, EscrowRefusal> {
        self.permits_execution(lease, maximum_charge)?;
        let summary = prepared.monetary_summary().ok_or(EscrowRefusal::MissingTransferSet)?;
        let charged = validate_program_debits(self, lease, &summary, None)?;
        if charged == 0 || charged > maximum_charge {
            return Err(EscrowRefusal::ChargeMismatch { maximum: maximum_charge, actual: charged });
        }
        let assignment = prepared.strict_settle(storage, kernel)
            .map_err(|failure| EscrowRefusal::Transfer(failure.error()))?;
        let settlement = assignment.settlement().copied().ok_or(EscrowRefusal::MissingTransferSet)?;
        self.spent = self.spent.checked_add(charged).ok_or(EscrowRefusal::ConservationViolation)?;
        if self.spent > self.funded { return Err(EscrowRefusal::ConservationViolation); }
        Ok(EscrowOutcome { assignment, settlement: Some(settlement), amount: charged })
    }

    fn binds(self, lease: &Lease) -> Result<(), EscrowRefusal> {
        if self.lease != lease.id() || self.account != lease.escrow_account()
            || self.asset != lease.escrow_asset() || self.funded != lease.escrow_amount()
        {
            return Err(EscrowRefusal::LeaseMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct EscrowOutcome {
    assignment: VerifiedStorageAssignment,
    settlement: Option<VerifiedProgramSettlement>,
    amount: u128,
}

impl EscrowOutcome {
    #[must_use] pub const fn assignment(&self) -> &VerifiedStorageAssignment { &self.assignment }
    #[must_use] pub const fn settlement(&self) -> Option<&VerifiedProgramSettlement> { self.settlement.as_ref() }
    #[must_use] pub const fn amount(&self) -> u128 { self.amount }
}

pub fn fund(
    lease: &Lease,
    prepared: PreparedAuthorizedActivity,
    storage: &mut Storage,
    kernel: &mut impl KernelTransferPrimitive,
) -> Result<(Escrow, EscrowOutcome), EscrowRefusal> {
    if lease.state() != LeaseState::Requested { return Err(EscrowRefusal::LeaseNotRequested); }
    let summary = prepared.monetary_summary().ok_or(EscrowRefusal::MissingTransferSet)?;
    if summary.program() != lease.host_program() || summary.principal() != lease.tenant()
        || summary.total_amount() != lease.escrow_amount() || summary.legs().len() != 1
    {
        return Err(EscrowRefusal::FundingMismatch);
    }
    let leg = &summary.legs()[0];
    let TransferSource::ProgramFunding { principal, binding } = leg.source() else {
        return Err(EscrowRefusal::FundingMismatch);
    };
    let seed = escrow_seed(lease.id());
    if *principal != lease.tenant() || binding.owner_program() != lease.host_program()
        || binding.seed() != seed || binding.destination_account() != lease.escrow_account()
        || binding.asset() != lease.escrow_asset() || leg.program() != lease.host_program()
        || leg.principal() != lease.tenant() || leg.asset() != lease.escrow_asset()
        || leg.to() != lease.escrow_account() || leg.amount() != lease.escrow_amount()
    {
        return Err(EscrowRefusal::FundingMismatch);
    }
    let assignment = prepared.strict_settle(storage, kernel)
        .map_err(|failure| EscrowRefusal::Transfer(failure.error()))?;
    let settlement = assignment.settlement().copied().ok_or(EscrowRefusal::MissingTransferSet)?;
    if settlement.leg_count() != 1 || settlement.total_amount() != lease.escrow_amount() {
        return Err(EscrowRefusal::FundingMismatch);
    }
    let escrow = Escrow { lease: lease.id(), account: lease.escrow_account(),
        asset: lease.escrow_asset(), funded: lease.escrow_amount(), spent: 0, refunded: 0,
        funding_root: settlement.transfer_set_root(), settlement_root: None, finalized: false };
    Ok((escrow, EscrowOutcome { assignment, settlement: Some(settlement), amount: lease.escrow_amount() }))
}

pub fn settle(
    escrow: &mut Escrow,
    lease: &Lease,
    prepared: PreparedAuthorizedActivity,
    storage: &mut Storage,
    kernel: &mut impl KernelTransferPrimitive,
) -> Result<EscrowOutcome, EscrowRefusal> {
    escrow.binds(lease)?;
    if !matches!(lease.state(), LeaseState::Settling | LeaseState::Expired) {
        return Err(EscrowRefusal::LeaseNotSettling);
    }
    if escrow.finalized {
        return Err(EscrowRefusal::AlreadySettled);
    }
    let refund = escrow.remaining()?;
    if refund == 0 {
        if prepared.has_monetary_effects() { return Err(EscrowRefusal::RefundMismatch { expected: 0, actual: prepared.monetary_summary().map_or(0, |summary| summary.total_amount()) }); }
        let assignment = prepared.strict_settle(storage, kernel)
            .map_err(|failure| EscrowRefusal::Transfer(failure.error()))?;
        escrow.finalized = true;
        return Ok(EscrowOutcome { assignment, settlement: None, amount: 0 });
    }
    let summary = prepared.monetary_summary().ok_or(EscrowRefusal::MissingTransferSet)?;
    let debited = validate_program_debits(escrow, lease, &summary, Some((lease.tenant().bytes(), refund)))?;
    if debited != refund || summary.legs().len() != 1 {
        return Err(EscrowRefusal::RefundMismatch { expected: refund, actual: debited });
    }
    let assignment = prepared.strict_settle(storage, kernel)
        .map_err(|failure| EscrowRefusal::Transfer(failure.error()))?;
    let settlement = assignment.settlement().copied().ok_or(EscrowRefusal::MissingTransferSet)?;
    escrow.refunded = refund;
    escrow.settlement_root = Some(settlement.transfer_set_root());
    escrow.finalized = true;
    if escrow.funded != escrow.spent.checked_add(escrow.refunded)
        .ok_or(EscrowRefusal::ConservationViolation)? {
        return Err(EscrowRefusal::ConservationViolation);
    }
    Ok(EscrowOutcome { assignment, settlement: Some(settlement), amount: refund })
}

fn ensure_charge(requested: u128, remaining: u128) -> Result<(), EscrowRefusal> {
    if requested == 0 || requested > remaining {
        Err(EscrowRefusal::EscrowExhausted { requested, remaining })
    } else {
        Ok(())
    }
}

fn validate_program_debits(
    escrow: &Escrow,
    lease: &Lease,
    summary: &PreparedMonetarySummary,
    exact_destination: Option<([u8; 32], u128)>,
) -> Result<u128, EscrowRefusal> {
    if summary.program() != lease.host_program() || summary.principal() != lease.tenant() {
        return Err(EscrowRefusal::TransferSetMismatch);
    }
    let seed = escrow_seed(lease.id());
    let mut total = 0u128;
    for leg in summary.legs() {
        let TransferSource::Program(authority) = leg.source() else {
            return Err(EscrowRefusal::TransferSetMismatch);
        };
        validate_authority(authority, lease, &seed, leg.asset(), leg.to(), leg.amount())?;
        if leg.program() != lease.host_program() || leg.principal() != lease.tenant()
            || leg.asset() != lease.escrow_asset() {
            return Err(EscrowRefusal::TransferSetMismatch);
        }
        if let Some((destination, amount)) = exact_destination {
            if leg.to() != destination || leg.amount() != amount {
                return Err(EscrowRefusal::RefundMismatch { expected: amount, actual: leg.amount() });
            }
        }
        total = total.checked_add(leg.amount()).ok_or(EscrowRefusal::ConservationViolation)?;
    }
    if total != summary.total_amount() { return Err(EscrowRefusal::TransferSetMismatch); }
    Ok(total)
}

fn validate_authority(
    authority: &ProgramAuthority, lease: &Lease, seed: &[u8], asset: [u8; 32],
    destination: [u8; 32], amount: u128,
) -> Result<(), EscrowRefusal> {
    if authority.owner_program() != lease.host_program() || authority.seed() != seed
        || authority.source_account() != lease.escrow_account() || authority.asset() != asset
        || authority.to() != destination || authority.amount() != amount {
        return Err(EscrowRefusal::TransferSetMismatch);
    }
    ProgramAuthority::validate_owner_frame(lease.host_program(), seed, lease.escrow_account(),
        asset, destination, amount).map_err(EscrowRefusal::Transfer)
}

fn escrow_seed(lease: LeaseId) -> Vec<u8> {
    let mut seed = Vec::with_capacity(ESCROW_SEED_DOMAIN.len() + 32);
    seed.extend_from_slice(ESCROW_SEED_DOMAIN);
    seed.extend_from_slice(&lease.bytes());
    seed
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EscrowRefusal {
    LeaseMismatch,
    LeaseNotRequested,
    LeaseNotExecutable,
    LeaseNotSettling,
    FundingMismatch,
    TransferSetMismatch,
    MissingTransferSet,
    EscrowExhausted { requested: u128, remaining: u128 },
    ChargeMismatch { maximum: u128, actual: u128 },
    RefundMismatch { expected: u128, actual: u128 },
    AlreadySettled,
    ConservationViolation,
    InvalidStateEncoding,
    Transfer(TransferLawError),
}

impl Display for EscrowRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { write!(formatter, "{self:?}") }
}

impl std::error::Error for EscrowRefusal {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LeaseId, LeaseLimits};
    use layerx_programs_runtime::{PrincipalId, ProgramId};

    fn lease(amount: u128) -> Lease {
        Lease::request(LeaseId::new([1; 32]).expect("lease"),
            PrincipalId::new([2; 32]).expect("principal"),
            ProgramId::new([3; 32]).expect("program"), [4; 32], [5; 32], amount,
            LeaseLimits { cpu_fuel: 1, memory_bytes: 1, storage_read_bytes: 1,
                storage_write_bytes: 1, output_values: 1, output_bytes: 1,
                table_elements: 1, namespace_bytes: 1 }, 1, 2).expect("lease")
    }

    fn escrow(lease: &Lease, spent: u128, refunded: u128) -> Escrow {
        Escrow { lease: lease.id(), account: lease.escrow_account(), asset: lease.escrow_asset(),
            funded: lease.escrow_amount(), spent, refunded, funding_root: [6; 32],
            settlement_root: (refunded != 0).then_some([7; 32]), finalized: refunded != 0 }
    }

    #[test]
    fn exhaustion_refuses_before_execution() {
        let lease = lease(100);
        let escrow = escrow(&lease, 100, 0);
        assert_eq!(escrow.remaining(), Ok(0));
        assert_eq!(ensure_charge(1, escrow.remaining().expect("remainder")),
            Err(EscrowRefusal::EscrowExhausted { requested: 1, remaining: 0 }));
    }

    #[test]
    fn zero_usage_conserves_the_whole_refund() {
        let lease = lease(100);
        let escrow = escrow(&lease, 0, 0);
        assert_eq!(escrow.remaining(), Ok(100));
        assert_eq!(escrow.funded(), escrow.spent() + escrow.remaining().expect("remainder"));
    }

    #[test]
    fn repeated_refund_is_refused_by_terminal_commitment() {
        let lease = lease(100);
        let escrow = escrow(&lease, 25, 75);
        assert_eq!(escrow.remaining(), Ok(0));
        assert_eq!(escrow.settlement_root(), Some([7; 32]));
        assert_eq!(escrow.funded(), escrow.spent() + escrow.refunded());
        assert!(escrow.finalized);
    }

    #[test]
    fn canonical_state_preserves_conservation_and_replay_marker() {
        let lease = lease(100);
        let escrow = escrow(&lease, 25, 75);
        let encoded = escrow.canonical_state();
        assert_eq!(Escrow::decode_state(&lease, &encoded), Ok(escrow));
        let mut altered = encoded;
        let last = altered.len() - 1;
        altered[last] = 0;
        assert_eq!(Escrow::decode_state(&lease, &altered), Err(EscrowRefusal::InvalidStateEncoding));
    }
}
