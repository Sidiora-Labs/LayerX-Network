use layerx_types::intent::EvmAddress;

use crate::json::Json;
use crate::rpc::{
    canonical_endpoint_identity, raw_call, EndpointConfig, EndpointFailure, EndpointFault,
};

/// Exact 32-byte Paxeer transaction hash.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TransactionHash([u8; 32]);

/// Why a textual transaction hash could not be decoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionHashError {
    MissingPrefix,
    WrongLength,
    NotHex,
}

impl TransactionHash {
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    /// Decodes a 0x-prefixed 64-digit hex hash.
    ///
    /// # Errors
    ///
    /// Returns the first prefix, length or digit defect.
    pub fn from_hex(text: &str) -> Result<Self, TransactionHashError> {
        let digits = text
            .strip_prefix("0x")
            .ok_or(TransactionHashError::MissingPrefix)?;
        if digits.len() != 64 {
            return Err(TransactionHashError::WrongLength);
        }
        let mut bytes = [0_u8; 32];
        for (slot, pair) in bytes.iter_mut().zip(digits.as_bytes().chunks_exact(2)) {
            match pair {
                &[high, low] => {
                    let upper = hex_nibble(high).ok_or(TransactionHashError::NotHex)?;
                    let lower = hex_nibble(low).ok_or(TransactionHashError::NotHex)?;
                    *slot = (upper << 4) | lower;
                }
                _ => return Err(TransactionHashError::WrongLength),
            }
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        let mut text = String::from("0x");
        for byte in self.0 {
            text.push(hex_char(byte >> 4));
            text.push(hex_char(byte & 0x0f));
        }
        text
    }
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

fn hex_char(nibble: u8) -> char {
    char::from(HEX_DIGITS[usize::from(nibble & 0x0f)])
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// One canonical block position: height and hash together.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockRef {
    pub number: u64,
    pub hash: [u8; 32],
}

/// Whether the included transaction's execution succeeded or reverted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionOutcome {
    Succeeded,
    Reverted,
}

/// A transaction's inclusion as the chain currently reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionInclusion {
    pub block: BlockRef,
    pub transaction_index: u64,
    pub execution: ExecutionOutcome,
    pub deployed_contract: Option<EvmAddress>,
}

/// The complete set of states a tracked custody transaction can read back as.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionView {
    Unknown,
    Pending,
    Included(TransactionInclusion),
}

/// One contract log an included transaction emitted, exactly as reported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogRecord {
    pub address: EvmAddress,
    pub topics: Vec<[u8; 32]>,
    pub data: Vec<u8>,
}

/// Every configured endpoint failed the same read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointError {
    pub failures: Vec<EndpointFailure>,
}

