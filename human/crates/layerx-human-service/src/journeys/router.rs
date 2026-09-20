//! One stated intent, one deterministic cross-domain plan, one signature set.
//!
//! A human states an intent once: move an amount of one asset from a source
//! endpoint to a destination endpoint, where either end may be the bound Paxeer
//! wallet or a LayerX account, agent or agent budget. [`plan`] turns that intent
//! plus one observed state snapshot into a single ordered plan spanning both
//! domains. Planning is pure: it reads no clock, no randomness and performs no
//! input/output, so the same intent over the same observed state always yields
//! byte-identical canonical plan bytes and therefore the same plan digest.
//!
//! # Canonical tie-breaking
//!
//! Every feasible candidate is ordered by, in strict priority:
//!
//! 1. fewest legs,
//! 2. lowest total fee,
//! 3. lexicographic order of the candidate's mechanism-label sequence,
//! 4. lexicographic order of the candidate's canonical plan bytes.
//!
//! Rules 1 to 3 are the documented routing rule. Rule 4 is the residual
//! tie-break that makes selection total, and it is the only rule an advisor may
//! displace: see [`RouteAdvisor`].
//!
//! # Top-ups
//!
//! A top-up leg — moving funds from the Paxeer side into LayerX, or funding an
//! agent budget, so that a payment becomes possible — is emitted only when the
//! observed state contains a user-signed allowance whose scope and remaining
//! caps cover that exact leg. Each top-up leg records the allowance it consumes.
//! Where no allowance covers the shortfall the plan is refused with
//! [`Refusal::TopUpNotAuthorized`]. No allowance is ever widened or synthesised.
//!
//! # Digest binding
//!
//! The plan digest commits to the intent and to every leg. Each leg's agent
//! action key and signing context are derived from that digest, so a signed plan
//! cannot be re-routed: altering any leg changes the digest, which changes every
//! action key and the engine idempotency key.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

use layerx_agent_api::identity::{AgentDid, AuthorityRef};
use layerx_types::account::{AccountId, AccountNamespace};
use layerx_types::amount::Amount;
use layerx_types::ids::AssetId;
use layerx_types::intent::{EvmAddress, TimestampSeconds};
use sha2::{Digest as _, Sha256};

use crate::custody::{KeyId, Operation};
use crate::notify::JourneyId;

use super::resolver::put_endpoint;
use super::{
    Endpoint, EndpointKind, JourneyKind, JourneyLeg, JourneyPlan, Mechanism, MovementTerm,
    Relationship, RouteError, RouteRequest, RouteResolver,
};

const PLAN_VERSION: u8 = 1;
const PLAN_DOMAIN: &[u8] = b"layerx-human-unified-plan/v1";
const ACTION_DOMAIN: &[u8] = b"layerx-human-unified-action/v1";
const CONTEXT_DOMAIN: &[u8] = b"layerx-human-unified-context/v1";
const MAXIMUM_PLAN_LEGS: usize = 12;
const MAXIMUM_CANDIDATES: usize = 32;
const MAXIMUM_NOTES: usize = 8;
const NOTE_LIMIT: usize = 256;

/// The two settlement domains one plan can span.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Domain {
    /// Paxeer EVM execution, reached through the bound wallet.
    Paxeer,
    /// The LayerX interaction layer, reached through typed intents.
    LayerX,
}

impl Domain {
    /// Returns the stable lowercase label used in APIs and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paxeer => "paxeer",
            Self::LayerX => "layerx",
        }
    }
}

/// The exact mechanism selected for one planned leg in either domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegMechanism {
    /// A Paxeer-side custody deposit into the LayerX custody precompile.
    PaxeerCustodyDeposit,
    /// The Paxeer-side settlement that completes an accepted withdrawal.
    PaxeerWithdrawFinalise,
    /// A LayerX protocol mechanism already resolved by [`RouteResolver`].
    Protocol(Mechanism),
}

impl LegMechanism {
    /// Returns the settlement domain that executes this mechanism.
    #[must_use]
    pub const fn domain(self) -> Domain {
        match self {
            Self::PaxeerCustodyDeposit | Self::PaxeerWithdrawFinalise => Domain::Paxeer,
            Self::Protocol(_) => Domain::LayerX,
        }
    }

    /// Returns the stable label that orders mechanisms lexicographically.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::PaxeerCustodyDeposit => "paxeer-custody-deposit",
            Self::PaxeerWithdrawFinalise => "paxeer-withdraw-finalise",
            Self::Protocol(Mechanism::BudgetCreate) => "budget-create",
            Self::Protocol(Mechanism::BudgetFund) => "budget-fund",
            Self::Protocol(Mechanism::Send) => "send",
            Self::Protocol(Mechanism::BudgetDefund) => "budget-defund",
            Self::Protocol(Mechanism::ReceiveUnderPayerGrant) => "receive-under-payer-grant",
            Self::Protocol(Mechanism::BridgeDepositCredit) => "bridge-deposit-credit",
            Self::Protocol(Mechanism::BridgeWithdrawRequest) => "bridge-withdraw-request",
        }
    }

    /// Returns the canonical wire discriminator.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::PaxeerCustodyDeposit => 1,
            Self::PaxeerWithdrawFinalise => 2,
            Self::Protocol(Mechanism::BudgetCreate) => 3,
            Self::Protocol(Mechanism::BudgetFund) => 4,
            Self::Protocol(Mechanism::Send) => 5,
            Self::Protocol(Mechanism::BudgetDefund) => 6,
            Self::Protocol(Mechanism::ReceiveUnderPayerGrant) => 7,
            Self::Protocol(Mechanism::BridgeDepositCredit) => 8,
            Self::Protocol(Mechanism::BridgeWithdrawRequest) => 9,
        }
    }

    /// Returns the LayerX protocol mechanism when this leg is a typed intent.
    #[must_use]
    pub const fn protocol(self) -> Option<Mechanism> {
        match self {
            Self::Protocol(mechanism) => Some(mechanism),
            Self::PaxeerCustodyDeposit | Self::PaxeerWithdrawFinalise => None,
        }
    }
}

/// The authority that must sign one planned leg.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequiredAuthority {
    /// The bound Paxeer wallet key signs this leg.
    PaxeerWalletKey,
    /// The account owner signs this leg directly.
    AccountOwner,
    /// A previously signed allowance already authorises this leg.
    Allowance(AllowanceId),
}

impl RequiredAuthority {
    const fn code(self) -> u8 {
        match self {
            Self::PaxeerWalletKey => 1,
            Self::AccountOwner => 2,
            Self::Allowance(_) => 3,
        }
    }
}

/// The stable identity of one user-signed allowance.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AllowanceId([u8; 32]);

impl AllowanceId {
    /// Constructs an allowance identity from its exact bytes.
    ///
    /// # Errors
    ///
    /// Refuses the reserved all-zero identity.
    pub fn new(bytes: [u8; 32]) -> Result<Self, Refusal> {
        if bytes == [0; 32] {
            return Err(Refusal::InvalidAllowance);
        }
        Ok(Self(bytes))
    }

    /// Returns the exact identity bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

/// The closed vocabulary of allowance primitives a top-up may consume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllowanceKind {
    /// An `lxp_authority` budget allowance with per-activity and period caps.
    BudgetAllowance,
    /// An `lxp_payer_grant` authorising an agent to draw on a payer.
    PayerGrant,
    /// A delegated capability issued to an agent by its owner.
    DelegatedCapability,
}

impl AllowanceKind {
    /// Returns the stable lowercase label used in APIs and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BudgetAllowance => "budget-allowance",
            Self::PayerGrant => "payer-grant",
            Self::DelegatedCapability => "delegated-capability",
        }
    }

    const fn code(self) -> u8 {
        match self {
            Self::BudgetAllowance => 1,
            Self::PayerGrant => 2,
            Self::DelegatedCapability => 3,
        }
    }
}

/// The exact movement one allowance was signed for. Scope match is equality:
/// an allowance never stretches to a neighbouring endpoint, asset or mechanism.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllowanceScope {
    source: Endpoint,
    destination: Endpoint,
    asset: AssetId,
    mechanism: LegMechanism,
}

impl AllowanceScope {
    /// Declares the exact movement the signed allowance covers.
    #[must_use]
    pub const fn new(
        source: Endpoint,
        destination: Endpoint,
        asset: AssetId,
        mechanism: LegMechanism,
    ) -> Self {
        Self {
            source,
            destination,
            asset,
            mechanism,
        }
    }

    /// Returns the scoped source endpoint.
    #[must_use]
    pub const fn source(&self) -> &Endpoint {
        &self.source
    }

    /// Returns the scoped destination endpoint.
    #[must_use]
    pub const fn destination(&self) -> &Endpoint {
        &self.destination
    }

    /// Returns the scoped asset.
    #[must_use]
    pub const fn asset(&self) -> AssetId {
        self.asset
    }

    /// Returns the scoped mechanism.
    #[must_use]
    pub const fn mechanism(&self) -> LegMechanism {
        self.mechanism
    }
}

/// One allowance the user has already signed, with its remaining headroom as
/// observed. The router only ever reads these bounds; it never widens them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedAllowance {
    id: AllowanceId,
    kind: AllowanceKind,
    scope: AllowanceScope,
    per_activity_cap: Amount,
    remaining_total: Amount,
    expires_at: TimestampSeconds,
}

impl SignedAllowance {
    /// Records one observed signed allowance.
    ///
    /// # Errors
    ///
    /// Refuses a zero per-activity cap, which authorises nothing.
    pub fn new(
        id: AllowanceId,
        kind: AllowanceKind,
        scope: AllowanceScope,
        per_activity_cap: Amount,
        remaining_total: Amount,
        expires_at: TimestampSeconds,
    ) -> Result<Self, Refusal> {
        if per_activity_cap.value() == 0 {
            return Err(Refusal::InvalidAllowance);
        }
        Ok(Self {
            id,
            kind,
            scope,
            per_activity_cap,
            remaining_total,
            expires_at,
        })
    }

    /// Returns the allowance identity.
    #[must_use]
    pub const fn id(&self) -> AllowanceId {
        self.id
    }

    /// Returns the allowance primitive that issued this authority.
    #[must_use]
    pub const fn kind(&self) -> AllowanceKind {
        self.kind
    }

    /// Returns the exact scope the allowance was signed for.
    #[must_use]
    pub const fn scope(&self) -> &AllowanceScope {
        &self.scope
    }

    /// Returns the per-activity cap.
    #[must_use]
    pub const fn per_activity_cap(&self) -> Amount {
        self.per_activity_cap
    }

    /// Returns the remaining total headroom in the current period.
    #[must_use]
    pub const fn remaining_total(&self) -> Amount {
        self.remaining_total
    }

    /// Returns the observed expiry.
    #[must_use]
    pub const fn expires_at(&self) -> TimestampSeconds {
        self.expires_at
    }

