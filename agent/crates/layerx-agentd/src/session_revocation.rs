//! Immediate session invalidation from core-produced authority events.

use layerx_types::ids::Did;

use crate::identity::ProtocolAuthority;
use crate::store::Store;

use super::{encode, next_generation, session_key, SessionError, SessionRef, SessionRegistry};

/// Core event that invalidates one authority or every authority under a DID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevocationEvent {
    pub did: Did,
    pub authority: Option<ProtocolAuthority>,
    pub reason: InvalidationReason,
    pub observed_sequence: u64,
}

/// Exact reason reported to affected clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidationReason {
    IdentityFrozen,
    PrimaryKeyRotated,
    SessionKeyRevoked,
    CapabilityGrantRevoked,
    AccountRecovered,
}

/// Local lifecycle state relevant to revocation handling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparationState {
    Prepared,
    Signed,
    Queued,
    Unknown,
    Executed,
    Failed,
}

/// One activity whose local handling may be changed by invalidation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingActivity {
    pub session: SessionRef,
    pub state: PreparationState,
    pub cancelled: bool,
    pub resolution_continues: bool,
}

/// Observable invalidation result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidationReport {
    pub reason: InvalidationReason,
    pub observed_sequence: u64,
    pub invalidated_sessions: Vec<SessionRef>,
    pub invalidated_generations: Vec<(SessionRef, u64)>,
    pub cancelled_preparations: usize,
    pub unresolved_left_for_resolution: usize,
    pub executed_untouched: usize,
}

pub(crate) fn apply_revocation(
    store: &mut Store,
    registry: &mut SessionRegistry,
    activities: &mut [PendingActivity],
    event: &RevocationEvent,
) -> Result<InvalidationReport, SessionError> {
    let mut invalidated_sessions = Vec::new();
    let mut invalidated_generations = Vec::new();
    let candidates = registry
        .records()
        .iter()
        .filter(|(_, record)| {
            record.open
                && record.request.agent == event.did
                && event
                    .authority
                    .as_ref()
                    .is_none_or(|authority| &record.request.authority == authority)
        })
        .map(|(session, record)| {
            let mut invalidated = record.clone();
            invalidated.generation = next_generation(&invalidated)?;
            invalidated.open = false;
            Ok((session.clone(), invalidated))
        })
        .collect::<Result<Vec<_>, SessionError>>()?;
    let updates = candidates
        .iter()
        .map(|(_, record)| Ok((session_key(&record.request)?, encode(record)?)))
        .collect::<Result<Vec<_>, SessionError>>()?;
    store.update_local_batch(updates)?;
    for (session, invalidated) in candidates {
        let invalidated_generation = invalidated
            .generation
            .checked_sub(1)
            .ok_or(SessionError::GenerationExhausted)?;
        registry.replace(session.clone(), invalidated);
        invalidated_generations.push((session.clone(), invalidated_generation));
        invalidated_sessions.push(session);
    }

    let mut cancelled_preparations = 0_usize;
    let mut unresolved_left_for_resolution = 0_usize;
    let mut executed_untouched = 0_usize;
    for activity in activities {
        if !invalidated_sessions.contains(&activity.session) {
            continue;
        }
        match activity.state {
            PreparationState::Prepared | PreparationState::Signed => {
                activity.cancelled = true;
                cancelled_preparations += 1;
            }
            PreparationState::Queued | PreparationState::Unknown => {
                activity.resolution_continues = true;
                unresolved_left_for_resolution += 1;
            }
            PreparationState::Executed | PreparationState::Failed => {
                executed_untouched += 1;
            }
        }
    }
    Ok(InvalidationReport {
        reason: event.reason,
        observed_sequence: event.observed_sequence,
        invalidated_sessions,
        invalidated_generations,
        cancelled_preparations,
        unresolved_left_for_resolution,
        executed_untouched,
    })
}
