//! Domain types used by human-plane intents before canonical compilation.

use crate::amount::Amount;
use crate::limits::{IDENTIFIER_BYTES, MAX_PAYLOAD_BYTES};

macro_rules! exact_identifier {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; IDENTIFIER_BYTES]);

        impl $name {
            #[must_use]
            pub const fn new(bytes: [u8; IDENTIFIER_BYTES]) -> Self {
                Self(bytes)
            }

            #[must_use]
            pub const fn bytes(self) -> [u8; IDENTIFIER_BYTES] {
                self.0
            }

            #[must_use]
            pub fn is_zero(self) -> bool {
                self.0.iter().all(|byte| *byte == 0)
            }
        }
    };
}

exact_identifier!(PublicKey, "A protocol public-key identifier.");
exact_identifier!(RecoveryRoot, "A recovery-policy commitment.");
exact_identifier!(PayerGrantId, "A payer-grant identifier.");
exact_identifier!(BudgetId, "A protocol budget identifier.");
exact_identifier!(DepositProofId, "An external deposit-proof identifier.");
exact_identifier!(WithdrawalId, "A bridge withdrawal identifier.");
exact_identifier!(PurposeHash, "A protocol purpose commitment.");
exact_identifier!(ContextHash, "A transaction context commitment.");

/// Exact 20-byte EVM payout address.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EvmAddress([u8; 20]);

impl EvmAddress {
    #[must_use]
    pub const fn new(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 20] {
        self.0
    }
}

/// Protocol account sequence with no signed or floating-point representation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Sequence(u64);

