//! The two relay loops.
//!
//! Inbound (Ethereum -> Paxeer): scan each registered vault's `BridgeDeposit`
//! logs up to `head - finality_depth` through the Ethereum RPC quorum, sign
//! the inbound digest with this instance's attestor key, and submit
//! `bridgeIn` to the precompile at `0x…1016` on Paxeer.
//!
//! Outbound (Paxeer -> Ethereum): scan the precompile's `BridgeOut` logs up to
//! the Paxeer head minus its finality depth, sign the outbound digest, and
//! submit `PaxeerXVault.release` on the burn's Ethereum chain.
//!
//! Inbound (Solana -> Paxeer), when a Solana entry is configured: follow the
//! custody program's deposits up to the head at the configured commitment
//! minus the slot finality depth (see [`crate::solana::observe`]) and submit
//! each through the same `bridgeIn` path under the stream `in:<solana id>`.
//!
//! Idempotency across relayer instances: every submission is preceded by a
//! read of the destination nullifier (`isNullified` on the precompile,
//! `nullified` on the vault), and a transaction that reverts is classified by
//! re-reading that nullifier. When another instance's call consumed it first,
//! this instance's call reverts on the consumed nullifier and the item is
//! recorded `AlreadyBridged`: it is never retried and nothing is minted or
//! released twice. With `threshold > 1` the instances exchange signatures
//! through the cosign directory (see [`crate::cosign`]); every instance
//! selects the same `threshold` lowest-address signers, so whichever instance
//! submits first wins and the others' identical or later calls resolve to
//! `AlreadyBridged`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use layerx_crypto::evm_transaction::Eip1559Call;
use serde_json::{json, Value};

use crate::abi::{
    self, decode_address_list, decode_bool, decode_burn_log, decode_deposit_log,
    decode_get_attestors, decode_get_chain, decode_threshold, encode_bridge_in, encode_get_chain,
    encode_is_nullified, encode_nullified, encode_release, AbiError, LAYERX_BRIDGE_PRECOMPILE,
};
use crate::attestation::{assemble_signatures, SignatureError};
use crate::cosign::CosignDirectory;
use crate::hex;
use crate::journal::{Completion, Entry, Journal, JournalError, Observation, Position, Submission};
use crate::rpc::{JsonRpc, RpcFault};
use crate::signer::{Attestor, KeyError, Submitter};
use crate::solana::observe::{observe_deposits, Finding, SolanaSettings};
use crate::solana::rpc::SolanaRpc;
use crate::solana::SOLANA_CHAIN_ID;
use crate::tx::{self, SignedTransaction};

const MAX_BLOCK_RANGE: u64 = 10_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RelayerError {
    Configuration(String),
    Rpc(RpcFault),
    Abi(AbiError),
    Key(KeyError),
    Journal(JournalError),
    /// A scanned log's block is not the canonical block at its height.
    Reorganised {
        block_number: u64,
    },
    /// This instance's attestor is not in the destination's attestor set.
    NotAttestor,
    /// The estimated gas exceeds the configured gas limit.
    GasLimit {
        estimated: u64,
        limit: u64,
    },
}

impl fmt::Display for RelayerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(detail) => write!(formatter, "configuration: {detail}"),
            Self::Rpc(error) => write!(formatter, "{error}"),
            Self::Abi(error) => write!(formatter, "{error}"),
            Self::Key(error) => write!(formatter, "{error}"),
            Self::Journal(error) => write!(formatter, "{error}"),
            Self::Reorganised { block_number } => {
                write!(
                    formatter,
                    "block {block_number} was reorganised during the scan"
                )
            }
            Self::NotAttestor => {
                formatter.write_str("this attestor is not in the destination attestor set")
            }
            Self::GasLimit { estimated, limit } => {
                write!(
                    formatter,
                    "estimated gas {estimated} exceeds the limit {limit}"
                )
            }
        }
    }
}

impl std::error::Error for RelayerError {}

impl From<RpcFault> for RelayerError {
    fn from(value: RpcFault) -> Self {
        Self::Rpc(value)
    }
}

impl From<AbiError> for RelayerError {
    fn from(value: AbiError) -> Self {
        Self::Abi(value)
    }
}

impl From<KeyError> for RelayerError {
    fn from(value: KeyError) -> Self {
        Self::Key(value)
    }
}

impl From<JournalError> for RelayerError {
    fn from(value: JournalError) -> Self {
        Self::Journal(value)
    }
}

/// Fee and gas bounds for transactions on one destination chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GasPolicy {
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
}

/// One registered Ethereum chain and its `PaxeerXVault`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainSettings {
    pub chain_id: u64,
    pub vault: [u8; 20],
    pub finality_depth: u64,
    pub start_block: u64,
    pub max_block_range: u64,
    pub gas: GasPolicy,
}

