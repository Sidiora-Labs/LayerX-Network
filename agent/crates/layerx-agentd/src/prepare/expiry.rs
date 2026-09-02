//! Prepared-activity expiry, reservation release and signed-byte retention.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::budget::{release, BudgetLimiter, LimitRefusal, ReleaseKind};
use crate::session::SessionRef;

use super::Prepared;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleState {
    Prepared,
    Signing,
    Signed,
    Submitted,
    Acknowledged,
    Unknown,
    Executed,
    Failed,
    Expired,
}

impl LifecycleState {
    const fn terminal(self) -> bool {
        matches!(self, Self::Executed | Self::Failed | Self::Expired)
    }

    const fn unresolved(self) -> bool {
        matches!(self, Self::Submitted | Self::Acknowledged | Self::Unknown)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayloadRedaction {
    Omit,
    DigestOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RetainedPreparation {
    authorization: Option<PreparationAuthorization>,
    state: LifecycleState,
    not_after: u64,
    reservation_ids: Vec<[u8; 32]>,
    signed_bytes: Option<Vec<u8>>,
    activity_id: Option<[u8; 32]>,
    payload_hash: [u8; 32],
    terminal_at_sequence: Option<u64>,
}

/// Exact session generation that owns a token-gated preparation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparationAuthorization {
    pub(crate) session: SessionRef,
    pub(crate) generation: u64,
}

#[derive(Default)]
pub struct PreparationLifecycle {
    records: Mutex<BTreeMap<[u8; 32], RetainedPreparation>>,
}

impl PreparationLifecycle {
    /// Records one prepared activity with its expiry bound and its held reservations.
    ///
    /// # Errors
    ///
    /// Returns `Unavailable` when the record lock is poisoned, or `Duplicate` when the
    /// preparation id is already registered.
    pub fn register(
        &self,
        preparation_id: [u8; 32],
        prepared: &Prepared,
        reservation_ids: Vec<[u8; 32]>,
    ) -> Result<(), LifecycleError> {
        self.register_inner(preparation_id, prepared, reservation_ids, None)
    }

    /// Records a preparation under the exact session generation that authorized it.
    pub(crate) fn register_authorized(
        &self,
        preparation_id: [u8; 32],
        prepared: &Prepared,
        reservation_ids: Vec<[u8; 32]>,
        authorization: PreparationAuthorization,
    ) -> Result<(), LifecycleError> {
        if authorization.generation == 0 {
            return Err(LifecycleError::InvalidAuthorization);
        }
        self.register_inner(
            preparation_id,
            prepared,
            reservation_ids,
            Some(authorization),
        )
    }

    fn register_inner(
        &self,
        preparation_id: [u8; 32],
        prepared: &Prepared,
        reservation_ids: Vec<[u8; 32]>,
        authorization: Option<PreparationAuthorization>,
    ) -> Result<(), LifecycleError> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| LifecycleError::Unavailable)?;
        if records.contains_key(&preparation_id) {
            return Err(LifecycleError::Duplicate);
        }
        records.insert(
            preparation_id,
            RetainedPreparation {
                authorization,
                state: LifecycleState::Prepared,
                not_after: prepared.envelope.timestamp_bound().not_after(),
                reservation_ids,
                signed_bytes: None,
                activity_id: None,
                payload_hash: prepared.envelope.payload_hash(),
                terminal_at_sequence: None,
            },
        );
        Ok(())
    }

