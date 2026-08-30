//! Total, deterministic routing from a human movement request to typed intents.

use std::fmt::{Display, Formatter};

use layerx_intents::{
    BridgeDepositCredit, BridgeWithdrawRequest, BudgetCreate, BudgetDefund, BudgetFund, Intent,
    IntentError, IntentKind, LxpReceive, LxpSend,
};
use layerx_types::account::{AccountId, AccountNamespace};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, CheckpointId, IdempotencyKey};
use layerx_types::intent::{
    BudgetId, ContextHash, DepositProofId, EvmAddress, NetworkId, PayerGrantId, PeriodLength,
    ProtocolVersion, PurposeHash, RolloverPolicy, SendAuthorization, Sequence, TimestampSeconds,
    WithdrawalId,
};

/// The only source and destination kinds accepted by the movement API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointKind {
    PaxeerWallet,
    Human,
    Agent,
    AgentBudget,
}

/// One declared movement endpoint. Protocol account identities stay typed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Endpoint {
    PaxeerWallet,
    Human(AccountId),
    Agent(AccountId),
    AgentBudget(AccountId),
}

/// Refusal to assign a validated account to a semantic movement endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointConstructionError {
    WrongAccountNamespace,
}

impl Endpoint {
    /// Reconstructs a Human endpoint while enforcing its declared namespace.
    pub fn human(account: AccountId) -> Result<Self, EndpointConstructionError> {
        if account.namespace() != AccountNamespace::AgentMain {
            return Err(EndpointConstructionError::WrongAccountNamespace);
        }
        Ok(Self::Human(account))
    }

    /// Reconstructs an Agent endpoint while enforcing its declared namespace.
    pub fn agent(account: AccountId) -> Result<Self, EndpointConstructionError> {
        if account.namespace() != AccountNamespace::AgentMain {
            return Err(EndpointConstructionError::WrongAccountNamespace);
        }
        Ok(Self::Agent(account))
    }

    /// Reconstructs a managed-budget endpoint while enforcing its namespace.
    pub fn agent_budget(account: AccountId) -> Result<Self, EndpointConstructionError> {
        if account.namespace() != AccountNamespace::AgentBudget {
            return Err(EndpointConstructionError::WrongAccountNamespace);
        }
        Ok(Self::AgentBudget(account))
    }

    /// Returns the closed endpoint discriminator used in typed refusals.
    #[must_use]
    pub const fn kind(&self) -> EndpointKind {
        match self {
            Self::PaxeerWallet => EndpointKind::PaxeerWallet,
            Self::Human(_) => EndpointKind::Human,
            Self::Agent(_) => EndpointKind::Agent,
            Self::AgentBudget(_) => EndpointKind::AgentBudget,
        }
    }

    fn account(&self) -> Option<&AccountId> {
        match self {
            Self::PaxeerWallet => None,
            Self::Human(account) | Self::Agent(account) | Self::AgentBudget(account) => {
                Some(account)
            }
        }
    }

    fn has_declared_namespace(&self) -> bool {
        match self {
            Self::PaxeerWallet => true,
            Self::Human(account) | Self::Agent(account) => {
                account.namespace() == AccountNamespace::AgentMain
            }
            Self::AgentBudget(account) => account.namespace() == AccountNamespace::AgentBudget,
        }
    }
}

/// Exact material needed for an authenticated 402LXP send.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SendRoute {
    pub account_sequence: Sequence,
    pub idempotency_key: IdempotencyKey,
    pub expires_at: TimestampSeconds,
    pub context_hash: ContextHash,
    pub authorization: SendAuthorization,
    pub network_id: NetworkId,
    pub protocol_version: ProtocolVersion,
}

/// Optional first leg when a managed-agent budget does not yet exist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BudgetCreation {
    pub per_period_limit: Amount,
    pub period_length: PeriodLength,
    pub rollover: RolloverPolicy,
    pub carry_cap: Amount,
    pub purpose: PurposeHash,
    pub expiry: TimestampSeconds,
}

/// Exact relationship material for funding or returning a managed budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BudgetRoute {
    pub budget_id: BudgetId,
    pub idempotency_key: IdempotencyKey,
    pub revocation_sequence: Sequence,
    pub create: Option<BudgetCreation>,
}

