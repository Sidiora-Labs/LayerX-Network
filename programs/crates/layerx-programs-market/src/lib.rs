#![no_std]

use layerx_program_sdk::{AccountId, Amount, AssetId, Field, ProgramError, Reason};

pub mod attest;

#[cfg(target_arch = "wasm32")]
use layerx_program_sdk::{
    context::Context,
    event, storage::shared, transfer, CallResult, EventData, EventTopic,
    ProgramAccountPayment, ProgramAccountSeed, ProgramDeposit, StorageValue,
};

#[cfg(target_arch = "wasm32")]
layerx_program_sdk::trap_on_panic!();

const VERSION: u8 = 1;
const REGISTER_OFFER: u8 = 1;
const OPEN_LEASE: u8 = 2;
const SETTLE_LEASE: u8 = 3;
const EXPIRE_LEASE: u8 = 4;
const CLOSE_OFFER: u8 = 5;
const CONFIGURE_ATTESTERS: u8 = 6;
const COMMIT_EXTERNAL_INPUT: u8 = 7;
const SEAL_EXTERNAL_INPUTS: u8 = 8;
const SUBMIT_ATTESTATION: u8 = 9;
const OFFER_PREFIX: &[u8] = b"lx.market.offer/";
const LEASE_PREFIX: &[u8] = b"lx.market.lease/";
const TOPIC_OFFER: &[u8] = b"lx.market.offer";
const TOPIC_LEASE: &[u8] = b"lx.market.lease";
const ID_BYTES: usize = 32;
const MAX_SEED_BYTES: usize = layerx_program_sdk::MAX_PROGRAM_ACCOUNT_SEED_BYTES;
const OFFER_CAPACITY: usize = 263 + MAX_SEED_BYTES;
const LEASE_CAPACITY: usize = 308 + MAX_SEED_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum VerificationModel {
    Bonded = 1,
    Attested = 2,
    FraudProvable = 3,
}

