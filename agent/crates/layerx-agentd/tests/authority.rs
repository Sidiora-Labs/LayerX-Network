use std::collections::BTreeSet;

use layerx_agentd::authority::{
    require_valid, resolve, AuthorityCache, AuthorityError, AuthorityResolver, AuthorityState,
};
use layerx_agentd::identity::ProtocolAuthority;

struct BoundaryAuthority {
    states: Vec<AuthorityState>,
    calls: usize,
}

impl AuthorityResolver for BoundaryAuthority {
    fn authority_state(
        &mut self,
        _authority: &ProtocolAuthority,
    ) -> Result<Option<AuthorityState>, AuthorityError> {
        let result = self.states.get(self.calls).cloned();
        self.calls += 1;
        Ok(result)
    }
}

fn state(generation: u64, observed: u64) -> AuthorityState {
    AuthorityState {
        authority: ProtocolAuthority::SessionKey([8; 32]),
        generation,
        valid_through_sequence: 100,
        revoked: false,
        permitted_activity_types: BTreeSet::from([7_u16]),
        observed_sequence: observed,
    }
}

#[test]
fn resolution_cache_has_an_explicit_freshness_bound() {
    let mut boundary = BoundaryAuthority {
        states: vec![state(1, 10), state(1, 13)],
        calls: 0,
    };
    let mut cache = AuthorityCache::default();
    let authority = ProtocolAuthority::SessionKey([8; 32]);
    assert!(resolve(&mut cache, &mut boundary, &authority, 10, 2).is_ok());
    assert!(resolve(&mut cache, &mut boundary, &authority, 12, 2).is_ok());
    assert_eq!(boundary.calls, 1);
    let refreshed = resolve(&mut cache, &mut boundary, &authority, 13, 2)
        .unwrap_or_else(|error| panic!("refresh failed: {error:?}"));
    assert_eq!(refreshed.observed_sequence, 13);
    assert_eq!(boundary.calls, 2);
}

#[test]
fn cached_authority_from_a_future_sequence_is_invalidated_and_refused() {
    let mut boundary = BoundaryAuthority {
        states: vec![state(1, 12), state(1, 9)],
        calls: 0,
    };
    let mut cache = AuthorityCache::default();
    let authority = ProtocolAuthority::SessionKey([8; 32]);
    assert!(resolve(&mut cache, &mut boundary, &authority, 12, 4).is_ok());
    assert_eq!(
        resolve(&mut cache, &mut boundary, &authority, 10, 4),
        Err(AuthorityError::SequenceRegression {
            current: 10,
            observed: 12,
        })
    );
    let refreshed = resolve(&mut cache, &mut boundary, &authority, 10, 4)
        .unwrap_or_else(|error| panic!("invalidated authority did not refresh: {error:?}"));
    assert_eq!(refreshed.observed_sequence, 9);
    assert_eq!(boundary.calls, 2);
}

#[test]
fn boundary_authority_from_a_future_sequence_is_not_cached() {
    let mut boundary = BoundaryAuthority {
        states: vec![state(1, 12), state(1, 10)],
        calls: 0,
    };
    let mut cache = AuthorityCache::default();
    let authority = ProtocolAuthority::SessionKey([8; 32]);
    assert_eq!(
        resolve(&mut cache, &mut boundary, &authority, 10, 4),
        Err(AuthorityError::SequenceRegression {
            current: 10,
            observed: 12,
        })
    );
    let refreshed = resolve(&mut cache, &mut boundary, &authority, 10, 4)
        .unwrap_or_else(|error| panic!("future observation poisoned cache: {error:?}"));
    assert_eq!(refreshed.observed_sequence, 10);
    assert_eq!(boundary.calls, 2);
}

#[test]
fn revocation_between_open_and_prepare_is_named_and_refused() {
    let authority = ProtocolAuthority::SessionKey([8; 32]);
    let mut revoked = state(1, 12);
    revoked.revoked = true;
    assert_eq!(
        require_valid(&authority, 1, 7, 12, &revoked),
        Err(AuthorityError::Revoked)
    );
}

#[test]
fn rotation_between_prepare_and_submit_is_named_and_refused() {
    let authority = ProtocolAuthority::SessionKey([8; 32]);
    assert_eq!(
        require_valid(&authority, 1, 7, 12, &state(2, 12)),
        Err(AuthorityError::Rotated)
    );
    assert_eq!(
        require_valid(&authority, 1, 99, 12, &state(1, 12)),
        Err(AuthorityError::Scope)
    );
    let resolved = require_valid(&authority, 1, 7, 12, &state(1, 12))
        .unwrap_or_else(|error| panic!("valid authority refused: {error:?}"));
    assert_eq!(resolved.authority, authority);
}
