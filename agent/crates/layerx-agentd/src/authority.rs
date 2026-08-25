//! Core-resolved authority checks performed at every write boundary.

use std::collections::{BTreeMap, BTreeSet};

use crate::identity::ProtocolAuthority;

/// Current protocol authority state returned by the node boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityState {
    pub authority: ProtocolAuthority,
    pub generation: u64,
    pub valid_through_sequence: u64,
    pub revoked: bool,
    pub permitted_activity_types: BTreeSet<u16>,
    pub observed_sequence: u64,
}

/// Core boundary seam used for authority refreshes.
pub trait AuthorityResolver {
    /// Reads current core authority state, or `None` when core holds no such authority.
    ///
    /// # Errors
    ///
    /// Returns `BoundaryUnavailable` when the node boundary cannot be consulted.
    fn authority_state(
        &mut self,
        authority: &ProtocolAuthority,
    ) -> Result<Option<AuthorityState>, AuthorityError>;
}

/// Bounded cache keyed by protocol authority.
#[derive(Default)]
pub struct AuthorityCache {
    entries: BTreeMap<Vec<u8>, AuthorityState>,
}

/// Successful check returned with the exact protocol authority used.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedAuthority {
    pub authority: ProtocolAuthority,
    pub generation: u64,
    pub observed_sequence: u64,
}

/// Stable refusal reasons for audit and caller responses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorityError {
    BoundaryUnavailable,
    Missing,
    SequenceRegression { current: u64, observed: u64 },
    Expired,
    Revoked,
    Rotated,
    Scope,
}

/// Resolves an authority, consulting a cache only inside an explicit sequence bound.
///
/// # Errors
///
/// Returns `Missing` when the boundary holds no state for the authority, and propagates the
/// resolver's own refusal otherwise.
pub fn resolve(
    cache: &mut AuthorityCache,
    resolver: &mut dyn AuthorityResolver,
    authority: &ProtocolAuthority,
    core_sequence: u64,
    maximum_cache_age: u64,
) -> Result<AuthorityState, AuthorityError> {
    let key = authority_key(authority);
    if let Some(cached) = cache.entries.get(&key).cloned() {
        if cached.observed_sequence > core_sequence {
            cache.entries.remove(&key);
            return Err(AuthorityError::SequenceRegression {
                current: core_sequence,
                observed: cached.observed_sequence,
            });
        }
        if core_sequence - cached.observed_sequence <= maximum_cache_age {
            return Ok(cached);
        }
    }
    let current = resolver
        .authority_state(authority)?
        .ok_or(AuthorityError::Missing)?;
    if current.observed_sequence > core_sequence {
        cache.entries.remove(&key);
        return Err(AuthorityError::SequenceRegression {
            current: core_sequence,
            observed: current.observed_sequence,
        });
    }
    cache.entries.insert(key, current.clone());
    Ok(current)
}

/// Requires current, unrevoked, unrotated protocol scope for one write.
///
/// # Errors
///
/// Returns `Rotated` when the authority or generation differs from the opened one, `Revoked` for
/// a revoked authority, `Expired` past its valid-through sequence, and `Scope` when the activity
/// type is not permitted.
pub fn require_valid(
    expected: &ProtocolAuthority,
    opened_generation: u64,
    activity_type: u16,
    core_sequence: u64,
    state: &AuthorityState,
) -> Result<ResolvedAuthority, AuthorityError> {
    if &state.authority != expected || state.generation != opened_generation {
        return Err(AuthorityError::Rotated);
    }
    if state.revoked {
        return Err(AuthorityError::Revoked);
    }
    if core_sequence > state.valid_through_sequence {
        return Err(AuthorityError::Expired);
    }
    if !state.permitted_activity_types.contains(&activity_type) {
        return Err(AuthorityError::Scope);
    }
    Ok(ResolvedAuthority {
        authority: state.authority.clone(),
        generation: state.generation,
        observed_sequence: state.observed_sequence,
    })
}

fn authority_key(authority: &ProtocolAuthority) -> Vec<u8> {
    let (tag, identifier) = match authority {
        ProtocolAuthority::PrimaryKey(identifier) => (1_u8, identifier),
        ProtocolAuthority::SessionKey(identifier) => (2_u8, identifier),
        ProtocolAuthority::CapabilityGrant(identifier) => (3_u8, identifier),
    };
    let mut key = Vec::with_capacity(33);
    key.push(tag);
    key.extend_from_slice(identifier);
    key
}