impl VerificationModel {
    fn decode(value: u8) -> Result<Self, ProgramError> {
        match value {
            1 => Ok(Self::Bonded),
            2 => Ok(Self::Attested),
            3 => Ok(Self::FraudProvable),
            _ => Err(malformed()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum OfferStatus {
    Open = 1,
    Closed = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum LeaseStatus {
    Funded = 1,
    Settled = 2,
    ExpiredRefunded = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Offer<'a> {
    pub id: [u8; ID_BYTES],
    pub provider: AccountId,
    pub payout: AccountId,
    pub asset: AssetId,
    pub stake_account: AccountId,
    pub stake_seed: &'a [u8],
    pub stake: Amount,
    pub unit_price: Amount,
    pub total_capacity: u64,
    pub available_capacity: u64,
    pub minimum_units: u64,
    pub maximum_units: u64,
    pub expires_at: u64,
    pub verification: VerificationModel,
    pub status: OfferStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComputeLease<'a> {
    pub id: [u8; ID_BYTES],
    pub offer_id: [u8; ID_BYTES],
    pub provider: AccountId,
    pub tenant: AccountId,
    pub provider_payout: AccountId,
    pub tenant_refund: AccountId,
    pub asset: AssetId,
    pub escrow_account: AccountId,
    pub escrow_seed: &'a [u8],
    pub units: u64,
    pub funded: Amount,
    pub opened_at: u64,
    pub expires_at: u64,
    pub verification: VerificationModel,
    pub status: LeaseStatus,
}

#[derive(Clone, Copy)]
pub struct RegisterOffer<'a> {
    pub id: [u8; ID_BYTES],
    pub provider: AccountId,
    pub payout: AccountId,
    pub asset: AssetId,
    pub stake_account: AccountId,
    pub stake_seed: &'a [u8],
    pub stake: Amount,
    pub unit_price: Amount,
    pub capacity: u64,
    pub minimum_units: u64,
    pub maximum_units: u64,
    pub expires_at: u64,
    pub verification: VerificationModel,
}

#[derive(Clone, Copy)]
pub struct OpenLease<'a> {
    pub id: [u8; ID_BYTES],
    pub offer_id: [u8; ID_BYTES],
    pub tenant: AccountId,
    pub refund: AccountId,
    pub escrow_account: AccountId,
    pub escrow_seed: &'a [u8],
    pub units: u64,
    pub funded: Amount,
    pub expires_at: u64,
}

pub fn register(
    request: RegisterOffer<'_>,
    principal: AccountId,
    height: u64,
) -> Result<Offer<'_>, ProgramError> {
    if request.id == [0; ID_BYTES]
        || request.provider != principal
        || request.capacity == 0
        || request.minimum_units == 0
        || request.maximum_units < request.minimum_units
        || request.maximum_units > request.capacity
        || request.expires_at <= height
        || request.stake.is_zero()
        || request.unit_price.is_zero()
        || request.stake_seed.is_empty()
        || request.stake_seed.len() > MAX_SEED_BYTES
    {
        return Err(malformed());
    }
    Ok(Offer {
        id: request.id,
        provider: request.provider,
        payout: request.payout,
        asset: request.asset,
        stake_account: request.stake_account,
        stake_seed: request.stake_seed,
        stake: request.stake,
        unit_price: request.unit_price,
        total_capacity: request.capacity,
        available_capacity: request.capacity,
        minimum_units: request.minimum_units,
        maximum_units: request.maximum_units,
        expires_at: request.expires_at,
        verification: request.verification,
        status: OfferStatus::Open,
    })
}

pub fn open<'a>(
    offer: Offer<'a>,
    request: OpenLease<'a>,
    principal: AccountId,
    height: u64,
) -> Result<(Offer<'a>, ComputeLease<'a>), ProgramError> {
    let expected = offer.unit_price.checked_mul(Amount::from_integer(request.units))?;
    if request.id == [0; ID_BYTES]
        || request.offer_id != offer.id
        || request.tenant != principal
        || offer.status != OfferStatus::Open
        || height >= offer.expires_at
        || request.expires_at <= height
        || request.expires_at > offer.expires_at
        || request.units < offer.minimum_units
        || request.units > offer.maximum_units
        || request.units > offer.available_capacity
        || request.funded != expected
        || request.escrow_seed.is_empty()
        || request.escrow_seed.len() > MAX_SEED_BYTES
    {
        return Err(malformed());
    }
    let mut updated = offer;
    updated.available_capacity = updated
        .available_capacity
        .checked_sub(request.units)
        .ok_or_else(malformed)?;
    let lease = ComputeLease {
        id: request.id,
        offer_id: offer.id,
        provider: offer.provider,
        tenant: request.tenant,
        provider_payout: offer.payout,
        tenant_refund: request.refund,
        asset: offer.asset,
        escrow_account: request.escrow_account,
        escrow_seed: request.escrow_seed,
        units: request.units,
        funded: request.funded,
        opened_at: height,
        expires_at: request.expires_at,
        verification: offer.verification,
        status: LeaseStatus::Funded,
    };
    Ok((updated, lease))
}

pub fn settle<'a>(
    offer: Offer<'a>,
    mut lease: ComputeLease<'a>,
    principal: AccountId,
    height: u64,
) -> Result<(Offer<'a>, ComputeLease<'a>), ProgramError> {
    if lease.offer_id != offer.id
        || lease.provider != principal
        || lease.status != LeaseStatus::Funded
        || height >= lease.expires_at
    {
        return Err(malformed());
    }
    let mut updated = offer;
    updated.available_capacity = updated
        .available_capacity
        .checked_add(lease.units)
        .filter(|capacity| *capacity <= updated.total_capacity)
        .ok_or_else(malformed)?;
    lease.status = LeaseStatus::Settled;
    Ok((updated, lease))
}

pub fn expire<'a>(
    offer: Offer<'a>,
    mut lease: ComputeLease<'a>,
    height: u64,
) -> Result<(Offer<'a>, ComputeLease<'a>), ProgramError> {
    if lease.offer_id != offer.id
        || lease.status != LeaseStatus::Funded
        || height < lease.expires_at
    {
        return Err(malformed());
    }
    let mut updated = offer;
    updated.available_capacity = updated
        .available_capacity
        .checked_add(lease.units)
        .filter(|capacity| *capacity <= updated.total_capacity)
        .ok_or_else(malformed)?;
    lease.status = LeaseStatus::ExpiredRefunded;
    Ok((updated, lease))
}

