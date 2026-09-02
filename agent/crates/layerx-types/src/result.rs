//! Exact protocol result codes derived from `include/layerx/lxp_result.h`.

/// Protocol result-code domain. Numeric ranges match the C17 core.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultDomain {
    /// Successful execution.
    Success,
    /// Canonical codec failure.
    Codec,
    /// Envelope validation failure.
    Envelope,
    /// Authority validation failure.
    Authority,
    /// Sequencing failure.
    Sequencing,
    /// Ledger invariant failure.
    Ledger,
    /// Exact arithmetic failure.
    Arithmetic,
    /// Metering or fee failure.
    Metering,
    /// Protocol module failure.
    Module,
    /// Batch or availability failure.
    Batch,
    /// Durable storage failure.
    Storage,
    /// Fatal consensus invariant failure.
    Fatal,
    /// A non-protocol or future positive value.
    Unknown,
}

/// Whether retrying byte-identical activity bytes can become meaningful after
/// external protocol state advances.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retriability {
    /// The same activity must not be retried automatically.
    Terminal,
    /// Resolution may be retried without changing the activity intent.
    Retriable,
}

macro_rules! protocol_result_codes {
    ($( $variant:ident = $raw:literal, $retry:ident; )+) => {
        /// Every result code currently declared by the normative C17 header.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(i32)]
        pub enum KnownResult {
            $( $variant = $raw, )+
        }

        impl KnownResult {
            /// All declared result variants, in canonical header order.
            pub const ALL: &'static [Self] = &[$( Self::$variant, )+];

            /// Returns the exact signed 32-bit consensus number.
            #[must_use]
            pub const fn raw(self) -> i32 {
                match self { $( Self::$variant => $raw, )+ }
            }

            /// Classifies retry behaviour from one auditable data table.
            #[must_use]
            pub const fn retriability(self) -> Retriability {
                match self { $( Self::$variant => Retriability::$retry, )+ }
            }

            const fn from_raw(raw: i32) -> Option<Self> {
                match raw { $( $raw => Some(Self::$variant), )+ _ => None }
            }
        }
    };
}