impl Sequence {
    #[must_use]
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Protocol timestamp expressed as exact whole seconds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TimestampSeconds(u64);

impl TimestampSeconds {
    #[must_use]
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Non-zero protocol network identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkId(u32);

impl NetworkId {
    /// Constructs a network identifier.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero network.
    pub const fn new(value: u32) -> Result<Self, IntentDomainError> {
        if value == 0 {
            Err(IntentDomainError::Zero("network_id"))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }
}

/// Non-zero protocol version carried by module authorization material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolVersion(u16);

impl ProtocolVersion {
    /// Constructs a protocol version.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero version.
    pub const fn new(value: u16) -> Result<Self, IntentDomainError> {
        if value == 0 {
            Err(IntentDomainError::Zero("protocol_version"))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// Non-zero recovery approval threshold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApprovalThreshold(u16);

impl ApprovalThreshold {
    /// Constructs a recovery threshold.
    ///
    /// # Errors
    ///
    /// Refuses zero approvals.
    pub const fn new(value: u16) -> Result<Self, IntentDomainError> {
        if value == 0 {
            Err(IntentDomainError::Zero("approval_threshold"))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// Non-zero recurring or budget period measured in exact whole seconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeriodLength(u64);

impl PeriodLength {
    /// Constructs a period length.
    ///
    /// # Errors
    ///
    /// Refuses a zero-length period.
    pub const fn new(value: u64) -> Result<Self, IntentDomainError> {
        if value == 0 {
            Err(IntentDomainError::Zero("period_length"))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Payer-grant draw schedule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrantSchedule {
    SingleUse,
    Recurring(PeriodLength),
}

/// Protocol budget rollover policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RolloverPolicy {
    None,
    Capped,
}

/// Closed protocol authorization tags carried by a 402LXP send payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SendAuthorizationKind {
    Owner = 1,
    SessionKey = 2,
    DelegatedCapability = 3,
    BudgetAllowance = 4,
    Escrow = 5,
    ProtocolModule = 6,
}

/// Exact fixed-width signature used inside a 402LXP send authorization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizationSignature([u8; 64]);

impl AuthorizationSignature {
    #[must_use]
    pub const fn new(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 64] {
        self.0
    }
}

/// Domain-typed authorization embedded in the canonical send payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SendAuthorization {
    kind: SendAuthorizationKind,
    public_key: PublicKey,
    signature: AuthorizationSignature,
}

impl SendAuthorization {
    #[must_use]
    pub const fn new(
        kind: SendAuthorizationKind,
        public_key: PublicKey,
        signature: AuthorizationSignature,
    ) -> Self {
        Self {
            kind,
            public_key,
            signature,
        }
    }

    #[must_use]
    pub const fn kind(self) -> SendAuthorizationKind {
        self.kind
    }

    #[must_use]
    pub const fn public_key(self) -> PublicKey {
        self.public_key
    }

    #[must_use]
    pub const fn signature(self) -> AuthorizationSignature {
        self.signature
    }
}

/// Construction failure for an intent-domain scalar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntentDomainError {
    Zero(&'static str),
}

exact_identifier!(ProgramId, "A deployed protocol program identifier.");

/// The agent-layer contract major the program-call operation is declared under.
///
/// The operation is an additive extension inside this major: it introduces new
/// types and a new canonical payload, and it changes no existing type, field,
/// ordering or encoding. Consumers pinned to this major keep working unchanged.
pub const PROGRAM_CALL_CONTRACT_MAJOR: u16 = 1;

/// Domain separation for the canonical program-call payload. Any change to the
/// wire layout must change this tag so a stale decoder cannot silently accept a
/// new encoding.
pub const PROGRAM_CALL_PAYLOAD_DOMAIN: &[u8] = b"LayerX/programs/call/v1\0";

/// Maximum calldata bytes carried by one program call, bounded before
/// allocation against the canonical module-payload ceiling.
pub const MAX_CALLDATA_BYTES: usize = MAX_PAYLOAD_BYTES;

/// Maximum successful response payload a program call may return, matching the
/// runtime call-response transport bound.
pub const MAX_CALL_RESPONSE_BYTES: usize = 1_048_576;

/// Stable graph anchor naming the agent-layer program-call operation.
#[must_use]
pub const fn programs_call_operation() -> &'static str {
    "layerx-programs-call-operation-v1"
}

/// The declared resource budget a caller commits to one program call. A call is
/// a money-adjacent state change, so both the execution fuel and the monetary
/// fee ceiling are named explicitly rather than defaulted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallBudget {
    fuel: u64,
    fee_limit: Amount,
}

impl CallBudget {
    /// Declares a fuel bound and a fee ceiling for one call.
    ///
    /// # Errors
    ///
    /// Returns [`ProgramCallError::ZeroFuel`] when the fuel bound is zero, as a
    /// zero-fuel call can make no progress and would only ever refuse.
    pub const fn new(fuel: u64, fee_limit: Amount) -> Result<Self, ProgramCallError> {
        if fuel == 0 {
            return Err(ProgramCallError::ZeroFuel);
        }
        Ok(Self { fuel, fee_limit })
    }

    /// Returns the declared fuel bound.
    #[must_use]
    pub const fn fuel(self) -> u64 {
        self.fuel
    }

    /// Returns the declared monetary fee ceiling.
    #[must_use]
    pub const fn fee_limit(self) -> Amount {
        self.fee_limit
    }
}

/// The closed set of capabilities a caller may request for one program call.
/// The runtime grants no authority a call did not request, so an omitted
/// capability is a denied capability by construction.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum CapabilityRequest {
    /// Read the caller-scoped storage namespace.
    StorageRead = 1,
    /// Write the caller-scoped storage namespace.
    StorageWrite = 2,
    /// Request 402LXP value transfers.
    Transfer = 3,
    /// Emit ordered protocol events.
    EmitEvent = 4,
    /// Compose further program-to-program calls.
    Compose = 5,
}

impl CapabilityRequest {
    /// Decodes a capability tag without accepting extensions.
    ///
    /// # Errors
    ///
    /// Returns [`ProgramCallError::UnknownCapability`] for an undeclared tag.
    pub const fn from_u8(value: u8) -> Result<Self, ProgramCallError> {
        match value {
            1 => Ok(Self::StorageRead),
            2 => Ok(Self::StorageWrite),
            3 => Ok(Self::Transfer),
            4 => Ok(Self::EmitEvent),
            5 => Ok(Self::Compose),
            other => Err(ProgramCallError::UnknownCapability(other)),
        }
    }

    /// Returns the canonical tag byte.
    #[must_use]
    pub const fn tag(self) -> u8 {
        self as u8
    }
}

/// A sorted, duplicate-free capability request set. Its canonical ordering is
/// fixed by the tag so the same requested authority always encodes identically.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestedCapabilities(Vec<CapabilityRequest>);

impl RequestedCapabilities {
    /// Constructs a canonically ordered, duplicate-free capability set.
    ///
    /// # Errors
    ///
    /// Returns [`ProgramCallError::DuplicateCapability`] when a capability is
    /// requested more than once.
    pub fn new(requested: &[CapabilityRequest]) -> Result<Self, ProgramCallError> {
        let mut sorted = requested.to_vec();
        sorted.sort_unstable();
        for pair in sorted.windows(2) {
            if pair[0] == pair[1] {
                return Err(ProgramCallError::DuplicateCapability(pair[0].tag()));
            }
        }
        Ok(Self(sorted))
    }

    /// Constructs the empty capability set, requesting no authority at all.
    #[must_use]
    pub const fn none() -> Self {
        Self(Vec::new())
    }

    /// Borrows the canonically ordered capabilities.
    #[must_use]
    pub fn as_slice(&self) -> &[CapabilityRequest] {
        &self.0
    }

    /// Reports whether a capability was requested.
    #[must_use]
    pub fn contains(&self, capability: CapabilityRequest) -> bool {
        self.0.binary_search(&capability).is_ok()
    }
}

/// Bounded calldata handed to a program-call entry point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Calldata(Box<[u8]>);

impl Calldata {
    /// Constructs bounded calldata, refusing before allocation over the bound.
    ///
    /// # Errors
    ///
    /// Returns [`ProgramCallError::CalldataLength`] when the byte count exceeds
    /// [`MAX_CALLDATA_BYTES`].
    pub fn new(bytes: &[u8]) -> Result<Self, ProgramCallError> {
        if bytes.len() > MAX_CALLDATA_BYTES {
            return Err(ProgramCallError::CalldataLength(bytes.len()));
        }
        Ok(Self(Box::<[u8]>::from(bytes)))
    }

    /// Borrows the exact calldata bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// The program-call operation carried by the agent-layer contract: which
/// program to enter, the calldata to hand it, the declared budget it may spend
/// and the capabilities it requests. The operation is compiled to a canonical
/// module payload that every surface — agent layer, CLI and emulator — encodes
/// identically, so the same call yields the same receipt regardless of surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramCall {
    callee: ProgramId,
    calldata: Calldata,
    budget: CallBudget,
    capabilities: RequestedCapabilities,
}

impl ProgramCall {
    /// Assembles one program-call operation.
    #[must_use]
    pub fn new(
        callee: ProgramId,
        calldata: Calldata,
        budget: CallBudget,
        capabilities: RequestedCapabilities,
    ) -> Self {
        Self {
            callee,
            calldata,
            budget,
            capabilities,
        }
    }

    /// Returns the callee program identifier.
    #[must_use]
    pub const fn callee(&self) -> ProgramId {
        self.callee
    }

    /// Borrows the calldata handed to the callee.
    #[must_use]
    pub const fn calldata(&self) -> &Calldata {
        &self.calldata
    }

    /// Returns the declared call budget.
    #[must_use]
    pub const fn budget(&self) -> CallBudget {
        self.budget
    }

    /// Borrows the requested capability set.
    #[must_use]
    pub const fn capabilities(&self) -> &RequestedCapabilities {
        &self.capabilities
    }

    /// Encodes the canonical program-call module payload. The layout is fixed:
    /// the domain tag, the callee identifier, the declared fuel and fee ceiling,
    /// the sorted requested capabilities, and the length-prefixed calldata. The
    /// encoding is total and deterministic so identical operations always yield
    /// identical bytes across every surface.
    #[must_use]
    pub fn canonical_payload(&self) -> Vec<u8> {
        let capabilities = self.capabilities.as_slice();
        let calldata = self.calldata.as_bytes();
        let mut payload = Vec::with_capacity(
            PROGRAM_CALL_PAYLOAD_DOMAIN
                .len()
                .saturating_add(IDENTIFIER_BYTES)
                .saturating_add(8)
                .saturating_add(16)
                .saturating_add(2)
                .saturating_add(capabilities.len())
                .saturating_add(4)
                .saturating_add(calldata.len()),
        );
        payload.extend_from_slice(PROGRAM_CALL_PAYLOAD_DOMAIN);
        payload.extend_from_slice(&self.callee.bytes());
        payload.extend_from_slice(&self.budget.fuel().to_be_bytes());
        payload.extend_from_slice(&self.budget.fee_limit().to_be_bytes());
        let capability_count = u16::try_from(capabilities.len()).unwrap_or(u16::MAX);
        payload.extend_from_slice(&capability_count.to_be_bytes());
        for capability in capabilities {
            payload.push(capability.tag());
        }
        let calldata_length = u32::try_from(calldata.len()).unwrap_or(u32::MAX);
        payload.extend_from_slice(&calldata_length.to_be_bytes());
        payload.extend_from_slice(calldata);
        payload
    }
}

/// The typed successful response returned by one program call: the callee's
/// non-negative result code and its bounded response bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramCallResponse {
    code: i32,
    body: Box<[u8]>,
}

impl ProgramCallResponse {
    /// Constructs a typed response, refusing a negative code or over-long body.
    ///
    /// # Errors
    ///
    /// Returns [`ProgramCallError::NegativeResponseCode`] for a negative code,
    /// which the failure taxonomy carries instead, and
    /// [`ProgramCallError::ResponseLength`] when the body exceeds
    /// [`MAX_CALL_RESPONSE_BYTES`].
    pub fn new(code: i32, body: &[u8]) -> Result<Self, ProgramCallError> {
        if code < 0 {
            return Err(ProgramCallError::NegativeResponseCode(code));
        }
        if body.len() > MAX_CALL_RESPONSE_BYTES {
            return Err(ProgramCallError::ResponseLength(body.len()));
        }
        Ok(Self {
            code,
            body: Box::<[u8]>::from(body),
        })
    }

    /// Returns the callee's non-negative result code.
    #[must_use]
    pub const fn code(&self) -> i32 {
        self.code
    }

    /// Borrows the exact response bytes.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

/// The closed taxonomy of typed program-call failures surfaced to the caller.
/// Each variant aborts the whole call, so no partial state, transfer or event
/// can survive a refused call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramCallFailure {
    /// The callee identifier resolves to no deployed program.
    UnknownProgram,
    /// The callee was already active on the call stack.
    Reentrancy,
    /// The call graph would nest deeper than the declared rule allows.
    DepthExceeded { limit: u32, attempted: u32 },
    /// One frame would issue more calls than the declared rule allows.
    FanoutExceeded { limit: u32, attempted: u32 },
    /// The callee refused with a negative result code.
    GuestRefused { code: i32 },
    /// The call was refused by the capability ABI.
    Authority,
    /// The call exhausted a metered resource, including its declared budget.
    Resource,
    /// Successful-response transport was refused.
    Response,
    /// The callee faulted deterministically.
    Fault,
}

impl ProgramCallFailure {
    /// Returns the stable class tag for the typed failure.
    #[must_use]
    pub const fn class_tag(self) -> u8 {
        match self {
            Self::UnknownProgram => 1,
            Self::Reentrancy => 2,
            Self::DepthExceeded { .. } => 3,
            Self::FanoutExceeded { .. } => 4,
            Self::GuestRefused { .. } => 5,
            Self::Authority => 6,
            Self::Resource => 7,
            Self::Response => 8,
            Self::Fault => 9,
        }
    }
}

/// The typed outcome of one program call: either the callee's response or a
/// typed failure. The outcome is the response the operation carries back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramCallOutcome {
    /// The call completed and returned a typed response.
    Completed(ProgramCallResponse),
    /// The call was refused with a typed failure.
    Refused(ProgramCallFailure),
}

impl ProgramCallOutcome {
    /// Reports whether the call completed with a response.
    #[must_use]
    pub const fn is_completed(&self) -> bool {
        matches!(self, Self::Completed(_))
    }