    fn covers(
        &self,
        source: &Endpoint,
        destination: &Endpoint,
        asset: AssetId,
        mechanism: LegMechanism,
        amount: u128,
        observed_at: u64,
    ) -> bool {
        &self.scope.source == source
            && &self.scope.destination == destination
            && self.scope.asset == asset
            && self.scope.mechanism == mechanism
            && observed_at <= self.expires_at.value()
            && amount <= self.per_activity_cap.value()
            && amount <= self.remaining_total.value()
    }
}

/// One observed spendable balance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BalanceEntry {
    endpoint: Endpoint,
    asset: AssetId,
    available: Amount,
}

impl BalanceEntry {
    /// Records one observed spendable balance.
    #[must_use]
    pub const fn new(endpoint: Endpoint, asset: AssetId, available: Amount) -> Self {
        Self {
            endpoint,
            asset,
            available,
        }
    }

    /// Returns the endpoint holding the balance.
    #[must_use]
    pub const fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Returns the asset of the balance.
    #[must_use]
    pub const fn asset(&self) -> AssetId {
        self.asset
    }

    /// Returns the observed spendable amount.
    #[must_use]
    pub const fn available(&self) -> Amount {
        self.available
    }
}

/// One observed managed-budget relationship.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetBinding {
    budget: AccountId,
    owner: AccountId,
}

impl BudgetBinding {
    /// Records the owner a managed budget belongs to.
    ///
    /// # Errors
    ///
    /// Refuses accounts outside the budget and main namespaces.
    pub fn new(budget: AccountId, owner: AccountId) -> Result<Self, Refusal> {
        if budget.namespace() != AccountNamespace::AgentBudget
            || owner.namespace() != AccountNamespace::AgentMain
        {
            return Err(Refusal::BudgetNotBound);
        }
        Ok(Self { budget, owner })
    }

    /// Returns the managed budget account.
    #[must_use]
    pub const fn budget(&self) -> &AccountId {
        &self.budget
    }

    /// Returns the owning main account.
    #[must_use]
    pub const fn owner(&self) -> &AccountId {
        &self.owner
    }
}

/// The observed custody boundary: the bound wallet and the system accounts the
/// deposit and withdrawal mechanisms settle through.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustodyContext {
    wallet: EvmAddress,
    reserve: AccountId,
    withdrawals: AccountId,
}

impl CustodyContext {
    /// Records the observed custody boundary.
    ///
    /// # Errors
    ///
    /// Refuses system accounts outside the reserve and withdrawal namespaces.
    pub fn new(
        wallet: EvmAddress,
        reserve: AccountId,
        withdrawals: AccountId,
    ) -> Result<Self, Refusal> {
        if reserve.namespace() != AccountNamespace::SystemPaxeerReserve
            || withdrawals.namespace() != AccountNamespace::SystemPaxeerWithdrawals
        {
            return Err(Refusal::WalletNotBound);
        }
        Ok(Self {
            wallet,
            reserve,
            withdrawals,
        })
    }

    /// Returns the bound Paxeer wallet address.
    #[must_use]
    pub const fn wallet(&self) -> EvmAddress {
        self.wallet
    }

    /// Returns the custody reserve account credits settle from.
    #[must_use]
    pub const fn reserve(&self) -> &AccountId {
        &self.reserve
    }

    /// Returns the withdrawals account requests settle through.
    #[must_use]
    pub const fn withdrawals(&self) -> &AccountId {
        &self.withdrawals
    }
}

/// The deterministic fee quoted for each mechanism the plan may use. A
/// mechanism with no declared fee is refused rather than silently defaulted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeSchedule {
    entries: Vec<(LegMechanism, u128)>,
}

impl FeeSchedule {
    /// Records the quoted fee per mechanism in canonical mechanism order.
    ///
    /// # Errors
    ///
    /// Refuses a mechanism quoted more than once.
    pub fn new(entries: Vec<(LegMechanism, u128)>) -> Result<Self, Refusal> {
        let mut entries = entries;
        entries.sort_by_key(|(mechanism, _)| mechanism.code());
        let mut seen = BTreeSet::new();
        if entries
            .iter()
            .any(|(mechanism, _)| !seen.insert(mechanism.code()))
        {
            return Err(Refusal::DuplicateObservation);
        }
        Ok(Self { entries })
    }

    /// Returns the quoted mechanisms in canonical order.
    #[must_use]
    pub fn entries(&self) -> &[(LegMechanism, u128)] {
        &self.entries
    }

    fn fee(&self, mechanism: LegMechanism) -> Result<u128, Refusal> {
        self.entries
            .iter()
            .find(|(quoted, _)| *quoted == mechanism)
            .map(|(_, fee)| *fee)
            .ok_or(Refusal::FeeUnknown {
                mechanism: mechanism.label(),
            })
    }
}

/// The complete deterministic snapshot planning reads. Every collection is
/// stored in canonical order, so a permuted observation yields an identical
/// plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedState {
    observed_at: TimestampSeconds,
    owner: AccountId,
    custody: Option<CustodyContext>,
    balances: Vec<BalanceEntry>,
    allowances: Vec<SignedAllowance>,
    budgets: Vec<BudgetBinding>,
    fees: FeeSchedule,
}

impl ObservedState {
    /// Canonicalises one observation snapshot.
    ///
    /// # Errors
    ///
    /// Refuses an owner outside the main namespace and any duplicated balance,
    /// allowance or budget observation.
    pub fn new(
        observed_at: TimestampSeconds,
        owner: AccountId,
        custody: Option<CustodyContext>,
        balances: Vec<BalanceEntry>,
        allowances: Vec<SignedAllowance>,
        budgets: Vec<BudgetBinding>,
        fees: FeeSchedule,
    ) -> Result<Self, Refusal> {
        if owner.namespace() != AccountNamespace::AgentMain {
            return Err(Refusal::InvalidOwner);
        }
        let mut balances = balances;
        balances.sort_by(|left, right| {
            endpoint_key(&left.endpoint)
                .cmp(&endpoint_key(&right.endpoint))
                .then_with(|| left.asset.bytes().cmp(&right.asset.bytes()))
        });
        let mut balance_keys = BTreeSet::new();
        if balances
            .iter()
            .any(|entry| !balance_keys.insert((endpoint_key(&entry.endpoint), entry.asset.bytes())))
        {
            return Err(Refusal::DuplicateObservation);
        }
        let mut allowances = allowances;
        allowances.sort_by_key(|allowance| allowance.id);
        let mut allowance_keys = BTreeSet::new();
        if allowances
            .iter()
            .any(|allowance| !allowance_keys.insert(allowance.id))
        {
            return Err(Refusal::DuplicateObservation);
        }
        let mut budgets = budgets;
        budgets.sort_by(|left, right| left.budget.canonical().cmp(right.budget.canonical()));
        let mut budget_keys = BTreeSet::new();
        if budgets
            .iter()
            .any(|binding| !budget_keys.insert(binding.budget.canonical().to_owned()))
        {
            return Err(Refusal::DuplicateObservation);
        }
        Ok(Self {
            observed_at,
            owner,
            custody,
            balances,
            allowances,
            budgets,
            fees,
        })
    }

    /// Returns the observation timestamp the plan is evaluated against.
    #[must_use]
    pub const fn observed_at(&self) -> TimestampSeconds {
        self.observed_at
    }

    /// Returns the acting human's main account.
    #[must_use]
    pub const fn owner(&self) -> &AccountId {
        &self.owner
    }

    /// Returns the observed custody boundary when a wallet is bound.
    #[must_use]
    pub const fn custody(&self) -> Option<&CustodyContext> {
        self.custody.as_ref()
    }

    /// Returns the observed allowances in canonical order.
    #[must_use]
    pub fn allowances(&self) -> &[SignedAllowance] {
        &self.allowances
    }

    /// Returns the observed spendable amount. An unobserved balance is zero.
    #[must_use]
    pub fn available(&self, endpoint: &Endpoint, asset: AssetId) -> Amount {
        self.balances
            .iter()
            .find(|entry| &entry.endpoint == endpoint && entry.asset == asset)
            .map_or(Amount::from_u128(0), |entry| entry.available)
    }

    fn home(&self) -> Endpoint {
        Endpoint::Human(self.owner.clone())
    }

    fn budget_is_owned(&self, budget: &AccountId) -> bool {
        self.budgets
            .iter()
            .any(|binding| &binding.budget == budget && binding.owner == self.owner)
    }
}

/// The user's declared bounds on one stated intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Constraints {
    deadline: TimestampSeconds,
    max_fee: u128,
    allow_top_up: bool,
}

impl Constraints {
    /// Declares the deadline, fee ceiling and whether automatic top-ups inside
    /// already-signed allowances are permitted at all.
    #[must_use]
    pub const fn new(deadline: TimestampSeconds, max_fee: u128, allow_top_up: bool) -> Self {
        Self {
            deadline,
            max_fee,
            allow_top_up,
        }
    }

    /// Returns the latest observation the plan stays valid for.
    #[must_use]
    pub const fn deadline(&self) -> TimestampSeconds {
        self.deadline
    }

    /// Returns the total fee ceiling across every leg.
    #[must_use]
    pub const fn max_fee(&self) -> u128 {
        self.max_fee
    }

    /// Returns whether automatic top-ups may be planned.
    #[must_use]
    pub const fn allow_top_up(&self) -> bool {
        self.allow_top_up
    }
}

/// One intent, stated once, in either or both domains.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnifiedIntent {
    source: Endpoint,
    destination: Endpoint,
    asset: AssetId,
    amount: Amount,
    constraints: Constraints,
}

impl UnifiedIntent {
    /// States one movement intent.
    ///
    /// # Errors
    ///
    /// Refuses a zero amount and an intent whose two ends are the same endpoint.
    pub fn new(
        source: Endpoint,
        destination: Endpoint,
        asset: AssetId,
        amount: Amount,
        constraints: Constraints,
    ) -> Result<Self, Refusal> {
        if amount.value() == 0 {
            return Err(Refusal::ZeroAmount);
        }
        if source == destination {
            return Err(Refusal::EndpointsIdentical);
        }
        Ok(Self {
            source,
            destination,
            asset,
            amount,
            constraints,
        })
    }

    /// Returns the source endpoint.
    #[must_use]
    pub const fn source(&self) -> &Endpoint {
        &self.source
    }

    /// Returns the destination endpoint.
    #[must_use]
    pub const fn destination(&self) -> &Endpoint {
        &self.destination
    }

    /// Returns the asset moved.
    #[must_use]
    pub const fn asset(&self) -> AssetId {
        self.asset
    }

    /// Returns the amount that must be delivered to the destination.
    #[must_use]
    pub const fn amount(&self) -> Amount {
        self.amount
    }

    /// Returns the declared constraints.
    #[must_use]
    pub const fn constraints(&self) -> Constraints {
        self.constraints
    }

    fn canonical_encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_endpoint(&mut out, &self.source);
        put_endpoint(&mut out, &self.destination);
        out.extend(self.asset.bytes());
        out.extend(self.amount.to_be_bytes());
        out.extend(self.constraints.deadline.value().to_be_bytes());
        out.extend(self.constraints.max_fee.to_be_bytes());
        out.push(u8::from(self.constraints.allow_top_up));
        out
    }
}