/// Exact material for an agent-to-human payer-grant draw.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayerGrantRoute {
    pub payer_grant: PayerGrantId,
    pub receiver_sequence: Sequence,
    pub idempotency_key: IdempotencyKey,
    pub context_hash: ContextHash,
}

/// Custody-boundary evidence. These are the only variants allowed to use the
/// words deposit and withdrawal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CustodyRoute {
    Deposit {
        deposit_proof: DepositProofId,
        checkpoint: CheckpointId,
        reserve: AccountId,
        idempotency_key: IdempotencyKey,
    },
    Withdrawal {
        withdrawal_id: WithdrawalId,
        withdrawals_account: AccountId,
        payout_address: EvmAddress,
        idempotency_key: IdempotencyKey,
    },
}

/// Declared authority relationship. The resolver consults no ambient state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Relationship {
    Direct(SendRoute),
    ManagedBudget(BudgetRoute),
    AgentAuthorized(SendRoute),
    PayerGrant(PayerGrantRoute),
    Custody(CustodyRoute),
}

/// Complete deterministic input to route resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteRequest {
    pub source: Endpoint,
    pub destination: Endpoint,
    pub relationship: Relationship,
    pub asset: AssetId,
    pub amount: Amount,
}

impl RouteRequest {
    /// Reconstructs one wire request and proves that the complete route is valid.
    pub fn from_wire_parts(
        source: Endpoint,
        destination: Endpoint,
        relationship: Relationship,
        asset: AssetId,
        amount: Amount,
    ) -> Result<Self, RouteError> {
        let request = Self {
            source,
            destination,
            relationship,
            asset,
            amount,
        };
        RouteResolver::resolve(&request)?;
        Ok(request)
    }

    /// Encodes the complete resolver input in one canonical, versioned form.
    #[must_use]
    pub fn canonical_encode(&self) -> Vec<u8> {
        let mut out = vec![1];
        put_endpoint(&mut out, &self.source);
        put_endpoint(&mut out, &self.destination);
        match &self.relationship {
            Relationship::Direct(v) => {
                out.push(1);
                put_send(&mut out, v)
            }
            Relationship::ManagedBudget(v) => {
                out.push(2);
                out.extend(v.budget_id.bytes());
                out.extend(v.idempotency_key.bytes());
                out.extend(v.revocation_sequence.value().to_be_bytes());
                match v.create {
                    None => out.push(0),
                    Some(c) => {
                        out.push(1);
                        out.extend(c.per_period_limit.to_be_bytes());
                        out.extend(c.period_length.value().to_be_bytes());
                        out.push(match c.rollover {
                            RolloverPolicy::None => 0,
                            RolloverPolicy::Capped => 1,
                        });
                        out.extend(c.carry_cap.to_be_bytes());
                        out.extend(c.purpose.bytes());
                        out.extend(c.expiry.value().to_be_bytes());
                    }
                }
            }
            Relationship::AgentAuthorized(v) => {
                out.push(3);
                put_send(&mut out, v)
            }
            Relationship::PayerGrant(v) => {
                out.push(4);
                out.extend(v.payer_grant.bytes());
                out.extend(v.receiver_sequence.value().to_be_bytes());
                out.extend(v.idempotency_key.bytes());
                out.extend(v.context_hash.bytes());
            }
            Relationship::Custody(CustodyRoute::Deposit {
                deposit_proof,
                checkpoint,
                reserve,
                idempotency_key,
            }) => {
                out.push(5);
                out.extend(deposit_proof.bytes());
                out.extend(checkpoint.bytes());
                put_text(&mut out, reserve.canonical());
                out.extend(idempotency_key.bytes());
            }
            Relationship::Custody(CustodyRoute::Withdrawal {
                withdrawal_id,
                withdrawals_account,
                payout_address,
                idempotency_key,
            }) => {
                out.push(6);
                out.extend(withdrawal_id.bytes());
                put_text(&mut out, withdrawals_account.canonical());
                out.extend(payout_address.bytes());
                out.extend(idempotency_key.bytes());
            }
        }
        out.extend(self.asset.bytes());
        out.extend(self.amount.to_be_bytes());
        out
    }

