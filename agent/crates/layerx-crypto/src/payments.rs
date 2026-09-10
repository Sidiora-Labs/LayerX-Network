//! Canonical payment and Programs payload codecs used by signer disclosure.
//!
//! Integers are big-endian. Native asset identifiers are
//! `SHA-256("LX:ASSET:v1" || issuer_did_id32 || salt32)`, where
//! `issuer_did_id32` is the existing `lxp_did_id_derive` identity
//! (`SHA-256("LXP/v1/did-id\0" || u16be(len) || did)`). Asset ordinal 9
//! (WITHDRAW) is refused. Receive uses the native 733-byte, ten-field
//! encoding; grant issue uses the 346-byte payer-grant concatenation.
//! Authority capabilities retain their separate codec. See [`crate::disclosure`].

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Grant {
    pub id: Id,
    pub from: Id,
    pub recipient: Id,
    pub asset: Id,
    pub per_draw_maximum: u128,
    pub allowance: u128,
    pub recurring: bool,
    pub window_length: u64,
    pub expiration: u64,
    pub purpose_hash: Id,
    pub has_reference: bool,
    pub reference_hash: Id,
    pub revocation_sequence: u64,
    pub public_key: Id,
    pub signature: [u8; 64],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiverAuthorization {
    pub kind: u8,
    pub controller: Id,
    pub public_key: Id,
    pub signature: [u8; 64],
    pub signed_context_hash: Id,
    pub network_id: u32,
    pub protocol_version: u16,
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
    Pause {
        asset: Id,
    },
    Unpause {
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
        receiver_authorization: ReceiverAuthorization,
        payer_grant: Box<Grant>,
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
            Self::Pause { .. } => (ModuleId::Asset, 2),
            Self::Unpause { .. } => (ModuleId::Asset, 3),
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
            Self::Receive { .. } => self.verify_receive()?,
            Self::Mint { amount, .. } | Self::Burn { amount, .. } => {
                if *amount == 0 {
                    return bad();
                }
            }
            Self::IssueGrant(g) => g.verify()?,
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
            Self::Pause { asset } | Self::Unpause { asset } => {
                if *asset == [0; 32] {
                    return bad();
                }
            }
            Self::OpenAccount { .. } | Self::RevokeGrant { .. } => {}
        }
        Ok(())
    }

    fn verify_receive(&self) -> Result<(), DisclosureError> {
        let Self::Receive {
            from,
            to,
            asset,
            amount,
            grant,
            sequence,
            idempotency_key,
            context_hash,
            receiver_authorization: auth,
            payer_grant: g,
        } = self
        else {
            return bad();
        };

        g.verify()?;
        if from == to
            || *amount == 0
            || *amount > g.per_draw_maximum
            || *from != g.from
            || *to != g.recipient
            || *asset != g.asset
            || *grant != g.id
            || !(1..=6).contains(&auth.kind)
            || auth.controller != *to
            || auth.signed_context_hash != *context_hash
            || auth.network_id == 0
            || !layerx_wire::limits::protocol_version_supported(auth.protocol_version)
        {
            return bad();
        }
        let mut h = Sha256::new();
        h.update(layerx_wire::hash::Domain::ContextHash.tag());
        h.update(g.purpose_hash);
        if g.has_reference {
            h.update(g.reference_hash);
        }
        let expected: Id = h.finalize().into();
        if expected != *context_hash {
            return bad();
        }
        let mut e = Encoder::new(512);
        e.fixed(b"LXP:RECEIVE:v1")?;
        e.fixed(from)?;
        e.fixed(to)?;
        e.fixed(asset)?;
        e.u128(*amount)?;
        e.fixed(grant)?;
        e.u64(*sequence)?;
        e.fixed(idempotency_key)?;
        e.fixed(context_hash)?;
        e.u8(auth.kind)?;
        e.fixed(&auth.controller)?;
        e.fixed(&auth.signed_context_hash)?;
        e.u32(auth.network_id)?;
        e.u16(auth.protocol_version)?;
        let mut h = Sha256::new();
        h.update(layerx_wire::hash::Domain::SignaturePreimage.tag());
        h.update(e.finish());
        crate::ed25519::verify_digest(&auth.public_key, &auth.signature, &h.finalize().into())
            .map_err(|_| DisclosureError::MalformedPayload)?;
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
            Self::OpenAccount { asset } | Self::Pause { asset } | Self::Unpause { asset } => {
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
                receiver_authorization: auth,
                payer_grant,
            } => {
                e.u16(0x5201)?;
                e.u16(10)?;
                e.fixed(from)?;
                e.fixed(to)?;
                e.fixed(asset)?;
                e.u128(*amount)?;
                e.fixed(grant)?;
                e.u64(*sequence)?;
                e.fixed(idempotency_key)?;
                e.fixed(context_hash)?;
                e.u8(auth.kind)?;
                e.fixed(&auth.controller)?;
                e.fixed(&auth.public_key)?;
                e.fixed(&auth.signature)?;
                e.fixed(&auth.signed_context_hash)?;
                e.u32(auth.network_id)?;
                e.u16(auth.protocol_version)?;
                encode_grant(&mut e, payer_grant)?;
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
        if module == ModuleId::Asset
            && matches!(ordinal, 1 | 2 | 3 | 4 | 8 | 10 | 11)
            && d.u16()? != 1
        {
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
            (ModuleId::Asset, 2) => Self::Pause {
                asset: fixed(&mut d)?,
            },
            (ModuleId::Asset, 3) => Self::Unpause {
                asset: fixed(&mut d)?,
            },
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
                if d.u16()? != 0x5201 || d.u16()? != 10 {
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
                    receiver_authorization: ReceiverAuthorization {
                        kind: d.u8()?,
                        controller: fixed(&mut d)?,
                        public_key: fixed(&mut d)?,
                        signature: fixed(&mut d)?,
                        signed_context_hash: fixed(&mut d)?,
                        network_id: d.u32()?,
                        protocol_version: d.u16()?,
                    },
                    payer_grant: Box::new(decode_grant(&mut d)?),
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
    e.fixed(&g.id)?;
    e.fixed(&g.from)?;
    e.fixed(&g.recipient)?;
    e.fixed(&g.asset)?;
    e.u128(g.per_draw_maximum)?;
    e.u128(g.allowance)?;
    e.u8(u8::from(g.recurring))?;
    e.u64(g.window_length)?;
    e.u64(g.expiration)?;
    e.fixed(&g.purpose_hash)?;
    e.u8(u8::from(g.has_reference))?;
    e.fixed(&g.reference_hash)?;
    e.u64(g.revocation_sequence)?;
    e.fixed(&g.public_key)?;
    e.fixed(&g.signature)?;
    Ok(())
}

fn boolean(d: &mut Decoder<'_>) -> Result<bool, DisclosureError> {
    match d.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => bad(),
    }
}

fn decode_grant(d: &mut Decoder<'_>) -> Result<Grant, DisclosureError> {
    Ok(Grant {
        id: fixed(d)?,
        from: fixed(d)?,
        recipient: fixed(d)?,
        asset: fixed(d)?,
        per_draw_maximum: d.u128()?,
        allowance: d.u128()?,
        recurring: boolean(d)?,
        window_length: d.u64()?,
        expiration: d.u64()?,
        purpose_hash: fixed(d)?,
        has_reference: boolean(d)?,
        reference_hash: fixed(d)?,
        revocation_sequence: d.u64()?,
        public_key: fixed(d)?,
        signature: fixed(d)?,
    })
}

impl Grant {
    fn verify(&self) -> Result<(), DisclosureError> {
        if self.per_draw_maximum == 0
            || self.allowance == 0
            || self.expiration == 0
            || self.recurring != (self.window_length != 0)
            || self.recipient == [0; 32]
            || self.asset == [0; 32]
            || self.purpose_hash == [0; 32]
        {
            return bad();
        }
        let mut e = Encoder::new(346);
        encode_grant(&mut e, self)?;
        let bytes = e.finish();
        let mut h = Sha256::new();
        h.update(layerx_wire::hash::Domain::AuthorityHash.tag());
        h.update(b"LXP:GRANT:v1");
        h.update(&bytes[32..282]);
        let digest: Id = h.finalize().into();
        if digest != self.id {
            return bad();
        }
        crate::ed25519::verify_digest(&self.public_key, &self.signature, &digest)
            .map_err(|_| DisclosureError::MalformedPayload)
    }
}