/// The allowance one top-up leg consumes, and how much of it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TopUpRecord {
    allowance: AllowanceId,
    kind: AllowanceKind,
    consumed: Amount,
}

impl TopUpRecord {
    /// Returns the consumed allowance identity.
    #[must_use]
    pub const fn allowance(&self) -> AllowanceId {
        self.allowance
    }

    /// Returns the allowance primitive consumed.
    #[must_use]
    pub const fn kind(&self) -> AllowanceKind {
        self.kind
    }

    /// Returns the amount drawn against the allowance.
    #[must_use]
    pub const fn consumed(&self) -> Amount {
        self.consumed
    }
}

/// One ordered leg of a unified plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedLeg {
    index: usize,
    mechanism: LegMechanism,
    term: MovementTerm,
    source: Endpoint,
    destination: Endpoint,
    asset: AssetId,
    amount: Amount,
    fee: u128,
    authority: RequiredAuthority,
    top_up: Option<TopUpRecord>,
}

impl PlannedLeg {
    /// Returns the zero-based position of this leg in the plan.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Returns the mechanism selected for this leg.
    #[must_use]
    pub const fn mechanism(&self) -> LegMechanism {
        self.mechanism
    }

    /// Returns the settlement domain executing this leg.
    #[must_use]
    pub const fn domain(&self) -> Domain {
        self.mechanism.domain()
    }

    /// Returns the movement vocabulary term for APIs, logs and copy.
    #[must_use]
    pub const fn term(&self) -> MovementTerm {
        self.term
    }

    /// Returns the leg source endpoint.
    #[must_use]
    pub const fn source(&self) -> &Endpoint {
        &self.source
    }

    /// Returns the leg destination endpoint.
    #[must_use]
    pub const fn destination(&self) -> &Endpoint {
        &self.destination
    }

    /// Returns the asset moved by this leg.
    #[must_use]
    pub const fn asset(&self) -> AssetId {
        self.asset
    }

    /// Returns the amount this leg delivers to its destination.
    #[must_use]
    pub const fn amount(&self) -> Amount {
        self.amount
    }

    /// Returns the deterministic fee quoted for this leg.
    #[must_use]
    pub const fn fee(&self) -> u128 {
        self.fee
    }

    /// Returns the authority that must sign this leg.
    #[must_use]
    pub const fn authority(&self) -> RequiredAuthority {
        self.authority
    }

    /// Returns the allowance consumed when this leg is an automatic top-up.
    #[must_use]
    pub const fn top_up(&self) -> Option<&TopUpRecord> {
        self.top_up.as_ref()
    }

    fn canonical_encode(&self, out: &mut Vec<u8>) {
        out.extend(u16::try_from(self.index).unwrap_or(u16::MAX).to_be_bytes());
        out.push(self.mechanism.code());
        out.push(term_code(self.term));
        put_endpoint(out, &self.source);
        put_endpoint(out, &self.destination);
        out.extend(self.asset.bytes());
        out.extend(self.amount.to_be_bytes());
        out.extend(self.fee.to_be_bytes());
        out.push(self.authority.code());
        match self.authority {
            RequiredAuthority::Allowance(id) => out.extend(id.bytes()),
            RequiredAuthority::PaxeerWalletKey | RequiredAuthority::AccountOwner => {}
        }
        match self.top_up {
            None => out.push(0),
            Some(record) => {
                out.push(1);
                out.extend(record.allowance.bytes());
                out.push(record.kind.code());
                out.extend(record.consumed.to_be_bytes());
            }
        }
    }
}

/// One complete cross-domain plan with its canonical encoding and digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnifiedPlan {
    intent: UnifiedIntent,
    legs: Vec<PlannedLeg>,
    total_fee: u128,
    digest: [u8; 32],
}

impl UnifiedPlan {
    fn seal(intent: UnifiedIntent, drafts: Vec<Draft>) -> Result<Self, Refusal> {
        if drafts.is_empty() {
            return Err(Refusal::NoRoute {
                source: intent.source.kind(),
                destination: intent.destination.kind(),
            });
        }
        if drafts.len() > MAXIMUM_PLAN_LEGS {
            return Err(Refusal::TooManyLegs { legs: drafts.len() });
        }
        let mut total_fee = 0_u128;
        let mut legs = Vec::with_capacity(drafts.len());
        for (index, draft) in drafts.into_iter().enumerate() {
            total_fee = total_fee
                .checked_add(draft.fee)
                .ok_or(Refusal::Arithmetic)?;
            legs.push(PlannedLeg {
                index,
                mechanism: draft.mechanism,
                term: draft.term,
                source: draft.source,
                destination: draft.destination,
                asset: intent.asset,
                amount: Amount::from_u128(draft.amount),
                fee: draft.fee,
                authority: draft.authority,
                top_up: draft.top_up,
            });
        }
        let mut plan = Self {
            intent,
            legs,
            total_fee,
            digest: [0; 32],
        };
        let mut digest = Sha256::new();
        digest.update(PLAN_DOMAIN);
        digest.update(plan.canonical_encode());
        plan.digest = digest.finalize().into();
        Ok(plan)
    }

    /// Returns the intent this plan serves.
    #[must_use]
    pub const fn intent(&self) -> &UnifiedIntent {
        &self.intent
    }

    /// Returns the ordered legs across both domains.
    #[must_use]
    pub fn legs(&self) -> &[PlannedLeg] {
        &self.legs
    }

    /// Returns the total fee across every leg.
    #[must_use]
    pub const fn total_fee(&self) -> u128 {
        self.total_fee
    }

    /// Returns the plan digest that binds the intent and every leg.
    #[must_use]
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }

    /// Encodes the complete plan in one canonical, versioned form. The digest
    /// is derived from these bytes and is therefore not part of them.
    #[must_use]
    pub fn canonical_encode(&self) -> Vec<u8> {
        let mut out = vec![PLAN_VERSION];
        out.extend(self.intent.canonical_encode());
        out.extend(
            u16::try_from(self.legs.len())
                .unwrap_or(u16::MAX)
                .to_be_bytes(),
        );
        for leg in &self.legs {
            leg.canonical_encode(&mut out);
        }
        out.extend(self.total_fee.to_be_bytes());
        out
    }

    /// Derives the agent action key for one leg from the plan digest. Altering
    /// any leg changes the digest and therefore every action key.
    ///
    /// # Errors
    ///
    /// Refuses a leg index outside the plan.
    pub fn action_key(&self, index: usize) -> Result<[u8; 32], Refusal> {
        self.derive(ACTION_DOMAIN, index)
    }

    /// Derives the signing context one authority signs for a leg.
    ///
    /// # Errors
    ///
    /// Refuses a leg index outside the plan.
    pub fn signing_context(&self, index: usize) -> Result<[u8; 32], Refusal> {
        self.derive(CONTEXT_DOMAIN, index)
    }

    fn derive(&self, domain: &[u8], index: usize) -> Result<[u8; 32], Refusal> {
        if index >= self.legs.len() {
            return Err(Refusal::LegMismatch { index });
        }
        let position = u64::try_from(index).map_err(|_| Refusal::Arithmetic)?;
        let mut digest = Sha256::new();
        digest.update(domain);
        digest.update(self.digest);
        digest.update(position.to_be_bytes());
        let derived: [u8; 32] = digest.finalize().into();
        if derived == [0; 32] {
            return Err(Refusal::LegMismatch { index });
        }
        Ok(derived)
    }

    /// Returns exactly what must be signed, in leg order.
    ///
    /// # Errors
    ///
    /// Refuses a plan whose derived signing material is unusable.
    pub fn signing_requirements(&self) -> Result<Vec<SigningRequirement>, Refusal> {
        self.legs
            .iter()
            .map(|leg| {
                Ok(SigningRequirement {
                    leg_index: leg.index,
                    domain: leg.domain(),
                    mechanism: leg.mechanism,
                    authority: leg.authority,
                    action_key: self.action_key(leg.index)?,
                    signing_context: self.signing_context(leg.index)?,
                })
            })
            .collect()
    }

    /// Returns the journey kind the engine records this plan under.
    #[must_use]
    pub fn journey_kind(&self) -> JourneyKind {
        if self
            .legs
            .iter()
            .any(|leg| leg.mechanism == LegMechanism::Protocol(Mechanism::BridgeDepositCredit))
        {
            return JourneyKind::Deposit;
        }
        if self
            .legs
            .iter()
            .any(|leg| leg.mechanism == LegMechanism::Protocol(Mechanism::BridgeWithdrawRequest))
        {
            return JourneyKind::Withdraw;
        }
        JourneyKind::Move
    }

    fn mechanism_sequence(&self) -> Vec<&'static str> {
        self.legs.iter().map(|leg| leg.mechanism.label()).collect()
    }

    fn documented_order(&self, other: &Self) -> Ordering {
        self.legs
            .len()
            .cmp(&other.legs.len())
            .then_with(|| self.total_fee.cmp(&other.total_fee))
            .then_with(|| self.mechanism_sequence().cmp(&other.mechanism_sequence()))
    }
}

/// Exactly what one authority must sign for one leg.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SigningRequirement {
    leg_index: usize,
    domain: Domain,
    mechanism: LegMechanism,
    authority: RequiredAuthority,
    action_key: [u8; 32],
    signing_context: [u8; 32],
}

impl SigningRequirement {
    /// Returns the leg this requirement belongs to.
    #[must_use]
    pub const fn leg_index(&self) -> usize {
        self.leg_index
    }

    /// Returns the settlement domain of the leg.
    #[must_use]
    pub const fn domain(&self) -> Domain {
        self.domain
    }

    /// Returns the mechanism of the leg.
    #[must_use]
    pub const fn mechanism(&self) -> LegMechanism {
        self.mechanism
    }

    /// Returns the authority that must sign.
    #[must_use]
    pub const fn authority(&self) -> RequiredAuthority {
        self.authority
    }

    /// Returns the plan-bound agent action key for the leg.
    #[must_use]
    pub const fn action_key(&self) -> [u8; 32] {
        self.action_key
    }

    /// Returns the plan-bound signing context for the leg.
    #[must_use]
    pub const fn signing_context(&self) -> [u8; 32] {
        self.signing_context
    }
}

/// One bounded annotation an advisor may attach to a decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Annotation(String);

impl Annotation {
    /// Records one bounded plain-text annotation.
    ///
    /// # Errors
    ///
    /// Refuses empty, over-long or control-bearing text.
    pub fn new(text: impl Into<String>) -> Result<Self, Refusal> {
        let text = text.into();
        if text.trim().is_empty() || text.len() > NOTE_LIMIT || text.chars().any(char::is_control) {
            return Err(Refusal::InvalidAnnotation);
        }
        Ok(Self(text))
    }

    /// Returns the annotation text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.0
    }
}