pub fn close<'a>(
    mut offer: Offer<'a>,
    principal: AccountId,
) -> Result<Offer<'a>, ProgramError> {
    if offer.provider != principal
        || offer.status != OfferStatus::Open
        || offer.available_capacity != offer.total_capacity
    {
        return Err(malformed());
    }
    offer.status = OfferStatus::Closed;
    Ok(offer)
}

fn malformed() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self { Self { bytes, offset: 0 } }
    fn take(&mut self, length: usize) -> Result<&'a [u8], ProgramError> {
        let end = self.offset.checked_add(length).ok_or_else(malformed)?;
        let value = self.bytes.get(self.offset..end).ok_or_else(malformed)?;
        self.offset = end;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProgramError> {
        self.take(N)?.try_into().map_err(|_| malformed())
    }
    fn byte(&mut self) -> Result<u8, ProgramError> { Ok(self.take(1)?[0]) }
    fn u64(&mut self) -> Result<u64, ProgramError> { Ok(u64::from_be_bytes(self.array()?)) }
    fn amount(&mut self) -> Result<Amount, ProgramError> { Ok(Amount::from_be_bytes(self.array()?)) }
    fn account(&mut self) -> Result<AccountId, ProgramError> { AccountId::new(self.array()?) }
    fn asset(&mut self) -> Result<AssetId, ProgramError> { AssetId::new(self.array()?) }
    fn seed(&mut self) -> Result<&'a [u8], ProgramError> {
        let length = usize::from(u16::from_be_bytes(self.array()?));
        if length == 0 { return Err(malformed()); }
        self.take(length)
    }
    fn remainder(self) -> &'a [u8] { &self.bytes[self.offset..] }
    fn finish(self) -> Result<(), ProgramError> {
        if self.offset == self.bytes.len() { Ok(()) } else { Err(malformed()) }
    }
}

fn append(output: &mut [u8], offset: &mut usize, value: &[u8]) -> Result<(), ProgramError> {
    let end = offset.checked_add(value.len()).ok_or_else(malformed)?;
    output.get_mut(*offset..end).ok_or_else(malformed)?.copy_from_slice(value);
    *offset = end;
    Ok(())
}

fn append_seed(output: &mut [u8], offset: &mut usize, seed: &[u8]) -> Result<(), ProgramError> {
    let length = u16::try_from(seed.len()).map_err(|_| malformed())?;
    append(output, offset, &length.to_be_bytes())?;
    append(output, offset, seed)
}

fn encode_offer(offer: Offer<'_>, output: &mut [u8]) -> Result<usize, ProgramError> {
    let mut offset = 0;
    append(output, &mut offset, &[VERSION, offer.status as u8, offer.verification as u8])?;
    append(output, &mut offset, &offer.id)?;
    append(output, &mut offset, &offer.provider.bytes())?;
    append(output, &mut offset, &offer.payout.bytes())?;
    append(output, &mut offset, &offer.asset.bytes())?;
    append(output, &mut offset, &offer.stake_account.bytes())?;
    append_seed(output, &mut offset, offer.stake_seed)?;
    append(output, &mut offset, &offer.stake.to_be_bytes())?;
    append(output, &mut offset, &offer.unit_price.to_be_bytes())?;
    for value in [offer.total_capacity, offer.available_capacity, offer.minimum_units, offer.maximum_units, offer.expires_at] {
        append(output, &mut offset, &value.to_be_bytes())?;
    }
    Ok(offset)
}

fn decode_offer(input: &[u8]) -> Result<Offer<'_>, ProgramError> {
    let mut cursor = Cursor::new(input);
    if cursor.byte()? != VERSION { return Err(malformed()); }
    let status = match cursor.byte()? { 1 => OfferStatus::Open, 2 => OfferStatus::Closed, _ => return Err(malformed()) };
    let verification = VerificationModel::decode(cursor.byte()?)?;
    let offer = Offer {
        id: cursor.array()?, provider: cursor.account()?, payout: cursor.account()?, asset: cursor.asset()?,
        stake_account: cursor.account()?, stake_seed: cursor.seed()?, stake: cursor.amount()?,
        unit_price: cursor.amount()?, total_capacity: cursor.u64()?, available_capacity: cursor.u64()?,
        minimum_units: cursor.u64()?, maximum_units: cursor.u64()?, expires_at: cursor.u64()?, verification, status,
    };
    cursor.finish()?;
    Ok(offer)
}

