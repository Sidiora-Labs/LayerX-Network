//! Direct projections over durable Human journey and receipt owners.

use std::collections::BTreeMap;

use super::backend::{ApiFailure, BackendResponse, ScopedRequest};
use crate::activity::{
    verification_status, EntryDetail, EvidenceBundle, ExportError, Feed, VerificationStatus,
    VerifyError,
};
use crate::journeys::{
    JourneyEngine, JourneyKind, JourneyPhase, JourneyState, VerifiedLegEvidence,
};
use crate::notify::ActivityEntryId;
use crate::notify::JourneyId;
use crate::store::PrincipalScope;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use layerx_proof::checkpoint::SettlementDomain;
use layerx_proof::receipt::AuthorizedBatch;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

pub(super) fn execute(
    feed: Feed,
    settlement_domain: SettlementDomain,
    scope: &PrincipalScope<'_>,
    request: &ScopedRequest<'_>,
) -> Option<Result<BackendResponse, ApiFailure>> {
    Some(match request.operation.name.as_str() {
        "journey.get" => journey_get(scope, settlement_domain, request),
        "journey.list" => journey_list(scope, settlement_domain),
        "evidence.get" => evidence_get(feed, settlement_domain, scope, request),
        _ => return None,
    })
}

pub(super) fn activity_entry(
    feed: Feed,
    settlement_domain: SettlementDomain,
    scope: &PrincipalScope<'_>,
    request: &ScopedRequest<'_>,
) -> Result<BackendResponse, ApiFailure> {
    let id = ActivityEntryId::new(
        request
            .path_parameters
            .get("entry_id")
            .cloned()
            .ok_or_else(|| ApiFailure::invalid_request(Some("entry_id")))?,
    )
    .map_err(|_| ApiFailure::invalid_request(Some("entry_id")))?;
    response(activity_entry_json(feed, settlement_domain, scope, &id)?)
}

pub(super) fn activity_entry_json(
    feed: Feed,
    settlement_domain: SettlementDomain,
    scope: &PrincipalScope<'_>,
    id: &ActivityEntryId,
) -> Result<Value, ApiFailure> {
    let entry = feed
        .entry(scope, id)
        .map_err(feed_failure)?
        .ok_or_else(ApiFailure::not_found)?;
    let mut actuals = Vec::with_capacity(entry.receipts().len());
    let mut labels = BTreeMap::new();
    for receipt in entry.receipts() {
        let bytes = receipt
            .canonical()
            .ok_or_else(ApiFailure::upstream_degraded)?;
        let digest =
            decode_digest(receipt.reference()).ok_or_else(ApiFailure::upstream_degraded)?;
        let row = receipt.authority().map(|authority| ReceiptRow {
            entry_id: id.clone(),
            canonical: bytes,
            authority,
        });
        labels.insert(
            receipt.reference().to_owned(),
            receipt_label(scope, settlement_domain, row, digest)?,
        );
        actuals.push(
            crate::activity::detail::ReceiptActual::from_verified_journey_bytes(
                bytes,
                receipt.reference(),
            )
            .map_err(|_| ApiFailure::upstream_degraded())?,
        );
    }
    let detail = EntryDetail::assemble(&entry, actuals, None)
        .map_err(|_| ApiFailure::upstream_degraded())?;
    let stages=detail.stages().iter().enumerate().map(|(index,stage)|json!({"stage_id":format!("stg_{}_{index}",id.as_str()),"copy_key":stage.label(),"state":stage_state(stage.state()),"evidence":[]})).collect::<Vec<_>>();
    let money = detail.actuals().first().map(
        |actual| json!({"amount":actual.amount().to_string(),"currency":hex(&actual.asset())}),
    );
    let fee_money = detail
        .actuals()
        .first()
        .map(|actual| json!({"amount":actual.fee().to_string(),"currency":hex(&actual.asset())}));
    let evidence = detail
        .evidence()
        .iter()
        .map(|link| {
            let verification = labels
                .get(link.reference())
                .copied()
                .ok_or_else(ApiFailure::unavailable)?;
            Ok(
                json!({"evidence_id":format!("evd_{}",link.reference()),"class":"layerx-receipt","verification":verification}),
            )
        })
        .collect::<Result<Vec<_>, ApiFailure>>()?;
    Ok(
        json!({"entry_id":id.as_str(),"kind":activity_kind(detail.kind()),"state":activity_status(detail.status()),"state_copy_key":format!("activity.state.{}",activity_status(detail.status())),"summary_copy_key":detail.sentence(),"occurred_at":entry.occurred_at(),"stages":stages,"evidence":evidence,"money":money,"fees":fee_money}),
    )
}