/// An opaque handle to one candidate the router itself produced.
///
/// Its fields are private and it has no public constructor, so the only way an
/// advisor can obtain one is from the [`CandidateSet`] the router hands it. That
/// is what makes it impossible, by type, for an advisor to name a plan the
/// router did not build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CandidateRef {
    token: u64,
    position: usize,
}

/// The tied candidates offered to an advisor, in canonical order.
#[derive(Debug)]
pub struct CandidateSet<'plans> {
    token: u64,
    plans: &'plans [UnifiedPlan],
}

impl CandidateSet<'_> {
    /// Returns each tied candidate with the handle that names it.
    #[must_use]
    pub fn candidates(&self) -> Vec<(CandidateRef, &UnifiedPlan)> {
        self.plans
            .iter()
            .enumerate()
            .map(|(position, plan)| {
                (
                    CandidateRef {
                        token: self.token,
                        position,
                    },
                    plan,
                )
            })
            .collect()
    }

    /// Returns the number of tied candidates.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.plans.len()
    }

    /// Returns whether the tied set is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.plans.is_empty()
    }
}

/// An advisor's output. It carries a preference among the router's own tied
/// candidates and bounded annotations, and nothing else: there is no field
/// through which a leg, an amount, an authority or an allowance can be reached,
/// so an advisor can never add, remove or alter a leg, nor authorise anything.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Advice {
    preferred: Option<CandidateRef>,
    notes: Vec<Annotation>,
}

impl Advice {
    /// Returns advice that changes nothing.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            preferred: None,
            notes: Vec::new(),
        }
    }

    /// Prefers one of the router's tied candidates.
    #[must_use]
    pub const fn prefer(candidate: CandidateRef) -> Self {
        Self {
            preferred: Some(candidate),
            notes: Vec::new(),
        }
    }

    /// Attaches bounded annotations to the advice.
    #[must_use]
    pub fn annotated(mut self, notes: Vec<Annotation>) -> Self {
        self.notes = notes;
        self
    }
}

/// Advisory ranking and annotation over candidate plans the router has already
/// built and validated. An advisor is consulted only where the documented rule
/// leaves an exact tie, and its output cannot express a plan.
pub trait RouteAdvisor {
    /// Ranks or annotates the tied candidates.
    fn advise(&self, candidates: &CandidateSet<'_>) -> Advice;
}

/// A chosen plan together with any advisory annotations. The annotations are
/// deliberately outside [`UnifiedPlan`], so advice can never change plan bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdvisedPlan {
    plan: UnifiedPlan,
    notes: Vec<Annotation>,
}

impl AdvisedPlan {
    /// Returns the chosen plan.
    #[must_use]
    pub const fn plan(&self) -> &UnifiedPlan {
        &self.plan
    }

    /// Returns the advisory annotations.
    #[must_use]
    pub fn notes(&self) -> &[Annotation] {
        &self.notes
    }

    /// Consumes the decision and returns the chosen plan.
    #[must_use]
    pub fn into_plan(self) -> UnifiedPlan {
        self.plan
    }
}

/// Plans one stated intent deterministically over one observed state.
///
/// # Errors
///
/// Returns the typed refusal for an unroutable intent, an unaffordable
/// movement, an unauthorised top-up or a breached constraint.
pub fn plan(intent: &UnifiedIntent, observed: &ObservedState) -> Result<UnifiedPlan, Refusal> {
    let candidates = candidates(intent, observed)?;
    Ok(select(candidates, None)?.plan)
}

/// Plans one stated intent with an advisory ranking hook.
///
/// The candidate set is built exactly as [`plan`] builds it. The advisor is
/// consulted only among candidates that are exactly tied under the documented
/// rule, and a preference naming anything else is ignored.
///
/// # Errors
///
/// Returns the same typed refusals as [`plan`].
pub fn plan_with_advisor(
    intent: &UnifiedIntent,
    observed: &ObservedState,
    advisor: &dyn RouteAdvisor,
) -> Result<AdvisedPlan, Refusal> {
    let candidates = candidates(intent, observed)?;
    select(candidates, Some(advisor))
}

#[derive(Clone, Debug)]
struct Draft {
    mechanism: LegMechanism,
    term: MovementTerm,
    source: Endpoint,
    destination: Endpoint,
    amount: u128,
    fee: u128,
    authority: RequiredAuthority,
    top_up: Option<TopUpRecord>,
}

#[derive(Clone, Debug)]
struct Shape {
    mechanism: LegMechanism,
    term: MovementTerm,
    source: Endpoint,
    destination: Endpoint,
    authority: RequiredAuthority,
}

fn candidates(
    intent: &UnifiedIntent,
    observed: &ObservedState,
) -> Result<Vec<UnifiedPlan>, Refusal> {
    let observed_at = observed.observed_at.value();
    let deadline = intent.constraints.deadline.value();
    if observed_at > deadline {
        return Err(Refusal::DeadlinePassed {
            deadline,
            observed_at,
        });
    }
    let base = base_shapes(intent, observed)?;
    let base_drafts = settle(&base, intent.amount.value(), observed)?;
    let base_fee = total_fee(&base_drafts)?;
    let funding = base
        .first()
        .ok_or(Refusal::NoRoute {
            source: intent.source.kind(),
            destination: intent.destination.kind(),
        })?
        .source
        .clone();
    let required = intent
        .amount
        .value()
        .checked_add(base_fee)
        .ok_or(Refusal::Arithmetic)?;
    let available = observed.available(&funding, intent.asset).value();
    let mut built = Vec::new();
    if available >= required {
        built.push(UnifiedPlan::seal(intent.clone(), base_drafts)?);
    } else {
        let shortfall = required.checked_sub(available).ok_or(Refusal::Arithmetic)?;
        if !intent.constraints.allow_top_up {
            return Err(Refusal::TopUpNotPermitted { shortfall });
        }
        if funding == Endpoint::PaxeerWallet {
            return Err(Refusal::InsufficientFunds { shortfall });
        }
        let prefixes = top_up_prefixes(intent, observed, &funding, shortfall)?;
        if prefixes.is_empty() {
            return Err(Refusal::TopUpNotAuthorized { shortfall });
        }
        for prefix in prefixes {
            let mut drafts = prefix;
            drafts.extend(base_drafts.clone());
            built.push(UnifiedPlan::seal(intent.clone(), drafts)?);
        }
    }
    let cheapest = built
        .iter()
        .map(UnifiedPlan::total_fee)
        .min()
        .ok_or(Refusal::NoCandidates)?;
    built.retain(|candidate| candidate.total_fee <= intent.constraints.max_fee);
    if built.is_empty() {
        return Err(Refusal::FeeCeilingExceeded {
            required: cheapest,
            ceiling: intent.constraints.max_fee,
        });
    }
    built.sort_by(|left, right| {
        left.documented_order(right)
            .then_with(|| left.canonical_encode().cmp(&right.canonical_encode()))
    });
    built.truncate(MAXIMUM_CANDIDATES);
    Ok(built)
}

fn base_shapes(intent: &UnifiedIntent, observed: &ObservedState) -> Result<Vec<Shape>, Refusal> {
    let home = observed.home();
    match (&intent.source, &intent.destination) {
        (Endpoint::PaxeerWallet, Endpoint::PaxeerWallet) => Err(Refusal::NoRoute {
            source: EndpointKind::PaxeerWallet,
            destination: EndpointKind::PaxeerWallet,
        }),
        (Endpoint::PaxeerWallet, destination) => {
            if observed.custody.is_none() {
                return Err(Refusal::WalletNotBound);
            }
            let mut shapes = vec![
                Shape {
                    mechanism: LegMechanism::PaxeerCustodyDeposit,
                    term: MovementTerm::Deposit,
                    source: Endpoint::PaxeerWallet,
                    destination: home.clone(),
                    authority: RequiredAuthority::PaxeerWalletKey,
                },
                Shape {
                    mechanism: LegMechanism::Protocol(Mechanism::BridgeDepositCredit),
                    term: MovementTerm::Deposit,
                    source: Endpoint::PaxeerWallet,
                    destination: home.clone(),
                    authority: RequiredAuthority::AccountOwner,
                },
            ];
            if destination != &home {
                shapes.push(internal_shape(&home, destination, observed)?);
            }
            Ok(shapes)
        }
        (source, Endpoint::PaxeerWallet) => {
            if observed.custody.is_none() {
                return Err(Refusal::WalletNotBound);
            }
            let mut shapes = Vec::new();
            if source != &home {
                shapes.push(internal_shape(source, &home, observed)?);
            }
            shapes.push(Shape {
                mechanism: LegMechanism::Protocol(Mechanism::BridgeWithdrawRequest),
                term: MovementTerm::Withdrawal,
                source: home.clone(),
                destination: Endpoint::PaxeerWallet,
                authority: RequiredAuthority::AccountOwner,
            });
            shapes.push(Shape {
                mechanism: LegMechanism::PaxeerWithdrawFinalise,
                term: MovementTerm::Withdrawal,
                source: home,
                destination: Endpoint::PaxeerWallet,
                authority: RequiredAuthority::PaxeerWalletKey,
            });
            Ok(shapes)
        }
        (source, destination) => Ok(vec![internal_shape(source, destination, observed)?]),
    }
}

fn internal_shape(
    source: &Endpoint,
    destination: &Endpoint,
    observed: &ObservedState,
) -> Result<Shape, Refusal> {
    let unroutable = Refusal::NoRoute {
        source: source.kind(),
        destination: destination.kind(),
    };
    let (mechanism, term) = match (source, destination) {
        (Endpoint::Human(_), Endpoint::AgentBudget(budget)) => {
            if !observed.budget_is_owned(budget) {
                return Err(Refusal::BudgetNotBound);
            }
            (Mechanism::BudgetFund, MovementTerm::Fund)
        }
        (Endpoint::AgentBudget(budget), Endpoint::Human(_)) => {
            if !observed.budget_is_owned(budget) {
                return Err(Refusal::BudgetNotBound);
            }
            (Mechanism::BudgetDefund, MovementTerm::Return)
        }
        (Endpoint::AgentBudget(budget), Endpoint::Agent(_)) => {
            if !observed.budget_is_owned(budget) {
                return Err(Refusal::BudgetNotBound);
            }
            (Mechanism::Send, MovementTerm::Allocate)
        }
        (Endpoint::Agent(_), Endpoint::Human(_)) => (Mechanism::Send, MovementTerm::Return),
        (Endpoint::Human(_), Endpoint::Human(_) | Endpoint::Agent(_))
        | (Endpoint::Agent(_), Endpoint::Agent(_)) => (Mechanism::Send, MovementTerm::Transfer),
        _ => return Err(unroutable),
    };
    Ok(Shape {
        mechanism: LegMechanism::Protocol(mechanism),
        term,
        source: source.clone(),
        destination: destination.clone(),
        authority: RequiredAuthority::AccountOwner,
    })
}

