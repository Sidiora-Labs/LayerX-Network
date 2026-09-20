//! One truthful observed state for the unified intent router.
//!
//! [`plan`](super::plan) is pure: it consumes an [`ObservedState`] and never
//! reads the network itself. This module is the only place that assembles that
//! snapshot, and it holds one rule above all others: **a state that is not
//! fully known is never planned on**. Every source that cannot answer produces
//! a typed [`ObservationError`] naming the source, and no absent reading is
//! ever substituted with a zero, an empty table or a default.
//!
//! The snapshot has three kinds of source, each read from the one place that
//! is authoritative for it:
//!
//! * **The bound wallet and its Paxeer balances** come from the Paxeer X
//!   Network gateway's `px_*` joins, through the shared
//!   [`layerx_network_gateway`] client. The gateway is the only party that sees
//!   both domains, and its answers stay tagged as gateway-reported.
//! * **The LayerX-side spendable balances, the allowance inventory and the
//!   budget bindings** come from the service's own authoritative records — the
//!   agent runtime's balance and budget reads and the journeys resolver's
//!   route inputs. They are never taken from the caller's request: a request
//!   states an intent, never the headroom that would authorise it.
//! * **The fee schedule and limits** come from the same component configuration
//!   that move, deposit and withdraw already take them from.
//!
//! [`ObservedStateBuilder`] refuses to build until every one of those sources
//! has answered, so a partially observed state cannot reach the planner.

use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

use layerx_network_gateway::{AccountBalances, AccountIdentifier, GatewayEndpoint, GatewayError};
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::AssetId;
use layerx_types::intent::{EvmAddress, TimestampSeconds};

use super::{
    BalanceEntry, BudgetBinding, CustodyContext, Endpoint, FeeSchedule, ObservedState, Refusal,
    SignedAllowance,
};

/// A source that could not answer, or an answer that cannot be planned on.
///
/// No variant carries a partial snapshot. Each one names the source that is
/// missing so the refusal the caller sees says which read failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservationError {
    /// The network gateway could not be reached or refused the read.
    Gateway(GatewayError),
    /// The gateway does not report a Paxeer wallet bound to this account.
    WalletNotBound,
    /// The gateway reports a different account than the one asked for.
    IdentityMismatch,
    /// The joined balance table carries no row for an asset the plan needs, so
    /// the wallet holding of that asset is unknown rather than zero.
    AssetNotJoined {
        /// The asset whose wallet holding could not be observed.
        asset: AssetId,
    },
    /// The joined row exists but reports no Paxeer half, so the wallet holding
    /// of that asset is unknown rather than zero.
    WalletBalanceUnknown {
        /// The asset whose wallet holding could not be observed.
        asset: AssetId,
    },
    /// The service's own spendable-balance read has not answered.
    LedgerBalancesUnavailable,
    /// The service's own allowance records have not answered.
    AllowanceInventoryUnavailable,
    /// The service's own budget-binding records have not answered.
    BudgetInventoryUnavailable,
    /// The component configuration has not supplied the fee schedule.
    FeeScheduleUnavailable,
    /// The same endpoint and asset was observed twice from two sources.
    DuplicateObservation {
        /// The asset observed twice for one endpoint.
        asset: AssetId,
    },
    /// The assembled snapshot is not one the router accepts.
    Invalid(Refusal),
}

impl Display for ObservationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gateway(error) => write!(formatter, "network gateway read failed: {error}"),
            Self::WalletNotBound => {
                formatter.write_str("no Paxeer wallet is bound to this account")
            }
            Self::IdentityMismatch => {
                formatter.write_str("the gateway answered for a different account")
            }
            Self::AssetNotJoined { asset } => write!(
                formatter,
                "the joined balance table carries no row for asset {}",
                hex(&asset.bytes())
            ),
            Self::WalletBalanceUnknown { asset } => write!(
                formatter,
                "the wallet holding of asset {} is unknown",
                hex(&asset.bytes())
            ),
            Self::LedgerBalancesUnavailable => {
                formatter.write_str("spendable balances are unavailable")
            }
            Self::AllowanceInventoryUnavailable => {
                formatter.write_str("the allowance inventory is unavailable")
            }
            Self::BudgetInventoryUnavailable => {
                formatter.write_str("budget bindings are unavailable")
            }
            Self::FeeScheduleUnavailable => formatter.write_str("the fee schedule is unavailable"),
            Self::DuplicateObservation { asset } => write!(
                formatter,
                "asset {} was observed twice for one endpoint",
                hex(&asset.bytes())
            ),
            Self::Invalid(refusal) => {
                write!(formatter, "the observed state is not plannable: {refusal}")
            }
        }
    }
}