    /// Decodes canonical resolver bytes and re-runs the total resolver.
    pub fn canonical_decode(bytes: &[u8]) -> Result<Self, RouteError> {
        let mut r = RouteWire::new(bytes);
        if r.u8()? != 1 {
            return Err(wire_error());
        }
        let source = r.endpoint()?;
        let destination = r.endpoint()?;
        let relationship = match r.u8()? {
            1 => Relationship::Direct(r.send()?),
            2 => {
                let budget_id = BudgetId::new(r.array()?);
                let idempotency_key = IdempotencyKey::new(r.array()?);
                let revocation_sequence = Sequence::from_u64(r.u64()?);
                let create = match r.u8()? {
                    0 => None,
                    1 => Some(BudgetCreation {
                        per_period_limit: Amount::from_u128(r.u128()?),
                        period_length: PeriodLength::new(r.u64()?).map_err(|_| wire_error())?,
                        rollover: match r.u8()? {
                            0 => RolloverPolicy::None,
                            1 => RolloverPolicy::Capped,
                            _ => return Err(wire_error()),
                        },
                        carry_cap: Amount::from_u128(r.u128()?),
                        purpose: PurposeHash::new(r.array()?),
                        expiry: TimestampSeconds::from_u64(r.u64()?),
                    }),
                    _ => return Err(wire_error()),
                };
                Relationship::ManagedBudget(BudgetRoute {
                    budget_id,
                    idempotency_key,
                    revocation_sequence,
                    create,
                })
            }
            3 => Relationship::AgentAuthorized(r.send()?),
            4 => Relationship::PayerGrant(PayerGrantRoute {
                payer_grant: PayerGrantId::new(r.array()?),
                receiver_sequence: Sequence::from_u64(r.u64()?),
                idempotency_key: IdempotencyKey::new(r.array()?),
                context_hash: ContextHash::new(r.array()?),
            }),
            5 => Relationship::Custody(CustodyRoute::Deposit {
                deposit_proof: DepositProofId::new(r.array()?),
                checkpoint: CheckpointId::new(r.array()?),
                reserve: AccountId::parse(&r.text(512)?).map_err(|_| wire_error())?,
                idempotency_key: IdempotencyKey::new(r.array()?),
            }),
            6 => Relationship::Custody(CustodyRoute::Withdrawal {
                withdrawal_id: WithdrawalId::new(r.array()?),
                withdrawals_account: AccountId::parse(&r.text(512)?).map_err(|_| wire_error())?,
                payout_address: EvmAddress::new(r.array()?),
                idempotency_key: IdempotencyKey::new(r.array()?),
            }),
            _ => return Err(wire_error()),
        };
        let asset = AssetId::new(r.array()?);
        let amount = Amount::from_u128(r.u128()?);
        if !r.done() {
            return Err(wire_error());
        }
        let request = Self::from_wire_parts(source, destination, relationship, asset, amount)?;
        if request.canonical_encode() != bytes {
            return Err(wire_error());
        }
        Ok(request)
    }
}

