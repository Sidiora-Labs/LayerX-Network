//! Exact protocol session-key grant issuance.

use std::collections::BTreeSet;
use std::fmt;

use crate::authority_grant::NativeFeeBudget;
use layerx_types::activity::Authority;
use layerx_types::payload::ActivityType;
use layerx_wire::decode::Decoder;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::Domain;
use sha2::{Digest as _, Sha256};

use crate::ct;

const GRANT_WIRE_TAG: u16 = 0x2001;
const SESSION_KEY_AUTHORITY: u8 = 2;
const MAX_GRANT_BYTES: usize = 1024;
const MAX_SESSION_ACTIVITY_TYPES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionPurpose {
    Activity,
    Authentication,
}

/// Explicit operator request for one protocol-enforced session authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionKeyRequest {
    /// Protocol identity delegating authority.
    pub grantor: [u8; 32],
    /// Public key that will exercise the delegated authority.
    pub session_public_key: [u8; 32],
    /// Inclusive lower validity bound.
    pub not_before: u64,
    /// Required inclusive upper validity bound.
    pub expires_at: Option<u64>,
    /// Exact activity types the session may submit.
    pub permitted_activity_types: Vec<ActivityType>,
    /// Required identity revocation sequence captured in protocol state.
    pub revocation_sequence: Option<u64>,
    pub fee_budget: Option<NativeFeeBudget>,
    pub purpose: SessionPurpose,
}

/// Protocol bytes and authority representation produced by issuance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuedSessionKey {
    pub grantor: [u8; 32],
    pub not_before: u64,
    /// Canonical `lxp_authority_grant` bytes for an ordinary registration activity.
    pub registration_payload: Vec<u8>,
    /// Exact protocol session-key authority representation, not a local record.
    pub authority: Authority,
    /// Public key bound into the protocol session authority.
    pub session_public_key: [u8; 32],
    /// Core-compatible authority-hash identifier of the grant payload.
    pub grant_id: [u8; 32],
    /// Exact activity set represented by the protocol scope.
    pub permitted_activity_types: Vec<ActivityType>,
    /// Required protocol expiry.
    pub expires_at: u64,
    /// Required protocol revocation sequence.
    pub revocation_sequence: u64,
    pub fee_budget: Option<NativeFeeBudget>,
    pub purpose: SessionPurpose,
}

/// Typed refusal for an unsafe or unrepresentable session-key request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionIssueError {
    /// No expiry was supplied.
    MissingExpiry,
    /// No permitted activity type was supplied.
    EmptyActivitySet,
    /// No positive revocation sequence was supplied.
    MissingRevocationSequence,
    /// Expiry does not follow the lower validity bound.
    InvalidExpiry,
    /// Grantor or session public key is the all-zero invalid value.
    InvalidIdentityOrKey,
    /// The exact set would be widened by core's module/range representation.
    NonRepresentableActivitySet,
    /// Canonical protocol encoding failed.
    Encoding,
}

impl fmt::Display for SessionIssueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingExpiry => "session key expiry is required",
            Self::EmptyActivitySet => "session key permitted activity set is required",
            Self::MissingRevocationSequence => {
                "session key revocation sequence is required and must be positive"
            }
            Self::InvalidExpiry => "session key expiry must follow not_before",
            Self::InvalidIdentityOrKey => "session key grantor and public key must be non-zero",
            Self::NonRepresentableActivitySet => {
                "session activity set cannot be represented by protocol scope without widening"
            }
            Self::Encoding => "session key protocol grant could not be encoded",
        })
    }
}

impl std::error::Error for SessionIssueError {}