impl std::error::Error for ObservationError {}

impl From<GatewayError> for ObservationError {
    fn from(error: GatewayError) -> Self {
        Self::Gateway(error)
    }
}

impl From<Refusal> for ObservationError {
    fn from(refusal: Refusal) -> Self {
        Self::Invalid(refusal)
    }
}

const DIGITS: &[u8; 16] = b"0123456789abcdef";

fn hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// What the network gateway reported about one account: the binding and the
/// Paxeer half of its joined balances.
///
/// This is the only part of the snapshot the service does not hold itself. It
/// stays separate from the service's own records so the two can never be
/// confused for one another.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkObservation {
    wallet: EvmAddress,
    balances: AccountBalances,
}

impl NetworkObservation {
    /// Reads the binding and joined balances for one account from the gateway.
    ///
    /// # Errors
    ///
    /// Reports the gateway's own refusal, and [`ObservationError::WalletNotBound`]
    /// when the network knows no wallet for this account.
    pub fn read(
        endpoint: &GatewayEndpoint,
        account: AccountIdentifier,
    ) -> Result<Self, ObservationError> {
        Self::accept(endpoint.get_balances(account)?)
    }

    /// Accepts one already-read joined balance answer.
    ///
    /// # Errors
    ///
    /// Refuses an answer that reports no bound wallet.
    pub fn accept(balances: AccountBalances) -> Result<Self, ObservationError> {
        if !balances.account.bound {
            return Err(ObservationError::WalletNotBound);
        }
        let wallet = balances
            .account
            .evm_address
            .ok_or(ObservationError::WalletNotBound)?;
        Ok(Self {
            wallet: EvmAddress::new(wallet),
            balances,
        })
    }

    /// The bound Paxeer wallet the custody context is built around.
    #[must_use]
    pub const fn wallet(&self) -> EvmAddress {
        self.wallet
    }

    /// The joined answer exactly as the gateway reported it.
    #[must_use]
    pub const fn joined(&self) -> &AccountBalances {
        &self.balances
    }

    /// The wallet holding of one asset.
    ///
    /// # Errors
    ///
    /// Refuses rather than reporting zero when the asset has no joined row, or
    /// when the row carries no Paxeer half. An unobserved holding is never a
    /// zero holding.
    pub fn wallet_balance(&self, asset: AssetId) -> Result<Amount, ObservationError> {
        let row = self
            .balances
            .asset(asset.bytes())
            .ok_or(ObservationError::AssetNotJoined { asset })?;
        let paxeer = row
            .paxeer
            .as_ref()
            .ok_or(ObservationError::WalletBalanceUnknown { asset })?;
        Ok(Amount::from_u128(paxeer.amount))
    }
}

/// Assembles one [`ObservedState`] and refuses to produce a partial one.
///
/// Every source is absent until it answers. [`ObservedStateBuilder::build`]
/// names the first source that never did, so a snapshot can never be planned on
/// while a read is still missing.
#[derive(Clone, Debug)]
pub struct ObservedStateBuilder {
    observed_at: TimestampSeconds,
    owner: AccountId,
    reserve: AccountId,
    withdrawals: AccountId,
    network: Option<NetworkObservation>,
    ledger: Option<Vec<BalanceEntry>>,
    allowances: Option<Vec<SignedAllowance>>,
    budgets: Option<Vec<BudgetBinding>>,
    fees: Option<FeeSchedule>,
    required: BTreeSet<[u8; 32]>,
}