    /// Returns the completed response, if any.
    #[must_use]
    pub const fn response(&self) -> Option<&ProgramCallResponse> {
        match self {
            Self::Completed(response) => Some(response),
            Self::Refused(_) => None,
        }
    }

    /// Returns the typed failure, if any.
    #[must_use]
    pub const fn failure(&self) -> Option<ProgramCallFailure> {
        match self {
            Self::Refused(failure) => Some(*failure),
            Self::Completed(_) => None,
        }
    }
}

/// Construction failure for a program-call operation or its typed response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramCallError {
    /// A declared fuel bound of zero was supplied.
    ZeroFuel,
    /// A capability tag outside the closed set was presented.
    UnknownCapability(u8),
    /// A capability was requested more than once.
    DuplicateCapability(u8),
    /// Calldata exceeded the protocol maximum.
    CalldataLength(usize),
    /// A response body exceeded the protocol maximum.
    ResponseLength(usize),
    /// A negative code was presented as a successful response.
    NegativeResponseCode(i32),
}

#[cfg(test)]
mod program_call_tests {
    use super::{
        programs_call_operation, Amount, CallBudget, Calldata, CapabilityRequest, ProgramCall,
        ProgramCallError, ProgramCallFailure, ProgramCallOutcome, ProgramCallResponse, ProgramId,
        RequestedCapabilities, MAX_CALLDATA_BYTES, MAX_CALL_RESPONSE_BYTES,
        PROGRAM_CALL_CONTRACT_MAJOR,
    };