protocol_result_codes! {
    Ok = 0, Terminal;
    Truncated = -1, Terminal;
    TrailingBytes = -2, Terminal;
    NonCanonical = -3, Terminal;
    UnsortedSequence = -4, Terminal;
    LengthLimit = -5, Terminal;
    InvalidTag = -6, Terminal;
    UnknownField = -7, Terminal;
    WrongNetwork = -100, Terminal;
    VersionUnsupported = -101, Terminal;
    UnknownModule = -102, Terminal;
    ModuleDisabled = -103, Retriable;
    PayloadHashMismatch = -104, Terminal;
    MalformedEnvelope = -105, Terminal;
    UnknownActivity = -106, Terminal;
    MalformedSend = -107, Terminal;
    MalformedReceive = -108, Terminal;
    UnknownDid = -200, Terminal;
    BadSignature = -201, Terminal;
    AuthExpired = -202, Terminal;
    AuthRevoked = -203, Terminal;
    AuthScope = -204, Terminal;
    AuthAllowance = -205, Terminal;
    IdentityFrozen = -206, Terminal;
    UnauthorizedDebit = -207, Terminal;
    UnknownAccountNamespace = -208, Terminal;
    MalformedGrant = -209, Terminal;
    UnknownAuthorityKind = -210, Terminal;
    GrantExhausted = -211, Terminal;
    StaleRevocation = -212, Terminal;
    ContextMismatch = -213, Terminal;
    NoPayerGrant = -214, Terminal;
    GrantScopeViolation = -215, Terminal;
    PurposeMismatch = -216, Terminal;
    InvoiceAlreadySettled = -217, Terminal;
    GrantExpired = -218, Terminal;
    GrantRevoked = -219, Terminal;
    SequenceGap = -300, Retriable;
    SequenceReused = -301, Terminal;
    IdempotentReplay = -302, Terminal;
    Expired = -303, Terminal;
    NotYetValid = -304, Retriable;
    SequenceMismatch = -305, Retriable;
    ConditionUnmet = -306, Terminal;
    InsufficientBalance = -400, Terminal;
    ZeroAmount = -401, Terminal;
    AssetMismatch = -402, Terminal;
    AssetPaused = -403, Retriable;
    AccountFrozen = -404, Retriable;
    Conservation = -405, Terminal;
    TooManyLegs = -406, Terminal;
    AccountNotEmpty = -407, Terminal;
    BalanceBypass = -408, Terminal;
    AccountIdMismatch = -409, Terminal;
    ClientSuppliedBalance = -410, Terminal;
    AssetAlreadyRegistered = -411, Terminal;
    InvalidAmount = -412, Terminal;
    DepositProofNotFinal = -413, Retriable;
    DepositAlreadyCredited = -414, Terminal;
    WithdrawalAlreadySettled = -415, Terminal;
    ChallengeWindowOpen = -416, Retriable;
    WithdrawalCancelled = -417, Terminal;
    WithdrawalAssetMismatch = -418, Terminal;
    Overflow = -500, Terminal;
    Underflow = -501, Terminal;
    DivZero = -502, Terminal;
    Precision = -503, Terminal;
    FeeLimit = -600, Terminal;
    GasExhausted = -601, Terminal;
    FeeUnpayable = -602, Terminal;
    EscrowState = -700, Terminal;
    BudgetPeriodCap = -701, Terminal;
    StreamUnderfunded = -702, Terminal;
    MarketHalted = -703, Retriable;
    OracleStale = -704, Retriable;
    MarginInsufficient = -705, Terminal;
    AgreementState = -706, Terminal;
    CaptureExceedsHold = -707, Terminal;
    UnauthorizedCapture = -708, Terminal;
    HoldExpired = -709, Terminal;
    UnauthorizedEscrowSpend = -710, Terminal;
    DisputeWindowClosed = -711, Terminal;
    HoldDisputed = -712, Terminal;
    BudgetAllowanceExceeded = -713, Terminal;
    InsufficientBudgetFunds = -714, Terminal;
    UnauthorizedDelegate = -715, Terminal;
    BudgetRevoked = -716, Terminal;
    NonMonotonicTime = -717, Terminal;
    AccrualOverflow = -718, Terminal;
    UnauthorizedMeter = -719, Terminal;
    MeterRegression = -720, Terminal;
    StreamClosed = -721, Terminal;
    ModuleMayNotWriteBalance = -722, Terminal;
    OfferUnavailable = -723, Retriable;
    TermsMismatch = -724, Terminal;
    InvalidAttestation = -725, Terminal;
    DeliverableMismatch = -726, Terminal;
    DeliveryDeadlinePassed = -727, Terminal;
    UnauthorizedDisputant = -728, Terminal;
    UnauthorizedOracle = -729, Terminal;
    OracleSequence = -730, Terminal;
    OracleBounds = -731, Terminal;
    OracleDeviation = -732, Terminal;
    MarketAlreadyExists = -733, Terminal;
    ParameterBounds = -734, Terminal;
    PausedScope = -735, Retriable;
    BatchGap = -800, Retriable;
    RootMismatch = -801, Terminal;
    TimestampRegression = -802, Terminal;
    AttestationThreshold = -803, Retriable;
    DaMissing = -804, Retriable;
    Equivocation = -805, Terminal;
    LogCorrupt = -900, Terminal;
    LogTruncated = -901, Terminal;
    SnapshotMismatch = -902, Terminal;
    ProjectionStale = -903, Retriable;
    Io = -904, Retriable;
    ArenaExhausted = -905, Retriable;
    SnapshotBlobsMissing = -906, Terminal;
    FatalInvariant = -1001, Terminal;
    FatalReplayDivergence = -1002, Terminal;
    FatalSupplyMismatch = -1003, Terminal;
}

/// A lossless protocol result code. Future unknown numbers survive unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultCode(i32);

impl ResultCode {
    /// Preserves any signed protocol number without reinterpretation.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    /// Returns the byte-for-byte numeric value carried in a receipt.
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Returns the typed variant when this version knows the number.
    #[must_use]
    pub const fn known(self) -> Option<KnownResult> {
        KnownResult::from_raw(self.0)
    }

    /// Returns the protocol domain using the exact C17 range partition.
    #[must_use]
    pub const fn domain(self) -> ResultDomain {
        match self.0 {
            0 => ResultDomain::Success,
            ..=-1000 => ResultDomain::Fatal,
            -999..=-900 => ResultDomain::Storage,
            -899..=-800 => ResultDomain::Batch,
            -799..=-700 => ResultDomain::Module,
            -699..=-600 => ResultDomain::Metering,
            -599..=-500 => ResultDomain::Arithmetic,
            -499..=-400 => ResultDomain::Ledger,
            -399..=-300 => ResultDomain::Sequencing,
            -299..=-200 => ResultDomain::Authority,
            -199..=-100 => ResultDomain::Envelope,
            -99..=-1 => ResultDomain::Codec,
            _ => ResultDomain::Unknown,
        }
    }

    /// Unknown result numbers are fail-closed and therefore terminal.
    #[must_use]
    pub const fn retriability(self) -> Retriability {
        match self.known() {
            Some(known) => known.retriability(),
            None => Retriability::Terminal,
        }
    }
}

impl From<KnownResult> for ResultCode {
    fn from(value: KnownResult) -> Self {
        Self(value.raw())
    }
}
