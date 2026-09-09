//! Canonical payment and Programs payload codecs used by signer disclosure.
//!
//! Integers are big-endian. Native asset identifiers are
//! `SHA-256("LX:ASSET:v1" || issuer_did_id32 || salt32)`, where
//! `issuer_did_id32` is the existing `lxp_did_id_derive` identity
//! (`SHA-256("LXP/v1/did-id\0" || u16be(len) || did)`). Asset ordinal 9
//! (WITHDRAW) is refused. Receive and grant-issue bytes match the
//! existing activity payloads (`0x5201` / 8 fields and `0x2001` grant
//! structure). See [`crate::disclosure`].

use layerx_types::payload::ModuleId;
use layerx_wire::{decode::Decoder, encode::Encoder};
use sha2::{Digest as _, Sha256};

use crate::disclosure::DisclosureError;

type Id = [u8; 32];

/// Asset registration fields for Asset ordinal 1.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Registration {
    pub asset: Id,
    pub salt: Id,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub supply_cap: u128,
    pub issuer_kind: u8,
    pub custody_ref: Vec<u8>,
}

/// Existing authority-grant structure used by Asset ordinal 7.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Grant {
    pub grantor: Id,
    pub grantee: Id,
    pub kind: u8,
    pub key: Id,
    pub module_mask: u64,
    pub ordinal_min: u16,
    pub ordinal_max: u16,
    pub asset: Id,
    pub maximum_per_activity: u128,
    pub maximum_total: u128,
    pub spent_total: u128,
    pub period_length: u64,
    pub maximum_per_period: u128,
    pub spent_this_period: u128,
    pub period_start: u64,
    pub purpose_hash: Id,
    pub not_before: u64,
    pub not_after: u64,
    pub revocation_sequence: u64,
    pub revoked: bool,
    pub revoked_at_sequence: u64,
    pub signature: [u8; 64],
}

/// One conserved Programs transfer leg (from, asset, to, amount).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferLeg {
    pub from: Id,
    pub asset: Id,
    pub to: Id,
    pub amount: u128,
}

/// Complete payment or Programs payload bound into a disclosure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Payment {
    Register(Registration),
    OpenAccount {
        asset: Id,
    },
    Receive {
        from: Id,
        to: Id,
        asset: Id,
        amount: u128,
        grant: Id,
        sequence: u64,
        idempotency_key: Id,
        context_hash: Id,
    },
    IssueGrant(Grant),
    RevokeGrant {
        grant: Id,
        revocation_sequence: u64,
    },
    Mint {
        asset: Id,
        to: Id,
        amount: u128,
    },
    Burn {
        asset: Id,
        from: Id,
        amount: u128,
    },
    ProgramTransfer {
        program: Id,
        legs: Vec<TransferLeg>,
    },
    ProgramAccount {
        program: Id,
        asset: Id,
        seed: Vec<u8>,
    },
}

fn bad<T>() -> Result<T, DisclosureError> {
    Err(DisclosureError::MalformedPayload)
}
fn fixed<const N: usize>(d: &mut Decoder<'_>) -> Result<[u8; N], DisclosureError> {
    d.fixed(N)?
        .try_into()
        .map_err(|_| DisclosureError::MalformedPayload)
}
fn bytes<const N: usize>(d: &mut Decoder<'_>) -> Result<[u8; N], DisclosureError> {
    d.bytes(N)?
        .try_into()
        .map_err(|_| DisclosureError::MalformedPayload)
}
fn short(d: &mut Decoder<'_>) -> Result<Vec<u8>, DisclosureError> {
    let n = usize::from(d.u8()?);
    Ok(d.fixed(n)?.to_vec())
}
fn put_short(e: &mut Encoder, b: &[u8]) -> Result<(), DisclosureError> {
    e.u8(u8::try_from(b.len()).map_err(|_| DisclosureError::MalformedPayload)?)?;
    e.fixed(b)?;
    Ok(())
}