/// Assigns per-leg amounts backwards from the delivered amount, so each leg
/// carries exactly what the remaining legs need plus their fees.
fn settle(
    shapes: &[Shape],
    delivered: u128,
    observed: &ObservedState,
) -> Result<Vec<Draft>, Refusal> {
    let amounts = backward_amounts(shapes, delivered, observed)?;
    let mut drafts = Vec::with_capacity(shapes.len());
    for (index, shape) in shapes.iter().enumerate() {
        drafts.push(Draft {
            mechanism: shape.mechanism,
            term: shape.term,
            source: shape.source.clone(),
            destination: shape.destination.clone(),
            amount: *amounts.get(index).ok_or(Refusal::Arithmetic)?,
            fee: observed.fees.fee(shape.mechanism)?,
            authority: shape.authority,
            top_up: None,
        });
    }
    Ok(drafts)
}

fn total_fee(drafts: &[Draft]) -> Result<u128, Refusal> {
    drafts.iter().try_fold(0_u128, |total, draft| {
        total.checked_add(draft.fee).ok_or(Refusal::Arithmetic)
    })
}

/// Builds every top-up prefix that delivers the shortfall into the funding
/// endpoint entirely inside allowances the user has already signed.
fn top_up_prefixes(
    intent: &UnifiedIntent,
    observed: &ObservedState,
    funding: &Endpoint,
    shortfall: u128,
) -> Result<Vec<Vec<Draft>>, Refusal> {
    let home = observed.home();
    let mut shapes_options: Vec<Vec<Shape>> = Vec::new();
    if funding != &home {
        if let Ok(shape) = internal_shape(&home, funding, observed) {
            shapes_options.push(vec![shape]);
        }
    }
    if observed.custody.is_some() {
        let mut wallet_route = vec![
            Shape {
                mechanism: LegMechanism::PaxeerCustodyDeposit,
                term: MovementTerm::Deposit,
                source: Endpoint::PaxeerWallet,
                destination: home.clone(),
                authority: RequiredAuthority::PaxeerWalletKey,
            },
            Shape {
                mechanism: LegMechanism::Protocol(Mechanism::BridgeDepositCredit),
                term: MovementTerm::Deposit,
                source: Endpoint::PaxeerWallet,
                destination: home.clone(),
                authority: RequiredAuthority::AccountOwner,
            },
        ];
        if funding != &home {
            if let Ok(shape) = internal_shape(&home, funding, observed) {
                wallet_route.push(shape);
            } else {
                wallet_route.clear();
            }
        }
        if !wallet_route.is_empty() {
            shapes_options.push(wallet_route);
        }
    }
    let mut prefixes = Vec::new();
    for shapes in shapes_options {
        let origin = match shapes.first() {
            None => continue,
            Some(shape) => shape.source.clone(),
        };
        let mut fee_total = 0_u128;
        let mut fees_known = true;
        for shape in &shapes {
            match observed.fees.fee(shape.mechanism) {
                Ok(fee) => {
                    fee_total = fee_total.checked_add(fee).ok_or(Refusal::Arithmetic)?;
                }
                Err(_) => {
                    fees_known = false;
                    break;
                }
            }
        }
        if !fees_known {
            continue;
        }
        let cost = shortfall
            .checked_add(fee_total)
            .ok_or(Refusal::Arithmetic)?;
        if observed.available(&origin, intent.asset).value() < cost {
            continue;
        }
        let amounts = backward_amounts(&shapes, shortfall, observed)?;
        let mut per_leg = Vec::with_capacity(shapes.len());
        for (index, shape) in shapes.iter().enumerate() {
            let amount = *amounts.get(index).ok_or(Refusal::Arithmetic)?;
            let covering = observed
                .allowances
                .iter()
                .filter(|allowance| {
                    allowance.covers(
                        &shape.source,
                        &shape.destination,
                        intent.asset,
                        shape.mechanism,
                        amount,
                        observed.observed_at.value(),
                    )
                })
                .map(|allowance| (allowance.id, allowance.kind))
                .collect::<Vec<_>>();
            if covering.is_empty() {
                per_leg.clear();
                break;
            }
            per_leg.push(covering);
        }
        if per_leg.len() != shapes.len() {
            continue;
        }
        for choice in combinations(&per_leg) {
            let mut drafts = Vec::with_capacity(shapes.len());
            for (index, shape) in shapes.iter().enumerate() {
                let amount = *amounts.get(index).ok_or(Refusal::Arithmetic)?;
                let (id, kind) = *choice.get(index).ok_or(Refusal::Arithmetic)?;
                drafts.push(Draft {
                    mechanism: shape.mechanism,
                    term: shape.term,
                    source: shape.source.clone(),
                    destination: shape.destination.clone(),
                    amount,
                    fee: observed.fees.fee(shape.mechanism)?,
                    authority: RequiredAuthority::Allowance(id),
                    top_up: Some(TopUpRecord {
                        allowance: id,
                        kind,
                        consumed: Amount::from_u128(amount),
                    }),
                });
            }
            prefixes.push(drafts);
            if prefixes.len() >= MAXIMUM_CANDIDATES {
                return Ok(prefixes);
            }
        }
    }
    Ok(prefixes)
}

fn backward_amounts(
    shapes: &[Shape],
    delivered: u128,
    observed: &ObservedState,
) -> Result<Vec<u128>, Refusal> {
    let mut amounts = vec![0_u128; shapes.len()];
    let mut carried = delivered;
    for index in (0..shapes.len()).rev() {
        amounts[index] = carried;
        let shape = shapes.get(index).ok_or(Refusal::Arithmetic)?;
        carried = carried
            .checked_add(observed.fees.fee(shape.mechanism)?)
            .ok_or(Refusal::Arithmetic)?;
    }
    Ok(amounts)
}

fn combinations(
    per_leg: &[Vec<(AllowanceId, AllowanceKind)>],
) -> Vec<Vec<(AllowanceId, AllowanceKind)>> {
    let mut built: Vec<Vec<(AllowanceId, AllowanceKind)>> = vec![Vec::new()];
    for options in per_leg {
        let mut next = Vec::new();
        for prefix in &built {
            for option in options {
                if next.len() >= MAXIMUM_CANDIDATES {
                    return next;
                }
                let mut extended = prefix.clone();
                extended.push(*option);
                next.push(extended);
            }
        }
        built = next;
    }
    built
}

fn select(
    candidates: Vec<UnifiedPlan>,
    advisor: Option<&dyn RouteAdvisor>,
) -> Result<AdvisedPlan, Refusal> {
    let mut candidates = candidates;
    candidates.sort_by(|left, right| {
        left.documented_order(right)
            .then_with(|| left.canonical_encode().cmp(&right.canonical_encode()))
    });
    let winner = candidates.first().cloned().ok_or(Refusal::NoCandidates)?;
    let tied = candidates
        .iter()
        .filter(|candidate| candidate.documented_order(&winner) == Ordering::Equal)
        .cloned()
        .collect::<Vec<_>>();
    let Some(advisor) = advisor else {
        return Ok(AdvisedPlan {
            plan: winner,
            notes: Vec::new(),
        });
    };
    let token = token_for(&winner);
    let set = CandidateSet {
        token,
        plans: &tied,
    };
    let advice = advisor.advise(&set);
    let mut notes = advice.notes;
    notes.truncate(MAXIMUM_NOTES);
    let chosen = advice
        .preferred
        .filter(|candidate| candidate.token == token)
        .and_then(|candidate| tied.get(candidate.position).cloned())
        .unwrap_or(winner);
    Ok(AdvisedPlan {
        plan: chosen,
        notes,
    })
}

fn token_for(plan: &UnifiedPlan) -> u64 {
    let digest = plan.digest();
    let mut head = [0_u8; 8];
    head.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(head)
}

fn endpoint_key(endpoint: &Endpoint) -> Vec<u8> {
    let mut out = Vec::new();
    put_endpoint(&mut out, endpoint);
    out
}

const fn term_code(term: MovementTerm) -> u8 {
    match term {
        MovementTerm::Deposit => 1,
        MovementTerm::Withdrawal => 2,
        MovementTerm::Fund => 3,
        MovementTerm::Allocate => 4,
        MovementTerm::Return => 5,
        MovementTerm::Transfer => 6,
    }
}

/// Signature-bearing material and execution context for one LayerX plan leg.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegBinding {
    leg_index: usize,
    action_key: [u8; 32],
    relationship: Relationship,
    actor: AgentDid,
    authority: AuthorityRef,
    account_sequence: u64,
    not_before: u64,
    not_after: u64,
    fee_limit: u128,
}

impl LegBinding {
    /// Binds one signed relationship and its execution context to one leg.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero action key and an inverted validity interval.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        leg_index: usize,
        action_key: [u8; 32],
        relationship: Relationship,
        actor: AgentDid,
        authority: AuthorityRef,
        account_sequence: u64,
        not_before: u64,
        not_after: u64,
        fee_limit: u128,
    ) -> Result<Self, Refusal> {
        if action_key == [0; 32] || not_after < not_before {
            return Err(Refusal::LegMismatch { index: leg_index });
        }
        Ok(Self {
            leg_index,
            action_key,
            relationship,
            actor,
            authority,
            account_sequence,
            not_before,
            not_after,
            fee_limit,
        })
    }

    /// Returns the leg this binding belongs to.
    #[must_use]
    pub const fn leg_index(&self) -> usize {
        self.leg_index
    }
}

/// A plan whose LayerX legs carry validated signed material. Construction is
/// atomic: no partially bound plan exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedPlan {
    plan: UnifiedPlan,
    legs: Vec<JourneyLeg>,
}

impl SignedPlan {
    /// Accepts signed material against the exact plan the user signed.
    ///
    /// The supplied digest must equal the plan digest, every binding must
    /// present the plan-derived action key for its leg, and each binding must
    /// resolve, through [`RouteResolver`], to exactly the planned mechanism,
    /// endpoints, asset and amount. Altering a leg after signing therefore
    /// cannot be re-bound to the signed digest.
    ///
    /// # Errors
    ///
    /// Refuses a digest mismatch, a missing or extra binding, a leg whose
    /// resolved route differs from the plan, and any typed resolver refusal.
    pub fn accept(
        plan: UnifiedPlan,
        signed_digest: [u8; 32],
        bindings: Vec<LegBinding>,
    ) -> Result<Self, Refusal> {
        if signed_digest != plan.digest {
            return Err(Refusal::PlanDigestMismatch);
        }
        let protocol_legs = plan
            .legs
            .iter()
            .filter(|leg| leg.domain() == Domain::LayerX)
            .collect::<Vec<_>>();
        if bindings.len() != protocol_legs.len() {
            return Err(Refusal::UnboundLegs {
                expected: protocol_legs.len(),
                bound: bindings.len(),
            });
        }
        let mut legs = Vec::with_capacity(bindings.len());
        for (leg, binding) in protocol_legs.iter().zip(bindings) {
            let index = leg.index;
            if binding.leg_index != index
                || binding.action_key != plan.action_key(index)?
                || binding.fee_limit != leg.fee
                || binding.not_after > plan.intent.constraints.deadline.value()
            {
                return Err(Refusal::LegMismatch { index });
            }
            let mechanism = leg
                .mechanism
                .protocol()
                .ok_or(Refusal::LegMismatch { index })?;
            let request = RouteRequest::from_wire_parts(
                leg.source.clone(),
                leg.destination.clone(),
                binding.relationship.clone(),
                leg.asset,
                leg.amount,
            )
            .map_err(Refusal::Route)?;
            let route = RouteResolver::resolve(&request).map_err(Refusal::Route)?;
            let resolved = match route.legs() {
                [single] => single,
                _ => return Err(Refusal::LegMismatch { index }),
            };
            if resolved.mechanism() != mechanism || resolved.term() != leg.term {
                return Err(Refusal::LegMismatch { index });
            }
            legs.push(
                JourneyLeg::new(
                    resolved.intent().clone(),
                    binding.action_key,
                    binding.actor,
                    binding.authority,
                    binding.account_sequence,
                    binding.not_before,
                    binding.not_after,
                    binding.fee_limit,
                )
                .map_err(|_| Refusal::LegMismatch { index })?,
            );
        }
        Ok(Self { plan, legs })
    }