fn encode_lease(lease: ComputeLease<'_>, output: &mut [u8]) -> Result<usize, ProgramError> {
    let mut offset = 0;
    append(output, &mut offset, &[VERSION, lease.status as u8, lease.verification as u8])?;
    append(output, &mut offset, &lease.id)?;
    append(output, &mut offset, &lease.offer_id)?;
    for account in [lease.provider, lease.tenant, lease.provider_payout, lease.tenant_refund] { append(output, &mut offset, &account.bytes())?; }
    append(output, &mut offset, &lease.asset.bytes())?;
    append(output, &mut offset, &lease.escrow_account.bytes())?;
    append_seed(output, &mut offset, lease.escrow_seed)?;
    append(output, &mut offset, &lease.units.to_be_bytes())?;
    append(output, &mut offset, &lease.funded.to_be_bytes())?;
    append(output, &mut offset, &lease.opened_at.to_be_bytes())?;
    append(output, &mut offset, &lease.expires_at.to_be_bytes())?;
    Ok(offset)
}

fn decode_lease(input: &[u8]) -> Result<ComputeLease<'_>, ProgramError> {
    let mut cursor = Cursor::new(input);
    if cursor.byte()? != VERSION { return Err(malformed()); }
    let status = match cursor.byte()? { 1 => LeaseStatus::Funded, 2 => LeaseStatus::Settled, 3 => LeaseStatus::ExpiredRefunded, _ => return Err(malformed()) };
    let verification = VerificationModel::decode(cursor.byte()?)?;
    let lease = ComputeLease {
        id: cursor.array()?, offer_id: cursor.array()?, provider: cursor.account()?, tenant: cursor.account()?,
        provider_payout: cursor.account()?, tenant_refund: cursor.account()?, asset: cursor.asset()?,
        escrow_account: cursor.account()?, escrow_seed: cursor.seed()?, units: cursor.u64()?, funded: cursor.amount()?,
        opened_at: cursor.u64()?, expires_at: cursor.u64()?, verification, status,
    };
    cursor.finish()?;
    Ok(lease)
}

#[cfg(target_arch = "wasm32")]
fn state_key(prefix: &[u8], id: [u8; ID_BYTES]) -> Result<([u8; 64], usize), ProgramError> {
    let mut key = [0; 64];
    let end = prefix.len().checked_add(ID_BYTES).ok_or_else(malformed)?;
    key[..prefix.len()].copy_from_slice(prefix);
    key[prefix.len()..end].copy_from_slice(&id);
    Ok((key, end))
}

#[cfg(target_arch = "wasm32")]
fn read_state<'a>(prefix: &[u8], id: [u8; 32], output: &'a mut [u8]) -> Result<&'a [u8], ProgramError> {
    let (key, length) = state_key(prefix, id)?;
    let written = shared::read(shared::SharedStorageKey::new(&key[..length])?, output)?
        .ok_or_else(|| ProgramError::value(Field::StorageValue, Reason::Malformed))?;
    output.get(..written).ok_or_else(malformed)
}

#[cfg(target_arch = "wasm32")]
fn write_state(prefix: &[u8], id: [u8; 32], value: &[u8]) -> Result<(), ProgramError> {
    let (key, length) = state_key(prefix, id)?;
    shared::write(shared::SharedStorageKey::new(&key[..length])?, StorageValue::new(value)?)
}

