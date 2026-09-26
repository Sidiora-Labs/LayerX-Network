//! Paxeer X Network sponsored submission library.
//!
//! Pass the JSON configuration file path to `StationConfig::load`. Fields are
//! `chain_id`, `endpoints`, `paymaster`, `token`, `decimals`, `sid_denom`,
//! `pax_denom`, `max_rate_age`, `spread_bps`, `margin_bps`, `per_account_limit`,
//! `per_interval_limit`, `per_quote_limit`, `interval_seconds`, `balance_floor`,
//! and `relayer_key_env`. Amount limits use SID base units; the balance floor
//! uses PAX base units. The configured `relayer_key_env` names the signing
//! environment variable, conventionally `PAXEER_RELAYER_KEY`.
//!
//! Supply a governed `PriceSource` to `GasStation`; the retired oracle cannot
//! supply rates. The paymaster also requires a governed rate before live use.
//! The caller opens a journal path such as `state/sponsorship.jsonl` and passes
//! it to the station. Each JSON line is `quoted`, `prepared`, or `completed`,
//! keyed by sponsor and quote nonce. Quotes persist reservations and signatures;
//! prepared entries persist the transaction nonce, hash and exact signed bytes;
//! completion records contain consumed, included or reverted outcomes. No key,
//! endpoint or credential is serialized. Entries are flushed and synced before
//! publication or broadcast. Exclusive locking prevents simultaneous writers;
//! corrupt or torn lines fail closed. Restart replays policy reservations and
//! rebroadcasts the saved bytes. Reservations remain conservative after settlement.

pub mod journal;
pub mod rpc;
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
use price::{OracleRate, PriceError, Pricing};
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
    /// Refuses invalid requests, oracle prices, exhausted budgets or signer failures.
    pub fn quote(
        &mut self,
        request: &QuoteRequest,
        rates: &[OracleRate],
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
            .quote(rates, request.gas_cost, now)
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
            max_token_amount: 2_100_000,
            gas_cost: 1_000_000_000_000_000_000,
            deadline: 1019,
            quote_nonce: word(7),
        };
        let rates = price::tests::rates();
        request.max_token_amount = 1;
        assert!(matches!(
            station.quote(&request, &rates, u128::MAX, 1000),
            Err(QuoteError::AboveMaximum)
        ));
        request.max_token_amount = 2_100_000;
        let result = station
            .quote(&request, &rates, u128::MAX, 1000)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(result.quote.token_amount, word(2_020_000));
        assert_eq!(
            result.digest,
            quote_digest(word(1325), request.account, &result.quote)
        );
        assert!(matches!(result.signature[64], 27 | 28));
        assert!(matches!(
            station.quote(&request, &rates, u128::MAX, 1000),
            Err(QuoteError::Policy(PolicyRefusal::PerAccount))
        ));
        request.deadline = 1020;
        assert!(matches!(
            station.quote(&request, &rates, u128::MAX, 1000),
            Err(QuoteError::InvalidRequest)
        ));
        Ok(())
    }
}