fn exact_scope(
    activity_types: &[ActivityType],
) -> Result<(u64, u16, u16, Vec<ActivityType>), SessionIssueError> {
    if activity_types.len() > MAX_SESSION_ACTIVITY_TYPES {
        return Err(SessionIssueError::NonRepresentableActivitySet);
    }
    if activity_types.is_empty() {
        return Err(SessionIssueError::EmptyActivitySet);
    }
    let unique: BTreeSet<_> = activity_types.iter().copied().collect();
    if unique.len() != activity_types.len() {
        return Err(SessionIssueError::NonRepresentableActivitySet);
    }
    let modules: BTreeSet<_> = unique.iter().map(|kind| kind.module() as u16).collect();
    let minimum = unique
        .iter()
        .map(|kind| kind.ordinal())
        .min()
        .ok_or(SessionIssueError::EmptyActivitySet)?;
    let maximum = unique
        .iter()
        .map(|kind| kind.ordinal())
        .max()
        .ok_or(SessionIssueError::EmptyActivitySet)?;
    let range_length = usize::from(maximum - minimum) + 1;
    let expected_length = modules
        .len()
        .checked_mul(range_length)
        .ok_or(SessionIssueError::NonRepresentableActivitySet)?;
    if unique.len() != expected_length
        || modules.iter().any(|module| {
            (minimum..=maximum).any(|ordinal| {
                !unique
                    .iter()
                    .any(|kind| kind.module() as u16 == *module && kind.ordinal() == ordinal)
            })
        })
    {
        return Err(SessionIssueError::NonRepresentableActivitySet);
    }
    let mut module_mask = 0_u64;
    for module in modules {
        module_mask |= 1_u64 << module;
    }
    Ok((module_mask, minimum, maximum, unique.into_iter().collect()))
}

fn encode_grant(
    request: &SessionKeyRequest,
    module_mask: u64,
    ordinal_min: u16,
    ordinal_max: u16,
    expires_at: u64,
    revocation_sequence: u64,
) -> Result<Vec<u8>, SessionIssueError> {
    let mut encoder = Encoder::new(MAX_GRANT_BYTES);
    macro_rules! write {
        ($expression:expr) => {
            $expression.map_err(|_| SessionIssueError::Encoding)?
        };
    }
    write!(encoder.structure_header(GRANT_WIRE_TAG));
    write!(encoder.u8(match request.purpose {
        SessionPurpose::Authentication => 3,
        SessionPurpose::Activity =>
            if request.fee_budget.is_some() {
                2
            } else {
                1
            },
    }));
    write!(encoder.bytes(&request.grantor, 32));
    write!(encoder.bytes(&request.grantor, 32));
    write!(encoder.u8(SESSION_KEY_AUTHORITY));
    write!(encoder.bytes(&request.session_public_key, 32));
    write!(encoder.u64(module_mask));
    write!(encoder.u16(ordinal_min));
    write!(encoder.u16(ordinal_max));
    write!(encoder.bytes(&[0_u8; 32], 32));
    write!(encoder.u128(0));
    write!(encoder.u128(0));
    write!(encoder.u128(0));
    write!(encoder.u64(0));
    write!(encoder.u128(0));
    write!(encoder.u128(0));
    write!(encoder.u64(0));
    write!(encoder.bytes(&[0_u8; 32], 32));
    write!(encoder.u64(request.not_before));
    write!(encoder.u64(expires_at));
    write!(encoder.u64(revocation_sequence));
    write!(encoder.u8(0));
    write!(encoder.u64(0));
    write!(encoder.bytes(&[0_u8; 64], 64));
    if let Some(fee) = request.fee_budget {
        fee.validate(request.not_before)
            .map_err(|_| SessionIssueError::Encoding)?;
        fee.encode(&mut encoder)
            .map_err(|_| SessionIssueError::Encoding)?;
    }
    if request.purpose == SessionPurpose::Authentication {
        write!(encoder.u8(1));
    }
    Ok(encoder.finish())
}

/// Issues a bounded authority as exact protocol grant bytes.
///
/// # Errors
///
/// Refuses every missing bound, zero identity/key, duplicate or unrepresentable
/// scope, and any canonical encoding failure.
pub fn issue_session_key(
    request: &SessionKeyRequest,
) -> Result<IssuedSessionKey, SessionIssueError> {
    let expires_at = request.expires_at.ok_or(SessionIssueError::MissingExpiry)?;
    if expires_at == 0 || expires_at <= request.not_before {
        return Err(SessionIssueError::InvalidExpiry);
    }
    let revocation_sequence = request
        .revocation_sequence
        .filter(|value| *value > 0)
        .ok_or(SessionIssueError::MissingRevocationSequence)?;
    if ct::eq_fixed(&request.grantor, &[0_u8; 32])
        || ct::eq_fixed(&request.session_public_key, &[0_u8; 32])
    {
        return Err(SessionIssueError::InvalidIdentityOrKey);
    }
    let (module_mask, ordinal_min, ordinal_max, permitted_activity_types) = match request.purpose {
        SessionPurpose::Activity => exact_scope(&request.permitted_activity_types)?,
        SessionPurpose::Authentication => {
            if !request.permitted_activity_types.is_empty() || request.fee_budget.is_some() {
                return Err(SessionIssueError::NonRepresentableActivitySet);
            }
            (0, 0, 0, Vec::new())
        }
    };
    let registration_payload = encode_grant(
        request,
        module_mask,
        ordinal_min,
        ordinal_max,
        expires_at,
        revocation_sequence,
    )?;
    let authority =
        Authority::session_key(&registration_payload).map_err(|_| SessionIssueError::Encoding)?;
    let mut hasher = Sha256::new();
    hasher.update(Domain::AuthorityHash.tag());
    hasher.update(&registration_payload);
    let grant_id = hasher.finalize().into();
    Ok(IssuedSessionKey {
        grantor: request.grantor,
        not_before: request.not_before,
        registration_payload,
        authority,
        session_public_key: request.session_public_key,
        grant_id,
        permitted_activity_types,
        expires_at,
        revocation_sequence,
        fee_budget: request.fee_budget,
        purpose: request.purpose,
    })
}