impl ObservedStateBuilder {
    /// Starts one snapshot for one owner at one observation time.
    ///
    /// The two system accounts are the custody reserve and withdrawal accounts
    /// the deployment already uses for deposits and withdrawals.
    #[must_use]
    pub const fn new(
        observed_at: TimestampSeconds,
        owner: AccountId,
        reserve: AccountId,
        withdrawals: AccountId,
    ) -> Self {
        Self {
            observed_at,
            owner,
            reserve,
            withdrawals,
            network: None,
            ledger: None,
            allowances: None,
            budgets: None,
            fees: None,
            required: BTreeSet::new(),
        }
    }

    /// Records what the gateway reported about the binding and wallet balances.
    #[must_use]
    pub fn with_network(mut self, network: NetworkObservation) -> Self {
        self.network = Some(network);
        self
    }

    /// Records the service's own authoritative spendable balances.
    #[must_use]
    pub fn with_ledger_balances(mut self, balances: Vec<BalanceEntry>) -> Self {
        self.ledger = Some(balances);
        self
    }

    /// Records the allowance inventory read from the service's own records.
    ///
    /// These are budget allowances, payer grants and delegated capabilities
    /// with their remaining caps as the service holds them. They are never
    /// taken from a caller's request.
    #[must_use]
    pub fn with_allowances(mut self, allowances: Vec<SignedAllowance>) -> Self {
        self.allowances = Some(allowances);
        self
    }

    /// Records the budget bindings read from the service's own records.
    #[must_use]
    pub fn with_budget_bindings(mut self, budgets: Vec<BudgetBinding>) -> Self {
        self.budgets = Some(budgets);
        self
    }

    /// Records the fee schedule taken from the component configuration.
    #[must_use]
    pub fn with_fees(mut self, fees: FeeSchedule) -> Self {
        self.fees = Some(fees);
        self
    }

    /// Declares one asset the plan may move, so its wallet holding must be
    /// observed rather than assumed.
    #[must_use]
    pub fn requiring(mut self, asset: AssetId) -> Self {
        self.required.insert(asset.bytes());
        self
    }

    /// Assembles the snapshot.
    ///
    /// # Errors
    ///
    /// Names the first source that has not answered, the first required asset
    /// whose wallet holding is unobserved, a duplicate observation of one
    /// endpoint and asset, and any snapshot the router itself refuses.
    pub fn build(self) -> Result<ObservedState, ObservationError> {
        let network = self.network.ok_or(ObservationError::WalletNotBound)?;
        let ledger = self
            .ledger
            .ok_or(ObservationError::LedgerBalancesUnavailable)?;
        let allowances = self
            .allowances
            .ok_or(ObservationError::AllowanceInventoryUnavailable)?;
        let budgets = self
            .budgets
            .ok_or(ObservationError::BudgetInventoryUnavailable)?;
        let fees = self.fees.ok_or(ObservationError::FeeScheduleUnavailable)?;

        let custody =
            CustodyContext::new(network.wallet(), self.reserve.clone(), self.withdrawals.clone())?;

        let mut balances = ledger;
        let mut seen = BTreeSet::new();
        for entry in &balances {
            if !seen.insert((endpoint_key(entry.endpoint()), entry.asset().bytes())) {
                return Err(ObservationError::DuplicateObservation {
                    asset: entry.asset(),
                });
            }
        }
        for asset in self.required.iter().copied().map(AssetId::new) {
            let available = network.wallet_balance(asset)?;
            if !seen.insert((endpoint_key(&Endpoint::PaxeerWallet), asset.bytes())) {
                return Err(ObservationError::DuplicateObservation { asset });
            }
            balances.push(BalanceEntry::new(Endpoint::PaxeerWallet, asset, available));
        }

        ObservedState::new(
            self.observed_at,
            self.owner,
            Some(custody),
            balances,
            allowances,
            budgets,
            fees,
        )
        .map_err(ObservationError::Invalid)
    }
}

