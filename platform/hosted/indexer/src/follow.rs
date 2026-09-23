//! Chain following shared by both ingesters: the step policy and the
//! reorganisation walk-back to the newest unit both sides still agree on.

use crate::store::Store;
use crate::IndexError;

/// How far behind the head units stay reversible, and how much one step
/// may ingest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FollowPolicy {
    /// Units at or below `position - finality_depth` are final; a reorg
    /// that would remove them stops the ingester instead.
    pub finality_depth: u64,
    /// The most units one step ingests before returning.
    pub max_units_per_step: u64,
}

/// What one ingester step did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepOutcome {
    /// The source has nothing beyond the cursor.
    Idle,
    /// Units were committed; the cursor is now at `position`.
    Advanced { units: u64, position: u64 },
    /// A reorganisation was detected and the store rolled back to `fork`
    /// (`None` meaning before the first indexed unit).
    RolledBack { fork: Option<u64> },
}

/// Walks back from `from` comparing each retained stored unit with the
/// source's canonical identity at the same position, and rolls the store
/// back to the newest match. `canonical(position)` answers `None` when the
/// source has no unit there.
///
/// # Errors
/// Returns [`IndexError::ReorgBeyondFinality`] when no retained unit matches
/// and the divergence reaches final history, and any source or store error.
pub fn walk_back<F>(
    store: &Store,
    chain: &str,
    from: u64,
    start: u64,
    mut canonical: F,
) -> Result<StepOutcome, IndexError>
where
    F: FnMut(u64) -> Result<Option<String>, IndexError>,
{
    let mut position = from;
    loop {
        let Some(stored) = store.link(chain, position)? else {
            break;
        };
        if canonical(position)?.as_deref() == Some(stored.hash.as_str()) {
            store.rollback(chain, Some(position))?;
            return Ok(StepOutcome::RolledBack {
                fork: Some(position),
            });
        }
        if position == start {
            store.rollback(chain, None)?;
            return Ok(StepOutcome::RolledBack { fork: None });
        }
        position -= 1;
    }
    Err(IndexError::ReorgBeyondFinality {
        source: chain.to_owned(),
        position,
    })
}
