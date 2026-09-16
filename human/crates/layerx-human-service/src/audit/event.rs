use crate::redaction::Label;

use super::wire::{push_bytes, Reader};
use super::AuditError;

/// How an authentication event was performed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthMethod {
    Passkey,
    FallbackCredential,
    StepUp,
}

impl AuthMethod {
    const fn code(self) -> u8 {
        match self {
            Self::Passkey => 1,
            Self::FallbackCredential => 2,
            Self::StepUp => 3,
        }
    }

    fn from_code(value: u8) -> Result<Self, AuditError> {
        match value {
            1 => Ok(Self::Passkey),
            2 => Ok(Self::FallbackCredential),
            3 => Ok(Self::StepUp),
            _ => Err(AuditError::Corrupt("unknown authentication method")),
        }
    }
}

/// Whether a guarded decision was granted or refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Granted,
    Refused,
}

impl Decision {
    const fn code(self) -> u8 {
        match self {
            Self::Granted => 1,
            Self::Refused => 2,
        }
    }

    fn from_code(value: u8) -> Result<Self, AuditError> {
        match value {
            1 => Ok(Self::Granted),
            2 => Ok(Self::Refused),
            _ => Err(AuditError::Corrupt("unknown decision")),
        }
    }
}

/// The step-up evidence recorded with a guarded decision: never the ceremony
/// material itself, only its digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepUpEvidence {
    NotRequired,
    Fresh { ceremony_digest: [u8; 32] },
    Missing,
}

impl StepUpEvidence {
    fn encode(self, output: &mut Vec<u8>) {
        match self {
            Self::NotRequired => output.push(0),
            Self::Fresh { ceremony_digest } => {
                output.push(1);
                output.extend_from_slice(&ceremony_digest);
            }
            Self::Missing => output.push(2),
        }
    }

    fn decode(reader: &mut Reader<'_>) -> Result<Self, AuditError> {
        match reader.byte()? {
            0 => Ok(Self::NotRequired),
            1 => Ok(Self::Fresh {
                ceremony_digest: reader.array()?,
            }),
            2 => Ok(Self::Missing),
            _ => Err(AuditError::Corrupt("unknown step-up evidence")),
        }
    }
}

/// The operation a signing decision was made for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SigningOperation {
    DidRegistration,
    KeyRotation,
    RecoveryRegistration,
    EvmPayoutBinding,
    LxpSend,
    LxpReceive,
    PayerGrantRegistration,
    BudgetCreate,
    BudgetFund,
    BudgetDefund,
    BridgeDepositCredit,
    BridgeWithdrawRequest,
    SessionProvision,
    ProtocolMutation,
    ApprovalDecision,
    SecuritySettings,
    SecretReveal,
    EmergencyExit,
    AgentArchive,
}

impl SigningOperation {
    const fn code(self) -> u8 {
        match self {
            Self::DidRegistration => 1,
            Self::KeyRotation => 2,
            Self::RecoveryRegistration => 3,
            Self::EvmPayoutBinding => 4,
            Self::LxpSend => 5,
            Self::LxpReceive => 6,
            Self::PayerGrantRegistration => 7,
            Self::BudgetCreate => 8,
            Self::BudgetFund => 9,
            Self::BudgetDefund => 10,
            Self::BridgeDepositCredit => 11,
            Self::BridgeWithdrawRequest => 12,
            Self::SessionProvision => 13,
            Self::ProtocolMutation => 14,
            Self::ApprovalDecision => 15,
            Self::SecuritySettings => 16,
            Self::SecretReveal => 17,
            Self::EmergencyExit => 18,
            Self::AgentArchive => 19,
        }
    }

    fn from_code(value: u8) -> Result<Self, AuditError> {
        match value {
            1 => Ok(Self::DidRegistration),
            2 => Ok(Self::KeyRotation),
            3 => Ok(Self::RecoveryRegistration),
            4 => Ok(Self::EvmPayoutBinding),
            5 => Ok(Self::LxpSend),
            6 => Ok(Self::LxpReceive),
            7 => Ok(Self::PayerGrantRegistration),
            8 => Ok(Self::BudgetCreate),
            9 => Ok(Self::BudgetFund),
            10 => Ok(Self::BudgetDefund),
            11 => Ok(Self::BridgeDepositCredit),
            12 => Ok(Self::BridgeWithdrawRequest),
            13 => Ok(Self::SessionProvision),
            14 => Ok(Self::ProtocolMutation),
            15 => Ok(Self::ApprovalDecision),
            16 => Ok(Self::SecuritySettings),
            17 => Ok(Self::SecretReveal),
            18 => Ok(Self::EmergencyExit),
            19 => Ok(Self::AgentArchive),
            _ => Err(AuditError::Corrupt("unknown signing operation")),
        }
    }
}