/// Why the declared endpoint configuration was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientConfigError {
    NoEndpoints,
    InvalidEndpoint {
        url: String,
        fault: EndpointFault,
    },
    ZeroRequestTimeout { url: String },
    InconsistentChainBinding {
        url: String,
        expected: u64,
        actual: u64,
    },
    MixedTransportModes,
    DuplicateEndpointIdentity {
        first_url: String,
        duplicate_url: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QuorumBinding {
    chain_id: u64,
    endpoint_sources: Vec<(String, crate::rpc::EndpointTransport)>,
    minimum_agreement: usize,
}

/// Read-only Paxeer client over one or more declared endpoints.
#[derive(Clone, Debug)]
pub struct PaxeerClient {
    endpoints: Vec<EndpointConfig>,
    expected_chain_id: u64,
    endpoint_sources: Vec<(String, crate::rpc::EndpointTransport)>,
}

impl PaxeerClient {
    /// Validates and adopts the declared endpoint configuration.
    ///
    /// # Errors
    ///
    /// Refuses an empty endpoint list, an unparseable URL, or a zero timeout.
    pub fn new(endpoints: Vec<EndpointConfig>) -> Result<Self, ClientConfigError> {
        if endpoints.is_empty() {
            return Err(ClientConfigError::NoEndpoints);
        }
        let expected_chain_id = endpoints[0].expected_chain_id;
        let production = matches!(
            &endpoints[0].transport,
            crate::rpc::EndpointTransport::PinnedTls { .. }
        );
        let mut identities = std::collections::BTreeMap::new();
        for endpoint in &endpoints {
            let identity = canonical_endpoint_identity(endpoint).map_err(|fault| {
                ClientConfigError::InvalidEndpoint {
                    url: endpoint.url.clone(),
                    fault,
                }
            })?;
            if endpoint.request_timeout.is_zero() {
                return Err(ClientConfigError::ZeroRequestTimeout {
                    url: endpoint.url.clone(),
                });
            }
            if endpoint.expected_chain_id != expected_chain_id {
                return Err(ClientConfigError::InconsistentChainBinding {
                    url: endpoint.url.clone(),
                    expected: expected_chain_id,
                    actual: endpoint.expected_chain_id,
                });
            }
            if matches!(
                &endpoint.transport,
                crate::rpc::EndpointTransport::PinnedTls { .. }
            ) != production
            {
                return Err(ClientConfigError::MixedTransportModes);
            }
            if let Some((first_url, _)) = identities.insert(
                identity,
                (endpoint.url.clone(), endpoint.transport.clone()),
            ) {
                return Err(ClientConfigError::DuplicateEndpointIdentity {
                    first_url,
                    duplicate_url: endpoint.url.clone(),
                });
            }
        }
        let endpoint_sources = identities
            .into_iter()
            .map(|(identity, (_, transport))| (identity, transport))
            .collect();
        Ok(Self {
            endpoints,
            expected_chain_id,
            endpoint_sources,
        })
    }

    pub(crate) fn quorum_binding(&self, minimum_agreement: usize) -> QuorumBinding {
        QuorumBinding {
            chain_id: self.expected_chain_id,
            endpoint_sources: self.endpoint_sources.clone(),
            minimum_agreement,
        }
    }

    fn verify_chain(&self, endpoint: &EndpointConfig) -> Result<(), EndpointFailure> {
        let value = raw_call(endpoint, "eth_chainId", &[])?;
        let actual = quantity(&value).map_err(|detail| EndpointFailure {
            url: endpoint.url.clone(),
            fault: EndpointFault::UnexpectedValue { detail },
        })?;
        if actual != self.expected_chain_id {
            return Err(EndpointFailure {
                url: endpoint.url.clone(),
                fault: EndpointFault::ChainMismatch {
                    expected: self.expected_chain_id,
                    actual,
                },
            });
        }
        Ok(())
    }

    fn bound_call(
        &self,
        endpoint: &EndpointConfig,
        method: &str,
        params: &[Json],
    ) -> Result<Json, EndpointFailure> {
        self.verify_chain(endpoint)?;
        raw_call(endpoint, method, params)
    }

    fn read<T>(
        &self,
        failovers: &mut Vec<EndpointFailure>,
        method: &str,
        params: &[Json],
        decode: impl Fn(&Json) -> Result<T, String>,
    ) -> Result<T, EndpointError> {
        let mut failures = Vec::new();
        for endpoint in &self.endpoints {
            match self.bound_call(endpoint, method, params) {
                Ok(value) => match decode(&value) {
                    Ok(decoded) => {
                        failovers.append(&mut failures);
                        return Ok(decoded);
                    }
                    Err(detail) => failures.push(EndpointFailure {
                        url: endpoint.url.clone(),
                        fault: EndpointFault::UnexpectedValue { detail },
                    }),
                },
                Err(failure) => failures.push(failure),
            }
        }
        Err(EndpointError { failures })
    }

    pub(crate) fn agreed_call(
        &self,
        method: &str,
        params: &[Json],
        minimum_agreement: usize,
    ) -> Result<Json, EndpointError> {
        let mut failures = Vec::new();
        let mut observations = Vec::new();
        for endpoint in &self.endpoints {
            match self.bound_call(endpoint, method, params) {
                Ok(value) => observations.push((endpoint.url.clone(), value)),
                Err(failure) => failures.push(failure),
            }
        }
        if let Some(index) = semantic_quorum_index(&observations, minimum_agreement) {
            let candidate = observations[index].1.clone();
            return Ok(candidate);
        }
        failures.extend(observations.into_iter().map(|(url, _)| EndpointFailure {
            url,
            fault: EndpointFault::InconsistentObservation,
        }));
        Err(EndpointError { failures })
    }

    pub(crate) fn agreed_contract_call(
        &self,
        contract: EvmAddress,
        data: &[u8],
        minimum_agreement: usize,
    ) -> Result<Vec<u8>, EndpointError> {
        let value = self.agreed_call(
            "eth_call",
            &[
                Json::Object(vec![
                    ("to".to_owned(), Json::Text(bytes_hex(&contract.bytes()))),
                    ("data".to_owned(), Json::Text(bytes_hex(data))),
                ]),
                Json::Text("latest".to_owned()),
            ],
            minimum_agreement,
        )?;
        variable_bytes(&value).map_err(|detail| EndpointError {
            failures: self
                .endpoints
                .iter()
                .map(|endpoint| EndpointFailure {
                    url: endpoint.url.clone(),
                    fault: EndpointFault::UnexpectedValue {
                        detail: detail.clone(),
                    },
                })
                .collect(),
        })
    }

    /// Reads the current canonical head number.
    ///
    /// # Errors
    ///
    /// Returns every endpoint's typed failure when no endpoint served the read.
    pub fn head_number(&self) -> Result<u64, EndpointError> {
        self.head_number_with_failovers(&mut Vec::new())
    }

    pub(crate) fn head_number_with_failovers(
        &self,
        failovers: &mut Vec<EndpointFailure>,
    ) -> Result<u64, EndpointError> {
        self.read(failovers, "eth_blockNumber", &[], quantity)
    }

    /// Reads the canonical block at one height, if that height exists.
    ///
    /// # Errors
    ///
    /// Returns every endpoint's typed failure when no endpoint served the read.
    pub fn block_by_number(&self, number: u64) -> Result<Option<BlockRef>, EndpointError> {
        self.block_by_number_with_failovers(&mut Vec::new(), number)
    }

    pub(crate) fn block_by_number_with_failovers(
        &self,
        failovers: &mut Vec<EndpointFailure>,
        number: u64,
    ) -> Result<Option<BlockRef>, EndpointError> {
        self.read(
            failovers,
            "eth_getBlockByNumber",
            &[Json::Text(format!("0x{number:x}")), Json::Bool(false)],
            block_reference,
        )
    }

    /// Reads a custody transaction's inclusion receipt, if it is included.
    ///
    /// # Errors
    ///
    /// Returns every endpoint's typed failure when no endpoint served the read.
    pub fn transaction_receipt(
        &self,
        transaction: TransactionHash,
    ) -> Result<Option<TransactionInclusion>, EndpointError> {
        self.transaction_receipt_with_failovers(&mut Vec::new(), transaction)
    }

    /// Reads the chain identifier the endpoints agree to serve.
    ///
    /// # Errors
    ///
    /// Returns every endpoint's typed failure when no endpoint served the read.
    pub fn chain_id(&self) -> Result<u64, EndpointError> {
        self.chain_id_with_failovers(&mut Vec::new())
    }

    pub(crate) fn chain_id_with_failovers(
        &self,
        failovers: &mut Vec<EndpointFailure>,
    ) -> Result<u64, EndpointError> {
        self.read(failovers, "eth_chainId", &[], quantity)
    }

    /// Reads a custody transaction's inclusion together with every log it
    /// emitted, from one atomic receipt read.
    ///
    /// # Errors
    ///
    /// Returns every endpoint's typed failure when no endpoint served the read.
    pub fn transaction_logs(
        &self,
        transaction: TransactionHash,
    ) -> Result<Option<(TransactionInclusion, Vec<LogRecord>)>, EndpointError> {
        self.transaction_logs_with_failovers(&mut Vec::new(), transaction)
    }

    pub(crate) fn transaction_logs_with_failovers(
        &self,
        failovers: &mut Vec<EndpointFailure>,
        transaction: TransactionHash,
    ) -> Result<Option<(TransactionInclusion, Vec<LogRecord>)>, EndpointError> {
        self.read(
            failovers,
            "eth_getTransactionReceipt",
            &[Json::Text(transaction.to_hex())],
            |value| inclusion_with_logs(value, transaction),
        )
    }

    pub(crate) fn transaction_receipt_with_failovers(
        &self,
        failovers: &mut Vec<EndpointFailure>,
        transaction: TransactionHash,
    ) -> Result<Option<TransactionInclusion>, EndpointError> {
        self.read(
            failovers,
            "eth_getTransactionReceipt",
            &[Json::Text(transaction.to_hex())],
            inclusion,
        )
    }

    /// Reads the full unknown-pending-included view of a custody transaction.
    ///
    /// # Errors
    ///
    /// Returns every endpoint's typed failure when no endpoint served the read.
    pub fn transaction(
        &self,
        transaction: TransactionHash,
    ) -> Result<TransactionView, EndpointError> {
        self.transaction_with_failovers(&mut Vec::new(), transaction)
    }

    pub(crate) fn transaction_with_failovers(
        &self,
        failovers: &mut Vec<EndpointFailure>,
        transaction: TransactionHash,
    ) -> Result<TransactionView, EndpointError> {
        if let Some(included) = self.transaction_receipt_with_failovers(failovers, transaction)? {
            return Ok(TransactionView::Included(included));
        }
        let known = self.read(
            failovers,
            "eth_getTransactionByHash",
            &[Json::Text(transaction.to_hex())],
            |value| Ok(!value.is_null()),
        )?;
        Ok(if known {
            TransactionView::Pending
        } else {
            TransactionView::Unknown
        })
    }

    pub(crate) fn agreed_finality_observation(
        &self,
        transaction: TransactionHash,
        minimum_agreement: usize,
    ) -> Result<(FinalityObservation, Vec<EndpointFailure>), EndpointError> {
        let mut failures = Vec::new();
        let mut observations = Vec::new();
        for endpoint in &self.endpoints {
            match self.endpoint_finality_observation(endpoint, transaction) {
                Ok(observation) => observations.push((endpoint.url.clone(), observation)),
                Err(failure) => failures.push(failure),
            }
        }
        for (_, candidate) in &observations {
            let agreeing = observations
                .iter()
                .filter(|(_, observed)| observed.same_chain_fact(candidate))
                .collect::<Vec<_>>();
            if agreeing.len() >= minimum_agreement {
                let head = agreeing
                    .iter()
                    .map(|(_, observed)| observed.head)
                    .min()
                    .unwrap_or(candidate.head);
                failures.extend(observations.iter().filter_map(|(url, observed)| {
                    (!observed.same_chain_fact(candidate)).then_some(EndpointFailure {
                        url: url.clone(),
                        fault: EndpointFault::InconsistentObservation,
                    })
                }));
                return Ok((
                    FinalityObservation {
                        head,
                        transaction: candidate.transaction,
                        canonical_block: candidate.canonical_block,
                        receipt_logs: candidate.receipt_logs.clone(),
                        chain_id: candidate.chain_id,
                    },
                    failures,
                ));
            }
        }
        failures.extend(observations.into_iter().map(|(url, _)| EndpointFailure {
            url,
            fault: EndpointFault::InconsistentObservation,
        }));
        Err(EndpointError { failures })
    }

    fn endpoint_finality_observation(
        &self,
        endpoint: &EndpointConfig,
        transaction: TransactionHash,
    ) -> Result<FinalityObservation, EndpointFailure> {
        self.verify_chain(endpoint)?;
        let head = raw_call(endpoint, "eth_blockNumber", &[]).and_then(|value| {
            quantity(&value).map_err(|detail| EndpointFailure {
                url: endpoint.url.clone(),
                fault: EndpointFault::UnexpectedValue { detail },
            })
        })?;
        let anchor_parameters = [Json::Text(format!("0x{head:x}")), Json::Bool(false)];
        let anchor_before = raw_call(endpoint, "eth_getBlockByNumber", &anchor_parameters)
            .and_then(|value| {
                block_reference(&value).map_err(|detail| EndpointFailure {
                    url: endpoint.url.clone(),
                    fault: EndpointFault::UnexpectedValue { detail },
                })
            })?
            .filter(|anchor| anchor.number == head)
            .ok_or_else(|| EndpointFailure {
                url: endpoint.url.clone(),
                fault: EndpointFault::InconsistentObservation,
            })?;
        let receipt = raw_call(
            endpoint,
            "eth_getTransactionReceipt",
            &[Json::Text(transaction.to_hex())],
        )?;
        let included = inclusion_with_logs(&receipt, transaction).map_err(|detail| {
            EndpointFailure {
                url: endpoint.url.clone(),
                fault: EndpointFault::UnexpectedValue { detail },
            }
        })?;
        let (transaction_view, canonical_block, receipt_logs) =
            if let Some((included, logs)) = included {
                if included.block.number > head {
                    return Err(EndpointFailure {
                        url: endpoint.url.clone(),
                        fault: EndpointFault::InconsistentObservation,
                    });
                }
                let block = raw_call(
                    endpoint,
                    "eth_getBlockByNumber",
                    &[
                        Json::Text(format!("0x{:x}", included.block.number)),
                        Json::Bool(false),
                    ],
                )?;
                let canonical = block_reference(&block).map_err(|detail| EndpointFailure {
                    url: endpoint.url.clone(),
                    fault: EndpointFault::UnexpectedValue { detail },
                })?;
                if canonical.is_some_and(|block| block.number != included.block.number) {
                    return Err(EndpointFailure {
                        url: endpoint.url.clone(),
                        fault: EndpointFault::InconsistentObservation,
                    });
                }
                (TransactionView::Included(included), canonical, Some(logs))
            } else {
                let value = raw_call(
                    endpoint,
                    "eth_getTransactionByHash",
                    &[Json::Text(transaction.to_hex())],
                )?;
                if !value.is_null() {
                    let hash = required(&value, "hash").map_err(|detail| EndpointFailure {
                        url: endpoint.url.clone(),
                        fault: EndpointFault::UnexpectedValue { detail },
                    })?;
                    let observed_hash = fixed::<32>(hash).map_err(|detail| EndpointFailure {
                        url: endpoint.url.clone(),
                        fault: EndpointFault::UnexpectedValue { detail },
                    })?;
                    if observed_hash != transaction.bytes() {
                        return Err(EndpointFailure {
                            url: endpoint.url.clone(),
                            fault: EndpointFault::InconsistentObservation,
                        });
                    }
                }
                let view = if value.is_null() {
                    TransactionView::Unknown
                } else {
                    TransactionView::Pending
                };
                (view, None, None)
            };
        let anchor_after = raw_call(endpoint, "eth_getBlockByNumber", &anchor_parameters)
            .and_then(|value| {
                block_reference(&value).map_err(|detail| EndpointFailure {
                    url: endpoint.url.clone(),
                    fault: EndpointFault::UnexpectedValue { detail },
                })
            })?;
        if anchor_after != Some(anchor_before) {
            return Err(EndpointFailure {
                url: endpoint.url.clone(),
                fault: EndpointFault::InconsistentObservation,
            });
        }
        Ok(FinalityObservation {
            head,
            transaction: transaction_view,
            canonical_block,
            receipt_logs,
            chain_id: endpoint.expected_chain_id,
        })
    }

    /// Executes a read-only contract call against the latest state.
    ///
    /// # Errors
    ///
    /// Returns every endpoint's typed failure when no endpoint served the call.
    pub fn call_contract(
        &self,
        contract: EvmAddress,
        data: &[u8],
    ) -> Result<Vec<u8>, EndpointError> {
        self.call_contract_with_failovers(&mut Vec::new(), contract, data)
    }

    pub(crate) fn call_contract_with_failovers(
        &self,
        failovers: &mut Vec<EndpointFailure>,
        contract: EvmAddress,
        data: &[u8],
    ) -> Result<Vec<u8>, EndpointError> {
        self.read(
            failovers,
            "eth_call",
            &[
                Json::Object(vec![
                    ("to".to_owned(), Json::Text(bytes_hex(&contract.bytes()))),
                    ("data".to_owned(), Json::Text(bytes_hex(data))),
                ]),
                Json::Text("latest".to_owned()),
            ],
            variable_bytes,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FinalityObservation {
    pub head: u64,
    pub transaction: TransactionView,
    pub canonical_block: Option<BlockRef>,
    pub receipt_logs: Option<Vec<LogRecord>>,
    pub chain_id: u64,
}

impl FinalityObservation {
    fn same_chain_fact(&self, other: &Self) -> bool {
        self.transaction == other.transaction
            && self.canonical_block == other.canonical_block
            && self.receipt_logs == other.receipt_logs
            && self.chain_id == other.chain_id
    }
}

fn semantic_quorum_index(
    observations: &[(String, Json)],
    minimum_agreement: usize,
) -> Option<usize> {
    observations.iter().enumerate().find_map(|(index, (_, candidate))| {
        let agreeing = observations
            .iter()
            .filter(|(_, observed)| semantic_json_eq(observed, candidate))
            .count();
        (agreeing >= minimum_agreement).then_some(index)
    })
}

fn semantic_json_eq(left: &Json, right: &Json) -> bool {
    match (left, right) {
        (Json::Null, Json::Null) => true,
        (Json::Bool(left), Json::Bool(right)) => left == right,
        (Json::Number(left), Json::Number(right)) | (Json::Text(left), Json::Text(right)) => {
            left == right
        }
        (Json::Array(left), Json::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| semantic_json_eq(left, right))
        }
        (Json::Object(left), Json::Object(right)) => {
            unique_members(left)
                && unique_members(right)
                && left.len() == right.len()
                && left.iter().all(|(name, left_value)| {
                    right
                        .iter()
                        .find(|(right_name, _)| right_name == name)
                        .is_some_and(|(_, right_value)| semantic_json_eq(left_value, right_value))
                })
        }
        _ => false,
    }
}

fn unique_members(members: &[(String, Json)]) -> bool {
    members.iter().enumerate().all(|(index, (name, _))| {
        members
            .iter()
            .skip(index.saturating_add(1))
            .all(|(other, _)| other != name)
    })
}

fn required<'a>(value: &'a Json, name: &str) -> Result<&'a Json, String> {
    value
        .member(name)
        .ok_or_else(|| format!("missing member {name}"))
}

fn quantity(value: &Json) -> Result<u64, String> {
    let text = value
        .as_text()
        .ok_or_else(|| format!("expected hex quantity, got {value:?}"))?;
    let digits = text
        .strip_prefix("0x")
        .ok_or_else(|| format!("missing 0x prefix in {text}"))?;
    if digits.is_empty() {
        return Err(format!("empty hex quantity in {text}"));
    }
    u64::from_str_radix(digits, 16).map_err(|_| format!("unparseable hex quantity {text}"))
}

fn fixed<const N: usize>(value: &Json) -> Result<[u8; N], String> {
    let text = value
        .as_text()
        .ok_or_else(|| format!("expected hex bytes, got {value:?}"))?;
    let digits = text
        .strip_prefix("0x")
        .ok_or_else(|| format!("missing 0x prefix in {text}"))?;
    if digits.len() != N.saturating_mul(2) {
        return Err(format!("expected {N} bytes, got {text}"));
    }
    let mut bytes = [0_u8; N];
    for (slot, pair) in bytes.iter_mut().zip(digits.as_bytes().chunks_exact(2)) {
        match pair {
            &[high, low] => {
                let upper = hex_nibble(high).ok_or_else(|| format!("non-hex digit in {text}"))?;
                let lower = hex_nibble(low).ok_or_else(|| format!("non-hex digit in {text}"))?;
                *slot = (upper << 4) | lower;
            }
            _ => return Err(format!("expected {N} bytes, got {text}")),
        }
    }
    Ok(bytes)
}

fn bytes_hex(bytes: &[u8]) -> String {
    let mut text = String::from("0x");
    for byte in bytes {
        text.push(hex_char(byte >> 4));
        text.push(hex_char(byte & 0x0f));
    }
    text
}

fn block_reference(value: &Json) -> Result<Option<BlockRef>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let number = quantity(required(value, "number")?)?;
    let hash = fixed::<32>(required(value, "hash")?)?;
    Ok(Some(BlockRef { number, hash }))
}