    /// The shared canonical program-call payload every surface must reproduce.
    /// The agent layer, the CLI and the emulator all encode this exact byte
    /// string for the same operation, so the same call yields the same receipt.
    pub(crate) const GOLDEN_PAYLOAD_HEX: &str = "4c61796572582f70726f6772616d732f63616c6c2f763100111111111111111111111111111111111111111111111111111111111111111100000000000003e8000000000000000000000000000000fa0002010300000002aabb";

    fn golden_call() -> Result<ProgramCall, ProgramCallError> {
        let callee = ProgramId::new([0x11; 32]);
        let calldata = Calldata::new(&[0xAA, 0xBB])?;
        let budget = CallBudget::new(1000, Amount::from_u128(250))?;
        let capabilities = RequestedCapabilities::new(&[
            CapabilityRequest::Transfer,
            CapabilityRequest::StorageRead,
        ])?;
        Ok(ProgramCall::new(callee, calldata, budget, capabilities))
    }

    fn hex(bytes: &[u8]) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
            encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        encoded
    }

    #[test]
    fn canonical_payload_matches_shared_golden_vector() -> Result<(), ProgramCallError> {
        assert_eq!(hex(&golden_call()?.canonical_payload()), GOLDEN_PAYLOAD_HEX);
        Ok(())
    }