fn endpoint_key(endpoint: &Endpoint) -> (u8, String) {
    match endpoint {
        Endpoint::PaxeerWallet => (0, String::new()),
        Endpoint::Human(account) => (1, account.canonical().to_owned()),
        Endpoint::Agent(account) => (2, account.canonical().to_owned()),
        Endpoint::AgentBudget(account) => (3, account.canonical().to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use layerx_network_gateway::{
        AssetBalance, Evidence, PaxeerAssetBalance, ResolvedIdentities,
    };

    use crate::journeys::{
        plan, Constraints, LegMechanism, Mechanism, UnifiedIntent,
    };

    const ASSET: AssetId = AssetId::new([9; 32]);
    const OTHER: AssetId = AssetId::new([8; 32]);
    const OWNER: &str = "agent:did:layerx:alice:main";
    const AGENT: &str = "agent:did:layerx:bob:main";

    fn account(value: &str) -> AccountId {
        AccountId::parse(value).unwrap_or_else(|error| panic!("account {value}: {error:?}"))
    }

    fn home() -> Endpoint {
        Endpoint::human(account(OWNER)).unwrap_or_else(|error| panic!("home: {error:?}"))
    }

    fn agent() -> Endpoint {
        Endpoint::agent(account(AGENT)).unwrap_or_else(|error| panic!("agent: {error:?}"))
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

    fn identities(bound: bool) -> ResolvedIdentities {
        ResolvedIdentities {
            evm_address: bound.then_some([7; 20]),
            pax_address: None,
            layerx_did: None,
            layerx_account: None,
            bound,
            evidence: Evidence::GatewayReported,
        }
    }

    fn row(asset: AssetId, paxeer: Option<u128>) -> AssetBalance {
        AssetBalance {
            asset_id: asset.bytes(),
            denom: Some("upax".to_owned()),
            custody: None,
            paxeer: paxeer.map(|amount| PaxeerAssetBalance {
                denom: "upax".to_owned(),
                amount,
            }),
            layerx: None,
        }
    }

    fn joined(bound: bool, balances: Vec<AssetBalance>) -> AccountBalances {
        AccountBalances {
            account: identities(bound),
            balances,
            joined_limit: 16,
            evidence: Evidence::GatewayReported,
        }
    }

    fn network(balances: Vec<AssetBalance>) -> NetworkObservation {
        NetworkObservation::accept(joined(true, balances))
            .unwrap_or_else(|error| panic!("network: {error}"))
    }

    fn builder() -> ObservedStateBuilder {
        ObservedStateBuilder::new(
            TimestampSeconds::from_u64(1_000),
            account(OWNER),
            account("system:paxeer-reserve"),
            account("system:paxeer-withdrawals"),
        )
    }

    #[test]
    fn an_unbound_account_is_refused_before_any_planning() {
        assert_eq!(
            NetworkObservation::accept(joined(false, Vec::new())),
            Err(ObservationError::WalletNotBound)
        );
    }

    #[test]
    fn every_missing_source_names_itself_rather_than_defaulting() {
        let complete = builder()
            .with_network(network(vec![row(ASSET, Some(400))]))
            .with_ledger_balances(Vec::new())
            .with_allowances(Vec::new())
            .with_budget_bindings(Vec::new())
            .with_fees(fees());
        assert!(complete.clone().build().is_ok());

        let without_ledger = builder()
            .with_network(network(vec![row(ASSET, Some(400))]))
            .with_allowances(Vec::new())
            .with_budget_bindings(Vec::new())
            .with_fees(fees());
        assert_eq!(
            without_ledger.build(),
            Err(ObservationError::LedgerBalancesUnavailable)
        );

        let without_allowances = builder()
            .with_network(network(vec![row(ASSET, Some(400))]))
            .with_ledger_balances(Vec::new())
            .with_budget_bindings(Vec::new())
            .with_fees(fees());
        assert_eq!(
            without_allowances.build(),
            Err(ObservationError::AllowanceInventoryUnavailable)
        );

        let without_budgets = builder()
            .with_network(network(vec![row(ASSET, Some(400))]))
            .with_ledger_balances(Vec::new())
            .with_allowances(Vec::new())
            .with_fees(fees());
        assert_eq!(
            without_budgets.build(),
            Err(ObservationError::BudgetInventoryUnavailable)
        );

        let without_fees = builder()
            .with_network(network(vec![row(ASSET, Some(400))]))
            .with_ledger_balances(Vec::new())
            .with_allowances(Vec::new())
            .with_budget_bindings(Vec::new());
        assert_eq!(
            without_fees.build(),
            Err(ObservationError::FeeScheduleUnavailable)
        );

        let without_network = builder()
            .with_ledger_balances(Vec::new())
            .with_allowances(Vec::new())
            .with_budget_bindings(Vec::new())
            .with_fees(fees());
        assert_eq!(without_network.build(), Err(ObservationError::WalletNotBound));
    }

    #[test]
    fn an_unjoined_asset_is_refused_and_never_observed_as_zero() {
        let refusal = builder()
            .with_network(network(vec![row(OTHER, Some(400))]))
            .with_ledger_balances(Vec::new())
            .with_allowances(Vec::new())
            .with_budget_bindings(Vec::new())
            .with_fees(fees())
            .requiring(ASSET)
            .build();
        assert_eq!(refusal, Err(ObservationError::AssetNotJoined { asset: ASSET }));
    }

    #[test]
    fn a_joined_row_without_a_paxeer_half_is_unknown_not_zero() {
        let refusal = builder()
            .with_network(network(vec![row(ASSET, None)]))
            .with_ledger_balances(Vec::new())
            .with_allowances(Vec::new())
            .with_budget_bindings(Vec::new())
            .with_fees(fees())
            .requiring(ASSET)
            .build();
        assert_eq!(
            refusal,
            Err(ObservationError::WalletBalanceUnknown { asset: ASSET })
        );
    }

    #[test]
    fn a_wallet_balance_observed_twice_is_refused() {
        let refusal = builder()
            .with_network(network(vec![row(ASSET, Some(400))]))
            .with_ledger_balances(vec![BalanceEntry::new(
                Endpoint::PaxeerWallet,
                ASSET,
                Amount::from_u128(1),
            )])
            .with_allowances(Vec::new())
            .with_budget_bindings(Vec::new())
            .with_fees(fees())
            .requiring(ASSET)
            .build();
        assert_eq!(
            refusal,
            Err(ObservationError::DuplicateObservation { asset: ASSET })
        );
    }

    #[test]
    fn the_assembled_snapshot_plans_against_the_real_router() {
        let observed = builder()
            .with_network(network(vec![row(ASSET, Some(400))]))
            .with_ledger_balances(vec![BalanceEntry::new(
                home(),
                ASSET,
                Amount::from_u128(1_000),
            )])
            .with_allowances(Vec::new())
            .with_budget_bindings(Vec::new())
            .with_fees(fees())
            .requiring(ASSET)
            .build()
            .unwrap_or_else(|error| panic!("observed: {error}"));

        let intent = UnifiedIntent::new(
            home(),
            agent(),
            ASSET,
            Amount::from_u128(100),
            Constraints::new(TimestampSeconds::from_u64(2_000), 1_000, false),
        )
        .unwrap_or_else(|error| panic!("intent: {error}"));

        let planned = plan(&intent, &observed).unwrap_or_else(|error| panic!("plan: {error}"));
        assert!(!planned.legs().is_empty());
        assert_ne!(planned.digest(), [0; 32]);
    }

    #[test]
    fn the_wallet_balance_read_carries_the_gateway_reported_amount() {
        let observation = network(vec![row(ASSET, Some(4_096))]);
        assert_eq!(observation.wallet(), EvmAddress::new([7; 20]));
        assert_eq!(
            observation.wallet_balance(ASSET),
            Ok(Amount::from_u128(4_096))
        );
        assert_eq!(observation.joined().evidence, Evidence::GatewayReported);
    }
}