fn wire_error() -> RouteError {
    RouteError::Unavailable {
        source: EndpointKind::Human,
        destination: EndpointKind::Human,
        relationship: "invalid-wire",
    }
}
fn put_text(out: &mut Vec<u8>, v: &str) {
    out.extend((v.len() as u16).to_be_bytes());
    out.extend(v.as_bytes())
}
fn put_endpoint(out: &mut Vec<u8>, v: &Endpoint) {
    match v {
        Endpoint::PaxeerWallet => out.push(0),
        Endpoint::Human(a) => {
            out.push(1);
            put_text(out, a.canonical())
        }
        Endpoint::Agent(a) => {
            out.push(2);
            put_text(out, a.canonical())
        }
        Endpoint::AgentBudget(a) => {
            out.push(3);
            put_text(out, a.canonical())
        }
    }
}
fn put_send(out: &mut Vec<u8>, v: &SendRoute) {
    out.extend(v.account_sequence.value().to_be_bytes());
    out.extend(v.idempotency_key.bytes());
    out.extend(v.expires_at.value().to_be_bytes());
    out.extend(v.context_hash.bytes());
    out.push(v.authorization.kind() as u8);
    out.extend(v.authorization.public_key().bytes());
    out.extend(v.authorization.signature().bytes());
    out.extend(v.network_id.value().to_be_bytes());
    out.extend(v.protocol_version.value().to_be_bytes())
}
struct RouteWire<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> RouteWire<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], RouteError> {
        let end = self.at.checked_add(n).ok_or_else(wire_error)?;
        let v = self.bytes.get(self.at..end).ok_or_else(wire_error)?;
        self.at = end;
        Ok(v)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], RouteError> {
        self.take(N)?.try_into().map_err(|_| wire_error())
    }
    fn u8(&mut self) -> Result<u8, RouteError> {
        Ok(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, RouteError> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32, RouteError> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, RouteError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn u128(&mut self) -> Result<u128, RouteError> {
        Ok(u128::from_be_bytes(self.array()?))
    }
    fn text(&mut self, max: usize) -> Result<String, RouteError> {
        let n = self.u16()? as usize;
        if n == 0 || n > max {
            return Err(wire_error());
        }
        let b = self.take(n)?;
        let s = std::str::from_utf8(b).map_err(|_| wire_error())?;
        if s.chars().any(char::is_control) {
            return Err(wire_error());
        }
        Ok(s.to_owned())
    }
    fn endpoint(&mut self) -> Result<Endpoint, RouteError> {
        match self.u8()? {
            0 => Ok(Endpoint::PaxeerWallet),
            1 => Endpoint::human(AccountId::parse(&self.text(512)?).map_err(|_| wire_error())?)
                .map_err(|_| wire_error()),
            2 => Endpoint::agent(AccountId::parse(&self.text(512)?).map_err(|_| wire_error())?)
                .map_err(|_| wire_error()),
            3 => Endpoint::agent_budget(
                AccountId::parse(&self.text(512)?).map_err(|_| wire_error())?,
            )
            .map_err(|_| wire_error()),
            _ => Err(wire_error()),
        }
    }
    fn send(&mut self) -> Result<SendRoute, RouteError> {
        let account_sequence = Sequence::from_u64(self.u64()?);
        let idempotency_key = IdempotencyKey::new(self.array()?);
        let expires_at = TimestampSeconds::from_u64(self.u64()?);
        let context_hash = ContextHash::new(self.array()?);
        let kind = match self.u8()? {
            1 => layerx_types::intent::SendAuthorizationKind::Owner,
            2 => layerx_types::intent::SendAuthorizationKind::SessionKey,
            3 => layerx_types::intent::SendAuthorizationKind::DelegatedCapability,
            4 => layerx_types::intent::SendAuthorizationKind::BudgetAllowance,
            5 => layerx_types::intent::SendAuthorizationKind::Escrow,
            6 => layerx_types::intent::SendAuthorizationKind::ProtocolModule,
            _ => return Err(wire_error()),
        };
        let authorization = SendAuthorization::new(
            kind,
            layerx_types::intent::PublicKey::new(self.array()?),
            layerx_types::intent::AuthorizationSignature::new(self.array()?),
        );
        let network_id = NetworkId::new(self.u32()?).map_err(|_| wire_error())?;
        let protocol_version = ProtocolVersion::new(self.u16()?).map_err(|_| wire_error())?;
        Ok(SendRoute {
            account_sequence,
            idempotency_key,
            expires_at,
            context_hash,
            authorization,
            network_id,
            protocol_version,
        })
    }
    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// Vocabulary permitted in APIs, logs and user-facing copy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MovementTerm {
    Deposit,
    Withdrawal,
    Fund,
    Allocate,
    Return,
    Transfer,
}

impl MovementTerm {
    /// Returns the exact lowercase contract term for APIs and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deposit => "deposit",
            Self::Withdrawal => "withdrawal",
            Self::Fund => "fund",
            Self::Allocate => "allocate",
            Self::Return => "return",
            Self::Transfer => "transfer",
        }
    }

    /// Returns the single user action covering every movement route.
    #[must_use]
    pub const fn user_action() -> &'static str {
        "Move money"
    }

    /// Whether this term is reserved to a custody crossing.
    #[must_use]
    pub const fn is_custody_boundary(self) -> bool {
        matches!(self, Self::Deposit | Self::Withdrawal)
    }
}