    #[test]
    fn canonical_payload_is_deterministic() -> Result<(), ProgramCallError> {
        assert_eq!(
            golden_call()?.canonical_payload(),
            golden_call()?.canonical_payload()
        );
        Ok(())
    }

    #[test]
    fn capabilities_are_sorted_and_unique() -> Result<(), ProgramCallError> {
        let capabilities = RequestedCapabilities::new(&[
            CapabilityRequest::Compose,
            CapabilityRequest::StorageRead,
        ])?;
        assert_eq!(
            capabilities.as_slice(),
            &[CapabilityRequest::StorageRead, CapabilityRequest::Compose]
        );
        assert!(capabilities.contains(CapabilityRequest::Compose));
        Ok(())
    }

    #[test]
    fn duplicate_capability_is_refused() {
        assert_eq!(
            RequestedCapabilities::new(&[
                CapabilityRequest::Transfer,
                CapabilityRequest::Transfer,
            ]),
            Err(ProgramCallError::DuplicateCapability(
                CapabilityRequest::Transfer.tag()
            ))
        );
    }

    #[test]
    fn zero_fuel_budget_is_refused() {
        assert_eq!(
            CallBudget::new(0, Amount::ZERO),
            Err(ProgramCallError::ZeroFuel)
        );
    }

    #[test]
    fn oversized_calldata_is_refused_before_allocation() {
        let oversized = vec![0_u8; MAX_CALLDATA_BYTES + 1];
        assert_eq!(
            Calldata::new(&oversized),
            Err(ProgramCallError::CalldataLength(MAX_CALLDATA_BYTES + 1))
        );
    }

    #[test]
    fn response_rejects_negative_code_and_oversized_body() {
        assert_eq!(
            ProgramCallResponse::new(-1, &[]),
            Err(ProgramCallError::NegativeResponseCode(-1))
        );
        let oversized = vec![0_u8; MAX_CALL_RESPONSE_BYTES + 1];
        assert_eq!(
            ProgramCallResponse::new(0, &oversized),
            Err(ProgramCallError::ResponseLength(MAX_CALL_RESPONSE_BYTES + 1))
        );
    }

    #[test]
    fn typed_outcome_carries_response_or_failure() -> Result<(), ProgramCallError> {
        let response = ProgramCallResponse::new(0, &[1, 2, 3])?;
        let completed = ProgramCallOutcome::Completed(response);
        assert!(completed.is_completed());
        assert_eq!(completed.response().map(ProgramCallResponse::code), Some(0));
        let refused = ProgramCallOutcome::Refused(ProgramCallFailure::UnknownProgram);
        assert_eq!(refused.failure(), Some(ProgramCallFailure::UnknownProgram));
        assert_eq!(ProgramCallFailure::UnknownProgram.class_tag(), 1);
        Ok(())
    }

    #[test]
    fn operation_is_additive_within_the_current_contract_major() {
        assert_eq!(PROGRAM_CALL_CONTRACT_MAJOR, 1);
        assert_eq!(
            programs_call_operation(),
            "layerx-programs-call-operation-v1"
        );
    }
}