    /// Returns the plan this signed material is bound to.
    #[must_use]
    pub const fn plan(&self) -> &UnifiedPlan {
        &self.plan
    }

    /// Produces the durable journey plan the engine executes, with the plan
    /// digest as the engine idempotency key so a re-routed plan is a different
    /// journey.
    ///
    /// # Errors
    ///
    /// Refuses a plan the engine rejects, such as one with no LayerX leg.
    pub fn journey_plan(
        &self,
        journey_id: JourneyId,
        custody_key: KeyId,
        operation: Operation,
    ) -> Result<JourneyPlan, Refusal> {
        JourneyPlan::new(
            journey_id,
            self.plan.journey_kind(),
            self.plan.digest,
            custody_key,
            operation,
            self.legs.clone(),
        )
        .map_err(|_| Refusal::InvalidJourneyPlan)
    }
}

/// Typed planning refusal. No variant carries a partial plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The intent moved nothing.
    ZeroAmount,
    /// The intent named the same endpoint twice.
    EndpointsIdentical,
    /// The observation is later than the intent deadline.
    DeadlinePassed {
        /// The declared deadline.
        deadline: u64,
        /// The observation timestamp.
        observed_at: u64,
    },
    /// No mechanism connects these endpoints.
    NoRoute {
        /// The source endpoint kind.
        source: EndpointKind,
        /// The destination endpoint kind.
        destination: EndpointKind,
    },
    /// The route crosses custody but no wallet is bound.
    WalletNotBound,
    /// The named managed budget does not belong to the acting owner.
    BudgetNotBound,
    /// The funding endpoint cannot cover the movement and no top-up applies.
    InsufficientFunds {
        /// The exact missing amount.
        shortfall: u128,
    },
    /// A top-up would be needed but the intent forbids automatic top-ups.
    TopUpNotPermitted {
        /// The exact missing amount.
        shortfall: u128,
    },
    /// A top-up would be needed and no signed allowance covers it.
    TopUpNotAuthorized {
        /// The exact missing amount.
        shortfall: u128,
    },
    /// The observed fee schedule does not quote a required mechanism.
    FeeUnknown {
        /// The unquoted mechanism label.
        mechanism: &'static str,
    },
    /// Every candidate costs more than the declared ceiling.
    FeeCeilingExceeded {
        /// The cheapest achievable total fee.
        required: u128,
        /// The declared ceiling.
        ceiling: u128,
    },
    /// The plan exceeded the bounded leg count.
    TooManyLegs {
        /// The refused leg count.
        legs: usize,
    },
    /// An observation was presented twice.
    DuplicateObservation,
    /// The observed acting account is not a main account.
    InvalidOwner,
    /// Selection was asked to choose from an empty candidate set.
    NoCandidates,
    /// An allowance is unusable, unknown, or does not cover its leg.
    InvalidAllowance,
    /// An advisory annotation was empty, over-long or control-bearing.
    InvalidAnnotation,
    /// Signed material was presented against a different plan.
    PlanDigestMismatch,
    /// One binding does not match the leg it claims.
    LegMismatch {
        /// The refused leg position.
        index: usize,
    },
    /// The bindings do not cover exactly the plan's LayerX legs.
    UnboundLegs {
        /// The number of LayerX legs in the plan.
        expected: usize,
        /// The number of bindings presented.
        bound: usize,
    },
    /// The resolver refused the bound relationship.
    Route(RouteError),
    /// The engine refused the derived journey plan.
    InvalidJourneyPlan,
    /// A bounded arithmetic operation overflowed.
    Arithmetic,
}

impl Display for Refusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroAmount => formatter.write_str("the intent moves nothing"),
            Self::EndpointsIdentical => {
                formatter.write_str("the intent names the same endpoint twice")
            }
            Self::DeadlinePassed {
                deadline,
                observed_at,
            } => write!(
                formatter,
                "the intent deadline {deadline} is before the observation {observed_at}"
            ),
            Self::NoRoute {
                source,
                destination,
            } => write!(formatter, "no route connects {source:?} to {destination:?}"),
            Self::WalletNotBound => formatter.write_str("no Paxeer wallet is bound"),
            Self::BudgetNotBound => {
                formatter.write_str("the managed budget does not belong to this owner")
            }
            Self::InsufficientFunds { shortfall } => {
                write!(formatter, "the movement is short by {shortfall}")
            }
            Self::TopUpNotPermitted { shortfall } => write!(
                formatter,
                "a top-up of {shortfall} is needed and automatic top-ups are not permitted"
            ),
            Self::TopUpNotAuthorized { shortfall } => write!(
                formatter,
                "no signed allowance covers the {shortfall} top-up"
            ),
            Self::FeeUnknown { mechanism } => {
                write!(formatter, "no fee is quoted for {mechanism}")
            }
            Self::FeeCeilingExceeded { required, ceiling } => write!(
                formatter,
                "the cheapest plan costs {required} above the ceiling {ceiling}"
            ),
            Self::TooManyLegs { legs } => write!(formatter, "the plan needs {legs} legs"),
            Self::DuplicateObservation => formatter.write_str("the observation repeats an entry"),
            Self::InvalidOwner => formatter.write_str("the acting account is not a main account"),
            Self::NoCandidates => formatter.write_str("no candidate plan was produced"),
            Self::InvalidAllowance => formatter.write_str("the allowance does not cover this leg"),
            Self::InvalidAnnotation => formatter.write_str("the annotation is not plain text"),
            Self::PlanDigestMismatch => {
                formatter.write_str("the signed material belongs to a different plan")
            }
            Self::LegMismatch { index } => {
                write!(
                    formatter,
                    "the binding for leg {index} does not match the plan"
                )
            }
            Self::UnboundLegs { expected, bound } => write!(
                formatter,
                "the plan has {expected} protocol legs and {bound} were bound"
            ),
            Self::Route(error) => write!(formatter, "route refused: {error}"),
            Self::InvalidJourneyPlan => formatter.write_str("the engine refused the plan"),
            Self::Arithmetic => formatter.write_str("a bounded amount overflowed"),
        }
    }
}

impl std::error::Error for Refusal {}