#[cfg(target_arch = "wasm32")]
fn absent(prefix: &[u8], id: [u8; 32], scratch: &mut [u8]) -> Result<(), ProgramError> {
    let (key, length) = state_key(prefix, id)?;
    if shared::read(shared::SharedStorageKey::new(&key[..length])?, scratch)?.is_some() {
        return Err(ProgramError::value(Field::StorageValue, Reason::Duplicate));
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn principal() -> Result<AccountId, ProgramError> { AccountId::new(Context::invoking_principal()?.bytes()) }

#[cfg(target_arch = "wasm32")]
fn emit(topic: &[u8], bytes: &[u8]) -> Result<(), ProgramError> {
    event::emit(EventTopic::new(topic)?, EventData::new(bytes)?)
}

#[cfg(target_arch = "wasm32")]
fn invoke(input: &[u8]) -> Result<CallResult, ProgramError> {
    let mut cursor = Cursor::new(input);
    if cursor.byte()? != VERSION { return Err(malformed()); }
    let operation = cursor.byte()?;
    let height = Context::batch_height()?;
    let caller = principal()?;
    match operation {
        REGISTER_OFFER => {
            let request = RegisterOffer {
                id: cursor.array()?, provider: cursor.account()?, payout: cursor.account()?, asset: cursor.asset()?,
                stake_account: cursor.account()?, stake_seed: cursor.seed()?, stake: cursor.amount()?, unit_price: cursor.amount()?,
                capacity: cursor.u64()?, minimum_units: cursor.u64()?, maximum_units: cursor.u64()?, expires_at: cursor.u64()?,
                verification: VerificationModel::decode(cursor.byte()?)?,
            };
            cursor.finish()?;
            let mut scratch = [0; OFFER_CAPACITY];
            absent(OFFER_PREFIX, request.id, &mut scratch)?;
            let offer = register(request, caller, height)?;
            transfer::fund_program_account(ProgramDeposit::new(ProgramAccountSeed::new(offer.stake_seed)?, offer.stake_account, offer.asset, offer.stake)?)?;
            let written = encode_offer(offer, &mut scratch)?;
            write_state(OFFER_PREFIX, offer.id, &scratch[..written])?;
            emit(TOPIC_OFFER, &scratch[..written])?;
            Ok(CallResult::OK)
        }
        OPEN_LEASE => {
            let request = OpenLease {
                id: cursor.array()?, offer_id: cursor.array()?, tenant: cursor.account()?, refund: cursor.account()?,
                escrow_account: cursor.account()?, escrow_seed: cursor.seed()?, units: cursor.u64()?, funded: cursor.amount()?, expires_at: cursor.u64()?,
            };
            cursor.finish()?;
            let mut offer_bytes = [0; OFFER_CAPACITY];
            let offer = decode_offer(read_state(OFFER_PREFIX, request.offer_id, &mut offer_bytes)?)?;
            let mut lease_bytes = [0; LEASE_CAPACITY];
            absent(LEASE_PREFIX, request.id, &mut lease_bytes)?;
            let (offer, lease) = open(offer, request, caller, height)?;
            transfer::fund_program_account(ProgramDeposit::new(ProgramAccountSeed::new(lease.escrow_seed)?, lease.escrow_account, lease.asset, lease.funded)?)?;
            let mut offer_output = [0; OFFER_CAPACITY];
            let offer_written = encode_offer(offer, &mut offer_output)?;
            let lease_written = encode_lease(lease, &mut lease_bytes)?;
            write_state(OFFER_PREFIX, offer.id, &offer_output[..offer_written])?;
            write_state(LEASE_PREFIX, lease.id, &lease_bytes[..lease_written])?;
            emit(TOPIC_LEASE, &lease_bytes[..lease_written])?;
            Ok(CallResult::OK)
        }
        SETTLE_LEASE | EXPIRE_LEASE => {
            let lease_id = cursor.array()?;
            cursor.finish()?;
            let mut lease_bytes = [0; LEASE_CAPACITY];
            let lease = decode_lease(read_state(LEASE_PREFIX, lease_id, &mut lease_bytes)?)?;
            let mut offer_bytes = [0; OFFER_CAPACITY];
            let offer = decode_offer(read_state(OFFER_PREFIX, lease.offer_id, &mut offer_bytes)?)?;
            let (offer, lease, destination) = if operation == SETTLE_LEASE {
                if lease.verification == VerificationModel::Attested {
                    attest::require_ready(lease.id)?;
                }
                let (offer, lease) = settle(offer, lease, caller, height)?;
                let destination = lease.provider_payout;
                (offer, lease, destination)
            } else {
                let (offer, lease) = expire(offer, lease, height)?;
                let destination = lease.tenant_refund;
                (offer, lease, destination)
            };
            transfer::pay_from_program_account(ProgramAccountPayment::new(ProgramAccountSeed::new(lease.escrow_seed)?, lease.escrow_account, lease.asset, destination, lease.funded)?)?;
            let mut offer_output = [0; OFFER_CAPACITY];
            let mut lease_output = [0; LEASE_CAPACITY];
            let offer_written = encode_offer(offer, &mut offer_output)?;
            let lease_written = encode_lease(lease, &mut lease_output)?;
            write_state(OFFER_PREFIX, offer.id, &offer_output[..offer_written])?;
            write_state(LEASE_PREFIX, lease.id, &lease_output[..lease_written])?;
            emit(TOPIC_LEASE, &lease_output[..lease_written])?;
            Ok(CallResult::OK)
        }
        CLOSE_OFFER => {
            let offer_id = cursor.array()?;
            cursor.finish()?;
            let mut bytes = [0; OFFER_CAPACITY];
            let offer = close(decode_offer(read_state(OFFER_PREFIX, offer_id, &mut bytes)?)?, caller)?;
            transfer::pay_from_program_account(ProgramAccountPayment::new(ProgramAccountSeed::new(offer.stake_seed)?, offer.stake_account, offer.asset, offer.provider, offer.stake)?)?;
            let mut output = [0; OFFER_CAPACITY];
            let written = encode_offer(offer, &mut output)?;
            write_state(OFFER_PREFIX, offer.id, &output[..written])?;
            emit(TOPIC_OFFER, &output[..written])?;
            Ok(CallResult::OK)
        }
        CONFIGURE_ATTESTERS => {
            let lease_id = cursor.array()?;
            let revision = cursor.u64()?;
            let count = usize::from(cursor.byte()?);
            if count == 0 || count > attest::MAX_ATTESTERS { return Err(malformed()); }
            let mut entries = [None; attest::MAX_ATTESTERS];
            for entry in entries.iter_mut().take(count) {
                let name_length = usize::from(cursor.byte()?);
                let name = cursor.take(name_length)?;
                *entry = Some(attest::Attester { name, ed25519_key: cursor.array()? });
            }
            cursor.finish()?;
            let mut lease_bytes = [0; LEASE_CAPACITY];
            let lease = decode_lease(read_state(LEASE_PREFIX, lease_id, &mut lease_bytes)?)?;
            if lease.tenant != caller || lease.verification != VerificationModel::Attested || lease.status != LeaseStatus::Funded {
                return Err(malformed());
            }
            let mut scratch = [0; attest::ATTESTER_SET_CAPACITY];
            attest::configure(lease_id, caller, revision, entries, &mut scratch)?;
            Ok(CallResult::OK)
        }
        COMMIT_EXTERNAL_INPUT => {
            let commitment = attest::InputCommitment {
                lease_id: cursor.array()?, input_id: cursor.array()?, payload_digest: cursor.array()?,
                payload_length: cursor.u64()?, source: match cursor.byte()? {
                    1 => attest::ExternalInputSource::HttpsApi,
                    2 => attest::ExternalInputSource::HardwareSensor,
                    3 => attest::ExternalInputSource::ConfidentialCompute,
                    4 => attest::ExternalInputSource::HumanOperator,
                    _ => return Err(malformed()),
                }, source_locator_digest: cursor.array()?,
            };
            cursor.finish()?;
            let mut lease_bytes = [0; LEASE_CAPACITY];
            let lease = decode_lease(read_state(LEASE_PREFIX, commitment.lease_id, &mut lease_bytes)?)?;
            if lease.tenant != caller || lease.verification != VerificationModel::Attested || lease.status != LeaseStatus::Funded {
                return Err(malformed());
            }
            attest::commit(commitment, caller)?;
            Ok(CallResult::OK)
        }
        SEAL_EXTERNAL_INPUTS => {
            let lease_id = cursor.array()?;
            cursor.finish()?;
            let mut lease_bytes = [0; LEASE_CAPACITY];
            let lease = decode_lease(read_state(LEASE_PREFIX, lease_id, &mut lease_bytes)?)?;
            if lease.tenant != caller || lease.verification != VerificationModel::Attested || lease.status != LeaseStatus::Funded {
                return Err(malformed());
            }
            attest::seal(lease_id, caller, height)?;
            Ok(CallResult::OK)
        }
        SUBMIT_ATTESTATION => {
            let attestation = attest::decode_attestation(cursor.remainder())?;
            let mut lease_bytes = [0; LEASE_CAPACITY];
            let lease = decode_lease(read_state(LEASE_PREFIX, attestation.input.lease_id, &mut lease_bytes)?)?;
            if lease.provider != caller || lease.verification != VerificationModel::Attested || lease.status != LeaseStatus::Funded {
                return Err(malformed());
            }
            attest::admit(attestation, height)?;
            Ok(CallResult::OK)
        }
        _ => Err(malformed()),
    }
}

#[cfg(target_arch = "wasm32")]
layerx_program_sdk::entrypoint!(invoke);

#[cfg(test)]
mod tests {
    use super::*;

    fn account(value: u8) -> AccountId { AccountId::new([value; 32]).unwrap_or_else(|error| panic!("account: {error}")) }
    fn asset() -> AssetId { AssetId::new([9; 32]).unwrap_or_else(|error| panic!("asset: {error}")) }
    fn offer<'a>(provider: AccountId, seed: &'a [u8]) -> Offer<'a> {
        register(RegisterOffer { id: [1; 32], provider, payout: account(2), asset: asset(), stake_account: account(3), stake_seed: seed, stake: Amount::from_integer(500u64), unit_price: Amount::from_integer(4u64), capacity: 100, minimum_units: 2, maximum_units: 20, expires_at: 50, verification: VerificationModel::FraudProvable }, provider, 1).unwrap_or_else(|error| panic!("offer: {error}"))
    }

    #[test]
    fn funded_lease_settles_and_releases_capacity() {
        let provider = account(1);
        let tenant = account(4);
        let offer = offer(provider, b"stake/offer-1");
        let request = OpenLease { id: [5; 32], offer_id: offer.id, tenant, refund: tenant, escrow_account: account(6), escrow_seed: b"lease/5", units: 10, funded: Amount::from_integer(40u64), expires_at: 20 };
        let (offer, lease) = open(offer, request, tenant, 2).unwrap_or_else(|error| panic!("open: {error}"));
        assert_eq!(offer.available_capacity, 90);
        let (offer, lease) = settle(offer, lease, provider, 19).unwrap_or_else(|error| panic!("settle: {error}"));
        assert_eq!(offer.available_capacity, 100);
        assert_eq!(lease.status, LeaseStatus::Settled);
    }

    #[test]
    fn provider_absence_refunds_expired_lease() {
        let provider = account(1);
        let tenant = account(4);
        let offer = offer(provider, b"stake/offer-1");
        let request = OpenLease { id: [5; 32], offer_id: offer.id, tenant, refund: tenant, escrow_account: account(6), escrow_seed: b"lease/5", units: 10, funded: Amount::from_integer(40u64), expires_at: 20 };
        let (offer, lease) = open(offer, request, tenant, 2).unwrap_or_else(|error| panic!("open: {error}"));
        assert!(settle(offer, lease, provider, 20).is_err());
        let (offer, lease) = expire(offer, lease, 20).unwrap_or_else(|error| panic!("expire: {error}"));
        assert_eq!(offer.available_capacity, 100);
        assert_eq!(lease.status, LeaseStatus::ExpiredRefunded);
    }

    #[test]
    fn unfunded_and_mid_work_expiry_are_refused() {
        let provider = account(1);
        let tenant = account(4);
        let offer = offer(provider, b"stake/offer-1");
        let request = OpenLease { id: [5; 32], offer_id: offer.id, tenant, refund: tenant, escrow_account: account(6), escrow_seed: b"lease/5", units: 10, funded: Amount::from_integer(39u64), expires_at: 20 };
        assert!(open(offer, request, tenant, 2).is_err());
        let funded = OpenLease { funded: Amount::from_integer(40u64), ..request };
        let (offer, lease) = open(offer, funded, tenant, 2).unwrap_or_else(|error| panic!("open: {error}"));
        assert!(expire(offer, lease, 19).is_err());
        assert!(settle(offer, lease, provider, 20).is_err());
    }
}