fn journey_get(
    scope: &PrincipalScope<'_>,
    settlement_domain: SettlementDomain,
    request: &ScopedRequest<'_>,
) -> Result<BackendResponse, ApiFailure> {
    let value = request
        .path_parameters
        .get("journey_id")
        .ok_or_else(|| ApiFailure::invalid_request(Some("journey_id")))?;
    let id = JourneyId::new(value.clone())
        .map_err(|_| ApiFailure::invalid_request(Some("journey_id")))?;
    let journey = JourneyEngine::load(scope, &id)
        .map_err(failure)?
        .ok_or_else(ApiFailure::not_found)?;
    response(journey_json(scope, settlement_domain, &journey)?)
}

fn journey_list(
    scope: &PrincipalScope<'_>,
    settlement_domain: SettlementDomain,
) -> Result<BackendResponse, ApiFailure> {
    let journeys = JourneyEngine::list(scope).map_err(failure)?;
    let mut digest = Sha256::new();
    digest.update(b"layerx-human-journey-list-cursor/v1");
    let values = journeys
        .iter()
        .map(|journey| {
            let value = journey_json(scope, settlement_domain, journey)?;
            digest.update(journey.updated_at().to_be_bytes());
            digest.update(
                journey
                    .status()
                    .map_err(failure)?
                    .journey_id()
                    .as_str()
                    .as_bytes(),
            );
            Ok(value)
        })
        .collect::<Result<Vec<_>, ApiFailure>>()?;
    response(
        json!({"journeys": values, "next_cursor": format!("cur_{}", hex(&digest.finalize().into()))}),
    )
}

fn evidence_get(
    feed: Feed,
    settlement_domain: SettlementDomain,
    scope: &PrincipalScope<'_>,
    request: &ScopedRequest<'_>,
) -> Result<BackendResponse, ApiFailure> {
    let id = request
        .path_parameters
        .get("evidence_id")
        .ok_or_else(|| ApiFailure::invalid_request(Some("evidence_id")))?;
    let expected = id
        .strip_prefix("evd_")
        .and_then(decode_digest)
        .ok_or_else(|| ApiFailure::invalid_request(Some("evidence_id")))?;
    let export_key = crate::store::RowKey::new(format!("activity-export-{}", hex(&expected)))
        .map_err(|_| ApiFailure::invalid_request(Some("evidence_id")))?;
    if let Some(row) = scope.get(crate::store::Table::Cache, &export_key) {
        return response(
            json!({"evidence_id": id, "class": "local-journey-state", "verification": "unverified",
            "content_type": "text/csv; charset=utf-8", "bytes_base64": STANDARD.encode(row.bytes())}),
        );
    }
    let bundle_key = crate::store::RowKey::new(format!("activity-evidence-{}", hex(&expected)))
        .map_err(|_| ApiFailure::invalid_request(Some("evidence_id")))?;
    if let Some(row) = scope.get(crate::store::Table::Cache, &bundle_key) {
        let bundle =
            EvidenceBundle::decode(row.bytes()).map_err(|_| ApiFailure::upstream_degraded())?;
        let receipt_authority = bundle
            .receipt_authority(feed, scope)
            .map_err(feed_failure)?;
        let status = verification_status(bundle.verify(
            expected,
            scope.principal(),
            settlement_domain,
            &receipt_authority,
        ));
        let verification = verification_label(&status)?;
        return response(
            json!({"evidence_id":id,"class":"local-journey-state","verification":verification,"content_type":"application/vnd.layerx.evidence-bundle","bytes_base64":STANDARD.encode(row.bytes())}),
        );
    }
    let cache_key = crate::store::RowKey::new(format!("state-proof-{}", hex(&expected)))
        .map_err(|_| ApiFailure::invalid_request(Some("evidence_id")))?;
    if let Some(row) = scope.get(crate::store::Table::Cache, &cache_key) {
        return response(
            json!({"evidence_id": id, "class": "checkpoint-proof", "verification": "checkpoint-finalised",
            "content_type": "application/vnd.layerx.state-proof", "bytes_base64": STANDARD.encode(row.bytes())}),
        );
    }
    if let Some((entry_id, evidence)) = journey_receipt(scope, expected)? {
        let verification = receipt_label(
            scope,
            settlement_domain,
            Some(ReceiptRow {
                entry_id,
                canonical: &evidence.canonical_receipt,
                authority: &evidence.authorised_batch,
            }),
            expected,
        )?;
        return response(
            json!({"evidence_id": id, "class": "layerx-receipt", "verification": verification,
            "content_type": "application/vnd.layerx.receipt", "bytes_base64": STANDARD.encode(evidence.canonical_receipt)}),
        );
    }
    Err(ApiFailure::not_found())
}