impl From<RouteError> for Refusal {
    fn from(value: RouteError) -> Self {
        Self::Route(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use layerx_types::ids::IdempotencyKey;
    use layerx_types::intent::{BudgetId, Sequence};

    use crate::journeys::{BudgetRoute, JourneyKind};

    const ASSET: AssetId = AssetId::new([9; 32]);
    const OWNER: &str = "agent:did:layerx:alice:main";
    const AGENT: &str = "agent:did:layerx:bob:main";
    const BUDGET: &str = "agent:did:layerx:bob:budget:one";

    fn account(value: &str) -> AccountId {
        AccountId::parse(value).unwrap_or_else(|error| panic!("account {value}: {error:?}"))
    }

    fn home() -> Endpoint {
        Endpoint::human(account(OWNER)).unwrap_or_else(|error| panic!("home: {error:?}"))
    }

    fn agent() -> Endpoint {
        Endpoint::agent(account(AGENT)).unwrap_or_else(|error| panic!("agent: {error:?}"))
    }

    fn budget() -> Endpoint {
        Endpoint::agent_budget(account(BUDGET)).unwrap_or_else(|error| panic!("budget: {error:?}"))
    }

    fn allowance_id(seed: u8) -> AllowanceId {
        AllowanceId::new([seed; 32]).unwrap_or_else(|error| panic!("allowance id: {error}"))
    }

    fn fees() -> FeeSchedule {
        FeeSchedule::new(vec![
            (LegMechanism::Protocol(Mechanism::Send), 5),
            (LegMechanism::Protocol(Mechanism::BudgetFund), 2),
            (LegMechanism::Protocol(Mechanism::BudgetDefund), 2),
            (LegMechanism::Protocol(Mechanism::BridgeDepositCredit), 3),
            (LegMechanism::Protocol(Mechanism::BridgeWithdrawRequest), 4),
            (LegMechanism::PaxeerCustodyDeposit, 1),
            (LegMechanism::PaxeerWithdrawFinalise, 1),
        ])
        .unwrap_or_else(|error| panic!("fees: {error}"))
    }

    fn custody() -> CustodyContext {
        CustodyContext::new(
            EvmAddress::new([7; 20]),
            account("system:paxeer-reserve"),
            account("system:paxeer-withdrawals"),
        )
        .unwrap_or_else(|error| panic!("custody: {error}"))
    }

    fn budget_binding() -> BudgetBinding {
        BudgetBinding::new(account(BUDGET), account(OWNER))
            .unwrap_or_else(|error| panic!("budget binding: {error}"))
    }

    fn observed(
        custody_context: Option<CustodyContext>,
        balances: Vec<BalanceEntry>,
        allowances: Vec<SignedAllowance>,
    ) -> ObservedState {
        ObservedState::new(
            TimestampSeconds::from_u64(1_000),
            account(OWNER),
            custody_context,
            balances,
            allowances,
            vec![budget_binding()],
            fees(),
        )
        .unwrap_or_else(|error| panic!("observed: {error}"))
    }

    fn constraints(allow_top_up: bool) -> Constraints {
        Constraints::new(TimestampSeconds::from_u64(2_000), 1_000, allow_top_up)
    }

    fn intent(source: Endpoint, destination: Endpoint, amount: u128) -> UnifiedIntent {
        UnifiedIntent::new(
            source,
            destination,
            ASSET,
            Amount::from_u128(amount),
            constraints(true),
        )
        .unwrap_or_else(|error| panic!("intent: {error}"))
    }

    fn balance(endpoint: Endpoint, available: u128) -> BalanceEntry {
        BalanceEntry::new(endpoint, ASSET, Amount::from_u128(available))
    }

    fn top_up_allowance(seed: u8, cap: u128) -> SignedAllowance {
        SignedAllowance::new(
            allowance_id(seed),
            AllowanceKind::BudgetAllowance,
            AllowanceScope::new(
                home(),
                budget(),
                ASSET,
                LegMechanism::Protocol(Mechanism::BudgetFund),
            ),
            Amount::from_u128(cap),
            Amount::from_u128(cap),
            TimestampSeconds::from_u64(5_000),
        )
        .unwrap_or_else(|error| panic!("allowance: {error}"))
    }

    fn planned(
        source: Endpoint,
        destination: Endpoint,
        amount: u128,
        state: &ObservedState,
    ) -> UnifiedPlan {
        let intent = intent(source, destination, amount);
        plan(&intent, state).unwrap_or_else(|error| panic!("plan: {error}"))
    }

    fn mechanisms(value: &UnifiedPlan) -> Vec<&'static str> {
        value
            .legs()
            .iter()
            .map(|leg| leg.mechanism().label())
            .collect()
    }

    #[test]
    fn same_domain_send_resolves_to_one_transfer_leg() {
        let state = observed(None, vec![balance(home(), 1_000)], Vec::new());
        let value = planned(home(), agent(), 100, &state);
        assert_eq!(mechanisms(&value), vec!["send"]);
        assert_eq!(value.legs().len(), 1);
        assert_eq!(value.total_fee(), 5);
        assert_eq!(value.journey_kind(), JourneyKind::Move);
        let leg = value.legs().first().unwrap_or_else(|| panic!("leg"));
        assert_eq!(leg.term(), MovementTerm::Transfer);
        assert_eq!(leg.domain(), Domain::LayerX);
        assert_eq!(leg.amount().value(), 100);
        assert_eq!(leg.authority(), RequiredAuthority::AccountOwner);
        assert!(leg.top_up().is_none());
    }

    #[test]
    fn identical_inputs_produce_byte_identical_plans() {
        let state = observed(None, vec![balance(home(), 1_000)], Vec::new());
        let first = planned(home(), agent(), 100, &state);
        let second = planned(home(), agent(), 100, &state);
        assert_eq!(first.canonical_encode(), second.canonical_encode());
        assert_eq!(first.digest(), second.digest());
    }

    #[test]
    fn permuted_observation_order_produces_the_same_plan() {
        let balances = vec![
            balance(home(), 1_000),
            balance(budget(), 100),
            balance(agent(), 7),
        ];
        let mut reversed = balances.clone();
        reversed.reverse();
        let allowances = vec![top_up_allowance(1, 5), top_up_allowance(2, 5)];
        let mut swapped = allowances.clone();
        swapped.reverse();
        let first = observed(None, balances, allowances);
        let second = observed(None, reversed, swapped);
        assert_eq!(first, second);
        let left = planned(budget(), agent(), 100, &first);
        let right = planned(budget(), agent(), 100, &second);
        assert_eq!(left.canonical_encode(), right.canonical_encode());
    }

    #[test]
    fn paxeer_source_plans_custody_deposit_credit_then_send() {
        let state = observed(
            Some(custody()),
            vec![balance(Endpoint::PaxeerWallet, 1_000)],
            Vec::new(),
        );
        let value = planned(Endpoint::PaxeerWallet, agent(), 100, &state);
        assert_eq!(
            mechanisms(&value),
            vec!["paxeer-custody-deposit", "bridge-deposit-credit", "send"]
        );
        assert_eq!(value.journey_kind(), JourneyKind::Deposit);
        let domains = value
            .legs()
            .iter()
            .map(PlannedLeg::domain)
            .collect::<Vec<_>>();
        assert_eq!(
            domains,
            vec![Domain::Paxeer, Domain::LayerX, Domain::LayerX]
        );
        let first = value.legs().first().unwrap_or_else(|| panic!("first leg"));
        assert_eq!(first.amount().value(), 108);
        let last = value.legs().last().unwrap_or_else(|| panic!("last leg"));
        assert_eq!(last.amount().value(), 100);
        assert_eq!(value.total_fee(), 9);
    }

    #[test]
    fn paxeer_destination_plans_withdraw_request_then_finalise() {
        let state = observed(Some(custody()), vec![balance(home(), 1_000)], Vec::new());
        let value = planned(home(), Endpoint::PaxeerWallet, 100, &state);
        assert_eq!(
            mechanisms(&value),
            vec!["bridge-withdraw-request", "paxeer-withdraw-finalise"]
        );
        assert_eq!(value.journey_kind(), JourneyKind::Withdraw);
        assert_eq!(value.total_fee(), 5);
        let finalise = value.legs().last().unwrap_or_else(|| panic!("finalise"));
        assert_eq!(finalise.domain(), Domain::Paxeer);
        assert_eq!(finalise.authority(), RequiredAuthority::PaxeerWalletKey);
    }

    #[test]
    fn agent_budget_funding_resolves_to_one_fund_leg() {
        let state = observed(None, vec![balance(home(), 1_000)], Vec::new());
        let value = planned(home(), budget(), 100, &state);
        assert_eq!(mechanisms(&value), vec!["budget-fund"]);
        let leg = value.legs().first().unwrap_or_else(|| panic!("leg"));
        assert_eq!(leg.term(), MovementTerm::Fund);
    }

    #[test]
    fn a_top_up_exactly_at_the_cap_is_authorized_and_recorded() {
        let state = observed(
            None,
            vec![balance(budget(), 100), balance(home(), 1_000)],
            vec![top_up_allowance(1, 5)],
        );
        let value = planned(budget(), agent(), 100, &state);
        assert_eq!(mechanisms(&value), vec!["budget-fund", "send"]);
        let top_up = value.legs().first().unwrap_or_else(|| panic!("top-up leg"));
        let record = top_up.top_up().unwrap_or_else(|| panic!("top-up record"));
        assert_eq!(record.allowance(), allowance_id(1));
        assert_eq!(record.consumed().value(), 5);
        assert_eq!(record.kind(), AllowanceKind::BudgetAllowance);
        assert_eq!(
            top_up.authority(),
            RequiredAuthority::Allowance(allowance_id(1))
        );
        assert_eq!(top_up.amount().value(), 5);
    }

    #[test]
    fn a_top_up_one_unit_over_the_cap_is_refused() {
        let state = observed(
            None,
            vec![balance(budget(), 100), balance(home(), 1_000)],
            vec![top_up_allowance(1, 4)],
        );
        let refusal = plan(&intent(budget(), agent(), 100), &state)
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(refusal, Refusal::TopUpNotAuthorized { shortfall: 5 });
    }

    #[test]
    fn a_top_up_without_any_allowance_is_refused() {
        let state = observed(
            None,
            vec![balance(budget(), 100), balance(home(), 1_000)],
            Vec::new(),
        );
        let refusal = plan(&intent(budget(), agent(), 100), &state)
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(refusal, Refusal::TopUpNotAuthorized { shortfall: 5 });
    }

    #[test]
    fn an_expired_allowance_never_covers_a_top_up() {
        let expired = SignedAllowance::new(
            allowance_id(1),
            AllowanceKind::BudgetAllowance,
            AllowanceScope::new(
                home(),
                budget(),
                ASSET,
                LegMechanism::Protocol(Mechanism::BudgetFund),
            ),
            Amount::from_u128(50),
            Amount::from_u128(50),
            TimestampSeconds::from_u64(999),
        )
        .unwrap_or_else(|error| panic!("allowance: {error}"));
        let state = observed(
            None,
            vec![balance(budget(), 100), balance(home(), 1_000)],
            vec![expired],
        );
        let refusal = plan(&intent(budget(), agent(), 100), &state)
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(refusal, Refusal::TopUpNotAuthorized { shortfall: 5 });
    }

    #[test]
    fn a_top_up_is_never_planned_when_the_intent_forbids_it() {
        let state = observed(
            None,
            vec![balance(budget(), 100), balance(home(), 1_000)],
            vec![top_up_allowance(1, 5)],
        );
        let forbidden = UnifiedIntent::new(
            budget(),
            agent(),
            ASSET,
            Amount::from_u128(100),
            constraints(false),
        )
        .unwrap_or_else(|error| panic!("intent: {error}"));
        let refusal = plan(&forbidden, &state)
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(refusal, Refusal::TopUpNotPermitted { shortfall: 5 });
    }

    #[test]
    fn a_fee_ceiling_below_the_cheapest_plan_is_refused() {
        let state = observed(None, vec![balance(home(), 1_000)], Vec::new());
        let capped = UnifiedIntent::new(
            home(),
            agent(),
            ASSET,
            Amount::from_u128(100),
            Constraints::new(TimestampSeconds::from_u64(2_000), 1, true),
        )
        .unwrap_or_else(|error| panic!("intent: {error}"));
        let refusal = plan(&capped, &state)
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(
            refusal,
            Refusal::FeeCeilingExceeded {
                required: 5,
                ceiling: 1
            }
        );
    }

    #[test]
    fn an_observation_after_the_deadline_is_refused() {
        let state = observed(None, vec![balance(home(), 1_000)], Vec::new());
        let stale = UnifiedIntent::new(
            home(),
            agent(),
            ASSET,
            Amount::from_u128(100),
            Constraints::new(TimestampSeconds::from_u64(10), 1_000, true),
        )
        .unwrap_or_else(|error| panic!("intent: {error}"));
        let refusal = plan(&stale, &state)
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(
            refusal,
            Refusal::DeadlinePassed {
                deadline: 10,
                observed_at: 1_000
            }
        );
    }

    #[test]
    fn a_wallet_to_wallet_intent_has_no_route() {
        let state = observed(
            Some(custody()),
            vec![balance(Endpoint::PaxeerWallet, 1_000)],
            Vec::new(),
        );
        let refusal = plan(
            &intent(Endpoint::PaxeerWallet, home(), 100),
            &observed(
                None,
                vec![balance(Endpoint::PaxeerWallet, 1_000)],
                Vec::new(),
            ),
        )
        .err()
        .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(refusal, Refusal::WalletNotBound);
        let unroutable = UnifiedIntent::new(
            Endpoint::PaxeerWallet,
            Endpoint::PaxeerWallet,
            ASSET,
            Amount::from_u128(100),
            constraints(true),
        );
        assert_eq!(unroutable.err(), Some(Refusal::EndpointsIdentical));
        let _ = state;
    }

    struct SilentAdvisor;

    impl RouteAdvisor for SilentAdvisor {
        fn advise(&self, _candidates: &CandidateSet<'_>) -> Advice {
            Advice::none()
        }
    }

    struct LastCandidateAdvisor;

    impl RouteAdvisor for LastCandidateAdvisor {
        fn advise(&self, candidates: &CandidateSet<'_>) -> Advice {
            let offered = candidates.candidates();
            match offered.last() {
                None => Advice::none(),
                Some((handle, _)) => Advice::prefer(*handle),
            }
        }
    }

    struct CountingAdvisor(std::cell::Cell<usize>);

    impl RouteAdvisor for CountingAdvisor {
        fn advise(&self, candidates: &CandidateSet<'_>) -> Advice {
            self.0.set(candidates.len());
            Advice::none()
        }
    }

    #[test]
    fn the_chosen_plan_is_independent_of_an_absent_advisor() {
        let state = observed(
            None,
            vec![balance(budget(), 100), balance(home(), 1_000)],
            vec![top_up_allowance(1, 5), top_up_allowance(2, 5)],
        );
        let stated = intent(budget(), agent(), 100);
        let without = plan(&stated, &state).unwrap_or_else(|error| panic!("plan: {error}"));
        let with = plan_with_advisor(&stated, &state, &SilentAdvisor)
            .unwrap_or_else(|error| panic!("advised plan: {error}"));
        assert_eq!(without.canonical_encode(), with.plan().canonical_encode());
        assert!(with.notes().is_empty());
    }

    #[test]
    fn an_advisor_only_chooses_among_the_routers_tied_candidates() {
        let state = observed(
            None,
            vec![balance(budget(), 100), balance(home(), 1_000)],
            vec![top_up_allowance(1, 5), top_up_allowance(2, 5)],
        );
        let stated = intent(budget(), agent(), 100);
        let canonical = plan(&stated, &state).unwrap_or_else(|error| panic!("plan: {error}"));
        let advised = plan_with_advisor(&stated, &state, &LastCandidateAdvisor)
            .unwrap_or_else(|error| panic!("advised plan: {error}"));
        assert_eq!(mechanisms(&canonical), mechanisms(advised.plan()));
        assert_eq!(canonical.total_fee(), advised.plan().total_fee());
        assert_eq!(canonical.legs().len(), advised.plan().legs().len());
        let canonical_choice = canonical
            .legs()
            .first()
            .and_then(PlannedLeg::top_up)
            .map(TopUpRecord::allowance);
        let advised_choice = advised
            .plan()
            .legs()
            .first()
            .and_then(PlannedLeg::top_up)
            .map(TopUpRecord::allowance);
        assert_eq!(canonical_choice, Some(allowance_id(1)));
        assert_eq!(advised_choice, Some(allowance_id(2)));
    }

    #[test]
    fn an_advisor_is_never_offered_a_strictly_worse_candidate() {
        let wallet_allowance = |seed: u8, mechanism: LegMechanism, source: Endpoint| {
            SignedAllowance::new(
                allowance_id(seed),
                AllowanceKind::DelegatedCapability,
                AllowanceScope::new(source, home(), ASSET, mechanism),
                Amount::from_u128(1_000),
                Amount::from_u128(1_000),
                TimestampSeconds::from_u64(5_000),
            )
            .unwrap_or_else(|error| panic!("allowance: {error}"))
        };
        let state = observed(
            Some(custody()),
            vec![
                balance(budget(), 100),
                balance(home(), 1_000),
                balance(Endpoint::PaxeerWallet, 1_000),
            ],
            vec![
                top_up_allowance(1, 5),
                wallet_allowance(
                    3,
                    LegMechanism::PaxeerCustodyDeposit,
                    Endpoint::PaxeerWallet,
                ),
                wallet_allowance(
                    4,
                    LegMechanism::Protocol(Mechanism::BridgeDepositCredit),
                    Endpoint::PaxeerWallet,
                ),
            ],
        );
        let stated = intent(budget(), agent(), 100);
        let counting = CountingAdvisor(std::cell::Cell::new(usize::MAX));
        let advised = plan_with_advisor(&stated, &state, &counting)
            .unwrap_or_else(|error| panic!("advised plan: {error}"));
        assert_eq!(counting.0.get(), 1);
        assert_eq!(mechanisms(advised.plan()), vec!["budget-fund", "send"]);
        let chosen = plan_with_advisor(&stated, &state, &LastCandidateAdvisor)
            .unwrap_or_else(|error| panic!("advised plan: {error}"));
        let canonical = plan(&stated, &state).unwrap_or_else(|error| panic!("plan: {error}"));
        assert_eq!(
            chosen.plan().canonical_encode(),
            canonical.canonical_encode()
        );
    }

    fn fund_binding(value: &UnifiedPlan, index: usize) -> LegBinding {
        let action_key = value
            .action_key(index)
            .unwrap_or_else(|error| panic!("action key: {error}"));
        LegBinding::new(
            index,
            action_key,
            Relationship::ManagedBudget(BudgetRoute {
                budget_id: BudgetId::new([4; 32]),
                idempotency_key: IdempotencyKey::new([6; 32]),
                revocation_sequence: Sequence::from_u64(0),
                create: None,
            }),
            AgentDid::new("did:layerx:alice").unwrap_or_else(|error| panic!("actor: {error:?}")),
            AuthorityRef::new("owner:alice").unwrap_or_else(|error| panic!("authority: {error:?}")),
            3,
            900,
            1_500,
            2,
        )
        .unwrap_or_else(|error| panic!("binding: {error}"))
    }

    #[test]
    fn a_signed_plan_becomes_a_journey_plan_keyed_by_its_digest() {
        let state = observed(None, vec![balance(home(), 1_000)], Vec::new());
        let value = planned(home(), budget(), 100, &state);
        let requirements = value
            .signing_requirements()
            .unwrap_or_else(|error| panic!("requirements: {error}"));
        assert_eq!(requirements.len(), 1);
        let requirement = requirements
            .first()
            .unwrap_or_else(|| panic!("requirement"));
        assert_eq!(requirement.leg_index(), 0);
        assert_eq!(requirement.domain(), Domain::LayerX);
        assert_ne!(requirement.action_key(), requirement.signing_context());
        let binding = fund_binding(&value, 0);
        let signed = SignedPlan::accept(value.clone(), value.digest(), vec![binding])
            .unwrap_or_else(|error| panic!("accept: {error}"));
        let journey = signed
            .journey_plan(
                JourneyId::new("jrn_unifiedrouter")
                    .unwrap_or_else(|error| panic!("journey id: {error}")),
                KeyId::new("human-primary").unwrap_or_else(|error| panic!("key: {error}")),
                Operation::ProtocolMutation,
            )
            .unwrap_or_else(|error| panic!("journey plan: {error}"));
        let request = RouteRequest::from_wire_parts(
            home(),
            budget(),
            Relationship::ManagedBudget(BudgetRoute {
                budget_id: BudgetId::new([4; 32]),
                idempotency_key: IdempotencyKey::new([6; 32]),
                revocation_sequence: Sequence::from_u64(0),
                create: None,
            }),
            ASSET,
            Amount::from_u128(100),
        )
        .unwrap_or_else(|error| panic!("route request: {error}"));
        let route =
            RouteResolver::resolve(&request).unwrap_or_else(|error| panic!("resolve: {error}"));
        let resolved = route.legs().first().unwrap_or_else(|| panic!("route leg"));
        assert_eq!(resolved.mechanism(), Mechanism::BudgetFund);
        let expected_leg = JourneyLeg::new(
            resolved.intent().clone(),
            value
                .action_key(0)
                .unwrap_or_else(|error| panic!("action key: {error}")),
            AgentDid::new("did:layerx:alice").unwrap_or_else(|error| panic!("actor: {error:?}")),
            AuthorityRef::new("owner:alice").unwrap_or_else(|error| panic!("authority: {error:?}")),
            3,
            900,
            1_500,
            2,
        )
        .unwrap_or_else(|error| panic!("journey leg: {error}"));
        let expected = JourneyPlan::new(
            JourneyId::new("jrn_unifiedrouter")
                .unwrap_or_else(|error| panic!("journey id: {error}")),
            JourneyKind::Move,
            value.digest(),
            KeyId::new("human-primary").unwrap_or_else(|error| panic!("key: {error}")),
            Operation::ProtocolMutation,
            vec![expected_leg],
        )
        .unwrap_or_else(|error| panic!("expected plan: {error}"));
        assert_eq!(journey, expected);
    }

    #[test]
    fn signed_material_is_refused_against_a_different_plan_digest() {
        let state = observed(None, vec![balance(home(), 1_000)], Vec::new());
        let value = planned(home(), budget(), 100, &state);
        let binding = fund_binding(&value, 0);
        let refusal = SignedPlan::accept(value, [1; 32], vec![binding])
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(refusal, Refusal::PlanDigestMismatch);
    }

    #[test]
    fn altering_a_leg_after_signing_changes_every_derived_key() {
        let state = observed(None, vec![balance(home(), 1_000)], Vec::new());
        let signed_plan = planned(home(), budget(), 100, &state);
        let altered = planned(home(), budget(), 101, &state);
        assert_ne!(signed_plan.digest(), altered.digest());
        let signed_key = signed_plan
            .action_key(0)
            .unwrap_or_else(|error| panic!("action key: {error}"));
        let altered_key = altered
            .action_key(0)
            .unwrap_or_else(|error| panic!("action key: {error}"));
        assert_ne!(signed_key, altered_key);
        let stale = fund_binding(&signed_plan, 0);
        let refusal = SignedPlan::accept(altered, signed_plan.digest(), vec![stale])
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(refusal, Refusal::PlanDigestMismatch);
    }

    #[test]
    fn a_binding_carrying_a_foreign_action_key_is_refused() {
        let state = observed(None, vec![balance(home(), 1_000)], Vec::new());
        let value = planned(home(), budget(), 100, &state);
        let other = planned(home(), budget(), 101, &state);
        let stale = fund_binding(&other, 0);
        let refusal = SignedPlan::accept(value.clone(), value.digest(), vec![stale])
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(refusal, Refusal::LegMismatch { index: 0 });
    }

    #[test]
    fn every_protocol_leg_must_be_bound() {
        let state = observed(
            None,
            vec![balance(budget(), 100), balance(home(), 1_000)],
            vec![top_up_allowance(1, 5)],
        );
        let value = planned(budget(), agent(), 100, &state);
        assert_eq!(value.legs().len(), 2);
        let refusal = SignedPlan::accept(value.clone(), value.digest(), Vec::new())
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(
            refusal,
            Refusal::UnboundLegs {
                expected: 2,
                bound: 0
            }
        );
    }

    #[test]
    fn an_unquoted_mechanism_is_refused_rather_than_defaulted() {
        let partial = FeeSchedule::new(vec![(LegMechanism::Protocol(Mechanism::BudgetFund), 2)])
            .unwrap_or_else(|error| panic!("fees: {error}"));
        let state = ObservedState::new(
            TimestampSeconds::from_u64(1_000),
            account(OWNER),
            None,
            vec![balance(home(), 1_000)],
            Vec::new(),
            vec![budget_binding()],
            partial,
        )
        .unwrap_or_else(|error| panic!("observed: {error}"));
        let refusal = plan(&intent(home(), agent(), 100), &state)
            .err()
            .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(refusal, Refusal::FeeUnknown { mechanism: "send" });
    }

    #[test]
    fn a_duplicated_observation_is_refused() {
        let duplicated = ObservedState::new(
            TimestampSeconds::from_u64(1_000),
            account(OWNER),
            None,
            vec![balance(home(), 1), balance(home(), 2)],
            Vec::new(),
            vec![budget_binding()],
            fees(),
        );
        assert_eq!(duplicated.err(), Some(Refusal::DuplicateObservation));
    }
}
