//! Paxeer X Network sponsored submission library.
//!
//! Pass the JSON configuration file path to `StationConfig::load`. Fields are
//! `chain_id`, `endpoints`, `paymaster`, `token`, `decimals`, `max_rate_age`,
//! `spread_bps`, `margin_bps`, `per_account_limit`, `per_interval_limit`,
//! `per_quote_limit`, `interval_seconds`, `balance_floor`, and `relayer_key_env`. Amount limits use SID base units; the balance floor
//! uses PAX base units. The configured `relayer_key_env` names the signing
//! environment variable, conventionally `PAXEER_RELAYER_KEY`.
//!
//! The station's only price source is the paymaster's governed rate:
//! `PaymasterRateSource` reads `currentRate()` and `rateUpdatedAt()` from the
//! configured paymaster, in Sidiora base units per whole Paxeer coin. A revert,
//! a missing rate, or a rate older than `max_rate_age` refuses the quote, and the
//! quoted amount is the governed price plus `margin_bps`, within `spread_bps`.
//! The caller opens a journal path such as `state/sponsorship.jsonl` and passes
//! it to the station. Each JSON line is `quoted`, `prepared`, `released`,
//! `replaced`, `cancelled`, or `completed`, keyed by sponsor and quote nonce.
//! Quotes persist reservations and signatures; prepared entries persist the
//! transaction nonce, hash and exact signed bytes; a released entry frees the
//! sponsor nonce of a submission the node refused; a replaced entry persists the
//! fee and signed bytes of the zero-value self-transfer that fills the sponsor
//! nonce of a dropped or expired submission, written before it is broadcast; a
//! cancelled entry records that replacement's inclusion; completion records
//! contain consumed, included, reverted or cancelled outcomes. No key,
//! endpoint or credential is serialized. Entries are flushed and synced before
//! publication or broadcast. Exclusive locking prevents simultaneous writers;
//! corrupt or torn lines fail closed. Restart replays policy reservations,
//! rebroadcasts the saved bytes and resumes each replacement from the journal.
//! Reservations remain conservative after settlement.
//!
//! The `paxeer-gas-station` binary runs as
//! `paxeer-gas-station --config PATH --journal PATH`. Its configuration file
//! carries the fields above plus `listen`, the socket address it serves on
//! (port zero is refused), `gas_limit`, the gas limit of every sponsored
//! transaction, and `max_priority_fee_per_gas` in PAX base units; a quote's
//! `gasCost` must be `gas_limit` times the maximum fee per gas it pays. The
//! service answers `POST /quote` with `{quote, relayerSignature}` and
//! `POST /submit` with `{transactionHash}`, with every numeric field a decimal
//! string and `decimals` a number. A request the station refuses is answered
//! with a 4xx status; an unavailable price source, an unreachable node, an
//! unusable clock or a failing journal with a 5xx status. It speaks plain
//! HTTP: the web adapter requires an `https` endpoint, so TLS is terminated in
//! front of the process. Request bodies and read time are capped, and neither
//! a response nor a log line carries key material, a credential or a signed
//! transaction byte.

pub mod journal;
pub mod rpc;
pub mod service;
pub mod station;
pub mod tx;
pub use station::GasStation;
pub mod config;
pub mod policy;
pub mod price;
pub mod quote;
pub mod signer;

use config::{ConfigError, StationConfig};
use policy::{PolicyRefusal, QuotePolicy};
use price::{GovernedRate, PriceError, Pricing};
use quote::{quote_digest, word, Address, Quote, Word};
use signer::{QuoteSigner, SignerError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuoteError {
    Price(PriceError),
    Policy(PolicyRefusal),
    Signer(SignerError),
    InvalidRequest,
    AboveMaximum,
}

pub struct QuoteRequest {
    pub account: Address,
    pub max_token_amount: u128,
    pub gas_cost: u128,
    pub deadline: u64,
    pub quote_nonce: Word,
}

pub struct SignedQuote {
    pub quote: Quote,
    pub digest: Word,
    pub signature: [u8; 65],
}

pub struct Station<S> {
    config: StationConfig,
    pricing: Pricing,
    policy: QuotePolicy,
    signer: S,
}
impl<S: QuoteSigner> Station<S> {
    /// # Errors
    /// Refuses inconsistent configuration or a zero sponsor.
    pub fn new(config: StationConfig, signer: S) -> Result<Self, ConfigError> {
        if signer.address() == [0; 20] {
            return Err(ConfigError {
                field: "relayer_key_env",
            });
        }
        let pricing = Pricing::new(&config)?;
        let policy = QuotePolicy::new(&config)?;
        Ok(Self {
            config,
            pricing,
            policy,
            signer,
        })
    }

    /// # Errors
    /// Refuses invalid requests, unusable governed rates, exhausted budgets or signer failures.
    pub fn quote(
        &mut self,
        request: &QuoteRequest,
        rate: &GovernedRate,
        balance: u128,
        now: u64,
    ) -> Result<SignedQuote, QuoteError> {
        if request.account == [0; 20]
            || request.account == self.signer.address()
            || request.deadline < now
            || request.deadline / self.config.interval_seconds != now / self.config.interval_seconds
        {
            return Err(QuoteError::InvalidRequest);
        }
        let amount = self
            .pricing
            .quote(rate, request.gas_cost, now)
            .map_err(QuoteError::Price)?;
        if amount > request.max_token_amount {
            return Err(QuoteError::AboveMaximum);
        }
        let quote = Quote {
            sponsor: self.signer.address(),
            token: self.config.token,
            max_token_amount: word(request.max_token_amount),
            token_amount: word(amount),
            deadline: word(u128::from(request.deadline)),
            nonce: request.quote_nonce,
            gas_cost: word(request.gas_cost),
        };
        let digest = quote_digest(
            word(u128::from(self.config.chain_id)),
            request.account,
            &quote,
        );
        self.policy
            .reserve(request.account, amount, request.gas_cost, balance, now)
            .map_err(QuoteError::Policy)?;
        let signature = self
            .signer
            .sign_digest(digest)
            .map_err(QuoteError::Signer)?;
        Ok(SignedQuote {
            quote,
            digest,
            signature,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quote_prices_signs_and_reserves_budget() -> Result<(), Box<dyn std::error::Error>> {
        let config = config::tests::config();
        let signer = signer::tests::signer()?;
        let mut station = Station::new(config, signer)?;
        let mut request = QuoteRequest {
            account: [0x11; 20],
            max_token_amount: 2_400_000,
            gas_cost: 750_000_000_000_000_000,
            deadline: 1019,
            quote_nonce: word(7),
        };
        let rate = price::tests::RATE;
        request.max_token_amount = 1;
        assert!(matches!(
            station.quote(&request, &rate, u128::MAX, 1000),
            Err(QuoteError::AboveMaximum)
        ));
        request.max_token_amount = 2_400_000;
        let result = station
            .quote(&request, &rate, u128::MAX, 1000)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(result.quote.token_amount, word(2_358_855));
        assert_eq!(
            result.digest,
            quote_digest(word(1325), request.account, &result.quote)
        );
        assert!(matches!(result.signature[64], 27 | 28));
        assert!(matches!(
            station.quote(&request, &rate, u128::MAX, 1000),
            Err(QuoteError::Policy(PolicyRefusal::PerAccount))
        ));
        request.deadline = 1020;
        assert!(matches!(
            station.quote(&request, &rate, u128::MAX, 1000),
            Err(QuoteError::InvalidRequest)
        ));
        Ok(())
    }
}