/// Paxeer, the other side of every bridge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaxeerSettings {
    /// The Paxeer EVM chain id transactions are signed for.
    pub chain_id: u64,
    pub finality_depth: u64,
    pub start_block: u64,
    pub max_block_range: u64,
    pub gas: GasPolicy,
}

pub struct ChainLink {
    pub settings: ChainSettings,
    pub rpc: Box<dyn JsonRpc>,
    pub submitter: Submitter,
}

pub struct PaxeerLink {
    pub settings: PaxeerSettings,
    pub rpc: Box<dyn JsonRpc>,
    pub submitter: Submitter,
}

pub struct RelayerParts {
    pub attestor: Attestor,
    pub paxeer: PaxeerLink,
    pub chains: Vec<ChainLink>,
    pub journal: Journal,
    pub cosign: Option<CosignDirectory>,
    /// Transactions one item may consume before it is refused for operator
    /// attention.
    pub max_submissions: u32,
}

/// The Solana custody program whose deposits are relayed to Paxeer.
pub struct SolanaLink {
    pub settings: SolanaSettings,
    pub rpc: SolanaRpc,
}

/// Everything a relayer is built from: the Ethereum and Paxeer parts and, when
/// configured, the Solana custody program.
pub struct RelayerAssembly {
    pub parts: RelayerParts,
    pub solana: Option<SolanaLink>,
}

impl From<RelayerParts> for RelayerAssembly {
    fn from(parts: RelayerParts) -> Self {
        Self {
            parts,
            solana: None,
        }
    }
}

/// What one pass did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StepReport {
    pub observed: usize,
    pub submitted: usize,
    pub completed: usize,
    pub waiting: usize,
    pub refused: usize,
}

enum Progress {
    Submitted,
    Completed,
    Waiting,
    Refused,
}