fn actor_id(actor: &[u8]) -> Result<Id, DisclosureError> {
    if actor.is_empty() || actor.len() > 255 {
        return bad();
    }
    let length = u16::try_from(actor.len()).map_err(|_| DisclosureError::MalformedPayload)?;
    let mut h = Sha256::new();
    h.update(b"LXP/v1/did-id\0");
    h.update(length.to_be_bytes());
    h.update(actor);
    Ok(h.finalize().into())
}

/// Derives a natively issued asset identifier from issuer identity and salt.
#[must_use]
pub fn asset_id(issuer: &Id, salt: &Id) -> Id {
    let mut h = Sha256::new();
    h.update(b"LX:ASSET:v1");
    h.update(issuer);
    h.update(salt);
    h.finalize().into()
}

impl Payment {
    #[must_use]
    pub const fn activity_type(&self) -> (ModuleId, u16) {
        match self {
            Self::Register(_) => (ModuleId::Asset, 1),
            Self::OpenAccount { .. } => (ModuleId::Asset, 4),
            Self::Receive { .. } => (ModuleId::Asset, 6),
            Self::IssueGrant(_) => (ModuleId::Asset, 7),
            Self::RevokeGrant { .. } => (ModuleId::Asset, 8),
            Self::Mint { .. } => (ModuleId::Asset, 10),
            Self::Burn { .. } => (ModuleId::Asset, 11),
            Self::ProgramTransfer { .. } => (ModuleId::Programs, 5),
            Self::ProgramAccount { .. } => (ModuleId::Programs, 6),
        }
    }

    fn validate(&self, actor: &[u8]) -> Result<(), DisclosureError> {
        match self {
            Self::Register(r) => {
                if r.symbol.is_empty()
                    || r.symbol.len() > 16
                    || !r.symbol.is_ascii()
                    || r.name.is_empty()
                    || r.name.len() > 32
                    || r.decimals > 38
                    || r.custody_ref.len() > 128
                    || !matches!(r.issuer_kind, 1 | 2)
                {
                    return bad();
                }
                if r.issuer_kind == 1 {
                    let issuer = actor_id(actor)?;
                    if !r.custody_ref.is_empty() || r.asset != asset_id(&issuer, &r.salt) {
                        return bad();
                    }
                }
            }
            Self::Receive {
                from, to, amount, ..
            } => {
                if from == to || *amount == 0 {
                    return bad();
                }
            }
            Self::Mint { amount, .. } | Self::Burn { amount, .. } => {
                if *amount == 0 {
                    return bad();
                }
            }
            Self::IssueGrant(g) => {
                if actor_id(actor)? != g.grantor
                    || !(1..=6).contains(&g.kind)
                    || g.not_after == 0
                    || g.not_after <= g.not_before
                    || g.grantee == [0; 32]
                    || g.key == [0; 32]
                    || g.module_mask == 0
                    || g.ordinal_min > g.ordinal_max
                {
                    return bad();
                }
                if matches!(g.kind, 3 | 4)
                    && (g.asset == [0; 32]
                        || g.maximum_per_activity == 0
                        || g.purpose_hash == [0; 32]
                        || g.revocation_sequence == 0
                        || (g.maximum_total == 0
                            && (g.period_length == 0 || g.maximum_per_period == 0)))
                {
                    return bad();
                }
            }
            Self::ProgramTransfer { legs, .. } => {
                if legs.is_empty()
                    || legs.len() > 256
                    || legs.iter().any(|l| l.amount == 0 || l.from == l.to)
                {
                    return bad();
                }
            }
            Self::ProgramAccount {
                program,
                asset,
                seed,
            } => {
                if *program == [0; 32] || *asset == [0; 32] || seed.len() > 128 {
                    return bad();
                }
            }
            Self::OpenAccount { .. } | Self::RevokeGrant { .. } => {}
        }
        Ok(())
    }