/// The final outcome of an approval hold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalOutcome {
    Approved,
    Rejected,
    Expired,
    Defective,
}

impl ApprovalOutcome {
    const fn code(self) -> u8 {
        match self {
            Self::Approved => 1,
            Self::Rejected => 2,
            Self::Expired => 3,
            Self::Defective => 4,
        }
    }

    fn from_code(value: u8) -> Result<Self, AuditError> {
        match value {
            1 => Ok(Self::Approved),
            2 => Ok(Self::Rejected),
            3 => Ok(Self::Expired),
            4 => Ok(Self::Defective),
            _ => Err(AuditError::Corrupt("unknown approval outcome")),
        }
    }
}

/// The journey kinds whose transitions are audited.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JourneyKind {
    Onboarding,
    WalletBinding,
    Deposit,
    Withdraw,
    Exit,
    Move,
    AgentCreate,
    AgentFund,
    AgentPause,
    AgentRetire,
}

impl JourneyKind {
    const fn code(self) -> u8 {
        match self {
            Self::Onboarding => 1,
            Self::WalletBinding => 2,
            Self::Deposit => 3,
            Self::Withdraw => 4,
            Self::Exit => 5,
            Self::Move => 6,
            Self::AgentCreate => 7,
            Self::AgentFund => 8,
            Self::AgentPause => 9,
            Self::AgentRetire => 10,
        }
    }

    fn from_code(value: u8) -> Result<Self, AuditError> {
        match value {
            1 => Ok(Self::Onboarding),
            2 => Ok(Self::WalletBinding),
            3 => Ok(Self::Deposit),
            4 => Ok(Self::Withdraw),
            5 => Ok(Self::Exit),
            6 => Ok(Self::Move),
            7 => Ok(Self::AgentCreate),
            8 => Ok(Self::AgentFund),
            9 => Ok(Self::AgentPause),
            10 => Ok(Self::AgentRetire),
            _ => Err(AuditError::Corrupt("unknown journey kind")),
        }
    }
}

/// The normative journey states a transition moves between.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JourneyState {
    GettingReady,
    Sending,
    Processing,
    Done,
    DoneFinalised,
    StillChecking,
    Refused,
    WaitingForYou,
}

impl JourneyState {
    const fn code(self) -> u8 {
        match self {
            Self::GettingReady => 1,
            Self::Sending => 2,
            Self::Processing => 3,
            Self::Done => 4,
            Self::DoneFinalised => 5,
            Self::StillChecking => 6,
            Self::Refused => 7,
            Self::WaitingForYou => 8,
        }
    }

    fn from_code(value: u8) -> Result<Self, AuditError> {
        match value {
            1 => Ok(Self::GettingReady),
            2 => Ok(Self::Sending),
            3 => Ok(Self::Processing),
            4 => Ok(Self::Done),
            5 => Ok(Self::DoneFinalised),
            6 => Ok(Self::StillChecking),
            7 => Ok(Self::Refused),
            8 => Ok(Self::WaitingForYou),
            _ => Err(AuditError::Corrupt("unknown journey state")),
        }
    }
}

/// The audited security-setting changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityChangeKind {
    DeviceAdded,
    SessionRevoked,
    SignOutEverywhere,
    TwoFactorEnabled,
    TwoFactorDisabled,
    RecoveryInitiated,
    KeyRotation,
    WalletRebinding,
    AgentArchive,
    KeyExported,
}