pub(super) fn journey_json(
    scope: &PrincipalScope<'_>,
    settlement_domain: SettlementDomain,
    journey: &JourneyEngine,
) -> Result<Value, ApiFailure> {
    let status = journey.status().map_err(failure)?;
    let entry_id =
        crate::activity::stable_entry_id(status.journey_id().as_str()).map_err(feed_failure)?;
    let mut references = Vec::with_capacity(status.phases().len());
    for index in 0..status.phases().len() {
        let Some(digest) = status.receipt_digests().get(index).copied().flatten() else {
            references.push(None);
            continue;
        };
        let material = status
            .receipt_material()
            .get(index)
            .and_then(Option::as_deref);
        let authority = status
            .receipt_authorities()
            .get(index)
            .and_then(Option::as_ref);
        let row = material
            .zip(authority)
            .map(|(canonical, authority)| ReceiptRow {
                entry_id: entry_id.clone(),
                canonical,
                authority,
            });
        let verification = receipt_label(scope, settlement_domain, row, digest)?;
        references.push(Some(evidence_ref(digest, verification)));
    }
    let stages = status.phases().iter().enumerate().map(|(index, phase)| {
        let evidence = references.get(index).cloned().flatten().into_iter().collect::<Vec<_>>();
        json!({"stage_id": format!("stg_{}_{index}", status.journey_id().as_str()),
            "copy_key": format!("journey.stage.{}", phase_label(*phase)), "state": phase_state(*phase), "evidence": evidence})
    }).collect::<Vec<_>>();
    let evidence = references.iter().flatten().cloned().collect::<Vec<_>>();
    let state = state_label(status.state());
    Ok(
        json!({"journey_id": status.journey_id().as_str(), "kind": kind_label(journey.kind()), "state": state,
        "state_copy_key": format!("journey.state.{state}"), "stages": stages, "evidence": evidence,
        "started_at": journey.started_at(), "updated_at": journey.updated_at()}),
    )
}

/// Labels one custody receipt digest through the verifier over the receipt
/// the service holds for it.
pub(super) fn custody_receipt_label(
    scope: &PrincipalScope<'_>,
    settlement_domain: SettlementDomain,
    digest: [u8; 32],
) -> Result<&'static str, ApiFailure> {
    let found = journey_receipt(scope, digest)?;
    let row = found.as_ref().map(|(entry_id, evidence)| ReceiptRow {
        entry_id: entry_id.clone(),
        canonical: &evidence.canonical_receipt,
        authority: &evidence.authorised_batch,
    });
    receipt_label(scope, settlement_domain, row, digest)
}

/// Maps a verifier status onto the wire verification level; an unavailable
/// verdict has no wire level and refuses the read instead.
pub(super) fn verification_label(status: &VerificationStatus) -> Result<&'static str, ApiFailure> {
    if status.is_unavailable() {
        Err(ApiFailure::unavailable())
    } else {
        Ok(status.label())
    }
}

struct ReceiptRow<'a> {
    entry_id: ActivityEntryId,
    canonical: &'a [u8],
    authority: &'a AuthorizedBatch,
}

fn receipt_label(
    scope: &PrincipalScope<'_>,
    settlement_domain: SettlementDomain,
    row: Option<ReceiptRow<'_>>,
    digest: [u8; 32],
) -> Result<&'static str, ApiFailure> {
    let status = match row {
        Some(row) => verification_status(EvidenceBundle::verify_receipt(
            row.entry_id,
            row.canonical,
            row.authority,
            digest,
            scope.principal(),
            settlement_domain,
        )),
        None => verification_status(Err(VerifyError::Unavailable(
            ExportError::AuthorityUnavailable {
                reference: hex(&digest),
            },
        ))),
    };
    verification_label(&status)
}

fn journey_receipt(
    scope: &PrincipalScope<'_>,
    digest: [u8; 32],
) -> Result<Option<(ActivityEntryId, VerifiedLegEvidence)>, ApiFailure> {
    for journey in JourneyEngine::list(scope).map_err(failure)? {
        let status = journey.status().map_err(failure)?;
        for index in 0..status.phases().len() {
            let Some(evidence) = journey.verified_leg_evidence(index).map_err(failure)? else {
                continue;
            };
            if evidence.receipt_digest == digest {
                let entry_id = crate::activity::stable_entry_id(status.journey_id().as_str())
                    .map_err(feed_failure)?;
                return Ok(Some((entry_id, evidence)));
            }
        }
    }
    Ok(None)
}

