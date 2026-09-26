use serde_json::{json, Value};

use crate::config::StationConfig;
use crate::journal::{Completion, Entry, Journal, JournalError, Key, QuoteRecord, Submission};
use crate::price::{PriceError, PriceSource};
use crate::quote::{address_word, keccak, word, Address, Word};
use crate::rpc::{bytes, hex, quantity, read, JsonRpc, RpcFault};
use crate::signer::QuoteSigner;
use crate::tx::{self, Authorization, Call, Fees, TransactionRequest, TxError};
use crate::{QuoteError, QuoteRequest, SignedQuote, Station};

#[derive(Debug)]
pub enum StationError {
    Rpc(RpcFault),
    Journal(JournalError),
    Quote(QuoteError),
    Price(PriceError),
    Transaction(TxError),
    Invalid,
    Missing,
    Conflict,
}
impl std::fmt::Display for StationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "station refused: {self:?}")
    }
}
impl std::error::Error for StationError {}
impl From<RpcFault> for StationError {
    fn from(e: RpcFault) -> Self {
        Self::Rpc(e)
    }
}
impl From<JournalError> for StationError {
    fn from(e: JournalError) -> Self {
        Self::Journal(e)
    }
}
impl From<TxError> for StationError {
    fn from(e: TxError) -> Self {
        Self::Transaction(e)
    }
}