impl SecurityChangeKind {
    const fn code(self) -> u8 {
        match self {
            Self::DeviceAdded => 1,
            Self::SessionRevoked => 2,
            Self::SignOutEverywhere => 3,
            Self::TwoFactorEnabled => 4,
            Self::TwoFactorDisabled => 5,
            Self::RecoveryInitiated => 6,
            Self::KeyRotation => 7,
            Self::WalletRebinding => 8,
            Self::AgentArchive => 9,
            Self::KeyExported => 10,
        }
    }

    fn from_code(value: u8) -> Result<Self, AuditError> {
        match value {
            1 => Ok(Self::DeviceAdded),
            2 => Ok(Self::SessionRevoked),
            3 => Ok(Self::SignOutEverywhere),
            4 => Ok(Self::TwoFactorEnabled),
            5 => Ok(Self::TwoFactorDisabled),
            6 => Ok(Self::RecoveryInitiated),
            7 => Ok(Self::KeyRotation),
            8 => Ok(Self::WalletRebinding),
            9 => Ok(Self::AgentArchive),
            10 => Ok(Self::KeyExported),
            _ => Err(AuditError::Corrupt("unknown security change")),
        }
    }
}

/// The notification classes whose dispatches are audited.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationClass {
    ApprovalWaiting,
    MoneyArrived,
    JourneyCompleted,
    JourneyFailed,
    ClaimReady,
    Security,
    Degradation,
}

impl NotificationClass {
    const fn code(self) -> u8 {
        match self {
            Self::ApprovalWaiting => 1,
            Self::MoneyArrived => 2,
            Self::JourneyCompleted => 3,
            Self::JourneyFailed => 4,
            Self::ClaimReady => 5,
            Self::Security => 6,
            Self::Degradation => 7,
        }
    }

    fn from_code(value: u8) -> Result<Self, AuditError> {
        match value {
            1 => Ok(Self::ApprovalWaiting),
            2 => Ok(Self::MoneyArrived),
            3 => Ok(Self::JourneyCompleted),
            4 => Ok(Self::JourneyFailed),
            5 => Ok(Self::ClaimReady),
            6 => Ok(Self::Security),
            7 => Ok(Self::Degradation),
            _ => Err(AuditError::Corrupt("unknown notification class")),
        }
    }
}

/// The channel a notification was dispatched on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationChannel {
    Push,
    Email,
    InApp,
}

impl NotificationChannel {
    const fn code(self) -> u8 {
        match self {
            Self::Push => 1,
            Self::Email => 2,
            Self::InApp => 3,
        }
    }

    fn from_code(value: u8) -> Result<Self, AuditError> {
        match value {
            1 => Ok(Self::Push),
            2 => Ok(Self::Email),
            3 => Ok(Self::InApp),
            _ => Err(AuditError::Corrupt("unknown notification channel")),
        }
    }
}

/// The identity-lifecycle milestones recorded with their activating receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityEvent {
    DidRegistration,
    KeyActivation,
    RecoveryRegistration,
    WalletBinding,
}

impl IdentityEvent {
    const fn code(self) -> u8 {
        match self {
            Self::DidRegistration => 1,
            Self::KeyActivation => 2,
            Self::RecoveryRegistration => 3,
            Self::WalletBinding => 4,
        }
    }

    fn from_code(value: u8) -> Result<Self, AuditError> {
        match value {
            1 => Ok(Self::DidRegistration),
            2 => Ok(Self::KeyActivation),
            3 => Ok(Self::RecoveryRegistration),
            4 => Ok(Self::WalletBinding),
            _ => Err(AuditError::Corrupt("unknown identity event")),
        }
    }
}

/// One audited event. Every field is a digest, a bounded label or a closed
/// enum, so payload secrets, key material, personal data and financial
/// values are unrepresentable in the audit log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuditEvent {
    Authentication {
        method: AuthMethod,
        outcome: Decision,
    },
    SigningDecision {
        operation: SigningOperation,
        disclosure_digest: [u8; 32],
        step_up: StepUpEvidence,
        outcome: Decision,
    },
    ApprovalDecision {
        hold_digest: [u8; 32],
        step_up: StepUpEvidence,
        outcome: ApprovalOutcome,
    },
    JourneyTransition {
        journey: Label,
        kind: JourneyKind,
        from: JourneyState,
        to: JourneyState,
    },
    SecurityChange {
        change: SecurityChangeKind,
        step_up: StepUpEvidence,
    },
    NotificationDispatch {
        class: NotificationClass,
        channel: NotificationChannel,
    },
    IdentityLifecycle {
        event: IdentityEvent,
        receipt_digest: [u8; 32],
    },
}