fn session_activities(
    module_mask: u64,
    minimum: u16,
    maximum: u16,
) -> Result<Vec<ActivityType>, SessionIssueError> {
    use layerx_types::payload::ModuleId;
    let invalid = || SessionIssueError::Encoding;
    let activity_count = usize::try_from(module_mask.count_ones())
        .map_err(|_| invalid())?
        .checked_mul(usize::from(maximum - minimum) + 1)
        .ok_or_else(invalid)?;
    if activity_count > MAX_SESSION_ACTIVITY_TYPES {
        return Err(invalid());
    }
    let mut permitted_activity_types = Vec::with_capacity(activity_count);
    for module in 1_u16..10 {
        if module_mask & (1_u64 << module) == 0 {
            continue;
        }
        let module = ModuleId::from_u16(module).map_err(|_| invalid())?;
        for ordinal in minimum..=maximum {
            permitted_activity_types
                .push(ActivityType::new(module, ordinal).map_err(|_| invalid())?);
        }
    }
    Ok(permitted_activity_types)
}

/// # Errors
/// Refuses any noncanonical session scope, unsupported version, or initial charge.
pub fn decode_session_key(bytes: &[u8]) -> Result<IssuedSessionKey, SessionIssueError> {
    let invalid = || SessionIssueError::Encoding;
    if bytes.len() > MAX_GRANT_BYTES {
        return Err(invalid());
    }
    let mut decoder = Decoder::new(bytes, 0);
    decoder
        .structure_header(GRANT_WIRE_TAG)
        .map_err(|_| invalid())?;
    let version = decoder.u8().map_err(|_| invalid())?;
    if !matches!(version, 1..=3) {
        return Err(invalid());
    }
    let grantor: [u8; 32] = decoder
        .bytes(32)
        .map_err(|_| invalid())?
        .try_into()
        .map_err(|_| invalid())?;
    if decoder.bytes(32).map_err(|_| invalid())? != grantor
        || decoder.u8().map_err(|_| invalid())? != SESSION_KEY_AUTHORITY
    {
        return Err(invalid());
    }
    let session_public_key = decoder
        .bytes(32)
        .map_err(|_| invalid())?
        .try_into()
        .map_err(|_| invalid())?;
    let module_mask = decoder.u64().map_err(|_| invalid())?;
    let minimum = decoder.u16().map_err(|_| invalid())?;
    let maximum = decoder.u16().map_err(|_| invalid())?;
    let purpose = if version == 3 {
        SessionPurpose::Authentication
    } else {
        SessionPurpose::Activity
    };
    if (purpose == SessionPurpose::Activity
        && (minimum == 0 || maximum < minimum || module_mask == 0))
        || (purpose == SessionPurpose::Authentication
            && (minimum != 0 || maximum != 0 || module_mask != 0))
        || module_mask & !0x03fe != 0
        || decoder.bytes(32).map_err(|_| invalid())? != [0; 32]
        || decoder.u128().map_err(|_| invalid())? != 0
        || decoder.u128().map_err(|_| invalid())? != 0
        || decoder.u128().map_err(|_| invalid())? != 0
        || decoder.u64().map_err(|_| invalid())? != 0
        || decoder.u128().map_err(|_| invalid())? != 0
        || decoder.u128().map_err(|_| invalid())? != 0
        || decoder.u64().map_err(|_| invalid())? != 0
        || decoder.bytes(32).map_err(|_| invalid())? != [0; 32]
    {
        return Err(invalid());
    }
    let not_before = decoder.u64().map_err(|_| invalid())?;
    let expires_at = decoder.u64().map_err(|_| invalid())?;
    let revocation_sequence = decoder.u64().map_err(|_| invalid())?;
    if decoder.u8().map_err(|_| invalid())? != 0
        || decoder.u64().map_err(|_| invalid())? != 0
        || decoder.bytes(64).map_err(|_| invalid())? != [0; 64]
    {
        return Err(invalid());
    }
    let fee_budget = if version == 2 {
        Some(NativeFeeBudget::decode(&mut decoder).map_err(|_| invalid())?)
    } else {
        None
    };
    if purpose == SessionPurpose::Authentication && decoder.u8().map_err(|_| invalid())? != 1 {
        return Err(invalid());
    }
    decoder.finish().map_err(|_| invalid())?;
    let permitted_activity_types = session_activities(module_mask, minimum, maximum)?;
    let issued = issue_session_key(&SessionKeyRequest {
        grantor,
        session_public_key,
        not_before,
        expires_at: Some(expires_at),
        revocation_sequence: Some(revocation_sequence),
        permitted_activity_types,
        fee_budget,
        purpose,
    })?;
    if issued.registration_payload != bytes {
        return Err(invalid());
    }
    Ok(issued)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionFeeState {
    pub grant: IssuedSessionKey,
    pub revoked_at_sequence: u64,
    pub spent_total: u128,
    pub spent_this_period: u128,
    pub period_start: u64,
    pub successor: [u8; 32],
    pub charge_commitment: [u8; 32],
}

impl SessionFeeState {
    /// # Errors
    /// Refuses malformed, mismatched or inconsistent committed session state.
    pub fn decode(expected_grant_id: [u8; 32], bytes: &[u8]) -> Result<Self, SessionIssueError> {
        fn take<const N: usize>(bytes: &mut &[u8]) -> Result<[u8; N], SessionIssueError> {
            let (head, tail) = bytes
                .split_at_checked(N)
                .ok_or(SessionIssueError::Encoding)?;
            *bytes = tail;
            head.try_into().map_err(|_| SessionIssueError::Encoding)
        }
        let mut remaining = bytes;
        let length = usize::from(u16::from_be_bytes(take(&mut remaining)?));
        let (body, tail) = remaining
            .split_at_checked(length)
            .ok_or(SessionIssueError::Encoding)?;
        remaining = tail;
        let grant = decode_session_key(body)?;
        if grant.grant_id != expected_grant_id || grant.purpose != SessionPurpose::Activity {
            return Err(SessionIssueError::Encoding);
        }
        let revoked_at_sequence = u64::from_be_bytes(take(&mut remaining)?);
        let counters: [u8; 72] = take(&mut remaining)?;
        let successor = take(&mut remaining)?;
        let charge_commitment = take(&mut remaining)?;
        if !remaining.is_empty() {
            return Err(SessionIssueError::Encoding);
        }
        let mut charged = counters.as_slice();
        let counter_id: [u8; 32] = take(&mut charged)?;
        let spent_total = u128::from_be_bytes(take(&mut charged)?);
        let spent_this_period = u128::from_be_bytes(take(&mut charged)?);
        let period_start = u64::from_be_bytes(take(&mut charged)?);
        if let Some(fee) = grant.fee_budget {
            if counter_id != expected_grant_id
                || spent_total > fee.maximum_total
                || spent_this_period > spent_total
                || (fee.period_length == 0 && period_start != 0)
                || (fee.period_length != 0
                    && (period_start < grant.not_before
                        || (period_start - grant.not_before) % fee.period_length != 0
                        || spent_this_period > fee.maximum_per_period))
            {
                return Err(SessionIssueError::Encoding);
            }
            if revoked_at_sequence != 0 {
                let mut digest = Sha256::new();
                digest.update(b"LXP/session-fee-replacement/v1\0");
                digest.update(expected_grant_id);
                digest.update(revoked_at_sequence.to_be_bytes());
                digest.update(counters);
                let expected: [u8; 32] = digest.finalize().into();
                if expected != charge_commitment {
                    return Err(SessionIssueError::Encoding);
                }
            } else if charge_commitment != [0; 32] || successor != [0; 32] {
                return Err(SessionIssueError::Encoding);
            }
        } else if counters != [0; 72] || charge_commitment != [0; 32] || successor != [0; 32] {
            return Err(SessionIssueError::Encoding);
        }
        Ok(Self {
            grant,
            revoked_at_sequence,
            spent_total,
            spent_this_period,
            period_start,
            successor,
            charge_commitment,
        })
    }
}
