//! Read-only access to the paxscan Blockscout Postgres database.
//!
//! Every range is read inside one read-only repeatable-read transaction,
//! so the six tables are a single consistent snapshot. The connection
//! requires TLS. The paxscan server presents a self-signed certificate, so
//! it is authenticated either by pinning the SHA-256 of its leaf
//! certificate or, when the operator opts in, left unauthenticated; the
//! backfill verifies every paxscan block hash against the node before
//! committing it either way.

use std::sync::Arc;
use std::time::Duration;

use postgres::config::SslMode;
use postgres::{Client, IsolationLevel, Row};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use sha2::{Digest as _, Sha256};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::blockscout::{
    BlockRow, BlockscoutRange, InternalTransactionRow, LogRow, TokenRow, TokenTransferRow,
    TransactionRow,
};
use crate::IndexError;

/// How the paxscan server certificate is authenticated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaxscanTls {
    /// The SHA-256 of the server's leaf certificate must equal this pin.
    PinnedLeaf([u8; 32]),
    /// TLS without server authentication (operator opt-in).
    Unauthenticated,
}

#[derive(Debug)]
struct PaxscanVerifier {
    pin: Option<[u8; 32]>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PaxscanVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self.pin {
            Some(pin) if Sha256::digest(end_entity.as_ref()).as_slice() != pin => Err(
                rustls::Error::General("paxscan certificate does not match its pin".to_owned()),
            ),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn source(error: &postgres::Error) -> IndexError {
    IndexError::Source(format!("paxscan: {error}"))
}

fn signed(value: u64) -> Result<i64, IndexError> {
    i64::try_from(value).map_err(|_| IndexError::Config(format!("height {value} is too large")))
}

fn unsigned(row: &Row, index: usize) -> Result<u64, IndexError> {
    let value: i64 = row.try_get(index).map_err(|error| source(&error))?;
    u64::try_from(value)
        .map_err(|_| IndexError::Decode(format!("paxscan column {index} is negative: {value}")))
}

fn get<'a, T: postgres::types::FromSql<'a>>(row: &'a Row, index: usize) -> Result<T, IndexError> {
    row.try_get(index).map_err(|error| source(&error))
}

/// A read-only session on the paxscan database.
pub struct PaxscanDatabase {
    client: Client,
}

impl PaxscanDatabase {
    /// Connects with TLS required. `url` is a libpq-style connection string;
    /// it is never echoed into errors.
    ///
    /// # Errors
    /// Refuses an unparseable connection string and returns connection or
    /// TLS failures.
    pub fn connect(url: &str, tls: &PaxscanTls) -> Result<Self, IndexError> {
        let mut config: postgres::Config = url.parse().map_err(|_| {
            IndexError::Config("PAXSCAN_DATABASE_PUBLIC_URL is not a Postgres URL".to_owned())
        })?;
        config
            .ssl_mode(SslMode::Require)
            .connect_timeout(Duration::from_secs(30))
            .application_name("layerx-indexer-backfill");
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = PaxscanVerifier {
            pin: match tls {
                PaxscanTls::PinnedLeaf(pin) => Some(*pin),
                PaxscanTls::Unauthenticated => None,
            },
            provider: Arc::clone(&provider),
        };
        let client_config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|error| IndexError::Config(format!("paxscan TLS: {error}")))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        let mut client = config
            .connect(MakeRustlsConnect::new(client_config))
            .map_err(|error| source(&error))?;
        client
            .batch_execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY")
            .map_err(|error| source(&error))?;
        Ok(Self { client })
    }

    /// Reads every Blockscout row of heights `from..=to` from one snapshot.
    ///
    /// # Errors
    /// Returns query failures and malformed columns.
    pub fn range(&mut self, from: u64, to: u64) -> Result<BlockscoutRange, IndexError> {
        let low = signed(from)?;
        let high = signed(to)?;
        let mut transaction = self
            .client
            .build_transaction()
            .read_only(true)
            .isolation_level(IsolationLevel::RepeatableRead)
            .start()
            .map_err(|error| source(&error))?;
        let mut query = |sql: &str| {
            transaction
                .query(sql, &[&low, &high])
                .map_err(|error| source(&error))
        };
        let blocks = query(
            "SELECT number, hash, parent_hash, EXTRACT(EPOCH FROM timestamp)::bigint,
                    miner_hash, gas_used::text
             FROM blocks WHERE consensus AND number BETWEEN $1::bigint AND $2::bigint
             ORDER BY number",
        )?
        .iter()
        .map(|row| {
            Ok(BlockRow {
                number: unsigned(row, 0)?,
                hash: get(row, 1)?,
                parent_hash: get(row, 2)?,
                timestamp: unsigned(row, 3)?,
                miner_hash: get(row, 4)?,
                gas_used: get(row, 5)?,
            })
        })
        .collect::<Result<Vec<_>, IndexError>>()?;
        let transactions = query(
            "SELECT t.hash, t.block_number::bigint, t.index::bigint, t.from_address_hash,
                    t.to_address_hash, t.value::text, t.gas_used::text, t.status::bigint, t.input,
                    t.created_contract_address_hash
             FROM transactions t JOIN blocks b ON b.hash = t.block_hash AND b.consensus
             WHERE t.block_number BETWEEN $1::bigint AND $2::bigint
             ORDER BY t.block_number, t.index",
        )?
        .iter()
        .map(|row| {
            Ok(TransactionRow {
                hash: get(row, 0)?,
                block_number: unsigned(row, 1)?,
                index: unsigned(row, 2)?,
                from_address_hash: get(row, 3)?,
                to_address_hash: get(row, 4)?,
                value: get(row, 5)?,
                gas_used: get(row, 6)?,
                status: get(row, 7)?,
                input: get(row, 8)?,
                created_contract_address_hash: get(row, 9)?,
            })
        })
        .collect::<Result<Vec<_>, IndexError>>()?;
        let logs = query(
            "SELECT l.transaction_hash, l.index::bigint, l.address_hash, l.first_topic,
                    l.second_topic, l.third_topic, l.fourth_topic, l.data, l.block_number::bigint
             FROM logs l JOIN blocks b ON b.hash = l.block_hash AND b.consensus
             WHERE l.block_number BETWEEN $1::bigint AND $2::bigint
             ORDER BY l.block_number, l.index",
        )?
        .iter()
        .map(|row| {
            Ok(LogRow {
                transaction_hash: get(row, 0)?,
                index: unsigned(row, 1)?,
                address_hash: get(row, 2)?,
                first_topic: get(row, 3)?,
                second_topic: get(row, 4)?,
                third_topic: get(row, 5)?,
                fourth_topic: get(row, 6)?,
                data: get(row, 7)?,
                block_number: unsigned(row, 8)?,
            })
        })
        .collect::<Result<Vec<_>, IndexError>>()?;
        let token_transfers = query(
            "SELECT tt.transaction_hash, tt.log_index::bigint, tt.from_address_hash,
                    tt.to_address_hash, tt.amount::text, tt.token_contract_address_hash,
                    tt.token_type, tt.block_number::bigint
             FROM token_transfers tt JOIN blocks b ON b.hash = tt.block_hash AND b.consensus
             WHERE tt.block_number BETWEEN $1::bigint AND $2::bigint
             ORDER BY tt.block_number, tt.log_index",
        )?
        .iter()
        .map(|row| {
            Ok(TokenTransferRow {
                transaction_hash: get(row, 0)?,
                log_index: unsigned(row, 1)?,
                from_address_hash: get(row, 2)?,
                to_address_hash: get(row, 3)?,
                amount: get(row, 4)?,
                token_contract_address_hash: get(row, 5)?,
                token_type: get(row, 6)?,
                block_number: unsigned(row, 7)?,
            })
        })
        .collect::<Result<Vec<_>, IndexError>>()?;
        let internal_transactions = query(
            "SELECT block_number::bigint, transaction_index::bigint, index::bigint,
                    from_address_hash, to_address_hash, value::text,
                    COALESCE(call_type, call_type_enum::text), type
             FROM internal_transactions
             WHERE block_number BETWEEN $1::bigint AND $2::bigint
             ORDER BY block_number, transaction_index, index",
        )?
        .iter()
        .map(|row| {
            Ok(InternalTransactionRow {
                block_number: unsigned(row, 0)?,
                transaction_index: unsigned(row, 1)?,
                index: unsigned(row, 2)?,
                from_address_hash: get(row, 3)?,
                to_address_hash: get(row, 4)?,
                value: get(row, 5)?,
                call_type: get(row, 6)?,
                kind: get(row, 7)?,
            })
        })
        .collect::<Result<Vec<_>, IndexError>>()?;
        let tokens = query(
            "SELECT contract_address_hash, symbol, name, decimals::text, type
             FROM tokens WHERE contract_address_hash IN (
                 SELECT token_contract_address_hash FROM token_transfers
                 WHERE block_number BETWEEN $1::bigint AND $2::bigint)
             ORDER BY contract_address_hash",
        )?
        .iter()
        .map(|row| {
            Ok(TokenRow {
                contract_address_hash: get(row, 0)?,
                symbol: get(row, 1)?,
                name: get(row, 2)?,
                decimals: get(row, 3)?,
                kind: get(row, 4)?,
            })
        })
        .collect::<Result<Vec<_>, IndexError>>()?;
        transaction.commit().map_err(|error| source(&error))?;
        Ok(BlockscoutRange {
            blocks,
            transactions,
            logs,
            token_transfers,
            internal_transactions,
            tokens,
        })
    }
}