    /// Fails every not-yet-submitted preparation owned by an invalidated exact generation while
    /// preserving submitted/unknown work for honest receipt resolution.
    pub fn invalidate_authorizations(
        &self,
        invalidated: &[(SessionRef, u64)],
        current_sequence: u64,
        limiter: &BudgetLimiter,
    ) -> Result<PreparationInvalidationReport, LifecycleError> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| LifecycleError::Unavailable)?;
        let mut report = PreparationInvalidationReport::default();
        for record in records.values_mut() {
            let selected = record.authorization.as_ref().is_some_and(|authorization| {
                invalidated.iter().any(|(session, generation)| {
                    &authorization.session == session && authorization.generation == *generation
                })
            });
            if !selected {
                continue;
            }
            match record.state {
                LifecycleState::Prepared | LifecycleState::Signing | LifecycleState::Signed => {
                    for reservation in &record.reservation_ids {
                        if release(limiter, *reservation, ReleaseKind::Failed, current_sequence)
                            .map_err(LifecycleError::Reservation)?
                        {
                            report.released_reservations.push(*reservation);
                        }
                    }
                    record.state = LifecycleState::Failed;
                    record.signed_bytes = None;
                    record.terminal_at_sequence = Some(current_sequence);
                    report.cancelled_preparations += 1;
                }
                LifecycleState::Submitted
                | LifecycleState::Acknowledged
                | LifecycleState::Unknown => {
                    report.unresolved_preserved += 1;
                }
                LifecycleState::Executed | LifecycleState::Failed | LifecycleState::Expired => {
                    report.terminal_untouched += 1;
                }
            }
        }
        Ok(report)
    }

    /// Advances one preparation along the permitted lifecycle edges only.
    ///
    /// # Errors
    ///
    /// Returns `Unavailable` when the record lock is poisoned, `NotFound` for an unregistered
    /// preparation, or `InvalidTransition` carrying both states for a disallowed edge.
    pub fn transition(
        &self,
        preparation_id: [u8; 32],
        next: LifecycleState,
        current_sequence: u64,
    ) -> Result<(), LifecycleError> {
        self.transition_inner(preparation_id, next, current_sequence, None)
    }

    /// Advances a token-bound preparation only for its exact owning session generation.
    pub(crate) fn transition_authorized(
        &self,
        preparation_id: [u8; 32],
        next: LifecycleState,
        current_sequence: u64,
        authorization: &PreparationAuthorization,
    ) -> Result<(), LifecycleError> {
        self.transition_inner(preparation_id, next, current_sequence, Some(authorization))
    }

    fn transition_inner(
        &self,
        preparation_id: [u8; 32],
        next: LifecycleState,
        current_sequence: u64,
        authorization: Option<&PreparationAuthorization>,
    ) -> Result<(), LifecycleError> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| LifecycleError::Unavailable)?;
        let record = records
            .get_mut(&preparation_id)
            .ok_or(LifecycleError::NotFound)?;
        require_authorization(record, authorization)?;
        if !valid_transition(record.state, next) {
            return Err(LifecycleError::InvalidTransition {
                from: record.state,
                to: next,
            });
        }
        record.state = next;
        if next.terminal() {
            record.terminal_at_sequence = Some(current_sequence);
        }
        Ok(())
    }

    /// Attaches signed bytes and their activity id to a preparation being signed.
    ///
    /// # Errors
    ///
    /// Returns `InvalidSignedBytes` for empty bytes, `Unavailable` when the record lock is
    /// poisoned, `NotFound` for an unregistered preparation, or `InvalidTransition` unless the
    /// record is currently `Signing`.
    pub fn retain_signed_bytes(
        &self,
        preparation_id: [u8; 32],
        signed_bytes: Vec<u8>,
        activity_id: [u8; 32],
    ) -> Result<(), LifecycleError> {
        self.retain_signed_bytes_inner(preparation_id, signed_bytes, activity_id, None)
    }

    /// Retains signed bytes only for the exact session generation that owns the preparation.
    pub(crate) fn retain_signed_bytes_authorized(
        &self,
        preparation_id: [u8; 32],
        signed_bytes: Vec<u8>,
        activity_id: [u8; 32],
        authorization: &PreparationAuthorization,
    ) -> Result<(), LifecycleError> {
        self.retain_signed_bytes_inner(
            preparation_id,
            signed_bytes,
            activity_id,
            Some(authorization),
        )
    }

    fn retain_signed_bytes_inner(
        &self,
        preparation_id: [u8; 32],
        signed_bytes: Vec<u8>,
        activity_id: [u8; 32],
        authorization: Option<&PreparationAuthorization>,
    ) -> Result<(), LifecycleError> {
        if signed_bytes.is_empty() {
            return Err(LifecycleError::InvalidSignedBytes);
        }
        let mut records = self
            .records
            .lock()
            .map_err(|_| LifecycleError::Unavailable)?;
        let record = records
            .get_mut(&preparation_id)
            .ok_or(LifecycleError::NotFound)?;
        require_authorization(record, authorization)?;
        if record.state != LifecycleState::Signing {
            return Err(LifecycleError::InvalidTransition {
                from: record.state,
                to: LifecycleState::Signed,
            });
        }
        record.signed_bytes = Some(signed_bytes);
        record.activity_id = Some(activity_id);
        record.state = LifecycleState::Signed;
        Ok(())
    }

    /// Admits a signed preparation for submission at the authoritative core batch time.
    ///
    /// # Errors
    ///
    /// Returns `Unavailable` when the record lock is poisoned, `NotFound` for an unregistered
    /// preparation, `PreparationExpired` past `not_after` or once already `Expired`, or
    /// `InvalidTransition` unless the record is `Signed`.
    pub fn admit_submission(
        &self,
        preparation_id: [u8; 32],
        core_batch_time: u64,
    ) -> Result<(), LifecycleError> {
        self.admit_submission_inner(preparation_id, core_batch_time, None)
    }

    /// Admits a token-bound preparation only for its exact owning session generation.
    pub(crate) fn admit_submission_authorized(
        &self,
        preparation_id: [u8; 32],
        core_batch_time: u64,
        authorization: &PreparationAuthorization,
    ) -> Result<(), LifecycleError> {
        self.admit_submission_inner(preparation_id, core_batch_time, Some(authorization))
    }

    fn admit_submission_inner(
        &self,
        preparation_id: [u8; 32],
        core_batch_time: u64,
        authorization: Option<&PreparationAuthorization>,
    ) -> Result<(), LifecycleError> {
        let records = self
            .records
            .lock()
            .map_err(|_| LifecycleError::Unavailable)?;
        let record = records
            .get(&preparation_id)
            .ok_or(LifecycleError::NotFound)?;
        require_authorization(record, authorization)?;
        if core_batch_time > record.not_after || record.state == LifecycleState::Expired {
            return Err(LifecycleError::PreparationExpired);
        }
        if record.state != LifecycleState::Signed {
            return Err(LifecycleError::InvalidTransition {
                from: record.state,
                to: LifecycleState::Submitted,
            });
        }
        Ok(())
    }

    /// Returns the current lifecycle state of one preparation.
    ///
    /// # Errors
    ///
    /// Returns `Unavailable` when the record lock is poisoned, or `NotFound` for an
    /// unregistered preparation.
    pub fn state(&self, preparation_id: [u8; 32]) -> Result<LifecycleState, LifecycleError> {
        let records = self
            .records
            .lock()
            .map_err(|_| LifecycleError::Unavailable)?;
        records
            .get(&preparation_id)
            .map(|record| record.state)
            .ok_or(LifecycleError::NotFound)
    }

    /// Reports whether signed bytes are still retained for one preparation.
    ///
    /// # Errors
    ///
    /// Returns `Unavailable` when the record lock is poisoned, or `NotFound` for an
    /// unregistered preparation.
    pub fn has_signed_bytes(&self, preparation_id: [u8; 32]) -> Result<bool, LifecycleError> {
        let records = self
            .records
            .lock()
            .map_err(|_| LifecycleError::Unavailable)?;
        records
            .get(&preparation_id)
            .map(|record| record.signed_bytes.is_some())
            .ok_or(LifecycleError::NotFound)
    }

    /// Renders one log line carrying the activity id and never the payload bytes.
    ///
    /// # Errors
    ///
    /// Returns `Unavailable` when the record lock is poisoned, `NotFound` for an unregistered
    /// preparation, or `ActivityIdUnavailable` before signing assigned an activity id.
    pub fn redacted_log(
        &self,
        preparation_id: [u8; 32],
        policy: PayloadRedaction,
    ) -> Result<String, LifecycleError> {
        let records = self
            .records
            .lock()
            .map_err(|_| LifecycleError::Unavailable)?;
        let record = records
            .get(&preparation_id)
            .ok_or(LifecycleError::NotFound)?;
        let activity_id = record
            .activity_id
            .ok_or(LifecycleError::ActivityIdUnavailable)?;
        let mut line = format!("activity_id={} payload=[redacted]", hex(&activity_id));
        if policy == PayloadRedaction::DigestOnly {
            line.push_str(" payload_hash=");
            line.push_str(&hex(&record.payload_hash));
        }
        Ok(line)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpirationReport {
    pub expired_preparations: Vec<[u8; 32]>,
    pub released_reservations: Vec<[u8; 32]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionReport {
    pub discarded_terminal_signed_bytes: usize,
    pub preserved_unresolved_signed_bytes: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PreparationInvalidationReport {
    pub cancelled_preparations: usize,
    pub unresolved_preserved: usize,
    pub terminal_untouched: usize,
    pub released_reservations: Vec<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleError {
    Duplicate,
    NotFound,
    Unavailable,
    InvalidSignedBytes,
    ActivityIdUnavailable,
    PreparationExpired,
    InvalidAuthorization,
    AuthorizationRequired,
    AuthorizationMismatch,
    InvalidTransition {
        from: LifecycleState,
        to: LifecycleState,
    },
    Reservation(LimitRefusal),
}

fn require_authorization(
    record: &RetainedPreparation,
    presented: Option<&PreparationAuthorization>,
) -> Result<(), LifecycleError> {
    match (&record.authorization, presented) {
        (None, None) => Ok(()),
        (Some(expected), Some(presented)) if expected == presented => Ok(()),
        (Some(_), None) => Err(LifecycleError::AuthorizationRequired),
        _ => Err(LifecycleError::AuthorizationMismatch),
    }
}

pub(crate) fn expire_elapsed(
    lifecycle: &PreparationLifecycle,
    limiter: &BudgetLimiter,
    core_batch_time: u64,
) -> Result<ExpirationReport, LifecycleError> {
    let mut records = lifecycle
        .records
        .lock()
        .map_err(|_| LifecycleError::Unavailable)?;
    let candidates: Vec<_> = records
        .iter()
        .filter(|(_, record)| {
            matches!(
                record.state,
                LifecycleState::Prepared | LifecycleState::Signing | LifecycleState::Signed
            ) && core_batch_time > record.not_after
        })
        .map(|(id, record)| (*id, record.reservation_ids.clone()))
        .collect();
    let mut report = ExpirationReport {
        expired_preparations: Vec::new(),
        released_reservations: Vec::new(),
    };
    for (preparation_id, reservations) in candidates {
        for reservation_id in reservations {
            if release(
                limiter,
                reservation_id,
                ReleaseKind::Expired,
                core_batch_time,
            )
            .map_err(LifecycleError::Reservation)?
            {
                report.released_reservations.push(reservation_id);
            }
        }
        if let Some(record) = records.get_mut(&preparation_id) {
            record.state = LifecycleState::Expired;
            record.terminal_at_sequence = Some(core_batch_time);
        }
        report.expired_preparations.push(preparation_id);
    }
    Ok(report)
}

pub(crate) fn sweep_retention(
    lifecycle: &PreparationLifecycle,
    current_sequence: u64,
    retention_sequences: u64,
) -> Result<RetentionReport, LifecycleError> {
    let mut records = lifecycle
        .records
        .lock()
        .map_err(|_| LifecycleError::Unavailable)?;
    let mut report = RetentionReport {
        discarded_terminal_signed_bytes: 0,
        preserved_unresolved_signed_bytes: 0,
    };
    for record in records.values_mut() {
        if record.state.unresolved() && record.signed_bytes.is_some() {
            report.preserved_unresolved_signed_bytes += 1;
            continue;
        }
        if record.state.terminal()
            && record.signed_bytes.is_some()
            && record.terminal_at_sequence.is_some_and(|terminal| {
                current_sequence >= terminal.saturating_add(retention_sequences)
            })
        {
            record.signed_bytes = None;
            report.discarded_terminal_signed_bytes += 1;
        }
    }
    Ok(report)
}

const fn valid_transition(from: LifecycleState, to: LifecycleState) -> bool {
    matches!(
        (from, to),
        (LifecycleState::Prepared, LifecycleState::Signing)
            | (LifecycleState::Signing, LifecycleState::Signed)
            | (LifecycleState::Signed, LifecycleState::Submitted)
            | (
                LifecycleState::Submitted,
                LifecycleState::Acknowledged
                    | LifecycleState::Unknown
                    | LifecycleState::Executed
                    | LifecycleState::Failed,
            )
            | (
                LifecycleState::Acknowledged,
                LifecycleState::Unknown | LifecycleState::Executed | LifecycleState::Failed,
            )
            | (
                LifecycleState::Unknown,
                LifecycleState::Executed | LifecycleState::Failed,
            )
    )
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}
