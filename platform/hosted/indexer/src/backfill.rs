//! Paxeer history backfill from the paxscan Blockscout database up to a
//! fixed cutover height, handed over to the live ingester at the cutover.
//!
//! Each height range is read from Blockscout, converted into the live
//! path's JSON-RPC shapes and decoded by the same [`PaxeerIngester`]
//! decoder, so every row is exactly what live ingestion writes. A height
//! Blockscout does not have is fetched with its receipts from the node.
//! Every Blockscout block is checked against the node's block at the same
//! height (hash and transaction list) before any unit of its range is
//! committed. Progress is a backfill cursor separate from the live cursor;
//! at the cutover the live cursor is written at exactly the cutover, so
//! live ingestion resumes at `cutover + 1` with no overlap and no gap.

use serde_json::Value;

use crate::blockscout::{rpc_blocks, BlockscoutRange};
use crate::codec::{hex0x, unhex_fixed};
use crate::paxeer::{PaxeerIngester, CHAIN};
use crate::store::{Store, Unit};
use crate::IndexError;

/// What one backfill step did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackfillOutcome {
    /// Heights `from..=to` were committed; `filled` of them came from the
    /// node because Blockscout lacks them.
    Advanced { from: u64, to: u64, filled: u64 },
    /// The backfill reached the cutover and the live cursor sits there.
    Finished { cutover: u64 },
}

/// A backfill run against one node up to one cutover height.
pub struct Backfill<'a> {
    node: &'a PaxeerIngester,
    cutover: u64,
    range_blocks: u64,
}

fn hash_of(value: &Value, what: &str) -> Result<String, IndexError> {
    let text = value
        .as_str()
        .ok_or_else(|| IndexError::Decode(format!("{what} is not text")))?;
    Ok(hex0x(&unhex_fixed::<32>(text)?))
}

fn node_transaction_hashes(header: &Value, height: u64) -> Result<Vec<String>, IndexError> {
    header
        .get("transactions")
        .and_then(Value::as_array)
        .ok_or_else(|| IndexError::Decode(format!("node block {height} has no transactions")))?
        .iter()
        .map(|entry| match entry {
            Value::String(_) => hash_of(entry, "transaction hash"),
            Value::Object(_) => hash_of(
                entry.get("hash").unwrap_or(&Value::Null),
                "transaction hash",
            ),
            _ => Err(IndexError::Decode(format!(
                "node block {height} lists a malformed transaction"
            ))),
        })
        .collect()
}

impl<'a> Backfill<'a> {
    /// Backfills through `node` up to and including `cutover`, reading at
    /// most `range_blocks` heights per step.
    #[must_use]
    pub const fn new(node: &'a PaxeerIngester, cutover: u64, range_blocks: u64) -> Self {
        Self {
            node,
            cutover,
            range_blocks,
        }
    }

    /// The next height to backfill, or `None` when the backfill has reached
    /// the cutover.
    ///
    /// # Errors
    /// Refuses a live cursor past the cutover and a backfill cursor past
    /// it, and returns store failures.
    pub fn next_height(&self, store: &Store) -> Result<Option<u64>, IndexError> {
        let live = store.cursor(CHAIN)?.map(|cursor| cursor.position);
        if let Some(live) = live {
            if live > self.cutover {
                return Err(IndexError::Config(format!(
                    "the live Paxeer cursor at {live} is already past the cutover {}",
                    self.cutover
                )));
            }
        }
        let backfilled = store.backfill_cursor(CHAIN)?.map(|cursor| cursor.position);
        if let Some(backfilled) = backfilled {
            if backfilled > self.cutover {
                return Err(IndexError::Config(format!(
                    "the Paxeer backfill cursor at {backfilled} is already past the cutover {}",
                    self.cutover
                )));
            }
        }
        let next = [live, backfilled]
            .into_iter()
            .flatten()
            .max()
            .map_or(self.node.start_block(), |position| position + 1)
            .max(self.node.start_block());
        Ok((next <= self.cutover).then_some(next))
    }

    /// The height range the next step covers, or `None` when done.
    ///
    /// # Errors
    /// As [`Backfill::next_height`].
    pub fn next_range(&self, store: &Store) -> Result<Option<(u64, u64)>, IndexError> {
        Ok(self.next_height(store)?.map(|from| {
            (
                from,
                self.cutover
                    .min(from.saturating_add(self.range_blocks.max(1) - 1)),
            )
        }))
    }

