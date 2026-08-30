//! Fail-closed inspection of untrusted version and kind discriminants.

use layerx_wire::decode::Decoder;
use layerx_wire::WireError;

/// The closed set of executable V1 intent kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum IntentKindTag {
    DidRegistration = 1,
    KeyRotation = 2,
    RecoveryRegistration = 3,
    EvmPayoutBinding = 4,
    LxpSend = 5,
    LxpReceive = 6,
    PayerGrantRegistration = 7,
    BudgetCreate = 8,
    BudgetFund = 9,
    BudgetDefund = 10,
    BridgeDepositCredit = 11,
    BridgeWithdrawRequest = 12,
    SessionGrant = 13,
    SessionRevoke = 14,
}

impl IntentKindTag {
    const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::DidRegistration),
            2 => Some(Self::KeyRotation),
            3 => Some(Self::RecoveryRegistration),
            4 => Some(Self::EvmPayoutBinding),
            5 => Some(Self::LxpSend),
            6 => Some(Self::LxpReceive),
            7 => Some(Self::PayerGrantRegistration),
            8 => Some(Self::BudgetCreate),
            9 => Some(Self::BudgetFund),
            10 => Some(Self::BudgetDefund),
            11 => Some(Self::BridgeDepositCredit),
            12 => Some(Self::BridgeWithdrawRequest),
            13 => Some(Self::SessionGrant),
            14 => Some(Self::SessionRevoke),
            _ => None,
        }
    }
}

/// A recognised intent header. Recognition alone never executes an intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntentHeader {
    pub version: u16,
    pub kind: IntentKindTag,
}

/// Typed reason an untrusted intent was preserved instead of executed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectReason {
    Malformed(WireError),
    UnknownVersion(u16),
    UnknownKind(u16),
}

/// Zero-copy diagnostic record retaining the exact untrusted input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RejectedIntent<'a> {
    input: &'a [u8],
    reason: RejectReason,
}

impl<'a> RejectedIntent<'a> {
    #[must_use]
    pub const fn input(&self) -> &'a [u8] {
        self.input
    }

    #[must_use]
    pub const fn reason(&self) -> RejectReason {
        self.reason
    }
}

/// Inspects only the version and kind of an untrusted intent envelope.
///
/// This function cannot execute or compile input. Unknown values retain a
/// zero-copy reference to every original byte for bounded diagnosis.
///
/// # Errors
///
/// Returns a typed rejection for truncated input, every version other than V1,
/// and every kind outside the closed fourteen-kind vocabulary.
pub fn inspect_intent(input: &[u8]) -> Result<IntentHeader, RejectedIntent<'_>> {
    let mut decoder = Decoder::new(input, 0);
    let version = decoder.u16().map_err(|error| RejectedIntent {
        input,
        reason: RejectReason::Malformed(error),
    })?;
    if version != 1 {
        return Err(RejectedIntent {
            input,
            reason: RejectReason::UnknownVersion(version),
        });
    }
    let raw_kind = decoder.u16().map_err(|error| RejectedIntent {
        input,
        reason: RejectReason::Malformed(error),
    })?;
    let Some(kind) = IntentKindTag::from_u16(raw_kind) else {
        return Err(RejectedIntent {
            input,
            reason: RejectReason::UnknownKind(raw_kind),
        });
    };
    Ok(IntentHeader { version, kind })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_version_and_kind_preserve_every_input_byte() {
        let unknown_version = [0, 2, 0, 5, 0xaa, 0xbb];
        let version_rejection = inspect_intent(&unknown_version)
            .err()
            .unwrap_or_else(|| panic!("unknown version accepted"));
        assert_eq!(version_rejection.input(), unknown_version);
        assert_eq!(version_rejection.reason(), RejectReason::UnknownVersion(2));

        let unknown_kind = [0, 1, 0xff, 0xfe, 0xcc];
        let kind_rejection = inspect_intent(&unknown_kind)
            .err()
            .unwrap_or_else(|| panic!("unknown kind accepted"));
        assert_eq!(kind_rejection.input(), unknown_kind);
        assert_eq!(kind_rejection.reason(), RejectReason::UnknownKind(65_534));
    }

    #[test]
    fn recognised_header_is_classification_not_execution() {
        assert_eq!(
            inspect_intent(&[0, 1, 0, 12, 0xde, 0xad]),
            Ok(IntentHeader {
                version: 1,
                kind: IntentKindTag::BridgeWithdrawRequest,
            })
        );
    }
}