impl Display for MovementTerm {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Exact protocol mechanism selected for one leg.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mechanism {
    BudgetCreate,
    BudgetFund,
    Send,
    BudgetDefund,
    ReceiveUnderPayerGrant,
    BridgeDepositCredit,
    BridgeWithdrawRequest,
}

/// One automatically derived, executable journey leg.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteLeg {
    term: MovementTerm,
    mechanism: Mechanism,
    intent: Intent,
}

impl RouteLeg {
    /// Returns the movement vocabulary term for APIs, logs and copy.
    #[must_use]
    pub const fn term(&self) -> MovementTerm {
        self.term
    }

    /// Returns the selected protocol mechanism.
    #[must_use]
    pub const fn mechanism(&self) -> Mechanism {
        self.mechanism
    }

    /// Returns the typed intent derived for this leg.
    #[must_use]
    pub const fn intent(&self) -> &Intent {
        &self.intent
    }
}

/// A complete route. Construction is atomic: no route exists on refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Route {
    legs: Vec<RouteLeg>,
}

impl Route {
    /// Returns the non-empty ordered leg list.
    #[must_use]
    pub fn legs(&self) -> &[RouteLeg] {
        &self.legs
    }

    /// Returns the product-level action name shared by every route.
    #[must_use]
    pub const fn user_action() -> &'static str {
        MovementTerm::user_action()
    }
}

/// Total resolver refusal. No variant contains or exposes a partial route.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouteError {
    Unavailable {
        source: EndpointKind,
        destination: EndpointKind,
        relationship: &'static str,
    },
    InvalidIntent(IntentError),
}

impl Display for RouteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable {
                source,
                destination,
                relationship,
            } => write!(
                formatter,
                "movement route unavailable: {source:?} to {destination:?} via {relationship}"
            ),
            Self::InvalidIntent(error) => write!(formatter, "movement intent refused: {error:?}"),
        }
    }
}

impl std::error::Error for RouteError {}

impl From<IntentError> for RouteError {
    fn from(value: IntentError) -> Self {
        Self::InvalidIntent(value)
    }
}

/// Human-changeable settings surfaces named by typed movement refusals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeSurface {
    Policy,
    Budget,
    Capability,
}

impl ChangeSurface {
    /// Returns the stable UI link for the setting the human can change.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Policy => "/settings/policies",
            Self::Budget => "/agents/budgets",
            Self::Capability => "/agents/capabilities",
        }
    }
}

/// Authority that refused a movement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitSource {
    Policy,
    Budget,
    Capability,
    Protocol,
}

/// Plain-language refusal with the exact limit and an actionable link only
/// where the limit belongs to the human.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LimitRefusal {
    source: LimitSource,
    limit: String,
    change_surface: Option<ChangeSurface>,
}

impl LimitRefusal {
    /// Constructs a typed refusal, rejecting empty limit names and mismatched
    /// change links.
    ///
    /// # Errors
    ///
    /// Refuses an empty limit or a change surface that does not own the limit.
    pub fn new(
        source: LimitSource,
        limit: impl Into<String>,
        change_surface: Option<ChangeSurface>,
    ) -> Result<Self, LimitRefusalError> {
        let limit = limit.into();
        if limit.trim().is_empty() {
            return Err(LimitRefusalError::EmptyLimit);
        }
        let link_matches = matches!(
            (source, change_surface),
            (LimitSource::Policy, Some(ChangeSurface::Policy))
                | (LimitSource::Budget, Some(ChangeSurface::Budget))
                | (LimitSource::Capability, Some(ChangeSurface::Capability))
                | (LimitSource::Protocol, None)
        );
        if !link_matches {
            return Err(LimitRefusalError::MismatchedChangeSurface);
        }
        Ok(Self {
            source,
            limit,
            change_surface,
        })
    }

    /// Returns the refusing authority.
    #[must_use]
    pub const fn source(&self) -> LimitSource {
        self.source
    }

    /// Returns the exact limit name supplied by the refusing boundary.
    #[must_use]
    pub fn limit(&self) -> &str {
        &self.limit
    }

    /// Returns the settings link only when the human owns the limit.
    #[must_use]
    pub fn change_path(&self) -> Option<&'static str> {
        self.change_surface.map(ChangeSurface::path)
    }

    /// Renders the refusal without protocol result-code leakage.
    #[must_use]
    pub fn plain_language(&self) -> String {
        let owner = match self.source {
            LimitSource::Policy => "policy",
            LimitSource::Budget => "budget",
            LimitSource::Capability => "capability",
            LimitSource::Protocol => "protocol",
        };
        format!("The {owner} limit '{}' refused this movement.", self.limit)
    }
}