    /// Runs one step: reads the next range through `fetch`, verifies and
    /// commits it, or hands over to live ingestion once the cutover is
    /// reached.
    ///
    /// # Errors
    /// Returns source, decode, integrity and store failures; a Blockscout
    /// block whose hash differs from the node's stops the backfill with an
    /// [`IndexError::Integrity`] naming the height.
    pub fn step<F>(&self, store: &Store, fetch: F) -> Result<BackfillOutcome, IndexError>
    where
        F: FnOnce(u64, u64) -> Result<BlockscoutRange, IndexError>,
    {
        let Some((from, to)) = self.next_range(store)? else {
            store.finish_backfill(CHAIN, self.cutover, self.node.policy().finality_depth)?;
            return Ok(BackfillOutcome::Finished {
                cutover: self.cutover,
            });
        };
        let rows = fetch(from, to)?;
        let filled = self.apply_range(store, from, to, &rows)?;
        Ok(BackfillOutcome::Advanced { from, to, filled })
    }

    /// Steps until the cutover is reached and handed over.
    ///
    /// # Errors
    /// As [`Backfill::step`].
    pub fn run<F>(&self, store: &Store, mut fetch: F) -> Result<u64, IndexError>
    where
        F: FnMut(u64, u64) -> Result<BlockscoutRange, IndexError>,
    {
        self.node.check_chain_id()?;
        loop {
            match self.step(store, &mut fetch)? {
                BackfillOutcome::Advanced { from, to, filled } => {
                    eprintln!(
                        "layerx-indexer backfill committed {from}..={to} ({filled} from the node)"
                    );
                }
                BackfillOutcome::Finished { cutover } => return Ok(cutover),
            }
        }
    }

    /// Verifies and commits heights `from..=to` from `rows`, filling every
    /// height `rows` lacks from the node. Answers how many heights came
    /// from the node.
    ///
    /// # Errors
    /// As [`Backfill::step`]. Nothing of the range is committed unless
    /// every height in it decoded and verified.
    pub fn apply_range(
        &self,
        store: &Store,
        from: u64,
        to: u64,
        rows: &BlockscoutRange,
    ) -> Result<u64, IndexError> {
        if from > to || to > self.cutover {
            return Err(IndexError::Config(format!(
                "backfill range {from}..={to} is outside the cutover {}",
                self.cutover
            )));
        }
        let mut converted = rpc_blocks(rows)?;
        if let Some(outside) = converted
            .keys()
            .find(|height| !(from..=to).contains(*height))
        {
            return Err(IndexError::Integrity(format!(
                "paxscan answered block {outside} for the range {from}..={to}"
            )));
        }
        let cosmos = self.node.cosmos_range(from, to)?;
        let is_pointer = |address: &str| store.is_pointer(address);
        let mut units: Vec<Unit> = Vec::new();
        let mut filled = 0;
        for height in from..=to {
            let (block, receipts) = if let Some(paxscan) = converted.remove(&height) {
                let header = self.node.header(height)?.ok_or_else(|| {
                    IndexError::Source(format!("the node has no block {height} to verify"))
                })?;
                let node_hash = hash_of(header.get("hash").unwrap_or(&Value::Null), "hash")?;
                let paxscan_hash =
                    hash_of(paxscan.block.get("hash").unwrap_or(&Value::Null), "hash")?;
                if node_hash != paxscan_hash {
                    return Err(IndexError::Integrity(format!(
                        "paxscan block {height} hash {paxscan_hash} differs from the node's {node_hash}"
                    )));
                }
                if node_transaction_hashes(&header, height)? != paxscan.transaction_hashes {
                    return Err(IndexError::Integrity(format!(
                        "paxscan block {height} transactions differ from the node's"
                    )));
                }
                (paxscan.block, paxscan.receipts)
            } else {
                filled += 1;
                self.node.block_with_receipts(height)?.ok_or_else(|| {
                    IndexError::Source(format!("the node has no block {height} to fill"))
                })?
            };
            let unit = self.node.decode(
                &block,
                &receipts,
                cosmos.get(&height).map_or(&[][..], Vec::as_slice),
                &is_pointer,
            )?;
            if unit.position != height {
                return Err(IndexError::Integrity(format!(
                    "block {} delivered for height {height}",
                    unit.position
                )));
            }
            let previous = match units.last() {
                Some(previous) => Some(previous.link.clone()),
                None => height
                    .checked_sub(1)
                    .map(|position| store.link(CHAIN, position))
                    .transpose()?
                    .flatten()
                    .map(|link| link.link),
            };
            if let Some(previous) = previous {
                if previous != unit.parent {
                    return Err(IndexError::Integrity(format!(
                        "block {height} parent {} does not extend the stored block {}",
                        unit.parent,
                        height - 1
                    )));
                }
            }
            units.push(unit);
        }
        let depth = self.node.policy().finality_depth;
        for unit in &units {
            store.commit_backfill(unit, depth)?;
        }
        Ok(filled)
    }
}
