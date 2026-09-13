use ed25519_dalek::{Signature, VerifyingKey};
use layerx_types::{amount::Amount, ids::AssetId, intent::EvmAddress};
use sha2::{Digest as _, Sha256};

use crate::{deposit::derive_deposit_id, CustodyDeposit};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCustodyError {
    Layout,
    Profile,
    Binding,
    Signature,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeCustodyExpectation {
    pub network_id: u32,
    pub beneficiary: [u8; 32],
    pub owner_key: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCustodyEvidence {
    EthereumReceipt {
        inclusion_height: u64,
        block_hash: [u8; 32],
        receipts_root: [u8; 32],
        finalized_height: u64,
        finalized_hash: [u8; 32],
        transaction_hash: [u8; 32],
        log_index: u32,
    },
    CometState {
        state_height: u64,
        state_header_hash: [u8; 32],
        application_root: [u8; 32],
        finalized_state_height: u64,
        finalized_header_hash: [u8; 32],
        proof_bundle_hash: [u8; 32],
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttestedNativeCustodyCredit {
    payload: [u8; 427],
    profile: [u8; 207],
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

fn profile_version(
    profile: &[u8],
    expected: NativeCustodyExpectation,
) -> Result<bool, NativeCustodyError> {
    let version = profile.get(..5).ok_or(NativeCustodyError::Profile)?;
    let comet = version == b"LXBC2";
    let chain = number(profile, 5)?;
    let confirmations = number(profile, 161)?;
    let name = b"system:paxeer-reserve";
    let mut reserve = Sha256::new();
    reserve.update(b"LX:ACCOUNT:v1");
    reserve.update(
        u32::try_from(name.len())
            .map_err(|_| NativeCustodyError::Profile)?
            .to_be_bytes(),
    );
    reserve.update(name);
    if profile.len() != 207
        || (!comet && version != b"LXBC1")
        || chain == 0
        || confirmations == 0
        || (comet && (chain != 125 || confirmations >= 8192))
        || expected.network_id == 0
        || field::<4>(profile, 201)? != expected.network_id.to_be_bytes()
        || field::<2>(profile, 205)? != 3_u16.to_be_bytes()
        || profile[129..161] != reserve.finalize()[..]
        || [13..33, 33..65, 97..129, 169..201]
            .iter()
            .any(|range| profile[range.clone()].iter().all(|byte| *byte == 0))
    {
        return Err(NativeCustodyError::Profile);
    }
    Ok(comet)
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

fn evidence(
    profile: &[u8],
    credit: &[u8],
    comet: bool,
) -> Result<NativeCustodyEvidence, NativeCustodyError> {
    let height = number(credit, 215)?;
    let finalized = number(credit, 287)?;
    let first_hash = field(credit, 223)?;
    let root = field(credit, 255)?;
    let final_hash = field(credit, 295)?;
    let reference = field(credit, 327)?;
    if height == 0
        || finalized < height
        || finalized - height < number(profile, 161)? - 1
        || [first_hash, root, final_hash, reference].contains(&[0; 32])
    {
        return Err(NativeCustodyError::Binding);
    }
    if comet {
        if height < 2
            || finalized >= i64::MAX.unsigned_abs()
            || finalized - height >= 8192
            || field::<4>(credit, 359)? != 1_u32.to_be_bytes()
        {
            return Err(NativeCustodyError::Binding);
        }
        Ok(NativeCustodyEvidence::CometState {
            state_height: height,
            state_header_hash: first_hash,
            application_root: root,
            finalized_state_height: finalized,
            finalized_header_hash: final_hash,
            proof_bundle_hash: reference,
        })
    } else {
        Ok(NativeCustodyEvidence::EthereumReceipt {
            inclusion_height: height,
            block_hash: first_hash,
            receipts_root: root,
            finalized_height: finalized,
            finalized_hash: final_hash,
            transaction_hash: reference,
            log_index: u32::from_be_bytes(field(credit, 359)?),
        })
    }
}

impl AttestedNativeCustodyCredit {
    /// Verifies the pinned attestor signature and native credit bindings; the
    /// explicit evidence variant preserves receipt versus state-fact semantics.
    ///
    /// # Errors
    /// Refuses malformed profiles, mixed versions, invalid signatures or any
    /// mismatch with the expected network, beneficiary, owner or deposit facts.
    pub fn verify(
        profile: &[u8],
        credit: &[u8],
        expected: NativeCustodyExpectation,
    ) -> Result<Self, NativeCustodyError> {
        if credit.len() != 427 {
            return Err(NativeCustodyError::Layout);
        }
        let comet = profile_version(profile, expected)?;
        if &credit[..5] != if comet { b"LXDC2" } else { b"LXDC1" } {
            return Err(NativeCustodyError::Binding);
        }
        let custody = custody(profile, credit, expected)?;
        let evidence = evidence(profile, credit, comet)?;
        let authority = VerifyingKey::from_bytes(&field(profile, 65)?)
            .map_err(|_| NativeCustodyError::Profile)?;
        let signature =
            Signature::from_slice(&credit[363..]).map_err(|_| NativeCustodyError::Signature)?;
        let mut message = if comet {
            b"LX:CUSTODY:CREDIT:v2"
        } else {
            b"LX:CUSTODY:CREDIT:v1"
        }
        .to_vec();
        message.extend_from_slice(&credit[..363]);
        authority
            .verify_strict(&message, &signature)
            .map_err(|_| NativeCustodyError::Signature)?;
        Ok(Self {
            payload: field(credit, 0)?,
            profile: field(profile, 0)?,
            custody,
            evidence,
        })
    }

    #[must_use]
    pub const fn profile_bytes(&self) -> &[u8; 207] {
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
    pub const fn canonical_bytes(&self) -> &[u8; 427] {
        &self.payload
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