/// Invalid typed refusal construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitRefusalError {
    EmptyLimit,
    MismatchedChangeSurface,
}

impl Display for LimitRefusalError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyLimit => formatter.write_str("movement refusal limit is empty"),
            Self::MismatchedChangeSurface => {
                formatter.write_str("movement refusal change surface does not own the limit")
            }
        }
    }
}

impl std::error::Error for LimitRefusalError {}

/// Stateless deterministic resolver.
#[derive(Clone, Copy, Debug, Default)]
pub struct RouteResolver;

impl RouteResolver {
    /// Resolves every declared input combination to a complete route or one
    /// typed unavailable refusal. No I/O, time, randomness or global state is
    /// consulted.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable route or the exact typed-intent refusal.
    #[allow(clippy::too_many_lines)]
    pub fn resolve(request: &RouteRequest) -> Result<Route, RouteError> {
        if !request.source.has_declared_namespace() || !request.destination.has_declared_namespace()
        {
            return Err(unavailable(request));
        }
        match (
            &request.source,
            &request.destination,
            request.relationship.clone(),
        ) {
            (
                Endpoint::PaxeerWallet,
                Endpoint::Human(recipient),
                Relationship::Custody(CustodyRoute::Deposit {
                    deposit_proof,
                    checkpoint,
                    reserve,
                    idempotency_key,
                }),
            ) if reserve.namespace() == AccountNamespace::SystemPaxeerReserve => Ok(one_leg(
                MovementTerm::Deposit,
                Mechanism::BridgeDepositCredit,
                BridgeDepositCredit::new(
                    deposit_proof,
                    checkpoint,
                    reserve,
                    recipient.clone(),
                    request.asset,
                    request.amount,
                    idempotency_key,
                )?,
                IntentKind::BridgeDepositCredit,
            )),
            (
                Endpoint::Human(owner),
                Endpoint::PaxeerWallet,
                Relationship::Custody(CustodyRoute::Withdrawal {
                    withdrawal_id,
                    withdrawals_account,
                    payout_address,
                    idempotency_key,
                }),
            ) if withdrawals_account.namespace() == AccountNamespace::SystemPaxeerWithdrawals => {
                Ok(one_leg(
                    MovementTerm::Withdrawal,
                    Mechanism::BridgeWithdrawRequest,
                    BridgeWithdrawRequest::new(
                        withdrawal_id,
                        owner.clone(),
                        withdrawals_account,
                        payout_address,
                        request.asset,
                        request.amount,
                        idempotency_key,
                    )?,
                    IntentKind::BridgeWithdrawRequest,
                ))
            }
            (
                Endpoint::Human(owner),
                Endpoint::AgentBudget(budget_account),
                Relationship::ManagedBudget(route),
            ) => budget_funding(request, owner, budget_account, route),
            (
                Endpoint::AgentBudget(budget_account),
                Endpoint::Human(owner),
                Relationship::ManagedBudget(route),
            ) => Ok(one_leg(
                MovementTerm::Return,
                Mechanism::BudgetDefund,
                BudgetDefund::new(
                    route.budget_id,
                    budget_account.clone(),
                    owner.clone(),
                    request.asset,
                    request.amount,
                    route.revocation_sequence,
                    route.idempotency_key,
                )?,
                IntentKind::BudgetDefund,
            )),
            (Endpoint::Agent(from), Endpoint::Human(to), Relationship::PayerGrant(route)) => {
                Ok(one_leg(
                    MovementTerm::Return,
                    Mechanism::ReceiveUnderPayerGrant,
                    LxpReceive::new(
                        from.clone(),
                        to.clone(),
                        request.asset,
                        request.amount,
                        route.payer_grant,
                        route.receiver_sequence,
                        route.idempotency_key,
                        route.context_hash,
                    )?,
                    IntentKind::LxpReceive,
                ))
            }
            (source, destination, Relationship::Direct(route))
                if direct_term(source.kind(), destination.kind()).is_some() =>
            {
                send(request, source, destination, route, false)
            }
            (source, destination, Relationship::AgentAuthorized(route))
                if agent_authorized_term(source.kind(), destination.kind()).is_some() =>
            {
                send(request, source, destination, route, true)
            }
            _ => Err(unavailable(request)),
        }
    }
}

