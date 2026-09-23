//! The durable decoded store: one SQLite file, written only in whole units.
//!
//! A unit is one LayerX batch or one Paxeer block. Its transfers, events,
//! assets, accounts, chain link and cursor advance are committed in a single
//! transaction, so a crash leaves the store at the previous unit boundary and
//! restart resumes from the durable cursor. Rollback deletes every row above
//! a fork point's boundary in one transaction.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension as _, Row};
use serde_json::{json, Value};

use crate::IndexError;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS accounts(
    chain TEXT NOT NULL,
    account TEXT NOT NULL,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    PRIMARY KEY(chain, account)
);
CREATE TABLE IF NOT EXISTS assets(
    asset TEXT PRIMARY KEY,
    chain TEXT NOT NULL,
    kind TEXT NOT NULL,
    address TEXT,
    denom TEXT,
    first_seen INTEGER NOT NULL,
    metadata_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS assets_address ON assets(address);
CREATE TABLE IF NOT EXISTS transfers(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    height_or_seq INTEGER NOT NULL,
    chain TEXT NOT NULL,
    kind TEXT NOT NULL,
    direction TEXT NOT NULL,
    account TEXT NOT NULL,
    counterparty TEXT,
    asset TEXT NOT NULL,
    amount TEXT NOT NULL,
    tx_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    decoded_json TEXT NOT NULL,
    UNIQUE(chain, tx_id, ordinal, direction)
);
CREATE INDEX IF NOT EXISTS transfers_account ON transfers(account, id);
CREATE INDEX IF NOT EXISTS transfers_chain_position ON transfers(chain, height_or_seq);
CREATE TABLE IF NOT EXISTS events(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    height_or_seq INTEGER NOT NULL,
    chain TEXT NOT NULL,
    source TEXT NOT NULL,
    name TEXT NOT NULL,
    contract TEXT,
    account TEXT,
    tx_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    decoded_json TEXT NOT NULL,
    UNIQUE(chain, source, tx_id, ordinal)
);
CREATE INDEX IF NOT EXISTS events_account ON events(account, id);
CREATE INDEX IF NOT EXISTS events_chain_position ON events(chain, height_or_seq);
CREATE TABLE IF NOT EXISTS cursors(
    chain TEXT PRIMARY KEY,
    position INTEGER NOT NULL,
    hash TEXT NOT NULL,
    finalized_position INTEGER,
    finalized_boundary INTEGER,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS chain_links(
    chain TEXT NOT NULL,
    position INTEGER NOT NULL,
    hash TEXT NOT NULL,
    parent TEXT NOT NULL,
    link TEXT NOT NULL,
    boundary INTEGER NOT NULL,
    PRIMARY KEY(chain, position)
);
";

/// One decoded transfer leg.
#[derive(Clone, Debug, PartialEq)]
pub struct TransferRow {
    pub height_or_seq: u64,
    pub kind: String,
    /// `in` for a credit to `account`, `out` for a debit from it.
    pub direction: &'static str,
    pub account: String,
    pub counterparty: Option<String>,
    pub asset: String,
    pub amount: String,
    pub tx_id: String,
    pub ordinal: u64,
    pub decoded: Value,
}

/// One decoded event.
#[derive(Clone, Debug, PartialEq)]
pub struct EventRow {
    pub height_or_seq: u64,
    pub source: String,
    pub name: String,
    pub contract: Option<String>,
    pub account: Option<String>,
    pub tx_id: String,
    pub ordinal: u64,
    pub decoded: Value,
}

/// One asset observed in a unit.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetRow {
    pub asset: String,
    pub chain: String,
    pub kind: String,
    pub address: Option<String>,
    pub denom: Option<String>,
    pub metadata: Value,
}

/// One chain unit: a LayerX batch or a Paxeer block.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Unit {
    pub chain: String,
    pub position: u64,
    /// The unit's own identity (batch id or block hash).
    pub hash: String,
    /// What the unit declares its predecessor's link to be.
    pub parent: String,
    /// What the successor must declare as its parent.
    pub link: String,
    /// The largest `height_or_seq` any row in this unit may carry.
    pub boundary: u64,
    pub transfers: Vec<TransferRow>,
    pub events: Vec<EventRow>,
    pub assets: Vec<AssetRow>,
    pub accounts: Vec<String>,
}