impl AuditEvent {
    /// Returns the emission label of the event's kind.
    #[must_use]
    pub const fn kind_label(&self) -> &'static str {
        match self {
            Self::Authentication { .. } => "authentication",
            Self::SigningDecision { .. } => "signing-decision",
            Self::ApprovalDecision { .. } => "approval-decision",
            Self::JourneyTransition { .. } => "journey-transition",
            Self::SecurityChange { .. } => "security-change",
            Self::NotificationDispatch { .. } => "notification-dispatch",
            Self::IdentityLifecycle { .. } => "identity-lifecycle",
        }
    }

    pub(super) fn encode(&self, output: &mut Vec<u8>) -> Result<(), AuditError> {
        match self {
            Self::Authentication { method, outcome } => {
                output.push(1);
                output.push(method.code());
                output.push(outcome.code());
            }
            Self::SigningDecision {
                operation,
                disclosure_digest,
                step_up,
                outcome,
            } => {
                output.push(2);
                output.push(operation.code());
                output.extend_from_slice(disclosure_digest);
                step_up.encode(output);
                output.push(outcome.code());
            }
            Self::ApprovalDecision {
                hold_digest,
                step_up,
                outcome,
            } => {
                output.push(3);
                output.extend_from_slice(hold_digest);
                step_up.encode(output);
                output.push(outcome.code());
            }
            Self::JourneyTransition {
                journey,
                kind,
                from,
                to,
            } => {
                output.push(4);
                push_bytes(output, journey.as_str().as_bytes())?;
                output.push(kind.code());
                output.push(from.code());
                output.push(to.code());
            }
            Self::SecurityChange { change, step_up } => {
                output.push(5);
                output.push(change.code());
                step_up.encode(output);
            }
            Self::NotificationDispatch { class, channel } => {
                output.push(6);
                output.push(class.code());
                output.push(channel.code());
            }
            Self::IdentityLifecycle {
                event,
                receipt_digest,
            } => {
                output.push(7);
                output.push(event.code());
                output.extend_from_slice(receipt_digest);
            }
        }
        Ok(())
    }

    pub(super) fn decode(reader: &mut Reader<'_>) -> Result<Self, AuditError> {
        match reader.byte()? {
            1 => Ok(Self::Authentication {
                method: AuthMethod::from_code(reader.byte()?)?,
                outcome: Decision::from_code(reader.byte()?)?,
            }),
            2 => Ok(Self::SigningDecision {
                operation: SigningOperation::from_code(reader.byte()?)?,
                disclosure_digest: reader.array()?,
                step_up: StepUpEvidence::decode(reader)?,
                outcome: Decision::from_code(reader.byte()?)?,
            }),
            3 => Ok(Self::ApprovalDecision {
                hold_digest: reader.array()?,
                step_up: StepUpEvidence::decode(reader)?,
                outcome: ApprovalOutcome::from_code(reader.byte()?)?,
            }),
            4 => {
                let text = std::str::from_utf8(reader.bytes()?)
                    .map_err(|_| AuditError::Corrupt("journey label is not UTF-8"))?;
                Ok(Self::JourneyTransition {
                    journey: Label::new(text)?,
                    kind: JourneyKind::from_code(reader.byte()?)?,
                    from: JourneyState::from_code(reader.byte()?)?,
                    to: JourneyState::from_code(reader.byte()?)?,
                })
            }
            5 => Ok(Self::SecurityChange {
                change: SecurityChangeKind::from_code(reader.byte()?)?,
                step_up: StepUpEvidence::decode(reader)?,
            }),
            6 => Ok(Self::NotificationDispatch {
                class: NotificationClass::from_code(reader.byte()?)?,
                channel: NotificationChannel::from_code(reader.byte()?)?,
            }),
            7 => Ok(Self::IdentityLifecycle {
                event: IdentityEvent::from_code(reader.byte()?)?,
                receipt_digest: reader.array()?,
            }),
            _ => Err(AuditError::Corrupt("unknown audit event kind")),
        }
    }
}