fn budget_funding(
    request: &RouteRequest,
    owner: &AccountId,
    budget_account: &AccountId,
    route: BudgetRoute,
) -> Result<Route, RouteError> {
    let mut legs = Vec::with_capacity(if route.create.is_some() { 2 } else { 1 });
    if let Some(create) = route.create {
        let intent = BudgetCreate::new(
            route.budget_id,
            owner.clone(),
            budget_account.clone(),
            request.asset,
            create.per_period_limit,
            create.period_length,
            create.rollover,
            create.carry_cap,
            create.purpose,
            create.expiry,
        )?;
        legs.push(RouteLeg {
            term: MovementTerm::Fund,
            mechanism: Mechanism::BudgetCreate,
            intent: Intent::v1(IntentKind::BudgetCreate(intent)),
        });
    }
    let fund = BudgetFund::new(
        route.budget_id,
        owner.clone(),
        budget_account.clone(),
        request.asset,
        request.amount,
        route.idempotency_key,
    )?;
    legs.push(RouteLeg {
        term: MovementTerm::Fund,
        mechanism: Mechanism::BudgetFund,
        intent: Intent::v1(IntentKind::BudgetFund(fund)),
    });
    Ok(Route { legs })
}

fn send(
    request: &RouteRequest,
    source: &Endpoint,
    destination: &Endpoint,
    route: SendRoute,
    agent_authorized: bool,
) -> Result<Route, RouteError> {
    let from = source.account().ok_or_else(|| unavailable(request))?;
    let to = destination.account().ok_or_else(|| unavailable(request))?;
    let term = if agent_authorized {
        agent_authorized_term(source.kind(), destination.kind())
    } else {
        direct_term(source.kind(), destination.kind())
    }
    .ok_or_else(|| unavailable(request))?;
    Ok(one_leg(
        term,
        Mechanism::Send,
        LxpSend::new(
            from.clone(),
            to.clone(),
            request.asset,
            request.amount,
            route.account_sequence,
            route.idempotency_key,
            route.expires_at,
            route.context_hash,
            route.authorization,
            route.network_id,
            route.protocol_version,
        )?,
        IntentKind::LxpSend,
    ))
}

const fn direct_term(source: EndpointKind, destination: EndpointKind) -> Option<MovementTerm> {
    match (source, destination) {
        (EndpointKind::Human | EndpointKind::Agent, EndpointKind::Agent)
        | (EndpointKind::Human, EndpointKind::Human) => Some(MovementTerm::Transfer),
        (EndpointKind::AgentBudget, EndpointKind::Agent) => Some(MovementTerm::Allocate),
        _ => None,
    }
}

const fn agent_authorized_term(
    source: EndpointKind,
    destination: EndpointKind,
) -> Option<MovementTerm> {
    match (source, destination) {
        (EndpointKind::Agent, EndpointKind::Human) => Some(MovementTerm::Return),
        (EndpointKind::Agent, EndpointKind::Agent) => Some(MovementTerm::Transfer),
        (EndpointKind::AgentBudget, EndpointKind::Agent) => Some(MovementTerm::Allocate),
        _ => None,
    }
}

fn one_leg<T>(
    term: MovementTerm,
    mechanism: Mechanism,
    value: T,
    wrap: fn(T) -> IntentKind,
) -> Route {
    Route {
        legs: vec![RouteLeg {
            term,
            mechanism,
            intent: Intent::v1(wrap(value)),
        }],
    }
}

fn unavailable(request: &RouteRequest) -> RouteError {
    RouteError::Unavailable {
        source: request.source.kind(),
        destination: request.destination.kind(),
        relationship: match request.relationship {
            Relationship::Direct(_) => "direct",
            Relationship::ManagedBudget(_) => "managed-budget",
            Relationship::AgentAuthorized(_) => "agent-authorized",
            Relationship::PayerGrant(_) => "payer-grant",
            Relationship::Custody(CustodyRoute::Deposit { .. }) => "custody-deposit",
            Relationship::Custody(CustodyRoute::Withdrawal { .. }) => "custody-withdrawal",
        },
    }
}