/// The stored link of one indexed unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredLink {
    pub position: u64,
    pub hash: String,
    pub parent: String,
    pub link: String,
    pub boundary: u64,
}

/// A chain's durable cursor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub position: u64,
    pub hash: String,
    pub finalized_position: Option<u64>,
    pub finalized_boundary: Option<u64>,
}

/// One page of API items.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub items: Vec<Value>,
    pub next_cursor: Option<String>,
}

/// The shared SQLite store.
pub struct Store {
    connection: Mutex<Connection>,
}

fn signed(value: u64) -> Result<i64, IndexError> {
    i64::try_from(value).map_err(|_| IndexError::Store(format!("{value} exceeds SQLite range")))
}

fn unsigned(value: i64) -> Result<u64, IndexError> {
    u64::try_from(value).map_err(|_| IndexError::Store(format!("stored {value} is negative")))
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

fn parse_json(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

impl Store {
    /// Opens (creating when absent) the store at `path`.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] when SQLite cannot open or migrate it.
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        Self::initialise(connection)
    }

    /// Opens a private in-memory store.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] when SQLite fails.
    pub fn open_in_memory() -> Result<Self, IndexError> {
        Self::initialise(Connection::open_in_memory()?)
    }

    fn initialise(connection: Connection) -> Result<Self, IndexError> {
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.execute_batch(SCHEMA)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, IndexError> {
        self.connection
            .lock()
            .map_err(|_| IndexError::Store("store lock is poisoned".to_owned()))
    }

    /// The durable cursor of `chain`, when it has indexed anything.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] on SQLite failure.
    pub fn cursor(&self, chain: &str) -> Result<Option<Cursor>, IndexError> {
        let connection = self.lock()?;
        let row = connection
            .query_row(
                "SELECT position, hash, finalized_position, finalized_boundary FROM cursors WHERE chain = ?1",
                params![chain],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(position, hash, finalized, boundary)| {
            Ok(Cursor {
                position: unsigned(position)?,
                hash,
                finalized_position: finalized.map(unsigned).transpose()?,
                finalized_boundary: boundary.map(unsigned).transpose()?,
            })
        })
        .transpose()
    }

    /// The stored link of `chain` at `position`.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] on SQLite failure.
    pub fn link(&self, chain: &str, position: u64) -> Result<Option<StoredLink>, IndexError> {
        let connection = self.lock()?;
        Self::link_in(&connection, chain, position)
    }

    fn link_in(
        connection: &Connection,
        chain: &str,
        position: u64,
    ) -> Result<Option<StoredLink>, IndexError> {
        let row = connection
            .query_row(
                "SELECT hash, parent, link, boundary FROM chain_links WHERE chain = ?1 AND position = ?2",
                params![chain, signed(position)?],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(hash, parent, link, boundary)| {
            Ok(StoredLink {
                position,
                hash,
                parent,
                link,
                boundary: unsigned(boundary)?,
            })
        })
        .transpose()
    }

    /// Commits one unit atomically and advances the cursor to it.
    /// `finality_depth` decides which older unit becomes final and how many
    /// links are retained for reorg detection.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] on SQLite failure; nothing is written.
    pub fn commit(&self, unit: &Unit, finality_depth: u64) -> Result<(), IndexError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let position = signed(unit.position)?;
        let stamp = now();
        for asset in &unit.assets {
            transaction.execute(
                "INSERT INTO assets(asset, chain, kind, address, denom, first_seen, metadata_json)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(asset) DO UPDATE SET metadata_json = CASE
                     WHEN excluded.metadata_json = '{}' THEN assets.metadata_json
                     ELSE excluded.metadata_json END,
                   denom = COALESCE(assets.denom, excluded.denom),
                   address = COALESCE(assets.address, excluded.address)",
                params![
                    asset.asset,
                    asset.chain,
                    asset.kind,
                    asset.address,
                    asset.denom,
                    stamp,
                    asset.metadata.to_string()
                ],
            )?;
        }
        let mut accounts: Vec<&str> = unit.accounts.iter().map(String::as_str).collect();
        for transfer in &unit.transfers {
            transaction.execute(
                "INSERT INTO transfers(height_or_seq, chain, kind, direction, account, counterparty,
                    asset, amount, tx_id, ordinal, decoded_json)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    signed(transfer.height_or_seq)?,
                    unit.chain,
                    transfer.kind,
                    transfer.direction,
                    transfer.account,
                    transfer.counterparty,
                    transfer.asset,
                    transfer.amount,
                    transfer.tx_id,
                    signed(transfer.ordinal)?,
                    transfer.decoded.to_string()
                ],
            )?;
            accounts.push(&transfer.account);
        }
        for event in &unit.events {
            transaction.execute(
                "INSERT INTO events(height_or_seq, chain, source, name, contract, account, tx_id,
                    ordinal, decoded_json)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    signed(event.height_or_seq)?,
                    unit.chain,
                    event.source,
                    event.name,
                    event.contract,
                    event.account,
                    event.tx_id,
                    signed(event.ordinal)?,
                    event.decoded.to_string()
                ],
            )?;
            if let Some(account) = &event.account {
                accounts.push(account);
            }
        }
        accounts.sort_unstable();
        accounts.dedup();
        for account in accounts {
            transaction.execute(
                "INSERT INTO accounts(chain, account, first_seen, last_seen) VALUES(?1, ?2, ?3, ?3)
                 ON CONFLICT(chain, account) DO UPDATE SET last_seen = MAX(accounts.last_seen, excluded.last_seen)",
                params![unit.chain, account, position],
            )?;
        }
        transaction.execute(
            "INSERT OR REPLACE INTO chain_links(chain, position, hash, parent, link, boundary)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                unit.chain,
                position,
                unit.hash,
                unit.parent,
                unit.link,
                signed(unit.boundary)?
            ],
        )?;
        let finalized = unit.position.checked_sub(finality_depth);
        let finalized_boundary = match finalized {
            Some(finalized) => Self::link_in(&transaction, &unit.chain, finalized)?
                .map(|link| signed(link.boundary))
                .transpose()?,
            None => None,
        };
        if let Some(finalized) = finalized {
            transaction.execute(
                "DELETE FROM chain_links WHERE chain = ?1 AND position < ?2",
                params![unit.chain, signed(finalized)?],
            )?;
        }
        transaction.execute(
            "INSERT INTO cursors(chain, position, hash, finalized_position, finalized_boundary, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(chain) DO UPDATE SET position = excluded.position, hash = excluded.hash,
               finalized_position = COALESCE(excluded.finalized_position, cursors.finalized_position),
               finalized_boundary = COALESCE(excluded.finalized_boundary, cursors.finalized_boundary),
               updated_at = excluded.updated_at",
            params![
                unit.chain,
                position,
                unit.hash,
                finalized.map(signed).transpose()?,
                finalized_boundary,
                stamp
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Rolls `chain` back so that `fork` is its newest unit, or to empty
    /// when `fork` is `None`. Refuses to cross the finalized position.
    ///
    /// # Errors
    /// Returns [`IndexError::ReorgBeyondFinality`] when the fork is below the
    /// finalized position and [`IndexError::Store`] on SQLite failure.
    pub fn rollback(&self, chain: &str, fork: Option<u64>) -> Result<(), IndexError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let finalized: Option<i64> = transaction
            .query_row(
                "SELECT finalized_position FROM cursors WHERE chain = ?1",
                params![chain],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let finalized = finalized.map(unsigned).transpose()?;
        if let Some(final_position) = finalized {
            if fork.is_none_or(|fork| fork < final_position) {
                return Err(IndexError::ReorgBeyondFinality {
                    source: chain.to_owned(),
                    position: fork.unwrap_or(0),
                });
            }
        }
        match fork {
            Some(fork) => {
                let link = Self::link_in(&transaction, chain, fork)?.ok_or_else(|| {
                    IndexError::ReorgBeyondFinality {
                        source: chain.to_owned(),
                        position: fork,
                    }
                })?;
                let boundary = signed(link.boundary)?;
                transaction.execute(
                    "DELETE FROM transfers WHERE chain = ?1 AND height_or_seq > ?2",
                    params![chain, boundary],
                )?;
                transaction.execute(
                    "DELETE FROM events WHERE chain = ?1 AND height_or_seq > ?2",
                    params![chain, boundary],
                )?;
                transaction.execute(
                    "DELETE FROM chain_links WHERE chain = ?1 AND position > ?2",
                    params![chain, signed(fork)?],
                )?;
                transaction.execute(
                    "UPDATE cursors SET position = ?2, hash = ?3, updated_at = ?4 WHERE chain = ?1",
                    params![chain, signed(fork)?, link.hash, now()],
                )?;
            }
            None => {
                for statement in [
                    "DELETE FROM transfers WHERE chain = ?1",
                    "DELETE FROM events WHERE chain = ?1",
                    "DELETE FROM chain_links WHERE chain = ?1",
                    "DELETE FROM cursors WHERE chain = ?1",
                ] {
                    transaction.execute(statement, params![chain])?;
                }
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// True when `address` is registered as a pointer contract.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] on SQLite failure.
    pub fn is_pointer(&self, address: &str) -> Result<bool, IndexError> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT 1 FROM assets WHERE address = ?1 AND kind = 'pointer'",
                params![address],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Registers operator-declared assets (for example pointer contracts)
    /// outside any unit.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] on SQLite failure.
    pub fn register_assets(&self, assets: &[AssetRow]) -> Result<(), IndexError> {
        let connection = self.lock()?;
        for asset in assets {
            connection.execute(
                "INSERT INTO assets(asset, chain, kind, address, denom, first_seen, metadata_json)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(asset) DO UPDATE SET kind = excluded.kind,
                   address = excluded.address, denom = excluded.denom",
                params![
                    asset.asset,
                    asset.chain,
                    asset.kind,
                    asset.address,
                    asset.denom,
                    now(),
                    asset.metadata.to_string()
                ],
            )?;
        }
        Ok(())
    }

    /// Newest-first transfer history of `account`. `cursor` is the exclusive
    /// upper row id returned as `next_cursor` by the previous page.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] on SQLite failure.
    pub fn history(
        &self,
        account: &str,
        cursor: Option<u64>,
        limit: usize,
        kind: Option<&str>,
    ) -> Result<Page, IndexError> {
        let connection = self.lock()?;
        let finality = Self::finality(&connection)?;
        let upper = cursor.map_or(Ok(i64::MAX), signed)?;
        let fetch = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
        let mut statement = connection.prepare(
            "SELECT id, height_or_seq, chain, kind, direction, account, counterparty, asset, amount,
                tx_id, ordinal, decoded_json
             FROM transfers
             WHERE account = ?1 AND id < ?2 AND (?3 IS NULL OR kind = ?3)
             ORDER BY id DESC LIMIT ?4",
        )?;
        let rows = statement
            .query_map(params![account, upper, kind, fetch], |row| {
                Ok(Self::transfer_document(row, &finality))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::page(rows, limit))
    }

    fn finality(connection: &Connection) -> Result<Vec<(String, Option<i64>)>, IndexError> {
        let mut statement = connection.prepare("SELECT chain, finalized_boundary FROM cursors")?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn transfer_document(row: &Row<'_>, finality: &[(String, Option<i64>)]) -> (i64, Value) {
        let id: i64 = row.get(0).unwrap_or_default();
        let position: i64 = row.get(1).unwrap_or_default();
        let chain: String = row.get(2).unwrap_or_default();
        let final_row = finality
            .iter()
            .find(|(name, _)| *name == chain)
            .and_then(|(_, boundary)| *boundary)
            .is_some_and(|boundary| position <= boundary);
        let decoded: String = row.get(11).unwrap_or_default();
        (
            id,
            json!({
                "id": id.to_string(),
                "height_or_seq": position.to_string(),
                "chain": chain,
                "kind": row.get::<_, String>(3).unwrap_or_default(),
                "direction": row.get::<_, String>(4).unwrap_or_default(),
                "account": row.get::<_, String>(5).unwrap_or_default(),
                "counterparty": row.get::<_, Option<String>>(6).unwrap_or_default(),
                "asset": row.get::<_, String>(7).unwrap_or_default(),
                "amount": row.get::<_, String>(8).unwrap_or_default(),
                "tx_id": row.get::<_, String>(9).unwrap_or_default(),
                "ordinal": row.get::<_, i64>(10).unwrap_or_default().to_string(),
                "final": final_row,
                "decoded": parse_json(&decoded),
            }),
        )
    }

    fn page(mut rows: Vec<(i64, Value)>, limit: usize) -> Page {
        let more = rows.len() > limit;
        rows.truncate(limit);
        let next_cursor = if more {
            rows.last().map(|(id, _)| id.to_string())
        } else {
            None
        };
        Page {
            items: rows.into_iter().map(|(_, value)| value).collect(),
            next_cursor,
        }
    }

    /// Every event row of `chain`, oldest first, for inspection and tests.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] on SQLite failure.
    pub fn events(&self, chain: &str) -> Result<Vec<Value>, IndexError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT height_or_seq, source, name, contract, account, tx_id, ordinal, decoded_json
             FROM events WHERE chain = ?1 ORDER BY id",
        )?;
        let rows = statement
            .query_map(params![chain], |row| {
                let decoded: String = row.get(7)?;
                Ok(json!({
                    "height_or_seq": row.get::<_, i64>(0)?.to_string(),
                    "source": row.get::<_, String>(1)?,
                    "name": row.get::<_, String>(2)?,
                    "contract": row.get::<_, Option<String>>(3)?,
                    "account": row.get::<_, Option<String>>(4)?,
                    "tx_id": row.get::<_, String>(5)?,
                    "ordinal": row.get::<_, i64>(6)?.to_string(),
                    "decoded": parse_json(&decoded),
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Every transfer row of `chain`, oldest first.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] on SQLite failure.
    pub fn transfers(&self, chain: &str) -> Result<Vec<Value>, IndexError> {
        let connection = self.lock()?;
        let finality = Self::finality(&connection)?;
        let mut statement = connection.prepare(
            "SELECT id, height_or_seq, chain, kind, direction, account, counterparty, asset, amount,
                tx_id, ordinal, decoded_json
             FROM transfers WHERE chain = ?1 ORDER BY id",
        )?;
        let rows = statement
            .query_map(params![chain], |row| {
                Ok(Self::transfer_document(row, &finality).1)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Whether `account` has been seen on `chain`.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] on SQLite failure.
    pub fn account_known(&self, chain: &str, account: &str) -> Result<bool, IndexError> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT 1 FROM accounts WHERE chain = ?1 AND account = ?2",
                params![chain, account],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    fn asset_document(row: &Row<'_>) -> rusqlite::Result<(i64, Value)> {
        let metadata: String = row.get(7)?;
        Ok((
            row.get(0)?,
            json!({
                "asset": row.get::<_, String>(1)?,
                "chain": row.get::<_, String>(2)?,
                "kind": row.get::<_, String>(3)?,
                "address": row.get::<_, Option<String>>(4)?,
                "denom": row.get::<_, Option<String>>(5)?,
                "first_seen": row.get::<_, i64>(6)?.to_string(),
                "metadata": parse_json(&metadata),
            }),
        ))
    }

    /// Assets in registration order.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] on SQLite failure.
    pub fn assets(&self, cursor: Option<u64>, limit: usize) -> Result<Page, IndexError> {
        let connection = self.lock()?;
        let lower = cursor.map_or(Ok(0), signed)?;
        let fetch = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
        let mut statement = connection.prepare(
            "SELECT rowid, asset, chain, kind, address, denom, first_seen, metadata_json
             FROM assets WHERE rowid > ?1 ORDER BY rowid LIMIT ?2",
        )?;
        let mut rows = statement
            .query_map(params![lower, fetch], Self::asset_document)?
            .collect::<Result<Vec<_>, _>>()?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let next_cursor = if more {
            rows.last().map(|(id, _)| id.to_string())
        } else {
            None
        };
        Ok(Page {
            items: rows.into_iter().map(|(_, value)| value).collect(),
            next_cursor,
        })
    }

    /// One asset with its transfer count.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] on SQLite failure.
    pub fn asset(&self, asset: &str) -> Result<Option<Value>, IndexError> {
        let connection = self.lock()?;
        let found = connection
            .query_row(
                "SELECT rowid, asset, chain, kind, address, denom, first_seen, metadata_json
                 FROM assets WHERE asset = ?1",
                params![asset],
                Self::asset_document,
            )
            .optional()?;
        let Some((_, mut document)) = found else {
            return Ok(None);
        };
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM transfers WHERE asset = ?1",
            params![asset],
            |row| row.get(0),
        )?;
        if let Some(object) = document.as_object_mut() {
            object.insert("transfer_legs".to_owned(), Value::String(count.to_string()));
        }
        Ok(Some(document))
    }

    /// A cheap liveness probe of the database.
    ///
    /// # Errors
    /// Returns [`IndexError::Store`] when SQLite cannot answer.
    pub fn ping(&self) -> Result<(), IndexError> {
        let connection = self.lock()?;
        connection.query_row("SELECT 1", [], |_| Ok(()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(position: u64, hash: &str, parent: &str, rows: usize) -> Unit {
        Unit {
            chain: "test".to_owned(),
            position,
            hash: hash.to_owned(),
            parent: parent.to_owned(),
            link: hash.to_owned(),
            boundary: position,
            transfers: (0..rows)
                .map(|ordinal| TransferRow {
                    height_or_seq: position,
                    kind: if ordinal % 2 == 0 { "a" } else { "b" }.to_owned(),
                    direction: "in",
                    account: "alice".to_owned(),
                    counterparty: Some("bob".to_owned()),
                    asset: "coin".to_owned(),
                    amount: (ordinal + 1).to_string(),
                    tx_id: format!("{hash}-tx"),
                    ordinal: ordinal as u64,
                    decoded: json!({}),
                })
                .collect(),
            ..Unit::default()
        }
    }

    #[test]
    fn history_pages_newest_first_with_an_exclusive_cursor_and_kind_filter() {
        let store = Store::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        store
            .commit(&unit(1, "h1", "h0", 3), 10)
            .unwrap_or_else(|error| panic!("{error}"));
        store
            .commit(&unit(2, "h2", "h1", 2), 10)
            .unwrap_or_else(|error| panic!("{error}"));
        let first = store
            .history("alice", None, 2, None)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(first.items.len(), 2);
        assert_eq!(first.items[0]["height_or_seq"], "2");
        assert_eq!(first.items[0]["amount"], "2");
        let next = first
            .next_cursor
            .clone()
            .unwrap_or_else(|| panic!("no next cursor"));
        let cursor: u64 = next.parse().unwrap_or(0);
        let second = store
            .history("alice", Some(cursor), 2, None)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(second.items.len(), 2);
        assert_eq!(second.items[0]["height_or_seq"], "1");
        let last_cursor: u64 = second.next_cursor.unwrap_or_default().parse().unwrap_or(0);
        let third = store
            .history("alice", Some(last_cursor), 2, None)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(third.items.len(), 1);
        assert_eq!(third.next_cursor, None);
        let only_b = store
            .history("alice", None, 10, Some("b"))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(only_b.items.len(), 2);
        assert!(only_b.items.iter().all(|item| item["kind"] == "b"));
        assert!(store.account_known("test", "alice").unwrap_or(false));
    }

    #[test]
    fn rollback_removes_rows_above_the_fork_and_refuses_final_history() {
        let store = Store::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        for position in 1..=5 {
            store
                .commit(
                    &unit(
                        position,
                        &format!("h{position}"),
                        &format!("h{}", position - 1),
                        1,
                    ),
                    2,
                )
                .unwrap_or_else(|error| panic!("{error}"));
        }
        let cursor = store
            .cursor("test")
            .unwrap_or_default()
            .unwrap_or_else(|| panic!("no cursor"));
        assert_eq!(cursor.position, 5);
        assert_eq!(cursor.finalized_position, Some(3));
        let history = store
            .history("alice", None, 10, None)
            .unwrap_or_else(|error| panic!("{error}"));
        let finals: Vec<bool> = history
            .items
            .iter()
            .map(|item| item["final"].as_bool().unwrap_or(false))
            .collect();
        assert_eq!(finals, vec![false, false, true, true, true]);
        assert!(matches!(
            store.rollback("test", Some(2)),
            Err(IndexError::ReorgBeyondFinality { .. })
        ));
        store
            .rollback("test", Some(3))
            .unwrap_or_else(|error| panic!("{error}"));
        let after = store
            .history("alice", None, 10, None)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(after.items.len(), 3);
        let cursor = store
            .cursor("test")
            .unwrap_or_default()
            .unwrap_or_else(|| panic!("no cursor"));
        assert_eq!((cursor.position, cursor.hash.as_str()), (3, "h3"));
        assert!(store.link("test", 4).unwrap_or_default().is_none());
    }
}