    /// # Errors
    /// Rejects malformed fields or an issuer inconsistent with the actor.
    pub fn encode(&self, actor: &[u8]) -> Result<Vec<u8>, DisclosureError> {
        self.validate(actor)?;
        let mut e = Encoder::new(32768);
        match self {
            Self::Register(r) => {
                e.u16(1)?;
                e.fixed(&r.asset)?;
                e.fixed(&r.salt)?;
                put_short(&mut e, r.symbol.as_bytes())?;
                put_short(&mut e, r.name.as_bytes())?;
                e.u8(r.decimals)?;
                e.u128(r.supply_cap)?;
                e.u8(r.issuer_kind)?;
                put_short(&mut e, &r.custody_ref)?;
            }
            Self::OpenAccount { asset } => {
                e.u16(1)?;
                e.fixed(asset)?;
            }
            Self::Mint {
                asset,
                to: account,
                amount,
            }
            | Self::Burn {
                asset,
                from: account,
                amount,
            } => {
                e.u16(1)?;
                e.fixed(asset)?;
                e.fixed(account)?;
                e.u128(*amount)?;
            }
            Self::RevokeGrant {
                grant,
                revocation_sequence,
            } => {
                e.u16(1)?;
                e.fixed(grant)?;
                e.u64(*revocation_sequence)?;
            }
            Self::Receive {
                from,
                to,
                asset,
                amount,
                grant,
                sequence,
                idempotency_key,
                context_hash,
            } => {
                e.u16(0x5201)?;
                e.u16(8)?;
                e.fixed(from)?;
                e.fixed(to)?;
                e.fixed(asset)?;
                e.u128(*amount)?;
                e.fixed(grant)?;
                e.u64(*sequence)?;
                e.fixed(idempotency_key)?;
                e.fixed(context_hash)?;
            }
            Self::IssueGrant(g) => encode_grant(&mut e, g)?,
            Self::ProgramTransfer { program, legs } => {
                e.fixed(program)?;
                e.u16(u16::try_from(legs.len()).map_err(|_| DisclosureError::MalformedPayload)?)?;
                for l in legs {
                    e.fixed(&l.from)?;
                    e.fixed(&l.asset)?;
                    e.fixed(&l.to)?;
                    e.u128(l.amount)?;
                }
            }
            Self::ProgramAccount {
                program,
                asset,
                seed,
            } => {
                e.fixed(program)?;
                e.fixed(b"LXPA1")?;
                e.fixed(asset)?;
                e.bytes(seed, 128)?;
            }
        }
        Ok(e.finish())
    }

    /// # Errors
    /// Refuses unknown ordinals, noncanonical bytes, invalid fields and trailing data.
    pub fn decode(
        module: ModuleId,
        ordinal: u16,
        payload: &[u8],
        actor: &[u8],
    ) -> Result<Self, DisclosureError> {
        let mut d = Decoder::new(payload, 0);
        if module == ModuleId::Asset && matches!(ordinal, 1 | 4 | 8 | 10 | 11) && d.u16()? != 1 {
            return bad();
        }
        let result = match (module, ordinal) {
            (ModuleId::Asset, 1) => Self::Register(Registration {
                asset: fixed(&mut d)?,
                salt: fixed(&mut d)?,
                symbol: String::from_utf8(short(&mut d)?)
                    .map_err(|_| DisclosureError::MalformedPayload)?,
                name: String::from_utf8(short(&mut d)?)
                    .map_err(|_| DisclosureError::MalformedPayload)?,
                decimals: d.u8()?,
                supply_cap: d.u128()?,
                issuer_kind: d.u8()?,
                custody_ref: short(&mut d)?,
            }),
            (ModuleId::Asset, 4) => Self::OpenAccount {
                asset: fixed(&mut d)?,
            },
            (ModuleId::Asset, 8) => Self::RevokeGrant {
                grant: fixed(&mut d)?,
                revocation_sequence: d.u64()?,
            },
            (ModuleId::Asset, 10) => Self::Mint {
                asset: fixed(&mut d)?,
                to: fixed(&mut d)?,
                amount: d.u128()?,
            },
            (ModuleId::Asset, 11) => Self::Burn {
                asset: fixed(&mut d)?,
                from: fixed(&mut d)?,
                amount: d.u128()?,
            },
            (ModuleId::Asset, 6) => {
                if d.u16()? != 0x5201 || d.u16()? != 8 {
                    return bad();
                }
                Self::Receive {
                    from: fixed(&mut d)?,
                    to: fixed(&mut d)?,
                    asset: fixed(&mut d)?,
                    amount: d.u128()?,
                    grant: fixed(&mut d)?,
                    sequence: d.u64()?,
                    idempotency_key: fixed(&mut d)?,
                    context_hash: fixed(&mut d)?,
                }
            }
            (ModuleId::Asset, 7) => Self::IssueGrant(decode_grant(&mut d)?),
            (ModuleId::Programs, 5) => {
                let program = fixed(&mut d)?;
                let n = usize::from(d.u16()?);
                if n == 0 || n > 256 {
                    return bad();
                }
                let mut legs = Vec::with_capacity(n);
                for _ in 0..n {
                    legs.push(TransferLeg {
                        from: fixed(&mut d)?,
                        asset: fixed(&mut d)?,
                        to: fixed(&mut d)?,
                        amount: d.u128()?,
                    });
                }
                Self::ProgramTransfer { program, legs }
            }
            (ModuleId::Programs, 6) => {
                let program = fixed(&mut d)?;
                if d.fixed(5)? != b"LXPA1" {
                    return bad();
                }
                Self::ProgramAccount {
                    program,
                    asset: fixed(&mut d)?,
                    seed: d.bytes(128)?.to_vec(),
                }
            }
            _ => {
                return Err(DisclosureError::UnsupportedActivity(
                    (u32::from(module as u16) << 16) | u32::from(ordinal),
                ))
            }
        };
        d.finish()?;
        result.validate(actor)?;
        if result.encode(actor)? != payload {
            return bad();
        }
        Ok(result)
    }
}