impl StepReport {
    fn count(&mut self, progress: &Progress) {
        match progress {
            Progress::Submitted => self.submitted += 1,
            Progress::Completed => self.completed += 1,
            Progress::Waiting => self.waiting += 1,
            Progress::Refused => self.refused += 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Side {
    Paxeer,
    Chain(usize),
}

/// The inbound scan stream of an Ethereum chain.
#[must_use]
pub fn inbound_stream(chain_id: u64) -> String {
    format!("in:{chain_id}")
}

/// The outbound scan stream of Paxeer.
pub const OUTBOUND_STREAM: &str = "out:paxeer";

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, RelayerError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(RelayerError::Rpc(RpcFault::Malformed))
}

fn quantity_of(value: &Value) -> Result<u64, RelayerError> {
    value
        .as_str()
        .and_then(|text| hex::parse_quantity(text).ok())
        .ok_or(RelayerError::Rpc(RpcFault::Malformed))
}

fn quantity_u128_of(value: &Value) -> Result<u128, RelayerError> {
    value
        .as_str()
        .and_then(|text| hex::parse_quantity_u128(text).ok())
        .ok_or(RelayerError::Rpc(RpcFault::Malformed))
}

fn eth_call(rpc: &dyn JsonRpc, to: &[u8; 20], data: &[u8]) -> Result<Vec<u8>, RelayerError> {
    let value = rpc.call(
        "eth_call",
        json!([{"to": hex::prefixed(to), "data": hex::prefixed(data)}, "latest"]),
    )?;
    value
        .as_str()
        .and_then(|text| hex::decode(text).ok())
        .ok_or(RelayerError::Rpc(RpcFault::Malformed))
}

fn block_number(rpc: &dyn JsonRpc) -> Result<u64, RelayerError> {
    quantity_of(&rpc.call("eth_blockNumber", json!([]))?)
}

fn chain_id(rpc: &dyn JsonRpc) -> Result<u64, RelayerError> {
    quantity_of(&rpc.call("eth_chainId", json!([]))?)
}

fn canonical_hash(rpc: &dyn JsonRpc, number: u64) -> Result<[u8; 32], RelayerError> {
    let block = rpc.call(
        "eth_getBlockByNumber",
        json!([hex::quantity(number), false]),
    )?;
    if quantity_of(block.get("number").unwrap_or(&Value::Null))? != number {
        return Err(RelayerError::Rpc(RpcFault::Malformed));
    }
    hex::fixed(text(&block, "hash")?).map_err(|_| RelayerError::Rpc(RpcFault::Malformed))
}

fn validate_range(start: u64, range: u64, what: &str) -> Result<(), RelayerError> {
    if range == 0 || range > MAX_BLOCK_RANGE || start.checked_add(range).is_none() {
        return Err(RelayerError::Configuration(format!(
            "{what} block range must be 1..={MAX_BLOCK_RANGE}"
        )));
    }
    Ok(())
}

fn validate_gas(gas: &GasPolicy, what: &str) -> Result<(), RelayerError> {
    if gas.gas_limit == 0
        || gas.max_fee_per_gas == 0
        || gas.max_priority_fee_per_gas > gas.max_fee_per_gas
    {
        return Err(RelayerError::Configuration(format!(
            "{what} gas policy must bound gas and fees"
        )));
    }
    Ok(())
}

pub struct Relayer {
    attestor: Attestor,
    paxeer: PaxeerLink,
    chains: Vec<ChainLink>,
    journal: Journal,
    cosign: Option<CosignDirectory>,
    max_submissions: u32,
    solana: Option<SolanaLink>,
}

impl Relayer {
    /// Validates the configuration against the live chains: every chain id
    /// answers as configured and every Ethereum chain is registered on Paxeer
    /// with the configured vault and a finality depth no deeper than ours; a
    /// configured Solana custody program is registered the same way under
    /// [`SOLANA_CHAIN_ID`].
    ///
    /// # Errors
    ///
    /// Refuses inconsistent configuration and unreachable or mismatched chains.
    pub fn new(assembly: impl Into<RelayerAssembly>) -> Result<Self, RelayerError> {
        let RelayerAssembly { parts, solana } = assembly.into();
        let RelayerParts {
            attestor,
            paxeer,
            chains,
            journal,
            cosign,
            max_submissions,
        } = parts;
        if chains.is_empty() || max_submissions == 0 {
            return Err(RelayerError::Configuration(
                "at least one chain and one submission per item are required".to_owned(),
            ));
        }
        let identifiers: BTreeSet<u64> = chains.iter().map(|link| link.settings.chain_id).collect();
        if identifiers.len() != chains.len() || identifiers.contains(&0) {
            return Err(RelayerError::Configuration(
                "chain ids must be distinct and non-zero".to_owned(),
            ));
        }
        validate_range(
            paxeer.settings.start_block,
            paxeer.settings.max_block_range,
            "paxeer",
        )?;
        validate_gas(&paxeer.settings.gas, "paxeer")?;
        if chain_id(paxeer.rpc.as_ref())? != paxeer.settings.chain_id {
            return Err(RelayerError::Configuration(
                "paxeer answers with a different chain id".to_owned(),
            ));
        }
        for link in &chains {
            let settings = link.settings;
            let name = format!("chain {}", settings.chain_id);
            validate_range(settings.start_block, settings.max_block_range, &name)?;
            validate_gas(&settings.gas, &name)?;
            if settings.vault == [0; 20] {
                return Err(RelayerError::Configuration(format!("{name} has no vault")));
            }
            if chain_id(link.rpc.as_ref())? != settings.chain_id {
                return Err(RelayerError::Configuration(format!(
                    "{name} answers with a different chain id"
                )));
            }
            let registration = decode_get_chain(&eth_call(
                paxeer.rpc.as_ref(),
                &LAYERX_BRIDGE_PRECOMPILE,
                &encode_get_chain(settings.chain_id),
            )?)?;
            if !registration.registered
                || registration.vault != settings.vault
                || registration.finality_depth > settings.finality_depth
            {
                return Err(RelayerError::Configuration(format!(
                    "{name} is not registered on paxeer with this vault and finality depth"
                )));
            }
        }
        if let Some(link) = &solana {
            let settings = link.settings;
            if settings.chain_id != SOLANA_CHAIN_ID || identifiers.contains(&settings.chain_id) {
                return Err(RelayerError::Configuration(
                    "solana must use its reserved chain id and no ethereum chain may".to_owned(),
                ));
            }
            validate_range(settings.start_slot, settings.max_slot_range, "solana")?;
            if settings.vault == [0; 20] || settings.program_id == [0; 32] {
                return Err(RelayerError::Configuration(
                    "solana has no vault or custody program".to_owned(),
                ));
            }
            let registration = decode_get_chain(&eth_call(
                paxeer.rpc.as_ref(),
                &LAYERX_BRIDGE_PRECOMPILE,
                &encode_get_chain(settings.chain_id),
            )?)?;
            if !registration.registered
                || registration.vault != settings.vault
                || registration.finality_depth > settings.finality_depth
            {
                return Err(RelayerError::Configuration(
                    "solana is not registered on paxeer with this vault and finality depth"
                        .to_owned(),
                ));
            }
        }
        Ok(Self {
            attestor,
            paxeer,
            chains,
            journal,
            cosign,
            max_submissions,
            solana,
        })
    }

    #[must_use]
    pub const fn journal(&self) -> &Journal {
        &self.journal
    }

    /// One pass of every loop: inbound for each chain, inbound from Solana
    /// when configured, then outbound.
    pub fn tick(&mut self) -> Vec<(String, Result<StepReport, RelayerError>)> {
        let mut results = Vec::with_capacity(self.chains.len() + 2);
        for index in 0..self.chains.len() {
            let stream = inbound_stream(self.chains[index].settings.chain_id);
            results.push((stream, self.inbound_step(index)));
        }
        if self.solana.is_some() {
            results.push((inbound_stream(SOLANA_CHAIN_ID), self.solana_step()));
        }
        results.push((OUTBOUND_STREAM.to_owned(), self.outbound_step()));
        results
    }

    /// Scans chain `index` for final deposits and advances every open
    /// inbound item of that chain.
    ///
    /// # Errors
    ///
    /// Returns the first RPC, decoding, signing or journal failure; the pass
    /// is retried from the journal on the next call.
    pub fn inbound_step(&mut self, index: usize) -> Result<StepReport, RelayerError> {
        let settings = self
            .chains
            .get(index)
            .ok_or_else(|| RelayerError::Configuration(format!("no chain at {index}")))?
            .settings;
        let mut report = StepReport {
            observed: self.scan_inbound(index)?,
            ..StepReport::default()
        };
        let keys = self.open_items(|observation| {
            matches!(observation, Observation::Inbound { chain_id, .. } if *chain_id == settings.chain_id)
        });
        for key in keys {
            let progress = self.advance(&key)?;
            report.count(&progress);
        }
        Ok(report)
    }

    /// Scans the Solana custody program for deposits at the configured depth,
    /// journals every deposit whose logged record and receipt disagree as
    /// refused, and advances every open Solana inbound item through the same
    /// `bridgeIn` path the Ethereum chains use.
    ///
    /// # Errors
    ///
    /// Refuses a relayer without a Solana entry, and returns the first RPC,
    /// decoding, signing or journal failure; the pass is retried from the
    /// journal on the next call.
    pub fn solana_step(&mut self) -> Result<StepReport, RelayerError> {
        let settings = self
            .solana
            .as_ref()
            .ok_or_else(|| RelayerError::Configuration("solana is not configured".to_owned()))?
            .settings;
        let mut report = self.scan_solana()?;
        let keys = self.open_items(|observation| {
            matches!(observation, Observation::Inbound { chain_id, .. } if *chain_id == settings.chain_id)
        });
        for key in keys {
            let progress = self.advance(&key)?;
            report.count(&progress);
        }
        Ok(report)
    }

    fn scan_solana(&mut self) -> Result<StepReport, RelayerError> {
        let mut report = StepReport::default();
        let Some(link) = &self.solana else {
            return Ok(report);
        };
        let settings = link.settings;
        let stream = inbound_stream(settings.chain_id);
        let head = link.rpc.get_slot(settings.commitment)?;
        let Some(safe) = head.checked_sub(settings.finality_depth) else {
            return Ok(report);
        };
        let Some((from, to)) =
            self.scan_range(&stream, settings.start_slot, settings.max_slot_range, safe)
        else {
            return Ok(report);
        };
        let findings = observe_deposits(&link.rpc, &settings, from, to)?;
        for finding in findings {
            match finding {
                Finding::Deposit(observation) => {
                    if self.observe(observation)? {
                        report.observed += 1;
                    }
                }
                Finding::Refused {
                    observation,
                    reason,
                } => {
                    if self.observe(observation)? {
                        report.observed += 1;
                        self.journal.append(&Entry::Refused {
                            item: observation.key(),
                            reason,
                        })?;
                        report.refused += 1;
                    }
                }
            }
        }
        self.journal.append(&Entry::Cursor {
            stream,
            next_block: to + 1,
        })?;
        Ok(report)
    }

    /// Scans Paxeer for final burns and advances every open outbound item.
    ///
    /// # Errors
    ///
    /// Returns the first RPC, decoding, signing or journal failure; the pass
    /// is retried from the journal on the next call.
    pub fn outbound_step(&mut self) -> Result<StepReport, RelayerError> {
        let mut report = StepReport {
            observed: self.scan_outbound()?,
            ..StepReport::default()
        };
        let keys =
            self.open_items(|observation| matches!(observation, Observation::Outbound { .. }));
        for key in keys {
            let progress = self.advance(&key)?;
            report.count(&progress);
        }
        Ok(report)
    }

    fn open_items(&self, filter: impl Fn(&Observation) -> bool) -> Vec<String> {
        self.journal
            .state()
            .items
            .iter()
            .filter(|(_, item)| item.is_open() && filter(&item.observation))
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn scan_range(&self, stream: &str, start: u64, range: u64, safe: u64) -> Option<(u64, u64)> {
        let from = self
            .journal
            .state()
            .cursors
            .get(stream)
            .copied()
            .unwrap_or(start);
        if from > safe {
            return None;
        }
        Some((from, safe.min(from.saturating_add(range - 1))))
    }

    fn observe(&mut self, observation: Observation) -> Result<bool, RelayerError> {
        let key = observation.key();
        let known = self
            .journal
            .state()
            .items
            .get(&key)
            .map(|item| item.observation);
        if known == Some(observation) {
            return Ok(false);
        }
        // A different observation under a known key is refused by the
        // journal as a conflict rather than silently replaced.
        self.journal.append(&Entry::Observed {
            item: key,
            observation,
        })?;
        Ok(true)
    }

    fn scan_inbound(&mut self, index: usize) -> Result<usize, RelayerError> {
        let link = &self.chains[index];
        let settings = link.settings;
        let stream = inbound_stream(settings.chain_id);
        let head = block_number(link.rpc.as_ref())?;
        let Some(safe) = head.checked_sub(settings.finality_depth) else {
            return Ok(0);
        };
        let Some((from, to)) = self.scan_range(
            &stream,
            settings.start_block,
            settings.max_block_range,
            safe,
        ) else {
            return Ok(0);
        };
        let logs = link.rpc.call(
            "eth_getLogs",
            json!([{
                "address": hex::prefixed(&settings.vault),
                "fromBlock": hex::quantity(from),
                "toBlock": hex::quantity(to),
                "topics": [hex::prefixed(&abi::BRIDGE_DEPOSIT_TOPIC)]
            }]),
        )?;
        let logs = logs
            .as_array()
            .ok_or(RelayerError::Rpc(RpcFault::Malformed))?;
        let mut canonical: BTreeMap<u64, [u8; 32]> = BTreeMap::new();
        let mut observations = Vec::with_capacity(logs.len());
        for value in logs {
            let log = decode_deposit_log(value, &settings.vault)?;
            let position = log.position;
            if position.block_number < from || position.block_number > to {
                return Err(RelayerError::Rpc(RpcFault::Malformed));
            }
            let hash = match canonical.get(&position.block_number) {
                Some(hash) => *hash,
                None => {
                    let hash = canonical_hash(link.rpc.as_ref(), position.block_number)?;
                    canonical.insert(position.block_number, hash);
                    hash
                }
            };
            if hash != position.block_hash {
                return Err(RelayerError::Reorganised {
                    block_number: position.block_number,
                });
            }
            observations.push(Observation::inbound(
                &log.attestation(settings.chain_id, settings.vault),
                Position::from(position),
            ));
        }
        let mut observed = 0;
        for observation in observations {
            if self.observe(observation)? {
                observed += 1;
            }
        }
        self.journal.append(&Entry::Cursor {
            stream,
            next_block: to + 1,
        })?;
        Ok(observed)
    }

    fn scan_outbound(&mut self) -> Result<usize, RelayerError> {
        let settings = self.paxeer.settings;
        let rpc = self.paxeer.rpc.as_ref();
        let head = block_number(rpc)?;
        let Some(safe) = head.checked_sub(settings.finality_depth) else {
            return Ok(0);
        };
        let Some((from, to)) = self.scan_range(
            OUTBOUND_STREAM,
            settings.start_block,
            settings.max_block_range,
            safe,
        ) else {
            return Ok(0);
        };
        let vaults: BTreeMap<u64, [u8; 20]> = self
            .chains
            .iter()
            .map(|link| (link.settings.chain_id, link.settings.vault))
            .collect();
        let chain_topics: Vec<String> = vaults
            .keys()
            .map(|chain| hex::prefixed(&crate::attestation::uint256_from_u64(*chain)))
            .collect();
        let logs = rpc.call(
            "eth_getLogs",
            json!([{
                "address": hex::prefixed(&LAYERX_BRIDGE_PRECOMPILE),
                "fromBlock": hex::quantity(from),
                "toBlock": hex::quantity(to),
                "topics": [hex::prefixed(&abi::BRIDGE_OUT_TOPIC), chain_topics]
            }]),
        )?;
        let logs = logs
            .as_array()
            .ok_or(RelayerError::Rpc(RpcFault::Malformed))?;
        let mut canonical: BTreeMap<u64, [u8; 32]> = BTreeMap::new();
        let mut observations = Vec::with_capacity(logs.len());
        for value in logs {
            let log = decode_burn_log(value)?;
            let position = log.position;
            if position.block_number < from || position.block_number > to {
                return Err(RelayerError::Rpc(RpcFault::Malformed));
            }
            let vault = *vaults.get(&log.chain_id).ok_or(AbiError::UnexpectedLog)?;
            let hash = match canonical.get(&position.block_number) {
                Some(hash) => *hash,
                None => {
                    let hash = canonical_hash(rpc, position.block_number)?;
                    canonical.insert(position.block_number, hash);
                    hash
                }
            };
            if hash != position.block_hash {
                return Err(RelayerError::Reorganised {
                    block_number: position.block_number,
                });
            }
            observations.push(Observation::outbound(
                &log.attestation(vault),
                Position::from(position),
            ));
        }
        let mut observed = 0;
        for observation in observations {
            if self.observe(observation)? {
                observed += 1;
            }
        }
        self.journal.append(&Entry::Cursor {
            stream: OUTBOUND_STREAM.to_owned(),
            next_block: to + 1,
        })?;
        Ok(observed)
    }

    fn destination(&self, observation: &Observation) -> Result<Side, RelayerError> {
        match observation {
            Observation::Inbound { .. } => Ok(Side::Paxeer),
            Observation::Outbound { chain_id, .. } => self
                .chains
                .iter()
                .position(|link| link.settings.chain_id == *chain_id)
                .map(Side::Chain)
                .ok_or_else(|| {
                    RelayerError::Configuration(format!("chain {chain_id} is not configured"))
                }),
        }
    }

    fn rpc(&self, side: Side) -> &dyn JsonRpc {
        match side {
            Side::Paxeer => self.paxeer.rpc.as_ref(),
            Side::Chain(index) => self.chains[index].rpc.as_ref(),
        }
    }

    fn submitter(&self, side: Side) -> &Submitter {
        match side {
            Side::Paxeer => &self.paxeer.submitter,
            Side::Chain(index) => &self.chains[index].submitter,
        }
    }

    fn transaction_chain(&self, side: Side) -> (u64, u64, GasPolicy, [u8; 20]) {
        match side {
            Side::Paxeer => (
                self.paxeer.settings.chain_id,
                self.paxeer.settings.finality_depth,
                self.paxeer.settings.gas,
                LAYERX_BRIDGE_PRECOMPILE,
            ),
            Side::Chain(index) => {
                let settings = self.chains[index].settings;
                (
                    settings.chain_id,
                    settings.finality_depth,
                    settings.gas,
                    settings.vault,
                )
            }
        }
    }

    /// Whether the destination has consumed this event's nullifier.
    fn consumed(&self, side: Side, observation: &Observation) -> Result<bool, RelayerError> {
        let rpc = self.rpc(side);
        let (_, _, _, target) = self.transaction_chain(side);
        let data = match observation {
            Observation::Inbound {
                chain_id,
                tx_hash,
                log_index,
                ..
            } => encode_is_nullified(*chain_id, tx_hash, *log_index),
            Observation::Outbound { .. } => {
                let attestation = observation
                    .outbound_attestation()
                    .ok_or(RelayerError::Rpc(RpcFault::Malformed))?;
                encode_nullified(&attestation.nullifier())
            }
        };
        Ok(decode_bool(&eth_call(rpc, &target, &data)?)?)
    }

    /// The destination's current attestor set and threshold.
    fn attestor_policy(&self, side: Side) -> Result<(Vec<[u8; 20]>, usize), RelayerError> {
        let rpc = self.rpc(side);
        let (signers, threshold) = match side {
            Side::Paxeer => {
                let set = decode_get_attestors(&eth_call(
                    rpc,
                    &LAYERX_BRIDGE_PRECOMPILE,
                    &abi::GET_ATTESTORS_SELECTOR,
                )?)?;
                (set.signers, set.threshold)
            }
            Side::Chain(index) => {
                let vault = self.chains[index].settings.vault;
                let threshold =
                    decode_threshold(&eth_call(rpc, &vault, &abi::THRESHOLD_SELECTOR)?)?;
                let signers =
                    decode_address_list(&eth_call(rpc, &vault, &abi::ATTESTORS_SELECTOR)?)?;
                (signers, threshold)
            }
        };
        let threshold = usize::try_from(threshold).map_err(|_| AbiError::OutOfRange)?;
        Ok((signers, threshold))
    }

    fn refusal(observation: &Observation) -> Option<&'static str> {
        match observation {
            Observation::Inbound { amount, .. } | Observation::Outbound { amount, .. }
                if *amount == [0; 32] =>
            {
                Some("zero amount")
            }
            Observation::Inbound { amount, .. } if amount[0] & 0x80 != 0 => {
                Some("amount is not below 2^255")
            }
            Observation::Inbound { .. } => observation
                .inbound_attestation()
                .and_then(|attestation| attestation.paxeer_recipient())
                .is_none()
                .then_some("recipient is not a left-padded non-zero Paxeer address"),
            Observation::Outbound { recipient, .. } => {
                (*recipient == [0; 20]).then_some("zero recipient")
            }
        }
    }

    fn advance(&mut self, key: &str) -> Result<Progress, RelayerError> {
        let Some(item) = self.journal.state().items.get(key).cloned() else {
            return Ok(Progress::Waiting);
        };
        if !item.is_open() {
            return Ok(Progress::Completed);
        }
        let observation = item.observation;
        let side = self.destination(&observation)?;
        if let Some(pending) = item.pending() {
            return self.resolve(key, side, &observation, pending);
        }
        if let Some(reason) = Self::refusal(&observation) {
            self.journal.append(&Entry::Refused {
                item: key.to_owned(),
                reason: reason.to_owned(),
            })?;
            return Ok(Progress::Refused);
        }
        if self.consumed(side, &observation)? {
            self.journal.append(&Entry::Completed {
                item: key.to_owned(),
                completion: Completion::AlreadyBridged,
            })?;
            return Ok(Progress::Completed);
        }
        if item.submissions.len() >= usize::try_from(self.max_submissions).unwrap_or(usize::MAX) {
            self.journal.append(&Entry::Refused {
                item: key.to_owned(),
                reason: format!(
                    "{} transactions failed to bridge the event",
                    item.submissions.len()
                ),
            })?;
            return Ok(Progress::Refused);
        }
        let (attestors, threshold) = self.attestor_policy(side)?;
        if !attestors.contains(&self.attestor.address()) {
            return Err(RelayerError::NotAttestor);
        }
        let (digest, signature) = match (
            observation.inbound_attestation(),
            observation.outbound_attestation(),
        ) {
            (Some(attestation), _) => (
                attestation.digest(),
                match item.signature {
                    Some(signature) => signature,
                    None => self.attestor.sign_inbound(&attestation)?,
                },
            ),
            (None, Some(attestation)) => (
                attestation.digest(),
                match item.signature {
                    Some(signature) => signature,
                    None => self.attestor.sign_outbound(&attestation)?,
                },
            ),
            (None, None) => return Err(RelayerError::Rpc(RpcFault::Malformed)),
        };
        if item.signature.is_none() {
            self.journal.append(&Entry::Signed {
                item: key.to_owned(),
                signature,
            })?;
        }
        let mut candidates = vec![signature];
        if let Some(cosign) = &self.cosign {
            cosign.publish(&digest, &self.attestor.address(), &signature)?;
            candidates.extend(cosign.collect(&digest));
        }
        let signatures = match assemble_signatures(&digest, candidates, &attestors, threshold) {
            Ok(signatures) => signatures,
            Err(SignatureError::BelowThreshold { .. }) => return Ok(Progress::Waiting),
            Err(error) => return Err(RelayerError::Key(KeyError::Signature(error))),
        };
        let calldata = match (
            observation.inbound_attestation(),
            observation.outbound_attestation(),
        ) {
            (Some(attestation), _) => encode_bridge_in(&attestation, &signatures),
            (None, Some(attestation)) => encode_release(&attestation, &signatures),
            (None, None) => return Err(RelayerError::Rpc(RpcFault::Malformed)),
        };
        let Some((signed, nonce)) = self.prepare_transaction(side, calldata)? else {
            return Ok(Progress::Waiting);
        };
        self.journal.append(&Entry::Submitted {
            item: key.to_owned(),
            submitter: self.submitter(side).address(),
            nonce,
            tx_hash: signed.hash,
            raw: signed.raw.clone(),
        })?;
        // The journaled bytes are authoritative from here on: whatever this
        // broadcast returns, the next pass resolves the same transaction.
        let _ = self
            .rpc(side)
            .send_raw_transaction(&signed.raw, &signed.hash);
        Ok(Progress::Submitted)
    }

    /// The next nonce for the side's submitter: the node's pending count, but
    /// never below a nonce this relayer has journaled and not yet resolved.
    fn next_nonce(&self, side: Side) -> Result<u64, RelayerError> {
        let address = self.submitter(side).address();
        let pending = quantity_of(&self.rpc(side).call(
            "eth_getTransactionCount",
            json!([hex::prefixed(&address), "pending"]),
        )?)?;
        let journaled = self
            .journal
            .state()
            .items
            .values()
            .filter(|item| self.destination(&item.observation).ok() == Some(side))
            .filter_map(|item| item.pending())
            .filter(|submission| submission.submitter == address)
            .map(|submission| submission.nonce.saturating_add(1))
            .max()
            .unwrap_or(0);
        Ok(pending.max(journaled))
    }

    /// Estimates, prices and signs the call. `None` means the call would not
    /// execute now (it reverts in simulation) or fees are above policy; the
    /// item waits for the next pass without spending anything.
    fn prepare_transaction(
        &self,
        side: Side,
        data: Vec<u8>,
    ) -> Result<Option<(SignedTransaction, u64)>, RelayerError> {
        let rpc = self.rpc(side);
        let submitter = self.submitter(side);
        let (chain_id, _, gas, to) = self.transaction_chain(side);
        let estimate = rpc.call(
            "eth_estimateGas",
            json!([{
                "from": hex::prefixed(&submitter.address()),
                "to": hex::prefixed(&to),
                "data": hex::prefixed(&data),
                "value": "0x0"
            }]),
        );
        let estimated = match estimate {
            Ok(value) => quantity_of(&value)?,
            Err(RpcFault::Configuration) => return Err(RelayerError::Rpc(RpcFault::Configuration)),
            Err(_) => return Ok(None),
        };
        if estimated > gas.gas_limit {
            return Err(RelayerError::GasLimit {
                estimated,
                limit: gas.gas_limit,
            });
        }
        let nonce = self.next_nonce(side)?;
        let priority = quantity_u128_of(&rpc.call("eth_maxPriorityFeePerGas", json!([]))?)?
            .min(gas.max_priority_fee_per_gas);
        let base = quantity_u128_of(&rpc.call("eth_gasPrice", json!([]))?)?;
        let floor = base.saturating_add(priority);
        if floor > gas.max_fee_per_gas {
            return Ok(None);
        }
        let max_fee = base
            .saturating_mul(2)
            .saturating_add(priority)
            .min(gas.max_fee_per_gas);
        let call = Eip1559Call {
            chain_id,
            nonce,
            max_priority_fee_per_gas: priority,
            max_fee_per_gas: max_fee,
            gas_limit: gas.gas_limit,
            to,
            data,
        };
        Ok(Some((tx::sign(&call, submitter)?, nonce)))
    }

    /// Classifies a journaled transaction without ever signing a replacement
    /// for it: final success completes the item; a revert or a refusal
    /// completes it when the nullifier is consumed and otherwise frees the
    /// item for a new transaction; an unknown transaction is rebroadcast
    /// byte for byte.
    fn resolve(
        &mut self,
        key: &str,
        side: Side,
        observation: &Observation,
        pending: &Submission,
    ) -> Result<Progress, RelayerError> {
        let rpc = self.rpc(side);
        let (_, depth, _, _) = self.transaction_chain(side);
        let hash = hex::prefixed(&pending.tx_hash);
        let receipt = rpc.call("eth_getTransactionReceipt", json!([hash]))?;
        if receipt.is_null() {
            let known = rpc.call("eth_getTransactionByHash", json!([hash]))?;
            if !known.is_null() {
                return Ok(Progress::Waiting);
            }
            return match rpc.send_raw_transaction(&pending.raw, &pending.tx_hash) {
                Ok(_) => Ok(Progress::Waiting),
                Err(RpcFault::Rejected { .. }) => {
                    if self.consumed(side, observation)? {
                        self.journal.append(&Entry::Completed {
                            item: key.to_owned(),
                            completion: Completion::AlreadyBridged,
                        })?;
                        return Ok(Progress::Completed);
                    }
                    self.journal.append(&Entry::Dropped {
                        item: key.to_owned(),
                        tx_hash: pending.tx_hash,
                    })?;
                    Ok(Progress::Waiting)
                }
                Err(error) => Err(RelayerError::Rpc(error)),
            };
        }
        if text(&receipt, "transactionHash")? != hash {
            return Err(RelayerError::Rpc(RpcFault::Malformed));
        }
        let included = quantity_of(receipt.get("blockNumber").unwrap_or(&Value::Null))?;
        let status = quantity_of(receipt.get("status").unwrap_or(&Value::Null))?;
        let head = block_number(rpc)?;
        if head < included || head - included < depth {
            return Ok(Progress::Waiting);
        }
        match status {
            1 => {
                self.journal.append(&Entry::Completed {
                    item: key.to_owned(),
                    completion: Completion::Included {
                        tx_hash: pending.tx_hash,
                        block_number: included,
                    },
                })?;
                Ok(Progress::Completed)
            }
            0 => {
                if self.consumed(side, observation)? {
                    self.journal.append(&Entry::Completed {
                        item: key.to_owned(),
                        completion: Completion::AlreadyBridged,
                    })?;
                    return Ok(Progress::Completed);
                }
                self.journal.append(&Entry::Reverted {
                    item: key.to_owned(),
                    tx_hash: pending.tx_hash,
                })?;
                Ok(Progress::Waiting)
            }
            _ => Err(RelayerError::Rpc(RpcFault::Malformed)),
        }
    }
}