pub enum QuoteOutcome {
    Signed(SignedQuote),
    Completed(Completion),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Progress {
    Pending,
    Completed(Completion),
}
pub struct SubmitRequest {
    pub key: Key,
    pub account: Address,
    pub calls: Vec<Call>,
    pub authorizations: Vec<Authorization>,
    pub account_signature: [u8; 65],
}
pub struct GasStation<S, R, P> {
    station: Station<S>,
    rpc: R,
    prices: P,
    journal: Journal,
}
impl<S: QuoteSigner, R: JsonRpc, P: PriceSource> GasStation<S, R, P> {
    /// # Errors
    /// Refuses incompatible journal identity, policy history, configuration or chain identity.
    pub fn new(
        config: StationConfig,
        signer: S,
        rpc: R,
        prices: P,
        journal: Journal,
    ) -> Result<Self, StationError> {
        let mut station = Station::new(config, signer).map_err(|_| StationError::Invalid)?;
        let chain: String = read(&rpc, "eth_chainId", json!([]))?;
        if quantity(&chain)? != u128::from(station.config.chain_id) {
            return Err(StationError::Invalid);
        }
        let mut quotes: Vec<_> = journal
            .state()
            .items
            .values()
            .filter_map(|i| i.quote.as_ref())
            .collect();
        quotes.sort_by_key(|q| q.issued_at);
        for quote in quotes {
            if quote.chain_id != station.config.chain_id
                || quote.paymaster != station.config.paymaster
                || quote.token != station.config.token
                || quote.key.sponsor != station.signer.address()
            {
                return Err(StationError::Conflict);
            }
            station
                .policy
                .reserve(
                    quote.account,
                    amount(quote.amount)?,
                    amount(quote.gas_cost)?,
                    u128::MAX,
                    quote.issued_at,
                )
                .map_err(|e| StationError::Quote(QuoteError::Policy(e)))?;
        }
        Ok(Self {
            station,
            rpc,
            prices,
            journal,
        })
    }
    #[must_use]
    pub const fn journal(&self) -> &Journal {
        &self.journal
    }
    fn call_word(&self, account: Address, data: &[u8], block: &str) -> Result<Word, StationError> {
        let result: String = read(
            &self.rpc,
            "eth_call",
            json!([{"to":hex(&account),"data":hex(data)},block]),
        )?;
        bytes(&result)?
            .try_into()
            .map_err(|_| StationError::Rpc(RpcFault::Malformed))
    }
    fn consumed(&self, key: Key, account: Address) -> Result<bool, StationError> {
        let data = [
            keccak(b"usedQuoteNonces(address,uint256)")[..4].to_vec(),
            address_word(key.sponsor).to_vec(),
            key.quote_nonce.to_vec(),
        ]
        .concat();
        match self.call_word(account, &data, "finalized")? {
            value if value == word(0) => Ok(false),
            value if value == word(1) => Ok(true),
            _ => Err(RpcFault::Malformed.into()),
        }
    }
    fn complete(
        &mut self,
        key: Key,
        account: Address,
        completion: Completion,
    ) -> Result<Progress, StationError> {
        self.journal.append(&Entry::Completed {
            key,
            account,
            completion,
        })?;
        Ok(Progress::Completed(completion))
    }
    /// # Errors
    /// Refuses invalid prices, policy, conflicting nonce reuse or an undurable quote.
    pub fn quote(
        &mut self,
        request: &QuoteRequest,
        fees: Fees,
        now: u64,
    ) -> Result<QuoteOutcome, StationError> {
        let key = Key {
            sponsor: self.station.signer.address(),
            quote_nonce: request.quote_nonce,
        };
        if let Some(item) = self.journal.state().items.get(&key) {
            if item.account != request.account {
                return Err(StationError::Conflict);
            }
            if let Some(completion) = item.completion {
                return Ok(QuoteOutcome::Completed(completion));
            }
            let saved = item.quote.as_ref().ok_or(StationError::Missing)?;
            if saved.maximum != word(request.max_token_amount)
                || saved.gas_cost != word(request.gas_cost)
                || saved.deadline != request.deadline
                || saved.fees != fees
            {
                return Err(StationError::Conflict);
            }
            return Ok(QuoteOutcome::Signed(saved.signed_quote()?));
        }
        if self.consumed(key, request.account)? {
            self.complete(key, request.account, Completion::Consumed)?;
            return Ok(QuoteOutcome::Completed(Completion::Consumed));
        }
        if fees.gas_cost()? != request.gas_cost {
            return Err(StationError::Invalid);
        }
        let balance: String = read(
            &self.rpc,
            "eth_getBalance",
            json!([hex(&key.sponsor), "pending"]),
        )?;
        let rate = self.prices.governed_rate().map_err(StationError::Price)?;
        let signed = self
            .station
            .quote(request, &rate, quantity(&balance)?, now)
            .map_err(StationError::Quote)?;
        self.journal.append(&Entry::Quoted {
            quote: Box::new(QuoteRecord {
                key,
                chain_id: self.station.config.chain_id,
                paymaster: self.station.config.paymaster,
                account: request.account,
                token: signed.quote.token,
                maximum: signed.quote.max_token_amount,
                amount: signed.quote.token_amount,
                deadline: request.deadline,
                gas_cost: signed.quote.gas_cost,
                issued_at: now,
                signature: signed.signature.to_vec(),
                fees,
            }),
        })?;
        Ok(QuoteOutcome::Signed(signed))
    }
    /// # Errors
    /// Refuses account signatures, authorizations, expired quotes or conflicting journal state.
    pub fn submit(&mut self, request: &SubmitRequest, now: u64) -> Result<Progress, StationError> {
        if request.key.sponsor != self.station.signer.address() {
            return Err(StationError::Invalid);
        }
        if let Some(item) = self.journal.state().items.get(&request.key) {
            if item.account != request.account {
                return Err(StationError::Conflict);
            }
            if let Some(done) = item.completion {
                return Ok(Progress::Completed(done));
            }
            if item.submission.is_some() {
                return self.resume(request.key);
            }
        }
        if self.consumed(request.key, request.account)? {
            return self.complete(request.key, request.account, Completion::Consumed);
        }
        let quote = self
            .journal
            .state()
            .items
            .get(&request.key)
            .and_then(|i| i.quote.clone())
            .ok_or(StationError::Missing)?;
        if quote.deadline < now {
            return Err(StationError::Invalid);
        }
        let signed = quote.signed_quote()?;
        let batch_nonce = self.call_word(request.account, &keccak(b"nonce()")[..4], "pending")?;
        let account_nonce: String = read(
            &self.rpc,
            "eth_getTransactionCount",
            json!([hex(&request.account), "pending"]),
        )?;
        if request.authorizations.len() != 1
            || u128::from(request.authorizations[0].nonce) != quantity(&account_nonce)?
        {
            return Err(StationError::Invalid);
        }
        let pending: String = read(
            &self.rpc,
            "eth_getTransactionCount",
            json!([hex(&request.key.sponsor), "pending"]),
        )?;
        let mut nonce = u64::try_from(quantity(&pending)?).map_err(|_| StationError::Invalid)?;
        for item in self.journal.state().items.values() {
            if let Some(previous) = &item.submission {
                nonce = nonce.max(previous.nonce.checked_add(1).ok_or(StationError::Invalid)?);
            }
        }
        let transaction = tx::sign(
            &TransactionRequest {
                chain_id: quote.chain_id,
                account: request.account,
                paymaster: quote.paymaster,
                nonce,
                batch_nonce,
                fees: quote.fees,
                calls: &request.calls,
                authorizations: &request.authorizations,
                quote: &signed,
                account_signature: &request.account_signature,
            },
            &self.station.signer,
        )?;
        self.journal.append(&Entry::Prepared {
            key: request.key,
            submission: Submission {
                nonce,
                hash: transaction.hash,
                raw: transaction.raw,
            },
        })?;
        self.broadcast(request.key)
    }
    fn broadcast(&self, key: Key) -> Result<Progress, StationError> {
        let submission = self
            .journal
            .state()
            .items
            .get(&key)
            .and_then(|i| i.submission.as_ref())
            .ok_or(StationError::Missing)?;
        match self
            .rpc
            .send_raw_transaction(&submission.raw, &submission.hash)
        {
            Ok(_)
            | Err(
                RpcFault::Unavailable
                | RpcFault::RateLimited
                | RpcFault::Divergence
                | RpcFault::Malformed,
            ) => Ok(Progress::Pending),
            Err(error) => Err(error.into()),
        }
    }
    /// # Errors
    /// Refuses malformed receipts or RPC failures; every rebroadcast uses the durable bytes.
    pub fn resume(&mut self, key: Key) -> Result<Progress, StationError> {
        let item = self
            .journal
            .state()
            .items
            .get(&key)
            .cloned()
            .ok_or(StationError::Missing)?;
        if let Some(done) = item.completion {
            return Ok(Progress::Completed(done));
        }
        if let Some(submission) = &item.submission {
            let receipt = self
                .rpc
                .call("eth_getTransactionReceipt", json!([hex(&submission.hash)]))?;
            if !receipt.is_null() {
                let block = field_quantity(&receipt, "blockNumber")?;
                let finalized = self
                    .rpc
                    .call("eth_getBlockByNumber", json!(["finalized", false]))?;
                if field_quantity(&finalized, "number")? < block {
                    return Ok(Progress::Pending);
                }
                let canonical = self.rpc.call(
                    "eth_getBlockByNumber",
                    json!([format!("0x{block:x}"), false]),
                )?;
                if receipt["blockHash"].as_str().is_none()
                    || canonical["hash"] != receipt["blockHash"]
                {
                    return Err(RpcFault::Divergence.into());
                }
                let completion = receipt_completion(
                    &receipt,
                    submission,
                    item.quote.as_ref().ok_or(StationError::Missing)?,
                )?;
                if matches!(completion, Completion::Reverted { .. })
                    && self.consumed(key, item.account)?
                {
                    return self.complete(key, item.account, Completion::Consumed);
                }
                return self.complete(key, item.account, completion);
            }
        }
        if self.consumed(key, item.account)? {
            return self.complete(key, item.account, Completion::Consumed);
        }
        if item.submission.is_some() {
            match self.broadcast(key) {
                Err(StationError::Rpc(RpcFault::Rejected { .. })) => Ok(Progress::Pending),
                result => result,
            }
        } else {
            Ok(Progress::Pending)
        }
    }
}
fn amount(value: Word) -> Result<u128, StationError> {
    if value[..16] != [0; 16] {
        return Err(StationError::Invalid);
    }
    Ok(u128::from_be_bytes(
        value[16..].try_into().map_err(|_| StationError::Invalid)?,
    ))
}
fn field_quantity(value: &Value, name: &str) -> Result<u128, RpcFault> {
    quantity(value[name].as_str().ok_or(RpcFault::Malformed)?)
}
fn receipt_completion(
    receipt: &Value,
    submission: &Submission,
    quote: &QuoteRecord,
) -> Result<Completion, StationError> {
    if receipt["transactionHash"] != hex(&submission.hash)
        || receipt["from"] != hex(&quote.key.sponsor)
        || receipt["to"] != hex(&quote.account)
    {
        return Err(RpcFault::Malformed.into());
    }
    let status = field_quantity(receipt, "status")?;
    if status == 0 {
        return Ok(Completion::Reverted {
            hash: submission.hash,
        });
    }
    if status != 1 {
        return Err(RpcFault::Malformed.into());
    }
    let gas = field_quantity(receipt, "gasUsed")?;
    let price = field_quantity(receipt, "effectiveGasPrice")?;
    if gas == 0 || gas > u128::from(quote.fees.gas_limit) || price > quote.fees.max_fee_per_gas {
        return Err(RpcFault::Malformed.into());
    }
    let spent = gas.checked_mul(price).ok_or(RpcFault::Malformed)?;
    let logs = receipt["logs"].as_array().ok_or(RpcFault::Malformed)?;
    let transfer = json!([
        hex(&keccak(b"Transfer(address,address,uint256)")),
        hex(&address_word(quote.account)),
        hex(&address_word(quote.key.sponsor))
    ]);
    let sponsored = json!([
        hex(&keccak(b"Sponsored(address,address,uint256,uint256)")),
        hex(&address_word(quote.key.sponsor)),
        hex(&address_word(quote.token))
    ]);
    let mut collected = 0_u128;
    let mut sponsored_count = 0;
    for log in logs {
        if log["removed"] == true {
            return Err(RpcFault::Malformed.into());
        }
        if log["address"] == hex(&quote.token) && log["topics"] == transfer {
            let data = bytes(log["data"].as_str().ok_or(RpcFault::Malformed)?)?;
            let value: Word = data.try_into().map_err(|_| RpcFault::Malformed)?;
            collected = collected
                .checked_add(amount(value)?)
                .ok_or(RpcFault::Malformed)?;
        }
        if log["address"] == hex(&quote.account) && log["topics"] == sponsored {
            if log["data"] != hex(&[quote.amount, quote.key.quote_nonce].concat()) {
                return Err(RpcFault::Malformed.into());
            }
            sponsored_count += 1;
        }
    }
    if word(collected) != quote.amount || sponsored_count != 1 {
        return Err(RpcFault::Malformed.into());
    }
    Ok(Completion::Included {
        hash: submission.hash,
        block_number: u64::try_from(field_quantity(receipt, "blockNumber")?)
            .map_err(|_| RpcFault::Malformed)?,
        sid_collected: word(collected),
        pax_spent: word(spent),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_wide_amounts_and_noncanonical_receipt_quantities() {
        assert!(amount([255; 32]).is_err());
        assert_eq!(amount(word(u128::MAX)).ok(), Some(u128::MAX));
        assert_eq!(
            field_quantity(&json!({"gasUsed":"0x00"}), "gasUsed"),
            Err(RpcFault::Malformed)
        );
        assert_eq!(
            field_quantity(&json!({}), "gasUsed"),
            Err(RpcFault::Malformed)
        );
    }
}
