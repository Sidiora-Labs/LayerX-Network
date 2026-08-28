use layerx_program_sdk::{AccountId, Amount, Field, ProgramError, Reason};

use crate::{ComputeLease, LeaseStatus, Offer};

#[cfg(target_arch = "wasm32")]
use layerx_program_sdk::{
    transfer, AssetId, ProgramAccountPayment, ProgramAccountSeed, ProgramDeposit,
};

pub const MAX_CHALLENGE_WINDOW_BATCHES: u64 = 100_000;
pub const MAX_USAGE_UNITS_PER_CLAIM: u64 = 1_000_000_000_000;
pub const CLAIM_CAPACITY: usize = 290;
pub const CHALLENGE_CAPACITY: usize = 427 + layerx_program_sdk::MAX_PROGRAM_ACCOUNT_SEED_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeteredUsageClaim {
    pub compute_units: u64,
    pub memory_byte_batches: u64,
    pub storage_read_bytes: u64,
    pub storage_written_bytes: u64,
    pub ingress_bytes: u64,
    pub egress_bytes: u64,
}

impl MeteredUsageClaim {
    fn validate(self) -> Result<(), ProgramError> {
        let values = [self.compute_units, self.memory_byte_batches,
            self.storage_read_bytes, self.storage_written_bytes,
            self.ingress_bytes, self.egress_bytes];
        if self.compute_units == 0 || values.into_iter().any(|value|
            value > MAX_USAGE_UNITS_PER_CLAIM) {
            return Err(malformed());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ClaimStatus {
    Challengeable = 1,
    Frozen = 2,
    Finalized = 3,
    ChallengerWon = 4,
    ProviderWon = 5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsageClaim {
    pub id: [u8; 32],
    pub lease_id: [u8; 32],
    pub provider: AccountId,
    pub input_commitment: [u8; 32],
    pub output_digest: [u8; 32],
    pub execution_state_root: [u8; 32],
    pub usage: MeteredUsageClaim,
    pub payable: Amount,
    pub challenger_stake: Amount,
    pub committed_at: u64,
    pub challenge_deadline: u64,
    pub status: ClaimStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChallengeWindow {
    pub opened_at: u64,
    pub last_challenge_height: u64,
}

impl ChallengeWindow {
    pub fn contains(self, height: u64) -> bool {
        height >= self.opened_at && height <= self.last_challenge_height
    }

    pub fn elapsed(self, height: u64) -> bool {
        height > self.last_challenge_height
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderCommitment {
    pub id: [u8; 32],
    pub lease_id: [u8; 32],
    pub input_commitment: [u8; 32],
    pub output_digest: [u8; 32],
    pub execution_state_root: [u8; 32],
    pub usage: MeteredUsageClaim,
    pub payable: Amount,
    pub challenger_stake: Amount,
    pub challenge_window_batches: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContradictingCommitment {
    pub input_commitment: [u8; 32],
    pub output_digest: [u8; 32],
    pub execution_state_root: [u8; 32],
    pub usage: MeteredUsageClaim,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsageChallenge<'a> {
    pub id: [u8; 32],
    pub claim_id: [u8; 32],
    pub lease_id: [u8; 32],
    pub offer_id: [u8; 32],
    pub provider: AccountId,
    pub tenant: AccountId,
    pub challenger: AccountId,
    pub stake_account: AccountId,
    pub stake_seed: &'a [u8],
    pub stake: Amount,
    pub contradictory: ContradictingCommitment,
    pub opened_at: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ArbiterVerdict {
    Provider = 1,
    Challenger = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArbiterResolution {
    pub(crate) claim_id: [u8; 32],
    pub(crate) challenge_id: [u8; 32],
    pub(crate) dispute_commitment: [u8; 32],
    pub(crate) verdict: ArbiterVerdict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettlementPlan {
    pub provider: Amount,
    pub tenant: Amount,
    pub challenger: Amount,
    pub stake_for_provider: Amount,
}

impl SettlementPlan {
    pub fn total(self) -> Result<Amount, ProgramError> {
        self.provider.checked_add(self.tenant)?
            .checked_add(self.challenger)?
            .checked_add(self.stake_for_provider)
    }
}

pub(crate) fn encode_claim(claim: UsageClaim, output: &mut [u8]) -> Result<usize, ProgramError> {
    let mut offset = 0;
    crate::append(output, &mut offset, &[1, claim.status as u8])?;
    for bytes in [claim.id, claim.lease_id, claim.provider.bytes(),
        claim.input_commitment, claim.output_digest, claim.execution_state_root] {
        crate::append(output, &mut offset, &bytes)?;
    }
    for value in [claim.usage.compute_units, claim.usage.memory_byte_batches,
        claim.usage.storage_read_bytes, claim.usage.storage_written_bytes,
        claim.usage.ingress_bytes, claim.usage.egress_bytes] {
        crate::append(output, &mut offset, &value.to_be_bytes())?;
    }
    crate::append(output, &mut offset, &claim.payable.to_be_bytes())?;
    crate::append(output, &mut offset, &claim.challenger_stake.to_be_bytes())?;
    crate::append(output, &mut offset, &claim.committed_at.to_be_bytes())?;
    crate::append(output, &mut offset, &claim.challenge_deadline.to_be_bytes())?;
    Ok(offset)
}

pub(crate) fn decode_claim(input: &[u8]) -> Result<UsageClaim, ProgramError> {
    let mut cursor = crate::Cursor::new(input);
    if cursor.byte()? != 1 { return Err(malformed()); }
    let status = match cursor.byte()? {
        1 => ClaimStatus::Challengeable, 2 => ClaimStatus::Frozen,
        3 => ClaimStatus::Finalized, 4 => ClaimStatus::ChallengerWon,
        5 => ClaimStatus::ProviderWon, _ => return Err(malformed()),
    };
    let claim = UsageClaim { id: cursor.array()?, lease_id: cursor.array()?,
        provider: cursor.account()?, input_commitment: cursor.array()?,
        output_digest: cursor.array()?, execution_state_root: cursor.array()?,
        usage: MeteredUsageClaim { compute_units: cursor.u64()?,
            memory_byte_batches: cursor.u64()?, storage_read_bytes: cursor.u64()?,
            storage_written_bytes: cursor.u64()?, ingress_bytes: cursor.u64()?,
            egress_bytes: cursor.u64()? }, payable: cursor.amount()?,
        challenger_stake: cursor.amount()?, committed_at: cursor.u64()?,
        challenge_deadline: cursor.u64()?, status };
    claim.usage.validate()?;
    cursor.finish()?;
    Ok(claim)
}

pub(crate) fn encode_challenge(
    challenge: UsageChallenge<'_>, output: &mut [u8],
) -> Result<usize, ProgramError> {
    let mut offset = 0;
    crate::append(output, &mut offset, &[1])?;
    for bytes in [challenge.id, challenge.claim_id, challenge.lease_id,
        challenge.offer_id, challenge.provider.bytes(), challenge.tenant.bytes(),
        challenge.challenger.bytes(), challenge.stake_account.bytes()] {
        crate::append(output, &mut offset, &bytes)?;
    }
    crate::append_seed(output, &mut offset, challenge.stake_seed)?;
    crate::append(output, &mut offset, &challenge.stake.to_be_bytes())?;
    for bytes in [challenge.contradictory.input_commitment,
        challenge.contradictory.output_digest,
        challenge.contradictory.execution_state_root] {
        crate::append(output, &mut offset, &bytes)?;
    }
    for value in [challenge.contradictory.usage.compute_units,
        challenge.contradictory.usage.memory_byte_batches,
        challenge.contradictory.usage.storage_read_bytes,
        challenge.contradictory.usage.storage_written_bytes,
        challenge.contradictory.usage.ingress_bytes,
        challenge.contradictory.usage.egress_bytes] {
        crate::append(output, &mut offset, &value.to_be_bytes())?;
    }
    crate::append(output, &mut offset, &challenge.opened_at.to_be_bytes())?;
    Ok(offset)
}

pub(crate) fn decode_challenge(input: &[u8]) -> Result<UsageChallenge<'_>, ProgramError> {
    let mut cursor = crate::Cursor::new(input);
    if cursor.byte()? != 1 { return Err(malformed()); }
    let challenge = UsageChallenge { id: cursor.array()?, claim_id: cursor.array()?,
        lease_id: cursor.array()?, offer_id: cursor.array()?, provider: cursor.account()?,
        tenant: cursor.account()?, challenger: cursor.account()?,
        stake_account: cursor.account()?, stake_seed: cursor.seed()?,
        stake: cursor.amount()?, contradictory: ContradictingCommitment {
            input_commitment: cursor.array()?, output_digest: cursor.array()?,
            execution_state_root: cursor.array()?, usage: MeteredUsageClaim {
                compute_units: cursor.u64()?, memory_byte_batches: cursor.u64()?,
                storage_read_bytes: cursor.u64()?, storage_written_bytes: cursor.u64()?,
                ingress_bytes: cursor.u64()?, egress_bytes: cursor.u64()? } },
        opened_at: cursor.u64()? };
    challenge.contradictory.usage.validate()?;
    cursor.finish()?;
    Ok(challenge)
}

pub fn commit_usage(
    offer: Offer<'_>, lease: ComputeLease<'_>, commitment: ProviderCommitment,
    principal: AccountId, height: u64,
) -> Result<(UsageClaim, ChallengeWindow), ProgramError> {
    commitment.usage.validate()?;
    let deadline = height.checked_add(commitment.challenge_window_batches)
        .ok_or_else(malformed)?;
    let expected = offer.unit_price.checked_mul(
        Amount::from_integer(commitment.usage.compute_units))?;
    if commitment.id == [0; 32] || commitment.lease_id != lease.id
        || offer.id != lease.offer_id || principal != lease.provider
        || lease.status != LeaseStatus::Funded || height >= lease.expires_at
        || commitment.input_commitment == [0; 32]
        || commitment.output_digest == [0; 32]
        || commitment.execution_state_root == [0; 32]
        || commitment.challenge_window_batches == 0
        || commitment.challenge_window_batches > MAX_CHALLENGE_WINDOW_BATCHES
        || deadline >= lease.expires_at || commitment.payable != expected
        || commitment.payable > lease.funded
        || commitment.challenger_stake.is_zero()
        || commitment.challenger_stake > offer.stake
    {
        return Err(malformed());
    }
    Ok((UsageClaim {
        id: commitment.id,
        lease_id: commitment.lease_id,
        provider: lease.provider,
        input_commitment: commitment.input_commitment,
        output_digest: commitment.output_digest,
        execution_state_root: commitment.execution_state_root,
        usage: commitment.usage,
        payable: commitment.payable,
        challenger_stake: commitment.challenger_stake,
        committed_at: height,
        challenge_deadline: deadline,
        status: ClaimStatus::Challengeable,
    }, ChallengeWindow { opened_at: height, last_challenge_height: deadline }))
}

pub fn challenge<'a>(
    offer: Offer<'_>, lease: ComputeLease<'_>, mut claim: UsageClaim,
    challenge_id: [u8; 32], challenger: AccountId,
    stake_account: AccountId, stake_seed: &'a [u8], stake: Amount,
    contradictory: ContradictingCommitment, height: u64,
) -> Result<(UsageClaim, UsageChallenge<'a>), ProgramError> {
    contradictory.usage.validate()?;
    let window = ChallengeWindow { opened_at: claim.committed_at,
        last_challenge_height: claim.challenge_deadline };
    let differs = contradictory.input_commitment != claim.input_commitment
        || contradictory.output_digest != claim.output_digest
        || contradictory.execution_state_root != claim.execution_state_root
        || contradictory.usage != claim.usage;
    if offer.id != lease.offer_id || lease.id != claim.lease_id
        || lease.provider != claim.provider || lease.status != LeaseStatus::Funded
        || claim.status != ClaimStatus::Challengeable || challenge_id == [0; 32]
        || challenger == claim.provider || stake_seed.is_empty()
        || stake_seed.len() > layerx_program_sdk::MAX_PROGRAM_ACCOUNT_SEED_BYTES
        || stake.is_zero()
        || stake != claim.challenger_stake || !window.contains(height) || !differs
    {
        return Err(malformed());
    }
    claim.status = ClaimStatus::Frozen;
    Ok((claim, UsageChallenge { id: challenge_id, claim_id: claim.id,
        lease_id: lease.id, offer_id: offer.id, provider: lease.provider,
        tenant: lease.tenant, challenger, stake_account, stake_seed, stake,
        contradictory, opened_at: height }))
}

pub fn finalize_unchallenged<'a>(
    offer: Offer<'a>, mut lease: ComputeLease<'a>, mut claim: UsageClaim,
    height: u64,
) -> Result<(Offer<'a>, ComputeLease<'a>, UsageClaim, SettlementPlan), ProgramError> {
    validate_binding(offer, lease, claim)?;
    if lease.status != LeaseStatus::Funded
        || claim.status != ClaimStatus::Challengeable
        || !(ChallengeWindow { opened_at: claim.committed_at,
            last_challenge_height: claim.challenge_deadline }).elapsed(height)
    {
        return Err(malformed());
    }
    let refund = lease.funded.checked_sub(claim.payable)?;
    let offer = release_capacity(offer, lease)?;
    lease.status = LeaseStatus::Settled;
    claim.status = ClaimStatus::Finalized;
    Ok((offer, lease, claim, SettlementPlan { provider: claim.payable,
        tenant: refund, challenger: Amount::ZERO,
        stake_for_provider: Amount::ZERO }))
}

pub(crate) fn resolve<'a>(
    offer: Offer<'a>, mut lease: ComputeLease<'a>, mut claim: UsageClaim,
    challenge: UsageChallenge<'_>, resolution: ArbiterResolution,
) -> Result<(Offer<'a>, ComputeLease<'a>, UsageClaim, SettlementPlan), ProgramError> {
    validate_binding(offer, lease, claim)?;
    if lease.status != LeaseStatus::Funded
        || claim.status != ClaimStatus::Frozen || challenge.claim_id != claim.id
        || challenge.lease_id != lease.id || challenge.offer_id != offer.id
        || challenge.provider != lease.provider || challenge.tenant != lease.tenant
        || resolution.claim_id != claim.id || resolution.challenge_id != challenge.id
        || resolution.dispute_commitment == [0; 32]
    {
        return Err(malformed());
    }
    let offer = release_capacity(offer, lease)?;
    lease.status = LeaseStatus::Settled;
    let plan = match resolution.verdict {
        ArbiterVerdict::Provider => {
            claim.status = ClaimStatus::ProviderWon;
            SettlementPlan { provider: claim.payable,
                tenant: lease.funded.checked_sub(claim.payable)?,
                challenger: Amount::ZERO, stake_for_provider: challenge.stake }
        }
        ArbiterVerdict::Challenger => {
            claim.status = ClaimStatus::ChallengerWon;
            SettlementPlan { provider: Amount::ZERO, tenant: lease.funded,
                challenger: challenge.stake, stake_for_provider: Amount::ZERO }
        }
    };
    let expected = lease.funded.checked_add(challenge.stake)?;
    if plan.total()? != expected { return Err(malformed()); }
    Ok((offer, lease, claim, plan))
}

fn validate_binding(
    offer: Offer<'_>, lease: ComputeLease<'_>, claim: UsageClaim,
) -> Result<(), ProgramError> {
    claim.usage.validate()?;
    let expected = offer.unit_price.checked_mul(
        Amount::from_integer(claim.usage.compute_units))?;
    if lease.offer_id != offer.id || lease.id != claim.lease_id
        || lease.provider != claim.provider || claim.committed_at > claim.challenge_deadline
        || claim.challenge_deadline >= lease.expires_at || claim.payable != expected
        || claim.payable > lease.funded || claim.challenger_stake.is_zero()
        || claim.challenger_stake > offer.stake
    {
        return Err(malformed());
    }
    Ok(())
}

fn release_capacity<'a>(mut offer: Offer<'a>, lease: ComputeLease<'_>)
    -> Result<Offer<'a>, ProgramError> {
    offer.available_capacity = offer.available_capacity.checked_add(lease.units)
        .filter(|capacity| *capacity <= offer.total_capacity)
        .ok_or_else(malformed)?;
    Ok(offer)
}

#[cfg(target_arch = "wasm32")]
pub fn fund_challenge(challenge: UsageChallenge<'_>, asset: AssetId) -> Result<(), ProgramError> {
    transfer::fund_program_account(ProgramDeposit::new(
        ProgramAccountSeed::new(challenge.stake_seed)?, challenge.stake_account,
        asset, challenge.stake)?)
}

#[cfg(target_arch = "wasm32")]
pub fn execute_settlement(
    lease: ComputeLease<'_>, challenge: Option<UsageChallenge<'_>>,
    plan: SettlementPlan,
) -> Result<(), ProgramError> {
    let escrow_seed = ProgramAccountSeed::new(lease.escrow_seed)?;
    if !plan.provider.is_zero() {
        transfer::pay_from_program_account(ProgramAccountPayment::new(
            escrow_seed, lease.escrow_account, lease.asset,
            lease.provider_payout, plan.provider)?)?;
    }
    if !plan.tenant.is_zero() {
        transfer::pay_from_program_account(ProgramAccountPayment::new(
            escrow_seed, lease.escrow_account, lease.asset,
            lease.tenant_refund, plan.tenant)?)?;
    }
    if !plan.challenger.is_zero() || !plan.stake_for_provider.is_zero() {
        let dispute = challenge.ok_or_else(malformed)?;
        let stake_seed = ProgramAccountSeed::new(dispute.stake_seed)?;
        let (destination, amount) = if !plan.challenger.is_zero() {
            (dispute.challenger, plan.challenger)
        } else {
            (lease.provider_payout, plan.stake_for_provider)
        };
        transfer::pay_from_program_account(ProgramAccountPayment::new(
            stake_seed, dispute.stake_account, lease.asset, destination, amount)?)?;
    }
    Ok(())
}

fn malformed() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{register, open, OpenLease, RegisterOffer, VerificationModel};

    fn account(byte: u8) -> AccountId { AccountId::new([byte; 32]).unwrap() }
    fn fixture<'a>() -> (Offer<'a>, ComputeLease<'a>) {
        let provider = account(1);
        let offer = register(RegisterOffer { id: [1; 32], provider,
            payout: account(2), asset: layerx_program_sdk::AssetId::new([9; 32]).unwrap(),
            stake_account: account(3), stake_seed: b"stake", stake: Amount::from_integer(500u64),
            unit_price: Amount::from_integer(4u64), capacity: 100, minimum_units: 2,
            maximum_units: 20, expires_at: 100, verification: VerificationModel::FraudProvable }, provider, 1).unwrap();
        open(offer, OpenLease { id: [5; 32], offer_id: offer.id, tenant: account(4),
            refund: account(4), escrow_account: account(6), escrow_seed: b"escrow",
            units: 10, funded: Amount::from_integer(40u64), expires_at: 80 }, account(4), 2).unwrap()
    }
    fn commitment() -> ProviderCommitment { ProviderCommitment { id: [7; 32], lease_id: [5; 32],
        input_commitment: [8; 32], output_digest: [9; 32], execution_state_root: [10; 32],
        usage: MeteredUsageClaim { compute_units: 10, memory_byte_batches: 20,
            storage_read_bytes: 30, storage_written_bytes: 40, ingress_bytes: 50, egress_bytes: 60 },
        payable: Amount::from_integer(40u64), challenger_stake: Amount::from_integer(25u64),
        challenge_window_batches: 10 } }

    #[test]
    fn last_height_challenge_freezes_and_late_challenge_is_refused() {
        let (offer, lease) = fixture();
        let (claim, window) = commit_usage(offer, lease, commitment(), account(1), 20).unwrap();
        let contradictory = ContradictingCommitment { output_digest: [11; 32],
            input_commitment: claim.input_commitment, execution_state_root: claim.execution_state_root,
            usage: claim.usage };
        assert!(challenge(offer, lease, claim, [12; 32], account(13), account(14), b"challenge/12",
            claim.challenger_stake, contradictory, window.last_challenge_height).is_ok());
        assert!(challenge(offer, lease, claim, [12; 32], account(13), account(14), b"challenge/12",
            claim.challenger_stake, contradictory, window.last_challenge_height + 1).is_err());
    }

    #[test]
    fn finalization_is_after_window_and_conserves_escrow() {
        let (offer, lease) = fixture();
        let (claim, window) = commit_usage(offer, lease, commitment(), account(1), 20).unwrap();
        assert!(finalize_unchallenged(offer, lease, claim, window.last_challenge_height).is_err());
        let (_, _, final_claim, plan) = finalize_unchallenged(offer, lease, claim,
            window.last_challenge_height + 1).unwrap();
        assert_eq!(final_claim.status, ClaimStatus::Finalized);
        assert_eq!(plan.total().unwrap(), lease.funded);
        assert!(finalize_unchallenged(offer, lease, final_claim,
            window.last_challenge_height + 2).is_err());
    }

    #[test]
    fn both_arbiter_outcomes_conserve_escrow_and_challenge_stake() {
        let (offer, lease) = fixture();
        let (claim, _) = commit_usage(offer, lease, commitment(), account(1), 20).unwrap();
        let contradictory = ContradictingCommitment { output_digest: [11; 32],
            input_commitment: claim.input_commitment, execution_state_root: claim.execution_state_root,
            usage: claim.usage };
        let (frozen, dispute) = challenge(offer, lease, claim, [12; 32], account(13), account(14),
            b"challenge/12", claim.challenger_stake, contradictory, 30).unwrap();
        let resolution = ArbiterResolution { claim_id: claim.id,
            challenge_id: dispute.id, dispute_commitment: [15; 32],
            verdict: ArbiterVerdict::Provider };
        let (_, _, provider_won, provider_plan) =
            resolve(offer, lease, frozen, dispute, resolution).unwrap();
        assert_eq!(provider_won.status, ClaimStatus::ProviderWon);
        assert_eq!(provider_plan.total().unwrap(),
            lease.funded.checked_add(dispute.stake).unwrap());
        let challenger_resolution = ArbiterResolution {
            verdict: ArbiterVerdict::Challenger, ..resolution };
        let (_, _, challenger_won, challenger_plan) =
            resolve(offer, lease, frozen, dispute, challenger_resolution).unwrap();
        assert_eq!(challenger_won.status, ClaimStatus::ChallengerWon);
        assert_eq!(challenger_plan.total().unwrap(),
            lease.funded.checked_add(dispute.stake).unwrap());
    }
}
