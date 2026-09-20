use ed25519_dalek::VerifyingKey;
use layerx_types::{amount::Amount, ids::AssetId, intent::EvmAddress, limits::MAX_PAYLOAD_BYTES};
use sha2::{Digest as _, Sha256};

use crate::{deposit::derive_deposit_id, CustodyDeposit};

pub const NATIVE_CUSTODY_PROFILE_BYTES: usize = 223;
pub const NATIVE_CUSTODY_CREDIT_HEAD_BYTES: usize = 363;
pub const NATIVE_CUSTODY_CREDIT_MAX_BYTES: usize = MAX_PAYLOAD_BYTES;
const MAX_UNIX_SECONDS: u64 = 253_402_300_799;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCustodyError {
    Layout,
    Profile,
    Binding,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeCustodyExpectation {
    pub network_id: u32,
    pub beneficiary: [u8; 32],
    pub owner_key: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeCustodyEvidence {
    pub state_height: u64,
    pub header_height: u64,
    pub header_hash: [u8; 32],
    pub application_root: [u8; 32],
    pub validators_hash: [u8; 32],
    pub proof_bundle_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCustodyCredit {
    payload: Vec<u8>,
    profile: [u8; NATIVE_CUSTODY_PROFILE_BYTES],
    custody: CustodyDeposit,
    evidence: NativeCustodyEvidence,
}

fn field<const N: usize>(bytes: &[u8], start: usize) -> Result<[u8; N], NativeCustodyError> {
    bytes
        .get(start..start + N)
        .ok_or(NativeCustodyError::Layout)?
        .try_into()
        .map_err(|_| NativeCustodyError::Layout)
}

fn number(bytes: &[u8], start: usize) -> Result<u64, NativeCustodyError> {
    Ok(u64::from_be_bytes(field(bytes, start)?))
}

fn comet_chain_id(profile: &[u8]) -> bool {
    let text = &profile[169..201];
    let length = text.iter().position(|byte| *byte == 0).unwrap_or(32);
    length != 0
        && text[..length]
            .iter()
            .all(|byte| (0x21..=0x7e).contains(byte))
        && text[length..].iter().all(|byte| *byte == 0)
}

fn validate_profile(
    profile: &[u8],
    expected: NativeCustodyExpectation,
) -> Result<(), NativeCustodyError> {
    if profile.len() != NATIVE_CUSTODY_PROFILE_BYTES {
        return Err(NativeCustodyError::Profile);
    }
    let name = b"system:paxeer-reserve";
    let mut reserve = Sha256::new();
    reserve.update(b"LX:ACCOUNT:v1");
    reserve.update(
        u32::try_from(name.len())
            .map_err(|_| NativeCustodyError::Profile)?
            .to_be_bytes(),
    );
    reserve.update(name);
    let mut module = Sha256::new();
    module.update(b"LX:CUSTODY:MODULE:v1");
    module.update(b"layerxcustody");
    module.update(&profile[13..33]);
    let trusted_height = number(profile, 161)?;
    let trusting_period = number(profile, 207)?;
    let trusted_time = number(profile, 215)?;
    if &profile[..5] != b"LXBC3"
        || number(profile, 5)? != 125
        || profile[33..65] != module.finalize()[..]
        || trusted_height == 0
        || trusted_height >= i64::MAX.unsigned_abs()
        || !comet_chain_id(profile)
        || expected.network_id == 0
        || field::<4>(profile, 201)? != expected.network_id.to_be_bytes()
        || field::<2>(profile, 205)? != 3_u16.to_be_bytes()
        || profile[129..161] != reserve.finalize()[..]
        || trusting_period == 0
        || trusting_period > u64::from(u32::MAX)
        || trusted_time == 0
        || trusted_time > MAX_UNIX_SECONDS
        || [13..33, 65..97, 97..129]
            .iter()
            .any(|range| profile[range.clone()].iter().all(|byte| *byte == 0))
    {
        return Err(NativeCustodyError::Profile);
    }
    Ok(())
}

fn custody(
    profile: &[u8],
    credit: &[u8],
    expected: NativeCustodyExpectation,
) -> Result<CustodyDeposit, NativeCustodyError> {
    let facts = CustodyDeposit {
        deposit_id: field(credit, 43)?,
        asset: AssetId::new(field(credit, 75)?),
        beneficiary: field(credit, 107)?,
        payer: EvmAddress::new(field(credit, 171)?),
        amount: Amount::from_be_bytes(field(credit, 191)?),
        nonce: number(credit, 207)?,
    };
    if credit[5..37] != Sha256::digest(profile)[..]
        || credit[37..43] != profile[201..207]
        || credit[75..107] != profile[97..129]
        || facts.beneficiary != expected.beneficiary
        || field::<32>(credit, 139)? != expected.owner_key
        || expected.beneficiary == [0; 32]
        || facts.payer.bytes() == [0; 20]
        || facts.amount.to_be_bytes() == [0; 16]
        || facts.nonce == 0
        || derive_deposit_id(
            number(profile, 5)?,
            EvmAddress::new(field(profile, 13)?),
            &facts,
        ) != facts.deposit_id
    {
        return Err(NativeCustodyError::Binding);
    }
    let owner =
        VerifyingKey::from_bytes(&expected.owner_key).map_err(|_| NativeCustodyError::Binding)?;
    if owner.is_weak() {
        return Err(NativeCustodyError::Binding);
    }
    Ok(facts)
}

fn hash8(bundle: &[u8], at: &mut usize) -> Result<Option<[u8; 32]>, NativeCustodyError> {
    let length = *bundle.get(*at).ok_or(NativeCustodyError::Binding)?;
    *at += 1;
    match length {
        0 => Ok(None),
        32 => {
            let value = field(bundle, *at).map_err(|_| NativeCustodyError::Binding)?;
            *at += 32;
            Ok(Some(value))
        }
        _ => Err(NativeCustodyError::Binding),
    }
}

struct BundleHeader {
    height: u64,
    validators_hash: [u8; 32],
    application_root: [u8; 32],
}

fn bundle_header(bundle: &[u8]) -> Result<BundleHeader, NativeCustodyError> {
    if bundle.get(..5) != Some(b"LXLB1".as_slice()) {
        return Err(NativeCustodyError::Binding);
    }
    let height = number(bundle, 21).map_err(|_| NativeCustodyError::Binding)?;
    let mut at = 41;
    hash8(bundle, &mut at)?;
    at += 4;
    hash8(bundle, &mut at)?;
    let mut hashes = [None; 8];
    for hash in &mut hashes {
        *hash = hash8(bundle, &mut at)?;
    }
    Ok(BundleHeader {
        height,
        validators_hash: hashes[2].ok_or(NativeCustodyError::Binding)?,
        application_root: hashes[5].ok_or(NativeCustodyError::Binding)?,
    })
}

fn evidence(credit: &[u8]) -> Result<NativeCustodyEvidence, NativeCustodyError> {
    let evidence = NativeCustodyEvidence {
        state_height: number(credit, 215)?,
        header_height: number(credit, 287)?,
        header_hash: field(credit, 223)?,
        application_root: field(credit, 255)?,
        validators_hash: field(credit, 295)?,
        proof_bundle_hash: field(credit, 327)?,
    };
    if evidence.state_height == 0
        || evidence.state_height >= i64::MAX.unsigned_abs() - 1
        || evidence.header_height != evidence.state_height + 1
        || field::<4>(credit, 359)? != 2_u32.to_be_bytes()
        || evidence.proof_bundle_hash
            != <[u8; 32]>::from(Sha256::digest(&credit[NATIVE_CUSTODY_CREDIT_HEAD_BYTES..]))
        || evidence.header_hash == [0; 32]
    {
        return Err(NativeCustodyError::Binding);
    }
    let header = bundle_header(&credit[NATIVE_CUSTODY_CREDIT_HEAD_BYTES..])?;
    if header.height != evidence.header_height
        || header.validators_hash != evidence.validators_hash
        || header.application_root != evidence.application_root
    {
        return Err(NativeCustodyError::Binding);
    }
    Ok(evidence)
}

impl NativeCustodyCredit {
    /// Checks the light-client credit head against the pinned profile, the
    /// expected account, the carried bundle hash and the bundle header's height,
    /// validator-set hash and application root. The header hash, commit and
    /// store proofs inside the bundle are verified by the LayerX bridge module,
    /// not here.
    ///
    /// # Errors
    /// Refuses malformed profiles, retired formats, or any mismatch with the
    /// expected network, beneficiary, owner, deposit facts or bundle hash.
    pub fn verify(
        profile: &[u8],
        credit: &[u8],
        expected: NativeCustodyExpectation,
    ) -> Result<Self, NativeCustodyError> {
        if credit.len() <= NATIVE_CUSTODY_CREDIT_HEAD_BYTES
            || credit.len() > NATIVE_CUSTODY_CREDIT_MAX_BYTES
        {
            return Err(NativeCustodyError::Layout);
        }
        validate_profile(profile, expected)?;
        if &credit[..5] != b"LXDC3" {
            return Err(NativeCustodyError::Binding);
        }
        let custody = custody(profile, credit, expected)?;
        let evidence = evidence(credit)?;
        Ok(Self {
            payload: credit.to_vec(),
            profile: field(profile, 0)?,
            custody,
            evidence,
        })
    }

    #[must_use]
    pub const fn profile_bytes(&self) -> &[u8; NATIVE_CUSTODY_PROFILE_BYTES] {
        &self.profile
    }

    #[must_use]
    pub fn nullifier(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"LX:DEPOSIT:NULLIFIER:v1");
        hash.update(self.custody.deposit_id);
        hash.finalize().into()
    }

    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.payload
    }

    #[must_use]
    pub fn head_bytes(&self) -> &[u8] {
        &self.payload[..NATIVE_CUSTODY_CREDIT_HEAD_BYTES]
    }

    #[must_use]
    pub fn bundle_bytes(&self) -> &[u8] {
        &self.payload[NATIVE_CUSTODY_CREDIT_HEAD_BYTES..]
    }

    #[must_use]
    pub const fn custody(&self) -> &CustodyDeposit {
        &self.custody
    }

    #[must_use]
    pub const fn evidence(&self) -> &NativeCustodyEvidence {
        &self.evidence
    }
}