fn encode_grant(e: &mut Encoder, g: &Grant) -> Result<(), DisclosureError> {
    e.structure_header(0x2001)?;
    e.u8(1)?;
    e.bytes(&g.grantor, 32)?;
    e.bytes(&g.grantee, 32)?;
    e.u8(g.kind)?;
    e.bytes(&g.key, 32)?;
    e.u64(g.module_mask)?;
    e.u16(g.ordinal_min)?;
    e.u16(g.ordinal_max)?;
    e.bytes(&g.asset, 32)?;
    e.u128(g.maximum_per_activity)?;
    e.u128(g.maximum_total)?;
    e.u128(g.spent_total)?;
    e.u64(g.period_length)?;
    e.u128(g.maximum_per_period)?;
    e.u128(g.spent_this_period)?;
    e.u64(g.period_start)?;
    e.bytes(&g.purpose_hash, 32)?;
    e.u64(g.not_before)?;
    e.u64(g.not_after)?;
    e.u64(g.revocation_sequence)?;
    e.u8(u8::from(g.revoked))?;
    e.u64(g.revoked_at_sequence)?;
    e.bytes(&g.signature, 64)?;
    Ok(())
}

fn decode_grant(d: &mut Decoder<'_>) -> Result<Grant, DisclosureError> {
    d.structure_header(0x2001)?;
    if d.u8()? != 1 {
        return bad();
    }
    let grantor = bytes(d)?;
    let grantee = bytes(d)?;
    let kind = d.u8()?;
    let key = bytes(d)?;
    let module_mask = d.u64()?;
    let ordinal_min = d.u16()?;
    let ordinal_max = d.u16()?;
    let asset = bytes(d)?;
    let maximum_per_activity = d.u128()?;
    let maximum_total = d.u128()?;
    let spent_total = d.u128()?;
    let period_length = d.u64()?;
    let maximum_per_period = d.u128()?;
    let spent_this_period = d.u128()?;
    let period_start = d.u64()?;
    let purpose_hash = bytes(d)?;
    let not_before = d.u64()?;
    let not_after = d.u64()?;
    let revocation_sequence = d.u64()?;
    let revoked = match d.u8()? {
        0 => false,
        1 => true,
        _ => return bad(),
    };
    Ok(Grant {
        grantor,
        grantee,
        kind,
        key,
        module_mask,
        ordinal_min,
        ordinal_max,
        asset,
        maximum_per_activity,
        maximum_total,
        spent_total,
        period_length,
        maximum_per_period,
        spent_this_period,
        period_start,
        purpose_hash,
        not_before,
        not_after,
        revocation_sequence,
        revoked,
        revoked_at_sequence: d.u64()?,
        signature: bytes(d)?,
    })
}