fn variable_bytes(value: &Json) -> Result<Vec<u8>, String> {
    let text = value
        .as_text()
        .ok_or_else(|| format!("expected hex bytes, got {value:?}"))?;
    let digits = text
        .strip_prefix("0x")
        .ok_or_else(|| format!("missing 0x prefix in {text}"))?;
    if digits.len() % 2 != 0 {
        return Err(format!("odd hex length in {text}"));
    }
    let mut bytes = Vec::with_capacity(digits.len() / 2);
    for pair in digits.as_bytes().chunks_exact(2) {
        match pair {
            &[high, low] => {
                let upper = hex_nibble(high).ok_or_else(|| format!("non-hex digit in {text}"))?;
                let lower = hex_nibble(low).ok_or_else(|| format!("non-hex digit in {text}"))?;
                bytes.push((upper << 4) | lower);
            }
            _ => return Err(format!("odd hex length in {text}")),
        }
    }
    Ok(bytes)
}

fn log_record(
    value: &Json,
    transaction: TransactionHash,
    inclusion: TransactionInclusion,
) -> Result<LogRecord, String> {
    if fixed::<32>(required(value, "transactionHash")?)? != transaction.bytes()
        || fixed::<32>(required(value, "blockHash")?)? != inclusion.block.hash
        || quantity(required(value, "blockNumber")?)? != inclusion.block.number
        || quantity(required(value, "transactionIndex")?)? != inclusion.transaction_index
        || !matches!(required(value, "removed")?, Json::Bool(false))
    {
        return Err("receipt log does not bind to its transaction inclusion".to_owned());
    }
    let address = EvmAddress::new(fixed::<20>(required(value, "address")?)?);
    let topics = match required(value, "topics")? {
        Json::Array(items) => items
            .iter()
            .map(fixed::<32>)
            .collect::<Result<Vec<_>, _>>()?,
        other => return Err(format!("expected topics array, got {other:?}")),
    };
    let data = variable_bytes(required(value, "data")?)?;
    Ok(LogRecord {
        address,
        topics,
        data,
    })
}