fn evidence_ref(digest: [u8; 32], verification: &'static str) -> Value {
    json!({"evidence_id": format!("evd_{}", hex(&digest)), "class": "layerx-receipt", "verification": verification})
}
fn phase_label(value: JourneyPhase) -> &'static str {
    match value {
        JourneyPhase::Compiled => "compiled",
        JourneyPhase::Preparing => "preparing",
        JourneyPhase::Prepared => "prepared",
        JourneyPhase::Signed => "signed",
        JourneyPhase::Submitted => "submitted",
        JourneyPhase::StillChecking => "still-checking",
        JourneyPhase::ReceiptVerified => "receipt-verified",
        JourneyPhase::Refused => "refused",
    }
}
fn phase_state(value: JourneyPhase) -> &'static str {
    match value {
        JourneyPhase::Compiled | JourneyPhase::Preparing | JourneyPhase::Prepared => {
            "getting-ready"
        }
        JourneyPhase::Signed => "sending",
        JourneyPhase::Submitted => "processing",
        JourneyPhase::StillChecking => "still-checking",
        JourneyPhase::ReceiptVerified => "done",
        JourneyPhase::Refused => "refused",
    }
}
fn state_label(value: JourneyState) -> &'static str {
    match value {
        JourneyState::GettingReady => "getting-ready",
        JourneyState::Sending => "sending",
        JourneyState::Processing => "processing",
        JourneyState::StillChecking => "still-checking",
        JourneyState::Done => "done",
        JourneyState::Refused => "refused",
    }
}
fn kind_label(value: JourneyKind) -> &'static str {
    match value {
        JourneyKind::Onboarding => "onboarding",
        JourneyKind::WalletBinding => "wallet-binding",
        JourneyKind::Deposit => "deposit",
        JourneyKind::Withdraw => "withdraw",
        JourneyKind::Exit => "exit",
        JourneyKind::Move => "move",
        JourneyKind::AgentCreate => "agent-create",
        JourneyKind::AgentFund => "agent-fund",
        JourneyKind::AgentPause => "agent-pause",
        JourneyKind::AgentRetire => "agent-retire",
    }
}
fn decode_digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut out = [0; 32];
    for (i, p) in value.as_bytes().chunks_exact(2).enumerate() {
        out[i] = nibble(p[0])? << 4 | nibble(p[1])?;
    }
    Some(out)
}
fn nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}
fn hex(value: &[u8; 32]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in value {
        out.push(char::from(D[usize::from(byte >> 4)]));
        out.push(char::from(D[usize::from(byte & 15)]));
    }
    out
}
fn response(result: Value) -> Result<BackendResponse, ApiFailure> {
    Ok(BackendResponse {
        result,
        session: None,
    })
}
fn failure(error: crate::journeys::JourneyError) -> ApiFailure {
    match error {
        crate::journeys::JourneyError::Store(_) => ApiFailure::unavailable(),
        _ => ApiFailure::upstream_degraded(),
    }
}
fn feed_failure(error: crate::activity::FeedError) -> ApiFailure {
    match error {
        crate::activity::FeedError::Store(_) => ApiFailure::unavailable(),
        _ => ApiFailure::upstream_degraded(),
    }
}
fn activity_kind(value: crate::activity::ActivityKind) -> &'static str {
    match value {
        crate::activity::ActivityKind::Deposit => "deposit",
        crate::activity::ActivityKind::Withdrawal => "withdrawal",
        crate::activity::ActivityKind::Movement => "movement",
        crate::activity::ActivityKind::AgentAction => "agent-action",
        crate::activity::ActivityKind::Approval => "approval",
        crate::activity::ActivityKind::Security => "security-event",
    }
}
fn activity_status(value: crate::activity::ActivityStatus) -> &'static str {
    use crate::activity::{ActivityStatus as S, DepositStage as D, WithdrawalStage as W};
    match value {
        S::GettingReady => "getting-ready",
        S::Sending => "sending",
        S::Processing
        | S::Deposit(D::ConfirmingOnPaxeer)
        | S::Deposit(D::Crediting)
        | S::Withdrawal(W::Processing)
        | S::Withdrawal(W::WaitingForSettlement) => "processing",
        S::StillChecking => "still-checking",
        S::WaitingForYou | S::Deposit(D::WaitingForWallet) | S::Withdrawal(W::ReadyToClaim) => {
            "waiting-for-you"
        }
        S::Done | S::Deposit(D::Done) | S::Withdrawal(W::PaidOut) => "done",
        S::DoneFinalised => "done-finalised",
        S::DidntGoThrough { .. } => "refused",
    }
}
fn stage_state(value: crate::activity::detail::StageState) -> &'static str {
    match value {
        crate::activity::detail::StageState::Complete => "done",
        crate::activity::detail::StageState::Current => "processing",
        crate::activity::detail::StageState::Upcoming => "getting-ready",
        crate::activity::detail::StageState::Failed => "refused",
    }
}