fn inclusion_with_logs(
    value: &Json,
    transaction: TransactionHash,
) -> Result<Option<(TransactionInclusion, Vec<LogRecord>)>, String> {
    let Some(included) = inclusion(value)? else {
        return Ok(None);
    };
    if fixed::<32>(required(value, "transactionHash")?)? != transaction.bytes() {
        return Err("receipt transaction hash does not match its request".to_owned());
    }
    let logs = match required(value, "logs")? {
        Json::Array(items) => items
            .iter()
            .map(|value| log_record(value, transaction, included))
            .collect::<Result<Vec<_>, _>>()?,
        other => return Err(format!("expected logs array, got {other:?}")),
    };
    Ok(Some((included, logs)))
}

fn inclusion(value: &Json) -> Result<Option<TransactionInclusion>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let number = quantity(required(value, "blockNumber")?)?;
    let hash = fixed::<32>(required(value, "blockHash")?)?;
    let transaction_index = quantity(required(value, "transactionIndex")?)?;
    let execution = match quantity(required(value, "status")?)? {
        0 => ExecutionOutcome::Reverted,
        1 => ExecutionOutcome::Succeeded,
        other => return Err(format!("unknown execution status {other}")),
    };
    let deployed_contract = match value.member("contractAddress") {
        None | Some(Json::Null) => None,
        Some(address) => Some(EvmAddress::new(fixed::<20>(address)?)),
    };
    Ok(Some(TransactionInclusion {
        block: BlockRef { number, hash },
        transaction_index,
        execution,
        deployed_contract,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn included() -> TransactionInclusion {
        TransactionInclusion {
            block: BlockRef {
                number: 9,
                hash: [4; 32],
            },
            transaction_index: 1,
            execution: ExecutionOutcome::Succeeded,
            deployed_contract: None,
        }
    }

    #[test]
    fn quorum_fact_rejects_split_receipt_logs() {
        let base = FinalityObservation {
            head: 12,
            transaction: TransactionView::Included(included()),
            canonical_block: Some(included().block),
            receipt_logs: Some(vec![LogRecord {
                address: EvmAddress::new([7; 20]),
                topics: vec![[8; 32]],
                data: vec![1],
            }]),
            chain_id: 31_337,
        };
        let mut split = base.clone();
        split.receipt_logs = Some(vec![LogRecord {
            address: EvmAddress::new([7; 20]),
            topics: vec![[8; 32]],
            data: vec![2],
        }]);

        assert!(!base.same_chain_fact(&split));
    }

    #[test]
    fn semantic_quorum_rejects_split_transaction_calldata() {
        let transaction = |input: &str| {
            Json::Object(vec![
                ("to".to_owned(), Json::Text("0x11".to_owned())),
                ("input".to_owned(), Json::Text(input.to_owned())),
                ("value".to_owned(), Json::Text("0x0".to_owned())),
            ])
        };
        let observations = vec![
            ("first".to_owned(), transaction("0x0102")),
            ("second".to_owned(), transaction("0x0103")),
        ];

        assert!(semantic_quorum_index(&observations, 2).is_none());
    }

    #[test]
    fn semantic_quorum_rejects_split_contract_views() {
        let observations = vec![
            ("first".to_owned(), Json::Text("0x01".to_owned())),
            ("second".to_owned(), Json::Text("0x02".to_owned())),
        ];

        assert!(semantic_quorum_index(&observations, 2).is_none());
    }
}
